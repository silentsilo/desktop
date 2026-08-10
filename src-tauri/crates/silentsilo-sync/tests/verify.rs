//! Checking a silo against its storage.
//!
//! The failures being looked for are the quiet ones: a provider that lost an
//! object, an upload that stopped half way, a bit that rotted. Every one of
//! them looks like a healthy backup until something is restored from it, so
//! each is reproduced here rather than assumed.

use std::collections::HashSet;

use silentsilo_crypto::{
    ContentKey, MasterDek, encrypt_file, generate_content_key, generate_dek, seal,
};
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{BLOBS_PREFIX, VerifyDepth, push_ops, verify_against};
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

/// One encrypted blob in the store, with the key that opens it.
async fn put_blob(store: &dyn ObjectStore, blob_id: Uuid, contents: &[u8]) -> ContentKey {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("p.bin");
    let enc = dir.path().join("b.sslo");
    std::fs::write(&plain, contents).unwrap();
    let key = generate_content_key();
    encrypt_file(&plain, &enc, &key, Uuid::new_v4(), blob_id).unwrap();
    store
        .put(
            &format!("{BLOBS_PREFIX}{blob_id}.sslo"),
            std::fs::read(&enc).unwrap(),
        )
        .await
        .unwrap();
    key
}

/// A silo's worth of storage: some records, three blobs, all referenced.
async fn populate(
    store: &dyn ObjectStore,
    dek: &MasterDek,
) -> (HashSet<Uuid>, std::collections::HashMap<Uuid, ContentKey>) {
    let device = Uuid::new_v4();
    let records: Vec<OpRecord> = (1..=3).map(|l| record(l, device)).collect();
    push_ops(store, dek, &records).await.unwrap();

    let mut expected = HashSet::new();
    let mut keys = std::collections::HashMap::new();
    for i in 0..3u8 {
        let blob_id = Uuid::new_v4();
        let key = put_blob(store, blob_id, &vec![i; 5000]).await;
        expected.insert(blob_id);
        keys.insert(blob_id, key);
    }
    (expected, keys)
}

fn opener(
    keys: std::collections::HashMap<Uuid, ContentKey>,
) -> impl FnMut(Uuid) -> Option<ContentKey> + Send {
    move |id| keys.get(&id).cloned()
}

#[tokio::test]
async fn a_sound_silo_reports_nothing_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let report = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Content,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(report.is_sound(), "{report:?}");
    assert_eq!(report.records_read, 3);
    assert_eq!(report.blobs_checked, 3);
    assert!(report.bytes_read > 0, "content was actually read back");
}

/// The finding that matters most: the silo believes it holds a file and
/// storage does not have it. Nothing else in the app notices until someone
/// tries to open that file, which may be years later.
#[tokio::test]
async fn an_object_the_provider_lost_is_reported_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let gone = *expected.iter().next().unwrap();
    store
        .delete(&format!("{BLOBS_PREFIX}{gone}.sslo"))
        .await
        .unwrap();

    let report = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(!report.is_sound());
    assert_eq!(report.missing, vec![gone]);
    // Found without downloading anything, which is what makes this worth
    // running on a large silo.
    assert_eq!(report.bytes_read, 0);
}

/// Bit rot. Only reading the content back catches it, which is the whole
/// reason the deeper check exists.
#[tokio::test]
async fn a_rotted_blob_is_found_by_reading_it_and_missed_by_listing() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let victim = *expected.iter().next().unwrap();
    let key = format!("{BLOBS_PREFIX}{victim}.sslo");
    let mut bytes = store.get(&key).await.unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0x01;
    store.put(&key, bytes).await.unwrap();

    let shallow = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys.clone()),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();
    assert!(
        shallow.is_sound(),
        "a listing cannot see inside an object, and should not pretend to"
    );

    let deep = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Content,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();
    assert!(!deep.is_sound());
    assert_eq!(deep.damaged.len(), 1);
    assert!(deep.damaged[0].0.contains(&victim.to_string()));
}

