//! At-rest encryption for `vault.db`, using the same sealed envelope as
//! everything else, keyed by the Master DEK. `vault.db` is a plaintext
//! working copy that exists only while a session is open; `vault.db.enc`
//! is the durable artifact read back on unlock. Locking removes the
//! plaintext.

use std::path::Path;

use silentsilo_crypto::{MasterDek, seal, unseal};
use zeroize::Zeroizing;

use crate::error::VaultError;

/// Encrypt `plain_path` under the vault DEK, writing the result to `enc_path`.
/// Written via a temp file + rename so a crash mid-write can't corrupt the
/// previously-encrypted snapshot.
///
/// The envelope comes from `silentsilo_crypto::seal`, so this file carries
/// the same magic and version byte as an operation record: one format to
/// version, one place to change the algorithm.
pub fn encrypt_vault_file(
    plain_path: &Path,
    enc_path: &Path,
    dek: &MasterDek,
) -> Result<(), VaultError> {
    let plaintext = std::fs::read(plain_path)?;
    let out = seal(&plaintext, dek).map_err(|e| VaultError::Crypto(e.to_string()))?;

    // Written to a sibling and renamed, synced first: this file is what
    // unlock reads after a clean lock, and renamed-but-unwritten survives a
    // power cut as an empty snapshot. A scanner holding the old snapshot
    // open cannot fail the save; see `silentsilo_core::durable`.
    silentsilo_core::write_replacing(enc_path, &out, |at| std::fs::File::create(at))?;
    Ok(())
}

/// Decrypt `enc_path` (written by [`encrypt_vault_file`]) into `plain_path`.
pub fn decrypt_vault_file(
    enc_path: &Path,
    plain_path: &Path,
    dek: &MasterDek,
) -> Result<(), VaultError> {
    let data = std::fs::read(enc_path)?;
    // The whole index in the clear: every folder and file name in the silo.
    // It is written to the working copy either way, but holding it in a
    // buffer that is wiped on the way out keeps it from outliving the call in
    // freed memory.
    let plaintext = Zeroizing::new(
        unseal(&data, dek).map_err(|e| VaultError::Corrupted(format!("vault snapshot: {e}")))?,
    );

    std::fs::write(plain_path, &*plaintext)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use silentsilo_crypto::generate_dek;
    use tempfile::tempdir;

    #[test]
    fn roundtrip() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("vault.db");
        let enc = dir.path().join("vault.db.enc");
        let out = dir.path().join("vault.db.restored");

        std::fs::write(&plain, b"pretend sqlite bytes").unwrap();
        let dek = generate_dek();

        encrypt_vault_file(&plain, &enc, &dek).unwrap();
        assert_ne!(std::fs::read(&enc).unwrap(), b"pretend sqlite bytes");

        decrypt_vault_file(&enc, &out, &dek).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"pretend sqlite bytes");
    }

    #[test]
    fn a_write_that_never_finished_leaves_the_last_good_snapshot_alone() {
        // The reason for the temp file and rename. A crash, a kill or a power
        // cut during the write must not be able to leave a half-encrypted
        // snapshot where the vault used to be.
        let dir = tempdir().unwrap();
        let plain = dir.path().join("vault.db");
        let enc = dir.path().join("vault.db.enc");
        let out = dir.path().join("vault.db.restored");
        let dek = generate_dek();

        std::fs::write(&plain, b"the good snapshot").unwrap();
        encrypt_vault_file(&plain, &enc, &dek).unwrap();

        // What a killed process leaves behind: a partial temp file next to an
        // intact snapshot, under the name `write_replacing` uses.
        let mut temp = enc.as_os_str().to_os_string();
        temp.push(".tmp");
        std::fs::write(std::path::PathBuf::from(temp), b"half-written junk").unwrap();

        decrypt_vault_file(&enc, &out, &dek).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"the good snapshot");
    }

    #[test]
    fn a_snapshot_from_an_unknown_format_says_so() {
        let dir = tempdir().unwrap();
        let enc = dir.path().join("vault.db.enc");
        let out = dir.path().join("vault.db.restored");

        // A valid envelope, relabelled as a version this build does not know.
        let plain = dir.path().join("vault.db");
        std::fs::write(&plain, b"content").unwrap();
        let dek = generate_dek();
        encrypt_vault_file(&plain, &enc, &dek).unwrap();
        let mut bytes = std::fs::read(&enc).unwrap();
        bytes[4] = 99;
        std::fs::write(&enc, &bytes).unwrap();

        let err = decrypt_vault_file(&enc, &out, &dek).unwrap_err();
        assert!(matches!(err, VaultError::Corrupted(_)), "got {err:?}");
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("vault.db");
        let enc = dir.path().join("vault.db.enc");
        let out = dir.path().join("vault.db.restored");

        std::fs::write(&plain, b"secret metadata").unwrap();
        encrypt_vault_file(&plain, &enc, &generate_dek()).unwrap();

        let err = decrypt_vault_file(&enc, &out, &generate_dek()).unwrap_err();
        assert!(matches!(err, VaultError::Corrupted(_)));
    }
}
