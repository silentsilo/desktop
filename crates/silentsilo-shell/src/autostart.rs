//! "Start when I sign in", as a per-user entry that needs no admin.
//! The app is meant to be running rather than launched: the shell verbs
//! forward to the unlocked instance, the sync timer only ticks while a
//! process exists, and a cold start costs a key prompt.
//!
//! On Windows that is a Run value under HKCU, removed by one DeleteRegValue
//! and listed in Task Manager's Startup tab. On macOS it is a LaunchAgent
//! plist in the user's library, which System Settings lists under Login
//! Items. On Linux it is a `.desktop` file in `~/.config/autostart`, the
//! freedesktop convention every desktop environment reads, and the one the
//! desktop's own settings panel edits. All three carry `--autostart`, the
//! flag that keeps the window hidden.

#[cfg(windows)]
use std::path::Path;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use std::path::PathBuf;

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "SilentSilo";

/// Whether this build can start with the OS at all. Callers branch on this
/// rather than on `cfg!`, so adding macOS or Linux later stays a change to
/// this module.
pub fn autostart_supported() -> bool {
    cfg!(any(windows, target_os = "macos", target_os = "linux"))
}

/// The command Windows runs at sign-in. `--autostart` is the whole reason
/// the flag exists: without it the app would put its window in front of
/// whatever the user is doing, on every boot.
#[cfg(windows)]
fn autostart_command(exe: &Path) -> String {
    // Not canonicalized, for the same reason as the context-menu commands in
    // `windows.rs`: the `\\?\` extended-length prefix breaks the shell that
    // expands this string. `current_exe()` is already absolute.
    format!("\"{}\" --autostart", exe.display())
}

#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|run| run.get_value::<String, _>(VALUE_NAME))
        .is_ok()
}

#[cfg(windows)]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let run = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?.0;
    if !enabled {
        return match run.delete_value(VALUE_NAME) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let exe = std::env::current_exe()?;
    run.set_value(VALUE_NAME, &autostart_command(&exe))
}

/// Called once per launch. On a machine that has never run this app it
/// turns autostart on, the default the rest of the design assumes. On
/// every later launch it only repairs a stale path and never re-creates a
/// deleted entry: silently undoing the user's "no" would be worse.
#[cfg(windows)]
pub fn ensure_autostart() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let marker = marker_path();
    if !marker.is_file() {
        set_autostart(true)?;
        if let Some(parent) = marker.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&marker, "")?;
        return Ok(());
    }

    if !autostart_enabled() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let expected = autostart_command(&exe);
    let run = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?.0;
    let current: String = run.get_value(VALUE_NAME).unwrap_or_default();
    if current != expected {
        run.set_value(VALUE_NAME, &expected)?;
    }
    Ok(())
}

/// Presence of this file, not the registry value, is what says "this machine
/// has already been asked once". The uninstaller deletes it along with the
/// Run entry, so a reinstall is treated as a first install again.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn marker_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SilentSilo")
        .join("autostart-initialized")
}

/// A LaunchAgent rather than `SMAppService`: the modern API registers the
/// app itself as a login item, and launchd then starts it with no arguments,
/// so there is no way to pass `--autostart` and the window would open on
/// every login. A LaunchAgent is a command line, exactly like the Run value
/// on Windows, and macOS 13 lists it under Login Items all the same.
#[cfg(target_os = "macos")]
mod mac {
    use super::marker_path;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// The bundle identifier, which is what launchd and System Settings key
    /// the entry by.
    const LABEL: &str = "com.silentsilo.desktop";

