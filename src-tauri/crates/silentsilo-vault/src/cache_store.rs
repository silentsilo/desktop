//! Local blob cache bookkeeping — separate from `vault.db`, never synced to the cloud.
//!
//! Tracks which encrypted blobs currently sit on local disk, when they were last
//! accessed, and whether they're already backed up to the cloud (only synced blobs
//! are ever evicted, so a blob pending its first upload is never at risk of loss).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::VaultError;

const CACHE_DB: &str = "cache.db";
const BLOBS_DIR: &str = "blobs";
const CACHE_SETTINGS_FILE: &str = "cache_settings.json";

/// No ceiling unless the user sets one. The cache is already bounded by
/// what this person opens, and a default ceiling would evict exactly the
/// files they used most recently.
pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = u64::MAX;

/// Kept beside the working copy rather than in the silo, for the same
/// reason: it lists how many blobs there are and how big they are, which is
/// not content but is not nothing either.
///
/// Unlike the working copy this survives a lock — it is this machine's
/// record of what has already reached the bucket, and throwing it away
/// would make the next sync re-check every blob.
fn cache_db_path(vault_root: &Path) -> PathBuf {
    crate::workdir::cache_dir_for(vault_root).join(CACHE_DB)
}

fn blobs_dir(vault_root: &Path) -> PathBuf {
    vault_root.join(BLOBS_DIR)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn open_cache_db(vault_root: &Path) -> Result<Connection, VaultError> {
    crate::workdir::create_private_dir(&crate::workdir::cache_dir_for(vault_root))?;
    let conn = Connection::open(cache_db_path(vault_root))?;
    // Parallel uploads (mapPool in the frontend) open this file from multiple
    // threads at once — without a busy timeout a concurrent writer gets an
    // immediate "database is locked" instead of waiting its turn.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS blob_cache (
            blob_id TEXT PRIMARY KEY,
            size_bytes INTEGER NOT NULL,
            last_accessed_at INTEGER NOT NULL,
            synced INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS cache_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        -- Which target holds which blob. `synced` above answers the
        -- narrower question of whether the blob may be evicted, so it can
        -- only mean \"every target has it\"; a target added later needs to
        -- be told what it is missing, which is what this records.
        CREATE TABLE IF NOT EXISTS blob_delivery (
            target  TEXT NOT NULL,
            blob_id TEXT NOT NULL,
            PRIMARY KEY (target, blob_id)
        );
        CREATE INDEX IF NOT EXISTS idx_blob_delivery_blob ON blob_delivery(blob_id);",
    )?;
    Ok(conn)
}

/// Record that a blob now exists on local disk (freshly imported or downloaded).
/// `synced` marks it as already backed up (safe to evict later if space is needed).
pub fn record_blob_present(
    vault_root: &Path,
    blob_id: Uuid,
    size_bytes: i64,
    synced: bool,
) -> Result<(), VaultError> {
    let conn = open_cache_db(vault_root)?;
    conn.execute(
        "INSERT INTO blob_cache (blob_id, size_bytes, last_accessed_at, synced)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(blob_id) DO UPDATE SET
            size_bytes = excluded.size_bytes,
            last_accessed_at = excluded.last_accessed_at,
            synced = max(synced, excluded.synced)",
        params![blob_id.to_string(), size_bytes, now(), synced as i64],
    )?;
    Ok(())
}

/// Records that one target now holds this blob.
pub fn record_blob_delivered(
    vault_root: &Path,
    blob_id: Uuid,
    target: Uuid,
) -> Result<(), VaultError> {
    let conn = open_cache_db(vault_root)?;
    conn.execute(
        "INSERT OR IGNORE INTO blob_delivery(target, blob_id) VALUES (?1, ?2)",
        params![target.to_string(), blob_id.to_string()],
    )?;
    Ok(())
}

