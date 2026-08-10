use serde::Serialize;
use silentsilo_shell::{
    ShellAction, autostart_enabled, autostart_supported, drain_download_queue, drain_upload_queue,
    parse_args, queue_download, queue_upload, read_clipboard_file_paths, set_autostart,
};
use tauri::{AppHandle, Emitter, Manager};

/// Paths queued by the OS shell "Upload to SilentSilo" action since the last
/// time this was called. Draining (not just peeking) so a picked-up batch
/// isn't shown twice.
#[tauri::command]
pub fn shell_upload_queue_pending() -> Result<Vec<String>, String> {
    drain_upload_queue().map_err(|e| e.to_string())
}

/// The target directory queued by the OS shell "Download from SilentSilo"
/// action (right-click on empty space in a folder or on the Desktop), if
/// any, since the last time this was called.
#[tauri::command]
pub fn shell_download_queue_pending() -> Result<Option<String>, String> {
    drain_download_queue().map_err(|e| e.to_string())
}

/// File/folder paths currently on the OS clipboard (e.g. from Ctrl+C in
/// Explorer), for the file explorer's Ctrl+V "paste to upload". Empty if the
/// clipboard holds anything other than files (plain text, an image, ...).
#[tauri::command]
pub fn clipboard_file_paths() -> Vec<String> {
    read_clipboard_file_paths()
}

#[derive(Serialize)]
pub struct AutostartStatus {
    /// False where the app cannot register itself with the OS at all, which
    /// is everywhere except Windows for now. The Settings toggle greys out
    /// rather than disappearing, so the setting stays where people look.
    supported: bool,
    enabled: bool,
}

/// Read from the OS rather than from a setting the app keeps, so turning
/// autostart off in Task Manager's Startup tab is reflected here.
#[tauri::command]
pub fn autostart_status() -> AutostartStatus {
    AutostartStatus {
        supported: autostart_supported(),
        enabled: autostart_enabled(),
    }
}

#[tauri::command]
pub fn autostart_set(enabled: bool) -> Result<(), String> {
    set_autostart(enabled).map_err(|e| e.to_string())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn handle_shell_action(app: &AppHandle, args: Vec<String>) {
    match parse_args(args) {
        ShellAction::Upload { paths } => {
            for path in &paths {
                let _ = queue_upload(&path.to_string_lossy());
            }
            // Let the user pick a destination folder instead of silently
            // dropping everything into Inbox. If the vault is locked, the
            // frontend re-checks the queue right after unlock.
            show_main_window(app);
            let _ = app.emit("shell-upload-pending", paths.len());
        }
        ShellAction::Download { target_dir } => {
            if !target_dir.as_os_str().is_empty() {
                let _ = queue_download(&target_dir.to_string_lossy());
                // Let the user pick which vault folder to pull down instead
                // of guessing. If the vault is locked, the frontend
                // re-checks the queue right after unlock.
                show_main_window(app);
                let _ = app.emit("shell-download-pending", ());
            }
        }
        // No arguments means a person started the app (double-clicked the
        // icon, or the installer's "run now"), which is a request for the
        // window. The window is created hidden, so this is what puts it on
        // screen at launch, and what brings it back when a second launch is
        // forwarded here by the single-instance plugin.
        ShellAction::Show | ShellAction::None => {
            show_main_window(app);
        }
        // Started by the OS at sign-in. It goes to the notification area and
        // waits there: unprompted, an unlock request on top of the login
        // screen would train people to touch their key without reading.
        ShellAction::Autostart => {}
    }
}
