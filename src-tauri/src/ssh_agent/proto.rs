//! The agent protocol, as pure functions: what a client asked, and the
//! bytes of each answer. RFC 9987 for the messages, OpenSSH's
//! `PROTOCOL.agent` for `session-bind@openssh.com`.

use signature::Verifier;
use ssh_key::{PublicKey, Signature};

pub const FAILURE: u8 = 5;
pub const SUCCESS: u8 = 6;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const EXTENSION: u8 = 27;

/// Sign request flags: an RSA key signs with SHA-256 or SHA-512.
pub const RSA_SHA2_256: u32 = 2;
pub const RSA_SHA2_512: u32 = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Identities,
    Sign {
        key: Vec<u8>,
        data: Vec<u8>,
        flags: u32,
    },
    /// The client bound this connection to a server: its host key, and
    /// whether the connection is forwarded from that server.
    SessionBind {
        host_key: Vec<u8>,
        session_id: Vec<u8>,
        signature: Vec<u8>,
        forwarding: bool,
    },
    /// Anything else: adding or removing keys, locking, smartcards,
    /// constraints, other extensions. All refused.
    Other,
}

/// Reads one wire string (a 32-bit big-endian length, then the bytes).
fn string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    if input.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes(input[..4].try_into().ok()?) as usize;
    let rest = &input[4..];
    if rest.len() < len {
        return None;
    }
    let (value, tail) = rest.split_at(len);
    *input = tail;
    Some(value)
}

fn uint32(input: &mut &[u8]) -> Option<u32> {
    if input.len() < 4 {
        return None;
    }
    let value = u32::from_be_bytes(input[..4].try_into().ok()?);
    *input = &input[4..];
    Some(value)
}

/// A message that does not parse is `Other`, and is refused like one.
pub fn parse(message: &[u8]) -> Request {
    let Some((&kind, mut body)) = message.split_first() else {
        return Request::Other;
    };
    let parsed = match kind {
        REQUEST_IDENTITIES if body.is_empty() => Some(Request::Identities),
        SIGN_REQUEST => (|| {
            let key = string(&mut body)?.to_vec();
            let data = string(&mut body)?.to_vec();
            let flags = uint32(&mut body)?;
            body.is_empty()
                .then_some(Request::Sign { key, data, flags })
        })(),
        EXTENSION => (|| {
            let name = string(&mut body)?;
            if name != b"session-bind@openssh.com" {
                return None;
            }
            let host_key = string(&mut body)?.to_vec();
            let session_id = string(&mut body)?.to_vec();
            let signature = string(&mut body)?.to_vec();
            let (&forwarding, rest) = body.split_first()?;
            (rest.is_empty() && forwarding <= 1).then_some(Request::SessionBind {
                host_key,
                session_id,
                signature,
                forwarding: forwarding == 1,
            })
        })(),
        _ => None,
    };
    parsed.unwrap_or(Request::Other)
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

pub fn failure() -> Vec<u8> {
    vec![FAILURE]
}

pub fn success() -> Vec<u8> {
    vec![SUCCESS]
}

/// The keys offered: each public key blob with its comment, here the
/// entry's name.
pub fn identities_answer(keys: &[(Vec<u8>, String)]) -> Vec<u8> {
    let mut out = vec![IDENTITIES_ANSWER];
    out.extend_from_slice(&(keys.len() as u32).to_be_bytes());
    for (blob, comment) in keys {
        put_string(&mut out, blob);
        put_string(&mut out, comment.as_bytes());
    }
    out
}

pub fn sign_answer(signature_blob: &[u8]) -> Vec<u8> {
    let mut out = vec![SIGN_RESPONSE];
    put_string(&mut out, signature_blob);
    out
}

/// A server the connection was bound to, once its proof checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// `SHA256:…` of the server's host key, as `ssh` shows it.
    pub host_fingerprint: String,
    pub forwarding: bool,
}

/// Checks the server's signature over the session id with the host key it
/// presented, as OpenSSH's own agent does. A binding that does not verify
/// is refused, and the connection is not trusted with anything after it.
pub fn verify_bind(
    host_key: &[u8],
    session_id: &[u8],
    signature: &[u8],
    forwarding: bool,
) -> Option<Bound> {
    let key = PublicKey::from_bytes(host_key).ok()?;
    let signature = Signature::try_from(signature).ok()?;
    Verifier::verify(&key, session_id, &signature).ok()?;
    Some(Bound {
        host_fingerprint: key.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
        forwarding,
    })
}

/// What a sign request is for, read from its data. `ssh` signs a user
/// authentication request (RFC 4252: the session id, then 50); `git` and
/// `ssh-keygen -Y sign` sign an SSHSIG blob naming its namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    Login { user: String },
    Sshsig { namespace: String },
    Unknown,
}

