//! The SSH agent: keys kept in the focused silo sign for `ssh`, `git` and
//! the editors, and never leave the app. Off until Settings turns it on.
//!
//! `silentsilo_shell::ssh_agent_channel` listens and says who connected;
//! `proto` reads the requests; `keys` is the one module that reads the
//! vault. Every signature waits for the person in this window, unless they
//! allowed that key for that server (or for git) until the silo locks. The
//! design and its reasons are in docs/ARCHITECTURE.md, "SSH agent".

mod keys;
#[cfg(all(test, any(windows, target_os = "linux", target_os = "macos")))]
mod openssh_tests;
mod proto;
#[cfg(test)]
mod test_keys;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use silentsilo_shell::ssh_agent_channel::{self, BindError, Peer};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;

use crate::commands::fido::run_blocking;
use crate::state::AppState;
use proto::{Bound, Purpose, Request};

/// How long a signature waits for the person.
const SIGN_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a request waits for a locked silo to be unlocked.
const UNLOCK_WAIT: Duration = Duration::from_secs(60);
/// After a declined signature, the dialog stays down this long.
const COOLDOWN: Duration = Duration::from_secs(5);
/// Requests one connection may make in a burst, and how fast they come back.
const BURST: u32 = 30;
const REFILL: Duration = Duration::from_millis(500);
/// The same for every connection together: a new connection is no new ration.
const AGENT_BURST: u32 = 60;
/// Servers one connection may bind to, as OpenSSH's agent allows.
const MAX_BINDS: usize = 16;
/// After a locked silo was not unlocked for a request, later ones do not
/// bring the window up again for this long: a tool that fetches in the
/// background would otherwise raise it every time.
const RAISE_QUIET: Duration = Duration::from_secs(10 * 60);

const GONE: &str = "This SSH request is no longer waiting.";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where a remembered allowance holds.
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "host")]
enum Destination {
    /// One server, by its host key's fingerprint.
    Host(String),
    /// Git signing a commit or a tag.
    Git,
}

#[derive(Default)]
struct Allowances {
    /// The silo and unlock they were given in; anything else forgets them.
    scope: Option<(Uuid, u64)>,
    keys: HashSet<(Vec<u8>, Destination)>,
}

impl Allowances {
    fn holds(&mut self, scope: (Uuid, u64), key: &[u8], to: &Destination) -> bool {
        if self.scope != Some(scope) {
            self.keys.clear();
            self.scope = Some(scope);
        }
        self.keys.contains(&(key.to_vec(), to.clone()))
    }

    fn allow(&mut self, scope: (Uuid, u64), key: Vec<u8>, to: Destination) {
        if self.scope != Some(scope) {
            self.keys.clear();
            self.scope = Some(scope);
        }
        self.keys.insert((key, to));
    }
}

#[derive(Default)]
pub struct SshAgent {
    server: Mutex<Option<Server>>,
    /// Why the agent is not listening, for Settings.
    problem: Mutex<Option<String>>,
    /// The one signature waiting for the person.
    pending: Mutex<Option<Pending>>,
    allowances: Mutex<Allowances>,
    /// Set when a signature was declined or timed out.
    declined_at: Mutex<Option<Instant>>,
    /// Every connection's requests together.
    ration: Mutex<AgentRation>,
    /// When the window was last brought up for a locked silo and nobody
    /// unlocked it.
    unanswered_raise: Mutex<Option<Instant>>,
}

/// [`Ration`] for the whole agent.
struct AgentRation(Ration);

impl Default for AgentRation {
    fn default() -> Self {
        Self(Ration {
            left: AGENT_BURST,
            since: Instant::now(),
            burst: AGENT_BURST,
        })
    }
}

struct Server {
    stop: watch::Sender<bool>,
    alive: Arc<AtomicBool>,
}

struct Pending {
    prompt: SignPrompt,
    /// `Some(remember)` once the person said yes.
    reply: Option<oneshot::Sender<Option<bool>>>,
    require_reauth: bool,
    verifying: bool,
}

