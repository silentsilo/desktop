//! Blob content moving through a real bucket.
//!
//! Skipped unless `SILENTSILO_TEST_S3_ENDPOINT` is set — see
//! `silentsilo-s3`'s `live_roundtrip` tests for how to bring a server up.

use silentsilo_core::S3Config;
use silentsilo_s3::S3Client;
use silentsilo_store::ObjectStore;
use silentsilo_sync::{fetch_blob, push_blobs, push_pending_blobs};
use silentsilo_vault::{list_local_blob_ids, list_undelivered_blob_ids, record_blob_present};

/// One backup target, the same one across the calls in a test.
fn target() -> Uuid {
    Uuid::from_u128(0x5170_0001)
}
use uuid::Uuid;

struct Vault {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Vault {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        Self { _dir: dir, root }
    }

    /// Writes a blob to disk and registers it as present but not yet
    /// uploaded, which is the state a freshly imported file is in.
    fn add_local_blob(&self, contents: &[u8]) -> Uuid {
        let id = Uuid::new_v4();
        std::fs::write(self.root.join("blobs").join(format!("{id}.sslo")), contents).unwrap();
        record_blob_present(&self.root, id, contents.len() as i64, false).unwrap();
        id
    }

    fn blob_bytes(&self, id: Uuid) -> Option<Vec<u8>> {
        std::fs::read(self.root.join("blobs").join(format!("{id}.sslo"))).ok()
    }

    fn evict(&self, id: Uuid) {
        std::fs::remove_file(self.root.join("blobs").join(format!("{id}.sslo"))).unwrap();
    }
}

fn client() -> Option<S3Client> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    S3Client::new(S3Config {
        endpoint,
        region: "us-east-1".into(),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        prefix: format!("blobs-{}", Uuid::new_v4()),
        access_key_id: std::env::var("SILENTSILO_TEST_S3_KEY")
            .unwrap_or_else(|_| "silentsilo".into()),
        secret_access_key: std::env::var("SILENTSILO_TEST_S3_SECRET")
            .unwrap_or_else(|_| "silentsilo123".into()),
        path_style: true,
    })
    .ok()
}

macro_rules! client_or_skip {
    () => {
        match client() {
            Some(c) => c,
            None => {
                eprintln!("skipped: SILENTSILO_TEST_S3_ENDPOINT is not set");
                return;
            }
        }
    };
}

#[tokio::test]
async fn a_blob_survives_the_round_trip_byte_for_byte() {
    let client = client_or_skip!();
    let source = Vault::new();
    // Binary with NULs and high bytes: blobs are ciphertext, and a transfer
    // that mangled either would corrupt the file beyond recovery.
    let content: Vec<u8> = (0u8..=255).cycle().take(9000).collect();
    let id = source.add_local_blob(&content);

    push_blobs(&client as &dyn ObjectStore, &source.root, target(), &[id])
        .await
        .unwrap();

    let target = Vault::new();
    fetch_blob(&client as &dyn ObjectStore, &target.root, id)
        .await
        .unwrap();
    assert_eq!(target.blob_bytes(id).unwrap(), content);
}

#[tokio::test]
async fn pushing_marks_blobs_as_synced() {
    // Until a blob is confirmed uploaded it is the only copy, so eviction
    // must leave it alone. Marking it is what releases that hold.
    let client = client_or_skip!();
    let vault = Vault::new();
    let id = vault.add_local_blob(b"content");

    assert_eq!(list_undelivered_blob_ids(&vault.root, target()), vec![id]);
    push_pending_blobs(&client as &dyn ObjectStore, &vault.root, target())
        .await
        .unwrap();
    assert!(list_undelivered_blob_ids(&vault.root, target()).is_empty());
}

#[tokio::test]
async fn a_failed_upload_leaves_the_blob_unsynced_for_the_next_attempt() {
    // Marking before the upload lands would make the blob evictable while
    // the bucket has no copy — that is how a file is lost for good.
    let Some(mut cfg) = client().map(|c| c.config().clone()) else {
        eprintln!("skipped: SILENTSILO_TEST_S3_ENDPOINT is not set");
        return;
    };
    cfg.secret_access_key = "wrong-secret".into();
    let broken = S3Client::new(cfg).unwrap();

    let vault = Vault::new();
    let id = vault.add_local_blob(b"precious");

    let outcome = push_blobs(&broken as &dyn ObjectStore, &vault.root, target(), &[id])
        .await
        .unwrap();
    assert_eq!(outcome.uploaded, 0);
    assert_eq!(outcome.failed.len(), 1, "the failure should be reported");
    assert_eq!(
        list_undelivered_blob_ids(&vault.root, target()),
        vec![id],
        "a blob that never reached the bucket must stay unsynced"
    );
}

#[tokio::test]
async fn a_second_push_skips_what_is_already_there() {
    let client = client_or_skip!();
    let vault = Vault::new();
    let id = vault.add_local_blob(b"content");

    let first = push_blobs(&client as &dyn ObjectStore, &vault.root, target(), &[id])
        .await
        .unwrap();
    assert_eq!(first.uploaded, 1);

    // Re-offering it, as a device that lost its cache bookkeeping would.
    record_blob_present(&vault.root, id, 7, false).unwrap();
    let second = push_blobs(&client as &dyn ObjectStore, &vault.root, target(), &[id])
        .await
        .unwrap();
    assert_eq!(second.uploaded, 0);
    assert_eq!(second.already_present, 1);
    assert!(
        list_undelivered_blob_ids(&vault.root, target()).is_empty(),
        "finding it already present should still release the eviction hold"
    );
}

#[tokio::test]
async fn an_evicted_blob_can_be_fetched_back() {
    let client = client_or_skip!();
    let vault = Vault::new();
    let content = b"the original bytes".to_vec();
    let id = vault.add_local_blob(&content);
    push_blobs(&client as &dyn ObjectStore, &vault.root, target(), &[id])
        .await
        .unwrap();

    vault.evict(id);
    assert!(vault.blob_bytes(id).is_none());

    fetch_blob(&client as &dyn ObjectStore, &vault.root, id)
        .await
        .unwrap();
    assert_eq!(vault.blob_bytes(id).unwrap(), content);
    assert!(
        list_local_blob_ids(&vault.root).contains(&id),
        "a re-fetched blob should be tracked as present again"
    );
}

#[tokio::test]
async fn a_missing_blob_reports_an_error_rather_than_writing_an_empty_file() {
    let client = client_or_skip!();
    let vault = Vault::new();
    let id = Uuid::new_v4();

    assert!(
        fetch_blob(&client as &dyn ObjectStore, &vault.root, id)
            .await
            .is_err()
    );
    assert!(
        vault.blob_bytes(id).is_none(),
        "a failed download must not leave a file behind"
    );
}

#[tokio::test]
async fn pushing_a_blob_whose_file_is_gone_is_skipped_quietly() {
    // Trashed and purged between listing and upload — nothing to send, and
    // nothing worth failing the pass over.
    let client = client_or_skip!();
    let vault = Vault::new();
    let id = vault.add_local_blob(b"x");
    vault.evict(id);

    let outcome = push_blobs(&client as &dyn ObjectStore, &vault.root, target(), &[id])
        .await
        .unwrap();
    assert_eq!(outcome.uploaded, 0);
    assert!(outcome.failed.is_empty());
}
