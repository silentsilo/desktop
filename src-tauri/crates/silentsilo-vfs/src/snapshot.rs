//! The state of a vault at one point in the total order, which is what
//! makes compaction possible: with the state at Lamport `horizon` written
//! down, every record at or below it can be thrown away.
//!
//! It covers every derived table the apply path reads, not only the visible
//! tree: `name_claims` and `device_labels` decide contested names and late
//! renames, so dropping them would make outcomes depend on when compaction
//! ran. The cut is inclusive on the Lamport value alone, so object keys can
//! be filtered without decrypting. The rule the caller keeps in exchange: a
//! record at or below the horizon must never be applied to a restored
//! snapshot; a device still holding one has to re-bootstrap.
//! [`capture_at`] replays the log into a scratch database rather than
//! copying the live tables, which hold state above the horizon.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use silentsilo_core::{CoreError, CoreResult};
use uuid::Uuid;

use crate::oplog::{OpBody, all_ops, apply_op};
use crate::schema::init_schema;

fn db(e: rusqlite::Error) -> CoreError {
    CoreError::Database(e.to_string())
}

/// Format version of the snapshot itself. Separate from `SCHEMA_VERSION`,
/// which may change freely; this describes bytes that outlive the machine
/// that wrote them, so it follows FORMATS.md and a higher version is
/// refused rather than guessed at.
pub const SNAPSHOT_VERSION: u32 = 1;

/// A row of `folders`, as written down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderRow {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub favorite: bool,
}

/// A row of `files`, as written down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRow {
    pub id: Uuid,
    pub folder_id: Uuid,
    pub name: String,
    pub blob_id: Uuid,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub content_hash: Option<String>,
    /// This content's key, still wrapped. A snapshot stands in for the
    /// records it replaces, and after compaction the record that named the
    /// key is gone; without it the file is ciphertext nobody can open.
    #[serde(default)]
    pub blob_key: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub favorite: bool,
    /// Which record put this content here, so an edit arriving after the
    /// snapshot can still tell whether it diverged from what the file holds.
    /// Absent on a snapshot written before conflict copies existed.
    #[serde(default)]
    pub content_op_id: Option<Uuid>,
    #[serde(default)]
    pub content_lamport: u64,
    #[serde(default)]
    pub content_device_id: Option<Uuid>,
}

/// A row of `passwords`. `data` stays sealed exactly as the column holds it:
/// a snapshot is written to the same bucket as everything else and must not
/// be the one object that carries credentials the vault key does not cover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PasswordRow {
    pub id: Uuid,
    pub data: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A row of `name_claims`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameClaimRow {
    pub entry_id: Uuid,
    pub is_folder: bool,
    pub scope_id: Uuid,
    pub key: String,
    pub desired: String,
    pub lamport: u64,
    pub device_id: Uuid,
    pub op_id: Uuid,
}

/// A row of `device_labels`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceLabelRow {
    pub device_id: Uuid,
    pub label: String,
    pub lamport: u64,
    pub author: String,
    pub system_name: Option<String>,
    pub platform: Option<String>,
}

/// The whole derived state at a horizon. Deliberately carries no device
/// identity: two devices sharing an id would order each other's records
/// inconsistently, the one failure this design cannot detect afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub vault_id: Uuid,
    /// Every record with `lamport <= horizon` is accounted for below.
    pub horizon: u64,
    /// Wall clock of the device that captured it. A label for the user, not
    /// an ordering: it comes from another machine's clock.
    pub captured_at: i64,
    pub folders: Vec<FolderRow>,
    pub files: Vec<FileRow>,
    pub passwords: Vec<PasswordRow>,
    pub name_claims: Vec<NameClaimRow>,
    pub device_labels: Vec<DeviceLabelRow>,
}

impl Snapshot {
    pub fn to_bytes(&self) -> CoreResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| CoreError::Database(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        let snapshot: Snapshot =
            serde_json::from_slice(bytes).map_err(|e| CoreError::Database(e.to_string()))?;
        if snapshot.version > SNAPSHOT_VERSION {
            return Err(CoreError::UnsupportedOperation(format!(
                "this silo was compacted by a newer version of SilentSilo \
                 (snapshot format {}): update to open it",
                snapshot.version
            )));
        }
        Ok(snapshot)
    }
}

// ── Choosing a horizon ──────────────────────────────────────────────

/// How far back compaction is allowed to reach. Two margins because they
/// fail differently: `retain_seconds` is the promise that a device syncing
/// at least this often never rebuilds, read from untrusted record
/// timestamps; `keep_recent` holds when a clock is wrong, keeping this many
/// records at the end of the log whatever the timestamps claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionPolicy {
    pub retain_seconds: i64,
    pub keep_recent: usize,
    /// Below this the log is not worth compacting: the pass costs a full
    /// replay and an upload, and a few hundred records cost nobody anything.
    pub min_records: usize,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            // A month. Long enough to cover a holiday, a broken laptop, or a
            // machine that only gets opened at the end of the quarter.
            retain_seconds: 30 * 24 * 60 * 60,
            keep_recent: 500,
            min_records: 5_000,
        }
    }
}

