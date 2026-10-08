//! The silo's activity log, as this app's commands write it.
//!
//! What leaves the silo (a secret shown or copied, a file opened or saved
//! outside it, a login filled) is recorded before it happens; a change inside
//! the silo once it is stored. On an organisation's silo an event that cannot
//! be written locks that silo and refuses the action: the device may not go
//! on unrecorded. On a personal one it is a diagnostic and the action goes
//! ahead.
//!
//! Never call these while holding the sessions lock: recording takes it.

use std::collections::HashMap;
use std::sync::Mutex;

use silentsilo_audit::Event;
pub use silentsilo_audit::codes;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::state::AppState;

const UNRECORDED: &str = crate::err::coded!(
    "err.audit_unrecorded",
    "This silo's activity log could not be written on this computer, so the silo was locked. Your organisation requires the log. Check that the disk has space, then unlock again."
);

/// An event of `code`, happening now.
pub fn event(code: u16) -> Event {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Event::new(code, now_ms)
}

/// Records `event` in the focused silo's log.
pub fn record(app: &AppHandle, event: Event) -> Result<(), String> {
    let id = crate::state::focused_id(&app.state::<AppState>())?;
    record_in(app, id, event)
}

/// [`record_in`] on the blocking pool, for an async command: writing waits
/// for the disk, and for the sync pass if it holds the log.
pub async fn record_off_thread(app: &AppHandle, id: Uuid, event: Event) -> Result<(), String> {
    let app = app.clone();
    crate::commands::fido::run_blocking(move || record_in(&app, id, event)).await
}

/// The key each open silo was unlocked with, as the log names it. Every
/// event the silo records while open carries it as `via`, so a row says
/// which key did what, not only on which device.
static UNLOCKED_WITH: Mutex<Option<HashMap<Uuid, String>>> = Mutex::new(None);

/// Set when a session opens: `None` for one opened without a key the log
/// can name (a new silo, a join), so an earlier session's key never sticks.
pub fn set_unlocked_with(id: Uuid, via: Option<String>) {
    let Ok(mut map) = UNLOCKED_WITH.lock() else {
        return;
    };
    let map = map.get_or_insert_with(HashMap::new);
    match via.filter(|v| !v.is_empty()) {
        Some(via) => map.insert(id, via),
        None => map.remove(&id),
    };
}

fn unlocked_with(id: Uuid) -> Option<String> {
    UNLOCKED_WITH.lock().ok()?.as_ref()?.get(&id).cloned()
}

/// What the built-in authenticator is called here.
const BUILT_IN: &str = if cfg!(target_os = "macos") {
    "Touch ID"
} else if cfg!(windows) {
    "Windows Hello"
} else {
    "Built-in key"
};

/// A key as the log names it: its label, or what it is when it has none.
pub fn key_name(key: &silentsilo_vault::StoredFidoCredential) -> String {
    if !key.label.is_empty() {
        key.label.clone()
    } else if key.platform {
        BUILT_IN.into()
    } else {
        format!("Security key {}", key.key_slot)
    }
}

/// What the log calls an unlock with the recovery code.
pub const VIA_RECOVERY_CODE: &str = "recovery code";

/// Records `event` in silo `id`'s log. An `Err` means the silo was locked
/// and the action must not happen.
pub fn record_in(app: &AppHandle, id: Uuid, mut event: Event) -> Result<(), String> {
    // The unlock itself already names its key.
    if event.c != codes::UNLOCKED
        && !event.x.contains_key("via")
        && let Some(via) = unlocked_with(id)
    {
        event = event.with("via", via);
    }
    let state = app.state::<AppState>();
    let Err(e) = state.audit_record(id, event) else {
        return Ok(());
    };
    if !state.audit_is_mandatory(id) {
        crate::diagnostics::warn("audit", e);
        return Ok(());
    }
    crate::diagnostics::warn("audit", format_args!("locking the silo: {e}"));
    crate::commands::vault::take_back_clipboard(app, Some(&[id]));
    let _ = state.close_session(id);
    crate::commands::cloud::forget_sign_ins_when_all_locked(app);
    let _ = app.emit("silo-audit-locked", id.to_string());
    Err(UNRECORDED.into())
}

/// The events the window may report itself: what happens on screen, which
/// no command sees. Everything else is recorded where it happens.
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Note {
    EntryRevealed,
    PasswordsImported,
}

