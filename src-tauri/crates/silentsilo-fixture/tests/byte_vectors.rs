//! The exact bytes of every persisted format, and what they decode to.
//! The whole-silo fixtures answer "did anything change"; these answer
//! "what changed", which is the difference between a morning and a minute.
//!
//! Every byte here was produced by a build that shipped the format. Fixing
//! a failure means fixing the code: if a vector no longer decodes, a silo
//! written before this change no longer opens.

use silentsilo_crypto::{ContentKey, decrypt_blob, unseal_with_key};
use silentsilo_sync::VaultManifest;
use silentsilo_vault::RecoveryEnvelope;
use silentsilo_vfs::{OpBody, OpRecord, VaultOp};
use uuid::Uuid;

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// `seal_with_key(b"the payload of an operation record", &[7u8; 32])`.
const SEALED: &str = "5353454101fb52a670ab2e97cc42b61f4cc496621dd88ce30c69ef878bc58a97f24e12592691135f2dba52b04687f5d93ea8adb833e985ec19b414307e2ed8cab94de6";

/// A whole `.sslo` file: `encrypt_file` over 37 bytes, content key `[9u8; 32]`.
const BLOB: &str = "53534c4f000100400000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff0000db03c46dabd60afaba7d693628cbe7fab5cec5b73e74935a7457d8f118d0a97e30d291c5051233693ff0485889608a5ab18e7a5d4294137f730385ebfd05cedaac0d3010893f2a66dfff72cb7e0546d1d12f82cdd3257950df76e3a5b3";

#[test]
fn the_sealed_envelope_still_decodes() {
    let key = [7u8; 32];
    let plain = unseal_with_key(&from_hex(SEALED), &key)
        .expect("a sealed payload this build wrote can no longer be opened");
    assert_eq!(plain, b"the payload of an operation record");

    // The first bytes are the discriminator, and they are what a future
    // change has to move past rather than through.
    let bytes = from_hex(SEALED);
    assert_eq!(&bytes[0..4], b"SSEA", "the magic moved");
    assert_eq!(bytes[4], 1, "the seal version moved");
}

#[test]
fn the_blob_header_still_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("v.sslo");
    let out = dir.path().join("out.bin");
    std::fs::write(&source, from_hex(BLOB)).unwrap();

    // A content key, not the vault DEK: content lives on a key of its own so
    // that rotating the vault key never has to rewrite the content.
    let key = ContentKey::from_bytes([9u8; 32]);
    let blob_id = Uuid::from_u128(0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000);
    let header = decrypt_blob(&source, &out, &key, blob_id)
        .expect("a blob this build wrote can no longer be opened");

    assert_eq!(header.version, 1, "the blob version moved");
    assert_eq!(header.chunk_size, 4 * 1024 * 1024, "the chunk size moved");
    assert_eq!(
        header.file_id,
        Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888),
        "the file id field moved"
    );
    assert_eq!(header.blob_id, blob_id, "the blob id field moved");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"blob contents, short enough to commit"
    );
}

/// An operation record as written to the bucket, one field per line so a
/// failure names the field.
const OP_RECORD: &str = r#"{"op_id":"0f9a4d3e-2c1b-4a5d-8e7f-1234567890ab","lamport":7,"device_id":"1a2b3c4d-5e6f-4788-9900-aabbccddeeff","at":1700000000,"skippable":false,"seq":4,"prev":"9f2c1a","op":"create_folder","id":"22222222-3333-4444-5555-666666666666","parent_id":"77777777-8888-4999-aaaa-bbbbbbbbbbbb","name":"Documents"}"#;

#[test]
fn the_operation_record_still_decodes() {
    let record = OpRecord::from_bytes(OP_RECORD.as_bytes())
        .expect("an operation record this build wrote can no longer be read");

    assert_eq!(
        record.op_id,
        Uuid::parse_str("0f9a4d3e-2c1b-4a5d-8e7f-1234567890ab").unwrap(),
        "op_id moved"
    );
    assert_eq!(record.lamport, 7, "lamport moved");
    assert_eq!(record.at, 1_700_000_000, "at moved");
    assert!(!record.skippable, "skippable moved");
    assert_eq!(record.seq, 4, "seq moved");
    assert_eq!(record.prev.as_deref(), Some("9f2c1a"), "prev moved");

    match record.op {
        OpBody::Known(VaultOp::CreateFolder { name, .. }) => {
            assert_eq!(name, "Documents", "the operation body moved")
        }
        other => panic!("the operation no longer decodes as CreateFolder: {other:?}"),
    }
}

