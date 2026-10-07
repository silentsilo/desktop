//! The browser extension, on the app's side.
//!
//! The extension talks to `silentsilo-browser-host`, which relays to a pipe
//! served here while Settings > Browser extension is on. Requests are
//! answered from the focused silo's login entries only (see `logins.rs`,
//! the one module that reads the vault), and a fill waits for the user to
//! confirm in this window and pass the same key check as revealing a
//! protected entry. A save waits for the person too; the window writes the
//! entry, through the same path as an edit. Only the host installed beside the app may connect
//! (`ClientCheck`), and what it may ask is rationed (`limits.rs`). The
//! contract is `docs/PROTOCOL.md` in silentsilo/browser.

mod limits;
mod logins;
mod protocol;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use silentsilo_shell::browser_pipe::Frame;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::commands::fido::run_blocking;
use crate::state::AppState;
use limits::{Bucket, ConnectionLimits, Cooldown, FillRefused, ShowGate, ShowRefused};
use logins::{Login, Secret};
use protocol::{Code, Failure, Item, Request};

/// How long a fill waits for the person, key check included.
const FILL_TIMEOUT: Duration = Duration::from_secs(90);

/// How long a save waits: the person may fix the label or the username.
const SAVE_TIMEOUT: Duration = Duration::from_secs(120);

const GONE: &str = "This browser request is no longer waiting.";
const TOO_MANY: &str = "Too many requests from the browser. Wait a moment.";
const COOLING_DOWN: &str = "A fill was just declined. Wait a few seconds before asking again.";
const SHOWN_JUST_NOW: &str = "The window was just brought forward. Wait a few seconds.";

pub struct BrowserBridge {
    server: Mutex<Option<Server>>,
    refs: Mutex<protocol::RefTable>,
    /// The one fill waiting for confirmation. A second one is `busy`.
    pending: Mutex<Option<Pending>>,
    /// The one save waiting for the person. A fill and a save do not wait
    /// together: either makes the other `busy`.
    saving: Mutex<Option<PendingSave>>,
    /// `logins` and `search` across every connection.
    lookups: Mutex<Bucket>,
    /// `search` alone, across every connection.
    searches: Mutex<Bucket>,
    /// The last fills that sent a password, newest first, shown under
    /// Settings > Browser extension so one nobody meant stands out. Memory
    /// only: gone when the app quits.
    recent: Mutex<VecDeque<RecentFill>>,
    /// Set when a fill ends without being confirmed: declined, timed out, or
    /// its connection gone.
    cooldown: Mutex<Cooldown>,
    /// `fill` across every connection.
    fills: Mutex<Bucket>,
    /// The last `show` acted on, across every connection.
    shown: Mutex<ShowGate>,
}

impl Default for BrowserBridge {
    fn default() -> Self {
        Self {
            server: Mutex::default(),
            refs: Mutex::default(),
            pending: Mutex::default(),
            saving: Mutex::default(),
            lookups: Mutex::new(limits::lookups_overall()),
            searches: Mutex::new(limits::searches_overall()),
            recent: Mutex::default(),
            cooldown: Mutex::default(),
            fills: Mutex::new(limits::fills_overall()),
            shown: Mutex::default(),
        }
    }
}

/// What one pipe connection has used, kept for as long as it stays open.
#[derive(Default)]
pub struct Connection {
    limits: Mutex<ConnectionLimits>,
}

// Only Windows serves the pipe; elsewhere nothing builds one.
#[cfg_attr(not(windows), allow(dead_code))]
struct Server {
    stop: watch::Sender<bool>,
    alive: Arc<AtomicBool>,
}

struct Pending {
    prompt: FillPrompt,
    reply: Option<oneshot::Sender<Option<()>>>,
    /// A key check is under way for it, so a second click does not start
    /// another.
    verifying: bool,
}

/// What the confirmation shows. No secret: the password is read only after
/// the person confirmed and the key check passed.
#[derive(Clone, Serialize)]
pub struct FillPrompt {
    request_id: String,
    /// The tab's host, with its port when it has one.
    site: String,
    label: String,
    username: String,
    /// Set when the login was not saved for this site, in words.
    mismatch: Option<String>,
}

struct PendingSave {
    prompt: SavePrompt,
    /// `Some(updated)` once the window wrote the entry.
    reply: Option<oneshot::Sender<Option<bool>>>,
}