/// Records what the window did with an entry it already holds, before it
/// shows it. `count` and `format` go with an import.
#[tauri::command]
pub async fn audit_note(
    app: AppHandle,
    note: Note,
    entry_id: Option<String>,
    label: Option<String>,
    count: Option<u32>,
    format: Option<String>,
) -> Result<(), String> {
    let mut event = event(match note {
        Note::EntryRevealed => codes::ENTRY_REVEALED,
        Note::PasswordsImported => codes::PASSWORDS_IMPORTED,
    });
    if let Some(id) = entry_id {
        event = event.on(id, label.unwrap_or_default());
    }
    if let Some(count) = count {
        event = event.with("count", count);
    }
    if let Some(format) = format {
        event = event.with("format", format);
    }
    crate::commands::fido::run_blocking(move || record(&app, event)).await
}

/// The log as the window shows it: core's status, and whether the silo is
/// an organisation's, which decides who may start and read its log.
#[derive(serde::Serialize)]
pub struct SiloAuditStatus {
    #[serde(flatten)]
    status: silentsilo_app::AuditStatus,
    org_controlled: bool,
}

fn status_of(app: &AppHandle) -> Result<SiloAuditStatus, String> {
    let state = app.state::<AppState>();
    let id = crate::state::focused_id(&state)?;
    let root = crate::state::vault_dir(app)?;
    let org_controlled = silentsilo_vault::load_fido_keys(&root)
        .map(|keys| keys.is_org_controlled())
        .unwrap_or(false);
    Ok(SiloAuditStatus {
        status: state.audit_status(id)?,
        org_controlled,
    })
}

/// The focused silo's log, as this computer knows it.
#[tauri::command]
pub async fn audit_status(app: AppHandle) -> Result<SiloAuditStatus, String> {
    crate::commands::fido::run_blocking(move || status_of(&app)).await
}

/// Turns the focused silo's own log on or off. Takes effect here at once;
/// the copies get it at the next sync, which the window then asks for.
#[tauri::command]
pub async fn audit_set_enabled(app: AppHandle, enabled: bool) -> Result<SiloAuditStatus, String> {
    crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let id = crate::state::focused_id(&state)?;
        state.set_audit_log(id, enabled)?;
        status_of(&app)
    })
    .await
}

/// A read of the log, held for the Activity page to page through.
pub struct HeldRead {
    silo: Uuid,
    epoch: u64,
    read: std::sync::Arc<silentsilo_app::audit_read::LogRead>,
    names: HashMap<Uuid, String>,
}

/// Most entries one page carries.
const MAX_PAGE: usize = 500;

/// One page of the log, newest first, with what is missing from the whole.
#[derive(serde::Serialize)]
pub struct AuditPage {
    entries: Vec<silentsilo_app::audit_read::LogEntry>,
    /// Entries the search matches, all of them, or the whole log's count
    /// when there is no search.
    matched: usize,
    total: usize,
    devices: Vec<silentsilo_app::audit_read::DeviceTrail>,
    unreadable: usize,
    copies_unread: Vec<String>,
}

/// Whether an entry matches a lowercased search: its name, its label, the
/// device's name, and what it carries.
fn matches(
    entry: &silentsilo_app::audit_read::LogEntry,
    names: &HashMap<Uuid, String>,
    term: &str,
) -> bool {
    let has = |text: &str| text.to_lowercase().contains(term);
    has(&entry.what)
        || entry.event.l.as_deref().is_some_and(has)
        || entry.event.o.as_deref().is_some_and(has)
        || names.get(&entry.device).is_some_and(|n| has(n))
        || entry.event.x.iter().any(|(key, value)| {
            has(key)
                || match value {
                    serde_json::Value::String(s) => has(s),
                    other => has(&other.to_string()),
                }
        })
}

