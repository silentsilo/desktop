use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use blake3::Hasher;
use rand::RngCore;
use uuid::Uuid;

use crate::dek::ContentKey;
use crate::error::CryptoError;

pub const SSLO_MAGIC: &[u8; 4] = b"SSLO";
/// `blob_id` is bound into every chunk's AAD, so ciphertext from one blob
/// cannot be substituted for another's by a party with write access to
/// storage. Content is encrypted under a per-blob key carried wrapped in
/// the operation record, which is what makes rotation affordable. The
/// version is compared exactly, not as a floor: a weaker binding is a path
/// an attacker can ask for by name.
pub const SSLO_VERSION: u16 = 1;
pub const CHUNK_SIZE: usize = 4 * 1024 * 1024;
const NONCE_SIZE: usize = 12;
/// Per-file random component of the nonce; the remaining 4 bytes are the
/// big-endian chunk index. Present even though every blob has a key of its
/// own, so a caller that reuses a content key by mistake does not
/// immediately repeat a (key, nonce) pair, which under AES-GCM breaks both
/// confidentiality and authentication.
const NONCE_PREFIX_SIZE: usize = 8;
const TAG_SIZE: usize = 16;
/// Fixed header: magic(4) + version(2) + chunk_size(4) + file_id(16) + blob_id(16) + hash(32) + nonce_prefix(8)
pub const HEADER_SIZE: usize = 82;

#[derive(Debug, Clone)]
pub struct BlobHeader {
    pub version: u16,
    pub chunk_size: u32,
    pub file_id: Uuid,
    pub blob_id: Uuid,
    pub content_hash: [u8; 32],
    pub nonce_prefix: [u8; NONCE_PREFIX_SIZE],
}

impl BlobHeader {
    fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(SSLO_MAGIC);
        buf[4..6].copy_from_slice(&self.version.to_be_bytes());
        buf[6..10].copy_from_slice(&self.chunk_size.to_be_bytes());
        buf[10..26].copy_from_slice(self.file_id.as_bytes());
        buf[26..42].copy_from_slice(self.blob_id.as_bytes());
        buf[42..74].copy_from_slice(&self.content_hash);
        buf[74..82].copy_from_slice(&self.nonce_prefix);
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, CryptoError> {
        if buf.len() < HEADER_SIZE {
            return Err(CryptoError::InvalidHeader("too short".into()));
        }
        if &buf[0..4] != SSLO_MAGIC {
            return Err(CryptoError::InvalidHeader("bad magic".into()));
        }
        let version = u16::from_be_bytes([buf[4], buf[5]]);
        if version != SSLO_VERSION {
            return Err(CryptoError::InvalidHeader(format!(
                "unsupported version {version}"
            )));
        }
        let chunk_size = u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]]);
        let file_id = Uuid::from_bytes(buf[10..26].try_into().unwrap());
        let blob_id = Uuid::from_bytes(buf[26..42].try_into().unwrap());
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&buf[42..74]);
        let mut nonce_prefix = [0u8; NONCE_PREFIX_SIZE];
        nonce_prefix.copy_from_slice(&buf[74..82]);
        Ok(Self {
            version,
            chunk_size,
            file_id,
            blob_id,
            content_hash,
            nonce_prefix,
        })
    }
}

fn chunk_nonce(nonce_prefix: &[u8; NONCE_PREFIX_SIZE], chunk_index: u32) -> [u8; NONCE_SIZE] {
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    nonce_bytes[0..NONCE_PREFIX_SIZE].copy_from_slice(nonce_prefix);
    nonce_bytes[NONCE_PREFIX_SIZE..].copy_from_slice(&chunk_index.to_be_bytes());
    nonce_bytes
}

pub struct EncryptResult {
    pub header: BlobHeader,
    pub size_bytes: u64,
}

