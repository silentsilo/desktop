//! OS shell integration — CLI args and single-instance upload queue.

mod autostart;
mod cli;
mod clipboard;
pub mod disk_space;
mod hardening;
mod identity;
#[cfg(target_os = "macos")]
mod macos;
mod os_integration;
mod secret_clipboard;
mod session_watch;
#[cfg(windows)]
mod windows;

pub use autostart::{autostart_enabled, autostart_supported, ensure_autostart, set_autostart};
pub use cli::{ShellAction, parse_args};
pub use clipboard::read_file_paths as read_clipboard_file_paths;
pub use hardening::harden_process;
pub use identity::{platform, system_name};
pub use os_integration::{ensure_os_integration, register_os_integration};
pub use secret_clipboard::{
    SECRET_CLIPBOARD_TTL_SECS, clear_expired as clear_expired_secret,
    clear_outstanding as clear_secret_clipboard_now, set_secret_tracked as set_secret_clipboard,
};
pub use session_watch::{SessionEvent, on_user_left};

const UPLOAD_QUEUE: &str = "upload-queue.txt";
const DOWNLOAD_QUEUE: &str = "download-queue.txt";

pub fn queue_upload(path: &str) -> std::io::Result<()> {
    use std::io::Write;
    let queue_path = queue_file_path();
    if let Some(parent) = queue_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(queue_path)?;
    writeln!(file, "{path}")?;
    Ok(())
}

pub fn drain_upload_queue() -> std::io::Result<Vec<String>> {
    let queue_path = queue_file_path();
    if !queue_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&queue_path)?;
    std::fs::write(&queue_path, "")?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn queue_file_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SilentSilo")
        .join(UPLOAD_QUEUE)
}

/// Overwrites rather than appends — a second "Save here from SilentSilo"
/// background click before the first is handled should replace the target
/// directory, not queue up two downloads.
pub fn queue_download(target_dir: &str) -> std::io::Result<()> {
    let queue_path = download_queue_file_path();
    if let Some(parent) = queue_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(queue_path, target_dir)
}

pub fn drain_download_queue() -> std::io::Result<Option<String>> {
    let queue_path = download_queue_file_path();
    if !queue_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&queue_path)?;
    std::fs::write(&queue_path, "")?;
    let trimmed = content.trim();
    Ok(if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    })
}

fn download_queue_file_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SilentSilo")
        .join(DOWNLOAD_QUEUE)
}