/// Marks as synced every blob that all `targets` hold, and forgets rows for
/// targets that are no longer configured. `synced` gates eviction, so it has
/// to mean every copy has it, not merely one.
pub fn settle_blob_delivery(vault_root: &Path, targets: &[Uuid]) -> Result<(), VaultError> {
    let conn = open_cache_db(vault_root)?;

    if targets.is_empty() {
        // No copy is configured, so nothing is backed up anywhere and
        // nothing may be evicted. Recomputed rather than left alone: a silo
        // whose only target was removed would otherwise keep blobs marked
        // evictable on the strength of a copy it can no longer reach.
        conn.execute("DELETE FROM blob_delivery", [])?;
        conn.execute("UPDATE blob_cache SET synced = 0", [])?;
        return Ok(());
    }

    let list: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
    let placeholders = vec!["?"; list.len()].join(",");

    conn.execute(
        &format!("DELETE FROM blob_delivery WHERE target NOT IN ({placeholders})"),
        rusqlite::params_from_iter(list.iter()),
    )?;
    // Both directions. Only ever setting it would leave a blob evictable
    // after the copy that held it was removed, which is how a local file
    // gets thrown away while no backup has it.
    conn.execute(
        &format!(
            "UPDATE blob_cache SET synced = CASE
                WHEN (SELECT COUNT(*) FROM blob_delivery d
                      WHERE d.blob_id = blob_cache.blob_id) = {} THEN 1
                ELSE 0 END",
            list.len()
        ),
        [],
    )?;
    Ok(())
}

/// Update the last-accessed time for a blob that was just read (opened/exported).
pub fn touch_blob_access(vault_root: &Path, blob_id: Uuid) -> Result<(), VaultError> {
    let conn = open_cache_db(vault_root)?;
    conn.execute(
        "UPDATE blob_cache SET last_accessed_at = ?2 WHERE blob_id = ?1",
        params![blob_id.to_string(), now()],
    )?;
    Ok(())
}

/// Blob ids currently present as encrypted `.sslo` files on local disk.
/// Filesystem is the ground truth here, not the bookkeeping table.
pub fn list_local_blob_ids(vault_root: &Path) -> Vec<Uuid> {
    let Ok(entries) = std::fs::read_dir(blobs_dir(vault_root)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            Uuid::parse_str(name.strip_suffix(".sslo")?).ok()
        })
        .collect()
}

/// Blobs that not every copy holds, for the counters on screen and for
/// eviction. `synced` answers "every configured target has it", which is
/// the only safe basis for throwing the local file away.
pub fn list_unsynced_blob_ids(vault_root: &Path) -> Vec<Uuid> {
    let Ok(conn) = open_cache_db(vault_root) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare("SELECT blob_id FROM blob_cache WHERE synced = 0") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok())
        .filter_map(|id| Uuid::parse_str(&id).ok())
        .collect()
}

/// Blobs one particular target does not have yet, which is what a push to
/// that target owes.
pub fn list_undelivered_blob_ids(vault_root: &Path, target: Uuid) -> Vec<Uuid> {
    let Ok(conn) = open_cache_db(vault_root) else {
        return Vec::new();
    };
    // Not `synced = 0`: that flag says every target had it at some point,
    // which is not an answer about a target added since.
    let Ok(mut stmt) = conn.prepare(
        "SELECT blob_id FROM blob_cache b
         WHERE NOT EXISTS (
             SELECT 1 FROM blob_delivery d
             WHERE d.blob_id = b.blob_id AND d.target = ?1
         )",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([target.to_string()], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok())
        .filter_map(|id| Uuid::parse_str(&id).ok())
        .collect()
}

/// What this silo currently occupies on local disk, and how much of it is
/// still the only copy.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CacheUsage {
    /// Total size of the encrypted blobs held locally.
    pub local_bytes: u64,
    /// Of that, the part not yet confirmed uploaded. Eviction cannot touch
    /// it, so it is also the floor the cache limit cannot push below.
    pub unsynced_bytes: u64,
    pub blob_count: u64,
    pub unsynced_count: u64,
}

/// Measured from the blobs directory, not from the bookkeeping table: the
/// table can hold rows for blobs no longer on disk, and summing it would
/// report space that is not occupied. The table is consulted only for the
/// synced flag.
pub fn cache_usage(vault_root: &Path) -> CacheUsage {
    let Ok(entries) = std::fs::read_dir(blobs_dir(vault_root)) else {
        return CacheUsage::default();
    };

    let unsynced: std::collections::HashSet<Uuid> =
        list_unsynced_blob_ids(vault_root).into_iter().collect();

    let mut usage = CacheUsage::default();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".sslo"))
            .and_then(|n| Uuid::parse_str(n).ok())
        else {
            continue;
        };
        let Ok(size) = entry.metadata().map(|m| m.len()) else {
            continue;
        };
        usage.local_bytes += size;
        usage.blob_count += 1;
        if unsynced.contains(&id) {
            usage.unsynced_bytes += size;
            usage.unsynced_count += 1;
        }
    }
    usage
}

