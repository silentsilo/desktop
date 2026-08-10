//! What the operation-record parser does with bytes it did not write.
//! Every record a sync pass replays comes from storage the threat model
//! does not trust, and the realistic failure in Rust is a panic: an index
//! from a length in the input, an unwrap on a missing field. A panic
//! inside a sync pass is a crash on a schedule nobody controls.
//!
//! Deterministic rather than coverage-guided: a seed reproduces the exact
//! bytes and it runs in an ordinary `cargo test`. Less depth than real
//! fuzzing and no substitute for it, but the shallow cases are the ones
//! nobody has looked at.

use silentsilo_vfs::oplog::OpRecord;

/// The same generator the convergence property test uses.
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

/// A record the parser accepts, as the starting point for damage.
///
/// Mutating something valid reaches far deeper than random noise, which is
/// rejected by the first few bytes almost every time.
const VALID: &str = r#"{
    "op_id": "0f9a4d3e-2c1b-4a5d-8e7f-1234567890ab",
    "lamport": 7,
    "device_id": "1a2b3c4d-5e6f-4788-9900-aabbccddeeff",
    "at": 1700000000,
    "skippable": false,
    "op": "CreateFolder",
    "id": "22222222-3333-4444-5555-666666666666",
    "parent_id": "77777777-8888-4999-aaaa-bbbbbbbbbbbb",
    "name": "Documents"
}"#;

fn damaged(rng: &mut Rng, source: &[u8]) -> Vec<u8> {
    let mut out = source.to_vec();
    match rng.below(6) {
        // Cut it short, which is what a partial upload leaves behind.
        0 => {
            let keep = rng.below(out.len());
            out.truncate(keep);
        }
        // One byte changed: the shape survives, a value does not.
        1 => {
            if !out.is_empty() {
                let at = rng.below(out.len());
                out[at] = rng.byte();
            }
        }
        // A run of bytes replaced, which breaks a field without breaking the
        // frame around it.
        2 => {
            if !out.is_empty() {
                let at = rng.below(out.len());
                let len = rng.below(out.len() - at).max(1);
                for byte in &mut out[at..at + len] {
                    *byte = rng.byte();
                }
            }
        }
        // Something spliced in, so lengths and offsets disagree.
        3 => {
            let at = rng.below(out.len() + 1);
            let len = rng.below(64) + 1;
            let junk: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            out.splice(at..at, junk);
        }
        // A number where the parser wants a string, and the reverse.
        4 => {
            let text = String::from_utf8_lossy(&out)
                .replace('"', "1")
                .replace(':', ",");
            out = text.into_bytes();
        }
        // Pure noise, for the cases the frame never survives.
        _ => {
            let len = rng.below(512);
            out = (0..len).map(|_| rng.byte()).collect();
        }
    }
    out
}

#[test]
fn no_operation_record_can_panic_the_parser() {
    for seed in 0..20000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let bytes = damaged(&mut rng, VALID.as_bytes());

        // Whatever it decides, it has to decide it. `catch_unwind` so a
        // failure names the seed and the input rather than only the panic.
        let attempt = std::panic::catch_unwind(|| {
            let _ = OpRecord::from_bytes(&bytes);
        });

        assert!(
            attempt.is_ok(),
            "seed {seed} panicked the parser on {} bytes: {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
    }
}

#[test]
fn a_record_that_survives_the_damage_still_re_encodes() {
    // Anything the parser accepts is something a device will store and hand
    // on, so it has to be able to write it back out. A record that decodes
    // and then fails to encode would stop a sync pass just as dead.
    for seed in 0..20000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x517CC1B727220A95) | 1);
        let bytes = damaged(&mut rng, VALID.as_bytes());

        let Ok(record) = OpRecord::from_bytes(&bytes) else {
            continue;
        };
        let encoded = record
            .to_bytes()
            .unwrap_or_else(|e| panic!("seed {seed}: decoded but would not encode: {e}"));
        let again = OpRecord::from_bytes(&encoded)
            .unwrap_or_else(|e| panic!("seed {seed}: its own output was rejected: {e}"));
        assert_eq!(record, again, "seed {seed}: round trip changed the record");
    }
}
