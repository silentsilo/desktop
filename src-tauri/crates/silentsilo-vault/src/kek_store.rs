//! The content KEK: the key every blob's content key is wrapped under.
//! Stored wrapped under the vault DEK, in the silo folder. It exists
//! because a record's fingerprint covers the wrapped content key inside it,
//! so that value can never be rewritten; wrapping content keys under a key
//! of their own moves what rotation has to touch out of the log.

use std::path::Path;

use silentsilo_crypto::{ContentKek, MasterDek, seal, unseal};
use zeroize::Zeroizing;

use crate::error::VaultError;

const KEK_FILE: &str = "content.kek.enc";

pub fn kek_path(root: &Path) -> std::path::PathBuf {
    root.join(KEK_FILE)
}

pub fn wrap_kek_bytes(kek: &ContentKek, dek: &MasterDek) -> Result<Vec<u8>, VaultError> {
    seal(kek.as_bytes(), dek).map_err(|e| VaultError::Crypto(e.to_string()))
}

pub fn save_kek(root: &Path, kek: &ContentKek, dek: &MasterDek) -> Result<(), VaultError> {
    crate::workdir::write_private(&kek_path(root), &wrap_kek_bytes(kek, dek)?)?;
    Ok(())
}

pub fn load_kek(root: &Path, dek: &MasterDek) -> Result<ContentKek, VaultError> {
    unwrap_kek_bytes(&std::fs::read(kek_path(root))?, dek)
}

/// Same deliberate vagueness as the DEK's: "wrong key" and "not a wrapping
/// this build understands" both mean the caller cannot open this, and the
/// difference is only useful to someone probing.
pub fn unwrap_kek_bytes(data: &[u8], dek: &MasterDek) -> Result<ContentKek, VaultError> {
    // `Zeroizing` because this is the key in the clear on its way into the
    // type that protects it.
    let plain = Zeroizing::new(unseal(data, dek).map_err(|_| VaultError::InvalidCredentials)?);
    let key: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::InvalidCredentials)?;
    Ok(ContentKek::from_bytes(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use silentsilo_crypto::{generate_content_kek, generate_dek};

    #[test]
    fn a_kek_round_trips_under_the_dek() {
        let dir = tempfile::tempdir().unwrap();
        let dek = generate_dek();
        let kek = generate_content_kek();

        save_kek(dir.path(), &kek, &dek).unwrap();
        let back = load_kek(dir.path(), &dek).unwrap();

        assert_eq!(back.as_bytes(), kek.as_bytes());
    }

    /// The property the whole rotation design rests on: re-wrapping the KEK
    /// under a new DEK leaves everything wrapped *under the KEK* readable,
    /// while closing it to anyone who only has the old DEK.
    ///
    /// If this ever stopped holding, rotation would mean re-encrypting every
    /// byte of content again, which is the outcome the design exists to
    /// avoid.
    #[test]
    fn rotating_the_dek_keeps_content_keys_readable() {
        use silentsilo_crypto::{generate_content_key, unwrap_content_key, wrap_content_key};

        let dir = tempfile::tempdir().unwrap();
        let old_dek = generate_dek();
        let kek = generate_content_kek();
        save_kek(dir.path(), &kek, &old_dek).unwrap();

        // A content key wrapped before the rotation, as it would sit inside
        // an operation record whose bytes may never change.
        let content = generate_content_key();
        let wrapped = wrap_content_key(&content, &kek).unwrap();

        // Rotation: one key re-wrapped, nothing else touched.
        let new_dek = generate_dek();
        save_kek(
            dir.path(),
            &load_kek(dir.path(), &old_dek).unwrap(),
            &new_dek,
        )
        .unwrap();

        let after = load_kek(dir.path(), &new_dek).unwrap();
        assert_eq!(
            unwrap_content_key(&wrapped, &after).unwrap().as_bytes(),
            content.as_bytes(),
            "content written before the rotation is still readable after it"
        );
        assert!(
            load_kek(dir.path(), &old_dek).is_err(),
            "and the old vault key no longer reaches the KEK"
        );
    }

    #[test]
    fn a_different_dek_cannot_open_it() {
        // The whole of rotation rests on this: re-wrapping the KEK under a
        // new DEK is what stops the old one reaching content keys.
        let dir = tempfile::tempdir().unwrap();
        let kek = generate_content_kek();
        save_kek(dir.path(), &kek, &generate_dek()).unwrap();

        assert!(load_kek(dir.path(), &generate_dek()).is_err());
    }
}
