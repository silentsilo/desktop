//! Files the app writes out of a silo in clear: exports, readable by this
//! user alone, and copies handed to another program, marked as coming from
//! elsewhere.

use std::io;
use std::path::Path;

/// Writes `bytes` to `path`, replacing any file there, readable by this
/// user alone: 0600 on Unix, and on Windows a protected access list naming
/// only this user rather than whatever the folder passes down.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` applies only to a file it creates.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        file.write_all(bytes)
    }
    #[cfg(windows)]
    {
        win::write_private_file(path, bytes)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::write(path, bytes)
    }
}

/// Marks a file as downloaded from the internet (Mark of the Web), so
/// Windows and the programs that honour it warn before running it or open
/// it protected. A copy decrypted from a silo may have come from anyone
/// who could write to it. Nothing on other systems, and nothing on a drive
/// that keeps no such mark.
pub fn mark_from_elsewhere(path: &Path) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;

    use ::windows::Win32::Foundation::{GENERIC_WRITE, HLOCAL, LocalFree};
    use ::windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use ::windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use ::windows::Win32::Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    };
    use ::windows::core::{HSTRING, PCWSTR};

    pub(super) fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let sid = crate::win_process::current_user_sid()?;
        let sddl = HSTRING::from(format!("D:P(A;;FA;;;{sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the SDDL string outlives the call; the descriptor is freed
        // below, after the file is created.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut sd,
                None,
            )?;
        }
        // A file that exists keeps its own list, whatever is asked here, so
        // the one the person chose to replace goes first.
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                // SAFETY: allocated by the call above.
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(sd.0)));
                }
                return Err(e);
            }
            _ => {}
        }
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: the name is NUL-terminated and outlives the call, as do the
        // attributes and their descriptor, freed right after.
        let handle = unsafe {
            let handle = CreateFileW(
                PCWSTR(wide.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                Some(&attributes),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            );
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            handle?
        };
        // SAFETY: a handle this call just opened, owned by the File from here.
        let mut file = unsafe { std::fs::File::from_raw_handle(handle.0) };
        file.write_all(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_file_replaces_what_was_there() {
        let dir = tempfile_dir();
        let path = dir.join("export.csv");
        std::fs::write(&path, b"older and longer").unwrap();
        write_private_file(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_file_is_this_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile_dir();
        let path = dir.join("export.kdbx");
        write_private_file(&path, b"x").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn an_opened_copy_carries_the_mark() {
        let dir = tempfile_dir();
        let path = dir.join("invoice.pdf");
        std::fs::write(&path, b"%PDF").unwrap();
        mark_from_elsewhere(&path);
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let mark = std::fs::read_to_string(stream).unwrap();
        assert!(mark.contains("ZoneId=3"), "{mark}");
        assert_eq!(std::fs::read(&path).unwrap(), b"%PDF");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "silentsilo-shell-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