/// What the dialog shows. Nothing secret: the key is read only after a yes.
#[derive(Clone, Serialize)]
pub struct SignPrompt {
    request_id: String,
    /// The entry's name.
    key: String,
    /// The program that asked, and the one that started it, as paths.
    program: Option<String>,
    parent: Option<String>,
    /// The server's host key fingerprint, when the client bound the session.
    host: Option<String>,
    /// The user name the login is for, when it is a login.
    user: Option<String>,
    /// "git" when Git signs a commit or a tag; another namespace otherwise.
    namespace: Option<String>,
    /// Whether the dialog may offer to remember: only for a known server or
    /// for git, and never for a key that asks for a touch every time.
    can_remember: bool,
    /// The entry asks for Windows Hello or the security key as well.
    require_reauth: bool,
}

/// Per connection: the servers it was bound to, and its own ration.
#[derive(Default)]
pub struct Connection {
    binding: Mutex<Binding>,
    ration: Mutex<Ration>,
}

#[derive(Default)]
struct Binding {
    /// Session ids already bound: a repeat is refused, as OpenSSH does.
    sessions: Vec<(Vec<u8>, Bound)>,
    /// A bind arrived on a forwarded connection: nothing is signed on it.
    forwarded: bool,
    /// A bind did not verify: nothing is signed on it.
    broken: bool,
}

struct Ration {
    left: u32,
    since: Instant,
    burst: u32,
}

impl Default for Ration {
    fn default() -> Self {
        Self {
            left: BURST,
            since: Instant::now(),
            burst: BURST,
        }
    }
}

impl Ration {
    fn take(&mut self, now: Instant) -> bool {
        let refilled =
            (now.saturating_duration_since(self.since).as_millis() / REFILL.as_millis()) as u32;
        if refilled > 0 {
            self.left = self.left.saturating_add(refilled).min(self.burst);
            self.since = now;
        }
        if self.left == 0 {
            return false;
        }
        self.left -= 1;
        true
    }
}

#[derive(Serialize)]
pub struct AgentStatus {
    /// Windows, Linux and macOS.
    supported: bool,
    enabled: bool,
    running: bool,
    /// Why it is not listening, when it should be.
    problem: Option<String>,
    /// What ssh is pointed at: the pipe, or the socket's path.
    address: Option<String>,
}

const SUPPORTED: bool = cfg!(any(windows, target_os = "linux", target_os = "macos"));
const NOT_SUPPORTED: &str = "The SSH agent is not available on this system.";

fn address() -> Option<String> {
    #[cfg(windows)]
    return Some(ssh_agent_channel::PIPE_NAME.to_string());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return ssh_agent_channel::socket_path()
        .ok()
        .map(|p| p.display().to_string());
    #[allow(unreachable_code)]
    None
}

impl SshAgent {
    fn running(&self) -> bool {
        lock(&self.server)
            .as_ref()
            .is_some_and(|server| server.alive.load(Ordering::SeqCst))
    }
}

fn status_of(app: &AppHandle) -> AgentStatus {
    let agent = app.state::<SshAgent>();
    AgentStatus {
        supported: SUPPORTED,
        enabled: ssh_agent_channel::agent_enabled(),
        running: agent.running(),
        problem: lock(&agent.problem).clone(),
        address: address(),
    }
}

