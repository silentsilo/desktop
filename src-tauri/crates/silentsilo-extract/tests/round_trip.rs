//! Put files into a silo, back it up, then get them out with nothing but the
//! recovery code.
//!
//! The claim this tool exists to support is "your data is recoverable without
//! us", and the only way to hold that claim honestly is to do it: build a
//! real silo, push it to storage, then read it back through the same path a
//! stranger would, and compare the bytes.

use std::path::Path;

use silentsilo_crypto::{encrypt_file, generate_content_key, wrap_content_key};
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{Vfs, pending_ops};
use uuid::Uuid;

/// Adds a file to a silo the way the application does: content under a key of
/// its own, that key wrapped under the silo's content KEK and carried in the
/// record.
fn add_file(session: &VaultSession, folder: Uuid, name: &str, contents: &[u8]) -> Uuid {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(name);
    std::fs::write(&source, contents).unwrap();

    let file_id = Uuid::now_v7();
    let blob_id = Uuid::new_v4();
    let content_key = generate_content_key();
    let wrapped = wrap_content_key(&content_key, &session.kek).unwrap();
    let result = encrypt_file(
        &source,
        &session.paths.blob_path(blob_id),
        &content_key,
        file_id,
        blob_id,
    )
    .unwrap();

    Vfs::new(session)
        .add_file(
            folder,
            name,
            blob_id,
            contents.len() as i64,
            &hex::encode(result.header.content_hash),
            None,
            &wrapped,
        )
        .unwrap();
    blob_id
}

/// Everything the application publishes, so what lands in storage is what a
/// real backup holds and not a convenient subset.
async fn publish(session: &VaultSession, store: &dyn ObjectStore, blobs: &[Uuid]) -> String {
    silentsilo_sync::put_manifest(store, session.vault_id)
        .await
        .unwrap();
    silentsilo_sync::push_ops(store, &session.dek, &pending_ops(&session.conn).unwrap())
        .await
        .unwrap();
    silentsilo_sync::push_blobs(store, &session.paths.root, blobs)
        .await
        .unwrap();

    let envelope = silentsilo_vault::wrap_kek_bytes(&session.kek, &session.dek).unwrap();
    silentsilo_sync::publish_content_kek(store, &envelope)
        .await
        .unwrap();

    let (code, recovery) = silentsilo_vault::create_recovery_envelope(&session.dek).unwrap();
    silentsilo_sync::push_recovery_envelope(store, &recovery)
        .await
        .unwrap();
    code
}

fn silo(dir: &Path) -> VaultSession {
    let session =
        VaultSession::provision(dir.to_path_buf(), Uuid::new_v4(), "extract-test").unwrap();
    Vfs::new(&session).ensure_initialized().unwrap();
    session
}

#[tokio::test]
async fn a_backup_and_a_code_give_the_files_back() {
    let silo_dir = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(store_dir.path().to_path_buf());

    let session = silo(silo_dir.path());
    let vfs = Vfs::new(&session);
    let root = vfs.root_folder_id().unwrap();
    let docs = vfs.create_folder(root, "Acte").unwrap();

    // A file at the root and one in a folder, with contents worth comparing
    // byte for byte rather than by length.
    let a = add_file(&session, root, "notes.txt", b"the quick brown fox");
    let b = add_file(&session, docs.id, "contract.bin", &vec![7u8; 40_000]);

    let code = publish(&session, &store as &dyn ObjectStore, &[a, b]).await;

    // From here on, nothing knows about the silo above: a folder and a code.
    let backup = silentsilo_extract::open(&store as &dyn ObjectStore, &code)
        .await
        .expect("the backup opens with the code alone");
    assert_eq!(backup.file_count(), 2);

    let out = silentsilo_extract::extract(
        &backup,
        &store as &dyn ObjectStore,
        out_dir.path(),
        &mut |_, _, _| {},
    )
    .await
    .unwrap();

    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert_eq!(out.written, 2);
    assert_eq!(
        std::fs::read(out_dir.path().join("notes.txt")).unwrap(),
        b"the quick brown fox",
        "the file came back byte for byte"
    );
    assert_eq!(
        std::fs::read(out_dir.path().join("Acte").join("contract.bin")).unwrap(),
        vec![7u8; 40_000],
        "and so did the one that spans chunks"
    );
}

#[tokio::test]
async fn the_wrong_code_opens_nothing() {
    let silo_dir = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(store_dir.path().to_path_buf());

    let session = silo(silo_dir.path());
    let root = Vfs::new(&session).root_folder_id().unwrap();
    let blob = add_file(&session, root, "secret.txt", b"not for you");
    publish(&session, &store as &dyn ObjectStore, &[blob]).await;

    let wrong = silentsilo_extract::open(&store as &dyn ObjectStore, "AAAA-BBBB-CCCC-DDDD").await;
    assert!(wrong.is_err(), "a guessed code must not open a backup");
}

/// A folder with nothing of ours in it should say so plainly rather than
/// producing an empty result that reads like a silo with no files.
#[tokio::test]
async fn a_folder_that_is_not_a_backup_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("holiday.jpg"), b"not a silo").unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());

    let result = silentsilo_extract::open(&store as &dyn ObjectStore, "ANY-CODE").await;
    assert!(matches!(
        result,
        Err(silentsilo_extract::ExtractError::NoSilo)
    ));
}

/// Trashed files are written out to one side rather than dropped. Someone
/// running this is recovering, and a file they had moved to the trash but not
/// finished deleting is not ours to decide about.
#[tokio::test]
async fn trashed_files_come_out_under_their_own_folder() {
    let silo_dir = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(store_dir.path().to_path_buf());

    let session = silo(silo_dir.path());
    let vfs = Vfs::new(&session);
    let root = vfs.root_folder_id().unwrap();
    let kept = add_file(&session, root, "kept.txt", b"keep me");
    let binned = add_file(&session, root, "binned.txt", b"was deleted");

    let file_id = vfs
        .list_folder(root)
        .unwrap()
        .into_iter()
        .find_map(|e| match e {
            silentsilo_core::VaultEntry::File(f) if f.name == "binned.txt" => Some(f.id),
            _ => None,
        })
        .expect("the file is there before it is binned");
    vfs.trash_file(file_id).unwrap();

    let code = publish(&session, &store as &dyn ObjectStore, &[kept, binned]).await;
    let backup = silentsilo_extract::open(&store as &dyn ObjectStore, &code)
        .await
        .unwrap();
    assert_eq!(backup.file_count(), 1);
    assert_eq!(backup.trashed_count(), 1);

    silentsilo_extract::extract(
        &backup,
        &store as &dyn ObjectStore,
        out_dir.path(),
        &mut |_, _, _| {},
    )
    .await
    .unwrap();

    assert!(out_dir.path().join("kept.txt").exists());
    assert_eq!(
        std::fs::read(out_dir.path().join("_trash").join("binned.txt")).unwrap(),
        b"was deleted"
    );
}
