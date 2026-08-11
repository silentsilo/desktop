//! Moves the operation log between a vault and the user's bucket.
//!
//! The model lives in `silentsilo-vfs::oplog`; this crate is only transport.
//! Every operation becomes one immutable object under `ops/`, so a push is
//! always a create and never an overwrite — which is what lets this work on
//! providers with no conditional-write support, and why two devices pushing
//! at the same moment cannot lose each other's work.
//!
//! Records are sealed with the vault DEK before they leave the machine: an
//! operation carries file and folder names, so the storage provider must see
//! ciphertext, exactly as it does for blob content.

use silentsilo_core::CoreError;
use silentsilo_crypto::{MasterDek, seal, unseal};
use silentsilo_store::{ObjectStore, StoreError};
use silentsilo_vfs::{
    CompactionPolicy, OpRecord, ReplayReport, Snapshot, capture_at, choose_horizon,
    highest_applied_lamport, replay,
};

use rusqlite::Connection;
use silentsilo_vault::{
    RecoveryEnvelope, StoredFidoCredential, StoredFidoKeys, list_undelivered_blob_ids,
    record_blob_delivered, record_blob_present, remove_blob_from_cache,
};
use std::path::Path;
use uuid::Uuid;

mod error;
pub use error::SyncError;

/// Where operation objects live inside the vault prefix.
pub const OPS_PREFIX: &str = "ops/";

/// Key for one record. `.op` keeps the listing readable and leaves room for
/// other object kinds under the same prefix later.
fn op_key(record: &OpRecord) -> String {
    format!("{OPS_PREFIX}{}.op", record.object_key())
}

