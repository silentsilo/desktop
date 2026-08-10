//! Folders on this computer that the silo keeps a copy of: scanned at
//! unlock, imported one way, never mirrored. Not a live watcher (fights
//! auto-lock and Windows recursive watches), not two-way sync (the app must
//! not write into folders it does not own), and not a mirror (a file
//! deleted from the folder stays in the silo; this is an archive).
//!
//! Each imported file is remembered by size and modified time, so a
//! second scan skips what did not move: the same trade every backup tool
//! makes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// One folder being copied into the silo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedFolder {
    /// Where it is on this computer.
    pub path: PathBuf,
    /// Where its contents land inside the silo, as a vault path. Kept
    /// explicitly rather than derived from the folder name, so renaming the
    /// folder on disk does not silently start a second copy beside the first.
    pub target: String,
}

/// The list, as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedFolders {
    pub folders: Vec<ProtectedFolder>,
}

fn config_path(vault_root: &Path) -> PathBuf {
    vault_root.join("protected.json")
}

/// Saved beside the silo's other settings, and local to this machine: the
/// paths mean nothing on another computer, so this is one of the few things
/// that must not travel with the vault.
pub fn save_protected(vault_root: &Path, folders: &ProtectedFolders) -> Result<(), VaultError> {
    std::fs::write(config_path(vault_root), crate::format::encode(folders)?)?;
    Ok(())
}

/// An empty list for a silo that has never had one, which is not an error.
pub fn load_protected(vault_root: &Path) -> Result<ProtectedFolders, VaultError> {
    match std::fs::read(config_path(vault_root)) {
        Ok(bytes) => crate::format::decode("the protected folder list", &bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProtectedFolders::default()),
        Err(e) => Err(e.into()),
    }
}

/// What a file looked like when it was last imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    pub size: u64,
    /// Seconds since the epoch. Whole seconds because that is what every
    /// filesystem agrees on, and finer resolution would only make the
    /// comparison fail more often on copies that are actually identical.
    pub modified: i64,
}

/// One file the scan decided to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingImport {
    pub source: PathBuf,
    /// Folder inside the silo, as a vault path.
    pub target_folder: String,
    pub stat: FileStat,
}

/// Walks a protected folder and lists what has changed since last time. A
/// file whose size and modified time both match `seen` is skipped. Nothing
/// here deletes. Symbolic links are not followed: a link into a parent
/// walks forever, and one pointing outside the folder would copy in
/// something the user never marked.
pub fn plan_scan(
    root: &Path,
    target: &str,
    seen: &HashMap<PathBuf, FileStat>,
) -> Result<Vec<PendingImport>, VaultError> {
    let mut pending = Vec::new();
    let mut stack = vec![(root.to_path_buf(), target.to_string())];

    while let Some((dir, vault_dir)) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // A folder that has become unreadable (an unplugged drive, a
            // permission change) is skipped rather than failing the scan: the
            // other protected folders still deserve their pass.
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }

            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };

            if meta.is_dir() {
                let child = join_vault_path(&vault_dir, &name);
                stack.push((path, child));
                continue;
            }
            if !meta.is_file() {
                continue;
            }

            let stat = FileStat {
                size: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            };
            if seen.get(&path) == Some(&stat) {
                continue;
            }
            pending.push(PendingImport {
                source: path,
                target_folder: vault_dir.clone(),
                stat,
            });
        }
    }

    // Shallowest first, and stable, so a run that is interrupted resumes
    // somewhere predictable rather than in a different order every time.
    pending.sort_by(|a, b| {
        (a.target_folder.matches('/').count(), &a.source)
            .cmp(&(b.target_folder.matches('/').count(), &b.source))
    });
    Ok(pending)
}

fn join_vault_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// What this machine has already taken from its protected folders. A fact
/// about this disk, kept beside the blob cache rather than in the vault
/// index. Losing it costs one expensive scan that resolves to the same
/// content.
fn seen_db_path(vault_root: &Path) -> PathBuf {
    crate::workdir::cache_dir_for(vault_root).join("protected.db")
}

fn open_seen_db(vault_root: &Path) -> Result<rusqlite::Connection, VaultError> {
    crate::workdir::create_private_dir(&crate::workdir::cache_dir_for(vault_root))?;
    let conn = rusqlite::Connection::open(seen_db_path(vault_root))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS protected_seen (
            path     TEXT PRIMARY KEY,
            size     INTEGER NOT NULL,
            modified INTEGER NOT NULL
        );",
    )?;
    Ok(conn)
}

