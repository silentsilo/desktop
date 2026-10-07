//! The agent's whole view of a silo: SSH-key entries with "Use with the SSH
//! agent" on, and of each its name, its public key and, for a signature it
//! was asked for, its private key.
//!
//! Read from the vault for every request through `list_passwords`, the one
//! call this module makes there, like the browser's `logins.rs`. A private
//! key is parsed only to sign or to name its public half, and is wiped when
//! dropped (`ssh-key` zeroizes its key material). Nothing is kept between
//! requests, so a locked silo leaves nothing of a key in memory.

use signature::Signer;
use silentsilo_vault::VaultSession;
use silentsilo_vfs::Vfs;
use ssh_key::private::{KeypairData, RsaKeypair};
use ssh_key::{HashAlg, PrivateKey, Signature};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use super::proto::{RSA_SHA2_256, RSA_SHA2_512};

/// One key the agent may offer.
pub struct Offered {
    pub entry: Uuid,
    /// The entry's name: the comment `ssh-add -l` shows.
    pub label: String,
    /// The public key as the wire carries it.
    pub blob: Vec<u8>,
    /// "Require a touch to reveal": a signature asks for it too.
    pub require_reauth: bool,
}

#[derive(serde::Deserialize, Default, Zeroize)]
#[serde(default)]
struct Row {
    id: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    service: String,
    ssh_private_key: String,
    ssh_agent: bool,
    require_reauth: bool,
}

impl Row {
    fn offered(&self) -> bool {
        self.kind.as_deref() == Some("ssh_key")
            && self.ssh_agent
            && !self.ssh_private_key.trim().is_empty()
    }
}

/// Every SSH-key entry the agent may use, parsed, each wiped when dropped.
fn rows(session: &VaultSession) -> Result<Vec<Zeroizing<Row>>, String> {
    let mut raw = Vfs::new(session)
        .list_passwords()
        .map_err(|e| e.to_string())?;
    let rows = raw
        .iter()
        .filter_map(|json| serde_json::from_str::<Row>(json).ok())
        .filter(|row| row.offered())
        .map(Zeroizing::new)
        .collect();
    for json in &mut raw {
        json.zeroize();
    }
    Ok(rows)
}

/// Why a key's text cannot be used by the agent.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    /// It has a passphrase: the editor asks for it once and stores the key
    /// without it.
    Encrypted,
    /// Not a key format the agent reads.
    Unreadable,
    /// A kind it reads but does not sign with: a security-key (`sk-`) key,
    /// DSA, or ECDSA P-521.
    Unsupported,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KeyError::Encrypted => "This key has a passphrase.",
            KeyError::Unreadable => {
                "This key is not in a format the SSH agent reads: OpenSSH, or an RSA key in PEM."
            }
            KeyError::Unsupported => {
                "The SSH agent signs with Ed25519, ECDSA P-256 or P-384, and RSA keys only."
            }
        })
    }
}

/// A private key from an entry's text: OpenSSH's format, or an RSA key in
/// PKCS#1 or PKCS#8 PEM. `ssh-keygen -p -f <file>` turns any other into
/// OpenSSH's.
pub fn private_key(text: &str) -> Result<PrivateKey, KeyError> {
    let text = text.trim();
    if text.contains("BEGIN OPENSSH PRIVATE KEY") {
        let key = PrivateKey::from_openssh(text).map_err(|_| KeyError::Unreadable)?;
        if key.is_encrypted() {
            return Err(KeyError::Encrypted);
        }
        // Offered only if it can sign: a key that cannot would be confirmed
        // in the dialog and logged, then fail.
        let signs = match key.key_data() {
            KeypairData::Ed25519(_) | KeypairData::Rsa(_) => true,
            KeypairData::Ecdsa(ecdsa) => matches!(
                ecdsa.curve(),
                ssh_key::EcdsaCurve::NistP256 | ssh_key::EcdsaCurve::NistP384
            ),
            _ => false,
        };
        if !signs {
            return Err(KeyError::Unsupported);
        }
        return Ok(key);
    }
    if text.contains("ENCRYPTED") {
        return Err(KeyError::Encrypted);
    }
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    let rsa = rsa::RsaPrivateKey::from_pkcs1_pem(text)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs8_pem(text))
        .map_err(|_| KeyError::Unreadable)?;
    let keypair = RsaKeypair::try_from(rsa).map_err(|_| KeyError::Unreadable)?;
    PrivateKey::new(KeypairData::Rsa(keypair), "").map_err(|_| KeyError::Unreadable)
}

/// A private key with its passphrase taken off, in OpenSSH's format, for
/// the editor to store. The passphrase is never kept.
pub fn without_passphrase(text: &str, passphrase: &str) -> Result<Zeroizing<String>, String> {
    let key = PrivateKey::from_openssh(text.trim())
        .map_err(|_| "This is not a key in OpenSSH's format.".to_string())?;
    let open = key
        .decrypt(passphrase)
        .map_err(|_| "That passphrase does not open this key.".to_string())?;
    let pem = open
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| e.to_string())?;
    Ok(Zeroizing::new(pem.to_string()))
}