/// What the save dialog shows and writes. It carries the password, which
/// the window needs to write the entry, as it does for any edit.
#[derive(Clone, Serialize)]
pub struct SavePrompt {
    request_id: String,
    /// The tab's host, with its port when it has one.
    site: String,
    /// The tab's origin, for the new entry's address.
    url: String,
    /// What a new login is called unless the person changes it.
    label: String,
    #[serde(serialize_with = "plain")]
    username: Zeroizing<String>,
    #[serde(serialize_with = "plain")]
    password: Zeroizing<String>,
    /// The login saved for this site with this username, offered for update.
    existing: Option<SaveExisting>,
}

#[derive(Clone, Serialize)]
pub struct SaveExisting {
    id: String,
    label: String,
}

fn plain<S: serde::Serializer>(value: &Zeroizing<String>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(value)
}

/// A fill that sent a password: where, which login, when (Unix seconds).
#[derive(Serialize, Clone)]
pub struct RecentFill {
    /// Whose fill it was: shown only while that silo is open.
    #[serde(skip)]
    silo: Uuid,
    site: String,
    label: String,
    at: i64,
}

/// How many fills Settings lists.
const RECENT_FILLS: usize = 20;

#[derive(Serialize)]
pub struct ExtensionStatus {
    /// Windows only for now: the host and its registration are Windows's.
    supported: bool,
    /// Whether the host sits beside the app. A release built before the
    /// extension has a store id ships without it.
    bundled: bool,
    enabled: bool,
    running: bool,
    recent: Vec<RecentFill>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl BrowserBridge {
    fn running(&self) -> bool {
        lock(&self.server)
            .as_ref()
            .is_some_and(|server| server.alive.load(Ordering::SeqCst))
    }
}

const NOT_BUNDLED: &str = "The browser extension is not part of this build.";

/// Whether the host sits beside this executable. Without it nothing could
/// reach the channel, so it is never opened.
#[cfg(any(windows, target_os = "linux"))]
fn host_bundled() -> bool {
    std::env::current_exe()
        .map(|exe| {
            exe.with_file_name(silentsilo_shell::browser_pipe::HOST_EXE)
                .is_file()
        })
        .unwrap_or(false)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn host_bundled() -> bool {
    false
}

/// Where the extension's channel exists. macOS comes with its release.
const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));
const NOT_SUPPORTED: &str = "The browser extension is available on Windows and Linux for now.";

/// On Linux the browsers find the host through manifests in this user's
/// home, written by the host itself so its lists stay in one place. From an
/// AppImage the host is copied out first: the image's files go when it
/// exits.
#[cfg(target_os = "linux")]
fn install_manifests() -> Result<(), String> {
    use silentsilo_shell::browser_pipe::{install_host_copy, installed_host_path};
    install_host_copy().map_err(|e| format!("the browser host could not be set up: {e}"))?;
    let host = installed_host_path().map_err(|e| e.to_string())?;
    let status = std::process::Command::new(&host)
        .arg("--install-manifests")
        .status()
        .map_err(|e| format!("the browser host could not run: {e}"))?;
    if !status.success() {
        return Err("The browsers could not be told where SilentSilo is.".into());
    }
    Ok(())
}

/// Takes the manifests away again, so turning the extension off undoes
/// everything turning it on did.
#[cfg(target_os = "linux")]
fn remove_manifests() {
    if let Ok(host) = silentsilo_shell::browser_pipe::installed_host_path() {
        let _ = std::process::Command::new(host)
            .arg("--remove-manifests")
            .status();
    }
}

/// Opens the pipe at startup when the setting is on and the host is there.
pub fn start_if_enabled(app: &AppHandle) {
    if !SUPPORTED || !silentsilo_shell::browser_pipe::extension_enabled() || !host_bundled() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start(&app).await {
            crate::diagnostics::warn("browser", e);
        }
    });
}

