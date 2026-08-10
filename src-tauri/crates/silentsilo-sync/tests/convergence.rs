//! Devices reaching the same tree when their clocks and schedules do not
//! line up. The regression pinned here shipped in review, not in a release:
//! a fetch bounded by the highest Lamport already applied skips a record
//! pushed late by a device that was offline, forever, and compaction then
//! deletes that record from the bucket before anyone else has seen it.

use rusqlite::Connection;
use silentsilo_crypto::{MasterDek, generate_dek};
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::sync_ops;
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{Vfs, clear_pushed, digest, digest_difference, pending_ops};
use uuid::Uuid;

struct Device {
    _dir: tempfile::TempDir,
    session: VaultSession,
}

impl Device {
    fn joining(vault_id: Uuid) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().to_path_buf(), vault_id, "test-secret").unwrap();
        Vfs::new(&session).ensure_initialized().unwrap();
        Self { _dir: dir, session }
    }

    fn conn(&self) -> &Connection {
        &self.session.conn
    }

    fn vfs(&self) -> Vfs<'_> {
        Vfs::new(&self.session)
    }

    fn root(&self) -> Uuid {
        self.vfs().root_folder_id().unwrap()
    }

    fn tree(&self) -> Vec<String> {
        digest(self.conn()).unwrap()
    }

    /// One pass against one store: push what is pending, pull what is
    /// missing, the same steps the app runs.
    async fn sync(&self, store: &dyn ObjectStore, dek: &MasterDek) -> silentsilo_sync::SyncOutcome {
        let pending = pending_ops(self.conn()).unwrap();
        let outcome = sync_ops(self.conn(), store, dek, &pending).await.unwrap();
        clear_pushed(self.conn(), &pending).unwrap();
        outcome
    }
}

fn assert_same_tree(a: &Device, b: &Device, when: &str) {
    let differences = digest_difference(&a.tree(), &b.tree());
    assert!(differences.is_empty(), "{when}: {differences:?}");
}

#[tokio::test]
async fn a_change_pushed_late_by_an_offline_device_still_reaches_the_others() {
    let vault_id = Uuid::new_v4();
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();

    let a = Device::joining(vault_id);
    let b = Device::joining(vault_id);

    // A makes a change and goes offline before it can push.
    a.vfs().create_folder(a.root(), "from-a").unwrap();

    // B keeps working and syncing, so its Lamport clock moves well past the
    // value A's stranded record carries.
    for i in 0..5 {
        b.vfs().create_folder(b.root(), &format!("b{i}")).unwrap();
    }
    b.sync(&store, &dek).await;

    // A comes back: its record enters the bucket below B's watermark.
    a.sync(&store, &dek).await;

    // The fetch diffs op ids, so B still receives it. A watermark fetch
    // skipped it forever, and a later compaction deleted it outright.
    let pulled = b.sync(&store, &dek).await;
    assert!(
        pulled.fetched > 0,
        "B never fetched the record A pushed late"
    );
    assert!(
        b.tree().iter().any(|line| line.contains("from-a")),
        "A's folder is missing on B: {:?}",
        b.tree()
    );
    assert_same_tree(&a, &b, "after the late push settled");
}

#[tokio::test]
async fn both_directions_converge_after_a_long_offline_gap() {
    let vault_id = Uuid::new_v4();
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();

    let a = Device::joining(vault_id);
    let b = Device::joining(vault_id);

    a.vfs().create_folder(a.root(), "shared").unwrap();
    a.sync(&store, &dek).await;
    b.sync(&store, &dek).await;

    // Both work offline, then return in either order.
    a.vfs().create_folder(a.root(), "a-offline").unwrap();
    for i in 0..4 {
        b.vfs().create_folder(b.root(), &format!("b-{i}")).unwrap();
    }
    b.sync(&store, &dek).await;
    a.sync(&store, &dek).await;
    b.sync(&store, &dek).await;

    assert_same_tree(&a, &b, "after both caught up");
    assert!(b.tree().iter().any(|line| line.contains("a-offline")));
}

#[tokio::test]
async fn a_compacted_device_does_not_reapply_records_below_its_horizon() {
    // An append-only copy keeps the whole log even after a compaction, so
    // the old records stay listed. The op-id diff alone would fetch them,
    // because the local log no longer holds them; the horizon filter is what
    // keeps the snapshot contract.
    let vault_id = Uuid::new_v4();
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();

    let mut a = Device::joining(vault_id);
    for i in 0..6 {
        a.vfs()
            .create_folder(a.root(), &format!("old-{i}"))
            .unwrap();
    }
    a.sync(&store, &dek).await;

    let policy = silentsilo_vfs::CompactionPolicy {
        retain_seconds: 0,
        keep_recent: 1,
        min_records: 2,
    };
    let snapshot = silentsilo_sync::plan_compaction(a.conn(), vault_id, &policy, 2_000_000_000)
        .unwrap()
        .expect("the log is long and old enough to compact");
    // Published without pruning, as an append-only target is.
    silentsilo_sync::publish_compaction(&store, &dek, &snapshot, false)
        .await
        .unwrap();
    silentsilo_vfs::snapshot::compact_local(&mut a.session.conn, &snapshot).unwrap();

    let before = a.tree();
    let outcome = a.sync(&store, &dek).await;

    assert_eq!(
        outcome.fetched, 0,
        "records below the horizon were fetched again"
    );
    assert_eq!(a.tree(), before, "the tree changed on an idle pass");
}

#[tokio::test]
async fn a_corrupt_object_holds_later_records_back_without_wedging_the_pass() {
    let vault_id = Uuid::new_v4();
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();

    let a = Device::joining(vault_id);
    a.vfs().create_folder(a.root(), "first").unwrap();
    a.vfs().create_folder(a.root(), "second").unwrap();
    a.vfs().create_folder(a.root(), "third").unwrap();
    a.sync(&store, &dek).await;

    // Rot the middle record on the target, keeping its original bytes.
    let middle = store
        .list("ops/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.key)
        .nth(1)
        .unwrap();
    let path = dir.path().join(&middle);
    let original = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"rotted").unwrap();

    let b = Device::joining(vault_id);
    let first_pass = b.sync(&store, &dek).await;

    assert_eq!(first_pass.unreadable.len(), 1, "the rot was not noticed");
    assert_eq!(
        first_pass.fetched, 1,
        "records below the hole should still apply"
    );
    assert_eq!(first_pass.held_back, 1, "the record above the hole waits");
    assert!(b.tree().iter().any(|line| line.contains("first")));
    assert!(!b.tree().iter().any(|line| line.contains("third")));

    // The object comes back (a healthy copy, a re-upload): the next pass
    // finishes the job with nothing left over.
    std::fs::write(&path, &original).unwrap();
    let second_pass = b.sync(&store, &dek).await;
    assert!(second_pass.unreadable.is_empty());
    assert_same_tree(&a, &b, "after the object was restored");
}
