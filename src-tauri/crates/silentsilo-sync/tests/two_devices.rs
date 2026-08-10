//! Two devices reaching the same vault state through a real bucket.
//!
//! The unit tests in `oplog` prove convergence given a set of operations;
//! these prove the transport actually delivers that set. Skipped unless
//! `SILENTSILO_TEST_S3_ENDPOINT` is set — see `silentsilo-s3`'s
//! `live_roundtrip` tests for how to bring a server up.

use rusqlite::Connection;
use silentsilo_core::S3Config;
use silentsilo_crypto::{MasterDek, generate_dek};
use silentsilo_s3::S3Client;
use silentsilo_sync::sync_ops;
use silentsilo_vault::VaultSession;
use silentsilo_vfs::{OpRecord, VaultOp, Vfs, next_lamport};
use uuid::Uuid;

/// A device with its own local vault, sharing one bucket prefix and one DEK
/// with the others in a test — which is what devices in a real vault share.
struct Device {
    _dir: tempfile::TempDir,
    session: VaultSession,
    id: Uuid,
}

impl Device {
    fn joining(vault_id: Uuid) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().to_path_buf(), vault_id, "test-secret").unwrap();
        Vfs::new(&session).ensure_initialized().unwrap();
        Self {
            _dir: dir,
            session,
            id: Uuid::new_v4(),
        }
    }

    fn conn(&self) -> &Connection {
        &self.session.conn
    }

    fn root(&self) -> Uuid {
        Vfs::new(&self.session).root_folder_id().unwrap()
    }

    /// Creates a record from this device, advancing its Lamport clock and
    /// continuing its chain as a real local change would.
    fn author(&self, op: VaultOp) -> OpRecord {
        let (seq, prev) = silentsilo_vfs::chain_tip(self.conn(), self.id).unwrap();
        OpRecord::authored(
            Uuid::new_v4(),
            next_lamport(self.conn()).unwrap(),
            self.id,
            1_700_000_000,
            seq,
            prev,
            op,
        )
    }

    fn tree(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stmt = self
            .conn()
            .prepare("SELECT path FROM folders WHERE deleted_at IS NULL ORDER BY path")
            .unwrap();
        for row in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
            out.push(format!("folder {}", row.unwrap()));
        }
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT fo.path, f.name FROM files f JOIN folders fo ON fo.id = f.folder_id
                 WHERE f.deleted_at IS NULL ORDER BY fo.path, f.name",
            )
            .unwrap();
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
        {
            let (path, name) = row.unwrap();
            out.push(format!("file {path}::{name}"));
        }
        out
    }
}

fn add_file(folder_id: Uuid, name: &str) -> VaultOp {
    VaultOp::AddFile {
        id: Uuid::new_v4(),
        folder_id,
        name: name.into(),
        blob_id: Uuid::new_v4(),
        size_bytes: 1,
        content_hash: "h".into(),
        mime_type: None,
        blob_key: String::new(),
    }
}

/// A fresh prefix per test keeps runs from seeing each other's operations.
fn client() -> Option<S3Client> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    S3Client::new(S3Config {
        endpoint,
        region: "us-east-1".into(),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        prefix: format!("sync-{}", Uuid::new_v4()),
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

#[tokio::test]
async fn one_devices_changes_reach_another() {
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);
    let b = Device::joining(vault);

    let folder = Uuid::new_v4();
    let ops = vec![
        a.author(VaultOp::CreateFolder {
            id: folder,
            parent_id: a.root(),
            name: "Docs".into(),
        }),
        a.author(add_file(folder, "notes.txt")),
    ];
    // A device applies its own work locally as well as pushing it.
    silentsilo_vfs::replay(a.conn(), ops.clone()).unwrap();

    let pushed = sync_ops(a.conn(), &client, &dek, &ops).await.unwrap();
    assert_eq!(pushed.pushed, 2);

    let pulled = sync_ops(b.conn(), &client, &dek, &[]).await.unwrap();
    assert_eq!(pulled.fetched, 2);
    assert_eq!(a.tree(), b.tree());
    assert!(b.tree().contains(&"file /Docs::notes.txt".to_string()));
}

#[tokio::test]
async fn concurrent_edits_from_both_devices_converge() {
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);
    let b = Device::joining(vault);
    let root = a.root();

    // Both add a file with the same name without having seen each other —
    // the case that must not lose either upload.
    let a_ops = vec![a.author(add_file(root, "report.txt"))];
    let b_ops = vec![b.author(add_file(root, "report.txt"))];
    silentsilo_vfs::replay(a.conn(), a_ops.clone()).unwrap();
    silentsilo_vfs::replay(b.conn(), b_ops.clone()).unwrap();

    sync_ops(a.conn(), &client, &dek, &a_ops).await.unwrap();
    sync_ops(b.conn(), &client, &dek, &b_ops).await.unwrap();
    // A second pass each way, so both have seen everything.
    sync_ops(a.conn(), &client, &dek, &[]).await.unwrap();
    sync_ops(b.conn(), &client, &dek, &[]).await.unwrap();

    assert_eq!(a.tree(), b.tree());
    let files: Vec<_> = a
        .tree()
        .into_iter()
        .filter(|l| l.starts_with("file "))
        .collect();
    assert_eq!(files.len(), 2, "neither upload may be lost: {files:?}");
}