/// Reads the Lamport counter back out of an object key, so records already
/// applied can be skipped without downloading and decrypting them.
fn lamport_from_key(key: &str) -> Option<u64> {
    key.strip_prefix(OPS_PREFIX)?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// What a sync pass did.
#[derive(Debug, Default, Clone)]
pub struct SyncOutcome {
    pub pushed: usize,
    pub fetched: usize,
    pub replay: ReplayReport,
}

/// Uploads records that aren't in the bucket yet.
///
/// Push before pull, always: a record that exists only locally is the one
/// thing here with no other copy, so it should reach the bucket before
/// anything else can go wrong.
pub async fn push_ops(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    records: &[OpRecord],
) -> Result<usize, SyncError> {
    let mut pushed = 0;
    for record in records {
        let key = op_key(record);
        // Records are immutable, so anything already up there is identical
        // and re-uploading it would only cost bandwidth.
        if client.head(&key).await?.is_some() {
            continue;
        }
        let payload = seal(&record.to_bytes()?, dek)?;
        client.put(&key, payload).await?;
        pushed += 1;
    }
    Ok(pushed)
}

/// Downloads every record at or above `from_lamport`. The bound is
/// **inclusive** because Lamport values are not unique across devices: two
/// devices can both sit at 7, and an exclusive bound would make each skip
/// the other's record forever. Re-applying the boundary is a no-op.
pub async fn fetch_ops_from(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    from_lamport: u64,
) -> Result<Vec<OpRecord>, SyncError> {
    fetch_ops_reporting(client, dek, from_lamport, &mut |_, _| {}).await
}

/// The same, calling back with `(done, total)` as each record lands, so a
/// long join can show progress. A callback rather than an event: this crate
/// is transport and knows nothing about the app it runs in.
pub async fn fetch_ops_reporting(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    from_lamport: u64,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> Result<Vec<OpRecord>, SyncError> {
    let listing: Vec<_> = client
        .list(OPS_PREFIX)
        .await?
        .into_iter()
        .filter(|entry| match lamport_from_key(&entry.key) {
            Some(lamport) => lamport >= from_lamport,
            // An unparseable key is something this version doesn't
            // understand. Kept rather than skipped: silently ignoring
            // objects would mean silently losing changes.
            None => true,
        })
        .collect();

    let total = listing.len();
    let mut records = Vec::with_capacity(total);
    progress(0, total);

    for entry in listing {
        if entry.size > MAX_OP_BYTES {
            return Err(SyncError::Vault(format!(
                "{} is {} bytes, which is far larger than any operation record: \
                 refusing to read it",
                entry.key, entry.size
            )));
        }
        let sealed = client.get(&entry.key).await?;
        records.push(OpRecord::from_bytes(&unseal(&sealed, dek)?)?);
        progress(records.len(), total);
    }
    Ok(records)
}

/// The most an operation record may weigh before this refuses to read it.
/// A megabyte is far above anything the app writes, and the ceiling stops a
/// hostile provider from answering a listing with an object sized to
/// exhaust memory. Checked against the listing, so an oversized object is
/// never downloaded. Blobs get no ceiling: their size is whatever the user
/// stored.
const MAX_OP_BYTES: i64 = 1024 * 1024;

/// One full pass: push what is local-only, pull what is new, replay it.
///
/// Replaying a contiguous run above the local high-water mark is exactly the
/// condition the operation log's convergence guarantee needs — see the
/// `oplog` module docs.
pub async fn sync_ops(
    conn: &Connection,
    client: &dyn ObjectStore,
    dek: &MasterDek,
    pending: &[OpRecord],
) -> Result<SyncOutcome, SyncError> {
    let applied_through = highest_applied_lamport(conn)?;
    check_horizon(client, applied_through).await?;

    let pushed = push_ops(client, dek, pending).await?;
    let incoming = fetch_ops_from(client, dek, applied_through).await?;
    let fetched = incoming.len();
    let report = replay(conn, incoming)?;

    Ok(SyncOutcome {
        pushed,
        fetched,
        replay: report,
    })
}

/// Refuses to sync a device that has fallen below the snapshot horizon:
/// it is about to be rebuilt, and pushing its pending records first would
/// put changes into the bucket that nobody would ever apply. The bound is
/// `applied_through <= horizon`, deliberately one record strict: Lamport
/// values are not unique across devices, so a device resting exactly on the
/// horizon may still be missing records at it, and those objects can no
/// longer be re-fetched once pruned.
/// The lowest horizon across every target: the right question is whether
/// *anywhere* still has what this device is missing, not whether the most
/// compacted target does. Zero means never compacted, the strongest answer.
/// An unreachable target is skipped rather than treated as zero, or a
/// device would convince itself it is fine and silently miss records.
pub async fn lowest_snapshot_horizon(clients: &[&dyn ObjectStore]) -> Result<u64, SyncError> {
    let mut lowest: Option<u64> = None;
    let mut reached = 0;

    for client in clients {
        let Ok(horizon) = snapshot_horizon(*client).await else {
            continue;
        };
        reached += 1;
        lowest = Some(match lowest {
            Some(current) => current.min(horizon),
            None => horizon,
        });
    }

    if reached == 0 {
        // Nothing answered. Reporting zero would say "no target has been
        // compacted", which is a claim, not an observation.
        return Err(SyncError::Storage(
            "no backup target could be reached".into(),
        ));
    }
    Ok(lowest.unwrap_or(0))
}

async fn check_horizon(client: &dyn ObjectStore, applied_through: u64) -> Result<(), SyncError> {
    let horizon = lowest_snapshot_horizon(&[client]).await?;
    if horizon > 0 && applied_through <= horizon {
        return Err(SyncError::BehindHorizon {
            applied_through,
            horizon,
        });
    }
    Ok(())
}

/// Rebuilds this device from the current state of the silo: the answer to
/// [`SyncError::BehindHorizon`], and how a device joins a compacted silo.
/// The device comes back with a new id;
/// [`silentsilo_vfs::snapshot::rebootstrap`] explains why.
/// For callers that can hold the vault connection across an await. The app
/// cannot, and uses the two phases below directly.
pub async fn rebootstrap_from_snapshot(
    conn: &mut Connection,
    client: &dyn ObjectStore,
    dek: &MasterDek,
) -> Result<RebootstrapOutcome, SyncError> {
    let (snapshot, incoming) = fetch_rebuild(client, dek)
        .await?
        .ok_or_else(|| SyncError::Vault("this silo has no snapshot to rebuild from".into()))?;
    apply_rebuild(conn, &snapshot, incoming)
}

/// Step one: download the state and everything that has happened since.
/// Touches only storage. `None` means never compacted: the whole log is
/// still there and an ordinary replay from the beginning is the way in.
pub async fn fetch_rebuild(
    client: &dyn ObjectStore,
    dek: &MasterDek,
) -> Result<Option<(Snapshot, Vec<OpRecord>)>, SyncError> {
    fetch_rebuild_reporting(client, dek, &mut |_, _| {}).await
}

/// The same, calling back with `(done, total)` while the log above the
/// horizon comes down.
pub async fn fetch_rebuild_reporting(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> Result<Option<(Snapshot, Vec<OpRecord>)>, SyncError> {
    let Some(snapshot) = latest_snapshot(client, dek).await? else {
        return Ok(None);
    };
    // Everything above the horizon, which is the whole of what the bucket
    // still holds under `ops/`.
    let incoming = fetch_ops_reporting(client, dek, snapshot.horizon + 1, progress).await?;
    Ok(Some((snapshot, incoming)))
}

/// Step two: make it this device's state. Touches only the database.
pub fn apply_rebuild(
    conn: &mut Connection,
    snapshot: &Snapshot,
    incoming: Vec<OpRecord>,
) -> Result<RebootstrapOutcome, SyncError> {
    let fetched = incoming.len();
    let device_id = silentsilo_vfs::snapshot::rebootstrap(conn, snapshot)?;
    let replay_report = replay(conn, incoming)?;

    Ok(RebootstrapOutcome {
        horizon: snapshot.horizon,
        device_id,
        fetched,
        replay: replay_report,
    })
}

/// Compacts the silo if the log has grown enough to be worth it. Run
/// straight after a successful [`sync_ops`] and from nowhere else: the
/// device must hold the whole log before declaring records deletable. The
/// order only goes one way: capture the state, put the snapshot in the
/// bucket, delete the records it covers, prune the local log. Stopping
/// between any two steps leaves a silo that still works; deleting before
/// the snapshot is up leaves one nobody can join.
/// `now` comes from the caller so the decision stays testable. For callers
/// that can hold the vault connection across an await; the app cannot, and
/// uses the phases below directly.
pub async fn compact_if_due(
    conn: &mut Connection,
    client: &dyn ObjectStore,
    dek: &MasterDek,
    vault_id: Uuid,
    policy: &CompactionPolicy,
    now: i64,
) -> Result<Option<CompactionOutcome>, SyncError> {
    let Some(snapshot) = plan_compaction(conn, vault_id, policy, now)? else {
        return Ok(None);
    };
    // One target, and it is the one the caller chose to keep tidy, so it
    // prunes. The role-aware path is the multi-target one in the app.
    let published = publish_compaction(client, dek, &snapshot, true).await?;
    let pruned_local = finish_compaction(conn, &snapshot)?;

    Ok(Some(CompactionOutcome {
        horizon: snapshot.horizon,
        pruned_remote: published.deleted,
        pruned_local: pruned_local.dropped,
        stranded: pruned_local.stranded,
    }))
}

/// Step one: decide, and capture the state. Touches only the database.
///
/// `None` when the log should be left alone, which is the answer almost every
/// time it is asked.
pub fn plan_compaction(
    conn: &Connection,
    vault_id: Uuid,
    policy: &CompactionPolicy,
    now: i64,
) -> Result<Option<Snapshot>, SyncError> {
    match choose_horizon(conn, policy, now)? {
        None => Ok(None),
        Some(horizon) => Ok(Some(capture_at(conn, vault_id, horizon)?)),
    }
}

/// Step two: publish the snapshot, then delete what it covers. Touches only
/// storage. Takes the snapshot by reference rather than a horizon, so it
/// cannot be called before one was captured: deleting records before their
/// replacement is readable leaves a silo nobody can join.
/// `prune` is false for an append-only target: the snapshot still goes up
/// (a PUT on a fresh key), but the delete afterwards cannot happen, so that
/// target keeps the whole log for ever.
pub async fn publish_compaction(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    snapshot: &Snapshot,
    prune: bool,
) -> Result<PruneOutcome, SyncError> {
    put_snapshot(client, dek, snapshot).await?;
    if !prune {
        return Ok(PruneOutcome::default());
    }
    prune_ops_below(client, snapshot.horizon).await
}

/// Step three: record the snapshot locally and drop the records it covers.
///
/// Safe to skip if the process dies first: the device simply keeps a log it
/// no longer strictly needs, and the next pass proposes the same work again.
pub fn finish_compaction(
    conn: &mut Connection,
    snapshot: &Snapshot,
) -> Result<silentsilo_vfs::PruneReport, SyncError> {
    Ok(silentsilo_vfs::snapshot::compact_local(conn, snapshot)?)
}

/// What a compaction pass did.
#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    pub horizon: u64,
    pub pruned_remote: usize,
    pub pruned_local: usize,
    /// Local records below the horizon that the bucket never confirmed. They
    /// are kept, and they are a problem: they belong below a line every other
    /// device is about to compact past. Surfaced rather than swallowed.
    pub stranded: usize,
}

/// What a re-bootstrap rebuilt.
#[derive(Debug, Clone)]
pub struct RebootstrapOutcome {
    /// The state the device restarted from.
    pub horizon: u64,
    /// Its new identity. Worth surfacing: the Devices list will show a new
    /// row, and the old one stops changing.
    pub device_id: Uuid,
    pub fetched: usize,
    pub replay: ReplayReport,
}

// ── Blob content ────────────────────────────────────────────────────

/// Where file content lives inside the vault prefix.
pub const BLOBS_PREFIX: &str = "blobs/";

fn blob_key(blob_id: Uuid) -> String {
    format!("{BLOBS_PREFIX}{blob_id}.sslo")
}

/// Result of a blob push pass.
#[derive(Debug, Default, Clone)]
pub struct BlobPushOutcome {
    pub uploaded: usize,
    pub already_present: usize,
    /// Blobs that could not be uploaded, with why. A failure here is not
    /// fatal to the pass: the rest still go, and these stay unsynced so the
    /// next attempt retries them.
    pub failed: Vec<(Uuid, String)>,
}

/// Uploads every blob on this device that isn't confirmed in the bucket.
/// Blobs are already ciphertext on disk and their keys are immutable, so
/// this is a byte-for-byte transfer with nothing to merge.
pub async fn push_blobs(
    client: &dyn ObjectStore,
    vault_root: &Path,
    target: Uuid,
    blob_ids: &[Uuid],
) -> Result<BlobPushOutcome, SyncError> {
    let mut outcome = BlobPushOutcome::default();

    for blob_id in blob_ids {
        let path = vault_root.join("blobs").join(format!("{blob_id}.sslo"));
        if !path.is_file() {
            // Already evicted, or trashed and purged between listing and
            // now. Nothing to send, and nothing to keep owing either: left
            // in place the row would be reported as an upload still pending
            // for as long as the silo exists, for content that is gone.
            let _ = remove_blob_from_cache(vault_root, *blob_id);
            continue;
        }

        let key = blob_key(*blob_id);
        match client.head(&key).await {
            Ok(Some(_)) => {
                // Already up there. Record that so eviction is allowed to
                // reclaim the space — without this the local copy would be
                // pinned forever.
                record_blob_delivered(vault_root, *blob_id, target)
                    .map_err(|e| SyncError::Vault(e.to_string()))?;
                outcome.already_present += 1;
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                outcome.failed.push((*blob_id, e.to_string()));
                continue;
            }
        }

        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                outcome.failed.push((*blob_id, e.to_string()));
                continue;
            }
        };

        match client.put(&key, bytes).await {
            Ok(()) => {
                // Only now: marking a blob synced before the upload lands
                // would make it evictable while the bucket has no copy,
                // which is how you lose a file for good.
                record_blob_delivered(vault_root, *blob_id, target)
                    .map_err(|e| SyncError::Vault(e.to_string()))?;
                outcome.uploaded += 1;
            }
            Err(e) => outcome.failed.push((*blob_id, e.to_string())),
        }
    }

    Ok(outcome)
}

