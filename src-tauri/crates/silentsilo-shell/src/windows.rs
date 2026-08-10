//! Windows Explorer context menu (per-user HKCU, no admin).
//!
//! Two distinct actions, each with its own registry key so they coexist and
//! can be told apart:
//!   - "Upload to SilentSilo" — right-click ON a file or a folder item
//!     (pushes that item up into the vault).
//!   - "Download from SilentSilo" — right-click on empty space inside a
//!     folder window, or on the Desktop itself (pulls a chosen vault folder
//!     down into wherever you right-clicked).

use std::path::Path;

const UPLOAD_KEY: &str = "SilentSiloUpload";
const UPLOAD_LABEL: &str = "Upload to SilentSilo";
const DOWNLOAD_KEY: &str = "SilentSiloDownload";
const DOWNLOAD_LABEL: &str = "Download from SilentSilo";

pub fn register_context_menu(exe_path: &Path) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    // Deliberately NOT canonicalized: on Windows, `Path::canonicalize()`
    // prepends the `\\?\` extended-length prefix, which breaks Explorer's
    // shell-verb command dispatch when embedded in a quoted command string
    // (surfaces as "Windows cannot access the specified device, path, or
    // file" when the verb is invoked). `exe_path` is already an absolute
    // path here (it comes from `std::env::current_exe()`), so no resolution
    // is needed anyway.
    let exe_str = exe_path.display().to_string();
    let icon = exe_str.clone();
    let cmd_upload = format!("\"{exe_str}\" --upload \"%1\"");
    let cmd_download = format!("\"{exe_str}\" --download \"%V\"");

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Right-click directly on a file or a folder item — upload it.
    write_shell_key(
        &hkcu,
        r"Software\Classes\*\shell",
        UPLOAD_KEY,
        UPLOAD_LABEL,
        &cmd_upload,
        &icon,
    )?;
    write_shell_key(
        &hkcu,
        r"Software\Classes\Directory\shell",
        UPLOAD_KEY,
        UPLOAD_LABEL,
        &cmd_upload,
        &icon,
    )?;

    // Right-click on empty space inside a folder window, or on the Desktop
    // (a separate registry namespace from a plain folder's background) —
    // download into that location.
    write_shell_key(
        &hkcu,
        r"Software\Classes\Directory\Background\shell",
        DOWNLOAD_KEY,
        DOWNLOAD_LABEL,
        &cmd_download,
        &icon,
    )?;
    write_shell_key(
        &hkcu,
        r"Software\Classes\DesktopBackground\shell",
        DOWNLOAD_KEY,
        DOWNLOAD_LABEL,
        &cmd_download,
        &icon,
    )?;

    Ok(())
}

fn write_shell_key(
    hkcu: &winreg::RegKey,
    classes_subkey: &str,
    menu_key: &str,
    label: &str,
    command: &str,
    icon: &str,
) -> std::io::Result<()> {
    let menu_path = format!(r"{classes_subkey}\{menu_key}");
    let menu = hkcu.create_subkey(&menu_path)?.0;
    menu.set_value("", &label)?;
    menu.set_value("Icon", &icon)?;

    let cmd_key = menu.create_subkey("command")?.0;
    cmd_key.set_value("", &command)?;
    Ok(())
}

