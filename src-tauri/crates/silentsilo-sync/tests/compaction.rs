//! Snapshots and pruning against a real store.
//!
//! A folder store rather than a mock: it is a genuine `ObjectStore`, it needs
//! no server, and it is what a NAS share or an external drive actually is. So
//! these run everywhere, unlike the bucket tests next door.

use silentsilo_crypto::generate_dek;
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{
    MANIFEST_VERSION, latest_snapshot, prune_ops_below, push_ops, put_manifest, put_snapshot,
    read_manifest, snapshot_horizon,
};
use silentsilo_vfs::{OpRecord, Snapshot, VaultOp};
use uuid::Uuid;

fn store() -> (tempfile::TempDir, FolderStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    (dir, store)
}

/// A snapshot with one folder in it, enough to prove it survives the trip.
fn snapshot_at(vault_id: Uuid, horizon: u64) -> Snapshot {
    Snapshot {
        version: silentsilo_vfs::SNAPSHOT_VERSION,
        vault_id,
        horizon,
        captured_at: 1_700_000_000,
        folders: vec![silentsilo_vfs::snapshot::FolderRow {
            id: Uuid::new_v4(),
            parent_id: None,
            name: String::new(),
            path: "/".into(),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            deleted_at: None,
            favorite: false,
        }],
        files: Vec::new(),
        passwords: Vec::new(),
        name_claims: Vec::new(),
        device_labels: Vec::new(),
    }
}

fn record(lamport: u64, device_id: Uuid) -> OpRecord {
    OpRecord::authored(
        Uuid::new_v4(),
        lamport,
        device_id,
        1_700_000_000,
        lamport,
        None,
        VaultOp::CreateFolder {
            id: Uuid::new_v4(),
            parent_id: Uuid::new_v4(),
            name: format!("folder-{lamport}"),
        },
    )
}

#[tokio::test]
async fn a_snapshot_comes_back_as_it_went_up() {
    let (_dir, store) = store();
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();
    let snapshot = snapshot_at(vault_id, 42);

    put_snapshot(&store, &dek, &snapshot).await.unwrap();
    let back = latest_snapshot(&store, &dek).await.unwrap().unwrap();

    assert_eq!(back, snapshot);
}

#[tokio::test]
async fn a_snapshot_is_ciphertext_in_the_bucket() {
    // It carries every name in the vault, so it is the last object that
    // could sit there readable.
    let (_dir, store) = store();
    let dek = generate_dek();
    let mut snapshot = snapshot_at(Uuid::new_v4(), 7);
    snapshot.folders[0].name = "Tax returns".into();
    snapshot.folders[0].path = "/Tax returns".into();

    put_snapshot(&store, &dek, &snapshot).await.unwrap();

    let raw = store
        .get("snapshots/00000000000000000007.snap")
        .await
        .unwrap();
    let as_text = String::from_utf8_lossy(&raw);
    assert!(
        !as_text.contains("Tax returns"),
        "a folder name reached storage in the clear"
    );
}

#[tokio::test]
async fn the_newest_snapshot_is_the_one_that_answers() {
    let (_dir, store) = store();
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();

    // Written out of order and spanning a decimal digit boundary, so the
    // answer has to come from reading the horizons rather than from taking
    // whatever the listing put last.
    for horizon in [100u64, 9, 42] {
        put_snapshot(&store, &dek, &snapshot_at(vault_id, horizon))
            .await
            .unwrap();
    }

    assert_eq!(snapshot_horizon(&store).await.unwrap(), 100);
    assert_eq!(
        latest_snapshot(&store, &dek)
            .await
            .unwrap()
            .unwrap()
            .horizon,
        100
    );
}

#[tokio::test]
async fn a_silo_without_snapshots_says_so_rather_than_failing() {
    let (_dir, store) = store();
    let dek = generate_dek();

    assert_eq!(snapshot_horizon(&store).await.unwrap(), 0);
    assert!(latest_snapshot(&store, &dek).await.unwrap().is_none());
}

#[tokio::test]
async fn pruning_removes_the_records_at_and_below_the_horizon() {
    let (_dir, store) = store();
    let dek = generate_dek();
    let device = Uuid::new_v4();
    let records: Vec<OpRecord> = (1..=5).map(|lamport| record(lamport, device)).collect();
    push_ops(&store, &dek, &records).await.unwrap();

    let outcome = prune_ops_below(&store, 3).await.unwrap();

    assert_eq!(outcome.deleted, 3);
    assert!(outcome.failed.is_empty());
    let left = store.list("ops/").await.unwrap();
    assert_eq!(left.len(), 2, "records above the horizon stay: {left:?}");
}

