//! The environment a developer machine never has and a real one always does:
//! something holding each file open as it is replaced. That once turned every
//! durable write into "Access is denied".

use silentsilo_crypto::{generate_content_kek, generate_dek};
use silentsilo_testkit::{BlockedPath, HeldOpen};
use silentsilo_vault::{
    LocalVaultAuth, SiloEntry, SiloRegistry, StoredFidoCredential, StoredFidoKeys, VaultSession,
    create_recovery_envelope, load_credentials, load_fido_keys, load_recovery_envelope,
    load_registry, save_credentials, save_fido_keys, save_recovery_envelope, save_registry,
};
use std::path::PathBuf;
use uuid::Uuid;

fn silo() -> (tempfile::TempDir, VaultSession) {
    let dir = tempfile::tempdir().unwrap();
    let vault_id = Uuid::new_v4();
    let session =
        VaultSession::provision(dir.path().to_path_buf(), vault_id, "hostile-secret").unwrap();
    // `provision` writes no schema, but unlock reads the vault id from here.
    session
        .conn
        .execute_batch(&format!(
            "CREATE TABLE vault_meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO vault_meta VALUES ('vault_id', '{vault_id}');"
        ))
        .unwrap();
    (dir, session)
}

fn credential(id: &str, wrapped: &str) -> StoredFidoCredential {
    StoredFidoCredential {
        kind: "fido2".into(),
        derivation: "hmac-secret-v1".into(),
        policy: String::new(),
        credential_id: id.into(),
        public_key: "3059".into(),
        key_slot: 0,
        rp_id: "silentsilo.com".into(),
        label: id.into(),
        wrapped_dek: wrapped.into(),
        platform: false,
        revoked: false,
    }
}

#[test]
fn the_encrypted_index_saves_while_a_scanner_holds_the_old_one() {
    // The file unlock reads. Losing this write leaves the session's work in
    // the plaintext copy alone, which the next lock deletes.
    let (_dir, session) = silo();
    session
        .conn
        .execute_batch("CREATE TABLE marker (v TEXT); INSERT INTO marker VALUES ('kept');")
        .unwrap();

    let scanner = HeldOpen::reading(&session.paths.db_enc_path());
    session
        .backup_locally()
        .expect("a held snapshot must not fail the save");
    drop(scanner);

    let paths = session.paths.clone();
    drop(session);
    silentsilo_vault::wipe_plaintext_working_copy(&paths);

    let reopened =
        VaultSession::open_with_device_secret(paths.root.clone(), "hostile-secret").unwrap();
    let kept: String = reopened
        .conn
        .query_row("SELECT v FROM marker", [], |row| row.get(0))
        .expect("the write landed and survived the lock");
    assert_eq!(kept, "kept");
}

#[test]
fn the_enrolled_keys_save_while_something_holds_the_file() {
    // On a silo whose only way in is a security key, a lost write here is
    // the difference between opening the vault and never opening it again.
    let (dir, _session) = silo();
    let mut keys = StoredFidoKeys { keys: Vec::new() };
    save_fido_keys(dir.path(), &keys, silentsilo_vault::Authority::Machine).unwrap();

    let scanner = HeldOpen::reading(&dir.path().join("keys").join("fido.json"));
    keys.keys.push(credential("aa11", "envelope-a"));
    save_fido_keys(dir.path(), &keys, silentsilo_vault::Authority::Machine)
        .expect("a held key file must not fail the save");
    drop(scanner);

    let back = load_fido_keys(dir.path()).unwrap();
    assert_eq!(back.keys.len(), 1);
    assert_eq!(back.keys[0].wrapped_dek, "envelope-a");
}

