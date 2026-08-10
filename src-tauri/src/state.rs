use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use silentsilo_core::{CoreError, CoreResult};
use silentsilo_vault::{SiloEntry, VaultSession, load_registry, save_registry};
use silentsilo_vfs::Vfs;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

/// The most silos that may be unlocked at the same time.
///
/// Each open silo means a decrypted working database on disk and a set of
/// keys in memory, so the number of them is the size of what a compromised
/// process gets while the user is unlocked. Switching between silos all day
/// would otherwise leave every one of them open, which is not something a
/// person would choose deliberately.
pub const MAX_OPEN_SILOS: usize = 3;

pub struct AppState {
    /// Which silo the app is currently pointed at.
    ///
    /// Everything path-shaped reads through this, so switching silos is a
    /// matter of changing it rather than of threading an id through every
    /// command. `None` only before the first one is opened.
    pub active_silo: Mutex<Option<SiloEntry>>,
    /// Every silo currently unlocked, keyed by id. More than one may be
    /// open so switching costs nothing; the alternative is a key tap every
    /// time somebody moves between work and personal. Bounded by
    /// [`MAX_OPEN_SILOS`]; see `sessions_mut` for the limit behaviour.
    pub sessions: Mutex<HashMap<Uuid, VaultSession>>,
    /// When each open silo was last used, for the idle timer and for
    /// deciding which one to close when the limit is reached.
    pub last_touched: Mutex<HashMap<Uuid, Instant>>,
    /// Set by `cancel_import`, checked between items by the long-running
    /// folder/paste import commands (which run as one blocking call, so a
    /// JS-side abort signal can't reach them directly). Callers reset it via
    /// `reset_import_cancel` before starting a new cancellable batch.
    pub import_cancelled: AtomicBool,
    /// Set by `cancel_verify`, checked between objects by `vault_verify`.
    /// Same pattern as `import_cancelled`: the check runs as one blocking
    /// invoke, so a JS-side abort signal cannot reach it directly. Reset at
    /// the start of every run rather than by a separate command.
    pub verify_cancelled: AtomicBool,
    /// Set by `cancel_seed`, checked between objects by `backup_target_seed`.
    /// Stopping a seed is safe: what already landed stays, and the next run
    /// skips it and carries on.
    pub seed_cancelled: AtomicBool,
    /// Held for the duration of a sync pass. Two passes at once — the timer
    /// firing while the user is holding the button — would each read the
    /// same pending queue and upload it twice.
    pub sync_in_flight: AtomicBool,
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Where the silo currently open keeps its files.
///
/// Named for what it is rather than for a fixed location: the user chooses
/// it per silo, so this is a lookup, not a path expression.
pub fn vault_dir(app: &AppHandle) -> Result<PathBuf, String> {
    active_silo(app).map(|silo| silo.path)
}

pub fn active_silo(app: &AppHandle) -> Result<SiloEntry, String> {
    app.state::<AppState>()
        .active_silo
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No silo is open".to_string())
}

/// The path a silo would get by default, before the user picks somewhere
/// else. Documents rather than app data, because a silo is the user's
/// property and belongs somewhere they can find, back up, and copy.
pub fn default_silo_parent(app: &AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .unwrap_or_else(|_| app.path().home_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("SilentSilo")
}

/// Records that a silo was opened, and makes it the one to open next time.
pub fn mark_silo_opened(app: &AppHandle, silo: &SiloEntry) -> Result<(), String> {
    let app_data = app_data_dir(app)?;
    let mut registry = load_registry(&app_data);
    let mut entry = silo.clone();
    entry.last_opened = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    registry.active = Some(entry.id);
    registry.upsert(entry);
    save_registry(&app_data, &registry).map_err(|e| e.to_string())
}

/// The open silo's credentials, without every call site having to resolve
/// which silo is focused.
pub fn silo_credentials(app: &AppHandle) -> Result<silentsilo_vault::LocalVaultAuth, String> {
    let silo = active_silo(app)?;
    silentsilo_vault::load_credentials(silo.id).map_err(|e| e.to_string())
}

/// The open silo's storage settings, or `None` when it syncs nowhere.
pub fn silo_store_config(app: &AppHandle) -> Option<silentsilo_store::StoreConfig> {
    let silo = active_silo(app).ok()?;
    silentsilo_vault::load_s3_config(silo.id)
}

/// The open silo's backup storage, ready to use.
///
/// `None` covers both "no storage configured" and "the stored settings no
/// longer open" — neither is an error at the call sites, which all treat
/// backup as optional.
pub fn silo_store(app: &AppHandle) -> Option<Box<dyn silentsilo_store::ObjectStore>> {
    silo_store_config(app)?.open().ok()
}

/// One named silo out of the registry, whether or not it is the one in focus.
///
/// For commands that must act on the silo the user was looking at when they
/// started, rather than on whichever one is focused by the time they finish.
/// Switching silos is a click, and a long operation that re-reads the focus
/// per step follows the user into somewhere they did not mean.
pub fn silo_by_id(app: &AppHandle, id: Uuid) -> Result<SiloEntry, String> {
    load_registry(&app_data_dir(app)?)
        .get(id)
        .cloned()
        .ok_or_else(|| "That silo is no longer in the list.".to_string())
}

/// Whether a named silo is still unlocked.
///
/// Long operations pin a silo and then work item by item without holding the
/// sessions lock, so it can close underneath them: the idle sweep does not do
/// it, because every step touches the silo, but locking the workstation and
/// walking away does, and so does opening a fourth silo. Asked between items,
/// this is what tells "that one file was locked by another program" apart
/// from "the silo is gone and nothing else will succeed either".
pub fn session_is_open(state: &State<AppState>, id: Uuid) -> bool {
    state
        .sessions
        .lock()
        .map(|sessions| sessions.contains_key(&id))
        .unwrap_or(false)
}

/// One target, opened, with everything a deletion needs to decide.
pub struct TargetHandle {
    pub store: Box<dyn silentsilo_store::ObjectStore>,
    pub role: silentsilo_vault::TargetRole,
    /// What to call it when telling the user what happened to it.
    pub label: String,
}

/// Every place the focused silo backs up to, opened.
///
/// Used by the operations that touch every copy: deletions, which done on
/// the primary alone would leave the object readable on the second one, and
/// downloads, which the second copy can serve when the first is a disk in a
/// drawer. Targets that will not open are skipped; the caller reports what
/// it managed.
pub fn silo_targets(app: &AppHandle) -> Vec<TargetHandle> {
    let Ok(silo) = active_silo(app) else {
        return Vec::new();
    };
    targets_for(silo.id)
}

/// [`silo_targets`] for a silo named by id, for commands that pin the silo
/// they started on rather than following the focus.
pub fn targets_for(silo_id: Uuid) -> Vec<TargetHandle> {
    silentsilo_vault::load_targets(silo_id)
        .into_iter()
        .filter_map(|target| {
            let store = target.config.open().ok()?;
            let label = if target.label.is_empty() {
                store.describe()
            } else {
                target.label.clone()
            };
            Some(TargetHandle {
                store,
                role: target.role,
                label,
            })
        })
        .collect()
}

/// The focused silo's session, held for as long as the caller needs it.
///
/// Deliberately shaped like the `Option<VaultSession>` this replaced, so
/// that every command asking "is a silo open, and which" reads the same as
/// it did when only one could be. Which silo it resolves to is the one
/// question they don't have to ask.
pub struct SessionGuard<'a> {
    sessions: std::sync::MutexGuard<'a, HashMap<Uuid, VaultSession>>,
    focused: Option<Uuid>,
}

impl SessionGuard<'_> {
    pub fn as_ref(&self) -> Option<&VaultSession> {
        self.sessions.get(&self.focused?)
    }

