//! Re-sealing a silo's storage under a new vault key.
//!
//! Folder stores throughout, so these run without a bucket. What is being
//! checked is not the transport but the property rotation exists for: after a
//! pass, the old key opens nothing and the new key opens everything, and the
//! records themselves are untouched.

use silentsilo_crypto::{MasterDek, generate_content_kek, generate_dek, seal, unseal};
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{
    CONTENT_KEK_KEY, OPS_PREFIX, fetch_ops_from, publish_content_kek, push_ops, put_snapshot,
    reseal_under_new_key,
};
use silentsilo_vfs::{OpRecord, Snapshot, VaultOp};
use uuid::Uuid;

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

fn snapshot_at(vault_id: Uuid, horizon: u64) -> Snapshot {
    Snapshot {
        version: silentsilo_vfs::SNAPSHOT_VERSION,
        vault_id,
        horizon,
        captured_at: 1_700_000_000,
        folders: Vec::new(),
        files: Vec::new(),
        passwords: Vec::new(),
        name_claims: Vec::new(),
        device_labels: Vec::new(),
    }
}

/// Fills a store the way a silo would, under `dek`.
async fn populate(store: &dyn ObjectStore, dek: &MasterDek) -> Vec<OpRecord> {
    let device = Uuid::new_v4();
    let records: Vec<OpRecord> = (1..=4).map(|l| record(l, device)).collect();
    push_ops(store, dek, &records).await.unwrap();
    put_snapshot(store, dek, &snapshot_at(Uuid::new_v4(), 2))
        .await
        .unwrap();

    let kek = generate_content_kek();
    let envelope = seal(kek.as_bytes(), dek).unwrap();
    publish_content_kek(store, &envelope).await.unwrap();
    records
}

#[tokio::test]
async fn after_rotation_the_old_key_opens_nothing_and_the_new_one_opens_everything() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let old = generate_dek();
    let new = generate_dek();

    let written = populate(&store as &dyn ObjectStore, &old).await;

    let outcome = reseal_under_new_key(&store as &dyn ObjectStore, &old, &new, &mut |_, _| {})
        .await
        .unwrap();
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(outcome.resealed, 6, "4 records, 1 snapshot, 1 KEK envelope");
    assert_eq!(outcome.already, 0);

    // The point of the exercise.
    assert!(
        fetch_ops_from(&store as &dyn ObjectStore, &old, 0)
            .await
            .is_err(),
        "the old vault key must no longer open the log"
    );
    let after = fetch_ops_from(&store as &dyn ObjectStore, &new, 0)
        .await
        .unwrap();
    assert_eq!(after.len(), written.len());
}

/// The property the whole design rests on: the record itself does not change,
/// only the envelope around it. Its fingerprint is a hash of the record, so a
/// changed body would break the log's chain and leave this device disagreeing
/// with every other about the bytes of the same operation.
#[tokio::test]
async fn the_records_themselves_are_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let old = generate_dek();
    let new = generate_dek();

    let written = populate(&store as &dyn ObjectStore, &old).await;
    reseal_under_new_key(&store as &dyn ObjectStore, &old, &new, &mut |_, _| {})
        .await
        .unwrap();

    let after = fetch_ops_from(&store as &dyn ObjectStore, &new, 0)
        .await
        .unwrap();
    for original in &written {
        let same = after
            .iter()
            .find(|r| r.op_id == original.op_id)
            .expect("every record survived");
        assert_eq!(
            same.fingerprint().unwrap(),
            original.fingerprint().unwrap(),
            "the record's fingerprint moved, which breaks the chain"
        );
    }
}

/// A rotation is interrupted by a dropped connection, not finished by one.
/// Running it again has to complete rather than start over, and must never
/// leave an object that opens under neither key.
#[tokio::test]
async fn an_interrupted_pass_finishes_on_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let old = generate_dek();
    let new = generate_dek();

    populate(&store as &dyn ObjectStore, &old).await;

    // Stand in for a pass that stopped partway: one object already carries
    // the new key, the rest do not.
    let first = store.list(OPS_PREFIX).await.unwrap()[0].key.clone();
    let plain = unseal(&store.get(&first).await.unwrap(), &old).unwrap();
    store
        .put(&first, seal(&plain, &new).unwrap())
        .await
        .unwrap();

    let outcome = reseal_under_new_key(&store as &dyn ObjectStore, &old, &new, &mut |_, _| {})
        .await
        .unwrap();

    assert_eq!(outcome.already, 1, "the finished object was left alone");
    assert_eq!(outcome.resealed, 5);
    assert!(outcome.failed.is_empty());

    // And a third run has nothing left to do, which is what makes it safe to
    // retry without counting attempts.
    let again = reseal_under_new_key(&store as &dyn ObjectStore, &old, &new, &mut |_, _| {})
        .await
        .unwrap();
    assert_eq!(again.resealed, 0);
    assert_eq!(again.already, 6);
}

/// The KEK envelope is the one object that must arrive: without it a joining
/// device has no way to open any content at all.
#[tokio::test]
async fn the_content_key_envelope_is_resealed_too() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let old = generate_dek();
    let new = generate_dek();

    populate(&store as &dyn ObjectStore, &old).await;
    reseal_under_new_key(&store as &dyn ObjectStore, &old, &new, &mut |_, _| {})
        .await
        .unwrap();

    let envelope = store.get(CONTENT_KEK_KEY).await.unwrap();
    assert!(unseal(&envelope, &new).is_ok(), "the new key opens it");
    assert!(unseal(&envelope, &old).is_err(), "the old one does not");
}

/// Progress counts towards a total known before the first write, because the
/// listing arrives whole. A rotation on a large silo is thousands of objects.
#[tokio::test]
async fn progress_counts_towards_a_total_known_up_front() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let old = generate_dek();

    populate(&store as &dyn ObjectStore, &old).await;

    let mut seen: Vec<(usize, usize)> = Vec::new();
    reseal_under_new_key(
        &store as &dyn ObjectStore,
        &old,
        &generate_dek(),
        &mut |done, total| seen.push((done, total)),
    )
    .await
    .unwrap();

    assert_eq!(seen.first(), Some(&(0, 6)));
    assert_eq!(seen.last(), Some(&(6, 6)));
}
