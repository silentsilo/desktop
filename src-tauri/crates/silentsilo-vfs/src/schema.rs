use rusqlite::{Connection, OptionalExtension};
use silentsilo_core::{CoreError, CoreResult};
use uuid::Uuid;

fn db(e: rusqlite::Error) -> CoreError {
    CoreError::Database(e.to_string())
}

/// Shape of the derived tables. Bump this whenever any table below
/// changes. Every table except `vault_meta` and `oplog` is a cache of the
/// log, so a mismatch is resolved by dropping the cache and replaying,
/// never by migrating in place, which is why a future release cannot fail
/// to open an older silo.
///
/// **Adding a `VaultOp` variant counts as a change and must bump this**:
/// the rebuild it triggers is what applies records an earlier build stored
/// but could not act on. See `oplog::rebuild_derived`.
pub const SCHEMA_VERSION: u32 = 1;

const SCHEMA_VERSION_KEY: &str = "schema_version";

/// Opens a vault database, rebuilding the derived tables if this build reads
/// a different schema than the one that wrote them.
///
/// Safe to call on every unlock: on a matching version it creates nothing and
/// replays nothing.
pub fn init_schema(conn: &Connection, vault_id: Uuid) -> CoreResult<()> {
    init_core(conn).map_err(db)?;

    let stored = stored_schema_version(conn)?;
    let stale = stored.is_some_and(|v| v != SCHEMA_VERSION);
    if stale {
        drop_derived(conn).map_err(db)?;
    }

    init_derived(conn, vault_id).map_err(db)?;

    if stale {
        // Errors here are database failures, not format problems: the log was
        // readable enough to get this far. Surfaced rather than swallowed,
        // because a half-rebuilt index must not be presented as the vault.
        crate::oplog::rebuild_derived(conn)?;
        bump_revision(conn).map_err(db)?;
    }

    conn.execute(
        "INSERT INTO vault_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![SCHEMA_VERSION_KEY, SCHEMA_VERSION.to_string()],
    )
    .map_err(db)?;
    Ok(())
}

fn stored_schema_version(conn: &Connection) -> CoreResult<Option<u32>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM vault_meta WHERE key = ?1",
            [SCHEMA_VERSION_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(db)?;
    Ok(raw.and_then(|s| s.parse().ok()))
}

/// The two tables that are not reconstructible from anything else: the log,
/// and the handful of local facts that are not in it (this device's id, its
/// Lamport clock, the vault id).
fn init_core(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS vault_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    crate::oplog::init_oplog_core(conn)?;
    // Core, not derived: the base snapshot is the one thing here that a
    // replay cannot produce, because it stands in for the records a replay
    // no longer has.
    crate::snapshot::init_base(conn)?;
    // Also core, and also not derived: what a sweep saw in a bucket last time
    // cannot be worked out again from the log.
    crate::snapshot::init_gc(conn)?;
    // Also core: which targets have received which records is in flight,
    // not derived, and losing it would re-send everything to everyone.
    crate::oplog::init_delivery(conn)
}

/// Drops every table [`init_derived`] creates, children first so the foreign
/// keys hold (the connection runs with `PRAGMA foreign_keys=ON`).
pub(crate) fn drop_derived(conn: &Connection) -> rusqlite::Result<()> {
    crate::oplog::drop_oplog_derived(conn)?;
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS passwords;
        DROP TABLE IF EXISTS files;
        DROP TABLE IF EXISTS folders;
        ",
    )
}

/// Lets the oplog tests simulate a build whose derived schema is gone, which
/// is the situation [`init_schema`] recovers from in the field.
#[cfg(test)]
pub(crate) fn drop_derived_for_test(conn: &Connection) -> rusqlite::Result<()> {
    drop_derived(conn)
}