/// Picks a horizon, or `None` when the log should be left alone. `now` is
/// passed in so the decision stays testable. A record is eligible when it
/// is older than `retain_seconds` **and** far enough from the end of the
/// log; records sharing a Lamport value are treated as one, since the cut
/// is inclusive on the value.
pub fn choose_horizon(
    conn: &Connection,
    policy: &CompactionPolicy,
    now: i64,
) -> CoreResult<Option<u64>> {
    let mut stmt = conn
        .prepare("SELECT lamport, at FROM oplog ORDER BY lamport, device_id, op_id")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)?))
        })
        .map_err(db)?;
    let records: Vec<(u64, i64)> = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;

    if records.len() < policy.min_records {
        return Ok(None);
    }

    // Everything from here on is off limits, whatever its timestamp says.
    let cutoff = records.len().saturating_sub(policy.keep_recent);
    let old_enough = now - policy.retain_seconds;

    let mut horizon = None;
    for (lamport, at) in records.iter().take(cutoff) {
        if *at <= old_enough {
            horizon = Some(*lamport);
        }
    }

    let Some(horizon) = horizon else {
        return Ok(None);
    };

    // A Lamport value is not split: if any record at this value survives the
    // cut, the whole value has to stay above the horizon.
    let kept_at_horizon = records
        .iter()
        .skip(cutoff)
        .any(|(lamport, _)| *lamport <= horizon);
    let horizon = if kept_at_horizon {
        match records
            .iter()
            .take(cutoff)
            .map(|(lamport, _)| *lamport)
            .filter(|lamport| *lamport < horizon)
            .max()
        {
            Some(lower) => lower,
            None => return Ok(None),
        }
    } else {
        horizon
    };

    // Nothing above it means nothing to compact to.
    let highest = records.last().map(|(lamport, _)| *lamport).unwrap_or(0);
    if horizon >= highest {
        return Ok(None);
    }

    // Already covered by the snapshot this device rebuilds from.
    if horizon <= base_horizon(conn)? {
        return Ok(None);
    }

    Ok(Some(horizon))
}

/// Builds the state as of `horizon` by replaying the log into a scratch
/// database. Refuses a horizon at or above the highest record on hand: how
/// much margin is enough is the caller's decision, but none at all never
/// is.
pub fn capture_at(conn: &Connection, vault_id: Uuid, horizon: u64) -> CoreResult<Snapshot> {
    let records = all_ops(conn)?;
    let highest = records.iter().map(|r| r.lamport).max().unwrap_or(0);
    if horizon >= highest {
        return Err(CoreError::Invalid(format!(
            "refusing to snapshot at {horizon}: the log reaches {highest}, \
             so nothing would be left above the horizon"
        )));
    }

    let scratch = Connection::open_in_memory().map_err(db)?;
    init_schema(&scratch, vault_id)?;
    for record in records.iter().filter(|r| r.lamport <= horizon) {
        // Records this build cannot act on are stored and forwarded, never
        // applied. Snapshotting one would mean writing down a change nobody
        // has interpreted, so they stay in the log instead, and a build that
        // understands them applies them on a rebuild.
        if matches!(record.op, OpBody::Known(_)) {
            apply_op(&scratch, record)?;
        }
    }

    dump(&scratch, vault_id, horizon)
}

/// Reads the derived tables of an already-restored database.
fn dump(conn: &Connection, vault_id: Uuid, horizon: u64) -> CoreResult<Snapshot> {
    Ok(Snapshot {
        version: SNAPSHOT_VERSION,
        vault_id,
        horizon,
        captured_at: chrono::Utc::now().timestamp(),
        folders: read_folders(conn)?,
        files: read_files(conn)?,
        passwords: read_passwords(conn)?,
        name_claims: read_name_claims(conn)?,
        device_labels: read_device_labels(conn)?,
    })
}