    pub fn is_some(&self) -> bool {
        self.as_ref().is_some()
    }

    pub fn is_none(&self) -> bool {
        !self.is_some()
    }
}

impl AppState {
    /// Locks `active_silo` before `sessions`, and never the other way
    /// round — the two are taken together often enough for the order to
    /// matter.
    pub fn focused_session(&self) -> Result<SessionGuard<'_>, String> {
        let focused = self
            .active_silo
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|s| s.id);
        Ok(SessionGuard {
            sessions: self.sessions.lock().map_err(|e| e.to_string())?,
            focused,
        })
    }

    /// Adds a freshly unlocked silo, closing the least recently used one if
    /// that would take the count past [`MAX_OPEN_SILOS`].
    ///
    /// Returns the silo that was closed to make room, so the caller can say
    /// so rather than leaving the user to discover it.
    pub fn open_session(&self, id: Uuid, session: VaultSession) -> Result<Option<Uuid>, String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let mut touched = self.last_touched.lock().map_err(|e| e.to_string())?;

        let evicted = if sessions.contains_key(&id) || sessions.len() < MAX_OPEN_SILOS {
            None
        } else {
            // Oldest by last use. A silo with no recorded touch is one that
            // was opened and never used, which makes it the best candidate,
            // so a missing entry sorts as maximally stale.
            stalest(sessions.keys().copied(), &touched).inspect(|stale| {
                if let Some(old) = sessions.remove(stale) {
                    close_one(old);
                }
                touched.remove(stale);
            })
        };

        sessions.insert(id, session);
        touched.insert(id, Instant::now());
        Ok(evicted)
    }

    /// Closes one silo, leaving any others open.
    pub fn close_session(&self, id: Uuid) -> Result<(), String> {
        let closed = self.sessions.lock().map_err(|e| e.to_string())?.remove(&id);
        if let Ok(mut touched) = self.last_touched.lock() {
            touched.remove(&id);
        }
        if let Some(session) = closed {
            close_one(session);
        }
        Ok(())
    }

    pub fn open_silo_ids(&self) -> Vec<Uuid> {
        self.sessions
            .lock()
            .map(|s| s.keys().copied().collect())
            .unwrap_or_default()
    }
}