/// Listens at startup when the setting is on.
pub fn start_if_enabled(app: &AppHandle) {
    if !SUPPORTED || !ssh_agent_channel::agent_enabled() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start(&app).await {
            crate::diagnostics::warn("ssh-agent", e);
        }
    });
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
async fn start(app: &AppHandle) -> Result<(), String> {
    use ssh_agent_channel::AgentServer;

    let agent = app.state::<SshAgent>();
    if agent.running() {
        return Ok(());
    }
    let (stop, stop_rx) = watch::channel(false);
    let server = match AgentServer::bind(stop_rx).await {
        Ok(server) => server,
        Err(e) => {
            let message = bind_problem(&e);
            *lock(&agent.problem) = Some(message.clone());
            return Err(message);
        }
    };
    *lock(&agent.problem) = None;
    let alive = Arc::new(AtomicBool::new(true));
    *lock(&agent.server) = Some(Server {
        stop,
        alive: alive.clone(),
    });
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let handler = move |peer: Arc<Peer>, connection: Arc<Connection>, message: Vec<u8>| {
            let app = handle.clone();
            async move { answer(&app, &peer, &connection, message).await }
        };
        let warn = |warning: String| crate::diagnostics::warn("ssh-agent", warning);
        if let Err(e) = server.run(handler, warn).await {
            crate::diagnostics::warn("ssh-agent", format_args!("stopped: {e}"));
        }
        alive.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
async fn start(_app: &AppHandle) -> Result<(), String> {
    Err(NOT_SUPPORTED.into())
}

/// What Settings says when the address is taken.
fn bind_problem(error: &BindError) -> String {
    match error {
        BindError::Taken { holder } => {
            let named = holder
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_lowercase());
            if cfg!(windows) && named.as_deref() == Some("ssh-agent.exe") {
                "Windows' own OpenSSH Authentication Agent is running and holds the agent's pipe. Stop it and set it to Disabled in Services (as an administrator), then turn this on again.".into()
            } else if let Some(path) = holder {
                format!(
                    "Another program holds the SSH agent's address: {}. Close it, then turn this on again.",
                    path.display()
                )
            } else {
                "Another program holds the SSH agent's address. Close it, then turn this on again."
                    .into()
            }
        }
        BindError::Other(e) => format!("The SSH agent could not start: {e}"),
    }
}

/// Stops listening; a signature still waiting ends with its connection.
pub fn stop(app: &AppHandle) {
    let agent = app.state::<SshAgent>();
    if let Some(server) = lock(&agent.server).take() {
        let _ = server.stop.send(true);
    }
    lock(&agent.allowances).keys.clear();
}

#[tauri::command]
pub async fn ssh_agent_status(app: AppHandle) -> Result<AgentStatus, String> {
    Ok(status_of(&app))
}

#[tauri::command]
pub async fn ssh_agent_set(app: AppHandle, enabled: bool) -> Result<AgentStatus, String> {
    if enabled && !SUPPORTED {
        return Err(NOT_SUPPORTED.into());
    }
    ssh_agent_channel::set_agent_enabled(enabled).map_err(|e| e.to_string())?;
    if enabled {
        // A refusal stays in the status, where Settings shows it.
        let _ = start(&app).await;
    } else {
        stop(&app);
        *lock(&app.state::<SshAgent>().problem) = None;
    }
    Ok(status_of(&app))
}

/// Whether an SSH key entry's text can be used by the agent: `ok`,
/// `encrypted` (ask for the passphrase), `unsupported` (a kind it does not
/// sign with) or `unreadable`.
#[tauri::command(async)]
pub fn ssh_key_check(key: String) -> String {
    match keys::private_key(&key) {
        Ok(_) => "ok".into(),
        Err(keys::KeyError::Encrypted) => "encrypted".into(),
        Err(keys::KeyError::Unreadable) => "unreadable".into(),
        Err(keys::KeyError::Unsupported) => "unsupported".into(),
    }
}

/// The key without its passphrase, for the editor to store. The
/// passphrase is dropped here.
#[tauri::command(async)]
pub fn ssh_key_remove_passphrase(key: String, passphrase: String) -> Result<String, String> {
    let mut passphrase = zeroize::Zeroizing::new(passphrase);
    let open = keys::without_passphrase(&key, &passphrase);
    zeroize::Zeroize::zeroize(&mut *passphrase);
    open.map(|text| text.to_string())
}

#[tauri::command(async)]
pub fn ssh_sign_pending(app: AppHandle) -> Option<SignPrompt> {
    lock(&app.state::<SshAgent>().pending)
        .as_ref()
        .map(|p| p.prompt.clone())
}

