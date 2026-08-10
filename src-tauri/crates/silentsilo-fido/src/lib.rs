//! FIDO2 security-key integration.
//!
//! - **Windows**: OS WebAuthn API (`webauthn.dll`) — no administrator rights.
//! - **Linux / macOS**: CTAP2 over USB HID.

mod backend;
// Only the hardware backends build client data; its deps are optional and
// follow the same feature.
#[cfg(feature = "hardware")]
mod client_data;
mod error;
mod types;

pub use error::FidoError;
pub use types::{
    Authenticator, CredentialInfo, Enrollment, EnrollmentChallenge, FidoStatus, UnlockMaterial,
};

const RP_ID: &str = "silentsilo.com";

/// On Windows, bind the main app window HWND so WebAuthn can show its security-key UI.
#[cfg(all(windows, feature = "hardware"))]
pub fn set_parent_hwnd(hwnd: isize) {
    backend::set_parent_hwnd(hwnd);
}

#[cfg(not(all(windows, feature = "hardware")))]
pub fn set_parent_hwnd(_hwnd: isize) {}

pub fn status() -> FidoStatus {
    #[cfg(feature = "hardware")]
    {
        FidoStatus {
            key_present: backend::fido_key_present(),
            fido_accessible: backend::fido_interface_accessible(),
        }
    }
    #[cfg(not(feature = "hardware"))]
    {
        FidoStatus {
            key_present: false,
            fido_accessible: false,
        }
    }
}

pub fn is_available() -> bool {
    status().fido_accessible
}

pub fn require_fido_ready() -> Result<(), FidoError> {
    if !is_available() {
        return Err(FidoError::NoDevice);
    }
    Ok(())
}

pub fn begin_enrollment(
    vault_id: &str,
    key_slot: u8,
    authenticator: Authenticator,
) -> Result<EnrollmentChallenge, FidoError> {
    #[cfg(feature = "hardware")]
    {
        backend::begin_enrollment(vault_id, key_slot, authenticator)
    }
    #[cfg(not(feature = "hardware"))]
    {
        let _ = (vault_id, key_slot, authenticator);
        Err(FidoError::NotAvailable)
    }
}

/// Whether this machine has a built-in authenticator that can wrap the DEK.
///
/// Presence is not enough — it has to support the `hmac-secret`/PRF
/// extension, which is what produces the wrap key. Offering the option on a
/// machine that would fail halfway through the ceremony is worse than not
/// offering it.
pub fn platform_authenticator_available() -> bool {
    #[cfg(feature = "hardware")]
    {
        backend::platform_authenticator_available()
    }
    #[cfg(not(feature = "hardware"))]
    {
        false
    }
}

/// Blocks until the platform is ready to be asked for another ceremony.
///
/// Enrolment runs two in a row, and on Windows the second one fails inside
/// the platform's own dialog when it starts too early. Call this between
/// them, on a blocking thread. `timeout_ms` caps the wait; the backends that
/// have nothing to wait for return at once.
pub fn wait_for_ceremony_teardown(timeout_ms: u64) {
    backend::wait_for_ceremony_teardown(timeout_ms);
}

pub fn complete_enrollment(challenge: &EnrollmentChallenge) -> Result<Enrollment, FidoError> {
    #[cfg(feature = "hardware")]
    {
        backend::complete_enrollment(challenge)
    }
    #[cfg(not(feature = "hardware"))]
    {
        let _ = challenge;
        Err(FidoError::NotAvailable)
    }
}

/// `on` pins which kind of authenticator to ask for.
///
/// `None` when unlocking, where any enrolled credential will do and the
/// allow-list already narrows it to this vault. Set during enrolment, where
/// the credential was created moments ago on a known authenticator: without
/// it Windows offers the whole menu a second time, so choosing Windows Hello
/// at step one leads to a security key and a QR code at step two.
pub fn derive_unlock_material(
    credential_ids: &[Vec<u8>],
    vault_id: &str,
    on: Option<Authenticator>,
) -> Result<UnlockMaterial, FidoError> {
    #[cfg(feature = "hardware")]
    {
        backend::derive_unlock_material(credential_ids, vault_id, on)
    }
    #[cfg(not(feature = "hardware"))]
    {
        let _ = (credential_ids, vault_id, on);
        Err(FidoError::NotAvailable)
    }
}

pub fn dek_salt_for_vault(vault_id: &str) -> String {
    format!("silentsilo-dek-v1:{vault_id}")
}
