//! The browser extension's protocol, as pure functions: parsing requests,
//! deciding which saved login belongs to which site, and shaping answers.
//! The contract is `docs/PROTOCOL.md` in silentsilo/browser; message
//! shapes change there first.

use std::collections::HashMap;
use std::net::IpAddr;

use serde::Serialize;
use silentsilo_shell::browser_pipe::MAX_FRAME;
use url::{Host, Url};
use uuid::Uuid;

use super::logins::Login;

/// At most this many answers to a `search`.
pub const SEARCH_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Locked,
    NoSilo,
    UnknownRef,
    Cancelled,
    BadRequest,
    Busy,
}

impl Code {
    fn as_str(self) -> &'static str {
        match self {
            Code::Locked => "locked",
            Code::NoSilo => "no-silo",
            Code::UnknownRef => "unknown-ref",
            Code::Cancelled => "cancelled",
            Code::BadRequest => "bad-request",
            Code::Busy => "busy",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Code::Locked => "SilentSilo is locked. Unlock it and try again.",
            Code::NoSilo => "SilentSilo has no silo yet.",
            Code::UnknownRef => "That login is out of date. Open the list again.",
            Code::Cancelled => "The fill was not confirmed.",
            Code::BadRequest => "SilentSilo could not read the request.",
            Code::Busy => "Another fill is waiting for confirmation in SilentSilo.",
        }
    }
}

/// An error answer: a code and the sentence the popup shows as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}

impl Failure {
    pub fn new(code: Code) -> Self {
        Self {
            code,
            message: code.message().to_string(),
        }
    }

    pub fn with(code: Code, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

impl From<Code> for Failure {
    fn from(code: Code) -> Self {
        Failure::new(code)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Status,
    Logins { origin: String },
    Search { query: String },
    Fill { origin: String, reference: String },
}

/// Parses one request. On failure the id is whatever could be read, empty
/// when none could, so the extension can still match the refusal.
pub fn parse_request(bytes: &[u8]) -> Result<(String, Request), (String, Failure)> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| (String::new(), Failure::new(Code::BadRequest)))?;
    let Some(id) = value.get("id").and_then(|v| v.as_str()) else {
        return Err((String::new(), Failure::new(Code::BadRequest)));
    };
    let id = id.to_string();
    let text = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_string);
    let request = match value.get("type").and_then(|v| v.as_str()) {
        Some("status") => Some(Request::Status),
        Some("logins") => text("origin").map(|origin| Request::Logins { origin }),
        Some("search") => text("query").map(|query| Request::Search { query }),
        Some("fill") => match (text("origin"), text("ref")) {
            (Some(origin), Some(reference)) => Some(Request::Fill { origin, reference }),
            _ => None,
        },
        _ => None,
    };
    match request {
        Some(request) => Ok((id, request)),
        None => Err((id, Failure::new(Code::BadRequest))),
    }
}

/// The site a tab is on, as the extension reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    host: String,
    /// Explicit or the scheme's default.
    port: Option<u16>,
    /// `host` or `host:port`, for the confirmation.
    pub shown: String,
}

/// Reads a tab origin. `Ok(None)` is a well-formed origin nothing is ever
/// filled into: any scheme but `https:`, except `http:` on localhost and
/// loopback addresses. `Err` is something that is not an origin at all.
pub fn parse_origin(origin: &str) -> Result<Option<Site>, ()> {
    let url = Url::parse(origin).map_err(|_| ())?;
    let bare = matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    let fillable = match url.scheme() {
        "https" => true,
        "http" => is_loopback(url.host()),
        _ => return Ok(None),
    };
    if !bare || url.host_str().is_none_or(str::is_empty) {
        return Err(());
    }
    if !fillable {
        return Ok(None);
    }
    Ok(Some(site_of(&url)))
}

fn is_loopback(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain(name)) => name == "localhost",
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    }
}

fn site_of(url: &Url) -> Site {
    let host = url.host_str().unwrap_or_default().to_string();
    let shown = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    Site {
        port: url.port_or_known_default(),
        host,
        shown,
    }
}

/// Where a login was saved for, read from the address the entry keeps. The
/// field is free text: `github.com`, `https://github.com/login` and
/// `www.github.com` all name the same place. Only web addresses count; an
/// app's `android://` identity names no site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSite {
    host: String,
    /// Only when the address names one other than its scheme's default.
    port: Option<u16>,
    pub shown: String,
}

