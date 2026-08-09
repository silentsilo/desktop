//! What a second device needs to open the vault at all.
//!
//! Syncing the tree and the blobs is not enough: without the wrapped DEK for
//! a security key, a joining device can fetch everything and still not
//! decrypt any of it. Skipped unless `SILENTSILO_TEST_S3_ENDPOINT` is set.

use silentsilo_core::S3Config;
use silentsilo_s3::S3Client;
use silentsilo_store::ObjectStore;
use silentsilo_sync::{
    fetch_key_envelopes, fetch_recovery_envelope, publish_key_envelopes, push_recovery_envelope,
    put_manifest, read_manifest, revoke_key_envelope, revoke_recovery_envelope,
};
use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};
use uuid::Uuid;

fn client() -> Option<S3Client> {
    let endpoint = std::env::var("SILENTSILO_TEST_S3_ENDPOINT").ok()?;
    S3Client::new(S3Config {
        endpoint,
        region: "us-east-1".into(),
        bucket: std::env::var("SILENTSILO_TEST_S3_BUCKET").unwrap_or_else(|_| "vault-test".into()),
        prefix: format!("join-{}", Uuid::new_v4()),
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

fn credential(id: &str, slot: u8, label: &str) -> StoredFidoCredential {
    StoredFidoCredential {
        kind: silentsilo_vault::KIND_FIDO2.to_string(),
        credential_id: id.into(),
        public_key: "cafe".into(),
        key_slot: slot,
        rp_id: "silentsilo.com".into(),
        label: label.into(),
        wrapped_dek: "deadbeef".into(),
        platform: false,
        revoked: false,
    }
}

#[tokio::test]
async fn an_empty_prefix_has_no_manifest() {
    // The normal state when someone points the app at a fresh bucket —
    // not an error to report.
    let client = client_or_skip!();
    assert!(
        read_manifest(&client as &dyn ObjectStore)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_joining_device_learns_the_vault_id_from_the_manifest() {
    let client = client_or_skip!();
    let vault_id = Uuid::new_v4();
    put_manifest(&client as &dyn ObjectStore, vault_id)
        .await
        .unwrap();

    let manifest = read_manifest(&client as &dyn ObjectStore)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manifest.vault_id, vault_id);
}

#[tokio::test]
async fn a_newer_manifest_format_is_refused_rather_than_misread() {
    let client = client_or_skip!();
    // Hand-written to look like a future version, since nothing here can
    // produce one yet.
    let future = serde_json::json!({ "vault_id": Uuid::new_v4(), "version": 999 });
    client
        .put("vault.json", serde_json::to_vec(&future).unwrap())
        .await
        .unwrap();

    let err = read_manifest(&client as &dyn ObjectStore)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("newer version"), "got: {err}");
}

#[tokio::test]
async fn enrolled_keys_reach_a_joining_device() {
    let client = client_or_skip!();
    let keys = StoredFidoKeys {
        keys: vec![
            credential("aa11", 0, "Primary"),
            credential("bb22", 1, "Backup"),
        ],
    };

    assert_eq!(
        publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
            .await
            .unwrap()
            .published,
        2
    );

    let mut fetched = fetch_key_envelopes(&client as &dyn ObjectStore)
        .await
        .unwrap();
    fetched.sort_by(|a, b| a.credential_id.cmp(&b.credential_id));
    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].credential_id, "aa11");
    assert_eq!(
        fetched[0].wrapped_dek, "deadbeef",
        "the wrapped DEK is the whole point — without it a joining device cannot unlock"
    );
    assert_eq!(fetched[1].label, "Backup");
}

#[tokio::test]
async fn a_credential_with_no_wrapped_dek_is_not_published() {
    // It cannot unlock anything, so publishing it would only mislead a
    // joining device into thinking that key works.
    let client = client_or_skip!();
    let mut useless = credential("cc33", 2, "Incomplete");
    useless.wrapped_dek = String::new();
    let keys = StoredFidoKeys {
        keys: vec![useless],
    };

    assert_eq!(
        publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
            .await
            .unwrap()
            .published,
        0
    );
    assert!(
        fetch_key_envelopes(&client as &dyn ObjectStore)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn revoking_removes_the_envelope_everywhere() {
    // Deleting only the local record would leave the bucket copy usable by
    // anything still holding the physical key.
    let client = client_or_skip!();
    let keys = StoredFidoKeys {
        keys: vec![
            credential("aa11", 0, "Primary"),
            credential("bb22", 1, "Backup"),
        ],
    };
    publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    revoke_key_envelope(&client as &dyn ObjectStore, "bb22")
        .await
        .unwrap();

    let remaining = fetch_key_envelopes(&client as &dyn ObjectStore)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].credential_id, "aa11");
}

#[tokio::test]
async fn re_pushing_an_envelope_replaces_it() {
    // Re-wrapping the DEK (after a key rotation, say) has to overwrite
    // rather than accumulate stale envelopes for the same credential.
    let client = client_or_skip!();
    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![credential("aa11", 0, "Old name")],
        },
        true,
    )
    .await
    .unwrap();

    let mut updated = credential("aa11", 0, "New name");
    updated.wrapped_dek = "feedface".into();
    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![updated],
        },
        true,
    )
    .await
    .unwrap();

    let fetched = fetch_key_envelopes(&client as &dyn ObjectStore)
        .await
        .unwrap();
    assert_eq!(fetched.len(), 1, "one credential, one envelope");
    assert_eq!(fetched[0].label, "New name");
    assert_eq!(fetched[0].wrapped_dek, "feedface");
}

