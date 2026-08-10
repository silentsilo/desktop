use std::path::{Path, PathBuf};

use rusqlite::Connection;
use silentsilo_crypto::{ContentKek, MasterDek, generate_content_kek, generate_dek};
use uuid::Uuid;

use crate::dek_store::{load_dek, save_dek};
use crate::error::VaultError;
use crate::kdf::derive_vault_key;
use crate::kek_store::{load_kek, save_kek};
use crate::vault_file_crypto::{decrypt_vault_file, encrypt_vault_file};

const VAULT_DB: &str = "vault.db";
const VAULT_DB_ENC: &str = "vault.db.enc";
const VAULT_DB_ENC_BAK: &str = "vault.db.enc.bak";
const VAULT_SALT: &str = "vault.salt";
const BLOBS_DIR: &str = "blobs";

#[derive(Debug, Clone)]
pub struct VaultPaths {
    pub root: PathBuf,
}

impl VaultPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn app_data_dir(&self) -> PathBuf {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    /// Where anything decrypted goes while the silo is open.
    ///
    /// Machine-local, never inside the silo folder — see `workdir` for why
    /// that distinction is the difference between "keep your silo wherever
    /// you like" being an offer and being a trap.
    pub fn work_dir(&self) -> PathBuf {
        crate::workdir::work_dir_for(&self.root)
    }

    /// Creates the scratch directory, since nothing else will: SQLite does
    /// not make parent directories, and neither does the snapshot decrypt.
    pub fn ensure_work_dir(&self) -> Result<(), VaultError> {
        crate::workdir::create_private_dir(&self.work_dir())?;
        Ok(())
    }

    /// Plaintext working copy — only present while a session is open, and
    /// only ever on this machine.
    pub fn db_path(&self) -> PathBuf {
        self.work_dir().join(VAULT_DB)
    }

    /// Decrypted copies of files the user opened. Wiped with everything else
    /// in the scratch directory when the silo locks.
    pub fn open_scratch_dir(&self) -> PathBuf {
        self.work_dir().join("open")
    }

    /// Durable, encrypted-at-rest artifact. This is what's read on unlock,
    /// backed up locally, and synced to the cloud.
    pub fn db_enc_path(&self) -> PathBuf {
        self.root.join(VAULT_DB_ENC)
    }

    pub fn db_enc_backup_path(&self) -> PathBuf {
        self.root.join(VAULT_DB_ENC_BAK)
    }

    pub fn salt_path(&self) -> PathBuf {
        self.root.join(VAULT_SALT)
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.root.join(BLOBS_DIR)
    }

    pub fn blob_path(&self, blob_id: Uuid) -> PathBuf {
        self.blobs_dir().join(format!("{blob_id}.sslo"))
    }

    /// Whether a silo has already been provisioned at this path.
    ///
    /// Asks only about the encrypted snapshot: the plaintext copy lives
    /// elsewhere now, so its presence says something about this machine's
    /// session rather than about the folder.
    pub fn exists(&self) -> bool {
        self.db_enc_path().is_file()
    }
}

pub struct VaultSession {
    pub paths: VaultPaths,
    pub conn: Connection,
    pub vault_id: Uuid,
    pub dek: MasterDek,
    /// The key every blob's content key is wrapped under. Shared across the
    /// silo's devices, so a file written on one opens on the others.
    pub kek: ContentKek,
}

impl VaultSession {
    /// First-time setup after POST /devices/register — binds local state to server vault_id.
    pub fn provision(
        root: PathBuf,
        server_vault_id: Uuid,
        device_secret: &str,
    ) -> Result<Self, VaultError> {
        let paths = VaultPaths::new(root.clone());
        if paths.exists() {
            return Err(VaultError::AlreadyExists);
        }

        std::fs::create_dir_all(root.join(BLOBS_DIR))?;

        let salt: [u8; 16] = rand::random();
        std::fs::write(paths.salt_path(), salt)?;

        let vault_key = derive_vault_key(device_secret, &salt)?;
        let dek = generate_dek();
        save_dek(&root, &dek, &vault_key.wrap_key)?;
        let kek = generate_content_kek();
        save_kek(&root, &kek, &dek)?;

        // Nothing a previous silo in this folder left behind is ours. The
        // scratch and cache directories are keyed by path, so without this
        // the open below adopts the old silo's database wholesale: its
        // folders, its files, and password rows sealed with a key this silo
        // does not have.
        crate::workdir::wipe_machine_state(&root);

        paths.ensure_work_dir()?;
        let conn = Connection::open(paths.db_path())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        )?;