/// Evict the oldest-accessed, already-synced blobs from local disk until total
/// local blob size is back under `max_bytes`. Blobs not yet confirmed synced to
/// the cloud are never evicted, regardless of how old they are.
pub fn enforce_cache_limit(vault_root: &Path, max_bytes: u64) -> Result<Vec<Uuid>, VaultError> {
    // A device asked to hold a full copy is not a cache. Evicting from it
    // would quietly turn the copy back into an index while the setting still
    // claimed otherwise, which is the exact failure that makes counting
    // devices as copies dishonest.
    if keep_full_copy(vault_root) {
        return Ok(Vec::new());
    }

    let conn = open_cache_db(vault_root)?;

    let rows: Vec<(String, i64, bool)> = {
        let mut stmt = conn.prepare(
            "SELECT blob_id, size_bytes, synced FROM blob_cache ORDER BY last_accessed_at ASC",
        )?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        })?
        .collect::<Result<_, _>>()?
    };

    let total: i64 = rows.iter().map(|(_, size, _)| *size).sum();
    if total < 0 || (total as u64) <= max_bytes {
        return Ok(Vec::new());
    }

    let dir = blobs_dir(vault_root);
    let mut remaining = total as u64;
    let mut evicted = Vec::new();

    for (blob_id_str, size, synced) in rows {
        if remaining <= max_bytes {
            break;
        }
        if !synced {
            continue;
        }
        let Ok(blob_id) = Uuid::parse_str(&blob_id_str) else {
            continue;
        };
        let path = dir.join(format!("{blob_id}.sslo"));
        if std::fs::remove_file(&path).is_ok() {
            conn.execute(
                "DELETE FROM blob_cache WHERE blob_id = ?1",
                params![blob_id_str],
            )?;
            remaining = remaining.saturating_sub(size.max(0) as u64);
            evicted.push(blob_id);
        }
    }

    Ok(evicted)
}

