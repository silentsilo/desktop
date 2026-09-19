//! What the native messaging host decides on its own: who may start it,
//! which app it may talk to, what its manifests say, and the answers it
//! gives when the app is not there to give them. Everything else it relays
//! without reading.

use std::path::{Path, PathBuf};

/// The name the extension connects to, and the registry key the installer
/// writes for it.
pub const HOST_NAME: &str = "com.silentsilo.desktop";

/// The Chromium manifest's file name (Chrome, Edge, Brave), written beside
/// the host.
pub const MANIFEST_FILE: &str = "silentsilo-browser-host.json";

/// Firefox's manifest, beside the host too: it lists add-on ids in
/// `allowed_extensions` where Chromium lists origins.
pub const FIREFOX_MANIFEST_FILE: &str = "silentsilo-browser-host.firefox.json";

/// The app's executable, installed beside the host.
pub const APP_EXE: &str = "SilentSilo.exe";

/// The store builds' ids, compiled in so the check needs no file that could
/// be swapped. `--write-manifest` writes the same lists into the manifests.
const RELEASE: &str = include_str!("../allowed-origins.json");

/// The development build's id. Always compiled in, so a release can check
/// it does not let it in; only [`DEV_ALLOWED`] builds let it in.
const DEV: &str = include_str!("../allowed-origins.dev.json");

/// Whether this build lets the development extension in: debug builds, and
/// builds with the `dev-extension` feature. A release build never.
pub const DEV_ALLOWED: bool = cfg!(any(debug_assertions, feature = "dev-extension"));

fn entries_in(json: &str, keys: &[&str]) -> Vec<String> {
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

/// The Chrome Web Store and Edge Add-ons ids, as written. Brave installs
/// from the Chrome Web Store, so it has no list of its own.
pub fn store_origins() -> Vec<String> {
    entries_in(RELEASE, &["chrome_web_store", "edge_add_ons"])
}

/// The Firefox Add-ons (AMO) ids, as written.
pub fn firefox_store_ids() -> Vec<String> {
    entries_in(RELEASE, &["firefox_add_ons"])
}

/// The development build's id, as written.
pub fn dev_origins() -> Vec<String> {
    entries_in(DEV, &["allowed_origins"])
}

/// The Chromium extension origins allowed to start this host.
pub fn allowed_origins() -> Vec<String> {
    let mut list = store_origins();
    if DEV_ALLOWED {
        list.extend(dev_origins());
    }
    list.retain(|o| is_extension_origin(o));
    list
}

/// The Firefox add-on ids allowed to start this host. The development build
/// carries the same id, so there is no development list.
pub fn allowed_firefox_ids() -> Vec<String> {
    let mut list = firefox_store_ids();
    list.retain(|id| is_firefox_id(id));
    list
}

/// What makes a host unfit to ship, empty when nothing does: it would let
/// the development id in (anyone can build an extension with it), or it
/// would let no extension in at all. `chromium` is the Chrome Web Store and
/// Edge Add-ons lists together, `firefox` the Firefox Add-ons list.
pub fn release_problems(
    chromium: &[String],
    firefox: &[String],
    dev: &[String],
    dev_allowed: bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    if dev_allowed {
        problems.push(
            "this build lets the development extension in (a debug build, or the dev-extension feature)"
                .to_string(),
        );
    }
    if chromium.is_empty() && firefox.is_empty() {
        problems.push(
            "allowed-origins.json names no store id: the host would let no extension in"
                .to_string(),
        );
    }
    for origin in chromium {
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
    for id in firefox {
        if !is_firefox_id(id) {
            problems.push(format!(
                "allowed-origins.json has a malformed Firefox id {id:?}"
            ));
        }
    }
    problems
}

/// [`release_problems`] for this build, as `--check-release` reports it.
pub fn this_build_release_problems() -> Vec<String> {
    release_problems(
        &store_origins(),
        &firefox_store_ids(),
        &dev_origins(),
        DEV_ALLOWED,
    )
}

/// What a release does with the host.
#[derive(Debug, PartialEq, Eq)]
pub enum ReleaseVerdict {
    /// Built, bundled, signed and registered.
    Ship,
    /// No store id yet: the release goes out without the host.
    LeaveOut,
    /// Not fit to ship, for these reasons.
    Refuse(Vec<String>),
}

/// The rule `build-release-local.ps1` applies before it builds anything.
/// All store lists empty leaves the host out rather than block the release;
/// any one of them, Firefox's included, ships it.
pub fn release_verdict(
    chromium: &[String],
    firefox: &[String],
    dev: &[String],
    dev_allowed: bool,
) -> ReleaseVerdict {
    if chromium.is_empty() && firefox.is_empty() {
        return ReleaseVerdict::LeaveOut;
    }
    let problems = release_problems(chromium, firefox, dev, dev_allowed);
    if problems.is_empty() {
        ReleaseVerdict::Ship
    } else {
        ReleaseVerdict::Refuse(problems)
    }
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

/// A Firefox add-on id as MDN defines it: `name@domain` (at most 80
/// characters, letters, digits, `-`, `.` and `_`), or a GUID in braces.
pub fn is_firefox_id(id: &str) -> bool {
    let plain = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    };
    let email = id.len() <= 80
        && id
            .split_once('@')
            .is_some_and(|(name, domain)| plain(name) && !domain.is_empty() && plain(domain));
    let guid = id
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|g| {
            let groups: Vec<&str> = g.split('-').collect();
            groups.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
                && groups
                    .iter()
                    .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
        });
    email || guid
}

/// Whether the caller the browser named is on the list. Exact match only.
pub fn caller_allowed(caller: &str, allowed: &[String]) -> bool {
    is_extension_origin(caller) && allowed.iter().any(|a| a == caller)
}

/// The two kinds of browser, told apart by how they start a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Chrome, Edge, Brave: `host <chrome-extension://id/> --parent-window=<n>`.
    Chromium,
    /// Firefox: `host <manifest path> <add-on id>`.
    Firefox,
}