/// Which open silo has gone longest without being used.
///
/// A silo with no recorded touch was opened and never used, which makes it
/// the best thing to close — and `None` sorting below every `Some` gives
/// that for free, which is worth stating because it is the kind of ordering
/// that is easy to get backwards.
fn stalest(ids: impl Iterator<Item = Uuid>, touched: &HashMap<Uuid, Instant>) -> Option<Uuid> {
    ids.min_by_key(|id| touched.get(id).copied())
}

/// Snapshots a session and clears the plaintext it was working through.
///
/// The session is dropped before the working copy is removed: Windows will
/// not delete a file that still has an open handle, so the order here is the
/// difference between locking a silo and leaving its decrypted database on
/// disk.
fn close_one(session: VaultSession) {
    if let Err(e) = session.backup_locally() {
        crate::diagnostics::warn("lock", format_args!("local snapshot failed: {e}"));
    }
    let paths = session.paths.clone();
    drop(session);
    crate::commands::vault::wipe_open_scratch(&paths.root);
    silentsilo_vault::wipe_plaintext_working_copy(&paths);
}

/// Registers a freshly unlocked session against the silo now in focus.
///
/// If holding it meant closing another, the frontend is told which one:
/// a silo that quietly stopped being open is something the user would
/// otherwise discover by being asked for their key again, with no
/// explanation of why.
pub fn open_focused_session(app: &AppHandle, session: VaultSession) -> Result<(), String> {
    let state = app.state::<AppState>();
    let id = focused_id(&state)?;
    if let Some(evicted) = state.open_session(id, session)? {
        let name = load_registry(&app_data_dir(app)?)
            .get(evicted)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let _ = app.emit("silo-auto-locked", serde_json::json!({ "name": name }));
    }
    announce_this_device(&state);
    Ok(())
}

/// Tells the silo what this machine calls itself, so the Devices list reads
/// as names rather than as hex.
///
/// Done here because every way into a silo passes through this function, and
/// best-effort because a silo that opened is open: failing to record a
/// computer name is not a reason to refuse it.
fn announce_this_device(state: &State<AppState>) {
    let Ok(guard) = state.focused_session() else {
        return;
    };
    let Some(session) = guard.as_ref() else {
        return;
    };
    let _ = Vfs::new(session).announce_device(
        silentsilo_shell::system_name().as_deref(),
        &silentsilo_shell::platform(),
    );
}

/// The id of the silo the user is looking at.
pub fn focused_id(state: &State<AppState>) -> Result<Uuid, String> {
    state
        .active_silo
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(|s| s.id)
        .ok_or_else(|| "No silo is open".to_string())
}

/// Records that a silo was used just now.
///
/// This is what the idle timer measures against, so it has to be called on
/// the way through anything the user did — which is why it lives in the two
/// accessors every command already goes through, rather than being
/// something each command has to remember.
pub fn touch(state: &State<AppState>, id: Uuid) {
    if let Ok(mut seen) = state.last_touched.lock() {
        seen.insert(id, Instant::now());
    }
}