pub fn saved_site(address: &str) -> Option<SavedSite> {
    let address = address.trim();
    if address.is_empty() {
        return None;
    }
    let url = match address.split_once("://") {
        Some((scheme, _)) => {
            if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
                return None;
            }
            Url::parse(address).ok()?
        }
        None => Url::parse(&format!("https://{address}")).ok()?,
    };
    let host = url.host_str().filter(|h| !h.is_empty())?.to_string();
    let site = site_of(&url);
    Some(SavedSite {
        host,
        port: url.port(),
        shown: site.shown,
    })
}

impl SavedSite {
    /// The whole matching rule. The hosts are equal, or one is `www.` plus
    /// the other; nothing else, so no parent domains and no look-alikes. A
    /// port the address names must be the tab's port.
    pub fn matches(&self, tab: &Site) -> bool {
        let same_host = self.host == tab.host
            || self.host.strip_prefix("www.") == Some(tab.host.as_str())
            || tab.host.strip_prefix("www.") == Some(self.host.as_str());
        same_host && self.port.is_none_or(|port| Some(port) == tab.port)
    }
}

/// The sentence the confirmation shows when a login is filled somewhere it
/// was not saved for.
pub fn mismatch_wording(saved: Option<&SavedSite>, tab: &Site) -> String {
    match saved {
        Some(saved) => format!(
            "This login was saved for {}, not for {}.",
            saved.shown, tab.shown
        ),
        None => format!(
            "This login has no saved address, so nothing ties it to {}.",
            tab.shown
        ),
    }
}

/// The logins saved for a site, by label.
pub fn logins_for<'a>(logins: &'a [Login], tab: &Site) -> Vec<&'a Login> {
    let mut found: Vec<&Login> = logins
        .iter()
        .filter(|login| saved_site(&login.url).is_some_and(|saved| saved.matches(tab)))
        .collect();
    found.sort_by_key(|login| login.label.to_lowercase());
    found
}

/// Logins whose label or username contains the query, ignoring case. An
/// empty query finds nothing: the popup asks only once something is typed.
pub fn search<'a>(logins: &'a [Login], query: &str) -> Vec<&'a Login> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<&Login> = logins
        .iter()
        .filter(|login| {
            login.label.to_lowercase().contains(&query)
                || login.username.to_lowercase().contains(&query)
        })
        .collect();
    found.sort_by_key(|login| login.label.to_lowercase());
    found.truncate(SEARCH_LIMIT);
    found
}

/// Opaque tokens standing for entries, valid for one unlock of one silo.
/// Anything that closes, opens or switches a silo moves the epoch, and the
/// first request after that finds the table empty.
#[derive(Default)]
pub struct RefTable {
    scope: Option<(Uuid, u64)>,
    by_ref: HashMap<String, Uuid>,
    by_entry: HashMap<Uuid, String>,
}

/// Bounds the table for a session left open for days.
const MAX_REFS: usize = 4096;

impl RefTable {
    fn rescope(&mut self, silo: Uuid, epoch: u64) {
        if self.scope != Some((silo, epoch)) || self.by_ref.len() >= MAX_REFS {
            self.by_ref.clear();
            self.by_entry.clear();
            self.scope = Some((silo, epoch));
        }
    }

    pub fn issue(&mut self, silo: Uuid, epoch: u64, entry: Uuid) -> String {
        self.rescope(silo, epoch);
        if let Some(existing) = self.by_entry.get(&entry) {
            return existing.clone();
        }
        let token = hex::encode(rand::random::<[u8; 16]>());
        self.by_ref.insert(token.clone(), entry);
        self.by_entry.insert(entry, token.clone());
        token
    }

    pub fn resolve(&mut self, silo: Uuid, epoch: u64, reference: &str) -> Option<Uuid> {
        if self.scope != Some((silo, epoch)) {
            self.by_ref.clear();
            self.by_entry.clear();
            return None;
        }
        self.by_ref.get(reference).copied()
    }
}

pub fn error_answer(id: &str, failure: &Failure) -> Vec<u8> {
    serde_json::json!({
        "id": id,
        "type": "error",
        "code": failure.code.as_str(),
        "message": failure.message,
    })
    .to_string()
    .into_bytes()
}

#[derive(Serialize)]
pub struct StatusAnswer<'a> {
    pub id: &'a str,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silo: Option<&'a str>,
    pub version: &'a str,
}

pub fn status_answer(id: &str, state: &'static str, silo: Option<&str>, version: &str) -> Vec<u8> {
    serde_json::to_vec(&StatusAnswer {
        id,
        kind: "status",
        state,
        silo,
        version,
    })
    .unwrap_or_default()
}