fn page_of(
    held: &HeldRead,
    offset: usize,
    limit: usize,
    search: &str,
    kinds: Option<&[u16]>,
) -> AuditPage {
    let term = search.trim().to_lowercase();
    let limit = limit.clamp(1, MAX_PAGE);
    let read = &held.read;
    let (entries, matched) = if term.is_empty() && kinds.is_none() {
        let entries = read
            .entries
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        (entries, read.entries.len())
    } else {
        let hits: Vec<&silentsilo_app::audit_read::LogEntry> = read
            .entries
            .iter()
            .filter(|e| kinds.is_none_or(|k| k.contains(&e.event.c)))
            .filter(|e| term.is_empty() || matches(e, &held.names, &term))
            .collect();
        let entries = hits
            .iter()
            .skip(offset)
            .take(limit)
            .map(|e| (*e).clone())
            .collect();
        (entries, hits.len())
    };
    AuditPage {
        entries,
        matched,
        total: read.entries.len(),
        devices: read.devices.clone(),
        unreadable: read.unreadable,
        copies_unread: read.copies_unread.clone(),
    }
}

/// A page of the focused silo's log. `refresh` reads it again (an
/// organisation's log asks for one of its keys first): what this computer
/// holds at once, then every copy in the background, announced with
/// `audit-copies-read` so the window asks for the page again. Otherwise the
/// page comes from the last read, which must be of this silo and this
/// unlock. Only what is shown crosses to the window.
#[tauri::command]
pub async fn audit_read(
    app: AppHandle,
    refresh: bool,
    offset: usize,
    limit: usize,
    search: String,
    kinds: Option<Vec<u16>>,
) -> Result<AuditPage, String> {
    let state = app.state::<AppState>();
    let silo = crate::state::focused_id(&state)?;
    if !refresh {
        let held = crate::state::lock_recovering(&state.audit_page);
        return match held.as_ref() {
            Some(held) if held.silo == silo && held.epoch == state.epoch() => {
                Ok(page_of(held, offset, limit, &search, kinds.as_deref()))
            }
            _ => Err(READ_AGAIN.into()),
        };
    }
    let epoch = state.epoch();
    // A newer read supersedes one still out on the copies.
    let closes = state
        .audit_closes
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    let reader = reader_for(
        &app,
        crate::commands::fido::Prompt::new(
            "org_read_log",
            "Touch the organisation's security key to read the activity log.",
        ),
    )
    .await?;
    let read = crate::commands::sync::read_audit_log_local(&app, &reader)?;
    let held = HeldRead {
        silo,
        epoch,
        read: std::sync::Arc::new(read),
        names: device_names(&app).unwrap_or_default(),
    };
    let page = page_of(&held, offset, limit, &search, kinds.as_deref());
    keep_read(&app, held, closes);

    // The copies after: a slow or unreachable one no longer holds the page.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = crate::commands::sync::read_audit_log(&app, &reader).await;
        let current = match result {
            Ok(read) => {
                let held = HeldRead {
                    silo,
                    epoch,
                    read: std::sync::Arc::new(read),
                    names: device_names(&app).unwrap_or_default(),
                };
                keep_read(&app, held, closes)
            }
            Err(_) => is_current(&app, epoch, closes),
        };
        if current {
            let _ = app.emit("audit-copies-read", ());
        }
    });
    Ok(page)
}

/// Whether nothing closed or switched the silo, and the page did not close
/// or read again, since `epoch` and `closes` were taken.
fn is_current(app: &AppHandle, epoch: u64, closes: u64) -> bool {
    let state = app.state::<AppState>();
    state.epoch() == epoch && state.audit_closes.load(std::sync::atomic::Ordering::SeqCst) == closes
}

/// Holds a read for the next pages, if it is still current. Checked under
/// the lock a close and a switch clear it under, after moving their
/// counter, so neither slips between.
fn keep_read(app: &AppHandle, held: HeldRead, closes: u64) -> bool {
    let state = app.state::<AppState>();
    let mut slot = crate::state::lock_recovering(&state.audit_page);
    let current = is_current(app, held.epoch, closes);
    if current {
        *slot = Some(held);
    }
    current
}

const READ_AGAIN: &str = crate::err::coded!(
    "err.audit_read_again",
    "The activity log needs reading again."
);

/// The Activity page closed: what it read goes.
#[tauri::command(async)]
pub fn audit_read_close(app: AppHandle) {
    let state = app.state::<AppState>();
    state
        .audit_closes
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *crate::state::lock_recovering(&state.audit_page) = None;
}

/// Who reads: the silo's own key, or an organisation key touched now.
async fn reader_for(
    app: &AppHandle,
    prompt: crate::commands::fido::Prompt,
) -> Result<silentsilo_app::audit_read::Reader, String> {
    use silentsilo_app::audit_read::Reader;
    let state = app.state::<AppState>();
    let id = crate::state::focused_id(&state)?;
    if !state.audit_status(id)?.organisation {
        return Ok(Reader::Silo);
    }
    let touch = touch_org_key(app, prompt).await?;
    Ok(Reader::Organisation {
        credential_id: touch.credential_id,
        wrap_key: touch.wrap_key,
    })
}

