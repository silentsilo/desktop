//! Staging a vault key rotation so a crash in the middle cannot lose a
//! silo. The order is forced: re-sealing storage before the new key is
//! durable on disk risks every object sealed under a key that existed only
//! in memory. So: stage the new key and re-wrapped KEK under `.next`
//! names, re-seal storage (interruptible, re-runnable), then commit in one
//! step. A crash before commit leaves the silo opening under the old key
//! and the resume path takes it forward; going backwards is not always
//! possible, so it is never attempted.

use std::path::{Path, PathBuf};

use silentsilo_crypto::{ContentKek, MasterDek, seal, unseal};
use zeroize::Zeroizing;

use crate::dek_store::dek_path;
use crate::error::VaultError;
use crate::kek_store::{kek_path, wrap_kek_bytes};

/// Suffix for a key that is written but not yet in force.
const STAGED: &str = ".next";

fn staged(path: PathBuf) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(STAGED);
    PathBuf::from(name)
}

pub fn staged_dek_path(root: &Path) -> PathBuf {
    staged(dek_path(root))
}

pub fn staged_kek_path(root: &Path) -> PathBuf {
    staged(kek_path(root))
}

/// Whether a rotation was started and never finished.
///
/// Checked on unlock. The answer being yes is not an error: it means the last
/// attempt stopped somewhere, and the work in front of it is well defined.
pub fn rotation_pending(root: &Path) -> bool {
    staged_dek_path(root).exists()
}

/// Writes the new key and the re-wrapped KEK without putting either in
/// force. KEK first, DEK second, because the DEK's presence is what marks
/// a rotation as pending. The new key is wrapped under the **old** one
/// rather than a security key, so an interrupted rotation is resumable
/// from any credential the silo has.
pub fn stage_rotation(
    root: &Path,
    new_dek: &MasterDek,
    kek: &ContentKek,
    old_dek: &MasterDek,
) -> Result<(), VaultError> {
    crate::workdir::write_private(&staged_kek_path(root), &wrap_kek_bytes(kek, new_dek)?)?;
    let wrapped =
        seal(new_dek.as_bytes(), old_dek).map_err(|e| VaultError::Crypto(e.to_string()))?;
    crate::workdir::write_private(&staged_dek_path(root), &wrapped)?;
    Ok(())
}

/// The staged key, for a rotation that has to be carried on.
///
/// Read with the key the silo currently opens under, which is the old one:
/// committing has not happened, or there would be nothing staged.
pub fn load_staged_dek(root: &Path, old_dek: &MasterDek) -> Result<MasterDek, VaultError> {
    let data = std::fs::read(staged_dek_path(root))?;
    let plain = Zeroizing::new(unseal(&data, old_dek).map_err(|_| VaultError::InvalidCredentials)?);
    let key: [u8; 32] = plain
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::InvalidCredentials)?;
    Ok(MasterDek::from_bytes(key))
}

/// Puts the staged KEK in force and clears the pending marker. The new
/// key's own envelopes live in `keys/fido.json`, written by the caller
/// right after this. Rotation does not touch `master.dek.enc`: once a
/// security key is enrolled, unlock reads each key's envelope from
/// `keys/fido.json` and that file is never consulted.
pub fn commit_rotation(root: &Path) -> Result<(), VaultError> {
    std::fs::rename(staged_kek_path(root), kek_path(root))?;
    // Last, because its absence is what says the rotation is over. Removing
    // it before the KEK above is in place would report a finished rotation
    // over a half-applied one.
    std::fs::remove_file(staged_dek_path(root))?;
    Ok(())
}