/// The kind of browser the arguments say started the host, when the
/// extension they name is allowed. A `chrome-extension://` first argument
/// is checked against the Chromium list only; otherwise the first argument
/// must be a manifest path and the second an id on the Firefox list.
pub fn allowed_caller(args: &[String], chromium: &[String], firefox: &[String]) -> Option<Engine> {
    let first = args.first()?;
    if first.starts_with("chrome-extension://") {
        return caller_allowed(first, chromium).then_some(Engine::Chromium);
    }
    let id = args.get(1)?;
    let manifest = first.to_ascii_lowercase().ends_with(".json") && !first.starts_with('-');
    (manifest && is_firefox_id(id) && firefox.iter().any(|a| a == id)).then_some(Engine::Firefox)
}

/// The browsers allowed to start the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Edge,
    Brave,
    Firefox,
}

impl Browser {
    /// The name on the browser's Authenticode certificate.
    pub fn publisher(self) -> &'static str {
        match self {
            Browser::Chrome => "Google LLC",
            Browser::Edge => "Microsoft Corporation",
            Browser::Brave => "Brave Software, Inc.",
            Browser::Firefox => "Mozilla Corporation",
        }
    }

    /// How this browser starts a host, and so which list its caller is on.
    pub fn engine(self) -> Engine {
        match self {
            Browser::Firefox => Engine::Firefox,
            _ => Engine::Chromium,
        }
    }
}

/// The registry keys the installer may write, by the `--registers` name.
/// Brave has none: on Windows it reads Chrome's key (then Chromium's), not
/// one under BraveSoftware.
pub fn registers_for(
    key: &str,
    chrome: &[String],
    edge: &[String],
    firefox: &[String],
    dev: &[String],
) -> Option<bool> {
    let any_origin = |list: &[String]| list.iter().any(|o| is_extension_origin(o));
    match key {
        "chrome" => Some(any_origin(chrome) || any_origin(dev)),
        "edge" => Some(any_origin(edge) || any_origin(dev)),
        "firefox" => Some(firefox.iter().any(|id| is_firefox_id(id))),
        _ => None,
    }
}