#[derive(Serialize)]
pub struct Item<'a> {
    #[serde(rename = "ref")]
    pub reference: String,
    pub label: &'a str,
    pub username: &'a str,
}

#[derive(Serialize)]
struct ListAnswer<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<&'a str>,
    logins: &'a [Item<'a>],
}

/// A `logins` answer (with the origin it was asked for) or a `search` one.
/// Trimmed from the end in the unlikely case it would not fit a frame.
pub fn list_answer(id: &str, kind: &'static str, origin: Option<&str>, items: &[Item]) -> Vec<u8> {
    let mut count = items.len();
    loop {
        let body = serde_json::to_vec(&ListAnswer {
            id,
            kind,
            origin,
            logins: &items[..count],
        })
        .unwrap_or_default();
        if body.len() <= MAX_FRAME || count == 0 {
            return body;
        }
        count -= 1;
    }
}

#[derive(Serialize)]
struct FillAnswer<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    username: &'a str,
    password: &'a str,
}

/// The one answer that carries a secret. Written into a buffer sized up
/// front, so serialising never reallocates and leaves a copy behind; the
/// pipe wipes the buffer once it is written.
pub fn fill_answer(id: &str, username: &str, password: &str) -> Vec<u8> {
    // Every character escaped as \uXXXX is the worst case.
    let mut out = Vec::with_capacity(128 + 6 * (id.len() + username.len() + password.len()));
    let _ = serde_json::to_writer(
        &mut out,
        &FillAnswer {
            id,
            kind: "fill",
            username,
            password,
        },
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(origin: &str) -> Site {
        parse_origin(origin).unwrap().unwrap()
    }

    fn saved_for(address: &str, origin: &str) -> bool {
        saved_site(address).is_some_and(|s| s.matches(&tab(origin)))
    }

    #[test]
    fn https_and_loopback_are_fillable() {
        assert_eq!(tab("https://github.com").shown, "github.com");
        assert_eq!(tab("https://github.com/").shown, "github.com");
        assert_eq!(tab("http://localhost:3000").shown, "localhost:3000");
        assert_eq!(tab("http://127.0.0.1:8080").shown, "127.0.0.1:8080");
        assert_eq!(tab("http://[::1]").shown, "[::1]");
        assert_eq!(tab("https://example.com:8443").shown, "example.com:8443");
    }

    #[test]
    fn other_schemes_are_well_formed_but_never_filled() {
        for origin in [
            "http://example.com",
            "http://192.168.1.1",
            "http://localhost.example.com",
            "ftp://example.com",
            "chrome://settings",
            "file:///C:/",
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert_eq!(parse_origin(origin), Ok(None), "{origin}");
        }
    }

    #[test]
    fn anything_but_an_origin_is_malformed() {
        for origin in [
            "",
            "github.com",
            "https://github.com/login",
            "https://github.com/?q=1",
            "https://user:pw@github.com",
            "https://github.com/#x",
            "https://",
        ] {
            assert_eq!(parse_origin(origin), Err(()), "{origin}");
        }
    }

    #[test]
    fn a_host_matches_itself_and_its_www_twin() {
        assert!(saved_for("github.com", "https://github.com"));
        assert!(saved_for("https://github.com/login", "https://github.com"));
        assert!(saved_for("www.github.com", "https://github.com"));
        assert!(saved_for("github.com", "https://www.github.com"));
        assert!(saved_for("HTTPS://GitHub.com", "https://github.com"));
        assert!(saved_for("http://github.com", "https://github.com"));
    }

    #[test]
    fn nothing_else_matches() {
        assert!(!saved_for("github.com", "https://gist.github.com"));
        assert!(!saved_for("gist.github.com", "https://github.com"));
        assert!(!saved_for("paypal.com", "https://paypal.com.evil.example"));
        assert!(!saved_for("paypal.com", "https://secure-paypal.com"));
        assert!(!saved_for("www.www.github.com", "https://github.com"));
        assert!(!saved_for("m.github.com", "https://www.github.com"));
        assert!(!saved_for("", "https://github.com"));
        assert!(!saved_for(
            "android://cert@com.github.android",
            "https://github.com"
        ));
        assert!(!saved_for("ftp://github.com", "https://github.com"));
    }

    #[test]
    fn a_saved_port_must_be_the_tab_port() {
        assert!(saved_for("localhost:3000", "http://localhost:3000"));
        assert!(!saved_for("localhost:3000", "http://localhost:4000"));
        assert!(!saved_for("localhost:3000", "http://localhost"));
        assert!(saved_for("https://example.com:443", "https://example.com"));
        assert!(!saved_for("example.com:8443", "https://example.com"));
        assert!(saved_for("example.com", "https://example.com:8443"));
    }

    #[test]
    fn the_mismatch_names_both_sites() {
        let saved = saved_site("https://bank.example/login").unwrap();
        assert_eq!(
            mismatch_wording(Some(&saved), &tab("https://bank-login.example")),
            "This login was saved for bank.example, not for bank-login.example."
        );
        assert!(mismatch_wording(None, &tab("https://bank.example")).contains("bank.example"));
    }

    #[test]
    fn requests_parse_to_their_types() {
        assert_eq!(
            parse_request(br#"{"id":"1","type":"status"}"#),
            Ok(("1".into(), Request::Status))
        );
        assert_eq!(
            parse_request(br#"{"id":"2","type":"logins","origin":"https://a.example"}"#),
            Ok((
                "2".into(),
                Request::Logins {
                    origin: "https://a.example".into()
                }
            ))
        );
        assert_eq!(
            parse_request(br#"{"id":"4","type":"fill","origin":"https://a.example","ref":"r"}"#),
            Ok((
                "4".into(),
                Request::Fill {
                    origin: "https://a.example".into(),
                    reference: "r".into()
                }
            ))
        );
    }

    #[test]
    fn a_bad_request_keeps_the_id_it_could_read() {
        let (id, failure) = parse_request(br#"{"id":"9","type":"delete"}"#).unwrap_err();
        assert_eq!((id.as_str(), failure.code), ("9", Code::BadRequest));
        let (id, _) = parse_request(br#"{"id":"9","type":"fill","origin":"x"}"#).unwrap_err();
        assert_eq!(id, "9");
        let (id, _) = parse_request(b"{").unwrap_err();
        assert_eq!(id, "");
    }

    #[test]
    fn a_ref_dies_with_its_unlock() {
        let (silo, entry) = (Uuid::new_v4(), Uuid::new_v4());
        let mut refs = RefTable::default();
        let token = refs.issue(silo, 1, entry);
        assert_eq!(refs.issue(silo, 1, entry), token, "stable within an unlock");
        assert_eq!(refs.resolve(silo, 1, &token), Some(entry));
        assert_eq!(
            refs.resolve(silo, 2, &token),
            None,
            "a lock or unlock moved the epoch"
        );
        assert_eq!(refs.resolve(silo, 1, &token), None, "and it stays gone");
    }

    #[test]
    fn a_ref_is_useless_in_another_silo() {
        let mut refs = RefTable::default();
        let token = refs.issue(Uuid::new_v4(), 1, Uuid::new_v4());
        assert_eq!(refs.resolve(Uuid::new_v4(), 1, &token), None);
        assert_eq!(refs.resolve(Uuid::new_v4(), 1, "not-a-ref"), None);
    }

    #[test]
    fn answers_have_the_contract_shapes() {
        let v: serde_json::Value =
            serde_json::from_slice(&status_answer("1", "unlocked", Some("Personal"), "1.2.0"))
                .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"id":"1","type":"status","state":"unlocked","silo":"Personal","version":"1.2.0"})
        );

        let items = [Item {
            reference: "c1f0".into(),
            label: "GitHub",
            username: "alex@example.com",
        }];
        let v: serde_json::Value = serde_json::from_slice(&list_answer(
            "2",
            "logins",
            Some("https://github.com"),
            &items,
        ))
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"id":"2","type":"logins","origin":"https://github.com",
                "logins":[{"ref":"c1f0","label":"GitHub","username":"alex@example.com"}]})
        );

        let v: serde_json::Value =
            serde_json::from_slice(&fill_answer("4", "alex", "p\"w\u{1}")).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"id":"4","type":"fill","username":"alex","password":"p\"w\u{1}"})
        );

        let v: serde_json::Value =
            serde_json::from_slice(&error_answer("4", &Failure::new(Code::Cancelled))).unwrap();
        assert_eq!(v["code"], "cancelled");
        assert_eq!(v["message"], "The fill was not confirmed.");
    }

    #[test]
    fn a_list_too_long_for_a_frame_is_trimmed() {
        let long = "x".repeat(1000);
        let items: Vec<Item> = (0..200)
            .map(|n| Item {
                reference: format!("{n:032}"),
                label: &long,
                username: "u",
            })
            .collect();
        let body = list_answer("3", "search", None, &items);
        assert!(body.len() <= MAX_FRAME);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["logins"].as_array().unwrap().len() < 200);
    }
}