// ── Seeding one target from another ─────────────────────────────────

/// What a seeding pass moved.
#[derive(Debug, Default, Clone)]
pub struct SeedOutcome {
    pub copied: usize,
    /// Objects the destination already held, at the same size. Skipped
    /// rather than rewritten, which is what makes a seed resumable: run it
    /// again after a cable is pulled and it carries on.
    pub skipped: usize,
    pub bytes: u64,
    /// What would not copy, with why. Reported rather than raised: one
    /// unreadable object should not throw away the several hundred gigabytes
    /// that did move.
    pub failed: Vec<(String, String)>,
}

/// Everything a silo keeps in its storage, in the order it is worth
/// copying: content and records first, the manifest last, so an interrupted
/// copy never looks like a silo that lost its history.
const SEED_PREFIXES: [&str; 4] = [BLOBS_PREFIX, OPS_PREFIX, SNAPSHOTS_PREFIX, KEYS_PREFIX];

/// Copies one target's contents into another, on ciphertext throughout: no
/// key needed. Both directions are the same operation. `cancel` is asked
/// between objects; stopping mid-seed is safe, the next run skips what
/// already landed.
pub async fn seed_target(
    from: &dyn ObjectStore,
    to: &dyn ObjectStore,
    progress: &mut (dyn FnMut(usize, usize) + Send),
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<SeedOutcome, SyncError> {
    let mut outcome = SeedOutcome::default();

    let mut work = Vec::new();
    for prefix in SEED_PREFIXES {
        work.extend(from.list(prefix).await?);
    }
    // The manifest is one fixed key rather than a prefix, and it goes last.
    let manifest_size = from.head(MANIFEST_KEY).await?;

    let total = work.len() + usize::from(manifest_size.is_some());
    progress(0, total);
    let mut done = 0;

    for entry in work {
        if cancel() {
            return Err(SyncError::Cancelled);
        }
        // Already there at the same size is treated as already there. A
        // deeper check would mean downloading both copies of every object,
        // which for the volumes this exists to move is the thing being
        // avoided. Records and blobs are written once to unique keys, so
        // "same key, same size" is a strong statement about them.
        if to.head(&entry.key).await? == Some(entry.size) {
            outcome.skipped += 1;
            done += 1;
            progress(done, total);
            continue;
        }
        match from.get(&entry.key).await {
            Ok(bytes) => {
                let len = bytes.len() as u64;
                match to.put(&entry.key, bytes).await {
                    Ok(()) => {
                        outcome.copied += 1;
                        outcome.bytes += len;
                    }
                    Err(e) => outcome.failed.push((entry.key.clone(), e.to_string())),
                }
            }
            Err(e) => outcome.failed.push((entry.key.clone(), e.to_string())),
        }
        done += 1;
        progress(done, total);
    }

    if manifest_size.is_some() {
        match from.get(MANIFEST_KEY).await {
            Ok(bytes) => match to.put(MANIFEST_KEY, bytes).await {
                Ok(()) => outcome.copied += 1,
                Err(e) => outcome
                    .failed
                    .push((MANIFEST_KEY.to_string(), e.to_string())),
            },
            Err(e) => outcome
                .failed
                .push((MANIFEST_KEY.to_string(), e.to_string())),
        }
        done += 1;
        progress(done, total);
    }

    Ok(outcome)
}

// ── Checking a silo against its storage ─────────────────────────────

/// What a scrub found.
///
/// Counts of what was looked at, and a list of what was wrong. The list is the
/// answer; the counts are what makes an empty list mean something, since
/// "nothing wrong" and "nothing checked" read identically otherwise.
#[derive(Debug, Default, Clone)]
pub struct VerifyReport {
    pub records_read: usize,
    pub blobs_checked: usize,
    /// Bytes of content actually read back and authenticated. Zero on a check
    /// that only looked for presence.
    pub bytes_read: u64,
    /// Referenced content storage does not have. The worst finding here: the
    /// silo believes it holds these files and it does not.
    pub missing: Vec<Uuid>,
    /// Objects that are there but wrong, with why. Bit rot, a partial upload,
    /// or something rewritten underneath.
    pub damaged: Vec<(String, String)>,
    /// Content in storage that nothing references. Not a fault: a sweep
    /// clears these, and one that arrived ahead of its record is normal.
    pub unreferenced: usize,
}

impl VerifyReport {
    /// Nothing wrong, as opposed to nothing found.
    pub fn is_sound(&self) -> bool {
        self.missing.is_empty() && self.damaged.is_empty()
    }
}

/// How hard to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyDepth {
    /// Reads every operation record and checks that each referenced blob is
    /// present. No content is downloaded, so any silo finishes in about the
    /// time a listing takes.
    Listing,
    /// Also reads back every referenced blob and authenticates it. Catches
    /// bit rot, which nothing else does, and costs a download of the silo.
    Content,
}

