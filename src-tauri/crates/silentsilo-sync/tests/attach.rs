//! What connecting a backup location may and may not walk into.
//!
//! The case that prompted this: a silo deleted locally, a new one made, and
//! the old silo's SFTP path attached to it. The pass then overwrote the old
//! vault's manifest and key envelopes, destroying that backup, and failed
//! forever on records the new key cannot open. The gate lives at attach
//! time because by the first pass it is already too late.

use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{put_manifest, refuse_foreign_vault};
use uuid::Uuid;

fn store() -> (tempfile::TempDir, FolderStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    (dir, store)
}

#[tokio::test]
async fn an_empty_place_is_accepted() {
    let (_dir, store) = store();
    refuse_foreign_vault(&store as &dyn ObjectStore, Uuid::new_v4())
        .await
        .unwrap();
}

#[tokio::test]
async fn this_silo_s_own_old_backup_is_accepted() {
    let (_dir, store) = store();
    let vault_id = Uuid::new_v4();
    put_manifest(&store as &dyn ObjectStore, vault_id)
        .await
        .unwrap();
    refuse_foreign_vault(&store as &dyn ObjectStore, vault_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn another_vault_s_home_is_refused_by_name() {
    let (_dir, store) = store();
    put_manifest(&store as &dyn ObjectStore, Uuid::new_v4())
        .await
        .unwrap();

    let err = refuse_foreign_vault(&store as &dyn ObjectStore, Uuid::new_v4())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("different silo"), "got: {err}");
}

#[tokio::test]
async fn a_manifest_from_a_newer_build_still_says_to_update() {
    let (_dir, store) = store();
    let future = serde_json::json!({ "vault_id": Uuid::new_v4(), "version": 999 });
    store
        .put("vault.json", serde_json::to_vec(&future).unwrap())
        .await
        .unwrap();

    let err = refuse_foreign_vault(&store as &dyn ObjectStore, Uuid::new_v4())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("newer version"), "got: {err}");
}
