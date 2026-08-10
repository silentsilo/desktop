use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("vault is locked")]
    VaultLocked,

    #[error("entry not found: {0}")]
    NotFound(String),

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("name conflict: {0}")]
    NameConflict(String),

    /// Carries its own wording because it is shown to the person who just
    /// typed the name, and "invalid path: a name cannot end in a dot" reads
    /// like a stack trace.
    #[error("{0}")]
    InvalidName(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("database error: {0}")]
    Database(String),

    /// A record written by a newer build, doing something this one cannot
    /// safely ignore. Refusing is the point: applying the rest of the log
    /// without it would present a tree that is quietly wrong.
    #[error(
        "this silo contains changes made by a newer version of SilentSilo ({0}). Update to open it."
    )]
    UnsupportedOperation(String),

    /// The caller asked for something the data cannot support: a snapshot at
    /// a horizon that would leave nothing behind it, a snapshot restored into
    /// a different vault. Carries its own wording, like [`Self::InvalidName`],
    /// because the sentence is the whole message.
    #[error("{0}")]
    Invalid(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type CoreResult<T> = Result<T, CoreError>;