#[tokio::test]
async fn pruning_leaves_objects_this_build_cannot_read() {
    // An object under `ops/` whose key says nothing this version recognises
    // belongs to a newer format. Deleting it would be this build throwing
    // away what a later one is relying on.
    let (_dir, store) = store();
    let dek = generate_dek();
    let device = Uuid::new_v4();
    push_ops(&store, &dek, &[record(1, device)]).await.unwrap();
    store
        .put("ops/something-from-the-future", b"opaque".to_vec())
        .await
        .unwrap();

    let outcome = prune_ops_below(&store, 10).await.unwrap();

    assert_eq!(outcome.deleted, 1);
    assert!(
        store
            .head("ops/something-from-the-future")
            .await
            .unwrap()
            .is_some(),
        "an unreadable object was deleted"
    );
}

#[tokio::test]
async fn the_manifest_announces_the_version_that_knows_about_snapshots() {
    let (_dir, store) = store();
    let vault_id = Uuid::new_v4();

    put_manifest(&store, vault_id).await.unwrap();

    let manifest = read_manifest(&store).await.unwrap().unwrap();
    assert_eq!(manifest.vault_id, vault_id);
    assert_eq!(manifest.version, MANIFEST_VERSION);
}

#[tokio::test]
async fn a_manifest_from_a_newer_build_is_refused() {
    let (_dir, store) = store();
    let ahead = serde_json::json!({ "vault_id": Uuid::new_v4(), "version": MANIFEST_VERSION + 1 });
    store
        .put("vault.json", serde_json::to_vec(&ahead).unwrap())
        .await
        .unwrap();

    let err = read_manifest(&store).await.unwrap_err();

    assert!(err.to_string().contains("newer version"), "was: {err}");
}

// ── A device that fell behind the horizon ───────────────────────────

use rusqlite::Connection;
use silentsilo_crypto::MasterDek;
use silentsilo_sync::{
    SyncError, apply_rebuild, fetch_rebuild, rebootstrap_from_snapshot, sync_ops,
};
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{Vfs, capture_at, clear_pushed, pending_ops};

/// A device with its own vault, sharing one folder store and one DEK with the
/// others, which is what devices in a real silo share.
struct Device {
    _dir: tempfile::TempDir,
    session: VaultSession,
    id: Uuid,
}

impl Device {
    fn joining(vault_id: Uuid) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().to_path_buf(), vault_id, "test-secret").unwrap();
        Vfs::new(&session).ensure_initialized().unwrap();
        let id = silentsilo_vfs::device_id(&session.conn).unwrap();
        Self {
            _dir: dir,
            session,
            id,
        }
    }

    fn conn(&self) -> &Connection {
        &self.session.conn
    }

    fn root(&self) -> Uuid {
        Vfs::new(&self.session).root_folder_id().unwrap()
    }

    /// Writes a change the way the app does: applied locally and queued for
    /// the next push. `apply_op` is the other path, for records that arrived
    /// from storage, and it marks them as already pushed.
    fn author(&self, op: VaultOp) {
        silentsilo_vfs::emit(self.conn(), op).unwrap();
    }

    async fn sync(&self, store: &FolderStore, dek: &MasterDek) -> Result<(), SyncError> {
        let pending = pending_ops(self.conn()).unwrap();
        sync_ops(self.conn(), store, dek, &pending).await?;
        clear_pushed(self.conn(), &pending).unwrap();
        Ok(())
    }

    fn tree(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT fo.path, f.name FROM files f JOIN folders fo ON fo.id = f.folder_id
                 WHERE f.deleted_at IS NULL ORDER BY fo.path, f.name",
            )
            .unwrap();
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
        {
            let (path, name) = row.unwrap();
            out.push(format!("file {path}::{name}"));
        }
        out
    }
}

fn added(folder_id: Uuid, name: &str) -> VaultOp {
    VaultOp::AddFile {
        id: Uuid::new_v4(),
        folder_id,
        name: name.into(),
        blob_id: Uuid::new_v4(),
        size_bytes: 1,
        content_hash: "h".into(),
        mime_type: None,
        blob_key: String::new(),
    }
}

