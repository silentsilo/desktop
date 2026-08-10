use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FidoStatus {
    /// A security key is expected to be available (platform-specific detection).
    pub key_present: bool,
    /// FIDO2 ceremonies can run (enrollment / unlock / check-in).
    pub fido_accessible: bool,
}

/// Which kind of authenticator an enrollment targets.
///
/// Both produce a wrap key the same way — through the `hmac-secret`/PRF
/// extension — so neither is cryptographically weaker than the other. What
/// differs is where the secret lives: a security key is portable and
/// survives the machine, a platform authenticator is sealed to this
/// computer's TPM and does not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Authenticator {
    /// A removable FIDO2 key (YubiKey, Nitrokey, SoloKey…).
    #[default]
    SecurityKey,
    /// Windows Hello, Touch ID — whatever this machine has built in.
    ThisDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentChallenge {
    pub challenge: Vec<u8>,
    pub rp_id: String,
    pub user_id: String,
    pub key_slot: u8,
    #[serde(default)]
    pub authenticator: Authenticator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialInfo {
    pub credential_id: Vec<u8>,
    pub public_key: Vec<u8>,
    pub key_slot: u8,
    pub rp_id: String,
    #[serde(default)]
    pub authenticator: Authenticator,
}

/// What an enrolment ceremony produced.
///
/// `unlock` is present when the platform evaluated the PRF while creating
/// the credential, which is what Windows Hello does: the salt goes in with
/// `makeCredential` and the output comes back attached to the attestation.
/// When it is there the wrap key already exists and the second ceremony can
/// be skipped, which is the only reliable way to avoid the failure that
/// happens when a second ceremony starts too soon after the first.
///
/// `None` means the wrap key has to be derived the long way, with an
/// assertion.
pub struct Enrollment {
    pub credential: CredentialInfo,
    pub unlock: Option<UnlockMaterial>,
}

/// What a touch of the security key produced.
///
/// `wrap_key` is derived from the authenticator's `hmac-secret` output and
/// unwraps the DEK, so it is the vault for as long as it exists. Wiped on
/// drop; the credential id beside it is public and is skipped.
///
/// No `Debug`, `Serialize` or `Clone`, deliberately, the way `MasterDek`
/// carries none: a key that can be printed, written to a log or copied by
/// accident is a key that outlives the wipe. It never crosses the IPC
/// boundary, so nothing needs those.
#[derive(zeroize::ZeroizeOnDrop)]
pub struct UnlockMaterial {
    pub wrap_key: [u8; 32],
    /// Credential that produced the wrap key (hex-capable bytes).
    #[zeroize(skip)]
    pub credential_id: Vec<u8>,
}

#[cfg(test)]
mod tests {
    /// Compile-time, because freed memory cannot be read without undefined
    /// behaviour, and because losing the derive in a refactor is the failure
    /// worth catching.
    #[test]
    fn the_wrap_key_is_wiped_when_dropped() {
        fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        wiped_on_drop::<super::UnlockMaterial>();
    }
}