#[test]
fn a_record_written_now_still_looks_like_that() {
    // The other direction. Decoding an old record is half the promise; the
    // other half is that today's writer produces something an old reader
    // would have recognised.
    let record = OpRecord::from_bytes(OP_RECORD.as_bytes()).unwrap();
    let written = String::from_utf8(record.to_bytes().unwrap()).unwrap();

    for field in [
        "\"op_id\":",
        "\"lamport\":",
        "\"device_id\":",
        "\"at\":",
        "\"skippable\":",
        "\"seq\":",
        "\"prev\":",
        "\"op\":",
    ] {
        assert!(
            written.contains(field),
            "{field} is gone from what we write"
        );
    }
}

/// The password entry as it travels: base64 of a sealed payload, carried
/// whole inside the operation.
const UPSERT_PASSWORD: &str = r#"{"op_id":"aaaaaaaa-1111-4222-8333-444444444444","lamport":11,"device_id":"1a2b3c4d-5e6f-4788-9900-aabbccddeeff","at":1700000000,"skippable":false,"seq":0,"op":"upsert_password","id":"55555555-6666-4777-8888-999999999999","data":"U1NFQQFhYmNkZWZnaGlqa2xtbm9wcXJzdHV2"}"#;

#[test]
fn the_password_entry_still_travels_whole() {
    let record = OpRecord::from_bytes(UPSERT_PASSWORD.as_bytes())
        .expect("a password operation this build wrote can no longer be read");

    match record.op {
        OpBody::Known(VaultOp::UpsertPassword { id, data }) => {
            assert_eq!(
                id,
                Uuid::parse_str("55555555-6666-4777-8888-999999999999").unwrap(),
                "the entry id moved"
            );
            // Opaque on purpose: the operation carries the sealed entry
            // without looking inside, which is what lets a build that does
            // not know a field still hand it on.
            assert_eq!(
                data, "U1NFQQFhYmNkZWZnaGlqa2xtbm9wcXJzdHV2",
                "the sealed entry is no longer carried verbatim"
            );
        }
        other => panic!("no longer decodes as UpsertPassword: {other:?}"),
    }
}

const RECOVERY_ENVELOPE: &str = r#"{"version":1,"kdf":{"algorithm":"argon2id","m_cost":65536,"t_cost":3,"p_cost":1},"salt":"0102030405060708090a0b0c0d0e0f10","wrapped_dek":"deadbeef","created_at":1700000000}"#;

#[test]
fn the_recovery_envelope_still_decodes() {
    let envelope: RecoveryEnvelope = serde_json::from_str(RECOVERY_ENVELOPE)
        .expect("a recovery envelope this build wrote can no longer be read");

    assert_eq!(envelope.version, 1, "the envelope version moved");
    assert_eq!(envelope.kdf.algorithm, "argon2id", "the algorithm moved");
    assert_eq!(envelope.kdf.m_cost, 65_536, "m_cost moved");
    assert_eq!(envelope.kdf.t_cost, 3, "t_cost moved");
    assert_eq!(envelope.kdf.p_cost, 1, "p_cost moved");
    assert_eq!(
        envelope.salt, "0102030405060708090a0b0c0d0e0f10",
        "the salt moved"
    );
    assert_eq!(envelope.wrapped_dek, "deadbeef", "the wrapped dek moved");
}

/// A key envelope as published to `keys/<credential_id>.env`: plain JSON of
/// one enrolled key. It has no file version on purpose; `kind` is the whole
/// discriminator, and these vectors are what hold that story together.
const KEY_ENVELOPE: &str = r#"{"kind":"fido2","credential_id":"aa11","public_key":"3059","key_slot":0,"rp_id":"silentsilo.com","label":"Primary","wrapped_dek":"deadbeef","platform":false,"revoked":false}"#;

