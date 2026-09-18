//! What the native messaging host decides on its own: who may start it,
//! which app it may talk to, what its manifest says, and the answers it
//! gives when the app is not there to give them. Everything else it relays
//! without reading.

use std::path::{Path, PathBuf};

/// The name the extension connects to, and the registry key the installer
/// writes for it.
pub const HOST_NAME: &str = "com.silentsilo.desktop";

/// The manifest's file name, written beside the host.
pub const MANIFEST_FILE: &str = "silentsilo-browser-host.json";

/// The app's executable, installed beside the host.
pub const APP_EXE: &str = "SilentSilo.exe";

/// The store builds' ids, compiled in so the check needs no file that could
/// be swapped. `--write-manifest` writes the same list into the manifest.
const RELEASE: &str = include_str!("../allowed-origins.json");

/// The development build's id. Always compiled in, so a release can check
/// it does not let it in; only [`DEV_ALLOWED`] builds let it in.
const DEV: &str = include_str!("../allowed-origins.dev.json");

/// Whether this build lets the development extension in: debug builds, and
/// builds with the `dev-extension` feature. A release build never.
pub const DEV_ALLOWED: bool = cfg!(any(debug_assertions, feature = "dev-extension"));

fn origins_in(json: &str, keys: &[&str]) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| value.get(*key)?.as_array())
        .flatten()
        .filter_map(|o| o.as_str())
        .map(str::to_string)
        .collect()
}

/// The Chrome Web Store and Edge Add-ons ids, as written.
pub fn store_origins() -> Vec<String> {
    origins_in(RELEASE, &["chrome_web_store", "edge_add_ons"])
}

/// The development build's id, as written.
pub fn dev_origins() -> Vec<String> {
    origins_in(DEV, &["allowed_origins"])
}

/// The extension origins allowed to start this host.
pub fn allowed_origins() -> Vec<String> {
    let mut list = store_origins();
    if DEV_ALLOWED {
        list.extend(dev_origins());
    }
    list.retain(|o| is_extension_origin(o));
    list
}

/// What makes a host unfit to ship, empty when nothing does: it would let
/// the development id in (anyone can build an extension with it), or it
/// would let no extension in at all.
pub fn release_problems(store: &[String], dev: &[String], dev_allowed: bool) -> Vec<String> {
    let mut problems = Vec::new();
    if dev_allowed {
        problems.push(
            "this build lets the development extension in (a debug build, or the dev-extension feature)"
                .to_string(),
        );
    }
    if store.is_empty() {
        problems.push(
            "allowed-origins.json names no store id: the host would let no extension in"
                .to_string(),
        );
    }
    for origin in store {
        if dev.contains(origin) {
            problems.push(format!(
                "allowed-origins.json holds the development id {origin}"
            ));
        } else if !is_extension_origin(origin) {
            problems.push(format!(
                "allowed-origins.json has a malformed entry {origin:?}"
            ));
        }
    }
    problems
}

/// [`release_problems`] for this build, as `--check-release` reports it.
pub fn this_build_release_problems() -> Vec<String> {
    release_problems(&store_origins(), &dev_origins(), DEV_ALLOWED)
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

/// The browsers the installer registers the host with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Edge,
}

impl Browser {
    /// The name on the browser's Authenticode certificate.
    pub fn publisher(self) -> &'static str {
        match self {
            Browser::Chrome => "Google LLC",
            Browser::Edge => "Microsoft Corporation",
        }
    }
}