/// Reads a silo's storage and reports what is wrong with it. `expected` is
/// what the local index says should be there: storage agreeing with itself
/// proves nothing. `open` turns a blob id into its content key, passed in
/// because this crate moves ciphertext and knows no key but the DEK.
pub async fn verify_against(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    expected: &std::collections::HashSet<Uuid>,
    depth: VerifyDepth,
    open: &mut (dyn FnMut(Uuid) -> Option<silentsilo_crypto::ContentKey> + Send),
    progress: &mut (dyn FnMut(usize, usize) + Send),
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<VerifyReport, SyncError> {
    let mut report = VerifyReport::default();

    let ops = client.list(OPS_PREFIX).await?;
    let blobs = client.list(BLOBS_PREFIX).await?;
    let total = ops.len() + blobs.len();
    let mut done = 0;
    progress(0, total);

    // Every record, opened. Unsealing is the check: the envelope is
    // authenticated, so a record that has been altered fails here rather than
    // replaying into a tree that is quietly wrong.
    for entry in ops {
        if cancel() {
            return Err(SyncError::Cancelled);
        }
        match client.get(&entry.key).await {
            Ok(sealed) => match unseal(&sealed, dek) {
                Ok(plain) => match OpRecord::from_bytes(&plain) {
                    Ok(_) => report.records_read += 1,
                    Err(e) => report.damaged.push((entry.key.clone(), e.to_string())),
                },
                Err(_) => report.damaged.push((
                    entry.key.clone(),
                    "does not open with this silo's key".to_string(),
                )),
            },
            Err(e) => report.damaged.push((entry.key.clone(), e.to_string())),
        }
        done += 1;
        progress(done, total);
    }

    let mut seen: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for entry in blobs {
        if cancel() {
            return Err(SyncError::Cancelled);
        }
        done += 1;

        let Some(blob_id) = blob_id_from_key(&entry.key) else {
            // A key this build does not understand. Counted as unreferenced
            // rather than damaged: not knowing what something is is not the
            // same as knowing it is broken.
            report.unreferenced += 1;
            progress(done, total);
            continue;
        };
        seen.insert(blob_id);

        if !expected.contains(&blob_id) {
            report.unreferenced += 1;
            progress(done, total);
            continue;
        }

        if entry.size <= 0 {
            // An upload that created the object and stopped. Worth catching
            // without downloading anything.
            report
                .damaged
                .push((entry.key.clone(), "the object is empty".to_string()));
        } else if depth == VerifyDepth::Content {
            match open(blob_id) {
                Some(key) => match verify_one(client, &entry.key, &key, blob_id).await {
                    Ok(read) => {
                        report.blobs_checked += 1;
                        report.bytes_read += read;
                    }
                    Err(e) => report.damaged.push((entry.key.clone(), e)),
                },
                None => report.damaged.push((
                    entry.key.clone(),
                    "no content key is recorded for this file".to_string(),
                )),
            }
        } else {
            report.blobs_checked += 1;
        }

        progress(done, total);
    }

    // Content the silo believes it has. Collected last so a missing object,
    // which is the finding that matters most, is not buried among the ones
    // that are merely odd.
    for blob_id in expected {
        if !seen.contains(blob_id) {
            report.missing.push(*blob_id);
        }
    }
    report.missing.sort();

    Ok(report)
}

/// Downloads one blob and authenticates it, returning the bytes read.
///
/// Written to a temporary file rather than checked in memory, because a blob
/// is whatever size the user stored and reading ten gigabytes into a buffer
/// would be a poor way to find out a disk is fine.
async fn verify_one(
    client: &dyn ObjectStore,
    key: &str,
    content_key: &silentsilo_crypto::ContentKey,
    blob_id: Uuid,
) -> Result<u64, String> {
    let bytes = client.get(key).await.map_err(|e| e.to_string())?;
    let read = bytes.len() as u64;

    let dir = tempfile::Builder::new()
        .prefix("silentsilo-verify")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = dir.path().join("blob.sslo");
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;

    silentsilo_crypto::verify_blob(&path, content_key, blob_id).map_err(|e| e.to_string())?;
    Ok(read)
}

// ── Rotating the vault key ──────────────────────────────────────────

/// What a re-sealing pass did.
#[derive(Debug, Default, Clone)]
pub struct ResealOutcome {
    pub resealed: usize,
    /// Objects that already opened under the new key, so a pass that was
    /// interrupted picks up where it stopped rather than starting over.
    pub already: usize,
    /// Objects that opened under neither key, with why. Reported rather than
    /// raised: one unreadable object must not abandon a rotation halfway,
    /// which is the state with no good way out.
    pub failed: Vec<(String, String)>,
}

/// Everything in storage that is sealed under the vault DEK.
///
/// Key envelopes are not here: they are wrapped under a credential's key, not
/// the DEK, and rotation rewrites them by a different route. The manifest is
/// plain JSON naming a random id and has nothing to re-seal.
const SEALED_PREFIXES: [&str; 2] = [OPS_PREFIX, SNAPSHOTS_PREFIX];

/// Re-seals every object under the new vault key, leaving their contents
/// exactly as they were: fingerprints hold and the log's chain stays
/// intact. Safe to interrupt and safe to run twice; each object is skipped
/// when it already opens under the new key, and the write happens only
/// after a successful read, so nothing is ever unreadable by both keys.
pub async fn reseal_under_new_key(
    client: &dyn ObjectStore,
    old: &MasterDek,
    new: &MasterDek,
    progress: &mut (dyn FnMut(usize, usize) + Send),
) -> Result<ResealOutcome, SyncError> {
    let mut outcome = ResealOutcome::default();

    let mut work = Vec::new();
    for prefix in SEALED_PREFIXES {
        work.extend(
            client
                .list(prefix)
                .await?
                .into_iter()
                .map(|entry| entry.key),
        );
    }
    // The KEK envelope is sealed under the DEK like the records, and it is
    // the one object that must arrive: without it a joining device has no
    // way to open any content at all.
    if client.head(CONTENT_KEK_KEY).await?.is_some() {
        work.push(CONTENT_KEK_KEY.to_string());
    }

    let total = work.len();
    progress(0, total);

    for (done, key) in work.into_iter().enumerate() {
        let sealed = match client.get(&key).await {
            Ok(bytes) => bytes,
            Err(e) => {
                outcome.failed.push((key, e.to_string()));
                progress(done + 1, total);
                continue;
            }
        };

        // Already done on an earlier pass. Checked by trying the new key
        // rather than by keeping a list, because the object itself is the
        // only record that cannot go out of step with the truth.
        if unseal(&sealed, new).is_ok() {
            outcome.already += 1;
            progress(done + 1, total);
            continue;
        }

        match unseal(&sealed, old) {
            Ok(plain) => match seal(&plain, new) {
                Ok(resealed) => match client.put(&key, resealed).await {
                    Ok(()) => outcome.resealed += 1,
                    Err(e) => outcome.failed.push((key, e.to_string())),
                },
                Err(e) => outcome.failed.push((key, e.to_string())),
            },
            Err(_) => outcome.failed.push((
                key,
                "opens under neither the old key nor the new one".to_string(),
            )),
        }
        progress(done + 1, total);
    }

    Ok(outcome)
}

/// Uploads everything this device holds that the bucket does not.
pub async fn push_pending_blobs(
    client: &dyn ObjectStore,
    vault_root: &Path,
    target: Uuid,
) -> Result<BlobPushOutcome, SyncError> {
    let pending = list_undelivered_blob_ids(vault_root, target);
    push_blobs(client, vault_root, target, &pending).await
}

/// One blob's sealed bytes, without a cache to put them in. For the
/// extraction tool, which writes plaintext straight out and must not leave
/// a half-silo on the machine someone is recovering from.
pub async fn fetch_blob_bytes(
    client: &dyn ObjectStore,
    blob_id: Uuid,
) -> Result<Vec<u8>, SyncError> {
    Ok(client.get(&blob_key(blob_id)).await?)
}

/// Downloads one blob into the local cache.
///
/// Called when a file is opened whose content isn't here — either because
/// another device uploaded it, or because eviction reclaimed the space.
pub async fn fetch_blob(
    client: &dyn ObjectStore,
    vault_root: &Path,
    blob_id: Uuid,
) -> Result<u64, SyncError> {
    let bytes = client.get(&blob_key(blob_id)).await?;
    let blobs_dir = vault_root.join("blobs");
    std::fs::create_dir_all(&blobs_dir).map_err(|e| SyncError::Vault(e.to_string()))?;

    // Write to a temp name and rename, so an interrupted download can't
    // leave a truncated file that later reads as corrupt ciphertext.
    let final_path = blobs_dir.join(format!("{blob_id}.sslo"));
    let temp_path = blobs_dir.join(format!("{blob_id}.sslo.part"));
    std::fs::write(&temp_path, &bytes).map_err(|e| SyncError::Vault(e.to_string()))?;
    std::fs::rename(&temp_path, &final_path).map_err(|e| SyncError::Vault(e.to_string()))?;

    let size = bytes.len() as i64;
    // Not marked evictable here, even though it plainly exists on the
    // target it came from: eviction needs every target to hold it, and this
    // call does not know the others. The next push settles that with a HEAD
    // per target, and holding the local copy until then errs the safe way.
    record_blob_present(vault_root, blob_id, size, false)
        .map_err(|e| SyncError::Vault(e.to_string()))?;
    Ok(size as u64)
}

/// Removes a blob from the bucket. Used when the trash is emptied.
///
/// Best-effort by design: an orphaned object costs storage, while failing
/// the purge would leave the vault index and the bucket disagreeing.
pub async fn delete_blob(client: &dyn ObjectStore, blob_id: Uuid) -> Result<(), SyncError> {
    client.delete(&blob_key(blob_id)).await?;
    Ok(())
}

// ── Snapshots ───────────────────────────────────────────────────────

/// Where compaction writes the state of the vault.
pub const SNAPSHOTS_PREFIX: &str = "snapshots/";

/// Zero-padded like op keys, so a listing comes back oldest first and the
/// newest snapshot is the last entry.
fn snapshot_key(horizon: u64) -> String {
    format!("{SNAPSHOTS_PREFIX}{horizon:020}.snap")
}

fn horizon_from_key(key: &str) -> Option<u64> {
    key.strip_prefix(SNAPSHOTS_PREFIX)?
        .strip_suffix(".snap")?
        .parse()
        .ok()
}

/// Publishes the state of the vault at a horizon, sealed with the vault
/// DEK. Write this **before** deleting anything it covers, or there is a
/// window in which the silo cannot be joined.
pub async fn put_snapshot(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    snapshot: &Snapshot,
) -> Result<(), SyncError> {
    let payload = seal(&snapshot.to_bytes()?, dek)?;
    client.put(&snapshot_key(snapshot.horizon), payload).await?;
    Ok(())
}

/// Publishes the local base snapshot unless this target already has one at
/// that horizon. Must run before the first push to a target, or a locally
/// compacted log starts mid-stream in the bucket.
pub async fn publish_base_if_missing(
    client: &dyn ObjectStore,
    dek: &MasterDek,
    base: &Snapshot,
) -> Result<bool, SyncError> {
    if client.head(&snapshot_key(base.horizon)).await?.is_some() {
        return Ok(false);
    }
    put_snapshot(client, dek, base).await?;
    Ok(true)
}

/// The highest horizon the bucket has a snapshot for, or 0 for none.
///
/// Answered from the listing alone, without downloading or decrypting
/// anything: every device asks this on every pass to find out whether it has
/// fallen below the horizon, and the answer is one integer.
pub async fn snapshot_horizon(client: &dyn ObjectStore) -> Result<u64, SyncError> {
    let entries = client.list(SNAPSHOTS_PREFIX).await?;
    Ok(entries
        .iter()
        .filter_map(|entry| horizon_from_key(&entry.key))
        .max()
        .unwrap_or(0))
}

/// Downloads the newest snapshot, for a device joining a compacted silo.
///
/// `None` when the silo has never been compacted, which is the normal case
/// and means the whole log is still there to replay.
pub async fn latest_snapshot(
    client: &dyn ObjectStore,
    dek: &MasterDek,
) -> Result<Option<Snapshot>, SyncError> {
    let horizon = snapshot_horizon(client).await?;
    if horizon == 0 {
        return Ok(None);
    }
    // No size ceiling here, unlike an operation record. A snapshot's honest
    // size is the size of the vault's index, so there is no bound to pick
    // that is not either useless or a limit on how large a silo may be. The
    // same reasoning as blobs, and the same fix when it matters: read it as
    // a stream rather than whole.
    let sealed = client.get(&snapshot_key(horizon)).await?;
    Ok(Some(Snapshot::from_bytes(&unseal(&sealed, dek)?)?))
}

/// Deletes the operation objects a snapshot has made redundant. Only ever
/// called after [`put_snapshot`] landed; a failure part way through leaves
/// merely redundant objects, so the pass reports rather than unwinds.
pub async fn prune_ops_below(
    client: &dyn ObjectStore,
    horizon: u64,
) -> Result<PruneOutcome, SyncError> {
    let mut outcome = PruneOutcome::default();
    for entry in client.list(OPS_PREFIX).await? {
        match lamport_from_key(&entry.key) {
            // Unparseable keys are left alone. This build does not know what
            // they are, and deleting an object nobody understands is how you
            // lose the thing a newer version was relying on.
            None => continue,
            Some(lamport) if lamport > horizon => continue,
            Some(_) => {}
        }
        match client.delete(&entry.key).await {
            Ok(()) => outcome.deleted += 1,
            Err(e) => outcome.failed.push((entry.key.clone(), e.to_string())),
        }
    }
    Ok(outcome)
}

/// What a bucket-side prune managed.
#[derive(Debug, Default, Clone)]
pub struct PruneOutcome {
    pub deleted: usize,
    /// Objects that would not delete, with why. Redundant rather than
    /// harmful, and the next pass tries them again.
    pub failed: Vec<(String, String)>,
}

// ── Collecting superseded content ───────────────────────────────────

/// What a sweep found and what it did.
#[derive(Debug, Default, Clone)]
pub struct SweepOutcome {
    pub deleted: usize,
    /// Blobs the bucket holds that nothing points at, seen this pass. Held
    /// for the next one rather than deleted now, and the caller has to store
    /// them for that to mean anything.
    pub candidates: Vec<Uuid>,
    pub failed: Vec<(Uuid, String)>,
}

/// Deletes content nothing points at any more, one pass behind: a blob
/// reaches the bucket before the record naming it, so a sweep only deletes
/// what was already unreferenced on the previous pass and still is.
/// Deleting on sight would destroy a file another device uploaded seconds
/// ago. The caller must run this from a device that has just synced, and
/// must pass the candidates from the previous pass and store the ones
/// returned.
pub async fn sweep_orphan_blobs(
    client: &dyn ObjectStore,
    referenced: &std::collections::HashSet<Uuid>,
    previous_candidates: &std::collections::HashSet<Uuid>,
) -> Result<SweepOutcome, SyncError> {
    let mut outcome = SweepOutcome::default();

    for entry in client.list(BLOBS_PREFIX).await? {
        let Some(blob_id) = blob_id_from_key(&entry.key) else {
            // Not a blob this build recognises. Left alone, like an
            // unreadable operation object.
            continue;
        };
        if referenced.contains(&blob_id) {
            continue;
        }
        if !previous_candidates.contains(&blob_id) {
            // First sighting. It may be content whose record has not landed
            // yet, so it waits a pass.
            outcome.candidates.push(blob_id);
            continue;
        }
        match client.delete(&entry.key).await {
            Ok(()) => outcome.deleted += 1,
            Err(e) => {
                outcome.failed.push((blob_id, e.to_string()));
                // Still a candidate: a delete that failed is not a delete.
                outcome.candidates.push(blob_id);
            }
        }
    }

    Ok(outcome)
}

fn blob_id_from_key(key: &str) -> Option<Uuid> {
    Uuid::parse_str(key.strip_prefix(BLOBS_PREFIX)?.strip_suffix(".sslo")?).ok()
}

// ── Key envelopes and the join manifest ─────────────────────────────

/// Wrapped DEKs, one object per enrolled security key.
pub const KEYS_PREFIX: &str = "keys/";

/// Where the content KEK's envelope lives: one object for the whole silo,
/// sealed under the vault DEK. Every device needs the same one, or files
/// written by one device could not be opened by the others.
pub const CONTENT_KEK_KEY: &str = "keys/content.kek";

/// Publishes the KEK envelope, writing only when it differs. Compared by
/// content, not presence or size: rotation re-wraps to the same length, and
/// skipping that write would leave every other device locked out.
pub async fn publish_content_kek(
    client: &dyn ObjectStore,
    envelope: &[u8],
) -> Result<bool, SyncError> {
    if client.head(CONTENT_KEK_KEY).await?.is_some()
        && client
            .get(CONTENT_KEK_KEY)
            .await
            .is_ok_and(|held| held == envelope)
    {
        return Ok(false);
    }
    client.put(CONTENT_KEK_KEY, envelope.to_vec()).await?;
    Ok(true)
}

/// The KEK envelope as stored, for a device joining the silo.
///
/// `None` when the silo has none, which a joining device must treat as a
/// refusal rather than a reason to mint one: content already there is
/// wrapped under a key this device would then never have.
pub async fn fetch_content_kek(client: &dyn ObjectStore) -> Result<Option<Vec<u8>>, SyncError> {
    if client.head(CONTENT_KEK_KEY).await?.is_none() {
        return Ok(None);
    }
    Ok(Some(client.get(CONTENT_KEK_KEY).await?))
}
/// Identifies which vault a prefix holds, for a device joining it.
pub const MANIFEST_KEY: &str = "vault.json";

/// The only object in the bucket that is not ciphertext.
///
/// It cannot be: a joining device has to know which vault it is looking at
/// *before* it can unwrap anything. A random UUID is all it discloses —
/// nothing about the contents, the owner, or how much is stored.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultManifest {
    pub vault_id: Uuid,
    /// Format version, so a future layout change can be detected rather
    /// than misread as corruption.
    pub version: u32,
}

