//! A written-down code that can unlock the vault when every key is gone.
//! Always app-generated: 160 bits of OS randomness is beyond brute force
//! whatever the provider does with the envelope. There is no passphrase
//! option anywhere in this crate, because a memorable phrase is roughly
//! forty bits and would quietly become the real security of the design.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dek_store::{unwrap_dek_hex, wrap_dek_bytes};
use crate::error::VaultError;
use crate::kdf::KdfParams;
use silentsilo_crypto::MasterDek;

const RECOVERY_FILE: &str = "keys/recovery.json";

/// Shape of the stored envelope.
///
/// Separate from [`KdfParams`], which covers tuning: changing `m_cost` needs
/// no bump, because every envelope carries its own. This moves only if the
/// structure changes, and a reader that sees a higher number says so instead
/// of guessing.
pub const RECOVERY_ENVELOPE_VERSION: u32 = 1;

/// 20 bytes = 160 bits, which reads as 32 characters in Base32. Longer
/// would not be meaningfully safer and this already has to be copied by
/// hand onto paper.
const CODE_BYTES: usize = 20;

/// Crockford's alphabet: no I, L, O or U, so nothing in a handwritten code
/// can be confused with 1, 0, or an obscenity.
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The recovery envelope as stored, locally and in the bucket. The salt is
/// public by necessity, and `kdf` is stored rather than assumed: a code
/// written on paper years ago has to keep working after a retuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub version: u32,
    pub kdf: KdfParams,
    pub salt: String,
    pub wrapped_dek: String,
    /// Unix seconds, so the UI can say how old the written-down code is.
    pub created_at: i64,
}

pub fn recovery_path(vault_root: &Path) -> PathBuf {
    vault_root.join(RECOVERY_FILE)
}

pub fn has_recovery_code(vault_root: &Path) -> bool {
    recovery_path(vault_root).is_file()
}

/// A fresh code, formatted the way it will be written down.
///
/// Grouped in fours because that is how people copy long strings without
/// losing their place, and the groups are stripped again before use.
pub fn generate_recovery_code() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; CODE_BYTES];
    rand::rng().fill_bytes(&mut bytes);

    let mut out = String::new();
    // 20 bytes divides evenly into 5-byte groups of 8 Base32 characters,
    // so there is never any padding to explain.
    for chunk in bytes.chunks(5) {
        let n = u64::from(chunk[0]) << 32
            | u64::from(chunk[1]) << 24
            | u64::from(chunk[2]) << 16
            | u64::from(chunk[3]) << 8
            | u64::from(chunk[4]);
        for j in (0..8).rev() {
            if !out.is_empty() && out.len() % 5 == 4 {
                out.push('-');
            }
            out.push(ALPHABET[((n >> (j * 5)) & 0x1f) as usize] as char);
        }
    }
    out
}

/// What the user typed, reduced to what was actually generated.
///
/// People retype these from paper, so the forgiving cases are the point:
/// lowercase, missing or extra separators, and the letter/digit pairs that
/// Crockford's alphabet excludes precisely because handwriting confuses
/// them. Being strict here would reject correct codes.
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            'U' => 'V',
            other => other,
        })
        .collect()
}

/// Wraps the vault's DEK under a freshly generated code.
///
/// Returns the code, which is the only time it exists in readable form —
/// nothing derived from it is stored except the envelope, so an app that
/// fails to show it to the user has lost it too.
pub fn create_recovery_envelope(dek: &MasterDek) -> Result<(String, RecoveryEnvelope), VaultError> {
    let code = generate_recovery_code();
    let envelope = seal_under_code(dek, &code)?;
    Ok((code, envelope))
}

/// Wraps the DEK under a code the caller chose. Behind a feature flag and
/// it stays there: a public "wrap under whatever string you like" is the
/// passphrase option this crate refuses to have. The one legitimate caller
/// is the fixture builder, which needs a code a test already knows.
#[cfg(feature = "fixture-support")]
pub fn seal_under_code_for_fixtures(
    dek: &MasterDek,
    code: &str,
) -> Result<RecoveryEnvelope, VaultError> {
    seal_under_code(dek, code)
}

