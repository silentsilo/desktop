//! Compaction against a real bucket.
//!
//! The folder-store tests next door prove the logic. This proves it survives
//! S3: a LIST that pages, a DELETE that returns success for a key that was
//! never there, and keys that come back in whatever order the server feels
//! like. Skipped unless `SILENTSILO_TEST_S3_ENDPOINT` is set, the same way
//! the other bucket tests are.

use silentsilo_core::S3Config;
use silentsilo_crypto::{MasterDek, generate_dek};
use silentsilo_s3::S3Client;
use silentsilo_sync::{apply_rebuild, compact_if_due, fetch_rebuild, snapshot_horizon, sync_ops};
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{CompactionPolicy, VaultOp, Vfs, clear_pushed, emit, pending_ops};
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

    fn root(&self) -> Uuid {
        Vfs::new(&self.session).root_folder_id().unwrap()
    }

    fn add(&self, name: &str) {
        emit(
            &self.session.conn,
            VaultOp::AddFile {
                id: Uuid::new_v4(),
                folder_id: self.root(),
                name: name.into(),
                blob_id: Uuid::new_v4(),
                size_bytes: 1,
                content_hash: "h".into(),
                mime_type: None,
                blob_key: String::new(),
            },
        )
        .unwrap();
    }

    async fn sync(&self, client: &S3Client, dek: &MasterDek) {
        let pending = pending_ops(&self.session.conn).unwrap();
        sync_ops(&self.session.conn, client, dek, &pending)
            .await
            .unwrap();
        clear_pushed(&self.session.conn, &pending).unwrap();
    }

    fn tree(&self) -> Vec<String> {
        let mut stmt = self
            .session
            .conn
            .prepare("SELECT f.name FROM files f WHERE f.deleted_at IS NULL ORDER BY f.name")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }
}

/// A fresh prefix per test keeps runs from seeing each other's objects.
fn client() -> Option<S3Client> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    S3Client::new(S3Config {
        endpoint,
        region: "us-east-1".into(),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        prefix: format!("compaction-{}", Uuid::new_v4()),
        access_key_id: std::env::var("SILENTSILO_TEST_S3_KEY")
            .unwrap_or_else(|_| "silentsilo".into()),
        secret_access_key: std::env::var("SILENTSILO_TEST_S3_SECRET")
            .unwrap_or_else(|_| "silentsilo123".into()),
        path_style: true,
    })
    .ok()
}

macro_rules! setup {
    ($client:ident, $dek:ident) => {
        let Some($client) = client() else {
            silentsilo_testkit::skip_or_fail("SILENTSILO_TEST_S3_ENDPOINT is not set");
            return;
        };
        let $dek: MasterDek = generate_dek();
    };
}

/// Compacts far enough back to be plausible, but with margins small enough
/// that a test does not have to write five thousand records.
fn eager() -> CompactionPolicy {
    CompactionPolicy {
        retain_seconds: 0,
        keep_recent: 3,
        min_records: 10,
    }
}

#[tokio::test]
async fn a_bucket_survives_a_full_compaction_cycle() {
    setup!(client, dek);
    let vault_id = Uuid::new_v4();
    let mut device = Device::joining(vault_id);

    for i in 0..20 {
        device.add(&format!("file-{i:02}.txt"));
    }
    device.sync(&client, &dek).await;
    let before = device.tree();

    // `now` is far in the future so every record counts as old: the count
    // margin is what decides where the horizon lands, which is the part
    // worth exercising against a real listing.
    let outcome = compact_if_due(
        &mut device.session.conn,
        &client,
        &dek,
        vault_id,
        &eager(),
        i64::MAX / 2,
    )
    .await
    .unwrap()
    .expect("twenty records with three held back is enough to compact");

    assert!(outcome.pruned_remote > 0, "nothing was deleted from S3");
    assert_eq!(
        snapshot_horizon(&client).await.unwrap(),
        outcome.horizon,
        "the snapshot the bucket reports is not the one just written"
    );

    // The whole point: a device that has never seen this silo builds the same
    // tree out of what is left.
    let mut newcomer = Device::joining(vault_id);
    let (snapshot, incoming) = fetch_rebuild(&client, &dek).await.unwrap().unwrap();
    apply_rebuild(&mut newcomer.session.conn, &snapshot, incoming).unwrap();

    assert_eq!(newcomer.tree(), before);
}