/// The same, written before the `kind` field existed. Every envelope already
/// in a bucket looks like this, and it must read as a FIDO2 key forever.
const KEY_ENVELOPE_PRE_KIND: &str = r#"{"credential_id":"bb22","public_key":"3059","key_slot":1,"rp_id":"silentsilo.com","label":"Backup","wrapped_dek":"cafef00d","platform":false,"revoked":false}"#;

/// A kind this build has never heard of, with a credential id that is not
/// hex. What a macOS build would publish; a Windows client must carry it
/// without using it and without failing over it.
const KEY_ENVELOPE_FOREIGN: &str = r#"{"kind":"secure-enclave","credential_id":"touch-id-key-1","public_key":"","key_slot":2,"rp_id":"silentsilo.com","label":"MacBook Touch ID","wrapped_dek":"beefcafe","platform":true,"revoked":false}"#;

#[test]
fn the_key_envelope_still_decodes() {
    use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};

    let current: StoredFidoCredential = serde_json::from_str(KEY_ENVELOPE)
        .expect("a key envelope this build wrote can no longer be read");
    assert_eq!(current.kind, silentsilo_vault::KIND_FIDO2, "the kind moved");
    assert_eq!(current.credential_id, "aa11", "the credential id moved");
    assert_eq!(current.wrapped_dek, "deadbeef", "the wrapped dek moved");

    let legacy: StoredFidoCredential = serde_json::from_str(KEY_ENVELOPE_PRE_KIND)
        .expect("an envelope from before the kind field can no longer be read");
    assert_eq!(
        legacy.kind,
        silentsilo_vault::KIND_FIDO2,
        "an envelope without a kind must read as a FIDO2 key"
    );

    let foreign: StoredFidoCredential =
        serde_json::from_str(KEY_ENVELOPE_FOREIGN).expect("an unknown kind must parse, not fail");

    // The composed behaviour the field exists for: three keys on the silo,
    // two this build can answer for, and the third neither offered to the
    // authenticator nor able to break the list.
    let keys = StoredFidoKeys {
        keys: vec![current, legacy, foreign],
    };
    assert_eq!(keys.active().count(), 3, "all three are enrolled");
    assert_eq!(
        keys.credential_ids_bytes()
            .expect("a foreign id must not fail the allow-list"),
        vec![vec![0xaa, 0x11], vec![0xbb, 0x22]],
        "the allow-list moved"
    );
}

/// `keys/fido.json` as written into the silo folder: the same credentials,
/// wrapped in the versioned silo-file envelope.
const FIDO_JSON: &str = r#"{"version":1,"data":{"keys":[{"kind":"fido2","credential_id":"aa11","public_key":"3059","key_slot":0,"rp_id":"silentsilo.com","label":"Primary","wrapped_dek":"deadbeef","platform":false,"revoked":false},{"kind":"secure-enclave","credential_id":"touch-id-key-1","public_key":"","key_slot":1,"rp_id":"silentsilo.com","label":"MacBook Touch ID","wrapped_dek":"beefcafe","platform":true,"revoked":false}]}}"#;

#[test]
fn the_enrolled_keys_file_still_loads() {
    // Through the real loader, not a bare serde call: the version envelope
    // and the refusal of newer ones are part of the format.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("keys")).unwrap();
    std::fs::write(root.path().join("keys/fido.json"), FIDO_JSON).unwrap();

    let keys = silentsilo_vault::load_fido_keys(root.path())
        .expect("a keys file this build wrote can no longer be loaded");
    assert_eq!(keys.keys.len(), 2, "a key was dropped on the way through");
    assert_eq!(keys.keys[1].kind, "secure-enclave", "the kind moved");
    assert_eq!(
        keys.usable().count(),
        1,
        "exactly one of these keys can unlock here"
    );
}

const MANIFEST: &str = r#"{"vault_id":"12345678-9abc-4def-8123-456789abcdef","version":1}"#;

#[test]
fn the_manifest_still_decodes() {
    let manifest: VaultManifest =
        serde_json::from_str(MANIFEST).expect("a manifest this build wrote can no longer be read");

    assert_eq!(
        manifest.vault_id,
        Uuid::parse_str("12345678-9abc-4def-8123-456789abcdef").unwrap(),
        "the vault id moved"
    );
    assert_eq!(manifest.version, 1, "the manifest version moved");
}