fn seal_under_code(dek: &MasterDek, code: &str) -> Result<RecoveryEnvelope, VaultError> {
    let salt: [u8; 16] = rand::random();
    let kdf = KdfParams::recovery_code();
    let key = kdf.derive(normalize_code(code).as_bytes(), &salt)?;
    let wrapped = wrap_dek_bytes(dek, &key)?;

    Ok(RecoveryEnvelope {
        version: RECOVERY_ENVELOPE_VERSION,
        kdf,
        salt: hex::encode(salt),
        wrapped_dek: hex::encode(wrapped),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

/// Recovers the DEK from a code the user typed.
///
/// Derives with the envelope's own parameters, which is what lets a code
/// printed under one set of settings survive a later retuning.
pub fn unwrap_with_code(envelope: &RecoveryEnvelope, code: &str) -> Result<MasterDek, VaultError> {
    if envelope.version > RECOVERY_ENVELOPE_VERSION {
        return Err(VaultError::Corrupted(format!(
            "recovery envelope version {} needs a newer SilentSilo",
            envelope.version
        )));
    }
    let salt = hex::decode(&envelope.salt).map_err(|_| VaultError::InvalidCredentials)?;
    let key = envelope
        .kdf
        .derive(normalize_code(code).as_bytes(), &salt)?;
    unwrap_dek_hex(&envelope.wrapped_dek, &key)
}

pub fn save_recovery_envelope(
    vault_root: &Path,
    envelope: &RecoveryEnvelope,
) -> Result<(), VaultError> {
    let json =
        serde_json::to_vec_pretty(envelope).map_err(|e| VaultError::Crypto(e.to_string()))?;
    crate::workdir::write_private(&recovery_path(vault_root), &json)?;
    Ok(())
}

pub fn load_recovery_envelope(vault_root: &Path) -> Result<RecoveryEnvelope, VaultError> {
    let bytes = std::fs::read(recovery_path(vault_root))?;
    serde_json::from_slice(&bytes).map_err(|e| VaultError::Corrupted(e.to_string()))
}

pub fn clear_recovery_envelope(vault_root: &Path) {
    let _ = std::fs::remove_file(recovery_path(vault_root));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdf::ARGON2ID;
    use silentsilo_crypto::generate_dek;

    #[test]
    fn a_generated_code_reads_as_eight_groups_of_four() {
        let code = generate_recovery_code();
        let groups: Vec<&str> = code.split('-').collect();
        assert_eq!(groups.len(), 8, "got {code}");
        assert!(groups.iter().all(|g| g.len() == 4), "got {code}");
        assert_eq!(normalize_code(&code).len(), 32);
    }

    #[test]
    fn two_codes_are_never_the_same() {
        let a = generate_recovery_code();
        let b = generate_recovery_code();
        assert_ne!(a, b);
    }

    #[test]
    fn a_code_only_uses_unambiguous_characters() {
        // The point of Crockford's alphabet: nothing generated can be
        // misread as something else when copied by hand.
        for _ in 0..50 {
            let code = generate_recovery_code();
            for c in code.chars().filter(|c| *c != '-') {
                assert!(
                    !matches!(c, 'I' | 'L' | 'O' | 'U'),
                    "{c} is confusable, in {code}"
                );
            }
        }
    }

    #[test]
    fn a_code_round_trips_the_dek() {
        let dek = generate_dek();
        let (code, envelope) = create_recovery_envelope(&dek).unwrap();
        let recovered = unwrap_with_code(&envelope, &code).unwrap();
        assert_eq!(recovered.as_bytes(), dek.as_bytes());
    }

    #[test]
    fn a_retyped_code_is_accepted_however_it_was_transcribed() {
        // Someone reading paper back into the app: lowercase, no dashes,
        // spaces instead, and the letters Crockford excludes because
        // handwriting confuses them.
        let dek = generate_dek();
        let (code, envelope) = create_recovery_envelope(&dek).unwrap();

        for variant in [
            code.to_lowercase(),
            code.replace('-', ""),
            code.replace('-', " "),
            code.replace('0', "O").replace('1', "l"),
        ] {
            let recovered = unwrap_with_code(&envelope, &variant)
                .unwrap_or_else(|e| panic!("{variant:?} should have worked: {e}"));
            assert_eq!(recovered.as_bytes(), dek.as_bytes());
        }
    }

    #[test]
    fn a_wrong_code_is_refused_rather_than_returning_junk() {
        let dek = generate_dek();
        let (_, envelope) = create_recovery_envelope(&dek).unwrap();
        assert!(unwrap_with_code(&envelope, "0000-0000-0000-0000-0000-0000-0000-0000").is_err());
    }

    #[test]
    fn one_code_does_not_open_another_vault() {
        // Each envelope carries its own salt, so the same code typed at a
        // different vault derives a different key.
        let (code, _) = create_recovery_envelope(&generate_dek()).unwrap();
        let (_, other) = create_recovery_envelope(&generate_dek()).unwrap();
        assert!(unwrap_with_code(&other, &code).is_err());
    }

    #[test]
    fn a_code_still_opens_an_envelope_written_under_other_parameters() {
        // The reason the parameters are stored rather than read from
        // constants: retuning the KDF, which a security audit is likely to
        // ask for, must not invalidate codes already written on paper.
        let dek = generate_dek();
        let (code, mut envelope) = create_recovery_envelope(&dek).unwrap();

        // Rewrap under deliberately different settings, as an older build
        // would have produced.
        let older = KdfParams {
            algorithm: ARGON2ID.into(),
            m_cost: 19_456,
            t_cost: 2,
            p_cost: 1,
        };
        let salt = hex::decode(&envelope.salt).unwrap();
        let key = older
            .derive(normalize_code(&code).as_bytes(), &salt)
            .unwrap();
        envelope.wrapped_dek = hex::encode(wrap_dek_bytes(&dek, &key).unwrap());
        envelope.kdf = older;

        let recovered = unwrap_with_code(&envelope, &code).unwrap();
        assert_eq!(recovered.as_bytes(), dek.as_bytes());
    }

    #[test]
    fn an_unknown_derivation_is_refused_with_something_the_ui_can_say() {
        let dek = generate_dek();
        let (code, mut envelope) = create_recovery_envelope(&dek).unwrap();
        envelope.kdf.algorithm = "scrypt".into();

        // Mapped away because `MasterDek` deliberately has no `Debug`.
        let err = unwrap_with_code(&envelope, &code).map(|_| ()).unwrap_err();
        assert!(
            matches!(&err, VaultError::Corrupted(m) if m.contains("newer")),
            "expected a version complaint, got {err:?}"
        );
    }

    #[test]
    fn a_future_envelope_is_refused_rather_than_read_as_this_one() {
        let dek = generate_dek();
        let (code, mut envelope) = create_recovery_envelope(&dek).unwrap();
        envelope.version = RECOVERY_ENVELOPE_VERSION + 1;

        assert!(unwrap_with_code(&envelope, &code).is_err());
    }

    #[test]
    fn an_envelope_survives_a_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let dek = generate_dek();
        let (code, envelope) = create_recovery_envelope(&dek).unwrap();

        assert!(!has_recovery_code(dir.path()));
        save_recovery_envelope(dir.path(), &envelope).unwrap();
        assert!(has_recovery_code(dir.path()));

        let loaded = load_recovery_envelope(dir.path()).unwrap();
        assert_eq!(
            unwrap_with_code(&loaded, &code).unwrap().as_bytes(),
            dek.as_bytes()
        );

        clear_recovery_envelope(dir.path());
        assert!(!has_recovery_code(dir.path()));
    }
}