/// Writes a snapshot's rows into the derived tables of `conn`. Only onto
/// freshly created tables; it refuses a populated vault rather than
/// clearing it. The seeded root folder from `init_schema` is replaced by
/// the snapshot's own root rather than merged with.
pub fn restore(conn: &Connection, snapshot: &Snapshot) -> CoreResult<()> {
    let stored_vault: Option<String> = conn
        .query_row(
            "SELECT value FROM vault_meta WHERE key = 'vault_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(db)?;
    if let Some(stored) = stored_vault
        && stored != snapshot.vault_id.to_string()
    {
        return Err(CoreError::Invalid(format!(
            "this snapshot belongs to vault {}, not {stored}",
            snapshot.vault_id
        )));
    }

    let folders: i64 = conn
        .query_row("SELECT COUNT(*) FROM folders", [], |row| row.get(0))
        .map_err(db)?;
    let files: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .map_err(db)?;
    if files > 0 || folders > 1 {
        return Err(CoreError::Invalid(
            "refusing to restore a snapshot over a vault that already has an index: \
             drop the derived tables first"
                .into(),
        ));
    }
    // At most the seeded root, which the snapshot is about to supply itself.
    conn.execute("DELETE FROM folders", []).map_err(db)?;

    // Parents are written before children, so the foreign key from a folder
    // to its parent holds at every point. The dump preserves that order.
    for row in &snapshot.folders {
        conn.execute(
            "INSERT INTO folders(id, parent_id, name, path, created_at, updated_at, deleted_at, favorite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.id.to_string(),
                row.parent_id.map(|id| id.to_string()),
                row.name,
                row.path,
                row.created_at,
                row.updated_at,
                row.deleted_at,
                row.favorite as i64,
            ],
        )
        .map_err(db)?;
    }

    for row in &snapshot.files {
        conn.execute(
            "INSERT INTO files(id, folder_id, name, blob_id, size_bytes, mime_type,
                               content_hash, created_at, updated_at, deleted_at, favorite,
                               content_op_id, content_lamport, content_device_id, blob_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                row.id.to_string(),
                row.folder_id.to_string(),
                row.name,
                row.blob_id.to_string(),
                row.size_bytes,
                row.mime_type,
                row.content_hash,
                row.created_at,
                row.updated_at,
                row.deleted_at,
                row.favorite as i64,
                row.content_op_id.map(|id| id.to_string()),
                row.content_lamport as i64,
                row.content_device_id.map(|id| id.to_string()),
                row.blob_key,
            ],
        )
        .map_err(db)?;
    }

    for row in &snapshot.passwords {
        conn.execute(
            "INSERT INTO passwords(id, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![row.id.to_string(), row.data, row.created_at, row.updated_at],
        )
        .map_err(db)?;
    }

    for row in &snapshot.name_claims {
        conn.execute(
            "INSERT INTO name_claims(entry_id, is_folder, scope_id, key, desired, lamport, device_id, op_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.entry_id.to_string(),
                row.is_folder as i64,
                row.scope_id.to_string(),
                row.key,
                row.desired,
                row.lamport as i64,
                row.device_id.to_string(),
                row.op_id.to_string(),
            ],
        )
        .map_err(db)?;
    }

    for row in &snapshot.device_labels {
        conn.execute(
            "INSERT INTO device_labels(device_id, label, lamport, author, system_name, platform)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.device_id.to_string(),
                row.label,
                row.lamport as i64,
                row.author,
                row.system_name,
                row.platform,
            ],
        )
        .map_err(db)?;
    }

    Ok(())
}

fn uuid_at(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let raw: String = row.get(index)?;
    Uuid::parse_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn optional_uuid_at(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<Uuid>> {
    match row.get::<_, Option<String>>(index)? {
        None => Ok(None),
        Some(raw) => Uuid::parse_str(&raw).map(Some).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        }),
    }
}

fn read_folders(conn: &Connection) -> CoreResult<Vec<FolderRow>> {
    // Shallowest first, so a restore never writes a child before its parent.
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, name, path, created_at, updated_at, deleted_at, favorite
             FROM folders ORDER BY length(path), path",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FolderRow {
                id: uuid_at(row, 0)?,
                parent_id: optional_uuid_at(row, 1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                deleted_at: row.get(6)?,
                favorite: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}

fn read_files(conn: &Connection) -> CoreResult<Vec<FileRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, folder_id, name, blob_id, size_bytes, mime_type, content_hash,
                    created_at, updated_at, deleted_at, favorite,
                    content_op_id, content_lamport, content_device_id, blob_key
             FROM files ORDER BY id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FileRow {
                id: uuid_at(row, 0)?,
                folder_id: uuid_at(row, 1)?,
                name: row.get(2)?,
                blob_id: uuid_at(row, 3)?,
                size_bytes: row.get(4)?,
                mime_type: row.get(5)?,
                content_hash: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                deleted_at: row.get(9)?,
                favorite: row.get::<_, i64>(10)? != 0,
                content_op_id: optional_uuid_at(row, 11)?,
                content_lamport: row.get::<_, i64>(12)? as u64,
                content_device_id: optional_uuid_at(row, 13)?,
                blob_key: row.get::<_, Option<String>>(14)?.unwrap_or_default(),
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}

fn read_passwords(conn: &Connection) -> CoreResult<Vec<PasswordRow>> {
    let mut stmt = conn
        .prepare("SELECT id, data, created_at, updated_at FROM passwords ORDER BY id")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PasswordRow {
                id: uuid_at(row, 0)?,
                data: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}

fn read_name_claims(conn: &Connection) -> CoreResult<Vec<NameClaimRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT entry_id, is_folder, scope_id, key, desired, lamport, device_id, op_id
             FROM name_claims ORDER BY entry_id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(NameClaimRow {
                entry_id: uuid_at(row, 0)?,
                is_folder: row.get::<_, i64>(1)? != 0,
                scope_id: uuid_at(row, 2)?,
                key: row.get(3)?,
                desired: row.get(4)?,
                lamport: row.get::<_, i64>(5)? as u64,
                device_id: uuid_at(row, 6)?,
                op_id: uuid_at(row, 7)?,
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}

fn read_device_labels(conn: &Connection) -> CoreResult<Vec<DeviceLabelRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT device_id, label, lamport, author, system_name, platform
             FROM device_labels ORDER BY device_id",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DeviceLabelRow {
                device_id: uuid_at(row, 0)?,
                label: row.get(1)?,
                lamport: row.get::<_, i64>(2)? as u64,
                author: row.get(3)?,
                system_name: row.get(4)?,
                platform: row.get(5)?,
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}

/// The snapshot this device rebuilds from, and the records it may drop.
///
/// Kept in its own table rather than a `vault_meta` row: it is bytes, it is
/// large, and it is the one thing in the database that is not derived from
/// the log and not recoverable by replaying it. A single row, replaced
/// whenever compaction advances the horizon.
pub fn init_base(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS vault_base (
            id       INTEGER PRIMARY KEY CHECK (id = 0),
            horizon  INTEGER NOT NULL,
            snapshot BLOB NOT NULL
        );
        ",
    )
}

/// Stores the snapshot a rebuild starts from.
pub fn write_base(conn: &Connection, snapshot: &Snapshot) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO vault_base(id, horizon, snapshot) VALUES (0, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET horizon = excluded.horizon, snapshot = excluded.snapshot",
        params![snapshot.horizon as i64, snapshot.to_bytes()?],
    )
    .map_err(db)?;
    Ok(())
}

