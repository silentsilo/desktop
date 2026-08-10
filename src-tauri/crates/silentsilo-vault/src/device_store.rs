use std::path::PathBuf;

use keyring::Entry;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dpapi;
use crate::error::VaultError;

const KEYRING_SERVICE: &str = "com.silentsilo.desktop";
const KEYRING_USER: &str = "vault-credentials";
/// Prefix marking the fallback file's contents as DPAPI-protected rather
/// than plain JSON. Anything not starting with this is treated as a
/// pre-existing plaintext file (or a plaintext write on a non-Windows
/// platform, where DPAPI isn't available and this stays the same as before).
const DPAPI_MAGIC: &[u8] = b"SSDPAPI1";

/// Identifies this machine's vault and holds the secret that bootstraps it
/// before a security key is enrolled. `device_secret` is a real
/// cryptographic input, not an auth token, which is why it lives in the OS
/// keyring and is wiped on drop. The id beside it is not a secret and is
/// skipped: `Uuid` has no `Zeroize`.
#[derive(Debug, Clone, Serialize, Deserialize, zeroize::ZeroizeOnDrop)]
pub struct LocalVaultAuth {
    #[zeroize(skip)]
    pub vault_id: Uuid,
    pub device_secret: String,
}

/// Machine-local, never inside the silo folder: the folder is made to be
/// carried around and may sit in OneDrive, and a device secret travelling
/// with it would mean whoever holds a copy of the folder holds the vault.
/// Keyed by silo id, so moving the folder does not lose the secret.
fn credentials_path(silo_id: Uuid) -> PathBuf {
    crate::workdir::secrets_dir_for(silo_id).join("credentials.json")
}

/// Whether this particular silo has credentials on this machine.
pub fn is_provisioned(silo_id: Uuid) -> bool {
    keyring_entry(silo_id)
        .and_then(|entry| entry.get_password())
        .is_ok()
        || read_fallback_file(silo_id).is_some_and(|creds| creds.vault_id == silo_id)
}

/// Reads and unwraps the fallback credentials file, if there is one to read.
/// `None` covers absent, unreadable and unparseable alike: every caller
/// treats all three as "no credentials here".
fn read_fallback_file(silo_id: Uuid) -> Option<LocalVaultAuth> {
    let raw = std::fs::read(credentials_path(silo_id)).ok()?;
    crate::format::decode("credentials", &unwrap_protected(&raw)?).ok()
}

/// Write to the OS keyring, then immediately read it back with a fresh
/// handle to confirm the write actually persisted. Some Windows Credential
/// Manager configurations report `set_password` as successful without the
/// entry actually becoming readable afterward, which would otherwise leave
/// `is_provisioned`/`load_credentials` unable to find credentials that were
/// supposedly just saved.
fn keyring_roundtrips(silo_id: Uuid, json: &str) -> bool {
    let Ok(entry) = keyring_entry(silo_id) else {
        return false;
    };
    if entry.set_password(json).is_err() {
        return false;
    }
    let Ok(verify_entry) = keyring_entry(silo_id) else {
        return false;
    };
    matches!(verify_entry.get_password(), Ok(stored) if stored == json)
}

pub fn save_credentials(creds: &LocalVaultAuth) -> Result<(), VaultError> {
    let json = String::from_utf8(crate::format::encode(creds)?)
        .map_err(|e| VaultError::Crypto(e.to_string()))?;

    if keyring_roundtrips(creds.vault_id, &json) {
        let stale = credentials_path(creds.vault_id);
        if stale.is_file() {
            let _ = std::fs::remove_file(stale);
        }
        return Ok(());
    }

    // Keyring write didn't verifiably persist on this machine — fall back to
    // a local file so provisioning still succeeds. `load_credentials` already
    // supports this path (and retries the keyring next time it's called).
    write_fallback_file(creds.vault_id, &json)
}

