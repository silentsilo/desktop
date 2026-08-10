//! Publishing and revoking the wrapped DEKs, plus what a pass refuses to read.
//!
//! Against a folder store rather than a bucket, so these run everywhere.
//! `joining.rs` covers the same ground over real S3 and is skipped unless an
//! endpoint is configured, which is precisely why the revocation bug below
//! survived: nothing exercised it on an ordinary `cargo test`.

use silentsilo_crypto::generate_dek;
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{fetch_key_envelopes, fetch_ops_from, publish_key_envelopes};
use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};

fn store(dir: &tempfile::TempDir) -> FolderStore {
    FolderStore::new(dir.path().to_path_buf())
}

fn credential(id: &str, slot: u8) -> StoredFidoCredential {
    StoredFidoCredential {
        kind: silentsilo_vault::KIND_FIDO2.to_string(),
        derivation: silentsilo_vault::DERIVATION_HMAC_V1.to_string(),
        credential_id: id.into(),
        public_key: "cafe".into(),
        key_slot: slot,
        rp_id: "silentsilo.com".into(),
        label: format!("Key {slot}"),
        wrapped_dek: "deadbeef".into(),
        platform: false,
        revoked: false,
    }
}

async fn published_ids(client: &dyn ObjectStore) -> Vec<String> {
    let mut ids: Vec<String> = fetch_key_envelopes(client)
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.credential_id)
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn enrolled_keys_are_published() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);
    let keys = StoredFidoKeys {
        keys: vec![credential("aa11", 0), credential("bb22", 1)],
    };

    let report = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    assert_eq!(report.published, 2);
    assert!(report.revoked.is_empty());
    assert_eq!(published_ids(&client).await, vec!["aa11", "bb22"]);
}

#[tokio::test]
async fn a_revoked_key_stops_being_published_and_its_envelope_goes() {
    // The demonstrated hole: removing a key deleted the local row, made one
    // attempt at storage, and reported success whatever happened. A pass that
    // only wrote had no way to notice a credential it no longer knew about,
    // so the published envelope kept opening the vault from any machine.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let mut keys = StoredFidoKeys {
        keys: vec![credential("aa11", 0), credential("bb22", 1)],
    };
    publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();
    assert_eq!(published_ids(&client).await, vec!["aa11", "bb22"]);

    keys.keys[1].revoked = true;
    let report = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    // Nothing written: the surviving envelope is already up there unchanged,
    // and rewriting it every pass is what this stopped doing. What the bucket
    // holds afterwards is the assertion that matters.
    assert_eq!(
        report.published, 0,
        "an unchanged envelope was written again"
    );
    assert_eq!(report.revoked, vec!["bb22"]);
    assert_eq!(published_ids(&client).await, vec!["aa11"]);
}

#[tokio::test]
async fn a_revocation_made_offline_is_retried_on_the_next_pass() {
    // The tombstone's whole reason: the first attempt happened with no
    // storage configured, so nothing was deleted and nothing recorded it.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let mut keys = StoredFidoKeys {
        keys: vec![credential("aa11", 0), credential("bb22", 1)],
    };
    publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    // Removed while offline: the row is marked, storage is untouched.
    keys.keys[1].revoked = true;
    assert_eq!(published_ids(&client).await, vec!["aa11", "bb22"]);

    // The next pass that does reach storage finishes the job.
    let report = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();
    assert_eq!(report.revoked, vec!["bb22"]);
    assert_eq!(published_ids(&client).await, vec!["aa11"]);
}

#[tokio::test]
async fn publishing_does_not_touch_envelopes_this_device_never_heard_of() {
    // Deleting everything absent from the local list would be a reconcile,
    // and it would revoke a key another device enrolled while this one was
    // away. Key envelopes are what let a device join at all, so that mistake
    // locks people out rather than merely losing state.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![credential("cc33", 7)],
        },
        true,
    )
    .await
    .unwrap();

    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![credential("aa11", 0)],
        },
        true,
    )
    .await
    .unwrap();

    assert_eq!(published_ids(&client).await, vec!["aa11", "cc33"]);
}

#[tokio::test]
async fn a_key_put_back_after_an_offline_removal_keeps_its_envelope() {
    // Enrolment clears the tombstone, so this is belt and braces: were both
    // rows to survive, the delete would undo the publish in the same pass.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let mut revoked = credential("aa11", 0);
    revoked.revoked = true;
    let keys = StoredFidoKeys {
        keys: vec![revoked, credential("aa11", 1)],
    };

    let report = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    assert_eq!(report.published, 1);
    assert!(report.revoked.is_empty());
    assert_eq!(published_ids(&client).await, vec!["aa11"]);
}

