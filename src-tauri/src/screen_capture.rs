//! Keeping the window out of screenshots, screen recordings and screen
//! sharing, Windows Recall included. On by default: the window shows open
//! passwords and file names. Settings > General turns it off for someone who
//! needs to show SilentSilo in a call.
//!
//! Tauri's content protection does the work: `SetWindowDisplayAffinity` with
//! `WDA_EXCLUDEFROMCAPTURE` on Windows, the window's sharing type set to none
//! on a Mac. Linux has nothing like it, and Settings says so.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "screen-capture.json";

/// The person's choice, kept on this computer.
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
#[serde(default)]
struct Settings {
    /// Let screenshots and screen sharing see the window.
    allow: bool,
}

#[derive(Serialize)]
pub struct Status {
    /// Whether this system can keep a window out of captures.
    supported: bool,
    /// Kept out of captures now.
    protected: bool,
}

const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::state::app_data_dir(app)
        .ok()
        .map(|d| d.join(SETTINGS_FILE))
}

fn load(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn apply(app: &AppHandle, settings: Settings) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_content_protected(!settings.allow);
    }
}

fn status(app: &AppHandle) -> Status {
    Status {
        supported: SUPPORTED,
        protected: SUPPORTED && !load(app).allow,
    }
}

/// At start, before the window is shown. The window is created protected
/// (`contentProtected` in tauri.conf.json); this lifts it for a person who
/// chose so.
pub fn start(app: &AppHandle) {
    apply(app, load(app));
}

#[tauri::command(async)]
pub fn screen_capture_status(app: AppHandle) -> Status {
    status(&app)
}

#[tauri::command(async)]
pub fn screen_capture_set(app: AppHandle, protected: bool) -> Result<Status, String> {
    let settings = Settings { allow: !protected };
    let path = settings_path(&app).ok_or("No app data folder.")?;
    let raw = serde_json::to_vec(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())?;
    apply(&app, settings);
    Ok(status(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_unless_someone_said_otherwise() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.allow);
        let s: Settings = serde_json::from_str(r#"{"allow":true}"#).unwrap();
        assert!(s.allow);
    }
}

/// Where storage passwords can be kept on this computer.
#[derive(serde::Serialize)]
pub struct SecretsStatus {
    /// A keyring or the system's own protection is there.
    protected: bool,
    /// Without one, writing them to a file only this account reads is allowed.
    allowed: bool,
}

/// "No keyring" only once the keyring has refused a secret in this run: on
/// Linux and macOS there is no protector, and saying so to everyone pushed
/// people with a working keyring towards a plain file. A switch already on
/// stays visible, so it can be turned off.
fn secrets_status() -> SecretsStatus {
    let allowed = silentsilo_vault::unprotected_secrets_allowed();
    let keyring_refused = silentsilo_vault::keyring_keeps_secrets() == Some(false);
    SecretsStatus {
        protected: !allowed && (silentsilo_vault::local_secrets_protected() || !keyring_refused),
        allowed,
    }
}

/// For Settings, General: shown only where there is no keyring.
#[tauri::command(async)]
pub fn secrets_storage_status() -> SecretsStatus {
    secrets_status()
}

/// The person's answer to keeping storage passwords in a file only their
/// account can read, on a computer with no keyring.
#[tauri::command(async)]
pub fn secrets_allow_unprotected(allow: bool) -> Result<SecretsStatus, String> {
    silentsilo_vault::allow_unprotected_secrets(allow).map_err(|e| e.to_string())?;
    Ok(secrets_status())
}