pub fn load_seen(vault_root: &Path) -> Result<HashMap<PathBuf, FileStat>, VaultError> {
    let conn = open_seen_db(vault_root)?;
    let mut stmt = conn.prepare("SELECT path, size, modified FROM protected_seen")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            PathBuf::from(row.get::<_, String>(0)?),
            FileStat {
                size: row.get::<_, i64>(1)? as u64,
                modified: row.get(2)?,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, stat) = row?;
        out.insert(path, stat);
    }
    Ok(out)
}

/// Records one file as taken, after it is in the silo and not before.
///
/// Written per file rather than per scan so an interrupted run keeps what it
/// managed. The other order, marking first, would skip a file that never
/// arrived and leave it missing until someone touched it again.
pub fn mark_seen(vault_root: &Path, path: &Path, stat: FileStat) -> Result<(), VaultError> {
    let conn = open_seen_db(vault_root)?;
    conn.execute(
        "INSERT INTO protected_seen(path, size, modified) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified",
        rusqlite::params![path.to_string_lossy(), stat.size as i64, stat.modified],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        path
    }

    fn stat_of(path: &Path) -> FileStat {
        let meta = std::fs::metadata(path).unwrap();
        FileStat {
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }

    #[test]
    fn the_first_scan_takes_everything() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes.txt", "one");
        write(dir.path(), "sub/deep.txt", "two");

        let plan = plan_scan(dir.path(), "/Protected", &HashMap::new()).unwrap();

        let targets: Vec<&str> = plan.iter().map(|p| p.target_folder.as_str()).collect();
        assert_eq!(plan.len(), 2);
        assert!(targets.contains(&"/Protected"));
        assert!(targets.contains(&"/Protected/sub"));
    }

    #[test]
    fn a_file_that_has_not_moved_is_skipped() {
        // The whole reason the scan is cheap the second time: encrypting a
        // file to find out whether it changed would mean re-encrypting the
        // tree on every unlock.
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "notes.txt", "one");
        let seen: HashMap<PathBuf, FileStat> =
            [(path.clone(), stat_of(&path))].into_iter().collect();

        assert!(
            plan_scan(dir.path(), "/Protected", &seen)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_file_whose_size_changed_is_taken_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "notes.txt", "one");
        let before = stat_of(&path);
        write(dir.path(), "notes.txt", "one and a bit more");
        let seen: HashMap<PathBuf, FileStat> = [(path.clone(), before)].into_iter().collect();

        let plan = plan_scan(dir.path(), "/Protected", &seen).unwrap();

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].source, path);
    }

    #[test]
    fn a_file_deleted_from_disk_is_not_removed_from_the_silo() {
        // An archive, not a mirror. The copy surviving what happens to the
        // original is the reason for putting a folder here at all.
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("deleted.txt");
        let seen: HashMap<PathBuf, FileStat> = [(
            gone,
            FileStat {
                size: 3,
                modified: 1,
            },
        )]
        .into_iter()
        .collect();

        let plan = plan_scan(dir.path(), "/Protected", &seen).unwrap();

        // Nothing to import, and nothing in the plan asking for a deletion:
        // there is no way to express one.
        assert!(plan.is_empty());
    }

    #[test]
    fn links_are_not_followed() {
        // A link into a parent walks forever; one pointing outside the folder
        // copies in something nobody marked.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "real.txt", "one");
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "secret.txt", "not yours");

        let link = dir.path().join("link");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(outside.path(), &link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(outside.path(), &link);
        if made.is_err() {
            // Windows needs developer mode or elevation for this. Said out
            // loud rather than passing on a link that was never created,
            // which would prove nothing at all.
            eprintln!("skipped: this machine will not create symbolic links");
            return;
        }

        let plan = plan_scan(dir.path(), "/Protected", &HashMap::new()).unwrap();

        assert!(
            plan.iter().all(|p| !p.source.ends_with("secret.txt")),
            "the scan followed a link out of the folder: {plan:?}"
        );
    }

    #[test]
    fn the_list_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let folders = ProtectedFolders {
            folders: vec![ProtectedFolder {
                path: PathBuf::from("C:/Users/alex/Documents/Taxes"),
                target: "/Protected/Taxes".into(),
            }],
        };

        save_protected(dir.path(), &folders).unwrap();

        assert_eq!(load_protected(dir.path()).unwrap(), folders);
    }

    #[test]
    fn a_silo_with_no_list_reports_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            load_protected(dir.path()).unwrap(),
            ProtectedFolders::default()
        );
    }
}