pub fn purpose(data: &[u8]) -> Purpose {
    if let Some(mut rest) = data.strip_prefix(b"SSHSIG") {
        if let Some(namespace) = string(&mut rest) {
            return Purpose::Sshsig {
                namespace: String::from_utf8_lossy(namespace).into_owned(),
            };
        }
        return Purpose::Unknown;
    }
    let mut rest = data;
    let login = (|| {
        string(&mut rest)?; // session id
        let (&kind, mut tail) = rest.split_first()?;
        if kind != 50 {
            return None;
        }
        let user = string(&mut tail)?;
        Some(String::from_utf8_lossy(user).into_owned())
    })();
    match login {
        Some(user) => Purpose::Login { user },
        None => Purpose::Unknown,
    }
}

/// The session id a user authentication request carries, to match it
/// against the one the connection was bound to.
pub fn session_id_of(data: &[u8]) -> Option<&[u8]> {
    let mut rest = data;
    string(&mut rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::Signer;
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    fn wire(kind: u8, parts: &[&[u8]], tail: &[u8]) -> Vec<u8> {
        let mut out = vec![kind];
        for part in parts {
            put_string(&mut out, part);
        }
        out.extend_from_slice(tail);
        out
    }

    #[test]
    fn requests_parse_and_anything_else_is_refused() {
        assert_eq!(parse(&[11]), Request::Identities);
        assert_eq!(
            parse(&wire(13, &[b"key", b"data"], &4u32.to_be_bytes())),
            Request::Sign {
                key: b"key".to_vec(),
                data: b"data".to_vec(),
                flags: 4
            }
        );
        // Add identity, remove all, lock, a smartcard key.
        for kind in [17u8, 19, 22, 20, 25, 9] {
            assert_eq!(parse(&[kind, 0, 0, 0, 0]), Request::Other, "{kind}");
        }
        assert_eq!(parse(&[]), Request::Other);
        assert_eq!(parse(&[11, 0]), Request::Other, "trailing bytes");
        assert_eq!(
            parse(&wire(13, &[b"key"], &[])),
            Request::Other,
            "cut short"
        );
        assert_eq!(
            parse(&[13, 0xff, 0xff, 0xff, 0xff]),
            Request::Other,
            "a length past the end"
        );
        assert_eq!(parse(&wire(27, &[b"query"], &[])), Request::Other);
    }

    #[test]
    fn answers_have_the_wire_shapes() {
        assert_eq!(failure(), [5]);
        assert_eq!(
            identities_answer(&[(b"k".to_vec(), "GitHub".into())]),
            [
                12, 0, 0, 0, 1, 0, 0, 0, 1, b'k', 0, 0, 0, 6, b'G', b'i', b't', b'H', b'u', b'b'
            ]
        );
        assert_eq!(sign_answer(b"s"), [14, 0, 0, 0, 1, b's']);
    }

    fn host() -> PrivateKey {
        PrivateKey::new(
            KeypairData::Ed25519(Ed25519Keypair::from_seed(&[7; 32])),
            "",
        )
        .unwrap()
    }

    #[test]
    fn a_session_bind_is_trusted_only_when_the_host_key_signed_it() {
        let host = host();
        let blob = host.public_key().to_bytes().unwrap();
        let session = b"session-id-bytes";
        let signature: Signature = host.try_sign(session).unwrap();
        let mut sig_blob = Vec::new();
        ssh_key::encoding::Encode::encode(&signature, &mut sig_blob).unwrap();

        let message = wire(
            27,
            &[b"session-bind@openssh.com", &blob, session, &sig_blob],
            &[1],
        );
        let Request::SessionBind {
            host_key,
            session_id,
            signature,
            forwarding,
        } = parse(&message)
        else {
            panic!("not a bind");
        };
        let bound = verify_bind(&host_key, &session_id, &signature, forwarding).unwrap();
        assert!(bound.forwarding);
        assert_eq!(
            bound.host_fingerprint,
            host.public_key()
                .fingerprint(ssh_key::HashAlg::Sha256)
                .to_string()
        );
        assert!(verify_bind(&host_key, b"another session", &signature, false).is_none());
    }

    #[test]
    fn the_purpose_is_read_from_what_is_signed() {
        let mut login = Vec::new();
        put_string(&mut login, b"sid");
        login.push(50);
        put_string(&mut login, b"git");
        put_string(&mut login, b"ssh-connection");
        assert_eq!(purpose(&login), Purpose::Login { user: "git".into() });
        assert_eq!(session_id_of(&login), Some(&b"sid"[..]));

        let mut sig = b"SSHSIG".to_vec();
        put_string(&mut sig, b"git");
        assert_eq!(
            purpose(&sig),
            Purpose::Sshsig {
                namespace: "git".into()
            }
        );
        assert_eq!(purpose(b"random"), Purpose::Unknown);
    }
}