fn lower(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn under_one_of(dir: &Path, roots: &[PathBuf]) -> bool {
    let dir = lower(dir);
    roots.iter().any(|root| lower(root) == dir)
}

/// The browser whose executable is at `image`, when it sits where that
/// browser installs: `<root>\Google\Chrome\Application\chrome.exe`,
/// `<root>\Microsoft\Edge\Application\msedge.exe`, or a Beta, Dev or Canary
/// (SxS) channel of either, with `<root>` one of `roots`.
pub fn browser_at(image: &Path, roots: &[PathBuf]) -> Option<Browser> {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    let file = name(image)?;
    let application = image.parent()?;
    let channel = application.parent()?;
    let vendor = channel.parent()?;
    let root = vendor.parent()?;
    if name(application)?.as_str() != "application" || !under_one_of(root, roots) {
        return None;
    }
    let (browser, vendor_name, product) = match file.as_str() {
        "chrome.exe" => (Browser::Chrome, "google", "chrome"),
        "msedge.exe" => (Browser::Edge, "microsoft", "edge"),
        _ => return None,
    };
    let channel = name(channel)?;
    let known_channel = [
        product.to_string(),
        format!("{product} beta"),
        format!("{product} dev"),
        format!("{product} sxs"),
    ]
    .contains(&channel);
    (name(vendor)?.as_str() == vendor_name && known_channel).then_some(browser)
}

/// `cmd.exe` in a system directory: Chrome and Edge start a native host
/// through it unless a policy tells them to start it directly.
pub fn is_system_shell(image: &Path, system_dirs: &[PathBuf]) -> bool {
    image
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("cmd.exe"))
        && image
            .parent()
            .is_some_and(|dir| under_one_of(dir, system_dirs))
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
pub const NOT_OURS: &str = "The channel to SilentSilo is held by another program. Nothing was sent to it. Restart SilentSilo.";
pub const TOO_LARGE: &str = "The request was too large.";
pub const MALFORMED: &str = "SilentSilo could not read the request.";

/// What the host answers a request when it has no app to relay to:
/// `app-not-running`, with `message` saying why.
pub fn not_running_answer(request: &[u8], message: &str) -> Vec<u8> {
    match request_id(request) {
        Some(id) => error_answer(&id, "app-not-running", message),
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

#[cfg(windows)]
pub use checks::{expected_server, started_by_browser, verify_server};

/// The checks that need Windows: who started the host, and who serves the
/// pipe it opened.
#[cfg(windows)]
mod checks {
    use std::io;
    use std::os::windows::io::RawHandle;
    use std::path::PathBuf;

    use silentsilo_shell::win_process::{
        current_user_sid, image_path, install_roots, owner_sid, parent_pid, pipe_server_pid,
        same_file, signer, system_dirs, user_sid,
    };

    use super::{APP_EXE, Browser, browser_at, is_system_shell};

    fn text(e: io::Error) -> String {
        e.to_string()
    }

    /// The browser that started this host, through `cmd.exe` or directly:
    /// Chrome or Edge from where they install, running as this user, signed
    /// by Google or Microsoft. Anything else is refused.
    pub fn started_by_browser() -> Result<Browser, String> {
        let mut pid = parent_pid(std::process::id()).map_err(text)?;
        let mut image = image_path(pid).map_err(text)?;
        if is_system_shell(&image, &system_dirs()) {
            pid = parent_pid(pid).map_err(text)?;
            image = image_path(pid).map_err(text)?;
        }
        let browser = browser_at(&image, &install_roots())
            .ok_or_else(|| format!("started by {}, not by Chrome or Edge", image.display()))?;
        if user_sid(pid).map_err(text)? != current_user_sid().map_err(text)? {
            return Err("the browser runs as another user".into());
        }
        let signed = signer(&image).map_err(text)?;
        if signed.name != browser.publisher() {
            return Err(format!(
                "{} is signed by {}, not {}",
                image.display(),
                signed.name,
                browser.publisher()
            ));
        }
        Ok(browser)
    }

    /// Where the app must be: beside this host. Debug builds may name
    /// another executable, for the tests that stand in for the app.
    pub fn expected_server() -> io::Result<PathBuf> {
        #[cfg(debug_assertions)]
        if let Some(path) = std::env::var_os("SILENTSILO_BROWSER_HOST_TEST_SERVER") {
            return Ok(PathBuf::from(path));
        }
        Ok(std::env::current_exe()?.with_file_name(APP_EXE))
    }

    /// Whether the pipe this host opened is the app's: created by this
    /// user, served by a process of this user running `expected`. A pipe of
    /// the same name made by anyone else fails.
    pub fn verify_server(pipe: RawHandle, expected: &std::path::Path) -> Result<(), String> {
        let own = current_user_sid().map_err(text)?;
        let owner = owner_sid(pipe).map_err(text)?;
        if owner != own {
            return Err(format!("the pipe is owned by {owner}"));
        }
        let pid = pipe_server_pid(pipe).map_err(text)?;
        if user_sid(pid).map_err(text)? != own {
            return Err("the pipe is served by another user".into());
        }
        let image = image_path(pid).map_err(text)?;
        if !same_file(&image, expected) {
            return Err(format!(
                "the pipe is served by {}, not {}",
                image.display(),
                expected.display()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV_ID: &str = "chrome-extension://acgmibddhpnmaegpegjcibekcnihpfic/";
    const STORE_ID: &str = "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/";

    #[test]
    fn the_lists_are_well_formed() {
        for list in [store_origins(), dev_origins()] {
            for origin in &list {
                assert!(is_extension_origin(origin), "{origin:?}");
            }
        }
        assert_eq!(dev_origins(), [DEV_ID]);
    }

    /// Whatever else changes, the release list never names the id anyone
    /// can reproduce from the public dev key.
    #[test]
    fn the_release_list_never_holds_the_dev_id() {
        let store = store_origins();
        for dev in dev_origins() {
            assert!(!store.contains(&dev), "allowed-origins.json holds {dev}");
        }
        assert!(!RELEASE.contains("acgmibddhpnmaegpegjcibekcnihpfic"));
    }

    #[test]
    fn a_release_check_refuses_the_dev_id_an_empty_list_and_a_dev_build() {
        let dev = [DEV_ID.to_string()];
        let store = [STORE_ID.to_string()];
        assert!(release_problems(&store, &dev, false).is_empty());

        let with_dev = [STORE_ID.to_string(), DEV_ID.to_string()];
        let problems = release_problems(&with_dev, &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("development id")),
            "{problems:?}"
        );

        let problems = release_problems(&[], &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("no store id")),
            "{problems:?}"
        );

        let problems = release_problems(&store, &dev, true);
        assert!(
            problems.iter().any(|p| p.contains("dev-extension")),
            "{problems:?}"
        );

        let problems = release_problems(&["chrome-extension://*/".to_string()], &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("malformed")),
            "{problems:?}"
        );
    }

    /// Debug builds and test runs let the development build in; a release
    /// never does.
    #[test]
    fn the_dev_id_is_let_in_exactly_when_the_build_allows_it() {
        assert_eq!(allowed_origins().contains(&DEV_ID.to_string()), DEV_ALLOWED);
        assert_eq!(
            DEV_ALLOWED,
            cfg!(any(debug_assertions, feature = "dev-extension"))
        );
    }

    /// Runs under `cargo test --release`: a release build of this tree must
    /// pass its own check, which it cannot while the store lists are empty.
    #[cfg(not(any(debug_assertions, feature = "dev-extension")))]
    #[test]
    fn this_release_build_is_fit_to_ship() {
        assert_eq!(this_build_release_problems(), Vec::<String>::new());
    }

    #[test]
    fn a_listed_extension_is_let_in() {
        assert!(caller_allowed(DEV_ID, &[DEV_ID.to_string()]));
    }

    #[test]
    fn an_unlisted_extension_is_refused() {
        assert!(!caller_allowed(STORE_ID, &[DEV_ID.to_string()]));
    }

    #[test]
    fn near_misses_are_refused() {
        let list = [DEV_ID.to_string()];
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
        assert!(!caller_allowed(DEV_ID, &list));
        assert!(!is_extension_origin("chrome-extension://*/"));
    }

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
            PathBuf::from(r"C:\Users\a\AppData\Local"),
        ]
    }

    #[test]
    fn the_browsers_are_known_where_they_install() {
        for (path, browser) in [
            (
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                Browser::Chrome,
            ),
            (
                r"C:\Users\a\AppData\Local\Google\Chrome SxS\Application\chrome.exe",
                Browser::Chrome,
            ),
            (
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                Browser::Edge,
            ),
            (
                r"c:\program files (x86)\microsoft\edge beta\application\MSEDGE.EXE",
                Browser::Edge,
            ),
        ] {
            assert_eq!(
                browser_at(Path::new(path), &roots()),
                Some(browser),
                "{path}"
            );
        }
    }

    #[test]
    fn a_browser_anywhere_else_is_not_one() {
        for path in [
            r"C:\Users\a\Downloads\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Google\Chrome\chrome.exe",
            r"C:\Program Files\Google\Chromium\Application\chrome.exe",
            r"C:\Program Files\Microsoft\Chrome\Application\chrome.exe",
            r"C:\Program Files\Google\Chrome\Application\msedge.exe",
            r"C:\Program Files\Google\Chrome\Application\evil.exe",
            r"C:\Program Files\x\Google\Chrome\Application\chrome.exe",
            r"chrome.exe",
        ] {
            assert_eq!(browser_at(Path::new(path), &roots()), None, "{path}");
        }
    }

    #[test]
    fn only_the_system_cmd_is_the_browsers_shell() {
        let system = [PathBuf::from(r"C:\Windows\System32")];
        assert!(is_system_shell(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &system
        ));
        assert!(is_system_shell(
            Path::new(r"c:\windows\system32\CMD.EXE"),
            &system
        ));
        assert!(!is_system_shell(Path::new(r"C:\Temp\cmd.exe"), &system));
        assert!(!is_system_shell(
            Path::new(r"C:\Windows\System32\pwsh.exe"),
            &system
        ));
    }

    /// A test binary is started by cargo, not by a browser.
    #[cfg(windows)]
    #[test]
    fn this_test_was_not_started_by_a_browser() {
        let refusal = started_by_browser().unwrap_err();
        assert!(refusal.contains("not by Chrome or Edge"), "{refusal}");
    }

    #[test]
    fn the_manifest_names_the_host_and_the_list() {
        let text = manifest(
            Path::new(r"C:\Apps\SilentSilo\silentsilo-browser-host.exe"),
            &[STORE_ID.to_string()],
        );
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["name"], "com.silentsilo.desktop");
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], r"C:\Apps\SilentSilo\silentsilo-browser-host.exe");
        assert_eq!(v["allowed_origins"], serde_json::json!([STORE_ID]));
    }

    #[test]
    fn with_no_app_each_request_gets_its_own_refusal() {
        let answer = not_running_answer(br#"{"id":"7","type":"status"}"#, NOT_RUNNING);
        let v: serde_json::Value = serde_json::from_slice(&answer).unwrap();
        assert_eq!(v["id"], "7");
        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], "app-not-running");
        assert_eq!(v["message"], NOT_RUNNING);
    }

    #[test]
    fn a_request_without_an_id_is_malformed() {
        for request in [&b"not json"[..], br#"{"type":"status"}"#, br#"{"id":7}"#] {
            let v: serde_json::Value =
                serde_json::from_slice(&not_running_answer(request, NOT_RUNNING)).unwrap();
            assert_eq!(v["code"], "bad-request");
            assert_eq!(v["id"], "");
        }
    }
}
