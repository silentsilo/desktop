//! End-to-end checks against a real S3-compatible server.
//!
//! Skipped unless `SILENTSILO_TEST_S3_ENDPOINT` is set, so `cargo test` stays
//! green on a machine with no server running. To run them:
//!
//! ```text
//! docker run -d --name silentsilo-minio -p 9100:9000 \
//!   -e MINIO_ROOT_USER=silentsilo -e MINIO_ROOT_PASSWORD=silentsilo123 \
//!   minio/minio server /data
//! # create a bucket named `vault-test`, then:
//! SILENTSILO_TEST_S3_ENDPOINT=http://localhost:9100 \
//! SILENTSILO_TEST_S3_KEY=silentsilo \
//! SILENTSILO_TEST_S3_SECRET=silentsilo123 \
//! SILENTSILO_TEST_S3_BUCKET=vault-test \
//!   cargo test -p silentsilo-s3 --test live_roundtrip
//! ```
//!
//! These exist because the unit tests only cover construction. Signing,
//! header compatibility and response parsing need a real server, and they
//! are exactly where an S3 client breaks against non-AWS providers.

use silentsilo_core::S3Config;
use silentsilo_s3::S3Client;
use uuid::Uuid;

fn config() -> Option<S3Config> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    Some(S3Config {
        endpoint,
        region: std::env::var("SILENTSILO_TEST_S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        // A unique prefix per run, so repeated runs and parallel tests never
        // see each other's objects.
        prefix: format!("test-{}", Uuid::new_v4()),
        access_key_id: std::env::var("SILENTSILO_TEST_S3_KEY")
            .unwrap_or_else(|_| "silentsilo".into()),
        secret_access_key: std::env::var("SILENTSILO_TEST_S3_SECRET")
            .unwrap_or_else(|_| "silentsilo123".into()),
        path_style: true,
    })
}

macro_rules! client_or_skip {
    () => {
        match config() {
            Some(c) => S3Client::new(c).expect("client should build"),
            None => {
                eprintln!("skipped: SILENTSILO_TEST_S3_ENDPOINT is not set");
                return;
            }
        }
    };
}

#[tokio::test]
async fn test_connection_round_trips_a_probe_object() {
    let client = client_or_skip!();
    client
        .test_connection()
        .await
        .expect("probe write/read/delete should succeed");
}

#[tokio::test]
async fn an_object_survives_a_put_and_get_unchanged() {
    let client = client_or_skip!();
    // Deliberately binary, including a NUL and high bytes: blobs are
    // ciphertext, not text, and a client that mangles either would corrupt
    // every file in the vault.
    let payload: Vec<u8> = (0u8..=255).cycle().take(5000).collect();

    client
        .put("blobs/binary.sslo", payload.clone())
        .await
        .unwrap();
    let fetched = client.get("blobs/binary.sslo").await.unwrap();

    assert_eq!(fetched, payload);
}

#[tokio::test]
async fn head_distinguishes_missing_from_present() {
    let client = client_or_skip!();
    assert_eq!(client.head("ops/never-written").await.unwrap(), None);

    client
        .put("ops/written", b"1234567890".to_vec())
        .await
        .unwrap();
    assert_eq!(client.head("ops/written").await.unwrap(), Some(10));
}

#[tokio::test]
async fn listing_returns_keys_relative_to_the_prefix() {
    let client = client_or_skip!();
    client.put("ops/a", b"x".to_vec()).await.unwrap();
    client.put("ops/b", b"y".to_vec()).await.unwrap();
    client.put("blobs/c", b"z".to_vec()).await.unwrap();

    let mut ops: Vec<String> = client
        .list("ops/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.key)
        .collect();
    ops.sort();

    // The configured prefix is stripped: callers work in vault-relative
    // keys and never have to know where in the bucket they live.
    assert_eq!(ops, vec!["ops/a".to_string(), "ops/b".to_string()]);
}

#[tokio::test]
async fn listing_pages_past_the_thousand_key_limit() {
    let client = client_or_skip!();
    // One page is 1000 keys. A truncated listing would read as "these
    // operations don't exist", which is the worst possible way to be wrong.
    for i in 0..1050 {
        client
            .put(&format!("many/{i:05}"), b"x".to_vec())
            .await
            .unwrap();
    }
    assert_eq!(client.list("many/").await.unwrap().len(), 1050);
}

#[tokio::test]
async fn delete_removes_the_object() {
    let client = client_or_skip!();
    client.put("tmp/gone", b"x".to_vec()).await.unwrap();
    client.delete("tmp/gone").await.unwrap();
    assert_eq!(client.head("tmp/gone").await.unwrap(), None);
}

#[tokio::test]
async fn wrong_credentials_report_something_actionable() {
    let Some(mut cfg) = config() else {
        eprintln!("skipped: SILENTSILO_TEST_S3_ENDPOINT is not set");
        return;
    };
    cfg.secret_access_key = "definitely-not-the-secret".into();
    let client = S3Client::new(cfg).unwrap();

    let err = client
        .put("x", b"y".to_vec())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("403") || err.to_lowercase().contains("signature"),
        "a rejected signature should say so, got: {err}"
    );
}
