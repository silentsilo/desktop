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

/// How the wrap key was derived from the authenticator: `hmac-secret` over
/// SHA-256 of `silentsilo-dek-v1:{vault_id}`, then BLAKE3 of the output.
/// Recorded rather than assumed, like the recovery envelope's KDF
/// parameters, so a backend deriving differently (a phone's PRF, a TPM) is
/// skipped by clients that cannot reproduce it instead of unwrapping to
/// nothing.
pub const DERIVATION_HMAC_V1: &str = "hmac-secret-v1";

/// A key an organisation administers, not the person holding this machine.
///
/// An employee cannot retire a key carrying it, so a company keeps a way into
/// a silo it provisioned after the employee leaves.
pub const POLICY_ORG: &str = "org";

fn default_kind() -> String {
    KIND_FIDO2.to_string()
}

fn default_derivation() -> String {
    DERIVATION_HMAC_V1.to_string()
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
    /// See [`DERIVATION_HMAC_V1`].
    #[serde(default = "default_derivation")]
    pub derivation: String,
    /// Who administers this key: empty for the ordinary case, whoever holds
    /// the machine, or [`POLICY_ORG`] for a key an organisation provisioned
    /// and the machine's user may not retire.
    ///
    /// Defaults like `kind` and `derivation` beside it, and for the same
    /// reason rather than for any older client: an envelope assembled by
    /// something that does not know the field (the extraction tool, another
    /// platform's build) must read as an ordinary key instead of being
    /// refused. A silo that will not load is the worst outcome available.
    #[serde(default)]
    pub policy: String,
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

impl StoredFidoCredential {
    /// Whether an organisation, rather than this machine's user, decides this
    /// key's fate.
    pub fn managed(&self) -> bool {
        self.policy == POLICY_ORG
    }
}

/// Every enrolled key is equivalent: each one wraps the same DEK, so any of
/// them can unlock the vault on its own. A second key is redundancy against
/// losing the first, not a differently-privileged "recovery" key.
///
/// A policy does not change that: an organisation key unlocks exactly like
/// any other. What it changes is who may retire it.
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

/// Evidence that whoever is writing holds one of the silo's organisation keys.
///
/// Constructible only by [`OrgProof::verify`], which checks the material rather
/// than taking anyone's word: a caller that has not actually derived the wrap
/// key of an enrolled organisation key cannot produce one of these. That is the
/// whole point. A guard a command has to remember to call is a guard a command
/// will one day forget, and forgetting it is exactly how rotation came to be
/// able to drop a company's keys without asking.
#[derive(Debug)]
pub struct OrgProof(());

impl OrgProof {
    /// Verifies that `wrap_key` really unwraps that credential's envelope, and
    /// that the credential is one this silo treats as an organisation key.
    ///
    /// Takes the whole key set rather than one credential so the membership
    /// half cannot be skipped by a caller passing a credential it made up.
    pub fn verify(
        keys: &StoredFidoKeys,
        credential_id: &[u8],
        wrap_key: &[u8; 32],
    ) -> Result<Self, VaultError> {
        let hex_id = hex::encode(credential_id);
        let key = keys
            .managed()
            .find(|k| k.credential_id == hex_id)
            .ok_or(VaultError::InvalidCredentials)?;
        crate::dek_store::unwrap_dek_hex(&key.wrapped_dek, wrap_key)?;
        Ok(OrgProof(()))
    }
}

/// Who is behind a write to the enrolled keys.
///
/// Named at every call site on purpose: a new command cannot reach the file
/// without saying which of these it is, and saying `Personal` does not let it
/// past the check below. Compare with a guard in the command itself, which a
/// new command simply never calls.
pub enum Authority<'a> {
    /// The person holding this machine, and every automatic path: sync
    /// tidying tombstones, a join writing what the bucket already holds,
    /// enrolling or renaming a key. None of those take anything away.
    Machine,
    /// Somebody who has just proved they hold an organisation key.
    Organisation(&'a OrgProof),
}

/// Whether going from `stored` to `next` would cut an organisation out.
///
/// The question is about the change, not about the caller, which is what lets
/// every legitimate write through without an allow-list of blessed commands:
/// adding keys, renaming them, retiring ordinary ones and reconciling
/// tombstones all leave the organisation's access exactly where it was.
fn removes_organisation_access(stored: &StoredFidoKeys, next: &StoredFidoKeys) -> bool {
    stored.managed().any(|had| {
        !next
            .managed()
            .any(|still| still.credential_id == had.credential_id)
    })
}

/// Replaces the enrolled keys.
///
/// Refuses, whoever is asking, a write that would leave an organisation-
/// administered silo with fewer organisation keys than it had, unless the
/// caller carries an [`OrgProof`]. The check reads what is on disk rather than
/// trusting the caller's idea of the previous state.
pub fn save_fido_keys(
    vault_root: &Path,
    keys: &StoredFidoKeys,
    authority: Authority<'_>,
) -> Result<(), VaultError> {
    if matches!(authority, Authority::Machine)
        && let Ok(stored) = load_fido_keys(vault_root)
        && removes_organisation_access(&stored, keys)
    {
        return Err(VaultError::OrganisationKeyRequired);
    }

    // Atomic: this file holds every enrolled key's envelope, and on a silo
    // with no recovery code it is the only door.
    crate::workdir::write_private(&fido_keys_path(vault_root), &crate::format::encode(keys)?)?;
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

    /// The keys an organisation administers. Empty on every silo a person set
    /// up for themselves, which is the case this must stay silent about.
    pub fn managed(&self) -> impl Iterator<Item = &StoredFidoCredential> {
        self.active().filter(|k| k.managed())
    }

    /// Whether this silo is administered by an organisation at all. What the
    /// guards on retiring keys and on regenerating the recovery code read:
    /// on an ordinary silo they must not change anything.
    pub fn is_org_controlled(&self) -> bool {
        self.managed().next().is_some()
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
        self.active()
            .filter(|k| k.kind == KIND_FIDO2 && k.derivation == DERIVATION_HMAC_V1)
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

    /// The allow-list for proving an organisation key is present.
    ///
    /// Over the managed keys only, and usable ones at that: an organisation
    /// key of a kind this machine cannot answer for is no use as proof here,
    /// however legitimately it is enrolled. Errors are skipped rather than
    /// collected, for the same reason as in [`Self::credential_ids_bytes`]:
    /// one unparseable id must not take down a ceremony the right key could
    /// have satisfied.
    pub fn managed_credential_ids_bytes(&self) -> Vec<Vec<u8>> {
        self.managed()
            .filter(|k| k.kind == KIND_FIDO2 && k.derivation == DERIVATION_HMAC_V1)
            .filter_map(|k| hex::decode(&k.credential_id).ok())
            .collect()
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
            derivation: DERIVATION_HMAC_V1.to_string(),
            policy: String::new(),
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
    fn an_envelope_that_names_no_kind_is_a_fido2_key() {
        // `kind`, `derivation` and `policy` default rather than being
        // required, so an envelope assembled by something that writes none of
        // them is read as the only thing it can be: an ordinary FIDO2 key.
        let json = br#"{"credential_id":"aa11","public_key":"3059","key_slot":0,
            "rp_id":"silentsilo.com","label":"Key","wrapped_dek":"de","platform":false}"#;
        let key: StoredFidoCredential = serde_json::from_slice(json).expect("parses");
        assert_eq!(key.kind, KIND_FIDO2);
        assert_eq!(key.derivation, DERIVATION_HMAC_V1);
        assert!(!key.managed(), "and not one an organisation administers");
        assert_eq!(StoredFidoKeys { keys: vec![key] }.usable().count(), 1);
    }

    #[test]
    fn a_derivation_this_build_cannot_reproduce_is_skipped() {
        // The phone case: same FIDO2 kind, wrap key derived another way.
        // Trying it would unwrap the DEK to nothing.
        let mut phone = fido2("cc33");
        phone.derivation = "prf-ios-v1".into();
        let keys = StoredFidoKeys {
            keys: vec![phone, fido2("aa11")],
        };

        assert_eq!(keys.active().count(), 2, "both are enrolled on the silo");
        assert_eq!(
            keys.credential_ids_bytes().expect("no error"),
            vec![vec![0xaa, 0x11]]
        );
        assert!(keys.find_by_credential_id(&[0xcc, 0x33]).is_none());
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

    /// An organisation key, as the escrow feature will write it.
    fn org(id: &str) -> StoredFidoCredential {
        StoredFidoCredential {
            policy: POLICY_ORG.into(),
            label: "Organization key".into(),
            ..fido2(id)
        }
    }

    #[test]
    fn a_key_on_an_ordinary_silo_belongs_to_whoever_holds_the_machine() {
        // The case every personal silo is in, and the one this must not make
        // stricter: an ordinary key stays removable by its owner.
        let keys = StoredFidoKeys {
            keys: vec![fido2("aa11"), fido2("bb22")],
        };
        assert!(!keys.keys[0].managed());
        assert!(!keys.is_org_controlled());
    }

    #[test]
    fn an_organisation_key_unlocks_like_any_other() {
        // The escrow key is the company's way back in, so filtering it out of
        // `usable` would defeat the entire feature.
        let keys = StoredFidoKeys {
            keys: vec![org("aa11"), fido2("bb22")],
        };
        assert_eq!(keys.usable().count(), 2);
        assert!(keys.find_by_credential_id(&[0xaa, 0x11]).is_some());
        assert_eq!(keys.managed().count(), 1);
        assert!(keys.is_org_controlled());
    }

    #[test]
    fn a_revoked_organisation_key_stops_counting() {
        // Retiring one is allowed from an organisation session, and once it is
        // gone the silo is no longer org-controlled if nothing else carries a
        // policy. Otherwise a silo would stay locked into escrow rules for
        // ever by a key that no longer exists.
        let mut retired = org("aa11");
        retired.revoked = true;
        let keys = StoredFidoKeys {
            keys: vec![retired, fido2("bb22")],
        };
        assert_eq!(keys.managed().count(), 0);
        assert!(!keys.is_org_controlled());
    }

    #[test]
    fn the_policy_survives_a_round_trip_through_the_silo_file() {
        // The employee's machine saves the whole key set locally after joining
        // a silo the company provisioned. Dropping the field here would turn
        // the organisation key into an ordinary one this client may retire.
        let keys = StoredFidoKeys {
            keys: vec![org("aa11"), fido2("bb22")],
        };
        let bytes = crate::format::encode(&keys).expect("encodes");
        let back: StoredFidoKeys = crate::format::decode("keys", &bytes).expect("decodes");
        assert_eq!(back.keys[0].policy, POLICY_ORG);
        assert!(back.is_org_controlled());
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

    /// What the write path refuses, and what it must not.
    ///
    /// These exist because the guards in the command layer cannot be tested:
    /// each one runs a FIDO ceremony that needs a physical key in a human
    /// hand. Moving the rule into the file write is what makes it reachable
    /// from a test at all, and the first case below is the bug that shipped
    /// and lived for five hours in a command that simply never asked.
    mod writing {
        use super::*;

        /// A silo the company provisioned: its key, and the employee's.
        fn provisioned(dir: &Path) -> StoredFidoKeys {
            let keys = StoredFidoKeys {
                keys: vec![org("aa11"), fido2("bb22")],
            };
            save_fido_keys(dir, &keys, Authority::Machine).expect("provisioning is a write");
            keys
        }

        #[test]
        fn dropping_the_organisation_key_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            provisioned(dir.path());

            // Exactly what rotation did: keep my key, drop everything else.
            let employee_only = StoredFidoKeys {
                keys: vec![fido2("bb22")],
            };
            let err = save_fido_keys(dir.path(), &employee_only, Authority::Machine)
                .expect_err("the company's key cannot be dropped by the machine's holder");
            assert!(matches!(err, VaultError::OrganisationKeyRequired));

            // And the file still says what it said.
            assert!(load_fido_keys(dir.path()).unwrap().is_org_controlled());
        }

        #[test]
        fn revoking_the_organisation_key_is_the_same_thing() {
            // Retirement marks rather than removes, so a check that only
            // looked for a missing entry would wave this straight through.
            let dir = tempfile::tempdir().unwrap();
            let mut keys = provisioned(dir.path());
            keys.keys[0].revoked = true;

            let err = save_fido_keys(dir.path(), &keys, Authority::Machine)
                .expect_err("a tombstone on the company's key is still losing it");
            assert!(matches!(err, VaultError::OrganisationKeyRequired));
        }

        #[test]
        fn clearing_the_policy_is_the_same_thing_again() {
            // The subtlest way to do it: keep the credential, drop the marking,
            // and every later guard reads an ordinary key.
            let dir = tempfile::tempdir().unwrap();
            let mut keys = provisioned(dir.path());
            keys.keys[0].policy = String::new();

            let err = save_fido_keys(dir.path(), &keys, Authority::Machine)
                .expect_err("a key stripped of its policy is a key the company lost");
            assert!(matches!(err, VaultError::OrganisationKeyRequired));
        }

        #[test]
        fn an_organisation_key_in_hand_is_allowed_to_retire_itself() {
            // Rotating the company's own key, and handing a silo over, both
            // end here. Refusing this would make escrow permanent, which is
            // its own kind of trap.
            let dir = tempfile::tempdir().unwrap();
            let keys = provisioned(dir.path());

            let wrap_key = [7u8; 32];
            let wrapped =
                crate::dek_store::wrap_dek_bytes(&silentsilo_crypto::generate_dek(), &wrap_key)
                    .expect("wraps");
            let mut with_real_envelope = keys.clone();
            with_real_envelope.keys[0].wrapped_dek = hex::encode(&wrapped);

            let proof = OrgProof::verify(&with_real_envelope, &[0xaa, 0x11], &wrap_key)
                .expect("the key really does unwrap its own envelope");

            let handed_over = StoredFidoKeys {
                keys: vec![fido2("bb22")],
            };
            save_fido_keys(dir.path(), &handed_over, Authority::Organisation(&proof))
                .expect("an organisation may retire its own key");
            assert!(!load_fido_keys(dir.path()).unwrap().is_org_controlled());
        }

        #[test]
        fn every_write_that_takes_nothing_away_still_works() {
            // The half that would break sync and joining if this were an
            // allow-list of blessed commands instead of a question about the
            // change: adding a key, renaming one, and retiring an ordinary
            // one all leave the company exactly where it was.
            let dir = tempfile::tempdir().unwrap();
            let keys = provisioned(dir.path());

            let mut added = keys.clone();
            added.keys.push(fido2("cc33"));
            save_fido_keys(dir.path(), &added, Authority::Machine).expect("adding a key");

            let mut renamed = added.clone();
            renamed.keys[0].label = "Company safe".into();
            save_fido_keys(dir.path(), &renamed, Authority::Machine).expect("renaming a key");

            let ordinary_gone = StoredFidoKeys {
                keys: vec![renamed.keys[0].clone(), fido2("cc33")],
            };
            save_fido_keys(dir.path(), &ordinary_gone, Authority::Machine)
                .expect("retiring an ordinary key");
        }

        #[test]
        fn a_personal_silo_is_not_made_stricter() {
            // The case almost every silo is in. Nothing here is administered,
            // so every write is the machine's to make.
            let dir = tempfile::tempdir().unwrap();
            save_fido_keys(
                dir.path(),
                &StoredFidoKeys {
                    keys: vec![fido2("aa11"), fido2("bb22")],
                },
                Authority::Machine,
            )
            .expect("provisioning");

            save_fido_keys(
                dir.path(),
                &StoredFidoKeys {
                    keys: vec![fido2("bb22")],
                },
                Authority::Machine,
            )
            .expect("an owner may retire their own key");
        }

        #[test]
        fn proof_cannot_be_made_from_a_key_that_is_not_the_organisation_s() {
            // The token is only worth something if it cannot be produced by
            // the employee's own key, or by a credential id somebody invented.
            let wrap_key = [7u8; 32];
            let wrapped =
                crate::dek_store::wrap_dek_bytes(&silentsilo_crypto::generate_dek(), &wrap_key)
                    .expect("wraps");
            let mut keys = StoredFidoKeys {
                keys: vec![org("aa11"), fido2("bb22")],
            };
            keys.keys[0].wrapped_dek = hex::encode(&wrapped);
            keys.keys[1].wrapped_dek = hex::encode(&wrapped);

            // The employee's key, whose envelope this wrap key does open.
            assert!(
                OrgProof::verify(&keys, &[0xbb, 0x22], &wrap_key).is_err(),
                "an ordinary key must not authorise anything"
            );
            // A credential that is not enrolled at all.
            assert!(OrgProof::verify(&keys, &[0xff, 0xff], &wrap_key).is_err());
            // The right key, the wrong material.
            assert!(OrgProof::verify(&keys, &[0xaa, 0x11], &[9u8; 32]).is_err());
            // And the one combination that should work.
            assert!(OrgProof::verify(&keys, &[0xaa, 0x11], &wrap_key).is_ok());
        }
    }
}
