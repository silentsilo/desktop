use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};

use crate::error::VaultError;

pub const ARGON2ID: &str = "argon2id";

/// How a wrap key was derived, stored next to whatever it wrapped. Written
/// into every envelope rather than read from constants, so retuning the
/// parameters never turns into "your recovery code no longer works".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// For a secret this machine generated and stores: 160 bits of OS
    /// randomness, so the KDF is defence in depth rather than the thing
    /// standing between an attacker and the vault.
    pub fn device_secret() -> Self {
        Self {
            algorithm: ARGON2ID.into(),
            m_cost: 19_456,
            t_cost: 2,
            p_cost: 1,
        }
    }

    /// Deliberately heavier, for the recovery envelope.
    ///
    /// Not because the code needs it (160 bits is unbreakable at any cost)
    /// but because that envelope is published to the bucket, and the cost of
    /// being wrong about that assumption should not be zero. 64 MiB is still
    /// under a second on any machine that runs this app.
    pub fn recovery_code() -> Self {
        Self {
            algorithm: ARGON2ID.into(),
            m_cost: 65_536,
            t_cost: 3,
            p_cost: 1,
        }
    }

    /// The most work an envelope is allowed to ask for. The parameters are
    /// chosen by whoever wrote the file, and `recovery.env` comes from
    /// storage the threat model does not trust: `m_cost` is in KiB, so a
    /// hostile `u32::MAX` asks Argon2 for four terabytes. A gigabyte is
    /// sixteen times the heaviest thing this app writes.
    const MAX_M_COST: u32 = 1024 * 1024;
    const MAX_T_COST: u32 = 64;
    const MAX_P_COST: u32 = 16;

    /// Derives a 32-byte wrap key, refusing an algorithm this build does not
    /// implement rather than silently substituting the one it does.
    pub fn derive(&self, secret: &[u8], salt: &[u8]) -> Result<[u8; 32], VaultError> {
        if self.algorithm != ARGON2ID {
            return Err(VaultError::Corrupted(format!(
                "key derivation '{}' needs a newer SilentSilo",
                self.algorithm
            )));
        }
        if self.m_cost > Self::MAX_M_COST
            || self.t_cost > Self::MAX_T_COST
            || self.p_cost > Self::MAX_P_COST
        {
            return Err(VaultError::Corrupted(format!(
                "key derivation asks for more work than any SilentSilo writes \
                 (m={}, t={}, p={}): refusing rather than trying it",
                self.m_cost, self.t_cost, self.p_cost
            )));
        }
        let params = Params::new(self.m_cost, self.t_cost, self.p_cost, Some(32))
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(secret, salt, &mut key)
            .map_err(|e| VaultError::Crypto(e.to_string()))?;
        Ok(key)
    }
}

/// A derived key, wiped when it goes out of scope.
///
/// It unwraps `master.dek.enc`, so it is worth exactly as much as the DEK for
/// as long as it exists. Deriving it costs an Argon2 pass, which means it is
/// held briefly and then dropped, and the wipe is what keeps the window that
/// short in memory as well as in code.
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct VaultKey {
    pub wrap_key: [u8; 32],
}

/// Derive local vault keys from a high-entropy secret (the device secret,
/// until a security key is enrolled).
pub fn derive_vault_key(secret: &str, salt: &[u8; 16]) -> Result<VaultKey, VaultError> {
    Ok(VaultKey {
        wrap_key: KdfParams::device_secret().derive(secret.as_bytes(), salt)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parameters_this_app_writes_are_accepted() {
        // The ceiling is worth nothing if it refuses our own envelopes.
        let salt = [7u8; 16];
        assert!(KdfParams::device_secret().derive(b"secret", &salt).is_ok());
        assert!(KdfParams::recovery_code().derive(b"secret", &salt).is_ok());
    }

    #[test]
    fn an_envelope_asking_for_four_terabytes_is_refused_rather_than_attempted() {
        // `recovery.env` comes from storage, and its parameters are chosen by
        // whoever wrote it. m_cost is in KiB, so this is the allocation that
        // kills the process before anything gets to reject it.
        let hostile = KdfParams {
            algorithm: ARGON2ID.into(),
            m_cost: u32::MAX,
            t_cost: 1,
            p_cost: 1,
        };
        let err = hostile.derive(b"secret", &[7u8; 16]).unwrap_err();
        assert!(
            format!("{err}").contains("more work than any SilentSilo writes"),
            "got {err}"
        );
    }

    #[test]
    fn absurd_time_and_parallelism_are_refused_too() {
        let salt = [7u8; 16];
        let slow = KdfParams {
            algorithm: ARGON2ID.into(),
            m_cost: 1024,
            t_cost: u32::MAX,
            p_cost: 1,
        };
        assert!(slow.derive(b"secret", &salt).is_err());

        let wide = KdfParams {
            algorithm: ARGON2ID.into(),
            m_cost: 1024,
            t_cost: 1,
            p_cost: u32::MAX,
        };
        assert!(wide.derive(b"secret", &salt).is_err());
    }
}
