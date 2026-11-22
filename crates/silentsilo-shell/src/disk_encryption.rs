//! Whether the disk the opened files land on is encrypted. A file opened in
//! another application is written there while it is open, and Windows, the
//! viewer and the page file leave traces of their own; full-disk encryption
//! (BitLocker or Device Encryption, FileVault, LUKS) is the one thing that
//! covers all of it against someone who takes the disk.
//!
//! Asked without admin rights. `None` when the answer is not known, which
//! the window treats as "say nothing": a wrong warning is worse than none.

use std::path::Path;

/// Whether the volume holding `path` is encrypted, as far as the system
/// tells a process without admin rights.
pub fn volume_encrypted(path: &Path) -> Option<bool> {
    platform::volume_encrypted(path)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::Win32::Storage::FileSystem::GetVolumePathNameW;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt32;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
    use windows::core::{PCWSTR, w};

    /// `System.Volume.BitLockerProtection`, the value Explorer shows as the
    /// padlock on a drive: 1, 3 and 5 are on, 2 is off. Undocumented as
    /// numbers; other values, or no value, mean unknown.
    fn from_shell_value(value: u32) -> Option<bool> {
        match value {
            1 | 3 | 5 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    pub(super) fn volume_encrypted(path: &Path) -> Option<bool> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut root = [0u16; 261];
        // SAFETY: `wide` is NUL-terminated and `root` is a writable buffer of
        // the length passed; the COM calls below take only live locals, and
        // the item is released when it goes out of scope.
        unsafe {
            GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root).ok()?;
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let mut key = Default::default();
            PSGetPropertyKeyFromName(w!("System.Volume.BitLockerProtection"), &mut key).ok()?;
            let item: IShellItem2 =
                SHCreateItemFromParsingName(PCWSTR(root.as_ptr()), None).ok()?;
            let value = item.GetProperty(&key).ok()?;
            from_shell_value(PropVariantToUInt32(&value).ok()?)
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_padlock_values_read_as_explorer_shows_them() {
            assert_eq!(super::from_shell_value(1), Some(true));
            assert_eq!(super::from_shell_value(3), Some(true));
            assert_eq!(super::from_shell_value(5), Some(true));
            assert_eq!(super::from_shell_value(2), Some(false));
            assert_eq!(super::from_shell_value(0), None);
            assert_eq!(super::from_shell_value(6), None);
        }

        #[test]
        fn the_system_drive_gives_an_answer_or_says_it_has_none() {
            // Whatever this machine is, asking must not fail or hang.
            let _ = super::volume_encrypted(std::path::Path::new("C:\\Windows"));
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    /// FileVault covers the whole internal disk; `fdesetup status` answers
    /// without admin rights. A file on an external disk is not covered by
    /// it, and is reported as unknown rather than guessed.
    pub(super) fn volume_encrypted(path: &Path) -> Option<bool> {
        if path.starts_with("/Volumes") {
            return None;
        }
        let out = std::process::Command::new("/usr/bin/fdesetup")
            .arg("status")
            .output()
            .ok()?;
        parse(&String::from_utf8_lossy(&out.stdout))
    }

    fn parse(status: &str) -> Option<bool> {
        if status.contains("FileVault is On") {
            Some(true)
        } else if status.contains("FileVault is Off") {
            Some(false)
        } else {
            None
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn fdesetup_answers_are_read() {
            assert_eq!(super::parse("FileVault is On.\n"), Some(true));
            assert_eq!(super::parse("FileVault is Off.\n"), Some(false));
            assert_eq!(
                super::parse("FileVault is On.\nDeferred enablement appears to be active"),
                Some(true)
            );
            assert_eq!(super::parse(""), None);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// The block device under `path`, followed down through device-mapper
    /// (LVM over LUKS is the common layout): encrypted when a dm-crypt
    /// device sits anywhere below it. A file system with no block device of
    /// its own (btrfs subvolumes, tmpfs, network mounts) gives no answer.
    pub(super) fn volume_encrypted(path: &Path) -> Option<bool> {
        let dev = std::fs::metadata(path).ok()?.dev();
        let (major, minor) = (libc::major(dev), libc::minor(dev));
        let node = std::fs::canonicalize(format!("/sys/dev/block/{major}:{minor}")).ok()?;
        Some(crypt_below(&node, 0))
    }

    fn crypt_below(node: &Path, depth: u8) -> bool {
        if depth > 8 {
            return false;
        }
        let uuid = std::fs::read_to_string(node.join("dm").join("uuid")).unwrap_or_default();
        if uuid.starts_with("CRYPT-") {
            return true;
        }
        // A partition's own `slaves` is empty; its disk's is the one to read.
        let mut parents: Vec<PathBuf> = Vec::new();
        for dir in [node.to_path_buf(), node.join("..")] {
            if let Ok(entries) = std::fs::read_dir(dir.join("slaves")) {
                parents.extend(
                    entries
                        .flatten()
                        .filter_map(|e| std::fs::canonicalize(e.path()).ok()),
                );
            }
        }
        parents.iter().any(|p| crypt_below(p, depth + 1))
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub(super) fn volume_encrypted(_path: &std::path::Path) -> Option<bool> {
        None
    }
}
