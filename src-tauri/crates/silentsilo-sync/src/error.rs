use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("storage: {0}")]
    Storage(String),

    #[error("vault: {0}")]
    Vault(String),

    #[error("encryption: {0}")]
    Crypto(String),

    /// This device is missing changes that have been compacted away, so there
    /// is nothing left in the bucket to catch up from. Its own kind of error
    /// because it is not a failure to be retried: the caller has to
    /// re-bootstrap, and the numbers are what it needs to explain why.
    #[error(
        "this device last saw change {applied_through}, and the silo has been compacted up to \
         {horizon}: the changes in between are no longer stored, so it has to be set up again \
         from the current state"
    )]
    BehindHorizon { applied_through: u64, horizon: u64 },

    /// The user pressed stop. Not a failure: the operations that accept a
    /// cancel check are safe to interrupt, and running them again either
    /// carries on or simply re-checks.
    #[error("cancelled")]
    Cancelled,
}
