//! Where a silo's plaintext lives while it is open. Not in the silo
//! folder, which the user may have put on a NAS, an external drive or a
//! OneDrive-redirected Documents. Anything decrypted lives here on the
//! local machine and is wiped when the silo locks; what stays in the silo
//! folder is ciphertext, a public salt and wrapped keys.

use std::path::{Path, PathBuf};

use uuid::Uuid;

/// Distinguishes this app's derived directory names from anyone else's.
const WORK_NAMESPACE: Uuid = Uuid::from_bytes([
    0x5c, 0x1e, 0x7a, 0x30, 0x9b, 0x44, 0x4e, 0x1a, 0xa2, 0x77, 0x63, 0x0f, 0x8d, 0x21, 0x4b, 0x90,
]);

/// The machine-local base for every silo's scratch space.
///
/// Local rather than roaming on Windows: a roaming profile is copied to a
/// domain server at logon, which would defeat the point of moving plaintext
/// out of a synced folder in the first place.
pub fn work_base() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("SilentSilo").join("work");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(cache) = std::env::var("XDG_CACHE_HOME") {
            return PathBuf::from(cache).join("silentsilo").join("work");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".cache")
                .join("silentsilo")
                .join("work");
        }
    }
    std::env::temp_dir().join("silentsilo-work")
}

/// The scratch directory for the silo stored at `silo_root`.
///
/// Derived from the path rather than from the silo's id, so it can be
/// answered without opening anything — `VaultPaths::exists()` needs it
/// before there is a session to ask. Moving a silo folder simply earns it a
/// new scratch directory; nothing durable lives here.
pub fn work_dir_for(silo_root: &Path) -> PathBuf {
    let key = silo_root.to_string_lossy().to_lowercase();
    work_base()
        .join("open")
        .join(Uuid::new_v5(&WORK_NAMESPACE, key.as_bytes()).to_string())
}

/// Where a silo's secrets go when the OS keyring will not hold them.
/// Keyed by the silo's id rather than its path, unlike everything else
/// here: a silo moved to another drive must not look unprovisioned.
/// Outside the silo folder, because the folder alone is not supposed to
/// open the vault.
pub fn secrets_dir_for(silo_id: Uuid) -> PathBuf {
    work_base().join("secrets").join(silo_id.to_string())
}

/// Writes a file only this user can read, creating its directory first.
///
/// The mode matters on Unix alone: Windows inherits a per-user ACL from
/// `LOCALAPPDATA`, while a fresh file under `~/.cache` is world-readable by
/// default on most distributions.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        return Ok(());
    }

    #[cfg(not(unix))]
    std::fs::write(path, bytes)
}

/// Creates a directory no other local user can enter. Everything this
/// module hands out holds plaintext, and `create_dir_all` leaves 0755
/// under a default umask. Only the leaf is narrowed; the directories above
/// it hold nothing but derived names.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

/// Marks a file the app just decrypted as read-only.
///
/// Read-only is wanted for one reason: an editor must not save into a copy
/// that is about to be wiped. `Permissions::set_readonly(true)` achieves that
/// on Unix by clearing the write bits alone, so a file created under the
/// usual umask lands on 0444 and every account on the machine can read it.
/// 0400 stops the editor just as well and stops the neighbours too.
pub fn seal_readonly(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400));
    }

    #[cfg(not(unix))]
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// Machine-local bookkeeping that outlives a lock.
///
/// Separate from the scratch directory precisely so that wiping plaintext
/// does not also throw away the record of which blobs are already in the
/// bucket — that record is expensive to rebuild and harmless to keep.
pub fn cache_dir_for(silo_root: &Path) -> PathBuf {
    let key = silo_root.to_string_lossy().to_lowercase();
    work_base()
        .join("cache")
        .join(Uuid::new_v5(&WORK_NAMESPACE, key.as_bytes()).to_string())
}

/// Removes this machine's bookkeeping for a silo: which blobs it holds and
/// which have reached the bucket.
///
/// Kept out of `wipe_work_dir` on purpose. Locking a silo throws away its
/// plaintext and keeps this, because the record is expensive to rebuild and
/// harmless to keep. It is only wrong when the silo it describes has ceased
/// to exist, which is the one case this is for.
pub fn wipe_cache_dir(silo_root: &Path) {
    let _ = std::fs::remove_dir_all(cache_dir_for(silo_root));
}

/// Everything this machine holds for a silo outside the silo folder itself.
///
/// Both directories are keyed by the silo's path, not by its id, so a new
/// silo made in a folder a previous one used lands on the previous one's
/// state. Provisioning and deletion both call this so that the only thing a
/// path can hand to its next tenant is an empty directory.
pub fn wipe_machine_state(silo_root: &Path) {
    wipe_work_dir(silo_root);
    wipe_cache_dir(silo_root);
}

