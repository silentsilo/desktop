//! Reading a SilentSilo backup with nothing but a recovery code: a single
//! standalone binary, no account, no key, no SilentSilo install. The
//! recovery code alone unlocks, deliberately, because the situation this is
//! for is a machine that is gone and a sheet of paper that survived. It
//! reuses the app's own replay rather than reimplementing it; a second
//! interpretation of the log is a second thing that can be wrong.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use silentsilo_store::ObjectStore;
use uuid::Uuid;

/// One file as it exists in the backup.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Path inside the silo, always with forward slashes.
    pub path: String,
    pub blob_id: Uuid,
    pub size_bytes: i64,
    /// Wrapped under the content KEK. The extractor unwraps it per file.
    pub blob_key: String,
    /// In the trash but not yet purged. Written out under `_trash/` rather
    /// than skipped: someone reaching for this tool is recovering, and a
    /// deleted file they had not finished deleting is not ours to drop.
    pub trashed: bool,
}

/// What a backup turned out to hold.
pub struct Backup {
    pub vault_id: Uuid,
    pub entries: Vec<Entry>,
    /// Every password entry, decrypted. Someone reaching for this tool has
    /// lost the machine, and the logins are as much theirs as the files.
    pub passwords: Vec<serde_json::Value>,
    /// Kept so extraction can open content without asking again. The vault
    /// key is not: it opened the records, and content is under keys of its
    /// own wrapped by this one.
    kek: silentsilo_crypto::ContentKek,
}

/// One file kept inside a password entry. It has no row in the tree; the
/// entry's JSON is its only reference.
pub struct Attachment {
    /// The entry it belongs to, for the folder it is written under.
    pub entry_title: String,
    pub name: String,
    pub blob_id: Uuid,
    pub blob_key: String,
}

