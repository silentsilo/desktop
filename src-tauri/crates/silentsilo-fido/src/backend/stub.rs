use crate::FidoError;
use crate::types::{Authenticator, Enrollment, EnrollmentChallenge, UnlockMaterial};

pub(crate) fn fido_key_present() -> bool {
    false
}

pub(crate) fn fido_interface_accessible() -> bool {
    false
}

pub fn probe_device() -> Result<(), FidoError> {
    Err(FidoError::NotAvailable)
}

pub fn begin_enrollment(
    _vault_id: &str,
    _key_slot: u8,
    _authenticator: Authenticator,
) -> Result<EnrollmentChallenge, FidoError> {
    Err(FidoError::NotAvailable)
}

pub(crate) fn platform_authenticator_available() -> bool {
    false
}

pub(crate) fn wait_for_ceremony_teardown(_timeout_ms: u64) {}

pub fn complete_enrollment(_challenge: &EnrollmentChallenge) -> Result<Enrollment, FidoError> {
    Err(FidoError::NotAvailable)
}

pub fn derive_unlock_material(
    _credential_ids: &[Vec<u8>],
    _vault_id: &str,
    _on: Option<Authenticator>,
) -> Result<UnlockMaterial, FidoError> {
    Err(FidoError::NotAvailable)
}