/// The person pressed Sign. A key that asks for a touch runs the same
/// check as revealing it first.
#[tauri::command]
pub async fn ssh_sign_confirm(
    app: AppHandle,
    request_id: String,
    remember: bool,
) -> Result<(), String> {
    let reauth = {
        let agent = app.state::<SshAgent>();
        let mut slot = lock(&agent.pending);
        match slot.as_mut() {
            Some(p) if p.prompt.request_id == request_id => {
                if p.verifying {
                    return Ok(());
                }
                p.verifying = p.require_reauth;
                p.require_reauth
                    .then(|| format!("sign with your {} SSH key", p.prompt.key))
            }
            _ => return Err(GONE.into()),
        }
    };
    if let Some(purpose) = reauth {
        let verified = crate::commands::vault::verify_presence(&app, &purpose).await;
        let agent = app.state::<SshAgent>();
        if let Some(p) = lock(&agent.pending).as_mut() {
            p.verifying = false;
        }
        verified?;
    }
    let agent = app.state::<SshAgent>();
    let mut slot = lock(&agent.pending);
    let Some(pending) = slot.as_mut().filter(|p| p.prompt.request_id == request_id) else {
        return Err(GONE.into());
    };
    let remember = remember && pending.prompt.can_remember;
    match pending.reply.take() {
        Some(reply) => reply.send(Some(remember)).map_err(|_| GONE.to_string()),
        None => Err(GONE.into()),
    }
}

#[tauri::command(async)]
pub fn ssh_sign_cancel(app: AppHandle, request_id: String) {
    let agent = app.state::<SshAgent>();
    if let Some(reply) = lock(&agent.pending)
        .as_mut()
        .filter(|p| p.prompt.request_id == request_id)
        .and_then(|p| p.reply.take())
    {
        let _ = reply.send(None);
    }
}

/// One message in, one answer out. Anything not understood or not allowed
/// is a plain failure, which tells a client nothing.
async fn answer(
    app: &AppHandle,
    peer: &Peer,
    connection: &Connection,
    message: Vec<u8>,
) -> Vec<u8> {
    let now = Instant::now();
    if !lock(&connection.ration).take(now) || !lock(&app.state::<SshAgent>().ration).0.take(now) {
        return proto::failure();
    }
    match proto::parse(&message) {
        Request::Identities => identities(app, connection).await,
        Request::Sign { key, data, flags } => sign(app, peer, connection, key, data, flags)
            .await
            .unwrap_or_else(|_| proto::failure()),
        Request::SessionBind {
            host_key,
            session_id,
            signature,
            forwarding,
        } => {
            let mut binding = lock(&connection.binding);
            if binding.sessions.len() >= MAX_BINDS {
                return proto::failure();
            }
            let repeat = binding.sessions.iter().any(|(id, _)| *id == session_id);
            match proto::verify_bind(&host_key, &session_id, &signature, forwarding) {
                Some(bound) if !repeat => {
                    binding.forwarded |= bound.forwarding;
                    binding.sessions.push((session_id, bound));
                    proto::success()
                }
                _ => {
                    binding.broken = true;
                    proto::failure()
                }
            }
        }
        Request::Other => proto::failure(),
    }
}

/// The focused silo and its unlock, if it is open.
fn open_silo(app: &AppHandle) -> Option<(Uuid, u64)> {
    let state = app.state::<AppState>();
    let epoch = state.epoch();
    let id = lock(&state.active_silo).as_ref()?.id;
    lock(&state.sessions)
        .contains_key(&id)
        .then_some((id, epoch))
}

/// The open silo, or, when it is locked, the window brought to its unlock
/// screen and a wait for the person to unlock it.
async fn open_or_wait(app: &AppHandle) -> Option<(Uuid, u64)> {
    let check = {
        let app = app.clone();
        move || {
            let app = app.clone();
            async move {
                run_blocking(move || Ok(open_silo(&app)))
                    .await
                    .ok()
                    .flatten()
            }
        }
    };
    if let Some(open) = check().await {
        return Some(open);
    }
    let any = crate::state::app_data_dir(app)
        .map(|dir| !silentsilo_vault::load_registry(&dir).silos.is_empty())
        .unwrap_or(false);
    if !any {
        return None;
    }
    let agent = app.state::<SshAgent>();
    if lock(&agent.unanswered_raise).is_some_and(|at| at.elapsed() < RAISE_QUIET) {
        return None;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || crate::commands::shell::show_main_window(&handle));
    let deadline = Instant::now() + UNLOCK_WAIT;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Some(open) = check().await {
            *lock(&agent.unanswered_raise) = None;
            return Some(open);
        }
    }
    *lock(&agent.unanswered_raise) = Some(Instant::now());
    None
}

