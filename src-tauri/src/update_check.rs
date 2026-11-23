//! Which version of SilentSilo is the latest, and nothing more. The app no
//! longer downloads or installs updates: it says one exists and opens the
//! download page, so nothing here can put code on this computer
//! (docs/ARCHITECTURE.md, "Updates").
//!
//! The answer is `{"version":"x.y.z"}`, read from the website, and from the
//! website's repository on GitHub when the site does not give a usable one.
//! Nothing in it decides where the user is sent: the download page is fixed
//! in the window, so a forged file can at worst announce a version that does
//! not exist. The request carries no version, platform or identifier.

use std::time::Duration;

/// The website's copy, deployed with the site from `public/latest.json`.
pub const LATEST_JSON: &str = "https://silentsilo.com/latest.json";

/// The same file in the website's public repository, for when the site is
/// down or unreachable.
pub const LATEST_JSON_FALLBACK: &str =
    "https://raw.githubusercontent.com/silentsilo/website/main/public/latest.json";

/// A file larger than this is not the one-line answer it should be.
const MAX_BYTES: usize = 4096;

static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn client() -> Result<&'static reqwest::Client, String> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        // No version in it: the request says nothing about this install.
        .user_agent("SilentSilo")
        // A redirect to plain http would let anyone on the path answer. An
        // e2e build asks a local test server instead.
        .https_only(cfg!(not(feature = "e2e")))
        .build()
        .map_err(|e| e.to_string())?;
    Ok(CLIENT.get_or_init(|| built))
}

/// `x.y.z`, optionally with a prerelease part. No build part: releases never
/// carry one, and it is free text the window would show.
fn is_version(text: &str) -> bool {
    if text.len() > 64 {
        return false;
    }
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    let numbers = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()));
    let pre_ok = pre.is_none_or(|pre| {
        !pre.is_empty()
            && pre.split('.').all(|id| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    });
    numbers && pre_ok
}

/// The version an answer names, if it names one. Fields other than
/// `version` are ignored, so the file can grow without breaking this.
pub fn parse_latest(body: &[u8]) -> Option<String> {
    if body.len() > MAX_BYTES {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let version = value.get("version")?.as_str()?.trim();
    is_version(version).then(|| version.to_string())
}

async fn fetch(url: &str) -> Result<String, String> {
    let mut response = client()?.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_BYTES as u64)
    {
        return Err(format!("{url} answered more than {MAX_BYTES} bytes"));
    }
    // Read in pieces, so an endless answer stops at the cap.
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return Err(format!("{url} answered more than {MAX_BYTES} bytes"));
        }
    }
    parse_latest(&body).ok_or_else(|| format!("{url} does not name a version"))
}

/// The latest published version: from the website, or from GitHub when the
/// website gives no usable answer. The window compares it with its own.
#[tauri::command]
pub async fn update_latest_version() -> Result<String, String> {
    #[cfg(feature = "e2e")]
    if let Some(url) = crate::e2e::latest_json() {
        return fetch(&url).await;
    }
    match fetch(LATEST_JSON).await {
        Ok(version) => Ok(version),
        Err(first) => fetch(LATEST_JSON_FALLBACK)
            .await
            .map_err(|second| format!("{first}; {second}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_version, parse_latest};

    #[test]
    fn the_version_is_read_and_other_fields_ignored() {
        assert_eq!(
            parse_latest(br#"{"version":"1.6.0"}"#).as_deref(),
            Some("1.6.0")
        );
        assert_eq!(
            parse_latest(br#"{"version":" 1.6.0 ","security":true,"notes":"x"}"#).as_deref(),
            Some("1.6.0")
        );
        assert_eq!(
            parse_latest(br#"{"version":"1.6.0-rc.2"}"#).as_deref(),
            Some("1.6.0-rc.2")
        );
    }

    #[test]
    fn anything_that_is_not_a_version_is_no_answer() {
        assert_eq!(parse_latest(b""), None);
        assert_eq!(parse_latest(b"<html>not found</html>"), None);
        assert_eq!(parse_latest(br#"{"version":1.6}"#), None);
        assert_eq!(parse_latest(br#"{"latest":"1.6.0"}"#), None);
        assert_eq!(parse_latest(br#"["1.6.0"]"#), None);
        assert_eq!(parse_latest(&[b' '; 5000]), None);
        for bad in [
            "",
            "1.6",
            "1.6.0.1",
            "v1.6.0",
            "1.6.x",
            "1.6.0-",
            "1..0",
            "1.6.0-a b",
            "1.6.0+build",
            "9.9.9+ reinstall from another site",
        ] {
            assert!(!is_version(bad), "{bad} read as a version");
        }
        let long = format!("1.6.0-{}", "a".repeat(64));
        assert!(!is_version(&long));
    }
}
