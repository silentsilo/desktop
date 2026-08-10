//! The network half of the breach check. The webview computes the SHA-1 and
//! keeps the password; all that arrives here is a five-character hash
//! prefix, which is all that ever goes on the wire (k-anonymity). The
//! response is returned as opaque text: parsing it, defensively, is the
//! front-end's job, so a format change on their side degrades to "check
//! unavailable" rather than anything worse.

/// Matches the range API's own alphabet, and doubles as the guard that what
/// goes in the URL is a hash fragment and nothing else.
fn is_prefix(s: &str) -> bool {
    s.len() == 5 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One client for the whole session.
///
/// A `reqwest::Client` owns a connection pool, so building one per request
/// threw the pool away every time and opened a fresh TLS connection for each
/// of the hundreds of prefixes a full check asks about. Built lazily, since
/// most silos never run this at all.
static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn client() -> Result<&'static reqwest::Client, String> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    Ok(CLIENT.get_or_init(|| built))
}

#[tauri::command]
pub async fn pwned_range(prefix: String) -> Result<String, String> {
    if !is_prefix(&prefix) {
        return Err("not a hash prefix".into());
    }
    let response = client()?
        .get(format!("https://api.pwnedpasswords.com/range/{prefix}"))
        // Padding makes every bucket the same shape, so the response size
        // reveals nothing about which prefix was asked about.
        .header("Add-Padding", "true")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("breach service answered {}", response.status()));
    }
    response.text().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_prefix;

    #[test]
    fn only_five_hex_characters_reach_the_url() {
        assert!(is_prefix("5BAA6"));
        assert!(is_prefix("00000"));
        assert!(!is_prefix("5BAA"));
        assert!(!is_prefix("5BAA61"));
        assert!(!is_prefix("../.."));
        assert!(!is_prefix("5BAAG"));
        assert!(!is_prefix(""));
    }
}