/// `None` on a vault that has never been compacted, which is every vault
/// until it grows enough to need it.
pub fn read_base(conn: &Connection) -> CoreResult<Option<Snapshot>> {
    let bytes: Option<Vec<u8>> = conn
        .query_row("SELECT snapshot FROM vault_base WHERE id = 0", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(db)?;
    bytes.map(|b| Snapshot::from_bytes(&b)).transpose()
}

/// The horizon this device rebuilds from, or 0 when it holds the whole log.
///
/// Read without decoding the snapshot, because this is asked on every sync
/// pass and the answer is one integer.
pub fn base_horizon(conn: &Connection) -> CoreResult<u64> {
    let horizon: Option<i64> = conn
        .query_row("SELECT horizon FROM vault_base WHERE id = 0", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(db)?;
    Ok(horizon.unwrap_or(0) as u64)
}

/// Drops the records a snapshot has made redundant. Only records the
/// bucket already holds: until pushed, this table is a record's only copy.
/// An unpushed record at or below the horizon is left alone and reported,
/// because the caller has to act on it.
pub fn prune_to(conn: &Connection, horizon: u64) -> CoreResult<PruneReport> {
    let stranded: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM oplog WHERE lamport <= ?1 AND pushed = 0",
            [horizon as i64],
            |row| row.get(0),
        )
        .map_err(db)?;

    let dropped = conn
        .execute(
            "DELETE FROM oplog WHERE lamport <= ?1 AND pushed = 1",
            [horizon as i64],
        )
        .map_err(db)?;

    Ok(PruneReport {
        dropped,
        stranded: stranded as usize,
    })
}

/// What [`prune_to`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PruneReport {
    pub dropped: usize,
    /// Records below the horizon that the bucket does not have. Never
    /// dropped, always worth surfacing.
    pub stranded: usize,
}

/// Compacts the local log: capture at `horizon`, store it, drop what it
/// covers.
///
/// The snapshot is written before anything is deleted, and both happen in one
/// transaction, so a crash in the middle leaves either the whole log or a log
/// plus the snapshot that covers its missing half. It never leaves a hole.
pub fn compact_local(conn: &mut Connection, snapshot: &Snapshot) -> CoreResult<PruneReport> {
    let horizon = snapshot.horizon;

    let tx = conn.transaction().map_err(db)?;
    init_base(&tx).map_err(db)?;
    write_base(&tx, snapshot)?;
    let report = prune_to(&tx, horizon)?;
    tx.commit().map_err(db)?;

    Ok(report)
}

/// Throws away everything derived from the log, installs `snapshot` as the
/// new base, and gives this device a fresh identity. The fresh id is the
/// part that is easy to get wrong: the old id's chain positions are derived
/// from the log being dropped, and reusing it would put two different
/// records at one position in one device's chain, exactly what
/// [`crate::verify_chains`] exists to catch. Unpushed records below the
/// horizon do not survive; [`prune_to`] reports them beforehand so the
/// caller can say so.
pub fn rebootstrap(conn: &mut Connection, snapshot: &Snapshot) -> CoreResult<Uuid> {
    let tx = conn.transaction().map_err(db)?;

    tx.execute("DELETE FROM oplog", []).map_err(db)?;
    crate::schema::drop_derived(&tx).map_err(db)?;
    crate::schema::init_derived(&tx, snapshot.vault_id).map_err(db)?;
    restore(&tx, snapshot)?;
    write_base(&tx, snapshot)?;

    let device_id = crate::oplog::set_device_id(&tx, Uuid::new_v4())?;
    // The clock moves up to the horizon, so the next local change sorts after
    // everything the snapshot accounts for rather than colliding with it.
    crate::oplog::observe_lamport(&tx, snapshot.horizon)?;

    tx.commit().map_err(db)?;
    Ok(device_id)
}

