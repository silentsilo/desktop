use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault already exists")]
    AlreadyExists,

    #[error("vault not found")]
    NotFound,

    #[error("no device credentials found for this silo: enroll this device first")]
    NotProvisioned,

    #[error("invalid device credentials")]
    InvalidCredentials,

    #[error("vault id mismatch")]
    VaultIdMismatch,

    #[error("this silo is administered by an organisation, so this change needs one of its keys")]
    OrganisationKeyRequired,

    #[error(
        "a key change on this silo was started and never finished: finish that one rather than \
         starting another"
    )]
    RotationPending,

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("database corrupted: {0}")]
    Corrupted(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<VaultError> for silentsilo_core::CoreError {
    fn from(err: VaultError) -> Self {
        silentsilo_core::CoreError::Database(err.to_string())
    }
}