/// Remove a blob's local cache file and bookkeeping row outright — used when
/// a file has been permanently deleted (trash emptied) and its blob no
/// longer belongs anywhere, unlike `enforce_cache_limit` which only evicts
/// already-synced blobs to free up space.
pub fn remove_blob_from_cache(vault_root: &Path, blob_id: Uuid) -> Result<(), VaultError> {
    let path = blobs_dir(vault_root).join(format!("{blob_id}.sslo"));
    let _ = std::fs::remove_file(&path);
    let conn = open_cache_db(vault_root)?;
    conn.execute(
        "DELETE FROM blob_cache WHERE blob_id = ?1",
        params![blob_id.to_string()],
    )?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheSettings {
    max_cache_bytes: u64,
}

/// Configured local cache size limit, or `DEFAULT_CACHE_LIMIT_BYTES`
/// (unlimited) if unset.
pub fn get_cache_limit_bytes(app_data_dir: &Path) -> u64 {
    std::fs::read_to_string(app_data_dir.join(CACHE_SETTINGS_FILE))
        .ok()
        .and_then(|json| serde_json::from_str::<CacheSettings>(&json).ok())
        .map(|s| s.max_cache_bytes)
        .unwrap_or(DEFAULT_CACHE_LIMIT_BYTES)
}

pub fn set_cache_limit_bytes(app_data_dir: &Path, max_bytes: u64) -> Result<(), VaultError> {
    let settings = CacheSettings {
        max_cache_bytes: max_bytes,
    };
    let json =
        serde_json::to_string_pretty(&settings).map_err(|e| VaultError::Crypto(e.to_string()))?;
    crate::workdir::write_private(&app_data_dir.join(CACHE_SETTINGS_FILE), json.as_bytes())?;
    Ok(())
}

// ── Keeping a full copy on this device ──────────────────────────────

/// Whether this machine is meant to hold every blob the silo references.
/// Off, this device is an index; on, it is a real copy and "three copies"
/// means what people think it means. Per silo and per machine: a statement
/// about this disk, so it does not travel with the vault.
pub fn keep_full_copy(vault_root: &Path) -> bool {
    let Ok(conn) = open_cache_db(vault_root) else {
        return false;
    };
    conn.query_row(
        "SELECT value FROM cache_meta WHERE key = 'keep_full_copy'",
        [],
        |row| row.get::<_, String>(0),
    )
    .map(|value| value == "1")
    .unwrap_or(false)
}

pub fn set_keep_full_copy(vault_root: &Path, on: bool) -> Result<(), VaultError> {
    let conn = open_cache_db(vault_root)?;
    conn.execute(
        "INSERT INTO cache_meta(key, value) VALUES ('keep_full_copy', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if on { "1" } else { "0" }],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn set_accessed_at(root: &Path, blob_id: Uuid, at: i64) {
        let conn = Connection::open(cache_db_path(root)).unwrap();
        conn.execute(
            "UPDATE blob_cache SET last_accessed_at = ?2 WHERE blob_id = ?1",
            params![blob_id.to_string(), at],
        )
        .unwrap();
    }

    #[test]
    fn eviction_targets_oldest_synced_blob_and_spares_unsynced() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(blobs_dir(&root)).unwrap();

        let old_synced = Uuid::new_v4();
        let new_synced = Uuid::new_v4();
        let unsynced = Uuid::new_v4();

        for (id, size) in [
            (old_synced, 5_000usize),
            (new_synced, 4_000),
            (unsynced, 3_000),
        ] {
            std::fs::write(blobs_dir(&root).join(format!("{id}.sslo")), vec![0u8; size]).unwrap();
        }

        record_blob_present(&root, old_synced, 5_000, true).unwrap();
        record_blob_present(&root, new_synced, 4_000, true).unwrap();
        record_blob_present(&root, unsynced, 3_000, false).unwrap();

        // unsynced is the oldest-accessed of all, but must never be evicted.
        set_accessed_at(&root, unsynced, 500);
        set_accessed_at(&root, old_synced, 1_000);
        set_accessed_at(&root, new_synced, 2_000);

        // Total is 12,000 bytes; an 8,000 byte budget only requires evicting one blob.
        let evicted = enforce_cache_limit(&root, 8_000).unwrap();

        assert_eq!(evicted, vec![old_synced]);
        assert!(!blobs_dir(&root).join(format!("{old_synced}.sslo")).exists());
        assert!(blobs_dir(&root).join(format!("{new_synced}.sslo")).exists());
        assert!(blobs_dir(&root).join(format!("{unsynced}.sslo")).exists());
    }

    /// The sidebar once reported tens of megabytes still waiting to be
    /// backed up while the backup was complete, because rows outlived the
    /// files they described.
    #[test]
    fn usage_ignores_rows_whose_blob_is_no_longer_on_disk() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(blobs_dir(&root)).unwrap();

        let present_synced = Uuid::new_v4();
        let present_unsynced = Uuid::new_v4();
        let ghost = Uuid::new_v4();

        for (id, size) in [(present_synced, 5_000usize), (present_unsynced, 3_000)] {
            std::fs::write(blobs_dir(&root).join(format!("{id}.sslo")), vec![0u8; size]).unwrap();
        }
        record_blob_present(&root, present_synced, 5_000, true).unwrap();
        record_blob_present(&root, present_unsynced, 3_000, false).unwrap();
        // Recorded, never written: a superseded blob, or one purged after the
        // row was made. It owes nothing and occupies nothing.
        record_blob_present(&root, ghost, 900_000, false).unwrap();

        let usage = cache_usage(&root);
        assert_eq!(usage.local_bytes, 8_000);
        assert_eq!(usage.blob_count, 2);
        assert_eq!(usage.unsynced_bytes, 3_000);
        assert_eq!(usage.unsynced_count, 1);
    }

    #[test]
    fn cache_limit_setting_roundtrips_and_defaults() {
        let dir = tempdir().unwrap();
        let app_data = dir.path().to_path_buf();

        assert_eq!(get_cache_limit_bytes(&app_data), DEFAULT_CACHE_LIMIT_BYTES);

        set_cache_limit_bytes(&app_data, 5_000_000_000).unwrap();
        assert_eq!(get_cache_limit_bytes(&app_data), 5_000_000_000);
    }
}

#[cfg(test)]
mod pin_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_silo_is_not_a_full_copy_until_someone_says_so() {
        let dir = tempdir().unwrap();

        assert!(!keep_full_copy(dir.path()));
    }

    #[test]
    fn the_setting_survives_being_read_back() {
        let dir = tempdir().unwrap();

        set_keep_full_copy(dir.path(), true).unwrap();
        assert!(keep_full_copy(dir.path()));

        set_keep_full_copy(dir.path(), false).unwrap();
        assert!(!keep_full_copy(dir.path()));
    }

    #[test]
    fn nothing_is_evicted_from_a_device_holding_a_full_copy() {
        // Otherwise the setting would say "this is a copy" while the cache
        // limit quietly turned it back into an index.
        let dir = tempdir().unwrap();
        let blob = Uuid::new_v4();
        std::fs::create_dir_all(blobs_dir(dir.path())).unwrap();
        std::fs::write(blobs_dir(dir.path()).join(format!("{blob}.sslo")), b"x").unwrap();
        record_blob_present(dir.path(), blob, 1_000_000, true).unwrap();

        // A limit far below what is stored: without the pin this evicts.
        assert_eq!(enforce_cache_limit(dir.path(), 1).unwrap().len(), 1);

        record_blob_present(dir.path(), blob, 1_000_000, true).unwrap();
        std::fs::write(blobs_dir(dir.path()).join(format!("{blob}.sslo")), b"x").unwrap();
        set_keep_full_copy(dir.path(), true).unwrap();

        assert!(
            enforce_cache_limit(dir.path(), 1).unwrap().is_empty(),
            "a pinned silo lost content to the cache limit"
        );
    }
}

