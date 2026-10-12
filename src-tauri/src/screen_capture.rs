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