#[tokio::test]
async fn an_operation_object_sized_to_exhaust_memory_is_refused() {
    // Storage is not trusted. Records name things and carry a password entry
    // at most, so an object this size is a hostile or broken provider, and
    // reading it whole is how the app runs out of memory.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    client
        .put(
            "ops/00000000000000000001-aa-bb.op",
            vec![0u8; 2 * 1024 * 1024],
        )
        .await
        .unwrap();

    let err = fetch_ops_from(&client as &dyn ObjectStore, &generate_dek(), 0)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("refusing to read it"), "got: {err}");
}

#[tokio::test]
async fn an_operation_of_ordinary_size_is_still_read() {
    // The ceiling must not be so eager that it refuses a real record. This
    // one is not decryptable with a fresh DEK, so reaching the unseal error
    // is what proves the size check let it through.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    client
        .put("ops/00000000000000000001-aa-bb.op", vec![0u8; 4096])
        .await
        .unwrap();

    let err = fetch_ops_from(&client as &dyn ObjectStore, &generate_dek(), 0)
        .await
        .unwrap_err()
        .to_string();

    assert!(!err.contains("refusing to read it"), "got: {err}");
}

#[tokio::test]
async fn a_credential_with_no_wrapped_dek_is_not_published() {
    // It cannot unlock anything on its own, so publishing it would only
    // mislead a joining device into thinking it had a usable key.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let mut useless = credential("aa11", 0);
    useless.wrapped_dek = String::new();

    let report = publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![useless],
        },
        true,
    )
    .await
    .unwrap();

    assert_eq!(report.published, 0);
    assert!(published_ids(&client).await.is_empty());
}

#[tokio::test]
async fn a_rewrapped_key_is_republished_even_at_the_same_length() {
    // The envelope is skipped when the bucket already holds it, which is what
    // stops a new version being written every couple of minutes. Deciding
    // that by size would be wrong in the one case that matters: a wrapped DEK
    // is the hex of a fixed-length key, so re-wrapping produces different
    // bytes of identical length. Skipping there leaves a device unable to
    // unlock with the rotated key, and nothing on screen would say why.
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let original = credential("aa11", 0);
    publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![original.clone()],
        },
        true,
    )
    .await
    .unwrap();

    let mut rewrapped = original.clone();
    rewrapped.wrapped_dek = "feedface".into();
    assert_eq!(
        rewrapped.wrapped_dek.len(),
        original.wrapped_dek.len(),
        "this test only means something while the two are the same length"
    );

    let report = publish_key_envelopes(
        &client as &dyn ObjectStore,
        &StoredFidoKeys {
            keys: vec![rewrapped],
        },
        true,
    )
    .await
    .unwrap();

    assert_eq!(report.published, 1, "the re-wrapped envelope was skipped");
    let held = fetch_key_envelopes(&client as &dyn ObjectStore)
        .await
        .unwrap();
    assert_eq!(held[0].wrapped_dek, "feedface");
}

#[tokio::test]
async fn an_unchanged_envelope_is_left_where_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);
    let keys = StoredFidoKeys {
        keys: vec![credential("aa11", 0)],
    };

    publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();
    let second = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();

    assert_eq!(second.published, 0);
}

/// A key of a kind this build cannot use still belongs in the bucket.
///
/// Publishing is bookkeeping about the silo, not about this machine: the Mac
/// that enrolled a Touch ID key has to be able to put its envelope up, and a
/// Windows client syncing afterwards must not treat "I cannot use this" as
/// "this should not be here". Retiring another platform's key behind its back
/// is how a two-device household ends up with one device locked out.
#[tokio::test]
async fn a_key_of_another_kind_is_published_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let client = store(&dir);

    let mut foreign = credential("touch-id-key-1", 1);
    foreign.kind = "secure-enclave".into();
    foreign.platform = true;
    let keys = StoredFidoKeys {
        keys: vec![credential("aa11", 0), foreign],
    };

    let report = publish_key_envelopes(&client as &dyn ObjectStore, &keys, true)
        .await
        .unwrap();
    assert_eq!(report.published, 2, "both envelopes belong in the bucket");

    let held = fetch_key_envelopes(&client as &dyn ObjectStore)
        .await
        .unwrap();
    assert_eq!(held.len(), 2, "reading back must not drop the unknown kind");

    // The half that matters on the reading side: the silo has two keys, and
    // exactly one of them can open it here.
    let held = StoredFidoKeys { keys: held };
    assert_eq!(held.usable().count(), 1);
    assert_eq!(
        held.credential_ids_bytes().expect("no error"),
        vec![vec![0xaa, 0x11]],
        "a credential id that is not hex must not fail the whole join"
    );
}