/// Replays the log on top of the base snapshot, if there is one.
///
/// This is what [`crate::oplog::rebuild_derived`] calls once the tables have
/// been dropped and recreated, and it is why compaction does not cost the
/// vault its ability to rebuild its own index.
pub fn restore_base(conn: &Connection) -> CoreResult<u64> {
    match read_base(conn)? {
        None => Ok(0),
        Some(snapshot) => {
            restore(conn, &snapshot)?;
            Ok(snapshot.horizon)
        }
    }
}

// ── Which blobs are still needed ────────────────────────────────────

/// Every blob the index still points at: the set that must survive a
/// sweep. Trashed files count, since a restore has to find their content.
/// Read from `files` rather than the log, which is what keeps it correct
/// after compaction.
pub fn referenced_blobs(conn: &Connection) -> CoreResult<std::collections::HashSet<Uuid>> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT blob_id FROM files")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db)?;

    let mut out = std::collections::HashSet::new();
    for row in rows {
        let raw = row.map_err(db)?;
        // A row whose blob id will not parse is not something to reason
        // about: treated as referencing nothing rather than as permission to
        // delete something.
        if let Ok(id) = Uuid::parse_str(&raw) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// Blobs seen unreferenced on the last sweep, per target: the two-pass
/// rule only works when both sightings are of the same pile of objects.
/// Local bookkeeping, not vault state; losing it costs one wasted pass.
pub fn init_gc(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS blob_gc_candidates (
            target  TEXT NOT NULL,
            blob_id TEXT NOT NULL,
            PRIMARY KEY (target, blob_id)
        );
        ",
    )
}