/// The keyring-unavailable fallback: DPAPI-wraps (where available) and
/// writes the credentials file. Split out from `save_credentials` so it's
/// directly testable without depending on the real OS keyring's behavior on
/// the test machine.
fn write_fallback_file(silo_id: Uuid, json: &str) -> Result<(), VaultError> {
    // Wrap it with DPAPI where available (Windows) so the file isn't usable
    // outside this specific Windows user's live logon session. Elsewhere the
    // file is the secret in the clear, which is why it is written private and
    // kept out of the silo folder.
    let to_write = match dpapi::protect(json.as_bytes()) {
        Some(protected) => {
            let mut out = DPAPI_MAGIC.to_vec();
            out.extend_from_slice(&protected);
            out
        }
        None => json.as_bytes().to_vec(),
    };
    crate::workdir::write_private(&credentials_path(silo_id), &to_write)?;
    Ok(())
}

pub fn load_credentials(silo_id: Uuid) -> Result<LocalVaultAuth, VaultError> {
    if let Ok(entry) = keyring_entry(silo_id)
        && let Ok(json) = entry.get_password()
    {
        return crate::format::decode("credentials", json.as_bytes());
    }

    let path = credentials_path(silo_id);
    if !path.is_file() {
        return Err(VaultError::NotProvisioned);
    }

    let raw = std::fs::read(&path)?;
    let json_bytes: Vec<u8> = match raw.strip_prefix(DPAPI_MAGIC) {
        Some(protected) => dpapi::unprotect(protected)
            .ok_or_else(|| VaultError::Crypto("failed to unwrap credential file".into()))?,
        None => raw,
    };
    let creds: LocalVaultAuth = crate::format::decode("credentials", &json_bytes)?;

    // Both lookups are keyed by silo id now, so a mismatch means the file
    // was tampered with or hand-edited rather than merely misfiled. Still
    // refused here rather than at unlock, which is where it used to surface:
    // after the person had already been asked to touch their key, and phrased
    // as a fact about their credentials rather than about the folder.
    if creds.vault_id != silo_id {
        return Err(VaultError::NotProvisioned);
    }

    // Try to migrate the fallback file into the keyring, but only remove it
    // if the keyring can actually be read back afterward — otherwise these
    // credentials would be silently lost on a machine where the keyring
    // write never verifiably persists (see `keyring_roundtrips`).
    let _ = save_credentials(&creds);
    if keyring_entry(silo_id)
        .and_then(|e| e.get_password())
        .is_ok()
    {
        let _ = std::fs::remove_file(path);
    }
    Ok(creds)
}

/// Forget this vault's credentials entirely (keyring entry and fallback
/// file) — used when resetting the app back to an unprovisioned state.
/// Best-effort: a missing entry isn't an error.
pub fn clear_credentials(silo_id: Uuid) {
    if let Ok(entry) = keyring_entry(silo_id) {
        let _ = entry.delete_credential();
    }
    let _ = std::fs::remove_file(credentials_path(silo_id));
}

/// Unwraps a file this crate wrote with DPAPI protection, if it is
/// protected; returns it unchanged otherwise.
fn unwrap_protected(raw: &[u8]) -> Option<Vec<u8>> {
    match raw.strip_prefix(DPAPI_MAGIC) {
        Some(protected) => dpapi::unprotect(protected),
        None => Some(raw.to_vec()),
    }
}

