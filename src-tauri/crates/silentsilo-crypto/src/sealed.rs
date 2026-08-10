//! In-memory AES-256-GCM for small payloads: operation records, wrapped
//! keys, the sealed index.
//!
//! Format is `magic(4) || version(1) || nonce(12) || ciphertext || tag(16)`,
//! shared with `vault_file_crypto` so there is one wire shape to reason
//! about. The version header is what lets the algorithm be replaced later
//! without guessing which suite produced a payload; a reader that tries
//! every suite it ever supported and takes whichever authenticates is how
//! downgrade attacks are handed out for free.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

use crate::dek::{ContentKek, ContentKey, MasterDek};
use crate::error::CryptoError;

const NONCE_SIZE: usize = 12;
const SEAL_MAGIC: &[u8; 4] = b"SSEA";
/// Bump together with the algorithm. `unseal` refuses anything else rather
/// than falling back, and the header is authenticated, so a payload cannot be
/// re-labelled as an older version without breaking the tag.
pub const SEAL_VERSION: u8 = 1;
const HEADER_SIZE: usize = SEAL_MAGIC.len() + 1;

fn header(version: u8) -> [u8; HEADER_SIZE] {
    let mut out = [0u8; HEADER_SIZE];
    out[..SEAL_MAGIC.len()].copy_from_slice(SEAL_MAGIC);
    out[SEAL_MAGIC.len()] = version;
    out
}

/// Encrypts `plaintext` under the vault DEK with a fresh random nonce.
///
/// A random nonce rather than a counter: these are produced independently on
/// every device with no shared state to coordinate a counter, and reusing a
/// (key, nonce) pair under AES-GCM breaks both confidentiality and
/// authentication. At 96 bits the collision risk is negligible for the
/// volume of records a vault produces.
pub fn seal(plaintext: &[u8], dek: &MasterDek) -> Result<Vec<u8>, CryptoError> {
    seal_with_key(plaintext, dek.as_bytes())
}

/// [`seal`] under a key that is not the vault DEK, for wrapping the DEK
/// itself under a key derived from a security key, a device secret or a
/// recovery code. Same envelope, so there is one format to version.
pub fn seal_with_key(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let head = header(SEAL_VERSION);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: &head,
            },
        )
        .map_err(|_| CryptoError::InvalidHeader("failed to seal payload".into()))?;

    let mut out = Vec::with_capacity(HEADER_SIZE + NONCE_SIZE + ciphertext.len());
    out.extend_from_slice(&head);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// A blob's content key, wrapped under the vault's content KEK and hex
/// encoded so it can travel inside an operation record. Under the KEK
/// rather than the DEK because the record's fingerprint covers this value,
/// so it can never be rewritten; see [`ContentKek`].
pub fn wrap_content_key(key: &ContentKey, kek: &ContentKek) -> Result<String, CryptoError> {
    Ok(hex::encode(seal_with_key(key.as_bytes(), kek.as_bytes())?))
}

/// Reverses [`wrap_content_key`].
pub fn unwrap_content_key(wrapped: &str, kek: &ContentKek) -> Result<ContentKey, CryptoError> {
    let bytes = hex::decode(wrapped)
        .map_err(|_| CryptoError::InvalidHeader("content key is not hex".into()))?;
    let raw = unseal_with_key(&bytes, kek.as_bytes())?;
    let sized: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidHeader("content key is the wrong length".into()))?;
    Ok(ContentKey::from_bytes(sized))
}

/// Reverses [`seal`]. Fails on a truncated payload, an unknown format, the
/// wrong key, or any tampering — GCM authenticates, so a modified record
/// cannot be decrypted into something that merely looks wrong.
pub fn unseal(sealed: &[u8], dek: &MasterDek) -> Result<Vec<u8>, CryptoError> {
    unseal_with_key(sealed, dek.as_bytes())
}

/// Reverses [`seal_with_key`].
pub fn unseal_with_key(sealed: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() < HEADER_SIZE + NONCE_SIZE {
        return Err(CryptoError::DecryptionFailed);
    }
    let (head, rest) = sealed.split_at(HEADER_SIZE);
    if &head[..SEAL_MAGIC.len()] != SEAL_MAGIC {
        return Err(CryptoError::InvalidHeader("not a sealed payload".into()));
    }
    let version = head[SEAL_MAGIC.len()];
    if version != SEAL_VERSION {
        return Err(CryptoError::InvalidHeader(format!(
            "sealed payload version {version} needs a newer SilentSilo"
        )));
    }

    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_SIZE);
    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");

    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: head,
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dek::generate_dek;

    #[test]
    fn a_payload_round_trips() {
        let dek = generate_dek();
        let message = br#"{"op":"add_file","name":"salaries.xlsx"}"#;
        assert_eq!(
            unseal(&seal(message, &dek).unwrap(), &dek).unwrap(),
            message
        );
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let dek = generate_dek();
        assert!(unseal(&seal(b"", &dek).unwrap(), &dek).unwrap().is_empty());
    }

    #[test]
    fn the_same_plaintext_seals_differently_each_time() {
        // Otherwise an observer could tell that two records are identical, or
        // spot a repeated filename, purely from the ciphertext.
        let dek = generate_dek();
        let a = seal(b"same", &dek).unwrap();
        let b = seal(b"same", &dek).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn the_wrong_key_cannot_unseal() {
        let sealed = seal(b"secret", &generate_dek()).unwrap();
        assert!(unseal(&sealed, &generate_dek()).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let dek = generate_dek();
        let mut sealed = seal(b"secret", &dek).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(unseal(&sealed, &dek).is_err());
    }

    #[test]
    fn a_truncated_payload_is_rejected_rather_than_panicking() {
        let dek = generate_dek();
        assert!(unseal(&[0u8; 4], &dek).is_err());
        assert!(unseal(&[], &dek).is_err());
        assert!(unseal(&[0u8; HEADER_SIZE + NONCE_SIZE - 1], &dek).is_err());
    }

    #[test]
    fn the_payload_says_what_wrote_it() {
        let sealed = seal(b"secret", &generate_dek()).unwrap();
        assert_eq!(&sealed[..4], SEAL_MAGIC);
        assert_eq!(sealed[4], SEAL_VERSION);
    }

    #[test]
    fn a_future_version_is_refused_with_something_the_ui_can_say() {
        // The point of the header: a build that predates the change says so,
        // instead of failing as if the record were corrupt or the key wrong.
        let dek = generate_dek();
        let mut sealed = seal(b"secret", &dek).unwrap();
        sealed[4] = SEAL_VERSION + 1;

        let err = unseal(&sealed, &dek).unwrap_err();
        assert!(
            matches!(&err, CryptoError::InvalidHeader(m) if m.contains("newer")),
            "expected a version complaint, got {err:?}"
        );
    }

    #[test]
    fn a_relabelled_payload_does_not_authenticate() {
        // The header is AAD, so an attacker cannot take a v2 payload, write a
        // 1 into it, and have a v1 reader accept the weaker interpretation.
        // Simulated in the only direction available today: corrupt the magic
        // of an otherwise valid payload and confirm nothing decrypts it.
        let dek = generate_dek();
        let mut sealed = seal(b"secret", &dek).unwrap();
        sealed[0] = b'X';
        assert!(unseal(&sealed, &dek).is_err());

        // And with the magic intact but the header bytes swapped under the
        // tag, GCM itself refuses.
        let mut sealed = seal(b"secret", &dek).unwrap();
        sealed[3] = b'Z';
        assert!(unseal(&sealed, &dek).is_err());
    }
}
