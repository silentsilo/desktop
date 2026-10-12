//! Signs release files with the updater key, the way `tauri signer sign`
//! does, and asks for the key's password once for all of them.
//!
//! `tauri signer sign` runs out of node_modules, which every npm package of
//! the app can rewrite during `npm ci`. This is built from its own lockfile
//! before the app is, and the release script checks its hash before it runs,
//! so the password only ever reaches code that did not come with the app.
//!
//!     silentsilo-release-signer --key <tauri key file> --pubkey <tauri pubkey>
//!         --version <x.y.z> <file>...
//!     silentsilo-release-signer --key <tauri key file> --check
//!
//! `--pubkey` is the `pubkey` of tauri.conf.json: a key that is not that
//! one signs nothing, since the app would refuse every file it signed.
//! `--check` only reads the key file, without the password, so a release
//! can find a missing or damaged key before it builds anything.
//!
//! For each file it writes `<file>.sig`: the minisign signature, base64 as
//! the updater expects it. The trusted comment names the version and the
//! file, and the app checks that the version is the one it was offered.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut key = None;
    let mut version = None;
    let mut pubkey = None;
    let mut check = false;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--key" => key = args.next().map(PathBuf::from),
            "--version" => version = args.next(),
            "--pubkey" => pubkey = args.next(),
            "--check" => check = true,
            _ => files.push(PathBuf::from(arg)),
        }
    }
    let key = key.ok_or("--key is missing")?;
    let wrapped = std::fs::read_to_string(&key).map_err(|e| format!("{}: {e}", key.display()))?;
    if check {
        read_key_box(&wrapped)?;
        println!("{} reads as an updater key", key.display());
        return Ok(());
    }
    let expected = expected_public_key(&pubkey.ok_or("--pubkey is missing")?)?;
    let version = version.ok_or("--version is missing")?;
    if files.is_empty() {
        return Err("no files to sign".into());
    }
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return Err(format!("{version} is not a version"));
    }

    let password = rpassword::prompt_password("Updater signing key password: ")
        .map_err(|e| format!("reading the password: {e}"))?;
    let (secret, public) = load_key(&wrapped, password)?;
    if public.to_base64() != expected.to_base64() {
        return Err(
            "this key is not the one whose public key the app carries; nothing was signed".into(),
        );
    }

    for file in &files {
        sign_one(file, &version, &secret, &public)?;
        println!("signed {}", file.display());
    }
    Ok(())
}

/// The Tauri key file is the minisign secret key, base64 again.
fn read_key_box(wrapped: &str) -> Result<minisign::SecretKeyBox, String> {
    let text = STANDARD
        .decode(wrapped.trim())
        .map_err(|e| format!("the key file is not base64: {e}"))?;
    let text = String::from_utf8(text).map_err(|_| "the key file is not text")?;
    // Checked by shape: the minisign crate parses a key only when it opens
    // it, and anything at all reads as one before that.
    let mut lines = text.lines();
    let comment = lines.next().unwrap_or_default();
    let body = lines.next().map(|l| STANDARD.decode(l.trim()));
    if !comment.starts_with("untrusted comment:")
        || !matches!(body, Some(Ok(bytes)) if bytes.len() == SECRET_KEY_BYTES)
    {
        return Err("not an updater key".into());
    }
    minisign::SecretKeyBox::from_string(&text).map_err(|e| format!("not an updater key: {e}"))
}

/// A minisign secret key as stored: algorithms, KDF parameters, key id and
/// the encrypted key pair.
const SECRET_KEY_BYTES: usize = 158;

/// The `pubkey` of tauri.conf.json: the minisign public key, base64 again.
fn expected_public_key(wrapped: &str) -> Result<minisign::PublicKey, String> {
    let text = STANDARD
        .decode(wrapped.trim())
        .map_err(|e| format!("the public key is not base64: {e}"))?;
    let text = String::from_utf8(text).map_err(|_| "the public key is not text")?;
    minisign::PublicKeyBox::from_string(&text)
        .and_then(|b| b.into_public_key())
        .map_err(|e| format!("not a minisign public key: {e}"))
}

