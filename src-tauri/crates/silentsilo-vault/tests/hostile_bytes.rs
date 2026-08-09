//! What the envelope readers do with bytes they did not write: envelopes
//! come from untrusted storage, and the silo-root JSON files from wherever
//! that folder has been. Rejecting is the expected outcome; a panic during
//! recovery is a crash in the one flow someone reaches when everything
//! else has already gone wrong.

use silentsilo_vault::{RecoveryEnvelope, StoredFidoKeys, format, unwrap_with_code};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn byte(&mut self) -> u8 {
        (self.next() & 0xff) as u8
    }
}

fn damaged(rng: &mut Rng, source: &[u8]) -> Vec<u8> {
    let mut out = source.to_vec();
    match rng.below(6) {
        0 => {
            let keep = rng.below(out.len());
            out.truncate(keep);
        }
        1 => {
            if !out.is_empty() {
                let at = rng.below(out.len());
                out[at] = rng.byte();
            }
        }
        2 => {
            if !out.is_empty() {
                let at = rng.below(out.len());
                let len = rng.below(out.len() - at).max(1);
                for byte in &mut out[at..at + len] {
                    *byte = rng.byte();
                }
            }
        }
        3 => {
            let at = rng.below(out.len() + 1);
            let len = rng.below(64) + 1;
            let junk: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            out.splice(at..at, junk);
        }
        // Numbers where strings belong, which is where a version field or a
        // KDF parameter turns into something nobody meant to write.
        4 => {
            let text = String::from_utf8_lossy(&out)
                .replace('"', "9")
                .replace('{', "[");
            out = text.into_bytes();
        }
        _ => {
            let len = rng.below(512);
            out = (0..len).map(|_| rng.byte()).collect();
        }
    }
    out
}

#[test]
fn no_silo_file_can_panic_the_reader() {
    let valid = format::encode(&StoredFidoKeys { keys: Vec::new() }).unwrap();

    for seed in 0..20000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let bytes = damaged(&mut rng, &valid);

        let attempt = std::panic::catch_unwind(|| {
            let _ = format::decode::<StoredFidoKeys>("the enrolled keys", &bytes);
        });

        assert!(
            attempt.is_ok(),
            "seed {seed} panicked the reader on: {}",
            String::from_utf8_lossy(&bytes)
        );
    }
}

#[test]
fn no_recovery_envelope_can_panic_the_reader() {
    // Far fewer rounds than the others: every envelope that still parses
    // runs a real Argon2 pass, so this is seconds where they are
    // milliseconds. The ceiling on those parameters is also what makes it
    // finish at all, since a damaged m_cost otherwise asks for terabytes.
    let dek = silentsilo_crypto::generate_dek();
    let (_code, envelope) = silentsilo_vault::create_recovery_envelope(&dek).unwrap();
    let valid = serde_json::to_vec(&envelope).unwrap();

    for seed in 0..300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x517CC1B727220A95) | 1);
        let bytes = damaged(&mut rng, &valid);

        let attempt = std::panic::catch_unwind(|| {
            if let Ok(parsed) = serde_json::from_slice::<RecoveryEnvelope>(&bytes) {
                let _ = unwrap_with_code(&parsed, "AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG-HHHH");
            }
        });

        assert!(
            attempt.is_ok(),
            "seed {seed} panicked the reader on: {}",
            String::from_utf8_lossy(&bytes)
        );
    }
}