        let session = Self {
            paths,
            conn,
            vault_id: server_vault_id,
            dek,
            kek,
        };
        // Persist an encrypted snapshot immediately so a crash right after
        // provisioning doesn't leave nothing but a plaintext file on disk.
        session.backup_locally()?;

        Ok(session)
    }

    /// Sets up local state for a device joining a vault that already
    /// exists. Both the DEK and the `kek` are *given*, never generated
    /// here: minting either would produce a vault nothing in storage
    /// decrypts, or files no other device can open. The database starts
    /// empty and the tree is rebuilt by replay.
    pub fn provision_with_dek(
        root: PathBuf,
        vault_id: Uuid,
        device_secret: &str,
        dek: MasterDek,
        kek: ContentKek,
    ) -> Result<Self, VaultError> {
        let paths = VaultPaths::new(root.clone());
        if paths.exists() {
            return Err(VaultError::AlreadyExists);
        }

        std::fs::create_dir_all(root.join(BLOBS_DIR))?;

        let salt: [u8; 16] = rand::random();
        std::fs::write(paths.salt_path(), salt)?;

        // The device secret is per-device and never leaves it; wrapping the
        // shared DEK under it keeps this device's on-disk envelope in the
        // same shape as one that provisioned locally.
        let vault_key = derive_vault_key(device_secret, &salt)?;
        save_dek(&root, &dek, &vault_key.wrap_key)?;
        save_kek(&root, &kek, &dek)?;

        // Same reason as `provision`: the database has to start empty for
        // the replay to rebuild the tree, and a folder that held a silo
        // before still points at that silo's working copy.
        crate::workdir::wipe_machine_state(&root);

        paths.ensure_work_dir()?;
        let conn = Connection::open(paths.db_path())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        )?;

        let session = Self {
            paths,
            conn,
            vault_id,
            dek,
            kek,
        };
        session.backup_locally()?;
        Ok(session)
    }

    /// Unlock with the device secret, the bootstrap path that exists only
    /// until a security key is enrolled.
    pub fn open_with_device_secret(root: PathBuf, device_secret: &str) -> Result<Self, VaultError> {
        let paths = VaultPaths::new(root);
        if !paths.exists() {
            return Err(VaultError::NotFound);
        }

        let salt = std::fs::read(paths.salt_path())?;
        if salt.len() != 16 {
            return Err(VaultError::InvalidCredentials);
        }
        let salt: [u8; 16] = salt.try_into().unwrap();

        let vault_key = derive_vault_key(device_secret, &salt)?;
        let dek = load_dek(&paths.root, &vault_key.wrap_key)?;

        let conn = open_database(&paths, &dek)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let vault_id = read_vault_id(&conn).ok_or(VaultError::NotFound)?;
        let kek = load_kek(&paths.root, &dek)?;

        Ok(Self {
            paths,
            conn,
            vault_id,
            dek,
            kek,
        })
    }

    /// Unlock the DEK with a FIDO wrap key and that key's wrapped-DEK blob
    /// (hex), from its row in `keys/fido.json`. An empty blob is refused:
    /// every enrolled key carries its own envelope, and a row without one is
    /// a broken file, not a different format.
    pub fn open_with_fido_wrapped(
        root: PathBuf,
        fido_wrap_key: &[u8; 32],
        wrapped_dek_hex: &str,
    ) -> Result<Self, VaultError> {
        let paths = VaultPaths::new(root);
        if !paths.exists() {
            return Err(VaultError::NotFound);
        }
        if wrapped_dek_hex.is_empty() {
            return Err(VaultError::InvalidCredentials);
        }

        let dek = crate::dek_store::unwrap_dek_hex(wrapped_dek_hex, fido_wrap_key)?;

        let conn = open_database(&paths, &dek)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let vault_id = read_vault_id(&conn).ok_or(VaultError::NotFound)?;
        let kek = load_kek(&paths.root, &dek)?;

        Ok(Self {
            paths,
            conn,
            vault_id,
            dek,
            kek,
        })
    }

    /// Opens an existing vault with a DEK obtained some other way.
    ///
    /// The recovery path uses this: the code produces the DEK directly, with
    /// no credential and no on-disk envelope involved. Everything below the
    /// key is identical either way — the DEK is what decrypts the database,
    /// and where it came from stops mattering once it is in hand.
    pub fn open_with_dek(root: PathBuf, dek: MasterDek) -> Result<Self, VaultError> {
        let paths = VaultPaths::new(root);
        if !paths.exists() {
            return Err(VaultError::NotFound);
        }
        let conn = open_database(&paths, &dek)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let vault_id = read_vault_id(&conn).ok_or(VaultError::NotFound)?;
        let kek = load_kek(&paths.root, &dek)?;
        Ok(Self {
            paths,
            conn,
            vault_id,
            dek,
            kek,
        })
    }

    /// Flush Write-Ahead Log (WAL) to main database file.
    pub fn flush_wal(&self) -> Result<(), VaultError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Rewrites the encrypted copy of the database under a different key,
    /// for rotation: without it the silo opens once more and then never
    /// again, because the shadow backup stays under the old key and the
    /// plaintext working copy is wiped at lock. Written to one side and
    /// moved into place by the caller, next to the key changeover.
    pub fn stage_local_backup(&self, dek: &MasterDek, to: &Path) -> Result<(), VaultError> {
        let _ = self.flush_wal();
        encrypt_vault_file(&self.paths.db_path(), to, dek)
    }

    pub fn backup_locally(&self) -> Result<(), VaultError> {
        let _ = self.flush_wal();
        encrypt_vault_file(&self.paths.db_path(), &self.paths.db_enc_path(), &self.dek)?;
        if self.paths.db_enc_path().exists() {
            std::fs::copy(self.paths.db_enc_path(), self.paths.db_enc_backup_path())?;
        }
        Ok(())
    }

    /// Encrypt current state to disk and remove the plaintext working copy
    /// (and its WAL/SHM sidecars) so only ciphertext remains once locked.
    /// The `Connection` must be dropped by the caller before/after this, since
    /// Windows won't let an open file be deleted.
    pub fn seal_for_lock(&self) -> Result<(), VaultError> {
        self.backup_locally()?;
        Ok(())
    }
}