/// [`registers_for`] with this build's lists: whether the installer points
/// the named browser's key at the host. A browser with no allowed id gets
/// no key.
pub fn registers(key: &str) -> Option<bool> {
    let dev = if DEV_ALLOWED {
        dev_origins()
    } else {
        Vec::new()
    };
    registers_for(
        key,
        &entries_in(RELEASE, &["chrome_web_store"]),
        &entries_in(RELEASE, &["edge_add_ons"]),
        &firefox_store_ids(),
        &dev,
    )
}

fn lower(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

fn under_one_of(dir: &Path, roots: &[PathBuf]) -> bool {
    let dir = lower(dir);
    roots.iter().any(|root| lower(root) == dir)
}

/// Where Firefox installs under a root: release and Beta share the first.
const FIREFOX_DIRS: [&str; 3] = [
    "mozilla firefox",
    "firefox developer edition",
    "firefox nightly",
];

/// The browser whose executable is at `image`, when it sits where that
/// browser installs, with `<root>` one of `roots`:
/// `<root>\Google\Chrome\Application\chrome.exe`,
/// `<root>\Microsoft\Edge\Application\msedge.exe` (either with a Beta, Dev
/// or Canary (SxS) channel),
/// `<root>\BraveSoftware\Brave-Browser\Application\brave.exe` (or its
/// `-Beta`, `-Dev`, `-Nightly` channel), and
/// `<root>\Mozilla Firefox\firefox.exe` (or Firefox Developer Edition or
/// Firefox Nightly).
pub fn browser_at(image: &Path, roots: &[PathBuf]) -> Option<Browser> {
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    let file = name(image)?;
    if file == "firefox.exe" {
        let install = image.parent()?;
        let known = FIREFOX_DIRS.contains(&name(install)?.as_str());
        return (known && under_one_of(install.parent()?, roots)).then_some(Browser::Firefox);
    }
    let application = image.parent()?;
    let channel = application.parent()?;
    let vendor = channel.parent()?;
    let root = vendor.parent()?;
    if name(application)?.as_str() != "application" || !under_one_of(root, roots) {
        return None;
    }
    let (browser, vendor_name, channels) = match file.as_str() {
        "chrome.exe" => (
            Browser::Chrome,
            "google",
            ["chrome", "chrome beta", "chrome dev", "chrome sxs"],
        ),
        "msedge.exe" => (
            Browser::Edge,
            "microsoft",
            ["edge", "edge beta", "edge dev", "edge sxs"],
        ),
        "brave.exe" => (
            Browser::Brave,
            "bravesoftware",
            [
                "brave-browser",
                "brave-browser-beta",
                "brave-browser-dev",
                "brave-browser-nightly",
            ],
        ),
        _ => return None,
    };
    let known_channel = channels.contains(&name(channel)?.as_str());
    (name(vendor)?.as_str() == vendor_name && known_channel).then_some(browser)
}

/// `cmd.exe` in a system directory: Chromium browsers start a native host
/// through it unless a policy tells them to start it directly. Firefox
/// starts an `.exe` host directly.
pub fn is_system_shell(image: &Path, system_dirs: &[PathBuf]) -> bool {
    image
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("cmd.exe"))
        && image
            .parent()
            .is_some_and(|dir| under_one_of(dir, system_dirs))
}

/// The Chromium native messaging manifest for a host at `host_path`.
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

