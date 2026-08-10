//! What the ciphertext readers do with bytes they did not write. Both
//! parse before they authenticate, because the header says which version
//! and which nonce and none of it can be verified until it is read: that
//! window, where a length from the input becomes an index, is the one
//! worth attacking. Rejecting is expected; a panic is a crash triggered by
//! whoever holds the bucket.

use silentsilo_crypto::{
    decrypt_blob, encrypt_file, generate_content_key, generate_dek, seal, unseal,
};
use uuid::Uuid;

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

/// Damage that keeps enough shape to get past the first check.
///
/// Pure noise is rejected on the magic bytes almost every time and reaches
/// nothing; starting from something valid is what gets into the parsing.
fn damaged(rng: &mut Rng, source: &[u8]) -> Vec<u8> {
    let mut out = source.to_vec();
    match rng.below(5) {
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
            // The length and version fields live near the front, so damage
            // concentrated there is worth more than damage spread evenly.
            let span = out.len().min(64);
            for _ in 0..rng.below(8) + 1 {
                if span > 0 {
                    let at = rng.below(span);
                    out[at] = rng.byte();
                }
            }
        }
        3 => {
            let at = rng.below(out.len() + 1);
            let len = rng.below(128) + 1;
            let junk: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            out.splice(at..at, junk);
        }
        _ => {
            let len = rng.below(256);
            out = (0..len).map(|_| rng.byte()).collect();
        }
    }
    out
}

#[test]
fn no_sealed_payload_can_panic_the_reader() {
    let dek = generate_dek();
    let valid = seal(b"an operation record, more or less", &dek).unwrap();

    for seed in 0..20000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let bytes = damaged(&mut rng, &valid);

        let attempt = std::panic::catch_unwind(|| {
            let _ = unseal(&bytes, &dek);
        });

        assert!(
            attempt.is_ok(),
            "seed {seed} panicked the reader on {} bytes",
            bytes.len()
        );
    }
}

#[test]
fn no_blob_can_panic_the_reader() {
    // Fewer rounds than the sealed payload: each one writes two files, and
    // the header is the part being attacked either way.
    let dir = tempfile::tempdir().unwrap();
    let key = generate_content_key();
    let blob_id = Uuid::new_v4();

    let plain = dir.path().join("plain.bin");
    let sealed = dir.path().join("blob.sslo");
    std::fs::write(&plain, vec![7u8; 9000]).unwrap();
    encrypt_file(&plain, &sealed, &key, Uuid::new_v4(), blob_id).unwrap();
    let valid = std::fs::read(&sealed).unwrap();

    let target = dir.path().join("out.bin");
    for seed in 0..2000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x517CC1B727220A95) | 1);
        let bytes = damaged(&mut rng, &valid);
        let source = dir.path().join("damaged.sslo");
        std::fs::write(&source, &bytes).unwrap();

        let attempt = std::panic::catch_unwind(|| {
            let _ = decrypt_blob(&source, &target, &key, blob_id);
        });

        assert!(
            attempt.is_ok(),
            "seed {seed} panicked the reader on {} bytes",
            bytes.len()
        );
    }
}