/// `key` belongs to this blob alone. The caller generates it, wraps it under
/// the vault DEK and stores the wrapping in the record that names the blob;
/// it is deliberately not written into the file, because a key inside the
/// object it protects would have to be rewritten to rotate, and rewriting is
/// exactly what this design exists to avoid.
pub fn encrypt_file(
    source: &Path,
    dest: &Path,
    key: &ContentKey,
    file_id: Uuid,
    blob_id: Uuid,
) -> Result<EncryptResult, CryptoError> {
    let mut source_file = File::open(source)?;
    let mut dest_file = File::create(dest)?;

    let mut hasher = Hasher::new();
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("valid key length");

    let mut nonce_prefix = [0u8; NONCE_PREFIX_SIZE];
    rand::rng().fill_bytes(&mut nonce_prefix);

    let header = BlobHeader {
        version: SSLO_VERSION,
        chunk_size: CHUNK_SIZE as u32,
        file_id,
        blob_id,
        content_hash: [0u8; 32],
        nonce_prefix,
    };
    dest_file.write_all(&header.to_bytes())?;

    let mut chunk_index: u32 = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = source_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);

        let nonce_bytes = chunk_nonce(&nonce_prefix, chunk_index);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut ciphertext = buf[..n].to_vec();
        let tag = cipher
            .encrypt_in_place_detached(nonce, blob_id.as_bytes(), &mut ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)?;

        dest_file.write_all(&ciphertext)?;
        dest_file.write_all(tag.as_slice())?;

        chunk_index += 1;
    }

    let content_hash = *hasher.finalize().as_bytes();
    let final_header = BlobHeader {
        content_hash,
        ..header
    };

    dest_file.seek(SeekFrom::Start(0))?;
    dest_file.write_all(&final_header.to_bytes())?;
    dest_file.flush()?;

    let size_bytes = dest_file.metadata()?.len();

    Ok(EncryptResult {
        header: final_header,
        size_bytes,
    })
}

/// `expected_blob_id` must match the id the caller believes this
/// ciphertext belongs to: without it, a party who can rewrite objects at
/// rest could substitute one file's ciphertext for another's and it would
/// still decrypt cleanly under the wrong identity. The plaintext is also
/// checked against the hash the header records, as `verify_blob` does: GCM
/// authenticates each chunk on its own, so a file cut at a chunk boundary
/// would otherwise come out silently shorter, and an attacker cannot forge
/// the hash of plaintext they never see.
pub fn decrypt_blob(
    source: &Path,
    dest: &Path,
    key: &ContentKey,
    expected_blob_id: Uuid,
) -> Result<BlobHeader, CryptoError> {
    let mut source_file = File::open(source)?;

    let mut header_buf = [0u8; HEADER_SIZE];
    source_file.read_exact(&mut header_buf)?;
    let header = BlobHeader::from_bytes(&header_buf)?;

    if header.blob_id != expected_blob_id {
        return Err(CryptoError::InvalidHeader(format!(
            "blob identity mismatch: expected {expected_blob_id}, found {}",
            header.blob_id
        )));
    }

    // Only now, because `File::create` truncates: a refused blob must not
    // leave an empty file where the caller asked for the plaintext, nor
    // destroy whatever was already at that path.
    let mut dest_file = File::create(dest)?;

    let aad: &[u8] = header.blob_id.as_bytes();

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("valid key length");
    let chunk_capacity = header.chunk_size as usize;
    let max_chunk = chunk_capacity + TAG_SIZE;
    let mut chunk_index: u32 = 0;
    let mut hasher = Hasher::new();

    loop {
        let mut chunk_buf = vec![0u8; max_chunk];
        let read = read_up_to(&mut source_file, &mut chunk_buf)?;
        if read == 0 {
            break;
        }
        if read < TAG_SIZE {
            return Err(CryptoError::InvalidHeader("truncated chunk".into()));
        }

        let (ciphertext, tag_bytes) = chunk_buf[..read].split_at_mut(read - TAG_SIZE);
        let tag = Tag::from_slice(tag_bytes);
        let nonce_bytes = chunk_nonce(&header.nonce_prefix, chunk_index);
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher
            .decrypt_in_place_detached(nonce, aad, ciphertext, tag)
            .map_err(|_| CryptoError::DecryptionFailed)?;

        hasher.update(ciphertext);
        dest_file.write_all(ciphertext)?;
        chunk_index += 1;
    }

    dest_file.flush()?;

    if *hasher.finalize().as_bytes() != header.content_hash {
        // The mismatch is only known once every chunk is out, so the partial
        // plaintext is already at `dest`. It cannot be left there: a caller
        // that treats an error as "the file was not produced" would otherwise
        // hand the user a silently shortened document. Dropped first, because
        // Windows will not remove an open file.
        drop(dest_file);
        let _ = std::fs::remove_file(dest);
        return Err(CryptoError::InvalidHeader(
            "content does not match the hash recorded for it".into(),
        ));
    }
    Ok(header)
}