/// One silo, two devices, and a compaction that happens while the second is
/// away. Returns both devices, the store and the horizon.
async fn silo_compacted_behind_a_sleeping_device() -> (
    tempfile::TempDir,
    FolderStore,
    MasterDek,
    Device,
    Device,
    u64,
) {
    let (dir, store) = store();
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();

    let awake = Device::joining(vault_id);
    let asleep = Device::joining(vault_id);

    // Both see the first two changes.
    awake.author(added(awake.root(), "one.txt"));
    awake.author(added(awake.root(), "two.txt"));
    awake.sync(&store, &dek).await.unwrap();
    asleep.sync(&store, &dek).await.unwrap();
    assert_eq!(asleep.tree().len(), 2, "the second device started level");

    // The first device keeps working while the second is away.
    for name in ["three.txt", "four.txt", "five.txt"] {
        awake.author(added(awake.root(), name));
    }
    awake.sync(&store, &dek).await.unwrap();

    // Compaction, from the device that has the whole log.
    let horizon = 3;
    let snapshot = capture_at(awake.conn(), vault_id, horizon).unwrap();
    put_snapshot(&store, &dek, &snapshot).await.unwrap();
    prune_ops_below(&store, horizon).await.unwrap();

    (dir, store, dek, awake, asleep, horizon)
}

#[tokio::test]
async fn a_device_below_the_horizon_is_refused_rather_than_left_short() {
    let (_dir, store, dek, _awake, asleep, horizon) =
        silo_compacted_behind_a_sleeping_device().await;

    let err = asleep.sync(&store, &dek).await.unwrap_err();

    match err {
        SyncError::BehindHorizon {
            applied_through,
            horizon: reported,
        } => {
            assert_eq!(reported, horizon);
            assert!(applied_through <= horizon, "was {applied_through}");
        }
        other => panic!("expected BehindHorizon, got {other}"),
    }
}

#[tokio::test]
async fn a_refused_device_pushes_nothing() {
    // Its pending records sit below a horizon every other device has already
    // compacted past, so nobody would ever apply them. Sending them would
    // leave litter in the bucket and tell the user their change was saved.
    let (_dir, store, dek, _awake, asleep, _) = silo_compacted_behind_a_sleeping_device().await;
    let before = store.list("ops/").await.unwrap().len();

    asleep.author(added(asleep.root(), "written-while-behind.txt"));
    asleep.sync(&store, &dek).await.unwrap_err();

    assert_eq!(store.list("ops/").await.unwrap().len(), before);
}

#[tokio::test]
async fn rebuilding_from_the_snapshot_catches_the_device_up() {
    let (_dir, store, dek, awake, mut asleep, horizon) =
        silo_compacted_behind_a_sleeping_device().await;

    let outcome = rebootstrap_from_snapshot(&mut asleep.session.conn, &store, &dek)
        .await
        .unwrap();

    assert_eq!(outcome.horizon, horizon);
    assert_eq!(
        asleep.tree(),
        awake.tree(),
        "the rebuilt device sees what the other one does"
    );
    assert_ne!(
        outcome.device_id, asleep.id,
        "a rebuilt device must not keep an identity whose chain it just dropped"
    );
}

#[tokio::test]
async fn a_rebuilt_device_can_sync_again_and_be_followed() {
    let (_dir, store, dek, awake, mut asleep, _) = silo_compacted_behind_a_sleeping_device().await;
    rebootstrap_from_snapshot(&mut asleep.session.conn, &store, &dek)
        .await
        .unwrap();
    asleep.id = silentsilo_vfs::device_id(asleep.conn()).unwrap();

    // A change written after the rebuild has to reach the other device, which
    // is what proves the new identity and the moved clock are consistent with
    // what the bucket already holds.
    asleep.author(added(asleep.root(), "after-the-rebuild.txt"));
    asleep.sync(&store, &dek).await.unwrap();
    awake.sync(&store, &dek).await.unwrap();

    assert!(
        awake.tree().iter().any(|r| r.contains("after-the-rebuild")),
        "the other device never saw it: {:?}",
        awake.tree()
    );
    assert!(
        silentsilo_vfs::verify_chains(awake.conn())
            .unwrap()
            .is_empty(),
        "the rebuilt device broke a chain"
    );
}

