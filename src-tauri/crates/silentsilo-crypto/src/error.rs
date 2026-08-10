use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid blob header: {0}")]
    InvalidHeader(String),

    #[error("decryption failed")]
    DecryptionFailed,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<CryptoError> for silentsilo_core::CoreError {
    fn from(err: CryptoError) -> Self {
        silentsilo_core::CoreError::Crypto(err.to_string())
    }
}