/// The gate for the whole bucket layout, not just for `vault.json`. A
/// change that redefines what the rest of the bucket means (compaction was
/// the example) bumps this, and an older build refuses to open rather than
/// misread a partial log as the whole vault.
pub const MANIFEST_VERSION: u32 = 1;

pub async fn put_manifest(client: &dyn ObjectStore, vault_id: Uuid) -> Result<(), SyncError> {
    let manifest = VaultManifest {
        vault_id,
        version: MANIFEST_VERSION,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|e| SyncError::Vault(e.to_string()))?;
    client.put(MANIFEST_KEY, bytes).await?;
    Ok(())
}

/// Writes the manifest only when the bucket does not already say the same
/// thing: on a versioned bucket an unconditional write per pass piles up
/// thousands of versions a day of two fields.
pub async fn ensure_manifest(client: &dyn ObjectStore, vault_id: Uuid) -> Result<bool, SyncError> {
    if let Some(existing) = read_manifest(client).await?
        && existing.vault_id == vault_id
        && existing.version == MANIFEST_VERSION
    {
        return Ok(false);
    }
    put_manifest(client, vault_id).await?;
    Ok(true)
}

/// `None` when the prefix holds no vault yet — the normal state when
/// connecting a fresh bucket, not a failure.
pub async fn read_manifest(client: &dyn ObjectStore) -> Result<Option<VaultManifest>, SyncError> {
    if client.head(MANIFEST_KEY).await?.is_none() {
        return Ok(None);
    }
    let bytes = client.get(MANIFEST_KEY).await?;
    let manifest: VaultManifest =
        serde_json::from_slice(&bytes).map_err(|e| SyncError::Vault(e.to_string()))?;
    if manifest.version > MANIFEST_VERSION {
        return Err(SyncError::Vault(format!(
            "this silo was written by a newer version of SilentSilo (format {}): update to open it",
            manifest.version
        )));
    }
    Ok(Some(manifest))
}

