use thiserror::Error;

#[derive(Debug, Error)]
pub enum FidoError {
    #[error("FIDO2 hardware not available on this platform")]
    NotAvailable,

    #[error(
        "no FIDO2 security key detected: insert your key; Windows will prompt when you enroll or unlock"
    )]
    NoDevice,

    #[error("enrollment failed: {0}")]
    EnrollmentFailed(String),

    #[error("unlock failed: {0}")]
    UnlockFailed(String),

    #[error("check-in signature failed: {0}")]
    CheckInFailed(String),
}
