//! Auto-type: a login typed into the window that was in front, for programs
//! the browser extension cannot reach. The flow and its checks are in
//! docs/ARCHITECTURE.md, "Auto-type"; the shortcut and the typing itself are
//! `silentsilo_shell::autotype`.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use silentsilo_shell::ForegroundWindow;
use silentsilo_shell::autotype::{Hotkey, HotkeyError, Step};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::err::coded;
use crate::front::Front;
use crate::state::AppState;

const SETTINGS_FILE: &str = "autotype.json";
/// How long the window has to come back in front before the type is dropped.
const BACK_IN_FRONT: std::time::Duration = std::time::Duration::from_millis(1500);

const GONE: &str = coded!(
    "err.autotype_gone",
    "This auto-type is no longer waiting. Press the shortcut again."
);
const NOT_IN_FRONT: &str = coded!(
    "err.autotype_not_in_front",
    "The window to type into did not come back in front, so nothing was typed."
);
const ELEVATED: &str = coded!(
    "err.autotype_elevated",
    "That window runs as administrator, and Windows does not let SilentSilo type into it."
);
const KEYS_HELD: &str = coded!(
    "err.autotype_keys_held",
    "A key was still held down, so nothing was typed. Release it and try again."
);
const REFUSED: &str = coded!(
    "err.autotype_refused",
    "The system did not take the typed text."
);
const NO_ACCESS: &str = coded!(
    "err.autotype_no_access",
    "macOS did not let SilentSilo type. Allow it under System Settings > Privacy & Security > Accessibility, then try again."
);
const SECURE_INPUT: &str = coded!(
    "err.autotype_secure_input",
    "Another program has secure keyboard entry on, and macOS lets nothing else type while it does. Turn it off there (in Terminal: Terminal > Secure Keyboard Entry) and try again."
);
const NOT_A_LOGIN: &str = coded!(
    "err.autotype_not_a_login",
    "That entry has no password to type."
);

/// The person's choices, kept on this computer.
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Press Enter after the password.
    pub enter: bool,
}

/// What the window shows while an auto-type waits for a login.
#[derive(Serialize, Clone)]
pub struct Prompt {
    pub request_id: String,
    /// The target window's title, as its program sets it.
    pub title: String,
    /// The program's file name ("putty.exe").
    pub program: String,
    /// Windows would drop the input: the window says so and offers no Type.
    pub elevated: bool,
}

struct Pending {
    prompt: Prompt,
    target: ForegroundWindow,
    process_id: u32,
    front: Front,
    verifying: bool,
}

#[derive(Default)]
pub struct AutoType {
    hotkey: Mutex<Option<Hotkey>>,
    pending: Mutex<Option<Pending>>,
}

/// The settings and whether the shortcut is held, for Settings > General.
#[derive(Serialize)]
pub struct Status {
    pub supported: bool,
    pub enabled: bool,
    pub enter: bool,
    /// Turned on, but another program holds Ctrl+Alt+A.
    pub taken: bool,
    /// The system lets SilentSilo type. Only a Mac can say no, until the
    /// person allows it under Accessibility.
    pub access: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::state::app_data_dir(app)
        .ok()
        .map(|d| d.join(SETTINGS_FILE))
}