fn load_key(
    wrapped: &str,
    password: String,
) -> Result<(minisign::SecretKey, minisign::PublicKey), String> {
    let key_box = read_key_box(wrapped)?;
    // No password is how `tauri signer generate` writes a key without one.
    let password = (!password.is_empty()).then_some(password);
    let secret = key_box
        .into_secret_key(password)
        .map_err(|_| "wrong password, or not the updater key")?;
    let public = minisign::PublicKey::from_secret_key(&secret).map_err(|e| e.to_string())?;
    Ok((secret, public))
}

fn sign_one(
    file: &Path,
    version: &str,
    secret: &minisign::SecretKey,
    public: &minisign::PublicKey,
) -> Result<(), String> {
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no usable name", file.display()))?;
    let comment = format!("version:{version}\tfile:{name}");
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = minisign::sign(Some(public), secret, data.as_slice(), Some(&comment), None)
        .map_err(|e| format!("{name}: {e}"))?;
    let text = signature.to_string();

    // Checked before it is written: a signature the updater would refuse is
    // worse than none, because it looks finished.
    let check = minisign::SignatureBox::from_string(&text).map_err(|e| e.to_string())?;
    let public_box = public.to_box().map_err(|e| e.to_string())?;
    minisign::verify(
        &public_box.into_public_key().map_err(|e| e.to_string())?,
        &check,
        std::io::Cursor::new(data.as_slice()),
        true,
        false,
        false,
    )
    .map_err(|e| format!("{name}: the signature does not verify: {e}"))?;

    let mut sig_name = file.as_os_str().to_os_string();
    sig_name.push(".sig");
    let mut out =
        std::fs::File::create(PathBuf::from(&sig_name)).map_err(|e| format!("{name}.sig: {e}"))?;
    out.write_all(STANDARD.encode(text.as_bytes()).as_bytes())
        .map_err(|e| format!("{name}.sig: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tauri_style_key_signs_what_the_updater_accepts() {
        let pair = minisign::KeyPair::generate_encrypted_keypair(Some("pw".into())).unwrap();
        let wrapped = STANDARD.encode(pair.sk.to_box(None).unwrap().to_string());
        assert!(
            read_key_box(&wrapped).is_ok(),
            "readable without the password"
        );
        assert!(read_key_box("bm90IGEga2V5").is_err());
        assert!(load_key(&wrapped, "wrong".into()).is_err());
        let (secret, public) = load_key(&wrapped, "pw".into()).unwrap();
        let carried = STANDARD.encode(pair.pk.to_box().unwrap().to_string());
        assert_eq!(
            expected_public_key(&carried).unwrap().to_base64(),
            public.to_base64()
        );

        let dir = std::env::temp_dir().join(format!("signer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("SilentSilo_1.5.0_x64-setup.exe");
        std::fs::write(&file, b"installer").unwrap();
        sign_one(&file, "1.5.0", &secret, &public).unwrap();

        let sig = std::fs::read_to_string(dir.join("SilentSilo_1.5.0_x64-setup.exe.sig")).unwrap();
        let text = String::from_utf8(STANDARD.decode(sig).unwrap()).unwrap();
        assert!(
            text.contains("trusted comment: version:1.5.0	file:SilentSilo_1.5.0_x64-setup.exe")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Against a key `tauri signer generate` wrote, when one is at hand.
    #[test]
    #[ignore]
    fn a_key_from_tauri_signer_loads() {
        let path = std::env::var("SIGNER_TEST_KEY").unwrap();
        let wrapped = std::fs::read_to_string(path).unwrap();
        let password = std::env::var("SIGNER_TEST_PASSWORD").unwrap_or_default();
        load_key(&wrapped, password).unwrap();
    }
}
