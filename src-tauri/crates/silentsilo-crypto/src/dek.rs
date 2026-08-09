use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// 256-bit data encryption key for blob content.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterDek([u8; 32]);

impl MasterDek {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn generate_dek() -> MasterDek {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    MasterDek(key)
}

/// The key one blob's content is encrypted under. Per blob rather than the
/// vault DEK so rotation never re-encrypts content: it re-wraps a few
/// hundred bytes per file and the ciphertext stays put. A separate type
/// from `MasterDek` rather than an alias, because passing one where the
/// other belongs would compile and quietly destroy that property.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ContentKey([u8; 32]);

impl ContentKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The key that content keys are wrapped under, one per vault. It exists
/// because a record's fingerprint covers the wrapped content key inside it,
/// so that value can never be rewritten without breaking the `prev` chain;
/// wrapping content keys under a key of their own moves what rotation
/// touches out of the log entirely. Stored wrapped under the vault DEK.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ContentKek([u8; 32]);

impl ContentKek {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn generate_content_kek() -> ContentKek {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    ContentKek(key)
}

pub fn generate_content_key() -> ContentKey {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    ContentKey(key)
}