#[tokio::test]
async fn a_recovery_envelope_reaches_a_machine_that_has_never_seen_the_vault() {
    // The situation the code exists for: no key, no local vault, nothing
    // but the bucket and a piece of paper.
    let client = client_or_skip!();
    let dek = silentsilo_crypto::generate_dek();
    let (code, envelope) = silentsilo_vault::create_recovery_envelope(&dek).unwrap();

    push_recovery_envelope(&client as &dyn ObjectStore, &envelope)
        .await
        .unwrap();

    let fetched = fetch_recovery_envelope(&client as &dyn ObjectStore)
        .await
        .unwrap()
        .unwrap();
    let recovered = silentsilo_vault::unwrap_with_code(&fetched, &code).unwrap();
    assert_eq!(
        recovered.as_bytes(),
        dek.as_bytes(),
        "the code has to yield the same key, or nothing in the bucket decrypts"
    );
}

#[tokio::test]
async fn a_bucket_with_no_recovery_code_says_so_rather_than_failing() {
    let client = client_or_skip!();
    assert!(
        fetch_recovery_envelope(&client as &dyn ObjectStore)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn revoking_recovery_stops_the_written_down_code_everywhere() {
    // Clearing only the local copy would leave the paper copy working from
    // any other machine — the opposite of what turning it off means.
    let client = client_or_skip!();
    let (_, envelope) =
        silentsilo_vault::create_recovery_envelope(&silentsilo_crypto::generate_dek()).unwrap();
    push_recovery_envelope(&client as &dyn ObjectStore, &envelope)
        .await
        .unwrap();

    revoke_recovery_envelope(&client as &dyn ObjectStore)
        .await
        .unwrap();
    assert!(
        fetch_recovery_envelope(&client as &dyn ObjectStore)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn replacing_a_recovery_code_invalidates_the_old_one() {
    let client = client_or_skip!();
    let dek = silentsilo_crypto::generate_dek();
    let (old_code, old) = silentsilo_vault::create_recovery_envelope(&dek).unwrap();
    push_recovery_envelope(&client as &dyn ObjectStore, &old)
        .await
        .unwrap();

    let (new_code, new) = silentsilo_vault::create_recovery_envelope(&dek).unwrap();
    push_recovery_envelope(&client as &dyn ObjectStore, &new)
        .await
        .unwrap();

    let published = fetch_recovery_envelope(&client as &dyn ObjectStore)
        .await
        .unwrap()
        .unwrap();
    assert!(
        silentsilo_vault::unwrap_with_code(&published, &old_code).is_err(),
        "the replaced code must stop working"
    );
    assert_eq!(
        silentsilo_vault::unwrap_with_code(&published, &new_code)
            .unwrap()
            .as_bytes(),
        dek.as_bytes()
    );
}