#[tokio::test]
async fn a_second_sync_transfers_nothing_new() {
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);
    let ops = vec![a.author(add_file(a.root(), "a.txt"))];
    silentsilo_vfs::replay(a.conn(), ops.clone()).unwrap();

    sync_ops(a.conn(), &client, &dek, &ops).await.unwrap();
    let second = sync_ops(a.conn(), &client, &dek, &ops).await.unwrap();

    assert_eq!(
        second.pushed, 0,
        "immutable records should not be re-uploaded"
    );
    assert_eq!(
        second.replay.applied, 0,
        "nothing new should be applied on an idle sync"
    );
    // The fetch diffs the listing against the op ids already held, so an
    // idle pass downloads nothing at all.
    assert_eq!(
        second.fetched, 0,
        "an idle sync should refetch nothing, got {}",
        second.fetched
    );
}

#[tokio::test]
async fn a_brand_new_device_builds_the_vault_from_the_log_alone() {
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);

    let folder = Uuid::new_v4();
    let ops = vec![
        a.author(VaultOp::CreateFolder {
            id: folder,
            parent_id: a.root(),
            name: "Archive".into(),
        }),
        a.author(add_file(folder, "old.txt")),
        a.author(VaultOp::RenameFolder {
            id: folder,
            name: "Records".into(),
        }),
    ];
    silentsilo_vfs::replay(a.conn(), ops.clone()).unwrap();
    sync_ops(a.conn(), &client, &dek, &ops).await.unwrap();

    // No snapshot involved: the root id is derived from the vault id, so a
    // device that has never synced can resolve every operation.
    let fresh = Device::joining(vault);
    sync_ops(fresh.conn(), &client, &dek, &[]).await.unwrap();

    assert_eq!(fresh.tree(), a.tree());
    assert!(fresh.tree().contains(&"file /Records::old.txt".to_string()));
}

#[tokio::test]
async fn stored_operations_are_not_readable_without_the_key() {
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);
    let ops = vec![a.author(add_file(a.root(), "salaries-2026.xlsx"))];
    sync_ops(a.conn(), &client, &dek, &ops).await.unwrap();

    // Operations carry file names, so the provider must not be able to read
    // them any more than it can read blob content.
    let listed = client.list("ops/").await.unwrap();
    assert_eq!(listed.len(), 1);
    let raw = client.get(&listed[0].key).await.unwrap();
    let as_text = String::from_utf8_lossy(&raw);
    assert!(
        !as_text.contains("salaries"),
        "the filename leaked into stored ciphertext"
    );

    let other_dek = generate_dek();
    assert!(
        silentsilo_crypto::unseal(&raw, &other_dek).is_err(),
        "a different key must not decrypt the record"
    );
}

#[tokio::test]
async fn devices_sitting_at_the_same_lamport_still_see_each_other() {
    // Regression for a silent data-loss bug: the fetch bound used to be
    // exclusive, so two devices that had each independently reached the same
    // Lamport value treated the other's records as already seen. Both
    // uploads were in the bucket; neither device ever downloaded the other.
    setup!(client, dek);
    let vault = Uuid::new_v4();
    let a = Device::joining(vault);
    let b = Device::joining(vault);

    // Both clocks start at zero, so both records land on Lamport 1.
    let a_ops = vec![a.author(add_file(a.root(), "from-a.txt"))];
    let b_ops = vec![b.author(add_file(b.root(), "from-b.txt"))];
    assert_eq!(a_ops[0].lamport, b_ops[0].lamport, "the premise of the bug");

    silentsilo_vfs::replay(a.conn(), a_ops.clone()).unwrap();
    silentsilo_vfs::replay(b.conn(), b_ops.clone()).unwrap();
    sync_ops(a.conn(), &client, &dek, &a_ops).await.unwrap();
    sync_ops(b.conn(), &client, &dek, &b_ops).await.unwrap();
    sync_ops(a.conn(), &client, &dek, &[]).await.unwrap();

    let tree = a.tree();
    assert!(tree.contains(&"file /::from-a.txt".to_string()), "{tree:?}");
    assert!(tree.contains(&"file /::from-b.txt".to_string()), "{tree:?}");
}
