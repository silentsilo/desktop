//! One set of assertions, run against every backend: a backend that passes
//! here can host a silo, and one that does not fails in ways the sync
//! layer cannot see. The S3 pass skips without
//! `SILENTSILO_TEST_S3_ENDPOINT`; the folder pass always runs, so the
//! contract is exercised on every `cargo test`.

use silentsilo_core::S3Config;
use silentsilo_store::{
    FolderStore, ObjectStore, SftpAuth, SftpConfig, SftpStore, StoreError, WebDavConfig,
    WebDavStore, probe_host_key,
};
use uuid::Uuid;

fn folder_store() -> (tempfile::TempDir, Box<dyn ObjectStore>) {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::new(dir.path().to_path_buf());
    (dir, Box::new(store))
}

fn s3_store() -> Option<Box<dyn ObjectStore>> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    let client = silentsilo_s3::S3Client::new(S3Config {
        endpoint,
        region: "us-east-1".into(),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        prefix: format!("contract-{}", Uuid::new_v4()),
        access_key_id: std::env::var("SILENTSILO_TEST_S3_KEY")
            .unwrap_or_else(|_| "silentsilo".into()),
        secret_access_key: std::env::var("SILENTSILO_TEST_S3_SECRET")
            .unwrap_or_else(|_| "silentsilo123".into()),
        path_style: true,
    })
    .ok()?;
    Some(Box::new(client))
}

/// Each run gets its own collection, so a failing test cannot leave state
/// that makes the next one pass or fail for the wrong reason.
fn webdav_store() -> Option<Box<dyn ObjectStore>> {
    let base = std::env::var("SILENTSILO_TEST_WEBDAV_URL").ok()?;
    WebDavStore::new(WebDavConfig {
        url: format!("{}/contract-{}", base.trim_end_matches('/'), Uuid::new_v4()),
        username: std::env::var("SILENTSILO_TEST_WEBDAV_USER")
            .unwrap_or_else(|_| "silentsilo".into()),
        password: std::env::var("SILENTSILO_TEST_WEBDAV_PASSWORD")
            .unwrap_or_else(|_| "silentsilo123".into()),
    })
    .ok()
    .map(|s| Box::new(s) as Box<dyn ObjectStore>)
}

/// The host key is learned first, exactly as the app does it — which also
/// means this test would fail if pinning were broken, since a store with no
/// confirmed fingerprint refuses to be built at all.
async fn sftp_store() -> Option<Box<dyn ObjectStore>> {
    let host = std::env::var("SILENTSILO_TEST_SFTP_HOST").ok()?;
    let port: u16 = std::env::var("SILENTSILO_TEST_SFTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222);
    let fingerprint = probe_host_key(&host, port).await.ok()?;

    SftpStore::new(SftpConfig {
        host,
        port,
        username: std::env::var("SILENTSILO_TEST_SFTP_USER")
            .unwrap_or_else(|_| "silentsilo".into()),
        auth: SftpAuth::Password {
            password: std::env::var("SILENTSILO_TEST_SFTP_PASSWORD")
                .unwrap_or_else(|_| "silentsilo123".into()),
        },
        path: format!("silo/contract-{}", Uuid::new_v4()),
        host_fingerprint: Some(fingerprint),
    })
    .ok()
    .map(|s| Box::new(s) as Box<dyn ObjectStore>)
}

/// Runs `body` against every backend available on this machine.
async fn for_each_store<F, Fut>(body: F)
where
    F: Fn(Box<dyn ObjectStore>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let (_dir, folder) = folder_store();
    body(folder).await;

    match s3_store() {
        Some(s3) => body(s3).await,
        None => silentsilo_testkit::skip_or_fail("S3: SILENTSILO_TEST_S3_ENDPOINT is not set"),
    }

    match webdav_store() {
        Some(dav) => body(dav).await,
        None => silentsilo_testkit::skip_or_fail("WebDAV: SILENTSILO_TEST_WEBDAV_URL is not set"),
    }

    match sftp_store().await {
        Some(sftp) => body(sftp).await,
        None => silentsilo_testkit::skip_or_fail("SFTP: SILENTSILO_TEST_SFTP_HOST is not set"),
    }
}

#[tokio::test]
async fn an_object_survives_the_round_trip_byte_for_byte() {
    for_each_store(|store| async move {
        // Binary with NULs and high bytes: everything stored is ciphertext,
        // and a backend that mangled either would corrupt a file beyond
        // recovery while looking like it worked.
        let content: Vec<u8> = (0u8..=255).cycle().take(5000).collect();
        store.put("ops/000001.op", content.clone()).await.unwrap();

        assert_eq!(store.get("ops/000001.op").await.unwrap(), content);
    })
    .await;
}

#[tokio::test]
async fn head_answers_without_moving_the_bytes() {
    for_each_store(|store| async move {
        assert_eq!(store.head("blobs/missing.sslo").await.unwrap(), None);

        store.put("blobs/a.sslo", vec![7; 1234]).await.unwrap();
        assert_eq!(store.head("blobs/a.sslo").await.unwrap(), Some(1234));
    })
    .await;
}

#[tokio::test]
async fn listing_is_ordered_by_key() {
    for_each_store(|store| async move {
        // Operation keys are the Lamport counter zero-padded, so
        // lexicographic order is logical order. Written out of order on
        // purpose — a backend that echoed insertion order would pass a
        // weaker test than this.
        for n in [7u32, 1, 30, 2] {
            store
                .put(&format!("ops/{n:020}.op"), vec![0])
                .await
                .unwrap();
        }

        let keys: Vec<String> = store
            .list("ops/")
            .await
            .unwrap()
            .into_iter()
            .map(|o| o.key)
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "callers rely on lexicographic order");
        assert_eq!(keys.len(), 4);
    })
    .await;
}