/// Firefox's native messaging manifest for a host at `host_path`.
pub fn firefox_manifest(host_path: &Path, allowed: &[String]) -> String {
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "SilentSilo",
        "path": host_path.to_string_lossy(),
        "type": "stdio",
        "allowed_extensions": allowed,
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

    use super::{APP_EXE, Browser, Engine, browser_at, is_system_shell};

    fn text(e: io::Error) -> String {
        e.to_string()
    }

    /// The browser that started this host, when it is one that starts
    /// `engine`'s kind of caller: Chrome, Edge or Brave (through `cmd.exe`
    /// or directly) for a Chromium origin, Firefox (always directly) for a
    /// Firefox add-on id. From where it installs, running as this user,
    /// signed by its publisher. Anything else is refused.
    pub fn started_by_browser(engine: Engine) -> Result<Browser, String> {
        let mut pid = parent_pid(std::process::id()).map_err(text)?;
        let mut image = image_path(pid).map_err(text)?;
        let through_shell = is_system_shell(&image, &system_dirs());
        if through_shell {
            pid = parent_pid(pid).map_err(text)?;
            image = image_path(pid).map_err(text)?;
        }
        let browser = browser_at(&image, &install_roots()).ok_or_else(|| {
            format!(
                "started by {}, not by Chrome, Edge, Brave or Firefox",
                image.display()
            )
        })?;
        if browser.engine() != engine {
            return Err(format!(
                "{browser:?} started the host for a {engine:?} caller"
            ));
        }
        if through_shell && browser == Browser::Firefox {
            return Err("Firefox starts the host directly, not through cmd.exe".into());
        }
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
    const FIREFOX_ID: &str = "browser@silentsilo.com";

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn the_lists_are_well_formed() {
        for list in [store_origins(), dev_origins()] {
            for origin in &list {
                assert!(is_extension_origin(origin), "{origin:?}");
            }
        }
        assert_eq!(dev_origins(), [DEV_ID]);
        for id in firefox_store_ids() {
            assert!(is_firefox_id(&id), "{id:?}");
        }
        assert_eq!(allowed_firefox_ids(), [FIREFOX_ID]);
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
        assert!(release_problems(&store, &[], &dev, false).is_empty());

        let with_dev = [STORE_ID.to_string(), DEV_ID.to_string()];
        let problems = release_problems(&with_dev, &[], &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("development id")),
            "{problems:?}"
        );

        let problems = release_problems(&[], &[], &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("no store id")),
            "{problems:?}"
        );

        let problems = release_problems(&store, &[], &dev, true);
        assert!(
            problems.iter().any(|p| p.contains("dev-extension")),
            "{problems:?}"
        );

        let problems = release_problems(&["chrome-extension://*/".to_string()], &[], &dev, false);
        assert!(
            problems.iter().any(|p| p.contains("malformed")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_firefox_id_alone_is_a_store_id() {
        let dev = s(&[DEV_ID]);
        assert!(release_problems(&[], &s(&[FIREFOX_ID]), &dev, false).is_empty());
        for bad in [
            DEV_ID,
            "*",
            "",
            "browser@",
            "a b@c.d",
            "chrome-extension://x/",
        ] {
            let problems = release_problems(&[], &s(&[bad]), &dev, false);
            assert!(
                problems.iter().any(|p| p.contains("malformed Firefox id")),
                "{bad:?}: {problems:?}"
            );
        }
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

    #[test]
    fn empty_store_lists_leave_the_host_out() {
        let dev = [DEV_ID.to_string()];
        assert_eq!(
            release_verdict(&[], &[], &dev, false),
            ReleaseVerdict::LeaveOut
        );
        assert_eq!(
            release_verdict(&[STORE_ID.to_string()], &[], &dev, false),
            ReleaseVerdict::Ship
        );
        for bad in [DEV_ID, "chrome-extension://*/"] {
            let verdict =
                release_verdict(&[STORE_ID.to_string(), bad.to_string()], &[], &dev, false);
            assert!(matches!(verdict, ReleaseVerdict::Refuse(_)), "{bad}");
        }
        assert!(matches!(
            release_verdict(&[STORE_ID.to_string()], &[], &dev, true),
            ReleaseVerdict::Refuse(_)
        ));
    }

    /// The Firefox Add-ons id counts as a store id: with it alone the host
    /// ships, and only Firefox is registered.
    #[test]
    fn the_firefox_id_alone_ships_the_host_for_firefox_only() {
        let dev = s(&[DEV_ID]);
        let firefox = s(&[FIREFOX_ID]);
        assert_eq!(
            release_verdict(&[], &firefox, &dev, false),
            ReleaseVerdict::Ship
        );
        assert!(matches!(
            release_verdict(&[], &s(&["not an id"]), &dev, false),
            ReleaseVerdict::Refuse(_)
        ));
        assert!(matches!(
            release_verdict(&[], &firefox, &dev, true),
            ReleaseVerdict::Refuse(_)
        ));
        for (key, expected) in [("chrome", false), ("edge", false), ("firefox", true)] {
            assert_eq!(
                registers_for(key, &[], &[], &firefox, &[]),
                Some(expected),
                "{key}"
            );
        }
        assert_eq!(registers_for("brave", &[], &[], &firefox, &[]), None);
    }

    #[test]
    fn each_key_is_written_only_for_its_own_list() {
        let chrome = s(&[STORE_ID]);
        assert_eq!(registers_for("chrome", &chrome, &[], &[], &[]), Some(true));
        assert_eq!(registers_for("edge", &chrome, &[], &[], &[]), Some(false));
        assert_eq!(registers_for("edge", &[], &chrome, &[], &[]), Some(true));
        assert_eq!(
            registers_for("firefox", &chrome, &chrome, &[], &[]),
            Some(false)
        );
        // The dev id is for unpacked Chromium builds, never Firefox.
        let dev = s(&[DEV_ID]);
        assert_eq!(registers_for("chrome", &[], &[], &[], &dev), Some(true));
        assert_eq!(registers_for("firefox", &[], &[], &[], &dev), Some(false));
        // Malformed entries register nothing.
        let bad = s(&["chrome-extension://*/"]);
        assert_eq!(registers_for("chrome", &bad, &[], &[], &[]), Some(false));
        assert_eq!(
            registers_for("firefox", &[], &[], &s(&["x"]), &[]),
            Some(false)
        );
    }

    /// Runs under `cargo test --release`: this tree either ships its host
    /// or leaves it out (no store id yet), and is never refused.
    #[cfg(not(any(debug_assertions, feature = "dev-extension")))]
    #[test]
    fn this_release_build_ships_the_host_or_leaves_it_out() {
        let verdict = release_verdict(
            &store_origins(),
            &firefox_store_ids(),
            &dev_origins(),
            DEV_ALLOWED,
        );
        assert!(!matches!(verdict, ReleaseVerdict::Refuse(_)), "{verdict:?}");
        if store_origins().is_empty() && firefox_store_ids().is_empty() {
            assert_eq!(verdict, ReleaseVerdict::LeaveOut);
        } else {
            assert_eq!(this_build_release_problems(), Vec::<String>::new());
        }
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
    fn firefox_ids_follow_mdn() {
        for id in [
            FIREFOX_ID,
            "@x",
            "a-b_c.d@e.f",
            "{daf44bf7-a45e-4450-979c-91cf07434c3d}",
        ] {
            assert!(is_firefox_id(id), "{id:?}");
        }
        let long = format!("{}@silentsilo.com", "a".repeat(70));
        for id in [
            "",
            "browser",
            "browser@",
            "a@b@c",
            "a b@c",
            "*@silentsilo.com",
            long.as_str(),
            "{daf44bf7-a45e-4450-979c-91cf07434c3}",
            "{daf44bf7a45e4450979c91cf07434c3d}",
            DEV_ID,
        ] {
            assert!(!is_firefox_id(id), "{id:?}");
        }
    }

    /// Chromium passes the origin first; Firefox passes its manifest path,
    /// then the add-on id. Each is checked against its own list only.
    #[test]
    fn each_argv_form_is_checked_against_its_own_list() {
        let chromium = s(&[STORE_ID]);
        let firefox = s(&[FIREFOX_ID]);
        let manifest = r"C:\Apps\SilentSilo\silentsilo-browser-host.firefox.json";
        assert_eq!(
            allowed_caller(&s(&[STORE_ID, "--parent-window=0"]), &chromium, &firefox),
            Some(Engine::Chromium)
        );
        assert_eq!(
            allowed_caller(&s(&[STORE_ID]), &chromium, &firefox),
            Some(Engine::Chromium)
        );
        assert_eq!(
            allowed_caller(&s(&[manifest, FIREFOX_ID]), &chromium, &firefox),
            Some(Engine::Firefox)
        );
        let no_json = manifest.trim_end_matches(".json");
        for refused in [
            // An id on the other list, or in the other position.
            s(&[manifest, STORE_ID]),
            s(&[FIREFOX_ID]),
            s(&["chrome-extension://browser@silentsilo.com/"]),
            // Not listed, or not quite the id.
            s(&[manifest, "other@silentsilo.com"]),
            s(&[manifest, "BROWSER@silentsilo.com"]),
            s(&[manifest, " browser@silentsilo.com"]),
            // The first argument is not a manifest path.
            s(&[no_json, FIREFOX_ID]),
            s(&["--write-manifest.json", FIREFOX_ID]),
            s(&["", FIREFOX_ID]),
            // Missing arguments.
            s(&[manifest]),
            s(&[]),
        ] {
            assert_eq!(
                allowed_caller(&refused, &chromium, &firefox),
                None,
                "{refused:?}"
            );
        }
        // A Chromium origin on the Firefox list still goes nowhere.
        assert_eq!(allowed_caller(&s(&[STORE_ID]), &[], &s(&[STORE_ID])), None);
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
            (
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                Browser::Brave,
            ),
            (
                r"C:\Users\a\AppData\Local\BraveSoftware\Brave-Browser-Nightly\Application\brave.exe",
                Browser::Brave,
            ),
            (
                r"C:\Program Files (x86)\BraveSoftware\Brave-Browser-Beta\Application\brave.exe",
                Browser::Brave,
            ),
            (
                r"C:\Program Files\Mozilla Firefox\firefox.exe",
                Browser::Firefox,
            ),
            (
                r"C:\Users\a\AppData\Local\Mozilla Firefox\firefox.exe",
                Browser::Firefox,
            ),
            (
                r"C:\Program Files\Firefox Developer Edition\firefox.exe",
                Browser::Firefox,
            ),
            (
                r"c:\program files\firefox nightly\FIREFOX.EXE",
                Browser::Firefox,
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
            r"C:\Program Files\Brave\Brave-Browser\Application\brave.exe",
            r"C:\Program Files\BraveSoftware\Brave-Origin\Application\brave.exe",
            r"C:\Program Files\BraveSoftware\Brave-Browser\brave.exe",
            r"C:\Program Files\Google\Chrome\Application\brave.exe",
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\chrome.exe",
            r"C:\Program Files\Mozilla Firefox\Application\firefox.exe",
            r"C:\Program Files\Mozilla\Firefox\firefox.exe",
            r"C:\Program Files\Tor Browser\firefox.exe",
            r"C:\Users\a\Downloads\Mozilla Firefox\firefox.exe",
            r"C:\Program Files\x\Mozilla Firefox\firefox.exe",
            r"C:\Program Files\Mozilla Firefox\chrome.exe",
            r"firefox.exe",
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

    /// Each browser is let in under its own publisher's name, and for its
    /// own kind of caller only.
    #[test]
    fn each_browser_has_its_publisher_and_its_engine() {
        for (browser, publisher, engine) in [
            (Browser::Chrome, "Google LLC", Engine::Chromium),
            (Browser::Edge, "Microsoft Corporation", Engine::Chromium),
            (Browser::Brave, "Brave Software, Inc.", Engine::Chromium),
            (Browser::Firefox, "Mozilla Corporation", Engine::Firefox),
        ] {
            assert_eq!(browser.publisher(), publisher);
            assert_eq!(browser.engine(), engine);
        }
    }

    /// A test binary is started by cargo, not by a browser.
    #[cfg(windows)]
    #[test]
    fn this_test_was_not_started_by_a_browser() {
        for engine in [Engine::Chromium, Engine::Firefox] {
            let refusal = started_by_browser(engine).unwrap_err();
            assert!(
                refusal.contains("not by Chrome, Edge, Brave or Firefox"),
                "{refusal}"
            );
        }
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
        assert!(v.get("allowed_extensions").is_none());
    }

    #[test]
    fn the_firefox_manifest_lists_add_on_ids() {
        let text = firefox_manifest(
            Path::new(r"C:\Apps\SilentSilo\silentsilo-browser-host.exe"),
            &[FIREFOX_ID.to_string()],
        );
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["name"], "com.silentsilo.desktop");
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], r"C:\Apps\SilentSilo\silentsilo-browser-host.exe");
        assert_eq!(v["allowed_extensions"], serde_json::json!([FIREFOX_ID]));
        assert!(v.get("allowed_origins").is_none());
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
