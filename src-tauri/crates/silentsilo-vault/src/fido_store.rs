use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::VaultError;

const FIDO_KEYS_FILE: &str = "keys/fido.json";

pub const KEY_SLOT_PRIMARY: u8 = 0;

/// The only kind of key this build can unlock with: a FIDO2 credential whose
/// `hmac-secret` output derives the key that unwraps `wrapped_dek`.
///
/// It is a string rather than an enum on purpose. The point of the field is
/// to be read by a build that has never heard of the value in it, and an enum
/// would make that a parse error instead of an answer.
pub const KIND_FIDO2: &str = "fido2";

fn default_kind() -> String {
    KIND_FIDO2.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFidoCredential {
    /// How this key's `wrapped_dek` is unwrapped. Every key enrolled today
    /// is [`KIND_FIDO2`]; a future Secure Enclave or TPM kind must be
    /// skippable by clients that have never heard of it, which is why the
    /// field ships now and defaults rather than being required. Installed
    /// clients cannot be taught after the fact.
    #[serde(default = "default_kind")]
    pub kind: String,
    pub credential_id: String,
    pub public_key: String,
    pub key_slot: u8,
    pub rp_id: String,
    /// User-visible name (e.g. "Office key").
    #[serde(default)]
    pub label: String,
    /// AES-GCM wrapped DEK (hex) so this key can unlock the vault locally.
    #[serde(default)]
    pub wrapped_dek: String,
    /// True for a built-in authenticator (Windows Hello, Touch ID).
    ///
    /// Cryptographically identical to a removable key, but it does not
    /// survive the machine — the UI has to say so, or someone will treat
    /// it as their backup and lose the vault with the laptop.
    #[serde(default)]
    pub platform: bool,
    /// Removed here, but its envelope may still be in the bucket.
    ///
    /// Deleting the row outright would be a lie whenever the delete does not
    /// reach storage: the published envelope keeps unlocking the vault from
    /// any machine, and nothing would ever try again. The entry therefore
    /// stays as a tombstone, counts as no key at all, and every sync pass
    /// retries the deletion until storage confirms it.
    #[serde(default)]
    pub revoked: bool,
}

/// Every enrolled key is equivalent: each one wraps the same DEK, so any of
/// them can unlock the vault on its own. A second key is redundancy against
/// losing the first, not a differently-privileged "recovery" key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFidoKeys {
    pub keys: Vec<StoredFidoCredential>,
}

pub fn fido_keys_path(vault_root: &Path) -> PathBuf {
    vault_root.join(FIDO_KEYS_FILE)
}

pub fn is_fido_enrolled(vault_root: &Path) -> bool {
    fido_keys_path(vault_root).is_file()
}

/// Whether a spare key is enrolled, for the UI nudge toward a second one.
/// Counts what *this machine* could unlock with, not what the silo has: a
/// Touch ID key on someone's laptop is no backup for the Windows desktop
/// next to it.
pub fn has_backup_key(vault_root: &Path) -> bool {
    load_fido_keys(vault_root)
        .ok()
        .map(|k| k.usable().count() > 1)
        .unwrap_or(false)
}

pub fn save_fido_keys(vault_root: &Path, keys: &StoredFidoKeys) -> Result<(), VaultError> {
    let path = fido_keys_path(vault_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, crate::format::encode(keys)?)?;
    Ok(())
}

pub fn load_fido_keys(vault_root: &Path) -> Result<StoredFidoKeys, VaultError> {
    let path = fido_keys_path(vault_root);
    let keys: StoredFidoKeys = crate::format::decode("the enrolled keys", &std::fs::read(path)?)?;
    if keys.keys.is_empty() {
        return Err(VaultError::InvalidCredentials);
    }
    Ok(keys)
}

impl StoredFidoKeys {
    /// The keys that still open the vault. Every question about "the enrolled
    /// keys" means this one, never the raw vector, which also holds the
    /// tombstones of revocations that have not reached storage yet.
    pub fn active(&self) -> impl Iterator<Item = &StoredFidoCredential> {
        self.keys.iter().filter(|k| !k.revoked)
    }

    /// Revocations still owed to storage.
    pub fn revoked_ids(&self) -> Vec<String> {
        self.keys
            .iter()
            .filter(|k| k.revoked)
            .map(|k| k.credential_id.clone())
            .collect()
    }

    /// The keys this build can actually unlock with. [`Self::active`]
    /// answers "is this enrolled on the silo", the right question for
    /// publishing and revoking; this answers "could a ceremony on this
    /// machine end with the DEK", the one every unlock path wants.
    pub fn usable(&self) -> impl Iterator<Item = &StoredFidoCredential> {
        self.active().filter(|k| k.kind == KIND_FIDO2)
    }

    pub fn primary(&self) -> Option<&StoredFidoCredential> {
        self.usable().next()
    }

    /// Counts tombstones too: a slot number that was used once must not come
    /// back while the envelope it named may still be in the bucket.
    pub fn next_slot(&self) -> u8 {
        self.keys
            .iter()
            .map(|k| k.key_slot)
            .max()
            .map(|m| m.saturating_add(1))
            .unwrap_or(KEY_SLOT_PRIMARY)
    }