/// How long each open silo has gone unused, in seconds.
pub fn idle_seconds(state: &State<AppState>) -> Vec<(Uuid, u64)> {
    let Ok(seen) = state.last_touched.lock() else {
        return Vec::new();
    };
    let now = Instant::now();
    seen.iter()
        .map(|(id, at)| (*id, now.saturating_duration_since(*at).as_secs()))
        .collect()
}

pub fn with_vfs<F, T>(state: &State<AppState>, f: F) -> Result<T, String>
where
    F: FnOnce(&VaultSession, &Vfs<'_>) -> CoreResult<T>,
{
    let id = focused_id(state)?;
    with_session_id(state, id, f)
}

/// Short-lock access to one specific open silo, named by id.
///
/// For long operations, which pin the silo they started on rather than
/// re-reading the focus per step: an import that resolved "the focused silo"
/// on every file would follow the user into a different silo mid-batch and
/// write the rest of the files there.
pub fn with_session_id<F, T>(state: &State<AppState>, id: Uuid, f: F) -> Result<T, String>
where
    F: FnOnce(&VaultSession, &Vfs<'_>) -> CoreResult<T>,
{
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    touch(state, id);
    let vfs = Vfs::new(session);
    f(session, &vfs).map_err(|e| e.to_string())
}

/// The cheap-to-clone parts of the focused silo's session, taken under a
/// lock held only for the copy. This is what lets encryption run without
/// the sessions mutex, which every command funnels through: holding it for
/// the length of a large file freezes the window for exactly that long.
/// The short per-item commits go back through [`with_session_id`].
pub struct SessionSnapshot {
    pub id: Uuid,
    pub root: PathBuf,
    pub kek: silentsilo_crypto::ContentKek,
}

pub fn snapshot_focused_session(state: &State<AppState>) -> Result<SessionSnapshot, String> {
    let id = focused_id(state)?;
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    touch(state, id);
    Ok(SessionSnapshot {
        id,
        root: session.paths.root.clone(),
        kek: session.kek.clone(),
    })
}

/// Push vault.db and pending blobs before lock or app exit.
///
/// Every open silo, not only the focused one: they all have unsaved work by
/// the same argument, and the reason to flush — the app is going away — does
/// not distinguish between them.
pub fn flush_vault_snapshot(state: State<'_, AppState>) {
    let Ok(sessions) = state.sessions.lock() else {
        return;
    };
    for session in sessions.values() {
        if let Err(e) = session.backup_locally() {
            crate::diagnostics::warn("flush", format_args!("local snapshot failed: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn the_silo_used_longest_ago_is_the_one_closed() {
        let now = Instant::now();
        let touched = HashMap::from([
            (id(1), now - std::time::Duration::from_secs(60)),
            (id(2), now - std::time::Duration::from_secs(600)),
            (id(3), now - std::time::Duration::from_secs(5)),
        ]);

        assert_eq!(
            stalest([id(1), id(2), id(3)].into_iter(), &touched),
            Some(id(2))
        );
    }

    #[test]
    fn a_silo_that_was_never_used_goes_first() {
        // Opened and left alone: nothing would be lost by closing it, and
        // every other candidate has at least been looked at.
        let touched =
            HashMap::from([(id(1), Instant::now() - std::time::Duration::from_secs(3600))]);

        assert_eq!(stalest([id(1), id(9)].into_iter(), &touched), Some(id(9)));
    }

    #[test]
    fn nothing_open_means_nothing_to_close() {
        assert_eq!(stalest(std::iter::empty(), &HashMap::new()), None);
    }

    /// The question a long import asks between items.
    ///
    /// It exists because "this one file would not open" and "the silo is
    /// gone" are the same error at the call site, and treating the second as
    /// the first is how locking the workstation mid-import produced a
    /// half-imported folder that the app then called imported. `AppState` is
    /// awkward to build in a unit test, so what is pinned here is the map
    /// the check reads: present means carry on, absent means stop.
    #[test]
    fn a_closed_silo_is_absent_from_the_session_map() {
        let mut sessions: HashMap<Uuid, ()> = HashMap::new();
        sessions.insert(id(1), ());

        assert!(sessions.contains_key(&id(1)), "an open silo is present");

        // What `close_session` does, and what `lock_all_silos` does to every
        // silo at once when the workstation locks.
        sessions.remove(&id(1));
        assert!(
            !sessions.contains_key(&id(1)),
            "a locked silo must read as gone, not as a file that failed"
        );
    }
}