/// What each copy holds, through the whole life of a target list: added,
/// removed, replaced, re-added. The bug these exist for shipped once: a
/// target added after the content was already backed up elsewhere was
/// offered nothing and reported itself up to date while holding nothing.
#[cfg(test)]
mod delivery_tests {
    use super::*;
    use tempfile::tempdir;

    fn silo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(blobs_dir(&root)).unwrap();
        (dir, root)
    }

    #[test]
    fn a_target_added_later_is_owed_everything() {
        let (_dir, root) = silo();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();

        record_blob_delivered(&root, blob, first).unwrap();
        settle_blob_delivery(&root, &[first]).unwrap();

        assert!(list_undelivered_blob_ids(&root, first).is_empty());
        assert_eq!(
            list_undelivered_blob_ids(&root, second),
            vec![blob],
            "a copy that has never seen this blob must be owed it"
        );
    }

    #[test]
    fn eviction_waits_for_every_copy_not_the_first() {
        let (_dir, root) = silo();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();

        record_blob_delivered(&root, blob, first).unwrap();
        settle_blob_delivery(&root, &[first, second]).unwrap();
        assert_eq!(
            list_unsynced_blob_ids(&root),
            vec![blob],
            "one copy out of two is not a reason to drop the local file"
        );

        record_blob_delivered(&root, blob, second).unwrap();
        settle_blob_delivery(&root, &[first, second]).unwrap();
        assert!(list_unsynced_blob_ids(&root).is_empty());
    }

    #[test]
    fn removing_a_target_forgets_what_it_held() {
        let (_dir, root) = silo();
        let (gone, kept) = (Uuid::new_v4(), Uuid::new_v4());
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();
        record_blob_delivered(&root, blob, gone).unwrap();
        record_blob_delivered(&root, blob, kept).unwrap();
        settle_blob_delivery(&root, &[gone, kept]).unwrap();

        // The user removes one copy. What it held says nothing about the
        // silo any more.
        settle_blob_delivery(&root, &[kept]).unwrap();

        assert!(list_undelivered_blob_ids(&root, kept).is_empty());
        assert_eq!(
            list_undelivered_blob_ids(&root, gone),
            vec![blob],
            "a target that comes back is owed everything again"
        );
    }

    #[test]
    fn replacing_one_target_with_another_owes_the_new_one_everything() {
        // Add, remove, add a different one: the sequence that produced an
        // empty second copy with a green tick.
        let (_dir, root) = silo();
        let (old, new) = (Uuid::new_v4(), Uuid::new_v4());
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();
        record_blob_delivered(&root, blob, old).unwrap();
        settle_blob_delivery(&root, &[old]).unwrap();
        assert!(
            list_unsynced_blob_ids(&root).is_empty(),
            "settled on the old"
        );

        settle_blob_delivery(&root, &[new]).unwrap();

        assert_eq!(list_undelivered_blob_ids(&root, new), vec![blob]);
        assert_eq!(
            list_unsynced_blob_ids(&root),
            vec![blob],
            "and the local file is pinned again, since no copy holds it"
        );
    }

    #[test]
    fn a_silo_with_no_targets_is_left_alone() {
        // Nothing configured is not "everyone has it": the empty list must
        // not settle anything, or the local copy becomes evictable with
        // nowhere to fetch it back from.
        let (_dir, root) = silo();
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();

        settle_blob_delivery(&root, &[]).unwrap();

        assert_eq!(list_unsynced_blob_ids(&root), vec![blob]);
    }

    #[test]
    fn delivery_is_recorded_once_however_often_it_is_reported() {
        let (_dir, root) = silo();
        let target = Uuid::new_v4();
        let blob = Uuid::new_v4();
        record_blob_present(&root, blob, 10, false).unwrap();

        record_blob_delivered(&root, blob, target).unwrap();
        record_blob_delivered(&root, blob, target).unwrap();

        settle_blob_delivery(&root, &[target]).unwrap();
        assert!(list_unsynced_blob_ids(&root).is_empty());
    }
}
