//! OS shell integration — CLI args and single-instance upload queue.

mod autostart;
pub mod autotype;
pub mod browser_pipe;
mod cli;
mod clipboard;
pub mod disk_space;
mod files;
mod foreground;
mod hardening;
mod identity;
#[cfg(target_os = "macos")]
mod mac_input;
#[cfg(target_os = "macos")]
mod macos;
mod os_integration;
mod secret_clipboard;
mod session_watch;
pub mod ssh_agent_channel;
mod tray;
#[cfg(unix)]
mod unix_place;
#[cfg(windows)]
mod win_pipe;
#[cfg(windows)]
pub mod win_process;
#[cfg(windows)]
mod windows;

pub use autostart::{autostart_enabled, autostart_supported, ensure_autostart, set_autostart};
pub use cli::{ShellAction, parse_args};
pub use clipboard::read_file_paths as read_clipboard_file_paths;
pub use files::{mark_from_elsewhere, write_private_file};
pub use foreground::{ForegroundWindow, foreground_window};
pub use hardening::harden_process;
pub use identity::{platform, system_name};
pub use os_integration::{ensure_os_integration, register_os_integration};
pub use secret_clipboard::{
    SECRET_CLIPBOARD_TTL_SECS, clear_expired as clear_expired_secret,
    clear_outstanding as clear_secret_clipboard_now, set_secret_tracked as set_secret_clipboard,
};
pub use session_watch::{SessionEvent, on_user_left};
pub use tray::available as tray_available;

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

/// The AppImage this app runs from, when it runs from one. `$APPIMAGE` alone
/// is not proof: a program started from a terminal inside another AppImage
/// inherits it. The runtime sets `$APPDIR` to its mount, and this executable
/// sits inside it only when it is the AppImage's own.
pub fn running_appimage() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    appimage_of(
        std::env::var_os("APPIMAGE").as_deref(),
        std::env::var_os("APPDIR").as_deref(),
        &exe,
    )
}

/// [`running_appimage`] with `$APPIMAGE`, `$APPDIR` and the executable
/// passed in.
fn appimage_of(
    appimage: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    exe: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let image = appimage.filter(|p| !p.is_empty())?;
    let mount = appdir.filter(|p| !p.is_empty())?;
    exe.starts_with(mount)
        .then(|| std::path::PathBuf::from(image))
}

#[cfg(test)]
mod appimage_tests {
    use super::appimage_of;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    const IMAGE: &str = "/home/ana/Apps/SilentSilo.AppImage";
    const MOUNT: &str = "/tmp/.mount_SilentAb12";

    #[test]
    fn inside_its_own_mount_it_is_the_appimage() {
        let exe = Path::new(MOUNT).join("usr/bin/silentsilo");
        assert_eq!(
            appimage_of(Some(OsStr::new(IMAGE)), Some(OsStr::new(MOUNT)), &exe),
            Some(PathBuf::from(IMAGE))
        );
    }

    #[test]
    fn appimage_inherited_from_another_appimage_is_not_ours() {
        // Started from a terminal inside another AppImage: both variables
        // are that one's, and this executable sits elsewhere.
        for exe in [
            "/usr/bin/silentsilo",
            "/tmp/.mount_SilentAb123/usr/bin/silentsilo",
        ] {
            assert_eq!(
                appimage_of(
                    Some(OsStr::new("/opt/Other.AppImage")),
                    Some(OsStr::new(MOUNT)),
                    Path::new(exe)
                ),
                None,
                "{exe}"
            );
        }
    }

    #[test]
    fn missing_or_empty_variables_mean_no_appimage() {
        let exe = Path::new(MOUNT).join("usr/bin/silentsilo");
        let (image, mount, empty) = (OsStr::new(IMAGE), OsStr::new(MOUNT), OsStr::new(""));
        for (appimage, appdir) in [
            (None, Some(mount)),
            (Some(image), None),
            (Some(empty), Some(mount)),
            (Some(image), Some(empty)),
            (None, None),
        ] {
            assert_eq!(appimage_of(appimage, appdir, &exe), None);
        }
    }
}