fn load(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, settings: Settings) -> Result<(), String> {
    let path = settings_path(app).ok_or("No app data folder.")?;
    let raw = serde_json::to_vec(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
}

const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

fn status(app: &AppHandle) -> Status {
    let settings = load(app);
    let held = lock(&app.state::<AutoType>().hotkey).is_some();
    Status {
        supported: SUPPORTED,
        enabled: settings.enabled,
        enter: settings.enter,
        taken: SUPPORTED && settings.enabled && !held,
        access: silentsilo_shell::autotype::access_granted(),
    }
}

/// Holds the shortcut when auto-type is on, lets it go when off.
fn apply(app: &AppHandle, settings: Settings) {
    let state = app.state::<AutoType>();
    let mut hotkey = lock(&state.hotkey);
    if !settings.enabled || !SUPPORTED {
        *hotkey = None;
        return;
    }
    if hotkey.is_some() {
        return;
    }
    let handle = app.clone();
    match silentsilo_shell::autotype::register_hotkey(move || pressed(&handle)) {
        Ok(registered) => *hotkey = Some(registered),
        Err(HotkeyError::Taken) => {
            crate::diagnostics::warn("autotype", "Ctrl+Alt+A is held by another program")
        }
        Err(HotkeyError::NotSupported) => {}
    }
}

/// At start: the shortcut, if auto-type was left on.
pub fn start(app: &AppHandle) {
    apply(app, load(app));
}

/// The shortcut was pressed: the window in front is the target. Nothing in
/// front but SilentSilo itself means nothing to type into.
fn pressed(app: &AppHandle) {
    let front = Front::capture(app);
    let Some(target) = front.caller() else {
        return;
    };
    let process_id = target.process_id();
    let prompt = Prompt {
        request_id: Uuid::new_v4().to_string(),
        title: target.title(),
        program: target.program(),
        elevated: target.is_elevated(),
    };
    let state = app.state::<AutoType>();
    *lock(&state.pending) = Some(Pending {
        prompt: prompt.clone(),
        target,
        process_id,
        front,
        verifying: false,
    });
    let _ = app.emit("autotype-request", &prompt);
    Front::raise(app);
}

#[tauri::command(async)]
pub fn autotype_status(app: AppHandle) -> Status {
    status(&app)
}

#[tauri::command(async)]
pub fn autotype_set(app: AppHandle, enabled: bool, enter: bool) -> Result<Status, String> {
    let settings = Settings { enabled, enter };
    save(&app, settings)?;
    apply(&app, settings);
    // A Mac asks once, at the moment the person turns it on.
    if enabled && !silentsilo_shell::autotype::access_granted() {
        silentsilo_shell::autotype::ask_for_access();
    }
    Ok(status(&app))
}

/// macOS: the Accessibility page of System Settings, where SilentSilo is
/// allowed to type.
#[tauri::command(async)]
pub fn autotype_open_access() {
    silentsilo_shell::autotype::open_access_settings();
}

/// The auto-type waiting for a login, for a window that mounted after it.
#[tauri::command(async)]
pub fn autotype_pending(app: AppHandle) -> Option<Prompt> {
    lock(&app.state::<AutoType>().pending)
        .as_ref()
        .map(|p| p.prompt.clone())
}

#[tauri::command(async)]
pub fn autotype_cancel(app: AppHandle, request_id: String) {
    let state = app.state::<AutoType>();
    let mut slot = lock(&state.pending);
    if slot
        .as_ref()
        .is_some_and(|p| p.prompt.request_id == request_id && !p.verifying)
        && let Some(pending) = slot.take()
    {
        pending.front.settle(&app, false);
    }
}

/// The person chose `entry` and pressed Type: the key check, the activity
/// log, focus back to the target, and only then the typing.
#[tauri::command]
pub async fn autotype_confirm(
    app: AppHandle,
    request_id: String,
    entry_id: String,
    label: String,
) -> Result<(), String> {
    let entry = Uuid::parse_str(&entry_id).map_err(|e| e.to_string())?;
    let program = {
        let state = app.state::<AutoType>();
        let mut slot = lock(&state.pending);
        match slot.as_mut() {
            Some(p) if p.prompt.request_id == request_id && !p.verifying => {
                if p.prompt.elevated {
                    return Err(ELEVATED.into());
                }
                p.verifying = true;
                p.prompt.program.clone()
            }
            _ => return Err(GONE.into()),
        }
    };
    let verified = crate::commands::vault::verify_presence(
        &app,
        crate::commands::vault::Presence::AutoType {
            label: label.clone(),
            program: program.clone(),
        },
    )
    .await;
    let pending = {
        let state = app.state::<AutoType>();
        let mut slot = lock(&state.pending);
        if let Err(e) = verified {
            if let Some(p) = slot.as_mut() {
                p.verifying = false;
            }
            return Err(e);
        }
        slot.take()
            .filter(|p| p.prompt.request_id == request_id)
            .ok_or_else(|| GONE.to_string())?
    };

    let silo = crate::state::focused_id(&app.state::<AppState>())?;
    // In the log before the secret is read, as every secret that leaves the
    // silo is: an organisation's silo that cannot record it locks instead.
    crate::audit::record_off_thread(
        &app,
        silo,
        crate::audit::event(crate::audit::codes::APP_FILLED)
            .on(entry.to_string(), label)
            .with("program", program),
    )
    .await?;

    let secret = {
        let handle = app.clone();
        crate::commands::fido::run_blocking(move || {
            let state = handle.state::<AppState>();
            let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
            let session = sessions
                .get(&silo)
                .ok_or_else(|| silentsilo_core::CoreError::VaultLocked.to_string())?;
            crate::browser::logins::secret(session, entry)
        })
        .await?
        .ok_or_else(|| NOT_A_LOGIN.to_string())?
    };
    let enter = load(&app).enter;

    pending.front.settle(&app, true);
    crate::commands::fido::run_blocking(move || {
        let deadline = std::time::Instant::now() + BACK_IN_FRONT;
        while !pending.target.is_in_front(pending.process_id) {
            if std::time::Instant::now() > deadline {
                return Err(NOT_IN_FRONT.to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut steps = Vec::new();
        if !secret.username.is_empty() {
            steps.push(Step::Text(Zeroizing::new(secret.username.to_string())));
            steps.push(Step::Tab);
        }
        steps.push(Step::Text(Zeroizing::new(secret.password.to_string())));
        if enter {
            steps.push(Step::Enter);
        }
        silentsilo_shell::autotype::type_steps(&steps).map_err(|e| {
            match e {
                silentsilo_shell::autotype::TypeError::KeysHeld => KEYS_HELD,
                silentsilo_shell::autotype::TypeError::NoAccess => NO_ACCESS,
                silentsilo_shell::autotype::TypeError::SecureInput => SECURE_INPUT,
                _ => REFUSED,
            }
            .to_string()
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_with_missing_fields_as_off() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.enabled && !s.enter);
        let s: Settings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(s.enabled && !s.enter);
    }
}
