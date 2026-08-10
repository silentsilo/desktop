//! Rebuilding a device's copy in place after its local database is lost:
//! the flow behind the app's repair command, minus the Tauri shell. The
//! silo folder and its blobs stay; everything derived comes back from
//! storage through the same join plan a new device uses.

use silentsilo_store::FolderStore;
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{Vfs, digest, digest_difference, pending_ops};
use uuid::Uuid;

#[tokio::test]
async fn a_device_rebuilds_in_place_from_its_storage() {
    let silo_dir = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(store_dir.path().to_path_buf());
    let root = silo_dir.path().to_path_buf();
    let vault_id = Uuid::new_v4();

    let session = VaultSession::provision(root.clone(), vault_id, "first-secret").unwrap();
    Vfs::new(&session).ensure_initialized().unwrap();
    let dek = session.dek.clone();
    let kek = session.kek.clone();

    let vfs = Vfs::new(&session);
    let tree_root = vfs.root_folder_id().unwrap();
    let docs = vfs.create_folder(tree_root, "Acte").unwrap();
    vfs.create_folder(docs.id, "2026").unwrap();
    vfs.upsert_password(Uuid::new_v4(), r#"{"id":"e1","service":"example"}"#)
        .unwrap();

    silentsilo_sync::push_ops(&store, &dek, &pending_ops(&session.conn).unwrap())
        .await
        .unwrap();
    let before = digest(&session.conn).unwrap();

    // A blob sits in the silo folder; the rebuild must not lose it.
    let blob_path = session.paths.blob_path(Uuid::new_v4());
    std::fs::write(&blob_path, b"ciphertext").unwrap();

    // Total local loss: the working copy, both encrypted snapshots, the
    // machine-local bookkeeping. What the repair command deletes and wipes.
    let paths = session.paths.clone();
    drop(session);
    silentsilo_vault::wipe_plaintext_working_copy(&paths);
    std::fs::remove_file(paths.db_enc_path()).unwrap();
    let _ = std::fs::remove_file(paths.db_enc_backup_path());

    // The rebuild, exactly as the command runs it.
    let mut rebuilt =
        VaultSession::provision_with_dek(root, vault_id, "fresh-secret", dek.clone(), kek).unwrap();
    Vfs::new(&rebuilt).ensure_initialized().unwrap();
    let plan = silentsilo_sync::fetch_join_plan(&store, &dek)
        .await
        .unwrap();
    plan.apply(&mut rebuilt.conn).unwrap();

    let differences = digest_difference(&before, &digest(&rebuilt.conn).unwrap());
    assert!(differences.is_empty(), "{differences:?}");
    assert!(blob_path.is_file(), "the rebuild lost content on disk");
}
