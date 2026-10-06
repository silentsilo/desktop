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