pub fn gc_candidates(
    conn: &Connection,
    target: Uuid,
) -> CoreResult<std::collections::HashSet<Uuid>> {
    let mut stmt = conn
        .prepare("SELECT blob_id FROM blob_gc_candidates WHERE target = ?1")
        .map_err(db)?;
    let rows = stmt
        .query_map([target.to_string()], |row| row.get::<_, String>(0))
        .map_err(db)?;
    let mut out = std::collections::HashSet::new();
    for row in rows {
        if let Ok(id) = Uuid::parse_str(&row.map_err(db)?) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// Replaces one target's candidate set with what the sweep just saw there.
/// Replaced rather than added to: a blob that has become referenced since
/// the last pass has to leave the set.
pub fn set_gc_candidates(
    conn: &mut Connection,
    target: Uuid,
    candidates: &std::collections::HashSet<Uuid>,
) -> CoreResult<()> {
    let tx = conn.transaction().map_err(db)?;
    tx.execute(
        "DELETE FROM blob_gc_candidates WHERE target = ?1",
        [target.to_string()],
    )
    .map_err(db)?;
    for id in candidates {
        tx.execute(
            "INSERT OR IGNORE INTO blob_gc_candidates(target, blob_id) VALUES (?1, ?2)",
            [target.to_string(), id.to_string()],
        )
        .map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok(())
}

fn sweep_at_key(target: Uuid) -> String {
    format!("blob_sweep_at:{target}")
}

/// Whether a sweep of this target is due. A sweep lists every object under
/// the blob prefix, the most expensive request a metered bucket sees, and
/// once a day is often enough for something whose only cost is bytes. Per
/// target, since a bucket added yesterday has never been swept.
pub fn sweep_due(
    conn: &Connection,
    target: Uuid,
    now: i64,
    interval_seconds: i64,
) -> CoreResult<bool> {
    let last: Option<String> = conn
        .query_row(
            "SELECT value FROM vault_meta WHERE key = ?1",
            [sweep_at_key(target)],
            |row| row.get(0),
        )
        .optional()
        .map_err(db)?;
    let last: i64 = last.and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(now - last >= interval_seconds)
}

pub fn record_sweep(conn: &Connection, target: Uuid, now: i64) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO vault_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![sweep_at_key(target), now.to_string()],
    )
    .map_err(db)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::tests::{bare_device, snapshot_of};
    use crate::oplog::{OpRecord, VaultOp, replay};
    use crate::schema::drop_derived_for_test;
    use crate::{rebuild_derived, root_folder_id_for};

    /// Records from one device, chained the way real ones are.
    struct Author {
        device_id: Uuid,
        seq: u64,
        prev: Option<String>,
    }

    impl Author {
        fn new() -> Self {
            Self {
                device_id: Uuid::new_v4(),
                seq: 0,
                prev: None,
            }
        }

        fn make(&mut self, lamport: u64, op: VaultOp) -> OpRecord {
            let record = OpRecord::authored(
                Uuid::new_v4(),
                lamport,
                self.device_id,
                1_700_000_000,
                self.seq,
                self.prev.clone(),
                op,
            );
            self.seq += 1;
            self.prev = Some(record.fingerprint().unwrap());
            record
        }
    }

    fn add_file(folder_id: Uuid, name: &str) -> VaultOp {
        VaultOp::AddFile {
            id: Uuid::new_v4(),
            folder_id,
            name: name.into(),
            blob_id: Uuid::new_v4(),
            size_bytes: 10,
            content_hash: "hash".into(),
            mime_type: None,
            blob_key: String::new(),
        }
    }

    fn file_id(record: &OpRecord) -> Uuid {
        match &record.op {
            OpBody::Known(VaultOp::AddFile { id, .. }) => *id,
            other => panic!("expected an AddFile record, got {other:?}"),
        }
    }

    /// Captures at a horizon and compacts to it, which is what every caller
    /// outside these tests does in two steps.
    fn compact(conn: &mut Connection, vault_id: Uuid, horizon: u64) -> PruneReport {
        let snapshot = capture_at(conn, vault_id, horizon).unwrap();
        compact_local(conn, &snapshot).unwrap()
    }

    /// Throws the derived tables away and builds them again, which is what
    /// a schema bump does in the field. This is the one moment a vault that
    /// compacted without recording its state would notice.
    fn rebuild_from_scratch(conn: &Connection, vault_id: Uuid) {
        drop_derived_for_test(conn).unwrap();
        crate::schema::init_schema(conn, vault_id).unwrap();
        rebuild_derived(conn).unwrap();
    }

    /// A vault with a small tree, and the records that built it in order.
    fn populated() -> (Connection, Uuid, Vec<OpRecord>) {
        let vault_id = Uuid::new_v4();
        let conn = bare_device(vault_id);
        let root = root_folder_id_for(vault_id);
        let mut author = Author::new();

        let invoices = Uuid::new_v4();
        let records = vec![
            author.make(
                1,
                VaultOp::CreateFolder {
                    id: invoices,
                    parent_id: root,
                    name: "Invoices".into(),
                },
            ),
            author.make(2, add_file(invoices, "march.pdf")),
            author.make(3, add_file(root, "notes.txt")),
            author.make(
                4,
                VaultOp::UpsertPassword {
                    id: Uuid::new_v4(),
                    data: "sealed-entry".into(),
                },
            ),
            author.make(5, add_file(root, "later.txt")),
        ];
        replay(&conn, records.clone()).unwrap();
        // Everything is in the bucket as far as this device knows, which is
        // what makes it prunable.
        conn.execute("UPDATE oplog SET pushed = 1", []).unwrap();
        (conn, vault_id, records)
    }

    /// A log of `count` records, all from one device, timestamped `at`.
    fn log_of(count: usize, at: i64) -> (Connection, Uuid) {
        let vault_id = Uuid::new_v4();
        let conn = bare_device(vault_id);
        let root = root_folder_id_for(vault_id);
        let mut author = Author::new();
        let mut records = Vec::with_capacity(count);
        for i in 0..count {
            let mut record = author.make(i as u64 + 1, add_file(root, &format!("f{i}.txt")));
            record.at = at;
            records.push(record);
        }
        replay(&conn, records).unwrap();
        conn.execute("UPDATE oplog SET at = ?1", [at]).unwrap();
        (conn, vault_id)
    }

    /// Old enough to compact under the default policy, with room to spare.
    const LONG_AGO: i64 = 1_600_000_000;
    const NOW: i64 = 1_700_000_000;

    fn small_policy() -> CompactionPolicy {
        CompactionPolicy {
            retain_seconds: 30 * 24 * 60 * 60,
            keep_recent: 5,
            min_records: 10,
        }
    }

    #[test]
    fn a_short_log_is_left_alone() {
        // Compaction costs a full replay and an upload. A log this size costs
        // nobody anything.
        let (conn, _) = log_of(9, LONG_AGO);

        assert_eq!(choose_horizon(&conn, &small_policy(), NOW).unwrap(), None);
    }

    #[test]
    fn recent_records_are_never_compacted() {
        // Every record was written moments ago, so however long the log is,
        // there is nothing old enough to drop.
        let (conn, _) = log_of(50, NOW - 60);

        assert_eq!(choose_horizon(&conn, &small_policy(), NOW).unwrap(), None);
    }

    #[test]
    fn the_horizon_stops_short_of_the_end_of_the_log() {
        // The count margin, which is the one that holds when a clock is
        // wrong: these records all claim to be ancient, and the last few are
        // still off limits.
        let (conn, _) = log_of(20, LONG_AGO);

        let horizon = choose_horizon(&conn, &small_policy(), NOW)
            .unwrap()
            .unwrap();

        assert_eq!(
            horizon, 15,
            "keep_recent is 5, so the last five records stay above the horizon"
        );
    }

    #[test]
    fn a_wrong_clock_cannot_compact_the_whole_log() {
        // The failure this margin exists for: a device whose clock is years
        // ahead writes records that look ancient the moment they land. The
        // count is what stops it taking the log with it.
        let (conn, _) = log_of(20, i64::MIN / 2);

        let horizon = choose_horizon(&conn, &small_policy(), NOW)
            .unwrap()
            .unwrap();

        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM oplog WHERE lamport > ?1",
                [horizon as i64],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 5);
    }

    #[test]
    fn a_horizon_already_covered_is_not_chosen_again() {
        // Otherwise every pass would re-upload a snapshot of the same state
        // and delete nothing.
        let (mut conn, vault_id) = log_of(20, LONG_AGO);
        let policy = small_policy();
        let horizon = choose_horizon(&conn, &policy, NOW).unwrap().unwrap();
        compact(&mut conn, vault_id, horizon);

        assert_eq!(choose_horizon(&conn, &policy, NOW).unwrap(), None);
    }

    #[test]
    fn a_lamport_value_is_never_split_across_the_horizon() {
        // Two devices wrote at the same Lamport value, and the count margin
        // falls in the middle of it. Cutting there would compact half of a
        // value and keep the other half, and the cut is inclusive, so the
        // kept half could never be applied again.
        let vault_id = Uuid::new_v4();
        let conn = bare_device(vault_id);
        let root = root_folder_id_for(vault_id);
        let mut one = Author::new();
        let mut two = Author::new();

        let mut records = Vec::new();
        for i in 1..=10u64 {
            records.push(one.make(i, add_file(root, &format!("a{i}.txt"))));
        }
        // Both devices at 11. With fourteen records and three held back, the
        // count margin falls between these two: one is eligible, the other is
        // not, and they share a Lamport value.
        records.push(one.make(11, add_file(root, "b.txt")));
        records.push(two.make(11, add_file(root, "c.txt")));
        records.push(one.make(12, add_file(root, "d.txt")));
        records.push(one.make(13, add_file(root, "e.txt")));
        replay(&conn, records).unwrap();
        conn.execute("UPDATE oplog SET at = ?1", [LONG_AGO])
            .unwrap();

        let policy = CompactionPolicy {
            keep_recent: 3,
            min_records: 10,
            ..small_policy()
        };
        let horizon = choose_horizon(&conn, &policy, NOW).unwrap().unwrap();

        assert!(
            horizon < 11,
            "the horizon landed inside a Lamport value: {horizon}"
        );
    }

    #[test]
    fn a_compacted_vault_rebuilds_to_the_same_tree() {
        // The property the whole design rests on: the derived tables are
        // disposable because the log can rebuild them. Compaction must not
        // cost the vault that, which is why the base snapshot exists.
        let (mut conn, vault_id, _) = populated();
        let before = snapshot_of(&conn);

        compact(&mut conn, vault_id, 3);
        rebuild_from_scratch(&conn, vault_id);

        assert_eq!(snapshot_of(&conn), before);
    }

    #[test]
    fn compaction_drops_only_what_the_snapshot_covers() {
        let (mut conn, vault_id, _) = populated();

        let report = compact(&mut conn, vault_id, 3);

        assert_eq!(
            report,
            PruneReport {
                dropped: 3,
                stranded: 0
            }
        );
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM oplog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 2, "records above the horizon stay");
        assert_eq!(base_horizon(&conn).unwrap(), 3);
    }

    #[test]
    fn an_unpushed_record_is_never_dropped() {
        // The bucket has no copy, so the log row is the only one. Dropping it
        // would lose the change outright.
        let (mut conn, vault_id, _) = populated();
        conn.execute("UPDATE oplog SET pushed = 0 WHERE lamport = 2", [])
            .unwrap();

        let report = compact(&mut conn, vault_id, 3);

        assert_eq!(report.dropped, 2);
        assert_eq!(report.stranded, 1);
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM oplog WHERE lamport = 2", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, 1);
    }

    #[test]
    fn a_compacted_device_still_converges_with_one_that_kept_everything() {
        // Two devices holding the same records, one compacted. Later changes
        // reach both, and they must still agree: compaction is a local
        // storage decision and has no business being visible in the outcome.
        let (mut compacted, vault_id, records) = populated();
        let intact = bare_device(vault_id);
        replay(&intact, records.clone()).unwrap();

        compact(&mut compacted, vault_id, 3);
        rebuild_from_scratch(&compacted, vault_id);

        let root = root_folder_id_for(vault_id);
        let mut latecomer = Author::new();
        let later = vec![
            latecomer.make(6, add_file(root, "arrived-after.txt")),
            latecomer.make(
                7,
                VaultOp::RenameFile {
                    // A file created below the horizon, so the rename can
                    // only land if the snapshot carried it.
                    id: file_id(&records[2]),
                    name: "renamed.txt".into(),
                },
            ),
        ];
        replay(&compacted, later.clone()).unwrap();
        replay(&intact, later).unwrap();

        assert_eq!(snapshot_of(&compacted), snapshot_of(&intact));
    }

    #[test]
    fn a_name_taken_below_the_horizon_still_wins_against_a_later_claim() {
        // The reason name_claims is in the snapshot. Two entries want one
        // name; the older claim keeps it and the newcomer is ranked. Drop the
        // claims and the newcomer would take the name outright, so the tree
        // would depend on when compaction ran.
        let (mut compacted, vault_id, _) = populated();
        let intact = bare_device(vault_id);
        let records = all_ops(&compacted).unwrap();
        replay(&intact, records).unwrap();

        compact(&mut compacted, vault_id, 3);
        rebuild_from_scratch(&compacted, vault_id);

        let root = root_folder_id_for(vault_id);
        let mut other = Author::new();
        // "notes.txt" was claimed at Lamport 3, below the horizon.
        let clash = vec![other.make(6, add_file(root, "notes.txt"))];
        replay(&compacted, clash.clone()).unwrap();
        replay(&intact, clash).unwrap();

        assert_eq!(snapshot_of(&compacted), snapshot_of(&intact));
        let names: Vec<String> = snapshot_of(&compacted)
            .into_iter()
            .filter(|row| row.contains("notes"))
            .collect();
        assert_eq!(
            names.len(),
            2,
            "both files exist, under different names: {names:?}"
        );
    }

    #[test]
    fn a_snapshot_holds_the_state_at_its_horizon_not_the_state_now() {
        // Captured by replaying into a scratch database rather than by
        // copying the live tables, which is the difference between a snapshot
        // labelled Lamport 3 and one that quietly contains Lamport 5.
        let (conn, vault_id, _) = populated();

        let snapshot = capture_at(&conn, vault_id, 3).unwrap();

        let names: Vec<&str> = snapshot.files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"march.pdf"));
        assert!(names.contains(&"notes.txt"));
        assert!(
            !names.contains(&"later.txt"),
            "a file added above the horizon is not part of the state at it"
        );
        assert!(
            snapshot.passwords.is_empty(),
            "the password entry is at Lamport 4, above the horizon"
        );
    }

    #[test]
    fn a_horizon_with_nothing_above_it_is_refused() {
        let (conn, vault_id, _) = populated();

        let err = capture_at(&conn, vault_id, 5).unwrap_err();

        assert!(
            err.to_string().contains("refusing to snapshot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_snapshot_from_a_newer_build_is_refused() {
        let (conn, vault_id, _) = populated();
        let mut snapshot = capture_at(&conn, vault_id, 3).unwrap();
        snapshot.version = SNAPSHOT_VERSION + 1;

        let err = Snapshot::from_bytes(&snapshot.to_bytes().unwrap()).unwrap_err();

        assert!(err.to_string().contains("newer version"), "was: {err}");
    }

    #[test]
    fn a_snapshot_from_another_vault_is_refused() {
        // Restoring one silo's state into another would not fail loudly on
        // its own: the ids would match nothing, and the user would be looking
        // at a tree that never existed.
        let (conn, vault_id, _) = populated();
        let mut snapshot = capture_at(&conn, vault_id, 3).unwrap();
        snapshot.vault_id = Uuid::new_v4();

        let err = restore(&conn, &snapshot).unwrap_err();

        assert!(err.to_string().contains("belongs to vault"), "was: {err}");
    }

    #[test]
    fn the_bytes_survive_a_round_trip() {
        let (conn, vault_id, _) = populated();
        let snapshot = capture_at(&conn, vault_id, 3).unwrap();

        let decoded = Snapshot::from_bytes(&snapshot.to_bytes().unwrap()).unwrap();

        assert_eq!(decoded, snapshot);
    }
}

#[cfg(test)]
mod gc_target_tests {
    use super::*;
    use crate::oplog::tests::bare_device;

    #[test]
    fn what_was_seen_in_one_target_says_nothing_about_another() {
        // The two-pass rule counts sightings of the same pile of objects. A
        // shared set would let two sightings in one bucket authorise a delete
        // from the other, which is the failure this keying exists to stop.
        let mut conn = bare_device(Uuid::new_v4());
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let blob = Uuid::new_v4();

        set_gc_candidates(&mut conn, first, &[blob].into_iter().collect()).unwrap();

        assert!(gc_candidates(&conn, first).unwrap().contains(&blob));
        assert!(
            gc_candidates(&conn, second).unwrap().is_empty(),
            "a sighting in one bucket leaked into another"
        );
    }

    #[test]
    fn replacing_one_targets_set_leaves_the_others_alone() {
        let mut conn = bare_device(Uuid::new_v4());
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let kept = Uuid::new_v4();

        set_gc_candidates(&mut conn, second, &[kept].into_iter().collect()).unwrap();
        set_gc_candidates(&mut conn, first, &std::collections::HashSet::new()).unwrap();

        assert_eq!(gc_candidates(&conn, second).unwrap().len(), 1);
    }

    #[test]
    fn a_target_added_today_is_due_whatever_the_other_one_did() {
        let conn = bare_device(Uuid::new_v4());
        let swept = Uuid::new_v4();
        let fresh = Uuid::new_v4();
        let day = 24 * 60 * 60;

        record_sweep(&conn, swept, 1_000_000).unwrap();

        assert!(!sweep_due(&conn, swept, 1_000_100, day).unwrap());
        assert!(
            sweep_due(&conn, fresh, 1_000_100, day).unwrap(),
            "a bucket that has never been swept was told to wait"
        );
    }
}