#[test]
fn the_recovery_envelope_saves_while_something_holds_the_file() {
    let (dir, _session) = silo();
    let (_, first) = create_recovery_envelope(&generate_dek()).unwrap();
    save_recovery_envelope(dir.path(), &first).unwrap();

    let scanner = HeldOpen::reading(&dir.path().join("keys").join("recovery.json"));
    let (code, replacement) = create_recovery_envelope(&generate_dek()).unwrap();
    save_recovery_envelope(dir.path(), &replacement)
        .expect("a held envelope must not fail the save");
    drop(scanner);

    // The code just shown to the user has to open what is on disk, or the
    // app has made a promise it cannot keep.
    let stored = load_recovery_envelope(dir.path()).unwrap();
    assert!(silentsilo_vault::unwrap_with_code(&stored, &code).is_ok());
}

#[test]
fn the_silo_registry_saves_while_something_holds_it() {
    // Creating a silo wrote everything into the folder and then could not
    // add the row that makes it findable.
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path().to_path_buf();
    save_registry(&app_data, &SiloRegistry::default()).unwrap();

    let scanner = HeldOpen::reading(&app_data.join("silos.json"));
    let mut registry = SiloRegistry::default();
    registry.upsert(SiloEntry {
        id: Uuid::new_v4(),
        name: "Personal".into(),
        path: PathBuf::from("D:/silos/Personal"),
        last_opened: 0,
        auto_lock_minutes: None,
    });
    save_registry(&app_data, &registry).expect("a held registry must not fail the save");
    drop(scanner);

    assert_eq!(load_registry(&app_data).silos.len(), 1);
}

#[test]
fn the_content_key_envelope_saves_while_something_holds_it() {
    // Without this file nothing in the silo opens, on any device.
    let (dir, session) = silo();
    let scanner = HeldOpen::reading(&dir.path().join("content.kek.enc"));

    silentsilo_vault::save_kek(dir.path(), &generate_content_kek(), &session.dek)
        .expect("a held KEK envelope must not fail the save");
    drop(scanner);

    assert!(silentsilo_vault::load_kek(dir.path(), &session.dek).is_ok());
}

#[test]
fn credentials_survive_being_written_twice() {
    // The keyring is tried first; where it will not hold them, the file is
    // the only copy of the secret that opens this device's silo.
    let silo_id = Uuid::new_v4();
    let creds = LocalVaultAuth {
        vault_id: silo_id,
        device_secret: "a-device-secret".into(),
    };
    save_credentials(&creds).unwrap();
    save_credentials(&creds).expect("a second save must not fail");

    let back = load_credentials(silo_id).unwrap();
    assert_eq!(back.device_secret, "a-device-secret");
    silentsilo_vault::clear_credentials(silo_id);
}

#[test]
fn a_write_whose_temp_file_is_refused_still_lands() {
    // Protection software withholds "create a file here" separately from
    // "change a file already there". The write has to take the second route
    // rather than reporting a failure the user can do nothing about.
    let (dir, _session) = silo();
    let mut keys = StoredFidoKeys { keys: Vec::new() };
    save_fido_keys(dir.path(), &keys, silentsilo_vault::Authority::Machine).unwrap();

    let blocked = BlockedPath::at(&dir.path().join("keys").join("fido.json.tmp"));
    keys.keys.push(credential("bb22", "envelope-b"));
    save_fido_keys(dir.path(), &keys, silentsilo_vault::Authority::Machine)
        .expect("a refused temp file must not fail the save");
    drop(blocked);

    assert_eq!(load_fido_keys(dir.path()).unwrap().keys.len(), 1);
}

#[test]
fn a_silo_provisions_while_its_folder_is_being_watched() {
    // Creating a silo writes a salt, two key envelopes, a marker and a
    // snapshot in one go, with a scanner reading each file as it appears.
    // This is the sequence that failed.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Watched");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("vault.salt"), b"placeholder").unwrap();
    let scanner = HeldOpen::reading(&root.join("vault.salt"));

    let session = VaultSession::provision(root.clone(), Uuid::new_v4(), "watched-secret")
        .expect("provisioning must survive a watched folder");
    drop(scanner);

    assert!(session.paths.db_enc_path().is_file());
    assert!(root.join("master.dek.enc").is_file());
    assert!(root.join("content.kek.enc").is_file());
}