/// Removes Explorer context-menu entries — used both for a clean uninstall
/// and to clear out a previous version's entries before re-registering
/// (see `os_integration::MENU_VERSION`).
pub fn unregister_context_menu() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for base in [
        r"Software\Classes\*\shell",
        r"Software\Classes\Directory\shell",
        r"Software\Classes\Directory\Background\shell",
        r"Software\Classes\DesktopBackground\shell",
    ] {
        for key in [UPLOAD_KEY, DOWNLOAD_KEY] {
            let path = format!(r"{base}\{key}");
            let _ = hkcu.delete_subkey_all(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    /// `cargo test` runs tests in parallel by default, but these two tests
    /// both write to the exact same registry keys — without serializing
    /// them, one test's `unregister_context_menu()` can wipe out state the
    /// other is mid-assertion on. `lock_or_recover` tolerates a poisoned
    /// mutex (one test panicking) rather than deadlocking the other.
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock_or_recover() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The exe path currently registered under the upload verb, if any —
    /// parsed straight out of the registry rather than tracked separately,
    /// so it reflects reality regardless of what wrote it (the real app,
    /// an earlier test run, ...).
    fn currently_registered_exe_path() -> Option<PathBuf> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let cmd: String = hkcu
            .open_subkey(format!(r"Software\Classes\*\shell\{UPLOAD_KEY}\command"))
            .ok()?
            .get_value("")
            .ok()?;
        let start = cmd.find('"')? + 1;
        let end = start + cmd[start..].find('"')?;
        Some(PathBuf::from(&cmd[start..end]))
    }

    /// These tests exercise the exact same HKCU keys the real app uses —
    /// necessary to actually prove the registration logic works, but that
    /// means a naive register-then-delete test would clobber a real
    /// installation on a dev machine as a side effect of `cargo test`.
    /// Capturing on construction and restoring on drop (even on panic)
    /// keeps that from happening.
    struct PreviousRegistration(Option<PathBuf>);

    impl PreviousRegistration {
        fn capture() -> Self {
            Self(currently_registered_exe_path())
        }
    }

    impl Drop for PreviousRegistration {
        fn drop(&mut self) {
            if let Some(exe) = &self.0 {
                let _ = register_context_menu(exe);
            }
        }
    }

    /// Exercises the real per-user registry (HKCU, no admin needed) — the
    /// same code path the app runs on every startup — then always cleans up
    /// afterwards so the test suite never leaves entries behind.
    #[test]
    fn register_then_unregister_round_trips_all_four_locations() {
        let _lock = lock_or_recover();
        let _restore = PreviousRegistration::capture();
        let exe = std::env::current_exe().expect("current_exe");
        register_context_menu(&exe).expect("register");

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let expect_label = |base: &str, key: &str, label: &str| {
            let menu = hkcu
                .open_subkey(format!(r"{base}\{key}"))
                .unwrap_or_else(|e| panic!("missing key {base}\\{key}: {e}"));
            let value: String = menu.get_value("").expect("default value");
            assert_eq!(value, label, "wrong label at {base}\\{key}");

            let cmd = menu.open_subkey("command").expect("command subkey");
            let command: String = cmd.get_value("").expect("command value");
            assert!(
                command.contains(&exe.display().to_string()),
                "command should reference the exe path"
            );
            // A `\\?\`-prefixed (canonicalized) path here breaks Explorer's
            // verb dispatch — this is the exact regression this test exists
            // to catch, and a plain `.contains()` check above wouldn't
            // notice it (the suffix still matches).
            assert!(
                !command.contains(r"\\?\"),
                "command must not use the \\\\?\\ extended-length prefix: {command}"
            );
        };

        expect_label(r"Software\Classes\*\shell", UPLOAD_KEY, UPLOAD_LABEL);
        expect_label(
            r"Software\Classes\Directory\shell",
            UPLOAD_KEY,
            UPLOAD_LABEL,
        );
        expect_label(
            r"Software\Classes\Directory\Background\shell",
            DOWNLOAD_KEY,
            DOWNLOAD_LABEL,
        );
        expect_label(
            r"Software\Classes\DesktopBackground\shell",
            DOWNLOAD_KEY,
            DOWNLOAD_LABEL,
        );

        unregister_context_menu().expect("unregister");

        for base in [
            r"Software\Classes\*\shell",
            r"Software\Classes\Directory\shell",
            r"Software\Classes\Directory\Background\shell",
            r"Software\Classes\DesktopBackground\shell",
        ] {
            assert!(
                hkcu.open_subkey(format!(r"{base}\{UPLOAD_KEY}")).is_err(),
                "upload key should be gone at {base}"
            );
            assert!(
                hkcu.open_subkey(format!(r"{base}\{DOWNLOAD_KEY}")).is_err(),
                "download key should be gone at {base}"
            );
        }
    }

    #[test]
    fn upload_command_uses_percent1_and_download_uses_percentv() {
        let _lock = lock_or_recover();
        let _restore = PreviousRegistration::capture();
        let exe = std::env::current_exe().expect("current_exe");
        register_context_menu(&exe).expect("register");

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let command_of = |base: &str, key: &str| -> String {
            hkcu.open_subkey(format!(r"{base}\{key}\command"))
                .unwrap()
                .get_value("")
                .unwrap()
        };

        assert!(command_of(r"Software\Classes\*\shell", UPLOAD_KEY).contains("--upload \"%1\""));
        assert!(
            command_of(r"Software\Classes\Directory\Background\shell", DOWNLOAD_KEY)
                .contains("--download \"%V\"")
        );

        unregister_context_menu().expect("cleanup");
    }
}
