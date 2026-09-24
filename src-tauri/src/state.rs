use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use silentsilo_core::{CoreError, CoreResult};
use silentsilo_vault::{SiloEntry, VaultSession, load_registry, save_registry};
use silentsilo_vfs::Vfs;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

/// The most silos that may be unlocked at the same time.
///
/// Each open silo means a decrypted index and a set of keys in memory, so
/// the number of them is the size of what a compromised process gets while
/// the user is unlocked. Switching between silos all day
/// would otherwise leave every one of them open, which is not something a
/// person would choose deliberately.
pub const MAX_OPEN_SILOS: usize = 3;

pub struct AppState {
    /// What this app shares with core's sync pass and flows: the focused
    /// silo, the open sessions, the idle clock and the flags a pass or a
    /// long command holds. Reached through `Deref`, so `state.sessions`
    /// reads as before; one set, so a pass core runs and a command here
    /// lock the same things.
    pub core: silentsilo_app::AppState,
    /// Moves whenever a silo opens, closes or takes the focus. The browser
    /// extension's login refs are valid for one value of it, so a lock or a
    /// switch leaves every ref it handed out useless.
    pub session_epoch: AtomicU64,
    /// Silo roots with a session being built and not yet in `sessions`. The
    /// scratch sweep after a lock keeps their working copies: it runs from
    /// other threads and would otherwise delete a copy being opened.
    pub opening: Mutex<Vec<PathBuf>>,
}

impl std::ops::Deref for AppState {
    type Target = silentsilo_app::AppState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// Registers a silo root as being opened until dropped. Taken before a
/// session is built and held until it is in the map or abandoned.
pub struct Opening {
    app: AppHandle,
    root: PathBuf,
}

pub fn opening(app: &AppHandle, root: &Path) -> Opening {
    lock_recovering(&app.state::<AppState>().opening).push(root.to_path_buf());
    Opening {
        app: app.clone(),
        root: root.to_path_buf(),
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        let mut opening = lock_recovering(&state.opening);
        if let Some(at) = opening.iter().position(|r| *r == self.root) {
            opening.remove(at);
        }
    }
}

/// A guard even when a panic poisoned the mutex. Only for the paths that
/// close silos or clear their scratch: a lock that fails over poisoning
/// would leave them open, which is the worse of the two.
fn lock_recovering<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    #[cfg(feature = "e2e")]
    return {
        let _ = app;
        Ok(crate::e2e::dir().join("app"))
    };
    #[cfg(not(feature = "e2e"))]
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Where the silo currently open keeps its files.
///
/// Named for what it is rather than for a fixed location: the user chooses
/// it per silo, so this is a lookup, not a path expression.
pub fn vault_dir(app: &AppHandle) -> Result<PathBuf, String> {
    active_silo(app).map(|silo| silo.path)
}

/// The active silo, only while it is unlocked. For commands that change
/// what protects it or where it backs up: none of those should work for
/// whoever sits at a locked app.
pub fn unlocked_silo(app: &AppHandle) -> Result<SiloEntry, String> {
    let silo = active_silo(app)?;
    let state = app.state::<AppState>();
    let open = state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .contains_key(&silo.id);
    if !open {
        return Err("Unlock the silo first.".into());
    }
    Ok(silo)
}

/// The active silo and the content KEK of its open session.
///
/// For the files sealed beside the silo under that key: the protected folder
/// list and its import ledger. They cannot be read at all while the silo is
/// locked, so this says so rather than letting a command answer with an
/// empty list, which would tell the user they protect nothing.
pub fn unlocked_silo_with_kek(
    app: &AppHandle,
) -> Result<(SiloEntry, silentsilo_crypto::ContentKek), String> {
    let silo = active_silo(app)?;
    let state = app.state::<AppState>();
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let kek = sessions
        .get(&silo.id)
        .ok_or_else(|| "Unlock the silo first.".to_string())?
        .kek
        .clone();
    Ok((silo, kek))
}

pub fn active_silo(app: &AppHandle) -> Result<SiloEntry, String> {
    app.state::<AppState>()
        .active_silo
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No silo is open.".to_string())
}

/// The path a silo would get by default, before the user picks somewhere
/// else. Documents rather than app data, because a silo is the user's
/// property and belongs somewhere they can find, back up, and copy.
pub fn default_silo_parent(app: &AppHandle) -> PathBuf {
    #[cfg(feature = "e2e")]
    return {
        let _ = app;
        crate::e2e::dir().join("Documents").join("SilentSilo")
    };
    #[cfg(not(feature = "e2e"))]
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

/// For a decrypt that ran without a lock held: if the silo locked meanwhile,
/// the plaintext it just wrote goes, and nothing opens it.
///
/// A lock wipes the scratch folder, but a file still being written is open
/// and survives the wipe, and a folder the lock had already emptied was made
/// again by the decrypt. Without this the copy landed after the lock and was
/// then handed to another application.
pub fn discard_if_locked(state: &State<AppState>, id: Uuid, dest: &Path) -> Result<(), String> {
    if session_is_open(state, id) {
        return Ok(());
    }
    let _ = std::fs::remove_file(dest);
    Err(CoreError::VaultLocked.to_string())
}

/// One target, opened, with everything a deletion needs to decide.
pub struct TargetHandle {
    /// The target's id, to note content fetched from it as held there.
    pub id: Uuid,
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
            let id = target.config.target_id();
            let store = target.config.open().ok()?;
            let label = if target.label.is_empty() {
                store.describe()
            } else {
                target.label.clone()
            };
            Some(TargetHandle {
                id,
                store,
                role: target.role,
                label,
            })
        })
        .collect()
}