async fn touch_org_key(
    app: &AppHandle,
    prompt: crate::commands::fido::Prompt,
) -> Result<silentsilo_app::audit_admin::OrgKeyTouch, String> {
    let root = crate::state::vault_dir(app)?;
    let keys = silentsilo_vault::load_fido_keys(&root).map_err(|e| e.to_string())?;
    crate::commands::fido::touch_organisation_key(app, &keys, "activity log", prompt)
        .await
        .map(|(_, touch)| touch)
}

/// Starts the organisation's log on the focused silo, read with the key
/// touched now. The copies get it at the next sync.
#[tauri::command]
pub async fn audit_org_start(
    app: AppHandle,
    retention_days: Option<u32>,
) -> Result<SiloAuditStatus, String> {
    let touch = touch_org_key(
        &app,
        crate::commands::fido::Prompt::new(
            "org_start_log",
            "Touch the organisation's security key to start its activity log.",
        ),
    )
    .await?;
    crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let id = crate::state::focused_id(&state)?;
        state.start_org_audit_log(id, &touch, retention_days)?;
        status_of(&app)
    })
    .await
}

/// Changes how long the organisation's log keeps its records.
#[tauri::command]
pub async fn audit_org_retention(
    app: AppHandle,
    retention_days: Option<u32>,
) -> Result<SiloAuditStatus, String> {
    touch_org_key(
        &app,
        crate::commands::fido::Prompt::new(
            "org_retention",
            "Touch the organisation's security key to change how long the log is kept.",
        ),
    )
    .await?;
    crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let id = crate::state::focused_id(&state)?;
        state.set_org_audit_retention(id, retention_days)?;
        status_of(&app)
    })
    .await
}

/// Removes the records past the organisation's retention from every copy
/// that takes deletes. Returns how many segments went.
#[tauri::command]
pub async fn audit_org_expire(app: AppHandle) -> Result<usize, String> {
    touch_org_key(
        &app,
        crate::commands::fido::Prompt::new(
            "org_prune",
            "Touch the organisation's security key to remove records past the retention.",
        ),
    )
    .await?;
    crate::commands::sync::expire_audit_segments(&app).await
}

#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Jsonl,
}

/// Writes the focused silo's log to `path`, one event per row or line.
/// Returns how many events it wrote.
#[tauri::command]
pub async fn audit_export(
    app: AppHandle,
    path: String,
    format: ExportFormat,
) -> Result<usize, String> {
    let reader = reader_for(
        &app,
        crate::commands::fido::Prompt::new(
            "org_export_log",
            "Touch the organisation's security key to export the activity log.",
        ),
    )
    .await?;
    let log = crate::commands::sync::read_audit_log(&app, &reader).await?;
    let names = device_names(&app)?;
    let body = match format {
        ExportFormat::Csv => silentsilo_audit::reading::to_csv(&log.entries, &names),
        ExportFormat::Jsonl => silentsilo_audit::reading::to_jsonl(&log.entries, &names),
    };
    let count = log.entries.len();
    crate::commands::fido::run_blocking(move || {
        crate::commands::vault::write_owner_only(std::path::Path::new(&path), body.as_bytes())
    })
    .await?;
    Ok(count)
}

/// What each device is called in this silo, for an export read elsewhere.
fn device_names(app: &AppHandle) -> Result<HashMap<Uuid, String>, String> {
    let state = app.state::<AppState>();
    let devices = crate::state::with_vfs(&state, |_session, vfs| vfs.list_devices())?;
    Ok(devices
        .into_iter()
        .map(|d| {
            let name = d.label.or(d.system_name).unwrap_or_default();
            (d.id, name)
        })
        .collect())
}

/// What a copied secret was, for the log. Sent with the copy, so the event
/// is written before the clipboard holds it.
#[derive(serde::Deserialize)]
pub struct CopiedSecret {
    pub entry_id: String,
    pub label: String,
    /// The field: "password", "card number", "one-time code".
    pub field: String,
}