/// Keyed by silo, so several silos on one machine don't overwrite each
/// other's secret in the keyring — which is what a fixed entry name would
/// do, silently, leaving every silo but the last one unopenable.
fn keyring_entry(silo_id: Uuid) -> Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, &format!("{KEYRING_USER}:{silo_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fallback file lives under `work_base()`, which is a real directory
    /// on the machine running the tests rather than a temporary one. Each
    /// test therefore takes a fresh random id, so parallel runs cannot
    /// collide, and removes what it wrote even when an assertion fails.
    struct Scratch(Uuid);

    impl Scratch {
        fn new() -> Self {
            Scratch(Uuid::new_v4())
        }
        fn id(&self) -> Uuid {
            self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(crate::workdir::secrets_dir_for(self.0));
        }
    }

    fn encoded(creds: &LocalVaultAuth) -> String {
        String::from_utf8(crate::format::encode(creds).unwrap()).unwrap()
    }

    /// The point of the whole file location. A silo folder is meant to be
    /// carried to another disk and may sit in OneDrive, and this secret
    /// unwraps `master.dek.enc` with the salt that lives in that same folder.
    #[test]
    fn the_fallback_file_never_lands_in_the_silo_folder() {
        let scratch = Scratch::new();
        let silo_root = std::path::Path::new(r"C:\Users\alex\OneDrive\Documents\Silo");

        let path = credentials_path(scratch.id());

        assert!(!path.starts_with(silo_root));
        assert!(path.starts_with(crate::workdir::work_base()));
    }

    /// Regression test for an activation bug hit in production: on a fresh
    /// install the parent directory does not exist yet, and the write failed
    /// with "path not found" even though nothing was actually wrong.
    #[test]
    fn creates_its_directory_before_writing() {
        let scratch = Scratch::new();
        assert!(!credentials_path(scratch.id()).exists());

        write_fallback_file(scratch.id(), r#"{"fake":"credentials"}"#).unwrap();

        assert!(credentials_path(scratch.id()).is_file());
    }

    #[test]
    fn writing_twice_replaces_rather_than_appends() {
        let scratch = Scratch::new();
        write_fallback_file(scratch.id(), r#"{"first":true}"#).unwrap();
        write_fallback_file(scratch.id(), r#"{"second":true}"#).unwrap();

        let raw = std::fs::read(credentials_path(scratch.id())).unwrap();
        let json = unwrap_protected(&raw).unwrap();
        assert_eq!(String::from_utf8(json).unwrap(), r#"{"second":true}"#);
    }

    #[cfg(unix)]
    #[test]
    fn the_fallback_file_is_readable_only_by_this_user() {
        // On Unix a fresh file under ~/.cache is world-readable by default,
        // and off Windows this file is the device secret in the clear.
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        write_fallback_file(scratch.id(), r#"{"fake":"credentials"}"#).unwrap();

        let mode = std::fs::metadata(credentials_path(scratch.id()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }

    /// A file whose contents name a different vault has been tampered with
    /// and is refused before anyone touches a key. Only the rejecting
    /// direction is asserted: the matching one would write to the real OS
    /// keyring of the test machine.
    #[test]
    fn refuses_a_file_naming_another_silo() {
        let scratch = Scratch::new();
        let owner = Uuid::new_v4();

        write_fallback_file(
            scratch.id(),
            &encoded(&LocalVaultAuth {
                vault_id: owner,
                device_secret: "aa".repeat(32),
            }),
        )
        .unwrap();

        assert!(
            matches!(
                load_credentials(scratch.id()),
                Err(VaultError::NotProvisioned)
            ),
            "another silo's credentials must not be handed over"
        );
        assert_eq!(
            read_fallback_file(scratch.id()).map(|c| c.vault_id),
            Some(owner),
            "and the file itself must be left alone"
        );
    }

    #[test]
    fn a_newer_credentials_file_says_so_instead_of_looking_unprovisioned() {
        // The failure the envelope exists to prevent. Read as "absent", this
        // sends someone to set up a silo they already have, on a machine
        // where the real credentials are sitting in the file being ignored.
        let scratch = Scratch::new();

        write_fallback_file(
            scratch.id(),
            r#"{"version": 99, "data": {"vault_id": "00000000-0000-0000-0000-000000000001",
               "device_secret": "aa"}}"#,
        )
        .unwrap();

        let err = load_credentials(scratch.id()).unwrap_err();
        assert!(
            matches!(&err, VaultError::Corrupted(m) if m.contains("newer version")),
            "expected a version complaint, got {err:?}"
        );
        assert!(
            !matches!(err, VaultError::NotProvisioned),
            "the one answer that must never come back"
        );
    }

    /// Touches the real OS keyring on purpose, which the tests above avoid.
    ///
    /// Without a platform backend, keyring 3 silently substitutes `mock`,
    /// which builds an empty credential on every `Entry::new` and so can
    /// never round-trip. That shipped: every secret took the fallback-file
    /// path instead, in plaintext on Linux and macOS. Nothing but a real
    /// round-trip catches it, because the code compiles and runs either way.
    #[test]
    fn the_platform_keyring_is_real_and_persists_a_write() {
        let id = Uuid::new_v4();
        let json = encoded(&LocalVaultAuth {
            vault_id: id,
            device_secret: "aa".repeat(32),
        });

        assert!(
            keyring_roundtrips(id, &json),
            "the OS keyring did not hand back what was just written to it, \
             which is what the mock backend does"
        );

        if let Ok(entry) = keyring_entry(id) {
            let _ = entry.delete_credential();
        }
    }
}
