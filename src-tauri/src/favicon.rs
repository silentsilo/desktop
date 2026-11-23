//! Site icons for the password list, fetched here rather than by the window.
//!
//! An `<img>` pointed at a site leaves the icon in the WebView's cache on
//! disk, unencrypted: a list of every site the silo holds a login for, kept
//! after the silo locks. Fetched here, the icon reaches the window as a
//! `data:` URL and lives only in memory. The address is checked after it is
//! resolved and the connection is pinned to it, so a public name that
//! resolves to this machine or the local network fetches nothing.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use base64::Engine;

/// Icons are small. Anything larger is not an icon.
const MAX_ICON_BYTES: usize = 64 * 1024;

/// A name the window may ask for: letters, digits, dots and hyphens, as a
/// DNS name is written, with at least one dot.
fn plausible_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && !host.starts_with(['.', '-'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && host.parse::<IpAddr>().is_err()
}

/// Whether an address is one on the public internet.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 224)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// A content type that is an image, and not SVG, which is a document.
fn image_type(value: &str) -> Option<String> {
    let mime = value.split(';').next()?.trim().to_ascii_lowercase();
    (mime.starts_with("image/") && !mime.contains("svg")).then_some(mime)
}

/// The icon of `host`, as a `data:` URL, or nothing. Nothing is an answer,
/// not an error: the list shows initials instead.
#[tauri::command]
pub async fn favicon_fetch(host: String) -> Option<String> {
    let host = host.to_ascii_lowercase();
    if !plausible_host(&host) {
        return None;
    }
    let addr: SocketAddr = tokio::net::lookup_host((host.as_str(), 443))
        .await
        .ok()?
        .find(|a| is_public(a.ip()))?;
    let same_host = host.clone();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("SilentSilo")
        .https_only(true)
        // Pinned to the address checked above: a second lookup could answer
        // with this machine.
        .resolve(&host, addr)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() < 3 && attempt.url().host_str() == Some(&same_host) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .ok()?;
    let mut response = client
        .get(format!("https://{host}/favicon.ico"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mime = image_type(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)?
            .to_str()
            .ok()?,
    )?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > MAX_ICON_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return None;
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
    Some(format!("data:{mime};base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_dns_name_is_asked_for() {
        for good in ["example.com", "www.bank-online.co.uk", "a1.b2.io"] {
            assert!(plausible_host(good), "{good}");
        }
        for bad in [
            "localhost",
            "127.0.0.1",
            "[::1]",
            "example.com:8080",
            "user@example.com",
            "example.com/x",
            ".example.com",
            "-x.com",
            "",
        ] {
            assert!(!plausible_host(bad), "{bad}");
        }
    }

    #[test]
    fn a_local_or_private_address_is_not_fetched() {
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!is_public(local.parse().unwrap()), "{local}");
        }
        for public in ["1.1.1.1", "8.8.8.8", "2606:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    /// Over the network, so only by hand: `cargo test -p silentsilo
    /// favicon -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn a_real_site_gives_its_icon_and_a_local_name_nothing() {
        let icon = favicon_fetch("github.com".into()).await.expect("an icon");
        assert!(icon.starts_with("data:image/"), "{}", &icon[..40]);
        assert!(favicon_fetch("localhost".into()).await.is_none());
    }

    #[test]
    fn only_an_image_that_is_not_svg_is_kept() {
        assert_eq!(image_type("image/x-icon").as_deref(), Some("image/x-icon"));
        assert_eq!(
            image_type("image/PNG; charset=binary").as_deref(),
            Some("image/png")
        );
        assert!(image_type("image/svg+xml").is_none());
        assert!(image_type("text/html").is_none());
    }
}