async fn with_session<T: Send + 'static>(
    app: &AppHandle,
    silo: Uuid,
    read: impl FnOnce(&silentsilo_vault::VaultSession) -> Result<T, String> + Send + 'static,
) -> Result<T, ()> {
    let app = app.clone();
    run_blocking(move || {
        let state = app.state::<AppState>();
        let sessions = lock(&state.sessions);
        match sessions.get(&silo) {
            Some(session) => read(session),
            None => Err("locked".into()),
        }
    })
    .await
    .map_err(|_| ())
}

async fn identities(app: &AppHandle, connection: &Connection) -> Vec<u8> {
    // A connection forwarded from a server, or one whose binding failed,
    // learns nothing: not the keys, not their names, and it does not bring
    // the window up.
    {
        let binding = lock(&connection.binding);
        if binding.forwarded || binding.broken {
            return proto::identities_answer(&[]);
        }
    }
    let Some((silo, _)) = open_or_wait(app).await else {
        return proto::identities_answer(&[]);
    };
    match with_session(app, silo, keys::offered).await {
        Ok(offered) => proto::identities_answer(
            &offered
                .into_iter()
                .map(|k| (k.blob.clone(), k.label.clone()))
                .collect::<Vec<_>>(),
        ),
        Err(()) => proto::identities_answer(&[]),
    }
}

/// Takes the pending slot back and closes the dialog however it ended.
struct PendingGuard {
    app: AppHandle,
    request_id: String,
    signed: bool,
    front: crate::front::Front,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let agent = self.app.state::<SshAgent>();
        let ours = {
            let mut slot = lock(&agent.pending);
            let ours = slot
                .as_ref()
                .is_some_and(|p| p.prompt.request_id == self.request_id);
            if ours {
                *slot = None;
            }
            ours
        };
        if !self.signed {
            *lock(&agent.declined_at) = Some(Instant::now());
        }
        if ours {
            self.front.settle(&self.app, self.signed);
        }
        let _ = self.app.emit("ssh-sign-ended", &self.request_id);
    }
}