#[tokio::test]
async fn listing_an_empty_prefix_is_not_an_error() {
    // The normal state of storage nothing has been written to yet.
    for_each_store(|store| async move {
        assert!(store.list("ops/").await.unwrap().is_empty());
    })
    .await;
}

#[tokio::test]
async fn listing_does_not_leak_a_neighbouring_prefix() {
    for_each_store(|store| async move {
        store.put("ops/000001.op", vec![1]).await.unwrap();
        store.put("blobs/a.sslo", vec![2]).await.unwrap();

        let keys: Vec<String> = store
            .list("ops/")
            .await
            .unwrap()
            .into_iter()
            .map(|o| o.key)
            .collect();
        assert_eq!(keys, vec!["ops/000001.op".to_string()]);
    })
    .await;
}

#[tokio::test]
async fn deleting_something_absent_is_the_desired_end_state() {
    // Callers use delete to clean up after a failure, where the object may
    // never have landed.
    for_each_store(|store| async move {
        store.delete("blobs/never-existed.sslo").await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_deleted_object_is_gone_from_both_head_and_list() {
    for_each_store(|store| async move {
        store.put("blobs/a.sslo", vec![1, 2, 3]).await.unwrap();
        store.delete("blobs/a.sslo").await.unwrap();

        assert_eq!(store.head("blobs/a.sslo").await.unwrap(), None);
        assert!(store.list("blobs/").await.unwrap().is_empty());
    })
    .await;
}

#[tokio::test]
async fn reading_something_absent_reports_not_found() {
    for_each_store(|store| async move {
        let err = store.get("ops/nothing-here.op").await.unwrap_err();
        assert!(
            matches!(err, StoreError::NotFound(_)),
            "callers distinguish a missing object from a broken connection: got {err:?}"
        );
    })
    .await;
}

#[tokio::test]
async fn rewriting_a_key_replaces_it() {
    // Not needed by the operation log, which never rewrites — but the
    // manifest and the key envelopes do.
    for_each_store(|store| async move {
        store.put("vault.json", b"first".to_vec()).await.unwrap();
        store.put("vault.json", b"second".to_vec()).await.unwrap();

        assert_eq!(store.get("vault.json").await.unwrap(), b"second");
        assert_eq!(
            store.list("").await.unwrap().len(),
            1,
            "one key, one object"
        );
    })
    .await;
}

#[tokio::test]
async fn the_write_check_round_trips_and_leaves_nothing_behind() {
    for_each_store(|store| async move {
        store.check().await.unwrap();
        assert!(
            store.list("").await.unwrap().is_empty(),
            "a probe left in the user's own storage is litter"
        );
    })
    .await;
}

#[tokio::test]
async fn sftp_refuses_a_server_whose_key_is_not_the_pinned_one() {
    // The unit tests cover refusing to build a store with no fingerprint.
    // This covers the case that actually protects the user: a real server,
    // reachable and offering a real key, that is not the expected one.
    let Ok(host) = std::env::var("SILENTSILO_TEST_SFTP_HOST") else {
        silentsilo_testkit::skip_or_fail("SFTP pinning: SILENTSILO_TEST_SFTP_HOST is not set");
        return;
    };

    let store = SftpStore::new(SftpConfig {
        host,
        port: 2222,
        username: "silentsilo".into(),
        auth: SftpAuth::Password {
            password: "silentsilo123".into(),
        },
        path: "silo".into(),
        host_fingerprint: Some("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into()),
    })
    .unwrap();

    let err = store.list("").await.unwrap_err();
    assert!(
        matches!(err, StoreError::Denied(_)),
        "a changed host key must be refused, not retried as a network problem: got {err:?}"
    );
    assert!(
        err.to_string().contains("identity has changed"),
        "the message has to say what happened: got {err}"
    );
}

#[tokio::test]
async fn a_folder_store_refuses_a_key_that_climbs_out_of_it() {
    // Only reachable through a bug or a tampered silo, but this is the one
    // backend where a bad key becomes a write anywhere on the user's disk.
    let (dir, store) = folder_store();
    assert!(store.put("../escaped.txt", vec![1]).await.is_err());
    assert!(!dir.path().parent().unwrap().join("escaped.txt").exists());
}

#[tokio::test]
async fn a_folder_store_hides_partial_writes_from_listings() {
    // A sync client watching the folder, or a rename that never finished.
    // Reporting one as an object would hand the caller a truncated file.
    let (dir, store) = folder_store();
    store.put("ops/000001.op", vec![1]).await.unwrap();
    std::fs::create_dir_all(dir.path().join("ops")).unwrap();
    std::fs::write(dir.path().join("ops/000002.op.part"), b"half").unwrap();

    let keys: Vec<String> = store
        .list("ops/")
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.key)
        .collect();
    assert_eq!(keys, vec!["ops/000001.op".to_string()]);
}

#[tokio::test]
#[cfg(unix)]
async fn a_folder_store_survives_a_link_pointing_back_at_itself() {
    // The target is a directory the user chose, so it can hold anything,
    // including a link to an ancestor. `is_dir()` follows those, and the
    // walk went round in a circle until the stack ran out: a sweep against
    // such a folder took the app down rather than reporting anything.
    let (dir, store) = folder_store();
    store.put("ops/000001.op", vec![1]).await.unwrap();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("ops/loop")).unwrap();

    let keys: Vec<String> = store
        .list("ops/")
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.key)
        .collect();
    assert!(
        keys.contains(&"ops/000001.op".to_string()),
        "the real object was lost: {keys:?}"
    );
}

#[tokio::test]
async fn a_folder_store_lists_a_nested_tree_once() {
    // The guard skips a directory it has already walked, so the ordinary
    // case has to keep working: nothing legitimate may be dropped, and
    // nothing may be reported twice.
    let (_dir, store) = folder_store();
    store.put("ops/000001.op", vec![1]).await.unwrap();
    store.put("ops/nested/000002.op", vec![2]).await.unwrap();

    let keys: Vec<String> = store
        .list("ops/")
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.key)
        .collect();
    assert_eq!(
        keys,
        vec![
            "ops/000001.op".to_string(),
            "ops/nested/000002.op".to_string()
        ]
    );
}

