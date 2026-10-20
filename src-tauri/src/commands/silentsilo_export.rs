//! The SilentSilo password export: every kind of entry, its history, its
//! settings and its attached files, under a password, for another silo or
//! another person who uses SilentSilo. The format is core's
//! (`silentsilo_vault::passwords_export`); this side gathers the entries and
//! files on the way out and files them into the open silo on the way in.

use serde_json::Value as Json;
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::commands::fido::run_blocking;
use crate::commands::kdbx::{text, whole_where_withheld, wipe};
use crate::commands::vault::{
    decrypt_attachment_bytes, encrypt_attachment_bytes, ensure_blobs_local,
};
use crate::state::AppState;
use silentsilo_vault::passwords_export;

/// Below this a file is too easy to guess offline, as for a KeePass one.
const MIN_PASSWORD_CHARS: usize = 8;

const PASSWORD_SHORT: &str = crate::err::coded!(
    "err.export_password_short",
    "Use a password of at least 8 characters for the export."
);
const NOT_AN_EXPORT: &str = crate::err::coded!(
    "err.not_a_silentsilo_export",
    "This is not a SilentSilo password export."
);
const WRONG_PASSWORD: &str = crate::err::coded!(
    "err.export_wrong_password",
    "Wrong password, or the file was changed since it was made."
);
const NEWER_EXPORT: &str = crate::err::coded!(
    "err.export_newer",
    "This export was made by a newer SilentSilo. Update SilentSilo to open it."
);
const TOO_LARGE: &str = crate::err::coded!(
    "err.export_too_large",
    "These entries and their files come to more than 1 GB."
);

/// The attachments of `entries`, by blob id.
fn blob_ids(entries: &[Json]) -> Vec<Uuid> {
    entries
        .iter()
        .flat_map(|e| {
            e.get("attachments")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|a| {
            a.get("blob_id")
                .and_then(Json::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .collect()
}

/// Writes `entries` (the silo's, as the window holds them) and the category
/// list to `path` under `password`, readable by this user alone. Asks for
/// the key like every export.
#[tauri::command]
pub async fn passwords_write_silentsilo(
    app: AppHandle,
    path: String,
    password: String,
    entries: String,
    categories: Option<String>,
) -> Result<(), String> {
    let password = Zeroizing::new(password);
    let text_in = Zeroizing::new(entries);
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(PASSWORD_SHORT.into());
    }
    crate::state::unlocked_silo(&app)?;
    crate::commands::vault::take_export_confirmation(&app).await?;
    let mut entries: Vec<Json> = serde_json::from_str(&text_in).map_err(|e| e.to_string())?;
    drop(text_in);
    whole_where_withheld(&app, &mut entries).await?;
    ensure_blobs_local(&app, &blob_ids(&entries)).await?;

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        crate::audit::record_in(
            &app,
            snapshot.id,
            crate::audit::event(crate::audit::codes::PASSWORDS_EXPORTED)
                .with("format", "silentsilo")
                .with("count", entries.len()),
        )?;
        let mut files = Vec::new();
        for attachment in entries.iter().flat_map(|e| {
            e.get("attachments")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
        }) {
            let id = Uuid::parse_str(text(&attachment, "blob_id")).map_err(|e| e.to_string())?;
            if files.iter().any(|(seen, _)| *seen == id) {
                continue;
            }
            let bytes = decrypt_attachment_bytes(
                &snapshot,
                text(&attachment, "blob_id"),
                text(&attachment, "blob_key"),
            )?;
            files.push((id, bytes));
        }
        // Each entry as the silo stores it, without what the window adds.
        let rows: Vec<String> = entries
            .iter()
            .map(|e| {
                let mut e = e.clone();
                if let Some(fields) = e.as_object_mut() {
                    fields.remove("withheld");
                }
                let row = e.to_string();
                wipe(&mut e);
                row
            })
            .collect();
        entries.iter_mut().for_each(wipe);
        let mut rows = rows;
        let file = passwords_export::write(&password, &rows, categories.as_deref(), &files)
            .map_err(|e| {
                if e.to_string().contains("more than 1 GB") {
                    TOO_LARGE.to_string()
                } else {
                    e.to_string()
                }
            });
        rows.iter_mut().for_each(zeroize::Zeroize::zeroize);
        crate::commands::vault::write_owner_only(std::path::Path::new(&path), &file?)
    })
    .await
}

/// What an export brought: its entries with their attached files already
/// encrypted into the open silo, and its category list.
#[derive(serde::Serialize)]
pub struct SilentsiloImport {
    entries: Vec<Json>,
    categories: Option<Json>,
    /// The blobs written for this import, so a cancelled import can drop them.
    blobs: Vec<String>,
}

/// Opens the export at `path` with `password`. Nothing is stored in the
/// silo here except the attached files, encrypted; the window files the
/// entries after asking where.
#[tauri::command]
pub async fn passwords_read_silentsilo(
    app: AppHandle,
    path: String,
    password: String,
) -> Result<SilentsiloImport, String> {
    let password = Zeroizing::new(password);
    run_blocking(move || {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > (passwords_export::MAX_EXPORT_BYTES as u64) + 64 * 1024 {
            return Err(TOO_LARGE.into());
        }
        let bytes = Zeroizing::new(std::fs::read(&path).map_err(|e| e.to_string())?);
        let opened = passwords_export::read(&password, &bytes).map_err(|e| {
            let said = e.to_string();
            if said.contains("not a SilentSilo password export") {
                NOT_AN_EXPORT.to_string()
            } else if said.contains("newer SilentSilo") {
                NEWER_EXPORT.to_string()
            } else if said.contains("Wrong password") {
                WRONG_PASSWORD.to_string()
            } else {
                said
            }
        })?;
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;

        let mut written: Vec<String> = Vec::new();
        let result = file_entries(&opened, &snapshot, &mut written);
        match result {
            Ok(entries) => Ok(SilentsiloImport {
                entries,
                categories: opened
                    .categories
                    .as_deref()
                    .and_then(|c| serde_json::from_str(c).ok()),
                blobs: written,
            }),
            Err(e) => {
                // What was encrypted before the failure is referred to by
                // nothing: it goes again.
                for blob in written {
                    if let Ok(id) = Uuid::parse_str(&blob) {
                        let _ = silentsilo_vault::remove_blob_from_cache(&snapshot.root, id);
                    }
                }
                Err(e)
            }
        }
    })
    .await
}

/// The export's entries, each attachment encrypted into the silo under a
/// new blob and key; one whose bytes the file lacks is left out.
fn file_entries(
    opened: &passwords_export::PasswordsExport,
    snapshot: &crate::state::SessionSnapshot,
    written: &mut Vec<String>,
) -> Result<Vec<Json>, String> {
    let mut out = Vec::new();
    for row in &opened.entries {
        let mut entry: Json = serde_json::from_str(row).map_err(|e| e.to_string())?;
        if let Some(attachments) = entry.get_mut("attachments").and_then(Json::as_array_mut) {
            let mut kept = Vec::new();
            for attachment in attachments.iter() {
                let Some(bytes) = Uuid::parse_str(text(attachment, "blob_id"))
                    .ok()
                    .and_then(|id| opened.attachments.get(&id))
                else {
                    continue;
                };
                let encrypted =
                    encrypt_attachment_bytes(snapshot, text(attachment, "name"), bytes)?;
                written.push(encrypted.blob_id().to_string());
                kept.push(serde_json::to_value(&encrypted).map_err(|e| e.to_string())?);
            }
            *attachments = kept;
        }
        out.push(entry);
    }
    Ok(out)
}
