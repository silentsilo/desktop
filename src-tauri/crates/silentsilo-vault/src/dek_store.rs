//! The Master DEK, wrapped under whatever key can currently produce it: one
//! derived from the device secret, one per enrolled security key, one from
//! the recovery code. Every wrapping uses the same versioned envelope as an
//! operation record, so there is a single format to change if the algorithm
//! ever has to move.

use std::path::Path;

use silentsilo_crypto::{MasterDek, seal_with_key, unseal_with_key};
use zeroize::Zeroizing;

use crate::error::VaultError;

const DEK_FILE: &str = "master.dek.enc";

pub fn dek_path(root: &Path) -> std::path::PathBuf {
    root.join(DEK_FILE)
}

pub fn wrap_dek_bytes(dek: &MasterDek, wrap_key: &[u8; 32]) -> Result<Vec<u8>, VaultError> {
    seal_with_key(dek.as_bytes(), wrap_key).map_err(|e| VaultError::Crypto(e.to_string()))
}

pub fn save_dek(root: &Path, dek: &MasterDek, wrap_key: &[u8; 32]) -> Result<(), VaultError> {
    let ciphertext = wrap_dek_bytes(dek, wrap_key)?;
    save_wrapped_dek_bytes(root, &ciphertext)
}

/// Writes an already-wrapped DEK envelope where unlock reads it. Atomic,
/// like every key file: this is the door to the silo, and a truncated write
/// during a crash must not be able to take it off its hinges.
pub fn save_wrapped_dek_bytes(root: &Path, ciphertext: &[u8]) -> Result<(), VaultError> {
    crate::workdir::write_private(&dek_path(root), ciphertext)?;
    Ok(())
}

pub fn load_dek(root: &Path, wrap_key: &[u8; 32]) -> Result<MasterDek, VaultError> {
    let data = std::fs::read(dek_path(root))?;
    unwrap_dek_bytes(&data, wrap_key)
}

/// Errors are deliberately indistinguishable between "wrong key" and "not a
/// wrapping this build understands": both mean the caller cannot open the
/// vault with what it has, and the difference is only useful to someone
/// probing.
pub fn unwrap_dek_bytes(data: &[u8], wrap_key: &[u8; 32]) -> Result<MasterDek, VaultError> {
    // `Zeroizing` because this vector is the DEK in the clear on its way into
    // the type that protects it. Without it the wrapper is careful about a
    // copy that was already left behind.
    let plain = Zeroizing::new(
        unseal_with_key(data, wrap_key).map_err(|_| VaultError::InvalidCredentials)?,
    );
    let key: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::InvalidCredentials)?;
    Ok(MasterDek::from_bytes(key))
}

pub fn unwrap_dek_hex(wrapped_hex: &str, wrap_key: &[u8; 32]) -> Result<MasterDek, VaultError> {
    let data = hex::decode(wrapped_hex).map_err(|_| VaultError::InvalidCredentials)?;
    unwrap_dek_bytes(&data, wrap_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use silentsilo_crypto::{SEAL_VERSION, generate_dek};

    #[test]
    fn wrap_unwrap_roundtrips_with_fresh_random_nonce() {
        let dek = generate_dek();
        let wrap_key = [7u8; 32];

        let wrapped_a = wrap_dek_bytes(&dek, &wrap_key).unwrap();
        let wrapped_b = wrap_dek_bytes(&dek, &wrap_key).unwrap();
        // Same DEK, same key, wrapped twice — must not reuse the nonce.
        assert_ne!(wrapped_a, wrapped_b);

        let unwrapped = unwrap_dek_bytes(&wrapped_a, &wrap_key).unwrap();
        assert_eq!(unwrapped.as_bytes(), dek.as_bytes());
    }

    #[test]
    fn a_wrapping_says_which_format_produced_it() {
        let wrapped = wrap_dek_bytes(&generate_dek(), &[7u8; 32]).unwrap();
        assert_eq!(&wrapped[..4], b"SSEA");
        assert_eq!(wrapped[4], SEAL_VERSION);
    }

    #[test]
    fn a_fixed_nonce_wrapping_is_refused_rather_than_recognised_by_length() {
        // The old scheme had no header and was identified by being exactly
        // 48 bytes. Length is not a format: a reader that guesses from it
        // can be steered, and the fixed nonce it selected is the one thing
        // AES-GCM must never see twice under one key.
        let dek = generate_dek();
        let wrap_key = [9u8; 32];

        let cipher = Aes256Gcm::new_from_slice(&wrap_key).unwrap();
        let nonce = Nonce::from_slice(b"sslo-dek-nce");
        let legacy = cipher.encrypt(nonce, dek.as_bytes().as_slice()).unwrap();
        assert_eq!(legacy.len(), 48);

        assert!(unwrap_dek_bytes(&legacy, &wrap_key).is_err());
    }

    #[test]
    fn unwrap_rejects_wrong_key() {
        let dek = generate_dek();
        let wrapped = wrap_dek_bytes(&dek, &[1u8; 32]).unwrap();
        assert!(unwrap_dek_bytes(&wrapped, &[2u8; 32]).is_err());
    }
}
