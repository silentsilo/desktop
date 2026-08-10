//! "Start when Windows starts", as a per-user Run entry (HKCU, no admin).
//! The app is meant to be running rather than launched: the Explorer verbs
//! forward to the unlocked instance, the sync timer only ticks while a
//! process exists, and a cold start costs a key prompt. HKCU\...\Run needs
//! no admin, is removed by one DeleteRegValue, and shows up in Task
//! Manager's Startup tab, where people go to turn these off.

#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "SilentSilo";

/// Whether this build can start with the OS at all. Callers branch on this
/// rather than on `cfg!`, so adding macOS or Linux later stays a change to
/// this module.
pub fn autostart_supported() -> bool {
    cfg!(windows)
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
#[cfg(windows)]
fn marker_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SilentSilo")
        .join("autostart-initialized")
}

#[cfg(not(windows))]
pub fn autostart_enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_autostart(_enabled: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "starting with the system is only implemented on Windows",
    ))
}

#[cfg(not(windows))]
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

    #[test]
    fn enable_then_disable_round_trips() {
        let _lock = lock_or_recover();
        let _restore = PreviousValue::capture();

        set_autostart(true).expect("enable");
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

        set_autostart(false).expect("first");
        set_autostart(false).expect("second");
        assert!(!autostart_enabled());
    }
}
