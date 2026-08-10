use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub const ORIGIN: &str = "https://silentsilo.com";

/// SHA-256 of UTF-8 `message` — CTAP hmac-secret salt for string inputs.
pub fn hmac_salt_from_string(message: &str) -> [u8; 32] {
    sha256(message.as_bytes())
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn b64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// WebAuthn client data JSON for `makeCredential`.
pub fn create_client_data_json(challenge: &[u8]) -> Vec<u8> {
    format!(
        r#"{{"type":"webauthn.create","challenge":"{}","origin":"{}","crossOrigin":false}}"#,
        b64url(challenge),
        ORIGIN,
    )
    .into_bytes()
}

/// WebAuthn client data JSON for `getAssertion` (DMS check-in).
pub fn get_client_data_json(challenge: &[u8]) -> Vec<u8> {
    format!(
        r#"{{"type":"webauthn.get","challenge":"{}","origin":"{}","crossOrigin":false}}"#,
        b64url(challenge),
        ORIGIN,
    )
    .into_bytes()
}