fn key_envelope_key(credential_id: &str) -> String {
    format!("{KEYS_PREFIX}{credential_id}.env")
}

/// Uploads the wrapped DEK for each enrolled security key. The one thing
/// that cannot be sealed with the vault DEK, because it is how a device
/// obtains the DEK. The wrapped key is still ciphertext, but the credential
/// id and label are visible to the provider.
/// What a publishing pass managed to do, so the caller can retire the
/// tombstones storage has now confirmed.
#[derive(Debug, Default)]
pub struct EnvelopeReport {
    /// Envelopes actually written this pass, not enrolled keys. An envelope
    /// the bucket already holds unchanged is left alone, so a steady state
    /// reports zero.
    pub published: usize,
    /// Credential ids whose envelope is now gone from storage.
    pub revoked: Vec<String>,
    /// Credential ids this target was not asked to delete, because it is
    /// append-only. Reported rather than dropped: the envelope is still up
    /// there and still usable by anything holding that key, and calling that
    /// revocation would be a lie the user would act on.
    pub withheld: Vec<String>,
}

/// Publishes the enrolled envelopes and retires the revoked ones.
/// Deliberately not a full reconciliation against a listing of `keys/`:
/// another device may have enrolled a key this one has never heard of, and
/// deleting "everything not in my list" would revoke it. Only credentials
/// this device explicitly revoked are removed, which is what the tombstone
/// is for.
pub async fn publish_key_envelopes(
    client: &dyn ObjectStore,
    keys: &StoredFidoKeys,
    allow_delete: bool,
) -> Result<EnvelopeReport, SyncError> {
    let mut report = EnvelopeReport::default();

    for key in keys.active() {
        if key.wrapped_dek.is_empty() {
            // Nothing to publish: this credential cannot unlock anything on
            // its own, so uploading it would only mislead a joining device.
            continue;
        }
        let bytes = serde_json::to_vec(key).map_err(|e| SyncError::Vault(e.to_string()))?;
        let key_path = key_envelope_key(&key.credential_id);

        // Written only when the bytes differ, compared by content: a
        // re-wrapped DEK has exactly the same length, so a size check would
        // skip the write that matters most after a rotation.
        if client.head(&key_path).await?.is_some()
            && client.get(&key_path).await.is_ok_and(|held| held == bytes)
        {
            continue;
        }

        client.put(&key_path, bytes).await?;
        report.published += 1;
    }

    for credential_id in keys.revoked_ids() {
        // A credential that is both revoked and enrolled means the same key
        // was put back after being removed offline. Enrolment clears the
        // tombstone, so this should not happen; deleting here anyway would
        // undo the envelope published moments ago, which is worth one check.
        if keys.active().any(|k| k.credential_id == credential_id) {
            continue;
        }
        // On an append-only target the delete is not attempted at all.
        // Trying it and swallowing the refusal would read the same in the
        // report, and this way the caller can tell the two apart and say so.
        if !allow_delete {
            report.withheld.push(credential_id);
            continue;
        }
        client.delete(&key_envelope_key(&credential_id)).await?;
        report.revoked.push(credential_id);
    }

    Ok(report)
}