/// [`targets_for`] for a key change, which must reach every target: one
/// skipped because it would not open keeps the old key readable there.
pub fn every_target_for(silo_id: Uuid) -> Result<Vec<TargetHandle>, String> {
    silentsilo_vault::load_targets(silo_id)
        .into_iter()
        .map(|target| {
            let store = target.config.open().map_err(|e| {
                let name = if target.label.is_empty() {
                    "A backup storage"
                } else {
                    target.label.as_str()
                };
                format!("{name} cannot be opened ({e}). Fix it or remove it first.")
            })?;
            let label = if target.label.is_empty() {
                store.describe()
            } else {
                target.label.clone()
            };
            Ok(TargetHandle {
                id: target.config.target_id(),
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
    pub fn epoch(&self) -> u64 {
        self.session_epoch.load(Ordering::SeqCst)
    }

    pub fn bump_epoch(&self) {
        self.session_epoch.fetch_add(1, Ordering::SeqCst);
    }

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
    /// so rather than leaving the user to discover it, and how many scratch
    /// folders still hold plaintext afterwards, as a lock reports it.
    ///
    /// The evicted session is snapshotted after both mutexes are released:
    /// sealing a large index under them froze every other command.
    pub fn open_session(
        &self,
        id: Uuid,
        session: VaultSession,
    ) -> Result<Option<(Uuid, usize)>, String> {
        let evicted = {
            let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
            let mut touched = self.last_touched.lock().map_err(|e| e.to_string())?;

            let evicted = if sessions.contains_key(&id) || sessions.len() < MAX_OPEN_SILOS {
                None
            } else {
                // Oldest by last use. A silo with no recorded touch is one
                // that was opened and never used, which makes it the best
                // candidate, so a missing entry sorts as maximally stale.
                stalest(sessions.keys().copied(), &touched).and_then(|stale| {
                    touched.remove(&stale);
                    sessions.remove(&stale).map(|old| (stale, old))
                })
            };

            sessions.insert(id, session);
            touched.insert(id, Instant::now());
            evicted
        };
        self.bump_epoch();
        Ok(evicted.map(|(stale, old)| {
            close_one(old);
            (stale, self.sweep_scratch())
        }))
    }

    /// Closes one silo, leaving any others open.
    ///
    /// Carries on through a poisoned mutex: a panic elsewhere must not turn
    /// Lock into a success that closed nothing.
    pub fn close_session(&self, id: Uuid) -> Result<(), String> {
        let closed = lock_recovering(&self.sessions).remove(&id);
        self.bump_epoch();
        lock_recovering(&self.last_touched).remove(&id);
        if let Some(session) = closed {
            close_one(session);
        }
        self.sweep_scratch();
        Ok(())
    }

    /// Removes the plaintext scratch of every silo that is not open, the one
    /// just closed and any a crash or kill left behind, keeping their
    /// ciphered working copies. Returns how many still hold plaintext because
    /// another application holds a file in them.
    ///
    /// A silo being unlocked right now is kept as if it were open.
    pub fn sweep_scratch(&self) -> usize {
        let mut roots: Vec<PathBuf> = lock_recovering(&self.sessions)
            .values()
            .map(|session| session.paths.root.clone())
            .collect();
        roots.extend(lock_recovering(&self.opening).iter().cloned());
        let open: Vec<&Path> = roots.iter().map(|r| r.as_path()).collect();
        silentsilo_vault::wipe_work_dirs_except(&open)
    }

    pub fn open_silo_ids(&self) -> Vec<Uuid> {
        lock_recovering(&self.sessions).keys().copied().collect()
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

/// Snapshots a session and clears the plaintext it was working through. The
/// ciphered working copy stays, so the next unlock reuses it.
///
/// The session is dropped before anything is removed: Windows will not
/// delete a file that still has an open handle.
fn close_one(session: VaultSession) {
    if let Err(e) = session.seal_for_lock() {
        crate::diagnostics::warn("lock", format_args!("local snapshot failed: {e}"));
    }
    let paths = session.paths.clone();
    drop(session);
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
    // An unlock waits on a key touch, a download or a disk restore, and the
    // focus can move in that time. Filed under whatever was focused at the
    // end, silo A's session sat under silo B's id, and a sync pass then
    // paired A's database with B's folder and B's storage.
    if session.vault_id != id {
        return Err("The silo on screen changed while this one was opening. Open it again.".into());
    }
    if let Some((evicted, scratch_left)) = state.open_session(id, session)? {
        // What a lock of that silo would have done: a password it copied
        // goes, and a file still held open is reported.
        crate::commands::vault::take_back_clipboard(app, Some(&[evicted]));
        if scratch_left > 0 {
            let _ = app.emit("scratch-still-open", scratch_left);
        }
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
        .ok_or_else(|| "No silo is open.".to_string())
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

/// [`touch`] for callers that do not hold the sessions lock and may name a
/// locked silo. A locked silo gets no idle timer: it would later "lock"
/// nothing and take back another silo's clipboard on the way.
pub fn touch_if_open(state: &State<AppState>, id: Uuid) {
    if session_is_open(state, id) {
        touch(state, id);
    }
}

/// How long each open silo has gone unused, in seconds. Only open ones: a
/// timer left for a silo that is not open would ask to lock it.
pub fn idle_seconds(state: &State<AppState>) -> Vec<(Uuid, u64)> {
    let open = state.open_silo_ids();
    let Ok(seen) = state.last_touched.lock() else {
        return Vec::new();
    };
    idle_of_open(&seen, &open, Instant::now())
}

fn idle_of_open(seen: &HashMap<Uuid, Instant>, open: &[Uuid], now: Instant) -> Vec<(Uuid, u64)> {
    seen.iter()
        .filter(|(id, _)| open.contains(id))
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

/// [`with_vfs`] for a read that does not count as use. The listing is also
/// refreshed whenever a sync pass applies another device's changes, and a
/// silo whose other devices kept changing it never reached its idle lock.
/// A person browsing is still counted: the window reports their input.
pub fn with_vfs_untouched<F, T>(state: &State<AppState>, f: F) -> Result<T, String>
where
    F: FnOnce(&VaultSession, &Vfs<'_>) -> CoreResult<T>,
{
    let id = focused_id(state)?;
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    let vfs = Vfs::new(session);
    f(session, &vfs).map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_open_silo_has_an_idle_timer() {
        // A silo focused while locked used to get a timer, and its expiry
        // then locked nothing and cleared another silo's clipboard.
        let now = Instant::now();
        let seen = HashMap::from([
            (id(1), now - std::time::Duration::from_secs(30)),
            (id(2), now - std::time::Duration::from_secs(900)),
        ]);
        let idle = idle_of_open(&seen, &[id(1)], now);
        assert_eq!(idle, vec![(id(1), 30)]);
    }

    #[test]
    fn a_poisoned_mutex_still_hands_over_what_it_holds() {
        // Lock has to close silos after a panic elsewhere, not report
        // success over an empty list.
        let map = std::sync::Arc::new(Mutex::new(vec![id(1)]));
        let poisoner = map.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison it");
        })
        .join();
        assert!(map.lock().is_err(), "the mutex is poisoned");
        assert_eq!(*lock_recovering(&map), vec![id(1)]);
    }

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