/// Reads a blob through without writing the plaintext anywhere, for
/// checking a silo against its storage. Decrypting is the check, and the
/// recomputed content hash catches ciphertext that is intact but is not
/// the file it says it is. Separate from `decrypt_blob` so verifying a
/// hundred gigabytes does not write a hundred gigabytes.
pub fn verify_blob(
    source: &Path,
    key: &ContentKey,
    expected_blob_id: Uuid,
) -> Result<BlobHeader, CryptoError> {
    let mut source_file = File::open(source)?;

    let mut header_buf = [0u8; HEADER_SIZE];
    source_file.read_exact(&mut header_buf)?;
    let header = BlobHeader::from_bytes(&header_buf)?;

    if header.blob_id != expected_blob_id {
        return Err(CryptoError::InvalidHeader(format!(
            "blob identity mismatch: expected {expected_blob_id}, found {}",
            header.blob_id
        )));
    }

    let aad: &[u8] = header.blob_id.as_bytes();
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("valid key length");
    let max_chunk = header.chunk_size as usize + TAG_SIZE;
    let mut chunk_index: u32 = 0;
    let mut hasher = Hasher::new();

    loop {
        let mut chunk_buf = vec![0u8; max_chunk];
        let read = read_up_to(&mut source_file, &mut chunk_buf)?;
        if read == 0 {
            break;
        }
        if read < TAG_SIZE {
            return Err(CryptoError::InvalidHeader("truncated chunk".into()));
        }

        let (ciphertext, tag_bytes) = chunk_buf[..read].split_at_mut(read - TAG_SIZE);
        let tag = Tag::from_slice(tag_bytes);
        let nonce_bytes = chunk_nonce(&header.nonce_prefix, chunk_index);
        let nonce = Nonce::from_slice(&nonce_bytes);
        cipher
            .decrypt_in_place_detached(nonce, aad, ciphertext, tag)
            .map_err(|_| CryptoError::DecryptionFailed)?;

        hasher.update(ciphertext);
        chunk_index += 1;
    }

    if *hasher.finalize().as_bytes() != header.content_hash {
        return Err(CryptoError::InvalidHeader(
            "content does not match the hash recorded for it".into(),
        ));
    }
    Ok(header)
}