impl CopiedSecret {
    pub fn event(self) -> Event {
        let code = if self.field == "one-time code" {
            codes::CODE_COPIED
        } else {
            codes::SECRET_COPIED
        };
        event(code)
            .on(self.entry_id, self.label)
            .with("field", self.field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copied(field: &str) -> CopiedSecret {
        CopiedSecret {
            entry_id: "e1".into(),
            label: "Bank".into(),
            field: field.into(),
        }
    }

    #[test]
    fn a_session_names_its_key_until_another_opens() {
        let id = Uuid::new_v4();
        set_unlocked_with(id, Some("YubiKey".into()));
        assert_eq!(unlocked_with(id).as_deref(), Some("YubiKey"));
        set_unlocked_with(id, Some(String::new()));
        assert_eq!(unlocked_with(id), None, "an empty name is no name");
        set_unlocked_with(id, Some("YubiKey".into()));
        set_unlocked_with(id, None);
        assert_eq!(unlocked_with(id), None);
    }

    #[test]
    fn a_copy_is_logged_as_what_it_was() {
        let code = copied("one-time code").event();
        assert_eq!(code.c, codes::CODE_COPIED);
        let password = copied("password").event();
        assert_eq!(password.c, codes::SECRET_COPIED);
        assert_eq!(password.o.as_deref(), Some("e1"));
        assert_eq!(password.l.as_deref(), Some("Bank"));
        assert_eq!(password.x["field"], "password");
    }

    #[test]
    fn the_window_may_note_only_what_it_alone_sees() {
        assert!(serde_json::from_str::<Note>(r#""entry_revealed""#).is_ok());
        assert!(serde_json::from_str::<Note>(r#""passwords_imported""#).is_ok());
        for other in ["secret_copied", "unlocked", "key_removed", "log_stopped"] {
            assert!(
                serde_json::from_str::<Note>(&format!("\"{other}\"")).is_err(),
                "{other}"
            );
        }
    }

    fn log_of(n: u64) -> HeldRead {
        use silentsilo_app::audit_read::{LogEntry, LogRead};
        let device = Uuid::new_v4();
        let entries = (0..n)
            .rev()
            .map(|i| {
                let mut event =
                    silentsilo_audit::Event::new(silentsilo_audit::codes::SECRET_COPIED, i as i64)
                        .on("e1", if i % 3 == 0 { "Mail" } else { "Bank" })
                        .with("field", "password");
                event.i = i;
                LogEntry {
                    device,
                    what: "Secret copied".into(),
                    event,
                }
            })
            .collect();
        HeldRead {
            silo: Uuid::new_v4(),
            epoch: 1,
            read: std::sync::Arc::new(LogRead {
                entries,
                ..LogRead::default()
            }),
            names: HashMap::from([(device, "Laptop".to_string())]),
        }
    }

    #[test]
    fn the_log_is_paged_and_searched_here() {
        let held = log_of(250);
        let first = page_of(&held, 0, 100, "", None);
        assert_eq!(
            (first.entries.len(), first.matched, first.total),
            (100, 250, 250)
        );
        assert_eq!(first.entries[0].event.i, 249, "newest first");
        let last = page_of(&held, 200, 100, "", None);
        assert_eq!(last.entries.len(), 50);

        let mail = page_of(&held, 0, 30, " MAIL ", None);
        assert_eq!(mail.matched, 84);
        assert_eq!(mail.entries.len(), 30);
        assert!(
            mail.entries
                .iter()
                .all(|e| e.event.l.as_deref() == Some("Mail"))
        );
        assert_eq!(
            page_of(&held, 0, 10, "laptop", None).matched,
            250,
            "by device name"
        );
        assert_eq!(
            page_of(&held, 0, 10, "password", None).matched,
            250,
            "by detail"
        );
        assert_eq!(page_of(&held, 0, 10, "nothing", None).matched, 0);
        let copies = page_of(
            &held,
            0,
            10,
            "",
            Some(&[silentsilo_audit::codes::SECRET_COPIED]),
        );
        assert_eq!(copies.matched, 250, "every one is a copy");
        assert_eq!(page_of(&held, 0, 10, "", Some(&[1, 2])).matched, 0);
        assert_eq!(page_of(&held, 0, 10, "mail", Some(&[11])).matched, 84);
        assert_eq!(
            page_of(&log_of(600), 0, 100_000, "", None).entries.len(),
            MAX_PAGE
        );
    }
}
