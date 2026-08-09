//! AES-256-GCM streaming encryption with SSLO blob format.

mod blob;
mod dek;
mod error;
mod sealed;

pub use blob::{
    BlobHeader, CHUNK_SIZE, HEADER_SIZE, SSLO_MAGIC, SSLO_VERSION, decrypt_blob, encrypt_file,
    verify_blob,
};
pub use dek::{
    ContentKek, ContentKey, MasterDek, generate_content_kek, generate_content_key, generate_dek,
};
pub use error::CryptoError;
pub use sealed::{
    SEAL_VERSION, seal, seal_with_key, unseal, unseal_with_key, unwrap_content_key,
    wrap_content_key,
};
