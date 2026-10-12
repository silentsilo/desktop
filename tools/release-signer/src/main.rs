//! Signs release files with the updater key, the way `tauri signer sign`
//! does, and asks for the key's password once for all of them.
//!
//! `tauri signer sign` runs out of node_modules, which every npm package of
//! the app can rewrite during `npm ci`. This is built from its own lockfile
//! before the app is, and the release script checks its hash before it runs,
//! so the password only ever reaches code that did not come with the app.
//!
//!     silentsilo-release-signer --key <tauri key file> --version <x.y.z> <file>...
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
    let mut files: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--key" => key = args.next().map(PathBuf::from),
            "--version" => version = args.next(),
            _ => files.push(PathBuf::from(arg)),
        }
    }
    let key = key.ok_or("--key is missing")?;
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

    let wrapped = std::fs::read_to_string(&key).map_err(|e| format!("{}: {e}", key.display()))?;
    let password = rpassword::prompt_password("Updater signing key password: ")
        .map_err(|e| format!("reading the password: {e}"))?;
    let (secret, public) = load_key(&wrapped, password)?;

    for file in &files {
        sign_one(file, &version, &secret, &public)?;
        println!("signed {}", file.display());
    }
    Ok(())
}

/// The Tauri key file is the minisign secret key, base64 again.
fn load_key(
    wrapped: &str,
    password: String,
) -> Result<(minisign::SecretKey, minisign::PublicKey), String> {
    let text = STANDARD
        .decode(wrapped.trim())
        .map_err(|e| format!("the key file is not base64: {e}"))?;
    let text = String::from_utf8(text).map_err(|_| "the key file is not text")?;
    let key_box = minisign::SecretKeyBox::from_string(&text).map_err(|e| e.to_string())?;
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
        assert!(load_key(&wrapped, "wrong".into()).is_err());
        let (secret, public) = load_key(&wrapped, "pw".into()).unwrap();

        let dir = std::env::temp_dir().join(format!("signer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("SilentSilo_1.5.0_x64-setup.exe");
        std::fs::write(&file, b"installer").unwrap();
        sign_one(&file, "1.5.0", &secret, &public).unwrap();

        let sig = std::fs::read_to_string(dir.join("SilentSilo_1.5.0_x64-setup.exe.sig")).unwrap();
        let text = String::from_utf8(STANDARD.decode(sig).unwrap()).unwrap();
        assert!(text.contains("trusted comment: version:1.5.0	file:SilentSilo_1.5.0_x64-setup.exe"));
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