#[tokio::test]
async fn a_device_joining_a_compacted_silo_sees_the_whole_tree() {
    // The hole compaction opens if nothing closes it. A joining device used
    // to replay the log from the beginning, which after a compaction is only
    // the records above the horizon: operations naming folders created below
    // it would resolve against nothing, and the user would be handed a
    // partial vault with no sign that anything was missing.
    let (_dir, store, dek, awake, _asleep, horizon) =
        silo_compacted_behind_a_sleeping_device().await;

    let mut newcomer = Device::joining(awake.session.vault_id);
    let (snapshot, incoming) = fetch_rebuild(&store, &dek).await.unwrap().unwrap();
    let outcome = apply_rebuild(&mut newcomer.session.conn, &snapshot, incoming).unwrap();

    assert_eq!(outcome.horizon, horizon);
    assert_eq!(
        newcomer.tree(),
        awake.tree(),
        "the joining device is missing what was compacted away"
    );
    assert!(
        !newcomer.tree().is_empty(),
        "a tree this test would pass on by being empty on both sides"
    );

    // And the old way in, to show this is a real hazard rather than a
    // precaution: replaying what is left of the log from the beginning gives
    // a different, smaller vault, with nothing to say so.
    let naive = Device::joining(awake.session.vault_id);
    let whole_log = silentsilo_sync::fetch_ops_from(&store, &dek, 0)
        .await
        .unwrap();
    let _ = silentsilo_vfs::replay(naive.conn(), whole_log);
    assert_ne!(
        naive.tree(),
        awake.tree(),
        "replaying a compacted log from zero produced the right tree, so this \
         test no longer proves anything"
    );
}

#[tokio::test]
async fn joining_an_uncompacted_silo_says_there_is_nothing_to_rebuild_from() {
    // Which is how the join path knows to replay the log from the beginning
    // instead. Not a failure: it is the normal state of every silo until it
    // grows enough to be worth compacting.
    let (_dir, store) = store();
    let dek = generate_dek();
    let device = Uuid::new_v4();
    push_ops(&store, &dek, &[record(1, device)]).await.unwrap();

    assert!(fetch_rebuild(&store, &dek).await.unwrap().is_none());
}

// ── Collecting superseded content ───────────────────────────────────

use std::collections::HashSet;

use silentsilo_sync::sweep_orphan_blobs;

async fn put_blob(store: &FolderStore, id: Uuid) {
    store
        .put(&format!("blobs/{id}.sslo"), b"ciphertext".to_vec())
        .await
        .unwrap();
}

async fn blobs_left(store: &FolderStore) -> usize {
    store.list("blobs/").await.unwrap().len()
}

#[tokio::test]
async fn nothing_is_deleted_on_the_first_sighting() {
    // A blob reaches the bucket before the record naming it does. In that
    // window it is real and unreferenced, which looks exactly like superseded
    // content. Deleting on sight would destroy a file another device uploaded
    // seconds ago.
    let (_dir, store) = store();
    let orphan = Uuid::new_v4();
    put_blob(&store, orphan).await;

    let outcome = sweep_orphan_blobs(&store, &HashSet::new(), &HashSet::new())
        .await
        .unwrap();

    assert_eq!(outcome.deleted, 0);
    assert_eq!(outcome.candidates, vec![orphan]);
    assert_eq!(blobs_left(&store).await, 1);
}

#[tokio::test]
async fn a_blob_unreferenced_two_passes_running_is_collected() {
    let (_dir, store) = store();
    let orphan = Uuid::new_v4();
    put_blob(&store, orphan).await;

    let first = sweep_orphan_blobs(&store, &HashSet::new(), &HashSet::new())
        .await
        .unwrap();
    let second = sweep_orphan_blobs(
        &store,
        &HashSet::new(),
        &first.candidates.into_iter().collect(),
    )
    .await
    .unwrap();

    assert_eq!(second.deleted, 1);
    assert_eq!(blobs_left(&store).await, 0);
}