/// Removes a silo's scratch directory and everything in it.
pub fn wipe_work_dir(silo_root: &Path) {
    let dir = work_dir_for(silo_root);
    if !dir.exists() {
        return;
    }
    // Files opened from the silo are written read-only so an editor cannot
    // save into a copy that is about to vanish; Windows will not delete a
    // read-only file, so the bit has to come off first.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            clear_readonly(&entry.path());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn clear_readonly(path: &Path) {
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                clear_readonly(&entry.path());
            }
        }
        return;
    }

    // Owner only. `set_readonly(false)` sets the write bit for group and
    // world too, which would widen a 0400 file on its way to being deleted.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() | 0o200;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
        }
    }

    #[cfg(not(unix))]
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// Whether a path looks like it is inside a consumer sync client's folder.
/// Advisory only: it is their disk. Name matching is crude and misses
/// relocated folders, and a false negative costs nothing since the
/// plaintext never goes into the silo folder either way.
pub fn detect_sync_provider(path: &Path) -> Option<&'static str> {
    let text = path.to_string_lossy().to_lowercase();
    for (needle, name) in [
        ("onedrive", "OneDrive"),
        ("dropbox", "Dropbox"),
        ("google drive", "Google Drive"),
        ("googledrive", "Google Drive"),
        ("g:\\my drive", "Google Drive"),
        ("icloud", "iCloud Drive"),
        ("nextcloud", "Nextcloud"),
        ("mega\\", "MEGA"),
        ("pcloud", "pCloud"),
        ("yandexdisk", "Yandex Disk"),
        ("box sync", "Box"),
    ] {
        if text.contains(needle) {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_silos_get_separate_scratch_directories() {
        let a = work_dir_for(Path::new("/silos/personal"));
        let b = work_dir_for(Path::new("/silos/work"));
        assert_ne!(a, b);
    }

    #[test]
    fn the_same_silo_always_gets_the_same_directory() {
        // It has to be answerable without opening anything, and stable
        // across launches, or a lock would fail to find what to wipe.
        assert_eq!(
            work_dir_for(Path::new("/silos/personal")),
            work_dir_for(Path::new("/silos/personal"))
        );
    }

    #[test]
    fn windows_path_casing_does_not_split_a_silo_in_two() {
        // `C:\Users` and `c:\users` are the same directory, and two scratch
        // spaces for one silo would mean a lock wiping only half of it.
        assert_eq!(
            work_dir_for(Path::new(r"C:\Users\alex\Silos\Personal")),
            work_dir_for(Path::new(r"c:\users\alex\silos\personal"))
        );
    }

    #[test]
    fn scratch_never_lands_inside_the_silo_itself() {
        // The whole point: nothing decrypted may be written where the user
        // pointed the silo.
        let root = Path::new(r"C:\Users\alex\OneDrive\Silos\Personal");
        assert!(!work_dir_for(root).starts_with(root));
    }

    #[test]
    fn common_sync_folders_are_recognised() {
        assert_eq!(
            detect_sync_provider(Path::new(r"C:\Users\alex\OneDrive\Documents\Silo")),
            Some("OneDrive")
        );
        assert_eq!(
            detect_sync_provider(Path::new("/home/alex/Dropbox/silo")),
            Some("Dropbox")
        );
        assert_eq!(
            detect_sync_provider(Path::new(r"C:\Users\alex\Google Drive\silo")),
            Some("Google Drive")
        );
    }

    #[test]
    fn an_ordinary_folder_is_not_flagged() {
        assert!(detect_sync_provider(Path::new(r"D:\Silos\Personal")).is_none());
        assert!(detect_sync_provider(Path::new("/home/alex/silos/personal")).is_none());
    }

    #[test]
    fn wiping_a_directory_that_is_not_there_is_not_an_error() {
        wipe_work_dir(Path::new("/silos/never-existed"));
    }

    #[test]
    fn a_sealed_file_cannot_be_written_over() {
        // The reason read-only is wanted at all: an editor must not save into
        // a copy that is about to be wiped.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opened.txt");
        std::fs::write(&path, b"plaintext").unwrap();

        seal_readonly(&path);

        assert!(
            std::fs::metadata(&path).unwrap().permissions().readonly(),
            "an opened file must be read-only"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_sealed_file_is_not_readable_by_other_users() {
        // `set_readonly(true)` only clears the write bits, so a file created
        // under the usual umask stayed at 0444 and every account on a shared
        // machine could read what the silo owner had open.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opened.txt");
        std::fs::write(&path, b"plaintext").unwrap();

        seal_readonly(&path);

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o400, "got {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn the_scratch_directory_is_not_readable_by_other_users() {
        // It holds the working copy of vault.db, which is the whole tree of
        // names in plaintext.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("work").join("nested");

        create_private_dir(&dir).unwrap();

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "got {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn making_a_file_deletable_again_does_not_widen_it() {
        // Wiping clears read-only first, because Windows will not delete a
        // read-only file. Doing that with `set_readonly(false)` sets the write
        // bit for group and world on the way past.
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opened.txt");
        std::fs::write(&path, b"plaintext").unwrap();
        seal_readonly(&path);

        clear_readonly(&path);

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got {mode:o}");
    }

    #[test]
    fn wiping_removes_read_only_files() {
        // An opened file is written read-only on purpose, and Windows
        // refuses to delete those — so plaintext would survive the lock.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("silo");
        let work = work_dir_for(&root);
        std::fs::create_dir_all(&work).unwrap();

        let locked = work.join("secret.txt");
        std::fs::write(&locked, b"plaintext").unwrap();
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&locked, perms).unwrap();

        wipe_work_dir(&root);
        assert!(
            !work.exists(),
            "the scratch directory must not survive a lock"
        );
    }
}