    /// The allow-list handed to the authenticator.
    ///
    /// Over [`Self::usable`], and that is the load-bearing part. A credential
    /// id only has to be hex because a FIDO2 one is; another kind is free to
    /// identify its key however its platform does. Built from every enrolled
    /// key, one such id would fail the `collect` and take the whole unlock or
    /// join down with it, on a machine whose own key is sitting right there.
    pub fn credential_ids_bytes(&self) -> Result<Vec<Vec<u8>>, VaultError> {
        self.usable()
            .map(|k| hex::decode(&k.credential_id).map_err(|_| VaultError::InvalidCredentials))
            .collect()
    }

    pub fn find_by_credential_id(&self, id: &[u8]) -> Option<&StoredFidoCredential> {
        let hex_id = hex::encode(id);
        self.usable().find(|k| k.credential_id == hex_id)
    }
}

/// What a build that has never heard of a key's kind does with it.
///
/// These are the tests the `kind` field exists for. They describe a client
/// meeting an envelope written by a platform it does not know, which is the
/// situation a macOS or Linux build creates on the day it ships and cannot
/// be arranged after the fact: the clients that have to cope are the ones
/// already installed.
#[cfg(test)]
mod tests {
    use super::*;

    fn fido2(id: &str) -> StoredFidoCredential {
        StoredFidoCredential {
            kind: KIND_FIDO2.to_string(),
            credential_id: id.into(),
            public_key: "3059".into(),
            key_slot: 0,
            rp_id: "silentsilo.com".into(),
            label: "Key".into(),
            wrapped_dek: "deadbeef".into(),
            platform: false,
            revoked: false,
        }
    }

    /// A key of a kind this build cannot use. `credential_id` is deliberately
    /// not hex: nothing says another platform identifies its keys the way a
    /// FIDO2 credential does, and code that assumes so is exactly what this
    /// guards.
    fn foreign(id: &str) -> StoredFidoCredential {
        StoredFidoCredential {
            kind: "secure-enclave".into(),
            platform: true,
            label: "MacBook Touch ID".into(),
            ..fido2(id)
        }
    }

    #[test]
    fn an_envelope_written_before_the_field_existed_is_a_fido2_key() {
        // The backwards half. Every envelope already in a bucket lacks the
        // field, and reading those as "unknown kind" would lock every
        // existing silo out of itself.
        let json = br#"{"credential_id":"aa11","public_key":"3059","key_slot":0,
            "rp_id":"silentsilo.com","label":"Key","wrapped_dek":"de","platform":false}"#;
        let key: StoredFidoCredential = serde_json::from_slice(json).expect("parses");
        assert_eq!(key.kind, KIND_FIDO2);
        assert_eq!(StoredFidoKeys { keys: vec![key] }.usable().count(), 1);
    }

    #[test]
    fn a_kind_this_build_cannot_use_is_not_offered_to_the_authenticator() {
        let keys = StoredFidoKeys {
            keys: vec![fido2("aa11"), foreign("touch-id-key-1")],
        };
        assert_eq!(keys.active().count(), 2, "both are enrolled on the silo");
        assert_eq!(
            keys.credential_ids_bytes().expect("no error"),
            vec![vec![0xaa, 0x11]],
            "only the key this build can answer for belongs in the allow-list"
        );
    }

    #[test]
    fn one_foreign_key_does_not_take_the_whole_unlock_down() {
        // The failure this is really about. `credential_ids_bytes` collects
        // into a Result, so before the kind filter a single id that was not
        // hex turned every unlock and every join into InvalidCredentials, on
        // a machine whose own key was plugged in and working.
        let keys = StoredFidoKeys {
            keys: vec![foreign("touch-id-key-1"), fido2("bb22")],
        };
        let ids = keys
            .credential_ids_bytes()
            .expect("a key of another kind must not fail the whole list");
        assert_eq!(ids, vec![vec![0xbb, 0x22]]);
        assert!(keys.primary().is_some(), "the usable key is still primary");
    }

    #[test]
    fn a_key_from_another_machine_is_no_backup_for_this_one() {
        // `has_backup_key` drives the nudge to enrol a spare. Counting a key
        // this machine cannot use would silence it on exactly the setup that
        // needs it most: one Windows key, and a silo that looks covered.
        let keys = StoredFidoKeys {
            keys: vec![fido2("aa11"), foreign("touch-id-key-1")],
        };
        assert_eq!(keys.usable().count(), 1);
        assert_eq!(keys.active().count(), 2);
    }

    #[test]
    fn a_revoked_key_of_a_usable_kind_is_still_gone() {
        // The two filters compose rather than replace each other.
        let mut revoked = fido2("aa11");
        revoked.revoked = true;
        let keys = StoredFidoKeys {
            keys: vec![revoked, fido2("bb22")],
        };
        assert_eq!(keys.usable().count(), 1);
        assert!(
            keys.find_by_credential_id(&[0xaa, 0x11]).is_none(),
            "a tombstone must not unlock anything"
        );
    }

    #[test]
    fn the_kind_survives_a_round_trip_through_the_silo_file() {
        // Written as well as read: a Windows client that joins a silo holding
        // a Mac's key saves the whole set locally, and dropping the field on
        // the way through would turn that key into a fido2 one it then tries
        // to use.
        let keys = StoredFidoKeys {
            keys: vec![fido2("aa11"), foreign("touch-id-key-1")],
        };
        let bytes = crate::format::encode(&keys).expect("encodes");
        let back: StoredFidoKeys = crate::format::decode("keys", &bytes).expect("decodes");
        assert_eq!(back.keys[1].kind, "secure-enclave");
        assert_eq!(back.usable().count(), 1);
    }
}