#[tokio::test]
async fn content_the_index_still_points_at_is_never_touched() {
    // Including a file in the trash: its row stays until the trash is purged,
    // and restoring it has to find the content where it left it.
    let (_dir, store) = store();
    let live = Uuid::new_v4();
    put_blob(&store, live).await;
    let referenced: HashSet<Uuid> = [live].into_iter().collect();

    // Twice, so it is not merely the first-sighting rule doing the work.
    let first = sweep_orphan_blobs(&store, &referenced, &HashSet::new())
        .await
        .unwrap();
    let second = sweep_orphan_blobs(&store, &referenced, &[live].into_iter().collect())
        .await
        .unwrap();

    assert!(first.candidates.is_empty());
    assert_eq!(second.deleted, 0);
    assert_eq!(blobs_left(&store).await, 1);
}

#[tokio::test]
async fn a_blob_that_became_referenced_between_passes_is_spared() {
    // The race this design exists for, played out: the object was there
    // first, its record arrived second, and the second pass has to notice
    // rather than act on what the first one decided.
    let (_dir, store) = store();
    let arriving = Uuid::new_v4();
    put_blob(&store, arriving).await;

    let first = sweep_orphan_blobs(&store, &HashSet::new(), &HashSet::new())
        .await
        .unwrap();
    let second = sweep_orphan_blobs(
        &store,
        &[arriving].into_iter().collect(),
        &first.candidates.into_iter().collect(),
    )
    .await
    .unwrap();

    assert_eq!(second.deleted, 0);
    assert_eq!(blobs_left(&store).await, 1);
}

#[tokio::test]
async fn objects_this_build_cannot_name_are_left_alone() {
    let (_dir, store) = store();
    store
        .put("blobs/not-a-uuid.sslo", b"opaque".to_vec())
        .await
        .unwrap();

    let first = sweep_orphan_blobs(&store, &HashSet::new(), &HashSet::new())
        .await
        .unwrap();
    let second = sweep_orphan_blobs(&store, &HashSet::new(), &HashSet::new())
        .await
        .unwrap();

    assert!(first.candidates.is_empty());
    assert_eq!(second.deleted, 0);
    assert_eq!(blobs_left(&store).await, 1);
}

// ── Progress while a join downloads ─────────────────────────────────

#[tokio::test]
async fn fetching_reports_a_total_before_the_first_record_arrives() {
    // What makes this a proportion rather than a number climbing towards an
    // unknown end: the listing comes back whole, so the total is known before
    // anything is downloaded.
    let (_dir, store) = store();
    let dek = generate_dek();
    let device = Uuid::new_v4();
    let records: Vec<OpRecord> = (1..=5).map(|lamport| record(lamport, device)).collect();
    push_ops(&store, &dek, &records).await.unwrap();

    let mut seen: Vec<(usize, usize)> = Vec::new();
    let fetched = silentsilo_sync::fetch_ops_reporting(&store, &dek, 0, &mut |done, total| {
        seen.push((done, total));
    })
    .await
    .unwrap();

    assert_eq!(fetched.len(), 5);
    assert_eq!(seen.first(), Some(&(0, 5)), "the total came late: {seen:?}");
    assert_eq!(seen.last(), Some(&(5, 5)));
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "the count went backwards: {seen:?}"
    );
}

#[tokio::test]
async fn a_silo_with_nothing_to_fetch_still_reports_once() {
    // Otherwise the screen shows a spinner and no number at all, which is the
    // state this was added to remove.
    let (_dir, store) = store();
    let dek = generate_dek();

    let mut seen = Vec::new();
    silentsilo_sync::fetch_ops_reporting(&store, &dek, 0, &mut |done, total| {
        seen.push((done, total));
    })
    .await
    .unwrap();

    assert_eq!(seen, vec![(0, 0)]);
}

// ── What an idle pass costs ─────────────────────────────────────────

#[tokio::test]
async fn the_manifest_is_written_once_and_then_left_alone() {
    // It used to be written on every sync pass, which on a versioned bucket
    // is thousands of versions a day of a file holding two fields that never
    // change.
    let (_dir, store) = store();
    let vault_id = Uuid::new_v4();

    assert!(
        silentsilo_sync::ensure_manifest(&store, vault_id)
            .await
            .unwrap(),
        "the first pass has to publish it"
    );
    assert!(
        !silentsilo_sync::ensure_manifest(&store, vault_id)
            .await
            .unwrap(),
        "a second pass rewrote a manifest that already said the same thing"
    );
}