impl Backup {
    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.trashed).count()
    }

    pub fn trashed_count(&self) -> usize {
        self.entries.iter().filter(|e| e.trashed).count()
    }

    pub fn password_count(&self) -> usize {
        self.passwords.len()
    }

    /// Every attachment across every entry.
    pub fn attachments(&self) -> Vec<Attachment> {
        let mut out = Vec::new();
        for entry in &self.passwords {
            let title = entry
                .get("service")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| entry.get("id").and_then(|v| v.as_str()))
                .unwrap_or("entry")
                .to_string();
            let Some(attachments) = entry.get("attachments").and_then(|a| a.as_array()) else {
                continue;
            };
            for attachment in attachments {
                let Some(blob_id) = attachment
                    .get("blob_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                else {
                    continue;
                };
                out.push(Attachment {
                    entry_title: title.clone(),
                    name: attachment
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("attachment")
                        .to_string(),
                    blob_id,
                    blob_key: attachment
                        .get("blob_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        out
    }
}

/// The password entries as a CSV any other manager imports: the same
/// Chrome-style columns the app's own export writes. Plaintext, which is the
/// point; the caller says so out loud. `None` when there are no entries.
pub fn passwords_csv(backup: &Backup) -> Option<String> {
    if backup.passwords.is_empty() {
        return None;
    }
    let field = |entry: &serde_json::Value, key: &str| -> String {
        let raw = entry.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        let escaped = raw.replace('"', "\"\"");
        format!("\"{escaped}\"")
    };
    let mut lines = vec!["name,url,username,password,totp,category,note".to_string()];
    for entry in &backup.passwords {
        lines.push(
            [
                field(entry, "service"),
                field(entry, "url"),
                field(entry, "username"),
                field(entry, "password"),
                field(entry, "totp_secret"),
                field(entry, "category"),
                field(entry, "notes"),
            ]
            .join(","),
        );
    }
    Some(format!("{}\n", lines.join("\n")))
}

#[derive(Debug)]
pub enum ExtractError {
    Storage(String),
    NoSilo,
    NoRecoveryCode,
    WrongCode,
    NoContentKey,
    Vault(String),
    Io(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "could not read the backup: {e}"),
            Self::NoSilo => write!(f, "there is no silo here: no vault.json was found"),
            Self::NoRecoveryCode => write!(
                f,
                "this backup has no recovery code published, so a code alone cannot open it"
            ),
            Self::WrongCode => write!(f, "that recovery code does not open this backup"),
            Self::NoContentKey => write!(
                f,
                "this backup has no content key, so its files cannot be opened"
            ),
            Self::Vault(e) => write!(f, "could not rebuild the file list: {e}"),
            Self::Io(e) => write!(f, "could not write the files out: {e}"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Opens a backup and rebuilds its file list, without writing anything.
///
/// Everything happens in a temporary database that is thrown away with the
/// call: reading a backup must not leave a silo behind on the machine doing
/// the reading, which may well not be the owner's.
pub async fn open(store: &dyn ObjectStore, code: &str) -> Result<Backup, ExtractError> {
    let manifest = silentsilo_sync::read_manifest(store)
        .await
        .map_err(|e| ExtractError::Storage(e.to_string()))?
        .ok_or(ExtractError::NoSilo)?;

    let envelope = silentsilo_sync::fetch_recovery_envelope(store)
        .await
        .map_err(|e| ExtractError::Storage(e.to_string()))?
        .ok_or(ExtractError::NoRecoveryCode)?;

    let dek =
        silentsilo_vault::unwrap_with_code(&envelope, code).map_err(|_| ExtractError::WrongCode)?;

    let kek_bytes = silentsilo_sync::fetch_content_kek(store)
        .await
        .map_err(|e| ExtractError::Storage(e.to_string()))?
        .ok_or(ExtractError::NoContentKey)?;
    let kek = silentsilo_vault::unwrap_kek_bytes(&kek_bytes, &dek)
        .map_err(|_| ExtractError::NoContentKey)?;

    // A compacted backup starts from its snapshot: the records below the
    // horizon are gone from storage, so replaying what is left onto an empty
    // tree would rebuild only the tail of the history.
    let snapshot = silentsilo_sync::latest_snapshot(store, &dek)
        .await
        .map_err(|e| ExtractError::Storage(e.to_string()))?;
    // Only what the snapshot does not already cover: a record below the
    // horizon must never be applied on top of it.
    let horizon = snapshot.as_ref().map(|s| s.horizon).unwrap_or(0);
    let records = silentsilo_sync::fetch_all_ops_above(store, &dek, horizon)
        .await
        .map_err(|e| ExtractError::Storage(e.to_string()))?;

    let conn =
        rusqlite::Connection::open_in_memory().map_err(|e| ExtractError::Vault(e.to_string()))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e| ExtractError::Vault(e.to_string()))?;
    silentsilo_vfs::init_schema(&conn, manifest.vault_id)
        .map_err(|e| ExtractError::Vault(e.to_string()))?;
    if let Some(snapshot) = snapshot {
        silentsilo_vfs::snapshot::restore(&conn, &snapshot)
            .map_err(|e| ExtractError::Vault(e.to_string()))?;
    }
    silentsilo_vfs::replay(&conn, records).map_err(|e| ExtractError::Vault(e.to_string()))?;

    Ok(Backup {
        vault_id: manifest.vault_id,
        entries: read_entries(&conn).map_err(|e| ExtractError::Vault(e.to_string()))?,
        passwords: read_passwords(&conn, &kek)?,
        kek,
    })
}

/// Decrypts every password row with the content KEK, the same key the app
/// itself uses. A row that will not open is a damaged backup, reported
/// rather than silently shortened: this is a password manager, and a
/// shorter list with nothing said is the worst answer it can give.
fn read_passwords(
    conn: &rusqlite::Connection,
    kek: &silentsilo_crypto::ContentKek,
) -> Result<Vec<serde_json::Value>, ExtractError> {
    use base64::Engine as _;
    let vault = |e: rusqlite::Error| ExtractError::Vault(e.to_string());

    let mut stmt = conn
        .prepare("SELECT data FROM passwords ORDER BY updated_at DESC")
        .map_err(vault)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(vault)?;

    let mut out = Vec::new();
    for row in rows {
        let sealed_b64 = row.map_err(vault)?;
        let sealed = base64::engine::general_purpose::STANDARD
            .decode(&sealed_b64)
            .map_err(|e| ExtractError::Vault(format!("a password entry is not readable: {e}")))?;
        let plain = silentsilo_crypto::unseal_with_key(&sealed, kek.as_bytes())
            .map_err(|_| ExtractError::Vault("a password entry would not decrypt".into()))?;
        let value: serde_json::Value = serde_json::from_slice(&plain)
            .map_err(|e| ExtractError::Vault(format!("a password entry is not readable: {e}")))?;
        out.push(value);
    }
    Ok(out)
}

fn read_entries(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<Entry>> {
    let mut stmt = conn.prepare(
        "SELECT fo.path, f.name, f.blob_id, f.size_bytes,
                COALESCE(f.blob_key, ''), f.deleted_at IS NOT NULL
           FROM files f JOIN folders fo ON fo.id = f.folder_id
          ORDER BY fo.path, f.name",
    )?;
    let rows = stmt.query_map([], |row| {
        let folder: String = row.get(0)?;
        let name: String = row.get(1)?;
        let blob: String = row.get(2)?;
        Ok(Entry {
            // Relative to the silo root, with no leading slash: this is
            // joined onto a destination directory, and a leading separator
            // would make it an absolute path on the way out.
            path: if folder == "/" {
                name
            } else {
                format!("{}/{}", folder.trim_matches('/'), name)
            },
            blob_id: Uuid::parse_str(&blob).unwrap_or(Uuid::nil()),
            size_bytes: row.get(3)?,
            blob_key: row.get(4)?,
            trashed: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// What an extraction managed.
#[derive(Debug, Default)]
pub struct Extracted {
    pub written: usize,
    /// Files that could not be written, with why. Reported rather than
    /// raised: one unreadable file must not abandon the rest, which is the
    /// whole reason someone is running this.
    pub failed: Vec<(String, String)>,
}

/// Writes every file out under `dest`, rebuilding the folder structure.
/// Password entries land beside them, in `_passwords/passwords.csv`, in
/// plaintext, with their attachments under `_passwords/attachments/`: the
/// situation this runs in is a machine that is gone, and the logins are as
/// much the owner's as the files.
pub async fn extract(
    backup: &Backup,
    store: &dyn ObjectStore,
    dest: &Path,
    progress: &mut (dyn FnMut(usize, usize, &str) + Send),
) -> Result<Extracted, ExtractError> {
    let mut out = Extracted::default();
    let attachments = backup.attachments();
    let total =
        backup.entries.len() + attachments.len() + usize::from(!backup.passwords.is_empty());
    let mut done = 0;

    for entry in &backup.entries {
        progress(done, total, &entry.path);
        done += 1;

        // Trashed content goes to one side rather than being mixed in with
        // what the owner still has, and rather than being dropped.
        let relative = if entry.trashed {
            format!("_trash/{}", entry.path)
        } else {
            entry.path.clone()
        };
        let Some(target) = safe_join(dest, &relative) else {
            out.failed
                .push((entry.path.clone(), "unsafe path in the backup".into()));
            continue;
        };

        if let Err(e) = write_one(backup, store, entry, &target).await {
            out.failed.push((entry.path.clone(), e));
            continue;
        }
        out.written += 1;
    }

    if let Some(csv) = passwords_csv(backup) {
        progress(done, total, "_passwords/passwords.csv");
        done += 1;
        let dir = dest.join("_passwords");
        let write = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join("passwords.csv"), csv));
        match write {
            Ok(()) => out.written += 1,
            Err(e) => out
                .failed
                .push(("_passwords/passwords.csv".into(), e.to_string())),
        }
    }

    for attachment in &attachments {
        progress(done, total, &attachment.name);
        done += 1;
        if let Err(e) = write_attachment(backup, store, attachment, dest).await {
            out.failed
                .push((format!("attachment {}", attachment.name), e));
            continue;
        }
        out.written += 1;
    }

    progress(total, total, "");
    Ok(out)
}

/// One attachment, under `_passwords/attachments/<entry>/<name>`. The names
/// come from entry JSON, which is data: joined through the same guard as
/// every file path, with the entry id as the fallback folder.
async fn write_attachment(
    backup: &Backup,
    store: &dyn ObjectStore,
    attachment: &Attachment,
    dest: &Path,
) -> Result<(), String> {
    let base = dest.join("_passwords").join("attachments");
    let folder = safe_join(&base, &attachment.entry_title)
        .unwrap_or_else(|| base.join(attachment.blob_id.to_string()));
    let target = safe_join(&folder, &attachment.name)
        .ok_or_else(|| "unsafe attachment name in the backup".to_string())?;

    let entry = Entry {
        path: attachment.name.clone(),
        blob_id: attachment.blob_id,
        size_bytes: 0,
        blob_key: attachment.blob_key.clone(),
        trashed: false,
    };
    write_one(backup, store, &entry, &target).await
}

async fn write_one(
    backup: &Backup,
    store: &dyn ObjectStore,
    entry: &Entry,
    target: &Path,
) -> Result<(), String> {
    if entry.blob_key.is_empty() {
        return Err("no content key is recorded for this file".into());
    }
    let key = silentsilo_crypto::unwrap_content_key(&entry.blob_key, &backup.kek)
        .map_err(|e| e.to_string())?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // Streamed through a temporary file, because `decrypt_blob` reads from a
    // path and a blob is whatever size the user stored: holding a ten
    // gigabyte file in memory would be a poor way to recover it.
    let staging = tempfile::Builder::new()
        .prefix("silentsilo-extract")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let sealed_path = staging.path().join("blob.sslo");
    silentsilo_sync::fetch_blob_to_file(store, entry.blob_id, &sealed_path)
        .await
        .map_err(|e| e.to_string())?;

    silentsilo_crypto::decrypt_blob(&sealed_path, target, &key, entry.blob_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Joins a path from the backup onto the destination, refusing anything that
/// would land outside it.
///
/// Names come from the silo, which is data rather than input this tool
/// controls. A backup written by a hostile device could carry a name shaped
/// to walk out of the destination directory, and a recovery tool is the worst
/// possible place to find that out.
fn safe_join(dest: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = dest.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        if part.contains('\\') || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    out.starts_with(dest).then_some(out)
}

/// The tree as text, for the listing command.
pub fn describe(backup: &Backup) -> Vec<String> {
    backup
        .entries
        .iter()
        .map(|e| {
            format!(
                "{:>12}  {}{}",
                e.size_bytes,
                e.path,
                if e.trashed { "   (in the trash)" } else { "" }
            )
        })
        .collect()
}

/// Every folder in the backup, including empty ones, so a listing shows the
/// shape of the silo and not only the files in it.
pub fn folders(backup: &Backup) -> Vec<String> {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for entry in &backup.entries {
        if let Some((folder, _)) = entry.path.rsplit_once('/') {
            seen.insert(folder, ());
        }
    }
    let mut out: Vec<String> = seen.keys().map(|s| s.to_string()).collect();
    out.sort();
    out
}

/// Opens a folder as a backup. The commonest case by far: an external disk,
/// a synced folder, or a bucket someone mirrored down with rclone.
pub fn folder_store(path: &Path) -> Box<dyn ObjectStore> {
    Box::new(silentsilo_store::FolderStore::new(path.to_path_buf()))
}

#[cfg(test)]
mod safe_join_tests {
    use super::safe_join;
    use std::path::Path;

    /// A backup is data, and the tool that reads it is what someone runs
    /// when everything else is gone. A name shaped to walk out of the
    /// destination must not be able to write anywhere on that machine.
    #[test]
    fn a_path_that_leaves_the_destination_is_refused() {
        let dest = Path::new("/home/alex/recovered");
        for bad in [
            "..",
            ".",
            "",
            "../etc/passwd",
            "docs/../../escape.txt",
            "C:evil",
            "sub/./x",
        ] {
            assert!(safe_join(dest, bad).is_none(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn a_path_inside_the_backup_is_rebuilt_under_the_destination() {
        let dest = Path::new("/home/alex/recovered");
        let joined = safe_join(dest, "Acte/2026/contract.pdf").expect("an ordinary path");
        assert!(joined.starts_with(dest));
        assert!(joined.ends_with("Acte/2026/contract.pdf"));
    }
}