/// The keys to offer, in the entries' order by name. One that cannot be
/// read is left out rather than failing the list.
pub fn offered(session: &VaultSession) -> Result<Vec<Offered>, String> {
    let mut keys: Vec<Offered> = rows(session)?
        .iter_mut()
        .filter_map(|row| {
            let key = private_key(&row.ssh_private_key).ok()?;
            Some(Offered {
                entry: Uuid::parse_str(&row.id).ok()?,
                label: std::mem::take(&mut row.service),
                blob: key.public_key().to_bytes().ok()?,
                require_reauth: row.require_reauth,
            })
        })
        .collect();
    keys.sort_by_key(|k| k.label.to_lowercase());
    Ok(keys)
}

/// Signs `data` with the offered key whose public blob is `blob`, as the
/// request's flags ask. The signature blob, or `None` when no offered key
/// has that blob. An RSA key without a SHA-2 flag is refused: SHA-1
/// signatures are not made.
pub fn sign(
    session: &VaultSession,
    blob: &[u8],
    data: &[u8],
    flags: u32,
) -> Result<Option<Vec<u8>>, String> {
    for row in rows(session)?.iter() {
        let Ok(key) = private_key(&row.ssh_private_key) else {
            continue;
        };
        if key.public_key().to_bytes().ok().as_deref() != Some(blob) {
            continue;
        }
        return sign_with(&key, data, flags).map(Some);
    }
    Ok(None)
}

pub fn sign_with(key: &PrivateKey, data: &[u8], flags: u32) -> Result<Vec<u8>, String> {
    let signature: Signature = match key.key_data() {
        KeypairData::Rsa(rsa) => {
            let hash = if flags & RSA_SHA2_512 != 0 {
                HashAlg::Sha512
            } else if flags & RSA_SHA2_256 != 0 {
                HashAlg::Sha256
            } else {
                return Err("SHA-1 RSA signatures are not made.".into());
            };
            (rsa, Some(hash)).try_sign(data)
        }
        other => other.try_sign(data),
    }
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    ssh_key::encoding::Encode::encode(&signature, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_agent::test_keys::{ED25519_WITH_PASSPHRASE, RSA_PKCS1, armour};
    use signature::Verifier;
    use ssh_key::private::Ed25519Keypair;

    fn ed25519() -> PrivateKey {
        PrivateKey::new(
            KeypairData::Ed25519(Ed25519Keypair::from_seed(&[9; 32])),
            "",
        )
        .unwrap()
    }

    fn verify(key: &PrivateKey, data: &[u8], blob: &[u8]) -> Signature {
        let signature = Signature::try_from(blob).unwrap();
        Verifier::verify(key.public_key(), data, &signature).unwrap();
        signature
    }

    #[test]
    fn an_openssh_key_signs_and_its_signature_verifies() {
        let key = ed25519();
        let text = key.to_openssh(ssh_key::LineEnding::LF).unwrap();
        let read = private_key(&text).unwrap();
        let blob = sign_with(&read, b"data", 0).unwrap();
        verify(&key, b"data", &blob);
    }

    #[test]
    fn rsa_signs_with_the_sha2_the_client_asked_for_and_never_sha1() {
        let key = private_key(&armour("RSA", RSA_PKCS1)).unwrap();
        for (flag, name) in [
            (RSA_SHA2_256, "rsa-sha2-256"),
            (RSA_SHA2_512, "rsa-sha2-512"),
        ] {
            let blob = sign_with(&key, b"data", flag).unwrap();
            assert_eq!(verify(&key, b"data", &blob).algorithm().as_str(), name);
        }
        assert!(sign_with(&key, b"data", 0).is_err());
    }

    #[test]
    fn a_key_with_a_passphrase_is_named_as_such_and_opened_once() {
        let locked = armour("OPENSSH", ED25519_WITH_PASSPHRASE);
        assert_eq!(private_key(&locked).err(), Some(KeyError::Encrypted));
        assert!(without_passphrase(&locked, "wrong").is_err());
        let open = without_passphrase(&locked, "hunter2").unwrap();
        let key = private_key(&open).unwrap();
        assert_eq!(
            key.public_key().key_data(),
            PrivateKey::from_openssh(&locked)
                .unwrap()
                .public_key()
                .key_data()
        );
        verify(&key, b"data", &sign_with(&key, b"data", 0).unwrap());
        assert_eq!(
            private_key("-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n").err(),
            Some(KeyError::Encrypted)
        );
        assert_eq!(private_key("not a key").err(), Some(KeyError::Unreadable));
    }

    #[test]
    fn only_entries_turned_on_for_the_agent_are_offered() {
        let row = |kind: &str, agent: bool, key: &str| Row {
            kind: Some(kind.into()),
            ssh_agent: agent,
            ssh_private_key: key.into(),
            ..Row::default()
        };
        assert!(row("ssh_key", true, "k").offered());
        assert!(!row("ssh_key", false, "k").offered());
        assert!(!row("ssh_key", true, " ").offered());
        assert!(!row("login", true, "k").offered());
    }
}
