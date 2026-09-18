//! What the native messaging host decides on its own: who may start it,
//! what its manifest says, and the answers it gives when the app is not
//! there to give them. Everything else it relays without reading.

use std::path::Path;

/// The name the extension connects to, and the registry key the installer
/// writes for it.
pub const HOST_NAME: &str = "com.silentsilo.desktop";

/// The manifest's file name, written beside the host.
pub const MANIFEST_FILE: &str = "silentsilo-browser-host.json";

/// The allowed list, compiled in so the check needs no file that could be
/// swapped. `--write-manifest` writes the same list into the manifest.
const ALLOWED: &str = include_str!("../allowed-origins.json");

/// The extension origins allowed to start this host.
pub fn allowed_origins() -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(ALLOWED)
        .ok()
        .and_then(|v| {
            v.get("allowed_origins")?.as_array().map(|list| {
                list.iter()
                    .filter_map(|o| o.as_str())
                    .filter(|o| is_extension_origin(o))
                    .map(str::to_string)
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// `chrome-extension://<id>/` with a 32-letter id in `a`..`p`, the only
/// shape an allowed origin takes. Anything else, a wildcard included, is
/// never allowed.
pub fn is_extension_origin(origin: &str) -> bool {
    origin
        .strip_prefix("chrome-extension://")
        .and_then(|rest| rest.strip_suffix('/'))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
}

/// Whether the caller the browser named is on the list. Exact match only.
pub fn caller_allowed(caller: &str, allowed: &[String]) -> bool {
    is_extension_origin(caller) && allowed.iter().any(|a| a == caller)
}

/// The native messaging manifest for a host at `host_path`.
pub fn manifest(host_path: &Path, allowed: &[String]) -> String {
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "SilentSilo",
        "path": host_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": allowed,
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// An error answer in the protocol's shape.
pub fn error_answer(id: &str, code: &str, message: &str) -> Vec<u8> {
    serde_json::json!({ "id": id, "type": "error", "code": code, "message": message })
        .to_string()
        .into_bytes()
}

pub const NOT_RUNNING: &str = "SilentSilo is not running, or its browser extension setting is off.";
pub const TOO_LARGE: &str = "The request was too large.";
pub const MALFORMED: &str = "SilentSilo could not read the request.";

/// What the host answers a request when nothing listens on the pipe.
pub fn not_running_answer(request: &[u8]) -> Vec<u8> {
    match request_id(request) {
        Some(id) => error_answer(&id, "app-not-running", NOT_RUNNING),
        None => error_answer("", "bad-request", MALFORMED),
    }
}

/// The `id` of a request, when it is JSON with a string id.
pub fn request_id(request: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(request)
        .ok()?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/";

    #[test]
    fn the_shipped_list_is_well_formed() {
        let list = allowed_origins();
        assert!(!list.is_empty(), "allowed-origins.json names no extension");
        let raw: serde_json::Value = serde_json::from_str(ALLOWED).unwrap();
        let entries = raw["allowed_origins"].as_array().unwrap();
        assert_eq!(
            entries.len(),
            list.len(),
            "every entry must be chrome-extension://<32 letters a-p>/"
        );
    }

    #[test]
    fn a_listed_extension_is_let_in() {
        assert!(caller_allowed(DEV, &[DEV.to_string()]));
    }

    #[test]
    fn an_unlisted_extension_is_refused() {
        let other = "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/";
        assert!(!caller_allowed(other, &[DEV.to_string()]));
    }

    #[test]
    fn near_misses_are_refused() {
        let list = [DEV.to_string()];
        for caller in [
            "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic",
            "chrome-extension://ACGMIBDDHPNMAEGPEGJCIBEKCNIHPFIC/",
            "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/x",
            " chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/",
            "moz-extension://acgmibddhpnmaegpegjcibekcnihpfic/",
            "https://acgmibddhpnmaegpegjcibekcnihpfic/",
            "",
            "--write-manifest",
        ] {
            assert!(!caller_allowed(caller, &list), "{caller:?} got in");
        }
    }

    #[test]
    fn a_wildcard_in_the_list_lets_nobody_in() {
        let list = ["chrome-extension://*/".to_string()];
        assert!(!caller_allowed(DEV, &list));
        assert!(!is_extension_origin("chrome-extension://*/"));
    }

    #[test]
    fn the_manifest_names_the_host_and_the_list() {
        let text = manifest(
            Path::new(r"C:\Apps\SilentSilo\silentsilo-browser-host.exe"),
            &[DEV.to_string()],
        );
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["name"], "com.silentsilo.desktop");
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], r"C:\Apps\SilentSilo\silentsilo-browser-host.exe");
        assert_eq!(v["allowed_origins"], serde_json::json!([DEV]));
    }

    #[test]
    fn with_no_app_each_request_gets_its_own_refusal() {
        let answer = not_running_answer(br#"{"id":"7","type":"status"}"#);
        let v: serde_json::Value = serde_json::from_slice(&answer).unwrap();
        assert_eq!(v["id"], "7");
        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], "app-not-running");
    }

    #[test]
    fn a_request_without_an_id_is_malformed() {
        for request in [&b"not json"[..], br#"{"type":"status"}"#, br#"{"id":7}"#] {
            let v: serde_json::Value =
                serde_json::from_slice(&not_running_answer(request)).unwrap();
            assert_eq!(v["code"], "bad-request");
            assert_eq!(v["id"], "");
        }
    }
}