async fn sign(
    app: &AppHandle,
    peer: &Peer,
    connection: &Connection,
    key: Vec<u8>,
    data: Vec<u8>,
    flags: u32,
) -> Result<Vec<u8>, ()> {
    // Where the signature goes. A connection forwarded from a server, or
    // one whose binding failed, signs nothing.
    let purpose = proto::purpose(&data);
    let host = {
        let binding = lock(&connection.binding);
        if binding.forwarded || binding.broken {
            return Err(());
        }
        match &purpose {
            Purpose::Login { .. } => proto::session_id_of(&data).and_then(|sid| {
                binding
                    .sessions
                    .iter()
                    .find(|(id, _)| id.as_slice() == sid)
                    .map(|(_, bound)| bound.host_fingerprint.clone())
            }),
            _ => None,
        }
    };
    let destination = match (&purpose, &host) {
        (Purpose::Sshsig { namespace }, _) if namespace == "git" => Some(Destination::Git),
        (Purpose::Login { .. }, Some(host)) => Some(Destination::Host(host.clone())),
        _ => None,
    };

    // An RSA request that does not ask for SHA-2 would be refused after the
    // dialog and the log: refused now instead.
    if proto::is_rsa(&key) && flags & (proto::RSA_SHA2_256 | proto::RSA_SHA2_512) == 0 {
        return Err(());
    }

    let scope = open_or_wait(app).await.ok_or(())?;
    let offered = with_session(app, scope.0, keys::offered).await?;
    let entry = offered.iter().find(|k| k.blob == key).ok_or(())?;

    let agent = app.state::<SshAgent>();
    let allowed = !entry.require_reauth
        && destination
            .as_ref()
            .is_some_and(|to| lock(&agent.allowances).holds(scope, &key, to));

    if !allowed {
        if lock(&agent.declined_at).is_some_and(|at| at.elapsed() < COOLDOWN) {
            return Err(());
        }
        let prompt = SignPrompt {
            request_id: Uuid::new_v4().to_string(),
            key: entry.label.clone(),
            program: peer.exe.as_ref().map(|p| p.display().to_string()),
            parent: peer.parent.as_ref().map(|p| p.display().to_string()),
            host: host.clone(),
            user: match &purpose {
                Purpose::Login { user } => Some(user.clone()),
                _ => None,
            },
            namespace: match &purpose {
                Purpose::Sshsig { namespace } => Some(namespace.clone()),
                _ => None,
            },
            can_remember: destination.is_some() && !entry.require_reauth,
            require_reauth: entry.require_reauth,
        };
        let (reply, decided) = oneshot::channel();
        {
            let mut slot = lock(&agent.pending);
            if slot.is_some() {
                return Err(());
            }
            *slot = Some(Pending {
                prompt: prompt.clone(),
                reply: Some(reply),
                require_reauth: entry.require_reauth,
                verifying: false,
            });
        }
        let mut guard = PendingGuard {
            app: app.clone(),
            request_id: prompt.request_id.clone(),
            signed: false,
            front: crate::front::Front::capture(app),
        };
        let _ = app.emit("ssh-sign-request", &prompt);
        crate::front::Front::raise(app);

        let remember = wait(app, decided, scope.1).await?;
        if remember && let Some(to) = destination.clone() {
            lock(&agent.allowances).allow(scope, key.clone(), to);
        }
        guard.signed = true;
    }

    // In the log before the key is read: an organisation's silo that cannot
    // record it signs nothing.
    let mut event = crate::audit::event(crate::audit::codes::SSH_SIGNED)
        .on(entry.entry.to_string(), entry.label.clone());
    if let Some(program) = peer.exe.as_ref().and_then(|p| p.file_name()) {
        event = event.with("program", program.to_string_lossy().into_owned());
    }
    if let Some(host) = &host {
        event = event.with("host", host.clone());
    }
    if let Purpose::Sshsig { namespace } = &purpose {
        event = event.with("for", namespace.clone());
    }
    crate::audit::record_off_thread(app, scope.0, event)
        .await
        .map_err(|_| ())?;

    let signature = with_session(app, scope.0, move |session| {
        keys::sign(session, &key, &data, flags)
    })
    .await?
    .ok_or(())?;
    Ok(proto::sign_answer(&signature))
}

/// Waits for the person: `Some(remember)` on a yes. A lock or a focus
/// change while waiting ends it.
async fn wait(
    app: &AppHandle,
    mut decided: oneshot::Receiver<Option<bool>>,
    epoch: u64,
) -> Result<bool, ()> {
    let deadline = tokio::time::Instant::now() + SIGN_TIMEOUT;
    loop {
        tokio::select! {
            answer = &mut decided => {
                return match answer {
                    Ok(Some(remember)) => Ok(remember),
                    _ => Err(()),
                };
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if tokio::time::Instant::now() >= deadline
                    || app.state::<AppState>().epoch() != epoch
                {
                    return Err(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allowance_dies_with_its_unlock() {
        let silo = Uuid::new_v4();
        let mut allowances = Allowances::default();
        let to = Destination::Host("SHA256:abc".into());
        allowances.allow((silo, 1), b"key".to_vec(), to.clone());
        assert!(allowances.holds((silo, 1), b"key", &to));
        assert!(!allowances.holds((silo, 1), b"key", &Destination::Git));
        assert!(!allowances.holds((silo, 1), b"other", &to));
        assert!(!allowances.holds((silo, 2), b"key", &to), "locked since");
        assert!(!allowances.holds((silo, 1), b"key", &to), "and stays gone");
    }

    #[test]
    fn a_connection_is_rationed_and_refills() {
        let start = Instant::now();
        let mut ration = Ration {
            left: BURST,
            since: start,
            burst: BURST,
        };
        for _ in 0..BURST {
            assert!(ration.take(start));
        }
        assert!(!ration.take(start));
        assert!(ration.take(start + REFILL));
    }
}
