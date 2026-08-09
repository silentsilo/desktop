//! What a target that never accepts a delete gets sent, and what it does not.
//!
//! A folder store, which accepts every delete happily, so these prove the
//! app withholds the delete rather than the storage refusing it. That is the
//! distinction that matters: the promise has to hold on storage that would
//! have complied, or it is not the app keeping it.

use silentsilo_crypto::generate_dek;
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{
    OPS_PREFIX, latest_snapshot, publish_compaction, publish_key_envelopes, push_ops,
};
use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};
use silentsilo_vfs::{OpRecord, Snapshot, VaultOp};
use uuid::Uuid;

fn store(dir: &tempfile::TempDir) -> FolderStore {
    FolderStore::new(dir.path().to_path_buf())
}

fn credential(id: &str, revoked: bool) -> StoredFidoCredential {
    StoredFidoCredential {
        kind: silentsilo_vault::KIND_FIDO2.to_string(),
        credential_id: id.into(),
        public_key: "cafe".into(),
        key_slot: 0,
        rp_id: "silentsilo.com".into(),
        label: "Key".into(),
        wrapped_dek: "deadbeef".into(),
        platform: false,
        revoked,
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

/// The one that would be a lie. Removing a security key deletes its envelope,
/// which is what lets that key unlock the silo from any device. On a target
/// the app never deletes from, the envelope stays and the key still works, so
/// the report has to say so instead of counting it as revoked.
#[tokio::test]
async fn a_revoked_envelope_is_left_alone_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let published = StoredFidoKeys {
        keys: vec![credential("aa11", false)],
    };
    publish_key_envelopes(&client as &dyn ObjectStore, &published, false)
        .await
        .unwrap();

    let revoked = StoredFidoKeys {
        keys: vec![credential("aa11", true)],
    };
    let report = publish_key_envelopes(&client as &dyn ObjectStore, &revoked, false)
        .await
        .unwrap();

    assert!(report.revoked.is_empty(), "nothing was actually revoked");
    assert_eq!(report.withheld, vec!["aa11".to_string()]);
    assert_eq!(
        client.list("keys/").await.unwrap().len(),
        1,
        "the envelope is still readable, which is what withheld means"
    );
}

/// The same call on a working target does delete, so the test above is about
/// the role rather than about the storage or the call being broken.
#[tokio::test]
async fn a_working_target_still_revokes() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![credential("aa11", false)],
        },
        true,
    )
    .await
    .unwrap();

    let report = publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![credential("aa11", true)],
        },
        true,
    )
    .await
    .unwrap();

    assert_eq!(report.revoked, vec!["aa11".to_string()]);
    assert!(report.withheld.is_empty());
    assert!(client.list("keys/").await.unwrap().is_empty());
}

/// Compaction on an append-only target writes the snapshot and keeps every
/// record. The snapshot is still worth sending: it is a write to a fresh key,
/// and it is what a joining device replays from instead of the log's start.
#[tokio::test]
async fn compaction_publishes_the_snapshot_and_prunes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);
    let dek = generate_dek();
    let vault_id = Uuid::new_v4();
    let device = Uuid::new_v4();

    let records: Vec<OpRecord> = (1..=5).map(|l| record(l, device)).collect();
    push_ops(&client as &dyn ObjectStore, &dek, &records)
        .await
        .unwrap();

    let snapshot = snapshot_at(vault_id, 3);
    let outcome = publish_compaction(&client as &dyn ObjectStore, &dek, &snapshot, false)
        .await
        .unwrap();

    assert_eq!(outcome.deleted, 0);
    assert!(outcome.failed.is_empty(), "nothing was even attempted");
    assert_eq!(
        client.list(OPS_PREFIX).await.unwrap().len(),
        5,
        "every record is still there, including the three below the horizon"
    );
    assert_eq!(
        latest_snapshot(&client as &dyn ObjectStore, &dek)
            .await
            .unwrap()
            .map(|s| s.horizon),
        Some(3),
        "the snapshot itself did go up"
    );
}