/// Fill `buf` as much as possible before hitting EOF. A single `Read::read`
/// call isn't guaranteed to fill the buffer even when more data remains, so
/// chunk boundaries (and therefore nonce reuse) must not depend on that.
fn read_up_to(source: &mut File, buf: &mut [u8]) -> Result<usize, CryptoError> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = source.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dek::generate_content_key;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_small_file() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("plain.txt");
        let enc = dir.path().join("blob.sslo");
        let out = dir.path().join("out.txt");

        std::fs::write(&plain, b"hello silentsilo").unwrap();

        let key = generate_content_key();
        let file_id = Uuid::new_v4();
        let blob_id = Uuid::new_v4();
        let result = encrypt_file(&plain, &enc, &key, file_id, blob_id).unwrap();
        assert_eq!(result.header.blob_id, blob_id);

        let header = decrypt_blob(&enc, &out, &key, blob_id).unwrap();
        assert_eq!(header.content_hash, result.header.content_hash);
        assert_eq!(std::fs::read(&out).unwrap(), b"hello silentsilo");
    }

    /// A party who can rewrite the ciphertext at rest (e.g. swap the whole
    /// object at a blob's storage key for a *different*, internally
    /// consistent blob) must not have that substitution go undetected just
    /// because the swapped-in ciphertext still authenticates against its
    /// own header.
    #[test]
    fn decrypt_rejects_blob_id_mismatch() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("plain.txt");
        let enc = dir.path().join("blob.sslo");
        let out = dir.path().join("out.txt");
        std::fs::write(&plain, b"hello silentsilo").unwrap();

        let key = generate_content_key();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        let err = decrypt_blob(&enc, &out, &key, Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidHeader(_)));
    }

    /// v2 bound nothing into the AAD, so a chunk could be moved between
    /// blobs undetected. Nothing in the world was ever written by it, and a
    /// reader that still accepts it is a downgrade an attacker can request
    /// by writing a `2` into the header.
    #[test]
    fn a_v2_header_is_refused_rather_than_read_with_empty_aad() {
        let dir = tempdir().unwrap();
        let enc = dir.path().join("legacy.sslo");
        let out = dir.path().join("out.txt");

        let key = generate_content_key();
        let blob_id = Uuid::new_v4();
        let plaintext = b"legacy plaintext";

        let mut nonce_prefix = [0u8; NONCE_PREFIX_SIZE];
        rand::rng().fill_bytes(&mut nonce_prefix);
        let mut hasher = Hasher::new();
        hasher.update(plaintext);
        let header = BlobHeader {
            version: 2,
            chunk_size: CHUNK_SIZE as u32,
            file_id: Uuid::new_v4(),
            blob_id,
            content_hash: *hasher.finalize().as_bytes(),
            nonce_prefix,
        };

        let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).unwrap();
        let nonce_bytes = chunk_nonce(&nonce_prefix, 0);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut ciphertext = plaintext.to_vec();
        let tag = cipher
            .encrypt_in_place_detached(nonce, b"", &mut ciphertext)
            .unwrap();

        let mut bytes = header.to_bytes().to_vec();
        bytes.extend_from_slice(&ciphertext);
        bytes.extend_from_slice(tag.as_slice());
        std::fs::write(&enc, &bytes).unwrap();

        let err = decrypt_blob(&enc, &out, &key, blob_id).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidHeader(_)));
        assert!(!out.exists(), "a refused blob must leave nothing behind");
    }

    /// Two files encrypted with the same vault DEK must never reuse the same
    /// (key, nonce) pair — that's what makes chunk 0 of every file "safe" to
    /// encrypt under one shared key. Regression test for the nonce-reuse fix.
    #[test]
    fn different_files_get_different_nonce_prefixes() {
        let dir = tempdir().unwrap();
        let key = generate_content_key();

        let plain_a = dir.path().join("a.txt");
        let enc_a = dir.path().join("a.sslo");
        std::fs::write(&plain_a, b"file a content").unwrap();
        let result_a =
            encrypt_file(&plain_a, &enc_a, &key, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        let plain_b = dir.path().join("b.txt");
        let enc_b = dir.path().join("b.sslo");
        std::fs::write(&plain_b, b"file b content").unwrap();
        let result_b =
            encrypt_file(&plain_b, &enc_b, &key, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        assert_ne!(
            result_a.header.nonce_prefix, result_b.header.nonce_prefix,
            "two files under the same DEK must not share a nonce prefix",
        );
    }

    /// The version check is exact, so a header from a future format is
    /// refused rather than read under this build's rules.
    #[test]
    fn rejects_a_version_this_build_does_not_know() {
        let mut header = [0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(SSLO_MAGIC);
        header[4..6].copy_from_slice(&(SSLO_VERSION + 1).to_be_bytes());
        let err = BlobHeader::from_bytes(&header).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidHeader(_)));
    }

    /// Verifying is what a scrub does to every blob, so it has to agree with
    /// decrypting on what is sound and catch the two ways one can rot.
    #[test]
    fn verify_accepts_a_good_blob_and_refuses_a_damaged_one() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("in.bin");
        let enc = dir.path().join("blob.sslo");
        let key = generate_content_key();
        let blob_id = Uuid::new_v4();
        std::fs::write(&plain, vec![42u8; CHUNK_SIZE + 1234]).unwrap();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), blob_id).unwrap();

        let header = verify_blob(&enc, &key, blob_id).unwrap();
        assert_eq!(header.blob_id, blob_id);

        // One flipped bit in the ciphertext. GCM authenticates every chunk,
        // so this fails rather than returning plausible rubbish.
        let mut bytes = std::fs::read(&enc).unwrap();
        let target = HEADER_SIZE + 64;
        bytes[target] ^= 0x01;
        let rotted = dir.path().join("rotted.sslo");
        std::fs::write(&rotted, &bytes).unwrap();
        assert!(verify_blob(&rotted, &key, blob_id).is_err());
    }

    /// The other half: ciphertext that decrypts cleanly but is not the file
    /// the header says it is. Only the recorded hash catches that.
    #[test]
    fn verify_refuses_content_that_does_not_match_its_recorded_hash() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("in.bin");
        let enc = dir.path().join("blob.sslo");
        let key = generate_content_key();
        let blob_id = Uuid::new_v4();
        std::fs::write(&plain, b"the real contents").unwrap();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), blob_id).unwrap();

        let mut bytes = std::fs::read(&enc).unwrap();
        // Claim a different hash while leaving the ciphertext intact. The
        // header is not authenticated, so this is a change an attacker with
        // write access to storage can actually make.
        bytes[42] ^= 0xff;
        let lying = dir.path().join("lying.sslo");
        std::fs::write(&lying, &bytes).unwrap();

        assert!(verify_blob(&lying, &key, blob_id).is_err());
    }

    /// GCM authenticates each chunk on its own, so a `.sslo` cut at a chunk
    /// boundary decrypts cleanly chunk by chunk and simply ends early. The
    /// hash recorded in the header is the only thing that notices, which is
    /// why decrypting checks it and not just `verify_blob`.
    #[test]
    fn a_blob_truncated_at_a_chunk_boundary_is_refused() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("in.bin");
        let enc = dir.path().join("blob.sslo");
        let out = dir.path().join("out.bin");
        let key = generate_content_key();
        let blob_id = Uuid::new_v4();
        // Two chunks, so the second can be cut off cleanly.
        std::fs::write(&plain, vec![42u8; CHUNK_SIZE + 1234]).unwrap();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), blob_id).unwrap();

        let bytes = std::fs::read(&enc).unwrap();
        let cut = dir.path().join("cut.sslo");
        std::fs::write(&cut, &bytes[..HEADER_SIZE + CHUNK_SIZE + TAG_SIZE]).unwrap();

        let err = decrypt_blob(&cut, &out, &key, blob_id).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidHeader(_)), "got {err:?}");
        assert!(
            !out.exists(),
            "a refused blob must not leave a shortened plaintext behind"
        );
    }

    /// Same check against a header whose hash was rewritten. The header is
    /// not authenticated, so this is a change an attacker with write access
    /// to storage can make; what they cannot do is compute the hash of a
    /// plaintext they never see, which is what makes the comparison hold.
    #[test]
    fn decrypt_refuses_content_that_does_not_match_its_recorded_hash() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("in.bin");
        let enc = dir.path().join("blob.sslo");
        let out = dir.path().join("out.bin");
        let key = generate_content_key();
        let blob_id = Uuid::new_v4();
        std::fs::write(&plain, b"the real contents").unwrap();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), blob_id).unwrap();

        let mut bytes = std::fs::read(&enc).unwrap();
        bytes[42] ^= 0xff;
        let lying = dir.path().join("lying.sslo");
        std::fs::write(&lying, &bytes).unwrap();

        assert!(decrypt_blob(&lying, &out, &key, blob_id).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn verify_refuses_a_blob_that_is_not_the_one_asked_for() {
        let dir = tempdir().unwrap();
        let plain = dir.path().join("in.bin");
        let enc = dir.path().join("blob.sslo");
        let key = generate_content_key();
        std::fs::write(&plain, b"contents").unwrap();
        encrypt_file(&plain, &enc, &key, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        assert!(verify_blob(&enc, &key, Uuid::new_v4()).is_err());
    }
}