/// An upload that created the object and stopped. Caught without reading
/// anything, because an empty object is wrong whatever is supposed to be in
/// it.
#[tokio::test]
async fn a_partial_upload_is_caught_by_the_cheap_check() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let victim = *expected.iter().next().unwrap();
    store
        .put(&format!("{BLOBS_PREFIX}{victim}.sslo"), Vec::new())
        .await
        .unwrap();

    let report = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(!report.is_sound());
    assert_eq!(report.damaged.len(), 1);
}

/// A record that has been altered underneath. The envelope is authenticated,
/// so this is caught by opening it rather than by any separate check.
#[tokio::test]
async fn a_tampered_record_is_reported_rather_than_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let ops = store.list("ops/").await.unwrap();
    let target = ops[0].key.clone();
    let mut bytes = store.get(&target).await.unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    store.put(&target, bytes).await.unwrap();

    let report = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(!report.is_sound());
    assert_eq!(report.records_read, 2, "the other two still opened");
    assert_eq!(report.damaged.len(), 1);
}

/// Content nothing references is normal, not a fault: a blob reaches storage
/// before the record that names it. Reporting it as damage would train people
/// to ignore the report.
#[tokio::test]
async fn unreferenced_content_is_counted_rather_than_condemned() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    put_blob(&store as &dyn ObjectStore, Uuid::new_v4(), b"just arrived").await;

    let report = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Content,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert!(report.is_sound());
    assert_eq!(report.unreferenced, 1);
}

/// A silo whose storage was written under a different key. Every record fails
/// to open, which is the honest reading: this storage does not belong to this
/// silo, and saying "damaged" beats replaying nothing and calling it fine.
#[tokio::test]
async fn storage_written_under_another_key_is_reported_rather_than_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let (expected, keys) = populate(&store as &dyn ObjectStore, &generate_dek()).await;

    let report = verify_against(
        &store as &dyn ObjectStore,
        &generate_dek(),
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |_, _| {},
        &|| false,
    )
    .await
    .unwrap();

    assert_eq!(report.records_read, 0);
    assert_eq!(report.damaged.len(), 3);
}

#[tokio::test]
async fn progress_counts_towards_a_total_known_up_front() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let mut seen: Vec<(usize, usize)> = Vec::new();
    verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |done, total| seen.push((done, total)),
        &|| false,
    )
    .await
    .unwrap();

    assert_eq!(seen.first(), Some(&(0, 6)), "3 records and 3 blobs");
    assert_eq!(seen.last(), Some(&(6, 6)));
}

/// Sealing is not part of the public surface here, but the test above needs
/// to know a sealed record is what `push_ops` writes, so this keeps the
/// assumption honest.
#[tokio::test]
async fn records_in_storage_are_sealed_under_the_vault_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    push_ops(
        &store as &dyn ObjectStore,
        &dek,
        &[record(1, Uuid::new_v4())],
    )
    .await
    .unwrap();

    let key = store.list("ops/").await.unwrap()[0].key.clone();
    let stored = store.get(&key).await.unwrap();
    assert_ne!(stored, seal(b"anything", &dek).unwrap());
    assert_eq!(&stored[..4], b"SSEA", "the sealed envelope's magic");
}

/// Stopping is answered between objects. Read-only work, so a stopped check
/// loses nothing but the answer, and the error names the reason rather than
/// pretending the storage failed.
#[tokio::test]
async fn a_cancelled_check_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    let dek = generate_dek();
    let (expected, keys) = populate(&store as &dyn ObjectStore, &dek).await;

    let result = verify_against(
        &store as &dyn ObjectStore,
        &dek,
        &expected,
        VerifyDepth::Listing,
        &mut opener(keys),
        &mut |_, _| {},
        &|| true,
    )
    .await;

    assert!(matches!(result, Err(silentsilo_sync::SyncError::Cancelled)));
}