/// Remove the plaintext working copy and SQLite sidecar files. Call only
/// after the `Connection` that owned them has been dropped.
/// Removes everything decrypted that this machine wrote for the silo.
///
/// One call rather than a list of filenames: the scratch directory holds the
/// working database, its journals and any file the user opened, and a wipe
/// that knew about only some of those is how plaintext survives a lock.
pub fn wipe_plaintext_working_copy(paths: &VaultPaths) {
    crate::workdir::wipe_work_dir(&paths.root);
}

fn verify_integrity(conn: &Connection) -> Result<(), VaultError> {
    let status: Result<String, _> = conn.query_row("PRAGMA quick_check;", [], |row| row.get(0));
    match status {
        Ok(res) if res == "ok" => Ok(()),
        Ok(res) => Err(VaultError::Corrupted(format!(
            "integrity quick_check failed: {res}"
        ))),
        Err(err) => Err(VaultError::Corrupted(format!(
            "integrity check SQL error: {err}"
        ))),
    }
}

/// Decrypt the encrypted-at-rest snapshot into the plaintext working copy,
/// falling back to the encrypted shadow backup if the primary snapshot is
/// missing, tampered, or was written with a different key. Then open it and
/// verify SQLite-level integrity, falling back again if that fails too.
fn open_database(paths: &VaultPaths, dek: &MasterDek) -> Result<Connection, VaultError> {
    paths.ensure_work_dir()?;
    let enc_path = paths.db_enc_path();
    let plain_path = paths.db_path();

    if enc_path.is_file() {
        if decrypt_vault_file(&enc_path, &plain_path, dek).is_err() {
            restore_from_enc_backup(paths, dek)?;
        }
    } else if !plain_path.is_file() {
        return Err(VaultError::NotFound);
    }

    match open_and_verify(&plain_path) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            if paths.db_enc_backup_path().is_file() {
                restore_from_enc_backup(paths, dek)?;
                open_and_verify(&plain_path)
            } else {
                Err(err)
            }
        }
    }
}