/// Throws a staged rotation away.
///
/// Only ever correct before storage has been touched. Once an object has been
/// re-sealed, the staged key is the only thing that can open it, and
/// discarding it is the loss this whole module exists to prevent.
pub fn discard_staged(root: &Path) {
    let _ = std::fs::remove_file(staged_dek_path(root));
    let _ = std::fs::remove_file(staged_kek_path(root));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dek_store::{load_dek, save_dek};
    use crate::kek_store::{load_kek, save_kek};
    use silentsilo_crypto::{generate_content_kek, generate_dek};

    /// A silo as it stands before a rotation: a key, a KEK under it.
    fn silo(dir: &Path, wrap_key: &[u8; 32]) -> (MasterDek, ContentKek) {
        let dek = generate_dek();
        let kek = generate_content_kek();
        save_dek(dir, &dek, wrap_key).unwrap();
        save_kek(dir, &kek, &dek).unwrap();
        (dek, kek)
    }

    #[test]
    fn staging_leaves_the_silo_opening_under_the_old_key() {
        // The whole point of staging: until it is committed, nothing has
        // changed for anyone opening the silo.
        let dir = tempfile::tempdir().unwrap();
        let wrap_key = [3u8; 32];
        let (old, kek) = silo(dir.path(), &wrap_key);

        stage_rotation(dir.path(), &generate_dek(), &kek, &old).unwrap();

        assert!(rotation_pending(dir.path()));
        let opened = load_dek(dir.path(), &wrap_key).unwrap();
        assert_eq!(opened.as_bytes(), old.as_bytes());
        assert_eq!(
            load_kek(dir.path(), &old).unwrap().as_bytes(),
            kek.as_bytes()
        );
    }

    #[test]
    fn committing_puts_the_new_key_in_force_and_keeps_the_kek_readable() {
        let dir = tempfile::tempdir().unwrap();
        let wrap_key = [3u8; 32];
        let (old, kek) = silo(dir.path(), &wrap_key);
        let new = generate_dek();

        stage_rotation(dir.path(), &new, &kek, &old).unwrap();
        commit_rotation(dir.path()).unwrap();

        assert!(!rotation_pending(dir.path()), "nothing is left staged");
        // The KEK is the same key, reachable under the new DEK and not the
        // old. That is what leaves every content key working while closing
        // them to whoever holds only the old one, and it is what commit puts
        // in force: the new DEK's own envelopes are the caller's to write.
        assert_eq!(
            load_kek(dir.path(), &new).unwrap().as_bytes(),
            kek.as_bytes()
        );
        assert!(load_kek(dir.path(), &old).is_err());
    }

    #[test]
    fn the_staged_key_survives_to_be_carried_on() {
        // What the resume path reads after a crash: the key storage was
        // being re-sealed under, still openable with the device's wrap key.
        let dir = tempfile::tempdir().unwrap();
        let wrap_key = [3u8; 32];
        let (old, kek) = silo(dir.path(), &wrap_key);
        let new = generate_dek();

        stage_rotation(dir.path(), &new, &kek, &old).unwrap();

        assert_eq!(
            load_staged_dek(dir.path(), &old).unwrap().as_bytes(),
            new.as_bytes()
        );
    }

    #[test]
    fn discarding_leaves_the_silo_exactly_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let wrap_key = [3u8; 32];
        let (old, kek) = silo(dir.path(), &wrap_key);

        stage_rotation(dir.path(), &generate_dek(), &kek, &old).unwrap();
        discard_staged(dir.path());

        assert!(!rotation_pending(dir.path()));
        assert_eq!(
            load_dek(dir.path(), &wrap_key).unwrap().as_bytes(),
            old.as_bytes()
        );
        assert_eq!(
            load_kek(dir.path(), &old).unwrap().as_bytes(),
            kek.as_bytes()
        );
    }

    /// Resuming has to work from any key the silo has, not only the one that
    /// started the rotation. Wrapping the staged key under the old vault key
    /// is what buys that: every successful unlock produces it.
    #[test]
    fn the_staged_key_reads_back_from_any_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let (old, kek) = silo(dir.path(), &[3u8; 32]);
        let new = generate_dek();

        stage_rotation(dir.path(), &new, &kek, &old).unwrap();

        // A second credential unlocks the same silo and so holds the same
        // old key, with no knowledge of how the rotation was started.
        assert_eq!(
            load_staged_dek(dir.path(), &old).unwrap().as_bytes(),
            new.as_bytes()
        );
        assert!(
            load_staged_dek(dir.path(), &generate_dek()).is_err(),
            "and a key that does not open this silo reads nothing"
        );
    }

    #[test]
    fn staging_twice_replaces_the_first_attempt() {
        // An abandoned rotation followed by a fresh one. The second attempt's
        // key is the one that commits, or the first attempt's storage work
        // would be stranded under a key nothing puts in force.
        let dir = tempfile::tempdir().unwrap();
        let wrap_key = [3u8; 32];
        let (old, kek) = silo(dir.path(), &wrap_key);

        stage_rotation(dir.path(), &generate_dek(), &kek, &old).unwrap();
        let second = generate_dek();
        stage_rotation(dir.path(), &second, &kek, &old).unwrap();
        commit_rotation(dir.path()).unwrap();

        // The KEK in force is the one the second attempt staged, readable
        // under the second key and not the first.
        assert_eq!(
            load_kek(dir.path(), &second).unwrap().as_bytes(),
            kek.as_bytes()
        );
    }
}