    fn plist_path() -> std::io::Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
        })?;
        Ok(home
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn xml_escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    /// `LimitLoadToSessionType Aqua`: only in a logged-in graphical session,
    /// never for an SSH login. `RunAtLoad` is the whole point.
    fn plist_for(exe: &Path) -> String {
        let exe = xml_escape(&exe.display().to_string());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--autostart</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#
        )
    }

    /// The launchd domain for the current user's graphical session.
    fn domain() -> String {
        // SAFETY: getuid has no preconditions and cannot fail.
        format!("gui/{}", unsafe { libc::getuid() })
    }

    /// Best effort, deliberately: the file is what "enabled" means, and
    /// launchd reads it at the next login regardless. Loading it now only
    /// makes the change take effect immediately, and `bootstrap` refuses a
    /// job that is already loaded, which is not an error worth surfacing.
    fn launchctl(verb: &str, plist: &Path) {
        let _ = Command::new("/bin/launchctl")
            .arg(verb)
            .arg(domain())
            .arg(plist)
            .status();
    }

    /// The command the plist would carry if written now.
    fn expected_plist() -> std::io::Result<String> {
        Ok(plist_for(&std::env::current_exe()?))
    }

    pub fn autostart_enabled() -> bool {
        plist_path().map(|p| p.is_file()).unwrap_or(false)
    }

    pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
        let plist = plist_path()?;
        if !enabled {
            if plist.is_file() {
                launchctl("bootout", &plist);
                std::fs::remove_file(&plist)?;
            }
            return Ok(());
        }
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&plist, expected_plist()?)?;
        launchctl("bootstrap", &plist);
        Ok(())
    }

    /// Same contract as the Windows version: on a machine that has never run
    /// this app it turns autostart on; on every later launch it only repairs
    /// a stale path, and never re-creates an entry the user removed.
    pub fn ensure_autostart() -> std::io::Result<()> {
        let marker = marker_path();
        if !marker.is_file() {
            set_autostart(true)?;
            if let Some(parent) = marker.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&marker, "")?;
            return Ok(());
        }

        if !autostart_enabled() {
            return Ok(());
        }
        let plist = plist_path()?;
        let expected = expected_plist()?;
        let current = std::fs::read_to_string(&plist).unwrap_or_default();
        if current != expected {
            launchctl("bootout", &plist);
            std::fs::write(&plist, expected)?;
            launchctl("bootstrap", &plist);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use mac::{autostart_enabled, ensure_autostart, set_autostart};

/// A `.desktop` file in `~/.config/autostart`, which is the freedesktop
/// spec every desktop environment implements and the one GNOME's and KDE's
/// own settings panels read and write. So turning autostart off in the
/// desktop's settings and turning it off here are the same operation on the
/// same file, which is what the hint in the interface promises.
#[cfg(target_os = "linux")]
mod linux {
    use super::marker_path;
    use std::path::{Path, PathBuf};

    /// The bundle identifier, matching the desktop entry the package
    /// installs, so the two are one entry rather than two.
    const LABEL: &str = "com.silentsilo.desktop";

    fn entry_path() -> std::io::Result<PathBuf> {
        let base = dirs::config_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "config directory not found")
        })?;
        Ok(base.join("autostart").join(format!("{LABEL}.desktop")))
    }

    /// Desktop entry values are not quoted, and the spec reserves a handful
    /// of characters inside them. A path holding one is rare and a silently
    /// broken Exec line is not worth the risk.
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    }

    /// `X-GNOME-Autostart-enabled` is what GNOME's own toggle writes, so an
    /// entry without it reads as enabled there, which is what we want.
    fn entry_for(exe: &Path) -> String {
        let exe = escape(&exe.display().to_string());
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=SilentSilo\n\
             Comment=Encrypted vault for files and passwords\n\
             Exec=\"{exe}\" --autostart\n\
             Icon={LABEL}\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        )
    }

    fn expected_entry() -> std::io::Result<String> {
        Ok(entry_for(&std::env::current_exe()?))
    }

    pub fn autostart_enabled() -> bool {
        entry_path().map(|p| p.is_file()).unwrap_or(false)
    }

    pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
        let entry = entry_path()?;
        if !enabled {
            if entry.is_file() {
                std::fs::remove_file(&entry)?;
            }
            return Ok(());
        }
        if let Some(parent) = entry.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&entry, expected_entry()?)
    }

    /// Same contract as the other two: on a machine that has never run this
    /// app it turns autostart on; on every later launch it only repairs a
    /// stale path, and never re-creates an entry the user removed.
    pub fn ensure_autostart() -> std::io::Result<()> {
        let marker = marker_path();
        if !marker.is_file() {
            set_autostart(true)?;
            if let Some(parent) = marker.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&marker, "")?;
            return Ok(());
        }

        if !autostart_enabled() {
            return Ok(());
        }
        let entry = entry_path()?;
        let expected = expected_entry()?;
        if std::fs::read_to_string(&entry).unwrap_or_default() != expected {
            std::fs::write(&entry, expected)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub use linux::{autostart_enabled, ensure_autostart, set_autostart};

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn autostart_enabled() -> bool {
    false
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn set_autostart(_enabled: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "starting with the system is only implemented on Windows, macOS and Linux",
    ))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn ensure_autostart() -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    /// These tests write the exact value a real installation uses, so they
    /// must not run at the same time as each other, and must put back what
    /// they found. A poisoned mutex is recovered rather than deadlocked on.
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock_or_recover() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct PreviousValue(Option<String>);

    impl PreviousValue {
        fn capture() -> Self {
            Self(
                RegKey::predef(HKEY_CURRENT_USER)
                    .open_subkey(RUN_KEY)
                    .and_then(|run| run.get_value::<String, _>(VALUE_NAME))
                    .ok(),
            )
        }
    }

    impl Drop for PreviousValue {
        fn drop(&mut self) {
            let run = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(RUN_KEY)
                .expect("run key")
                .0;
            match &self.0 {
                Some(value) => {
                    let _ = run.set_value(VALUE_NAME, value);
                }
                None => {
                    let _ = run.delete_value(VALUE_NAME);
                }
            }
        }
    }

    /// Whether this machine lets an unknown test binary touch the Run key at
    /// all. Antivirus startup protection denies it outright, and a denied
    /// environment proves nothing about the code either way.
    fn environment_forbids(result: &std::io::Result<()>) -> bool {
        matches!(result, Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied)
    }

    #[test]
    fn enable_then_disable_round_trips() {
        let _lock = lock_or_recover();
        let _restore = PreviousValue::capture();

        let enabled = set_autostart(true);
        if environment_forbids(&enabled) {
            eprintln!("skipped: this machine denies Run-key writes to test binaries");
            return;
        }
        enabled.expect("enable");
        assert!(autostart_enabled());

        let run = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN_KEY)
            .expect("run key");
        let command: String = run.get_value(VALUE_NAME).expect("value");
        let exe = std::env::current_exe().expect("current_exe");
        assert!(command.contains(&exe.display().to_string()));
        assert!(
            command.ends_with("--autostart"),
            "must start hidden, got: {command}"
        );
        assert!(
            !command.contains(r"\\?\"),
            "must not use the extended-length prefix: {command}"
        );

        set_autostart(false).expect("disable");
        assert!(!autostart_enabled());
    }

    #[test]
    fn disabling_twice_is_not_an_error() {
        let _lock = lock_or_recover();
        let _restore = PreviousValue::capture();

        let first = set_autostart(false);
        if environment_forbids(&first) {
            eprintln!("skipped: this machine denies Run-key writes to test binaries");
            return;
        }
        first.expect("first");
        set_autostart(false).expect("second");
        assert!(!autostart_enabled());
    }
}