/// Every published key envelope, for a device joining the vault.
pub async fn fetch_key_envelopes(
    client: &dyn ObjectStore,
) -> Result<Vec<StoredFidoCredential>, SyncError> {
    let mut out = Vec::new();
    for entry in client.list(KEYS_PREFIX).await? {
        let bytes = client.get(&entry.key).await?;
        match serde_json::from_slice::<StoredFidoCredential>(&bytes) {
            Ok(credential) => out.push(credential),
            // One unreadable envelope should not stop a device joining with
            // a key whose envelope is fine.
            Err(e) => eprintln!("skipping unreadable key envelope {}: {e}", entry.key),
        }
    }
    Ok(out)
}

/// Removes a key's envelope so it can no longer unlock the vault from any
/// device.
///
/// This is what revocation means once envelopes are shared: deleting the
/// local record alone would leave the bucket copy usable by anything that
/// still has the physical key.
pub async fn revoke_key_envelope(
    client: &dyn ObjectStore,
    credential_id: &str,
) -> Result<(), SyncError> {
    client.delete(&key_envelope_key(credential_id)).await?;
    Ok(())
}

impl From<CoreError> for SyncError {
    fn from(err: CoreError) -> Self {
        SyncError::Vault(err.to_string())
    }
}

impl From<StoreError> for SyncError {
    fn from(err: StoreError) -> Self {
        SyncError::Storage(err.to_string())
    }
}

