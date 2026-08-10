//! Copying one target's contents into another.
//!
//! Folder stores at both ends, which is also the real case: the point of
//! seeding is filling a disk locally instead of pushing hundreds of
//! gigabytes over a home connection.

use silentsilo_crypto::generate_dek;
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{
    BLOBS_PREFIX, MANIFEST_KEY, OPS_PREFIX, fetch_ops_from, push_ops, put_manifest, read_manifest,
    seed_target,
};
use silentsilo_vfs::{OpRecord, VaultOp};
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

/// Fills a store the way a silo would: some records, some content, a manifest.
async fn populate(store: &dyn ObjectStore, dek: &silentsilo_crypto::MasterDek, vault_id: Uuid) {
    let device = Uuid::new_v4();
    let records: Vec<OpRecord> = (1..=4).map(|l| record(l, device)).collect();
    push_ops(store, dek, &records).await.unwrap();
    for i in 0..3u8 {
        store
            .put(
                &format!("{BLOBS_PREFIX}{}.sslo", Uuid::new_v4()),
                vec![i; 128],
            )
            .await
            .unwrap();
    }
    put_manifest(store, vault_id).await.unwrap();
}

#[tokio::test]
async fn a_seeded_target_holds_everything_the_first_one_did() {
    let source_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let source = FolderStore::new(source_dir.path().to_path_buf());
    let dest = FolderStore::new(dest_dir.path().to_path_buf());
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();

    populate(&source as &dyn ObjectStore, &dek, vault_id).await;

    let outcome = seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(outcome.copied, 8, "4 records, 3 blobs, 1 manifest");
    assert_eq!(outcome.skipped, 0);

    // The point of the exercise: what came out of the copy is a silo, not a
    // pile of files. Records decrypt with the same key and the manifest
    // still names the same vault.
    let records = fetch_ops_from(&dest as &dyn ObjectStore, &dek, 0)
        .await
        .unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        read_manifest(&dest as &dyn ObjectStore)
            .await
            .unwrap()
            .map(|m| m.vault_id),
        Some(vault_id)
    );
}

/// A seed is interrupted by a cable, not finished by one. Running it again
/// has to carry on rather than start over, or the operation is unusable at
/// exactly the size it exists for.
#[tokio::test]
async fn running_it_again_copies_only_what_is_missing() {
    let source_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let source = FolderStore::new(source_dir.path().to_path_buf());
    let dest = FolderStore::new(dest_dir.path().to_path_buf());
    let dek = generate_dek();

    populate(&source as &dyn ObjectStore, &dek, Uuid::new_v4()).await;
    seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    // One more record arrives at the source after the first pass.
    push_ops(
        &source as &dyn ObjectStore,
        &dek,
        &[record(9, Uuid::new_v4())],
    )
    .await
    .unwrap();

    let second = seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert_eq!(second.copied, 2, "the new record, and the manifest again");
    assert_eq!(second.skipped, 7);
    assert_eq!(
        dest.list(OPS_PREFIX).await.unwrap().len(),
        5,
        "and nothing already there was lost"
    );
}

/// Progress is reported against a total known before the first byte moves,
/// because the listing arrives whole. A count going up towards an unknown end
/// is not progress on an operation that runs for hours.
#[tokio::test]
async fn progress_counts_towards_a_total_known_up_front() {
    let source_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let source = FolderStore::new(source_dir.path().to_path_buf());
    let dest = FolderStore::new(dest_dir.path().to_path_buf());

    populate(&source as &dyn ObjectStore, &generate_dek(), Uuid::new_v4()).await;

    let mut seen: Vec<(usize, usize)> = Vec::new();
    seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |done, total| seen.push((done, total)),
        &|| false,
    )
    .await
    .unwrap();

    assert_eq!(
        seen.first(),
        Some(&(0, 8)),
        "the total is known at the start"
    );
    assert_eq!(seen.last(), Some(&(8, 8)));
    assert!(
        seen.windows(2).all(|w| w[0].0 <= w[1].0),
        "and it never goes backwards"
    );
}

/// Seeding is a copy of ciphertext. It never needs the vault key, which is
/// what makes "fill the disk at the office" something an operator can do.
#[tokio::test]
async fn seeding_needs_no_key_and_changes_no_bytes() {
    let source_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let source = FolderStore::new(source_dir.path().to_path_buf());
    let dest = FolderStore::new(dest_dir.path().to_path_buf());
    let dek = generate_dek();

    populate(&source as &dyn ObjectStore, &dek, Uuid::new_v4()).await;
    seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    for entry in source.list(OPS_PREFIX).await.unwrap() {
        assert_eq!(
            source.get(&entry.key).await.unwrap(),
            dest.get(&entry.key).await.unwrap(),
            "{} came across byte for byte",
            entry.key
        );
    }
    assert!(dest.head(MANIFEST_KEY).await.unwrap().is_some());
}

/// Stopping is answered between objects, and a stopped seed is an error the
/// caller can tell apart from a failed one: nothing about it needs retrying
/// differently, but the user asked for it and the report must say so.
#[tokio::test]
async fn a_cancelled_seed_stops_and_says_so() {
    let source_dir = tempfile::tempdir().unwrap();
    let dest_dir = tempfile::tempdir().unwrap();
    let source = FolderStore::new(source_dir.path().to_path_buf());
    let dest = FolderStore::new(dest_dir.path().to_path_buf());
    let dek = generate_dek();

    populate(&source as &dyn ObjectStore, &dek, Uuid::new_v4()).await;

    let result = seed_target(
        &source as &dyn ObjectStore,
        &dest as &dyn ObjectStore,
        &mut |_, _| {},
        &|| true,
    )
    .await;

    assert!(matches!(result, Err(silentsilo_sync::SyncError::Cancelled)));
}