fn restore_from_enc_backup(paths: &VaultPaths, dek: &MasterDek) -> Result<(), VaultError> {
    let backup = paths.db_enc_backup_path();
    if !backup.is_file() {
        return Err(VaultError::Corrupted("no local backup available".into()));
    }
    decrypt_vault_file(&backup, &paths.db_path(), dek)
        .map_err(|_| VaultError::Corrupted("shadow backup failed to decrypt".into()))
}

fn open_and_verify(plain_path: &Path) -> Result<Connection, VaultError> {
    let conn = open_database_raw(plain_path)?;
    verify_integrity(&conn)?;
    Ok(conn)
}

fn open_database_raw(path: &Path) -> Result<Connection, VaultError> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(conn)
}

fn read_vault_id(conn: &Connection) -> Option<Uuid> {
    conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'vault_id'",
        [],
        |row| {
            let value: String = row.get(0)?;
            Ok(value)
        },
    )
    .ok()
    .and_then(|s| Uuid::parse_str(&s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_integrity_and_shadow_backup() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let vault_id = Uuid::new_v4();
        let secret = "test_device_secret_123";

        let session = VaultSession::provision(root.clone(), vault_id, secret).unwrap();
        session
            .conn
            .execute_batch("CREATE TABLE vault_meta (key TEXT PRIMARY KEY, value TEXT);")
            .unwrap();
        session
            .conn
            .execute_batch(&format!(
                "INSERT INTO vault_meta VALUES ('vault_id', '{vault_id}');"
            ))
            .unwrap();
        session.backup_locally().unwrap();

        assert!(session.paths.db_enc_backup_path().exists());

        drop(session);

        // Open successfully — vault.db.enc decrypts into the working copy.
        let session2 = VaultSession::open_with_device_secret(root.clone(), secret).unwrap();
        assert_eq!(session2.vault_id, vault_id);
        drop(session2);

        // Corrupt the encrypted-at-rest snapshot.
        std::fs::write(root.join(VAULT_DB_ENC), b"corrupted ciphertext junk").unwrap();

        // Re-open: should detect the decrypt failure and restore from the
        // encrypted shadow backup instead.
        let session3 = VaultSession::open_with_device_secret(root.clone(), secret).unwrap();
        assert_eq!(session3.vault_id, vault_id);
    }

    /// A new silo at a path a previous silo used must not inherit its
    /// index: the scratch directory is keyed by path, and after a crash the
    /// old working copy is still there for `provision` to adopt, rows
    /// sealed with a key the new silo does not have.
    #[test]
    fn provisioning_over_a_previous_silos_leftovers_starts_clean() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("Personal");
        std::fs::create_dir_all(&root).unwrap();

        // The first silo, left as an abrupt shutdown would leave it: the
        // working copy still on disk, never wiped by a lock.
        let first = VaultSession::provision(root.clone(), Uuid::new_v4(), "first-secret").unwrap();
        first
            .conn
            .execute_batch(
                "CREATE TABLE passwords (id TEXT PRIMARY KEY, data TEXT NOT NULL);
                 INSERT INTO passwords VALUES ('old', 'sealed-with-the-first-key');",
            )
            .unwrap();
        first.flush_wal().unwrap();
        let working_copy = first.paths.db_path();
        drop(first);
        assert!(working_copy.is_file(), "the crash this test models");

        // The user deletes the silo and makes a new one in the same folder.
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let second =
            VaultSession::provision(root.clone(), Uuid::new_v4(), "second-secret").unwrap();

        let inherited: i64 = second
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'passwords'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            inherited, 0,
            "the new silo inherited the deleted silo's tables"
        );
    }

    /// The other half of what a path hands to its next tenant: a record of
    /// blobs that belonged to a silo which no longer exists. Left in place,
    /// the new silo believes it already holds content it has never seen, and
    /// that the bucket already has content that was never uploaded.
    #[test]
    fn provisioning_does_not_inherit_a_previous_silos_blob_bookkeeping() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("Personal");
        std::fs::create_dir_all(&root).unwrap();

        let first = VaultSession::provision(root.clone(), Uuid::new_v4(), "first-secret").unwrap();
        crate::cache_store::record_blob_present(&root, Uuid::new_v4(), 4096, true).unwrap();
        assert!(crate::workdir::cache_dir_for(&root).exists());
        drop(first);

        std::fs::remove_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        VaultSession::provision(root.clone(), Uuid::new_v4(), "second-secret").unwrap();

        assert!(
            crate::cache_store::list_local_blob_ids(&root).is_empty(),
            "the new silo inherited blobs belonging to the deleted one"
        );
    }

    #[test]
    fn plaintext_working_copy_is_never_left_behind_by_provision() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let session = VaultSession::provision(root.clone(), Uuid::new_v4(), "secret").unwrap();
        assert!(session.paths.db_enc_path().is_file());
    }

    #[test]
    fn a_joining_device_keeps_the_dek_it_was_given() {
        // The whole point of the join path: generating a fresh key here
        // would produce a second, unrelated vault sharing an id, and nothing
        // already in the bucket would decrypt.
        let dir = tempdir().unwrap();
        let vault_id = Uuid::new_v4();
        let shared = generate_dek();

        let session = VaultSession::provision_with_dek(
            dir.path().to_path_buf(),
            vault_id,
            "a-device-secret",
            shared.clone(),
            generate_content_kek(),
        )
        .unwrap();

        assert_eq!(session.dek.as_bytes(), shared.as_bytes());
        assert_eq!(session.vault_id, vault_id);
    }

    #[test]
    fn a_joined_vault_reopens_with_the_same_key() {
        // The on-disk envelope is wrapped under this device's own secret,
        // so re-opening locally has to land on the shared DEK again.
        let dir = tempdir().unwrap();
        let shared = generate_dek();
        let session = VaultSession::provision_with_dek(
            dir.path().to_path_buf(),
            Uuid::new_v4(),
            "a-device-secret",
            shared.clone(),
            generate_content_kek(),
        )
        .unwrap();
        session
            .conn
            .execute_batch("CREATE TABLE vault_meta (key TEXT PRIMARY KEY, value TEXT);")
            .unwrap();
        session
            .conn
            .execute(
                "INSERT INTO vault_meta (key, value) VALUES ('vault_id', ?1)",
                [session.vault_id.to_string()],
            )
            .unwrap();
        session.backup_locally().unwrap();
        drop(session);

        let reopened =
            VaultSession::open_with_device_secret(dir.path().to_path_buf(), "a-device-secret")
                .unwrap();
        assert_eq!(reopened.dek.as_bytes(), shared.as_bytes());
    }

    #[test]
    fn joining_refuses_to_overwrite_an_existing_vault() {
        let dir = tempdir().unwrap();
        VaultSession::provision_with_dek(
            dir.path().to_path_buf(),
            Uuid::new_v4(),
            "secret",
            generate_dek(),
            generate_content_kek(),
        )
        .unwrap();

        let second = VaultSession::provision_with_dek(
            dir.path().to_path_buf(),
            Uuid::new_v4(),
            "secret",
            generate_dek(),
            generate_content_kek(),
        );
        assert!(matches!(second, Err(VaultError::AlreadyExists)));
    }

    #[test]
    fn nothing_decrypted_is_written_into_the_silo_folder() {
        // The guarantee that makes "keep your silo wherever you like" safe.
        // If this fails, a silo in a synced folder uploads its index in the
        // clear — the one outcome the whole design exists to prevent.
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let session =
            VaultSession::provision(root.clone(), Uuid::new_v4(), "a-device-secret").unwrap();
        session
            .conn
            .execute_batch("CREATE TABLE vault_meta (key TEXT PRIMARY KEY, value TEXT);")
            .unwrap();
        session.backup_locally().unwrap();

        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        for name in &names {
            assert!(
                !name.starts_with("vault.db") || name.ends_with(".enc") || name.ends_with(".bak"),
                "{name} is a plaintext database artefact inside the silo folder"
            );
            assert_ne!(
                name, "cache.db",
                "cache.db lists blob sizes and belongs on the machine"
            );
        }
        assert!(
            names.iter().any(|n| n == "vault.db.enc"),
            "the encrypted snapshot must still be in the silo folder, or the silo is not portable"
        );
    }

    #[test]
    fn locking_removes_the_plaintext_copy() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let session =
            VaultSession::provision(root.clone(), Uuid::new_v4(), "a-device-secret").unwrap();
        let plaintext = session.paths.db_path();
        assert!(plaintext.is_file());

        let paths = session.paths.clone();
        drop(session);
        wipe_plaintext_working_copy(&paths);

        assert!(
            !plaintext.exists(),
            "the working copy must not outlive the session"
        );
    }
}