impl From<silentsilo_crypto::CryptoError> for SyncError {
    fn from(err: silentsilo_crypto::CryptoError) -> Self {
        SyncError::Crypto(err.to_string())
    }
}

// ── The recovery envelope ───────────────────────────────────────────

/// Where the recovery envelope lives in the bucket.
///
/// Beside the key envelopes rather than inside `keys/`, so listing enrolled
/// security keys never has to filter it out.
pub const RECOVERY_KEY: &str = "recovery.env";

/// Publishes the recovery envelope so the code works from any device.
///
/// This is the difference between a recovery code and a spare key: the code
/// has to work on a machine that has never seen this vault, which is the
/// situation someone is in precisely when they need it. Keeping it local
/// would make it useless in the one case it exists for.
pub async fn push_recovery_envelope(
    client: &dyn ObjectStore,
    envelope: &RecoveryEnvelope,
) -> Result<(), SyncError> {
    let bytes = serde_json::to_vec(envelope).map_err(|e| SyncError::Vault(e.to_string()))?;
    client.put(RECOVERY_KEY, bytes).await?;
    Ok(())
}

/// Publishes the recovery envelope if this target does not already have
/// it. Called on every pass so a target added after the code was generated
/// still receives it. Compared by content, so a regenerated envelope
/// replaces the old one instead of being skipped.
pub async fn ensure_recovery_envelope(
    client: &dyn ObjectStore,
    envelope: &RecoveryEnvelope,
) -> Result<bool, SyncError> {
    let bytes = serde_json::to_vec(envelope).map_err(|e| SyncError::Vault(e.to_string()))?;
    if client.head(RECOVERY_KEY).await?.is_some()
        && client
            .get(RECOVERY_KEY)
            .await
            .is_ok_and(|held| held == bytes)
    {
        return Ok(false);
    }
    client.put(RECOVERY_KEY, bytes).await?;
    Ok(true)
}

/// `None` when no recovery code has been set up for this vault.
pub async fn fetch_recovery_envelope(
    client: &dyn ObjectStore,
) -> Result<Option<RecoveryEnvelope>, SyncError> {
    if client.head(RECOVERY_KEY).await?.is_none() {
        return Ok(None);
    }
    let bytes = client.get(RECOVERY_KEY).await?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| SyncError::Vault(e.to_string()))
}

/// Removes the published envelope, so the written-down code stops working
/// everywhere rather than only on the device that revoked it.
pub async fn revoke_recovery_envelope(client: &dyn ObjectStore) -> Result<(), SyncError> {
    client.delete(RECOVERY_KEY).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_carries_its_lamport_value() {
        assert_eq!(
            lamport_from_key("ops/00000000000000000042-abc-def.op"),
            Some(42)
        );
    }

    #[test]
    fn a_key_without_the_prefix_is_not_ours() {
        assert_eq!(lamport_from_key("blobs/00000000000000000042-x.sslo"), None);
    }

    #[test]
    fn an_unparseable_key_yields_nothing_rather_than_a_wrong_number() {
        // Callers treat `None` as "fetch it anyway", so guessing here would
        // mean quietly skipping records.
        assert_eq!(lamport_from_key("ops/not-a-number-x-y.op"), None);
    }

    #[test]
    fn padding_keeps_ordering_lexicographic_across_magnitudes() {
        let mut keys = [
            "ops/00000000000000000100-a-b.op",
            "ops/00000000000000000002-a-b.op",
            "ops/00000000000000000011-a-b.op",
        ];
        keys.sort();
        let values: Vec<u64> = keys.iter().filter_map(|k| lamport_from_key(k)).collect();
        assert_eq!(values, vec![2, 11, 100]);
    }
}