#[cfg(any(windows, target_os = "linux"))]
async fn start(app: &AppHandle) -> Result<(), String> {
    use silentsilo_shell::browser_pipe::{ClientCheck, PipeServer};

    if !host_bundled() {
        return Err(NOT_BUNDLED.into());
    }
    #[cfg(target_os = "linux")]
    install_manifests()?;
    let bridge = app.state::<BrowserBridge>();
    if bridge.running() {
        return Ok(());
    }
    let (stop, stop_rx) = watch::channel(false);
    // Only the host beside this executable, signed like it in a release.
    let check = ClientCheck::host_beside_this_exe().map_err(|e| e.to_string())?;
    let server = PipeServer::bind(stop_rx, check).await.map_err(|e| {
        format!("SilentSilo could not open the connection to the browser extension: {e}")
    })?;
    let alive = Arc::new(AtomicBool::new(true));
    {
        let mut slot = lock(&bridge.server);
        if slot
            .as_ref()
            .is_some_and(|s| s.alive.load(Ordering::SeqCst))
        {
            return Ok(());
        }
        *slot = Some(Server {
            stop,
            alive: alive.clone(),
        });
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let handler = move |connection: Arc<Connection>, frame: Frame| {
            let app = handle.clone();
            async move { answer(&app, &connection, frame).await }
        };
        let warn = |warning: String| crate::diagnostics::warn("browser", warning);
        if let Err(e) = server.run(handler, warn).await {
            crate::diagnostics::warn("browser", format_args!("channel stopped: {e}"));
        }
        alive.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
async fn start(_app: &AppHandle) -> Result<(), String> {
    Err(NOT_SUPPORTED.into())
}

/// Closes the pipe. Connections end, and a fill still waiting is answered
/// with nothing: its connection is gone.
pub fn stop(app: &AppHandle) {
    let bridge = app.state::<BrowserBridge>();
    if let Some(server) = lock(&bridge.server).take() {
        let _ = server.stop.send(true);
    }
}

fn status_of(app: &AppHandle) -> ExtensionStatus {
    ExtensionStatus {
        supported: SUPPORTED,
        bundled: host_bundled(),
        enabled: silentsilo_shell::browser_pipe::extension_enabled(),
        running: app.state::<BrowserBridge>().running(),
        recent: recent_while_open(app),
    }
}

/// The fills of silos open now. Those of a silo that has locked since are
/// forgotten: Settings is reachable with every silo locked, and the list
/// says which login went to which site.
fn recent_while_open(app: &AppHandle) -> Vec<RecentFill> {
    let open = app.state::<AppState>().open_silo_ids();
    let bridge = app.state::<BrowserBridge>();
    let mut recent = lock(&bridge.recent);
    recent.retain(|fill| open.contains(&fill.silo));
    recent.iter().cloned().collect()
}

#[tauri::command]
pub async fn browser_extension_status(app: AppHandle) -> Result<ExtensionStatus, String> {
    Ok(status_of(&app))
}

/// The Settings toggle: saved, then acted on. Off means the pipe is gone.
#[tauri::command]
pub async fn browser_extension_set(
    app: AppHandle,
    enabled: bool,
) -> Result<ExtensionStatus, String> {
    if enabled && !SUPPORTED {
        return Err(NOT_SUPPORTED.into());
    }
    if enabled && !host_bundled() {
        return Err(NOT_BUNDLED.into());
    }
    silentsilo_shell::browser_pipe::set_extension_enabled(enabled).map_err(|e| e.to_string())?;
    if enabled {
        start(&app).await?;
    } else {
        stop(&app);
        #[cfg(target_os = "linux")]
        remove_manifests();
    }
    Ok(status_of(&app))
}

/// The fill waiting for confirmation, for a window that mounted after the
/// request arrived.
#[tauri::command(async)]
pub fn browser_fill_pending(app: AppHandle) -> Result<Option<FillPrompt>, String> {
    let bridge = app.state::<BrowserBridge>();
    let pending = lock(&bridge.pending);
    Ok(pending.as_ref().map(|p| p.prompt.clone()))
}

/// The person pressed Fill: the key check runs, and only when it passes is
/// the waiting request let through to read the password.
#[tauri::command]
pub async fn browser_fill_confirm(app: AppHandle, request_id: String) -> Result<(), String> {
    {
        let bridge = app.state::<BrowserBridge>();
        let mut slot = lock(&bridge.pending);
        match slot.as_mut() {
            Some(p) if p.prompt.request_id == request_id => {
                if p.verifying {
                    return Ok(());
                }
                p.verifying = true;
            }
            _ => return Err(GONE.into()),
        }
    }
    // The prompt names the site, so the key check says what it is for even
    // if the dialog behind it changed.
    let purpose = {
        let bridge = app.state::<BrowserBridge>();
        let slot = lock(&bridge.pending);
        slot.as_ref()
            .map(|p| format!("fill your {} login on {}", p.prompt.label, p.prompt.site))
            .unwrap_or_else(|| "fill a login in your browser".into())
    };
    let verified = crate::commands::vault::verify_presence(&app, &purpose).await;
    let bridge = app.state::<BrowserBridge>();
    let mut slot = lock(&bridge.pending);
    let Some(pending) = slot.as_mut().filter(|p| p.prompt.request_id == request_id) else {
        return Err(GONE.into());
    };
    pending.verifying = false;
    verified?;
    match pending.reply.take() {
        Some(reply) => reply.send(Some(())).map_err(|_| GONE.to_string()),
        None => Err(GONE.into()),
    }
}

#[tauri::command(async)]
pub fn browser_fill_cancel(app: AppHandle, request_id: String) -> Result<(), String> {
    let bridge = app.state::<BrowserBridge>();
    let mut slot = lock(&bridge.pending);
    if let Some(pending) = slot.as_mut().filter(|p| p.prompt.request_id == request_id)
        && let Some(reply) = pending.reply.take()
    {
        let _ = reply.send(None);
    }
    Ok(())
}

/// The save waiting for the person, for a window that mounted after the
/// request arrived.
#[tauri::command(async)]
pub fn browser_save_pending(app: AppHandle) -> Result<Option<SavePrompt>, String> {
    let bridge = app.state::<BrowserBridge>();
    let saving = lock(&bridge.saving);
    Ok(saving.as_ref().map(|p| p.prompt.clone()))
}

/// The window wrote the entry: a new one, or the password of `existing`.
#[tauri::command(async)]
pub fn browser_save_done(app: AppHandle, request_id: String, updated: bool) -> Result<(), String> {
    let bridge = app.state::<BrowserBridge>();
    let mut slot = lock(&bridge.saving);
    match slot
        .as_mut()
        .filter(|p| p.prompt.request_id == request_id)
        .and_then(|p| p.reply.take())
    {
        Some(reply) => reply.send(Some(updated)).map_err(|_| GONE.to_string()),
        None => Err(GONE.into()),
    }
}

#[tauri::command(async)]
pub fn browser_save_cancel(app: AppHandle, request_id: String) -> Result<(), String> {
    let bridge = app.state::<BrowserBridge>();
    let mut slot = lock(&bridge.saving);
    if let Some(reply) = slot
        .as_mut()
        .filter(|p| p.prompt.request_id == request_id)
        .and_then(|p| p.reply.take())
    {
        let _ = reply.send(None);
    }
    Ok(())
}

/// One request in, one answer out. Never logs what it was asked.
async fn answer(app: &AppHandle, connection: &Connection, frame: Frame) -> Vec<u8> {
    let mut bytes = match frame {
        Frame::Message(bytes) => bytes,
        Frame::TooLarge => {
            return protocol::error_answer(
                "",
                &Failure::with(Code::BadRequest, "The request was too large."),
            );
        }
    };
    let parsed = protocol::parse_request(&bytes);
    // A save carries a typed password: the frame goes as soon as it is read.
    zeroize::Zeroize::zeroize(&mut bytes);
    let (id, request) = match parsed {
        Ok(parsed) => parsed,
        Err((id, failure)) => return protocol::error_answer(&id, &failure),
    };
    let result = match request {
        Request::Status => Ok(status(app, &id).await),
        Request::Logins { origin } => match lookup_allowed(app, connection) {
            Ok(()) => list_logins(app, &id, &origin).await,
            Err(failure) => Err(failure),
        },
        Request::Search { query } => {
            match lookup_allowed(app, connection).and_then(|()| search_allowed(app)) {
                Ok(()) => search(app, &id, &query).await,
                Err(failure) => Err(failure),
            }
        }
        Request::Fill { origin, reference } => {
            fill(app, connection, &id, &origin, &reference).await
        }
        Request::Show => show(app, connection, &id),
        Request::Save {
            origin,
            username,
            password,
        } => save(app, connection, &id, &origin, username, password).await,
    };
    result.unwrap_or_else(|failure| protocol::error_answer(&id, &failure))
}

/// A `logins` or `search` within both this connection's ration and the
/// app-wide one.
fn lookup_allowed(app: &AppHandle, connection: &Connection) -> Result<(), Failure> {
    let now = Instant::now();
    let bridge = app.state::<BrowserBridge>();
    let mut mine = lock(&connection.limits);
    if limits::take_lookup(&mut mine.lookups, &mut lock(&bridge.lookups), now) {
        Ok(())
    } else {
        Err(Failure::with(Code::Busy, TOO_MANY))
    }
}

/// A `search` within its own app-wide ration, after the lookup ones.
fn search_allowed(app: &AppHandle) -> Result<(), Failure> {
    let bridge = app.state::<BrowserBridge>();
    if lock(&bridge.searches).take(Instant::now()) {
        Ok(())
    } else {
        Err(Failure::with(Code::Busy, TOO_MANY))
    }
}

/// Brings the window forward, on its unlock screen while the silo is
/// locked; the unlock happens there as always. The answer says nothing about
/// the app's state, and nothing waits for the unlock. Not kept on top.
fn show(app: &AppHandle, connection: &Connection, id: &str) -> Result<Vec<u8>, Failure> {
    let admitted = {
        let bridge = app.state::<BrowserBridge>();
        let mut mine = lock(&connection.limits);
        let mut overall = lock(&bridge.lookups);
        let mut gate = lock(&bridge.shown);
        limits::admit_show(&mut mine.lookups, &mut overall, &mut gate, Instant::now())
    };
    match admitted {
        Ok(()) => {}
        Err(ShowRefused::TooMany) => return Err(Failure::with(Code::Busy, TOO_MANY)),
        Err(ShowRefused::TooSoon) => return Err(Failure::with(Code::Busy, SHOWN_JUST_NOW)),
    }
    // Window calls belong on the main thread, where the single-instance
    // handler already runs.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || crate::commands::shell::show_main_window(&handle));
    Ok(protocol::show_answer(id))
}

enum Access {
    NoSilo,
    Locked,
    Open {
        silo: Uuid,
        name: String,
        epoch: u64,
    },
}

/// The focused silo, if it is unlocked. The extension sees the silo on
/// screen and no other, the same one the window shows.
fn access(app: &AppHandle) -> Access {
    let state = app.state::<AppState>();
    let epoch = state.epoch();
    let focused = lock(&state.active_silo).clone();
    match focused {
        Some(silo) if lock(&state.sessions).contains_key(&silo.id) => Access::Open {
            silo: silo.id,
            name: silo.name,
            epoch,
        },
        Some(_) => Access::Locked,
        None => {
            let known = crate::state::app_data_dir(app)
                .map(|dir| !silentsilo_vault::load_registry(&dir).silos.is_empty())
                .unwrap_or(true);
            if known {
                Access::Locked
            } else {
                Access::NoSilo
            }
        }
    }
}

/// [`access`] off the async workers: the sessions lock can be held for the
/// length of a sync pass's replay.
async fn access_now(app: &AppHandle) -> Access {
    let app = app.clone();
    run_blocking(move || Ok(access(&app)))
        .await
        .unwrap_or(Access::Locked)
}

async fn open(app: &AppHandle) -> Result<(Uuid, u64), Failure> {
    match access_now(app).await {
        Access::Open { silo, epoch, .. } => Ok((silo, epoch)),
        Access::Locked => Err(Code::Locked.into()),
        Access::NoSilo => Err(Code::NoSilo.into()),
    }
}

async fn status(app: &AppHandle, id: &str) -> Vec<u8> {
    let version = app.package_info().version.to_string();
    match access_now(app).await {
        Access::Open { name, .. } => protocol::status_answer(id, "unlocked", Some(&name), &version),
        Access::Locked => protocol::status_answer(id, "locked", None, &version),
        Access::NoSilo => protocol::status_answer(id, "no-silo", None, &version),
    }
}

/// Runs `read` against the silo's session on the blocking pool: the
/// sessions lock is shared with every command, and an async worker should
/// not wait on it.
async fn with_session<T: Send + 'static>(
    app: &AppHandle,
    silo: Uuid,
    read: impl FnOnce(&silentsilo_vault::VaultSession) -> Result<T, String> + Send + 'static,
) -> Result<T, Failure> {
    let app = app.clone();
    let result = run_blocking(move || {
        let state = app.state::<AppState>();
        let sessions = lock(&state.sessions);
        match sessions.get(&silo) {
            Some(session) => read(session).map(Some),
            None => Ok(None),
        }
    })
    .await;
    match result {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(Code::Locked.into()),
        // Unlocked but unreadable: saying `locked` would send the person to
        // unlock a silo that already is.
        Err(_) => Err(Code::ReadFailed.into()),
    }
}

/// The silo's logins, without their passwords.
async fn read_logins(app: &AppHandle, silo: Uuid) -> Result<Vec<Login>, Failure> {
    with_session(app, silo, logins::read).await
}

/// One login's username and password, read fresh once a fill is confirmed.
async fn read_secret(app: &AppHandle, silo: Uuid, entry: Uuid) -> Result<Secret, Failure> {
    with_session(app, silo, move |session| logins::secret(session, entry))
        .await?
        .ok_or(Failure::new(Code::UnknownRef))
}

fn items_answer(
    app: &AppHandle,
    id: &str,
    kind: &'static str,
    origin: Option<&str>,
    (silo, epoch): (Uuid, u64),
    found: &[&Login],
) -> Vec<u8> {
    let bridge = app.state::<BrowserBridge>();
    let mut refs = lock(&bridge.refs);
    // A search result says where it was saved, so the popup can name a
    // login meant for another site before anyone asks to fill it.
    let with_site = kind == "search";
    let items: Vec<Item> = found
        .iter()
        .map(|login| Item {
            reference: refs.issue(silo, epoch, login.id),
            label: &login.label,
            username: &login.username,
            site: with_site.then(|| protocol::saved_site_shown(&login.url)),
        })
        .collect();
    protocol::list_answer(id, kind, origin, &items)
}

async fn list_logins(app: &AppHandle, id: &str, origin: &str) -> Result<Vec<u8>, Failure> {
    let site = match protocol::parse_origin(origin) {
        Ok(Some(site)) => site,
        Ok(None) => return Ok(protocol::list_answer(id, "logins", Some(origin), &[])),
        Err(()) => {
            return Err(Failure::with(
                Code::BadRequest,
                "That is not a page address SilentSilo can read.",
            ));
        }
    };
    let scope = open(app).await?;
    let logins = read_logins(app, scope.0).await?;
    let found = protocol::logins_for(&logins, &site);
    Ok(items_answer(app, id, "logins", Some(origin), scope, &found))
}

async fn search(app: &AppHandle, id: &str, query: &str) -> Result<Vec<u8>, Failure> {
    let scope = open(app).await?;
    let logins = read_logins(app, scope.0).await?;
    let found = protocol::search(&logins, query);
    Ok(items_answer(app, id, "search", None, scope, &found))
}

/// Takes the pending slot back and closes the dialog, however the fill or
/// save ended: answered, declined, timed out, or its connection gone.
///
/// Any end that was not a confirmation starts the cooldown, including a
/// connection that went away. Started only on an explicit cancel, a client
/// could drop its connection and send the next fill at once, swapping the
/// dialog under a click aimed at the first one.
///
/// After a confirmation the window goes back to hidden or minimised if that
/// is how the dialog found it, and the window that was in front when the
/// request came in (the browser) gets the focus back, so the person is left
/// on the page.
struct PendingGuard {
    app: AppHandle,
    kind: Question,
    request_id: String,
    confirmed: bool,
    front: crate::front::Front,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Question {
    Fill,
    Save,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let bridge = self.app.state::<BrowserBridge>();
        let ours = match self.kind {
            Question::Fill => {
                let mut slot = lock(&bridge.pending);
                let ours = slot
                    .as_ref()
                    .is_some_and(|p| p.prompt.request_id == self.request_id);
                if ours {
                    *slot = None;
                }
                ours
            }
            Question::Save => {
                let mut slot = lock(&bridge.saving);
                let ours = slot
                    .as_ref()
                    .is_some_and(|p| p.prompt.request_id == self.request_id);
                if ours {
                    *slot = None;
                }
                ours
            }
        };
        if !self.confirmed {
            lock(&bridge.cooldown).start(Instant::now());
        }
        // Only for its own dialog: a newer one may already be on top.
        if ours {
            self.front.settle(&self.app, self.confirmed);
        }
        let ended = match self.kind {
            Question::Fill => "browser-fill-ended",
            Question::Save => "browser-save-ended",
        };
        let _ = self.app.emit(ended, &self.request_id);
    }
}

/// Whether the focused silo has a key check a fill can ask for, off the
/// async workers: it reads the silo's key file.
async fn authenticator_enrolled(app: &AppHandle) -> bool {
    let app = app.clone();
    run_blocking(move || Ok(crate::commands::vault::presence_check_enrolled(&app)))
        .await
        .unwrap_or(false)
}

/// Whether a fill may raise the dialog now: not while it is cooling down
/// after a cancel, not while another fill waits, and not past the rations
/// (`limits::admit_fill`).
fn fill_allowed(app: &AppHandle, connection: &Connection) -> Result<(), Failure> {
    let bridge = app.state::<BrowserBridge>();
    let busy = waiting(&bridge);
    let cooldown = lock(&bridge.cooldown);
    let mut mine = lock(&connection.limits);
    let mut overall = lock(&bridge.fills);
    match limits::admit_fill(
        &cooldown,
        busy,
        &mut mine.fills,
        &mut overall,
        Instant::now(),
    ) {
        Ok(()) => Ok(()),
        Err(FillRefused::CoolingDown) => Err(Failure::with(Code::Busy, COOLING_DOWN)),
        Err(FillRefused::Busy) => Err(Code::Busy.into()),
        Err(FillRefused::TooMany) => Err(Failure::with(Code::Busy, TOO_MANY)),
    }
}

/// Whether a fill or a save is waiting for the person.
fn waiting(bridge: &BrowserBridge) -> bool {
    lock(&bridge.pending).is_some() || lock(&bridge.saving).is_some()
}

/// Whether a save may raise its dialog: as a fill, but on the rations of
/// `logins`, since it reads nothing out.
fn save_allowed(app: &AppHandle, connection: &Connection) -> Result<(), Failure> {
    let bridge = app.state::<BrowserBridge>();
    let busy = waiting(&bridge);
    let cooldown = lock(&bridge.cooldown);
    let mut mine = lock(&connection.limits);
    let mut overall = lock(&bridge.lookups);
    match limits::admit_fill(
        &cooldown,
        busy,
        &mut mine.lookups,
        &mut overall,
        Instant::now(),
    ) {
        Ok(()) => Ok(()),
        Err(FillRefused::CoolingDown) => Err(Failure::with(Code::Busy, COOLING_DOWN)),
        Err(FillRefused::Busy) => Err(Code::Busy.into()),
        Err(FillRefused::TooMany) => Err(Failure::with(Code::Busy, TOO_MANY)),
    }
}

/// A login typed on a page, offered by the extension. The dialog shows it
/// and the window writes it; nothing is written unless the person presses
/// Save. No key check: saving reveals nothing.
async fn save(
    app: &AppHandle,
    connection: &Connection,
    id: &str,
    origin: &str,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
) -> Result<Vec<u8>, Failure> {
    let Ok(Some(site)) = protocol::parse_origin(origin) else {
        return Err(Failure::with(
            Code::BadRequest,
            "SilentSilo does not save logins for this page.",
        ));
    };
    let (silo, epoch) = open(app).await?;
    save_allowed(app, connection)?;
    let logins = read_logins(app, silo).await?;
    let existing = protocol::same_login(&logins, &site, &username).map(|login| SaveExisting {
        id: login.id.to_string(),
        label: login.label.clone(),
    });
    let prompt = SavePrompt {
        request_id: Uuid::new_v4().to_string(),
        site: site.shown.clone(),
        url: origin.to_string(),
        label: protocol::suggested_label(&site),
        username: Zeroizing::new(username.trim().to_string()),
        password,
        existing,
    };

    let (reply, decided) = oneshot::channel();
    {
        let bridge = app.state::<BrowserBridge>();
        // Both held, fill's first, as a fill takes them: checked apart, a
        // fill and a save arriving together could each find the other free.
        let filling = lock(&bridge.pending);
        let mut slot = lock(&bridge.saving);
        if filling.is_some() || slot.is_some() {
            return Err(Code::Busy.into());
        }
        *slot = Some(PendingSave {
            prompt: prompt.clone(),
            reply: Some(reply),
        });
    }
    let mut guard = PendingGuard {
        app: app.clone(),
        kind: Question::Save,
        request_id: prompt.request_id.clone(),
        confirmed: false,
        front: crate::front::Front::capture(app),
    };
    let _ = app.emit("browser-save-request", &prompt);
    drop(prompt);
    crate::front::Front::raise(app);

    let updated = wait_for(
        app,
        decided,
        epoch,
        SAVE_TIMEOUT,
        Failure::with(Code::Cancelled, "The login was not saved."),
    )
    .await?;
    guard.confirmed = true;
    Ok(protocol::save_answer(id, updated))
}

async fn fill(
    app: &AppHandle,
    connection: &Connection,
    id: &str,
    origin: &str,
    reference: &str,
) -> Result<Vec<u8>, Failure> {
    let Ok(Some(site)) = protocol::parse_origin(origin) else {
        return Err(Failure::with(
            Code::BadRequest,
            "SilentSilo does not fill this page.",
        ));
    };
    let (silo, epoch) = open(app).await?;
    // Said before the dialog, not after it: without a key or Windows Hello
    // the confirmation could never pass.
    if !authenticator_enrolled(app).await {
        return Err(Code::NoAuthenticator.into());
    }
    fill_allowed(app, connection)?;
    let entry = {
        let bridge = app.state::<BrowserBridge>();
        let mut refs = lock(&bridge.refs);
        refs.resolve(silo, epoch, reference)
            .ok_or(Failure::new(Code::UnknownRef))?
    };

    let prompt = {
        let logins = read_logins(app, silo).await?;
        let login = logins
            .iter()
            .find(|login| login.id == entry)
            .ok_or(Failure::new(Code::UnknownRef))?;
        let saved = protocol::saved_site(&login.url);
        let matches = saved.as_ref().is_some_and(|saved| saved.matches(&site));
        FillPrompt {
            request_id: Uuid::new_v4().to_string(),
            site: site.shown.clone(),
            label: login.label.clone(),
            username: login.username.clone(),
            mismatch: (!matches).then(|| protocol::mismatch_wording(saved.as_ref(), &site)),
        }
    };

    let (reply, decided) = oneshot::channel();
    {
        let bridge = app.state::<BrowserBridge>();
        let mut slot = lock(&bridge.pending);
        let saving = lock(&bridge.saving);
        if slot.is_some() || saving.is_some() {
            return Err(Code::Busy.into());
        }
        *slot = Some(Pending {
            prompt: prompt.clone(),
            reply: Some(reply),
            verifying: false,
        });
    }
    let mut guard = PendingGuard {
        app: app.clone(),
        kind: Question::Fill,
        request_id: prompt.request_id.clone(),
        confirmed: false,
        front: crate::front::Front::capture(app),
    };
    let _ = app.emit("browser-fill-request", &prompt);
    crate::front::Front::raise(app);

    wait_for(
        app,
        decided,
        epoch,
        FILL_TIMEOUT,
        Failure::new(Code::Cancelled),
    )
    .await?;
    guard.confirmed = true;

    // Confirmed. The silo must still be the one the ref was issued in, and
    // the login still there; only now is its password read.
    let (now_silo, now_epoch) = open(app).await?;
    if (now_silo, now_epoch) != (silo, epoch) {
        return Err(Code::UnknownRef.into());
    }
    // In the log before the password is read: an organisation's silo that
    // cannot record it is locked instead.
    crate::audit::record_off_thread(
        app,
        silo,
        crate::audit::event(crate::audit::codes::BROWSER_FILLED)
            .on(entry.to_string(), prompt.label.clone())
            .with("site", prompt.site.clone()),
    )
    .await
    .map_err(|_| Failure::new(Code::Locked))?;
    let secret = read_secret(app, silo, entry).await?;
    {
        let bridge = app.state::<BrowserBridge>();
        let mut recent = lock(&bridge.recent);
        recent.push_front(RecentFill {
            silo,
            site: prompt.site.clone(),
            label: prompt.label.clone(),
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
        });
        recent.truncate(RECENT_FILLS);
    }
    Ok(protocol::fill_answer(
        id,
        &secret.username,
        &secret.password,
    ))
}

/// Waits for the person's answer: `Some` goes on, `None` or a dropped
/// sender is `declined`. A silo that locks or changes ends the wait.
async fn wait_for<T>(
    app: &AppHandle,
    mut decided: oneshot::Receiver<Option<T>>,
    epoch: u64,
    timeout: Duration,
    declined: Failure,
) -> Result<T, Failure> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        tokio::select! {
            answer = &mut decided => {
                return match answer {
                    Ok(Some(value)) => Ok(value),
                    _ => Err(declined),
                };
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(Failure::with(Code::Cancelled, "Nothing was decided in time."));
                }
                if app.state::<AppState>().epoch() != epoch {
                    return Err(match access_now(app).await {
                        Access::Open { .. } => Code::UnknownRef,
                        Access::Locked => Code::Locked,
                        Access::NoSilo => Code::NoSilo,
                    }
                    .into());
                }
            }
        }
    }
}