/// Blobs move through disk rather than memory, so every backend has to
/// implement the file-shaped transfer as well as the buffer-shaped one.
/// Overridden natively by each of them and easy to get subtly wrong: a
/// truncated upload or a download that drops its tail is a file the user
/// cannot open, and nothing above this layer would notice.
#[tokio::test]
async fn a_file_survives_the_round_trip_through_disk() {
    for_each_store(|store| async move {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("blob.sslo");
        let restored = dir.path().join("restored.sslo");
        // Bigger than one buffer of every streaming loop in the backends,
        // and not a round multiple of any of them, so a boundary bug shows.
        let bytes: Vec<u8> = (0..(1024 * 1024 + 7)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&source, &bytes).unwrap();

        store
            .put_from_file("blobs/streamed.sslo", &source)
            .await
            .unwrap();

        assert_eq!(
            store.head("blobs/streamed.sslo").await.unwrap(),
            Some(bytes.len() as i64),
            "{}: the stored object is not the size that went up",
            store.describe()
        );
        store
            .get_to_file("blobs/streamed.sslo", &restored)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(&restored).unwrap(),
            bytes,
            "{}: the bytes came back different",
            store.describe()
        );

        // And the buffer-shaped read agrees with the file-shaped one, since
        // callers mix them: verify streams, exports do not.
        assert_eq!(
            store.get("blobs/streamed.sslo").await.unwrap(),
            bytes,
            "{}: get and get_to_file disagree",
            store.describe()
        );
    })
    .await;
}

/// An empty file is a real case: a zero-byte document, or a blob whose
/// content was truncated before it was sealed. A backend that refuses one,
/// or turns it into a missing object, breaks the sync pass rather than the
/// file.
#[tokio::test]
async fn an_empty_file_round_trips_too() {
    for_each_store(|store| async move {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("empty.bin");
        let restored = dir.path().join("restored.bin");
        std::fs::write(&source, b"").unwrap();

        store
            .put_from_file("blobs/empty.sslo", &source)
            .await
            .unwrap();

        assert_eq!(
            store.head("blobs/empty.sslo").await.unwrap(),
            Some(0),
            "{}: an empty object went missing",
            store.describe()
        );
        store
            .get_to_file("blobs/empty.sslo", &restored)
            .await
            .unwrap();
        assert!(std::fs::read(&restored).unwrap().is_empty());
    })
    .await;
}

/// Downloading something that is not there must not leave a file behind:
/// the caller renames what it fetched into the blob cache, and an empty
/// file there reads as content that decrypts to nothing.
#[tokio::test]
async fn a_download_that_finds_nothing_reports_it() {
    for_each_store(|store| async move {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nothing.sslo");

        let result = store.get_to_file("blobs/absent.sslo", &dest).await;

        assert!(
            matches!(result, Err(StoreError::NotFound(_))),
            "{}: expected NotFound, got {result:?}",
            store.describe()
        );
    })
    .await;
}