#[tokio::test]
async fn a_manifest_naming_another_vault_is_replaced() {
    // The prefix was reused for a different silo. Leaving the old id there
    // would send a joining device looking for the wrong vault.
    let (_dir, store) = store();
    silentsilo_sync::ensure_manifest(&store, Uuid::new_v4())
        .await
        .unwrap();

    let now_holding = Uuid::new_v4();
    assert!(
        silentsilo_sync::ensure_manifest(&store, now_holding)
            .await
            .unwrap()
    );
    assert_eq!(
        read_manifest(&store).await.unwrap().unwrap().vault_id,
        now_holding
    );
}

// ── Reading the horizon from more than one target ───────────────────

#[tokio::test]
async fn the_lowest_horizon_is_the_one_that_counts() {
    // One target compacted to 100, another only to 50. The second still
    // holds records 51 to 100, so a device that has applied through 60 can
    // still catch up: taking the highest would send it off to be rebuilt for
    // no reason.
    let (_a, ahead) = store();
    let (_b, behind) = store();
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();

    put_snapshot(&ahead, &dek, &snapshot_at(vault_id, 100))
        .await
        .unwrap();
    put_snapshot(&behind, &dek, &snapshot_at(vault_id, 50))
        .await
        .unwrap();

    let lowest = silentsilo_sync::lowest_snapshot_horizon(&[&ahead, &behind])
        .await
        .unwrap();

    assert_eq!(lowest, 50);
}

#[tokio::test]
async fn a_target_that_has_never_been_compacted_wins() {
    // It still holds everything, which is the strongest answer there is.
    let (_a, compacted) = store();
    let (_b, untouched) = store();
    let dek = generate_dek();

    put_snapshot(&compacted, &dek, &snapshot_at(Uuid::new_v4(), 900))
        .await
        .unwrap();

    assert_eq!(
        silentsilo_sync::lowest_snapshot_horizon(&[&compacted, &untouched])
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn a_target_nobody_can_reach_is_not_read_as_zero() {
    // Guessing that an unreachable bucket still holds everything is how a
    // device talks itself into carrying on and silently misses records.
    // A path with a character no filesystem accepts, so listing it fails
    // rather than quietly returning nothing.
    let unreachable = FolderStore::new(std::path::PathBuf::from("nul-device/\0/nowhere"));

    let err = silentsilo_sync::lowest_snapshot_horizon(&[&unreachable])
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("could be reached"), "was: {err}");
}

#[tokio::test]
async fn a_silo_with_no_target_compacts_itself_and_still_seeds_a_bucket() {
    // Local-only compaction may drop unpushed records because the base
    // written in the same transaction replaces them; the debt is that any
    // later target gets the base before the first push.
    let mut d = Device::joining(Uuid::new_v4());
    for i in 0..40 {
        d.author(added(d.root(), &format!("f{i}.txt")));
    }
    let before = d.tree();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 60;
    let policy = silentsilo_vfs::CompactionPolicy {
        retain_seconds: 0,
        keep_recent: 5,
        min_records: 10,
    };
    let snapshot = silentsilo_sync::plan_compaction(d.conn(), d.session.vault_id, &policy, now)
        .unwrap()
        .unwrap();
    let dropped =
        silentsilo_vfs::snapshot::compact_covered(&mut d.session.conn, &snapshot).unwrap();
    assert!(dropped >= 35, "dropped {dropped}");
    let left: i64 = d
        .conn()
        .query_row("SELECT COUNT(*) FROM oplog", [], |r| r.get(0))
        .unwrap();
    assert!(left as usize <= policy.keep_recent, "left {left}");
    assert_eq!(d.tree(), before, "the tree must survive its own compaction");

    // The first target arrives after the fact: base first, then the tail.
    let (_dir, store) = store();
    let dek = generate_dek();
    assert!(
        silentsilo_sync::publish_base_if_missing(&store, &dek, &snapshot)
            .await
            .unwrap()
    );
    assert!(
        !silentsilo_sync::publish_base_if_missing(&store, &dek, &snapshot)
            .await
            .unwrap(),
        "publishing twice must not write twice"
    );
    d.sync(&store, &dek).await.unwrap();

    let mut newcomer = Device::joining(d.session.vault_id);
    let (snap, tail) = fetch_rebuild(&store, &dek).await.unwrap().unwrap();
    apply_rebuild(&mut newcomer.session.conn, &snap, tail).unwrap();
    assert_eq!(
        newcomer.tree(),
        before,
        "a newcomer must see the whole tree"
    );
}