pub(crate) fn init_derived(conn: &Connection, vault_id: Uuid) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS folders (
            id          TEXT PRIMARY KEY,
            parent_id   TEXT REFERENCES folders(id),
            name        TEXT NOT NULL,
            path        TEXT NOT NULL UNIQUE,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            deleted_at  INTEGER,
            favorite    INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS files (
            id           TEXT PRIMARY KEY,
            folder_id    TEXT NOT NULL REFERENCES folders(id),
            name         TEXT NOT NULL,
            blob_id      TEXT NOT NULL,
            size_bytes   INTEGER NOT NULL,
            mime_type    TEXT,
            content_hash TEXT,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL,
            deleted_at   INTEGER,
            favorite     INTEGER NOT NULL DEFAULT 0,
            -- Which record put this content here. Kept on the row rather
            -- than looked up in the log, because after compaction that
            -- record may be gone, and a concurrent edit is exactly when it
            -- is needed. See oplog::conflict_copy.
            content_op_id     TEXT,
            content_lamport   INTEGER NOT NULL DEFAULT 0,
            content_device_id TEXT,
            -- This content's key, wrapped under the vault DEK. The only
            -- copy: content is encrypted under a key of its own, and losing
            -- this row's value means losing the file. It is derived, like
            -- everything else here, because the record that created it
            -- carries the same value and a replay puts it back.
            blob_key          TEXT,
            UNIQUE(folder_id, name)
        );

        -- The password store. Rows rather than a file in the tree: a login
        -- has no name to claim, no folder to live in and no business being
        -- listed next to the user's documents. Keeping it here also makes
        -- sync per entry, so two devices adding different logins offline
        -- both keep them, which a single rewritten blob cannot do.
        --
        -- `data` is the whole entry, sealed with the vault key and base64
        -- encoded. Sealed per row rather than relying on the encryption of
        -- the database as a whole: the working copy of vault.db is plaintext
        -- on disk while the silo is unlocked, which is a reasonable place
        -- for file names and not one for credentials.
        --
        -- Splitting the entry into columns would buy indexing on exactly the
        -- fields worth not spreading around, and the panel loads everything
        -- anyway to search it in memory.
        CREATE TABLE IF NOT EXISTS passwords (
            id         TEXT PRIMARY KEY,
            data       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_id) WHERE deleted_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_files_folder ON files(folder_id) WHERE deleted_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_folders_path ON folders(path);
        CREATE INDEX IF NOT EXISTS idx_folders_favorite ON folders(favorite) WHERE favorite = 1 AND deleted_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_files_favorite ON files(favorite) WHERE favorite = 1 AND deleted_at IS NULL;
        ",
    )?;

    crate::oplog::init_oplog_derived(conn)?;

    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT OR IGNORE INTO vault_meta(key, value) VALUES ('vault_id', ?1)",
        [vault_id.to_string()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO vault_meta(key, value) VALUES ('revision', '0')",
        [],
    )?;

    let root_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM folders WHERE path = '/'", [], |row| {
            row.get(0)
        })?;

    if root_count == 0 {
        let root_id = root_folder_id_for(vault_id);
        conn.execute(
            "INSERT INTO folders(id, parent_id, name, path, created_at, updated_at)
             VALUES (?1, NULL, '', '/', ?2, ?2)",
            rusqlite::params![root_id.to_string(), now],
        )?;
    }

    Ok(())
}

/// The root folder's id, derived from the vault id rather than generated.
///
/// Every device that opens the same vault must arrive at the same root id:
/// operations reference folders by id, so a randomly-generated root would
/// leave a second device unable to resolve anything the first one created.
/// Deriving it also lets a brand-new device bootstrap purely from the
/// operation log, with no snapshot to seed the tree first.
pub fn root_folder_id_for(vault_id: Uuid) -> Uuid {
    Uuid::new_v5(&vault_id, b"silentsilo-root-folder")
}

pub fn bump_revision(conn: &Connection) -> rusqlite::Result<i64> {
    conn.execute(
        "UPDATE vault_meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'revision'",
        [],
    )?;
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM vault_meta WHERE key = 'revision'",
        [],
        |row| row.get(0),
    )
}
