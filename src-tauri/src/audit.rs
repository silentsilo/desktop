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

use silentsilo_app::audit_read::LogEntry;
use silentsilo_audit::Event;
pub use silentsilo_audit::codes;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::state::AppState;

const UNRECORDED: &str = "This silo's activity log could not be written on this computer, so the silo was locked. Your organisation requires the log. Check that the disk has space, then unlock again.";

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

/// Records `event` in silo `id`'s log. An `Err` means the silo was locked
/// and the action must not happen.
pub fn record_in(app: &AppHandle, id: Uuid, event: Event) -> Result<(), String> {
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

/// The focused silo's log, as this computer knows it.
#[tauri::command]
pub async fn audit_status(app: AppHandle) -> Result<silentsilo_app::AuditStatus, String> {
    crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let id = crate::state::focused_id(&state)?;
        state.audit_status(id)
    })
    .await
}

/// Turns the focused silo's own log on or off. Takes effect here at once;
/// the copies get it at the next sync, which the window then asks for.
#[tauri::command]
pub async fn audit_set_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<silentsilo_app::AuditStatus, String> {
    crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let id = crate::state::focused_id(&state)?;
        state.set_audit_log(id, enabled)?;
        state.audit_status(id)
    })
    .await
}

/// The focused silo's whole log, newest first, with what is missing from it.
#[tauri::command]
pub async fn audit_read(app: AppHandle) -> Result<silentsilo_app::audit_read::LogRead, String> {
    crate::commands::sync::read_audit_log(&app).await
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
    let log = crate::commands::sync::read_audit_log(&app).await?;
    let names = device_names(&app)?;
    let body = match format {
        ExportFormat::Csv => export_csv(&log.entries, &names),
        ExportFormat::Jsonl => export_jsonl(&log.entries, &names),
    };
    let count = log.entries.len();
    crate::commands::fido::run_blocking(move || {
        std::fs::write(&path, body).map_err(|e| e.to_string())
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

/// UTC, to the millisecond, the way spreadsheets and log tools read it.
fn utc(ms: i64) -> String {
    use chrono::{Datelike, Timelike};
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(at) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            at.year(),
            at.month(),
            at.day(),
            at.hour(),
            at.minute(),
            at.second(),
            at.timestamp_subsec_millis()
        ),
        None => String::new(),
    }
}

/// One CSV cell. A label is whatever someone typed, so one starting like a
/// formula is kept as text rather than run by the spreadsheet.
fn cell(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

fn export_csv(entries: &[LogEntry], names: &HashMap<Uuid, String>) -> String {
    let mut out =
        String::from("time_utc,device,device_name,event,code,count,object,label,details,number\n");
    for entry in entries {
        let e = &entry.event;
        let details = if e.x.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&e.x).unwrap_or_default()
        };
        let row = [
            utc(e.t),
            entry.device.to_string(),
            names.get(&entry.device).cloned().unwrap_or_default(),
            entry.what.clone(),
            e.c.to_string(),
            e.n.to_string(),
            e.o.clone().unwrap_or_default(),
            e.l.clone().unwrap_or_default(),
            details,
            e.i.to_string(),
        ];
        out.push_str(&row.iter().map(|v| cell(v)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

fn export_jsonl(entries: &[LogEntry], names: &HashMap<Uuid, String>) -> String {
    let mut out = String::new();
    for entry in entries {
        let line = serde_json::json!({
            "time_utc": utc(entry.event.t),
            "device": entry.device,
            "device_name": names.get(&entry.device),
            "event": entry.what,
            "record": entry.event,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
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
    fn an_export_keeps_a_typed_formula_as_text() {
        assert_eq!(cell("Bank"), "Bank");
        assert_eq!(cell("=HYPERLINK(1)"), "'=HYPERLINK(1)");
        assert_eq!(cell("a, \"b\""), "\"a, \"\"b\"\"\"");
        assert_eq!(utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(utc(1_789_000_000_123), "2026-09-10T00:26:40.123Z");
    }

    #[test]
    fn an_export_has_a_row_per_event_with_its_device() {
        let device = Uuid::new_v4();
        let entry = LogEntry {
            device,
            what: "Secret copied".into(),
            event: event(codes::SECRET_COPIED)
                .on("e1", "Bank")
                .with("field", "password"),
        };
        let names = HashMap::from([(device, "Laptop".to_string())]);
        let csv = export_csv(std::slice::from_ref(&entry), &names);
        let row = csv.lines().nth(1).unwrap();
        assert!(row.contains(",Laptop,Secret copied,11,1,e1,Bank,"));
        assert!(row.contains("\"{\"\"field\"\":\"\"password\"\"}\""));
        let line = export_jsonl(&[entry], &names);
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["device_name"], "Laptop");
        assert_eq!(parsed["record"]["l"], "Bank");
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
}
