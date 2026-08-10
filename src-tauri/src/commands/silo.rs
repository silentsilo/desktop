//! Creating, listing, opening and forgetting silos.
//!
//! A silo is a folder the user chose the location of, holding everything
//! that silo needs: the encrypted index, the blobs, the key envelopes, the
//! credentials. The app keeps only an index of where they are, so a silo can
//! sit on an external drive or a synced folder and still be the same silo.

use std::path::PathBuf;

use silentsilo_vault::{
    LocalVaultAuth, SiloEntry, VaultPaths, VaultSession, available_path, clear_credentials,
    clear_s3_config, load_registry, save_registry,
};
use silentsilo_vfs::Vfs;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::state::{AppState, active_silo, app_data_dir, default_silo_parent, mark_silo_opened};

/// A silo as the picker needs to see it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SiloView {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_opened: i64,
    /// False when the folder isn't reachable — an external drive that isn't
    /// plugged in, or one the user moved. Shown as unavailable rather than
    /// offered as a silo that fails to open for invisible reasons.
    pub present: bool,
    /// Whether this silo is unlocked right now, so the picker can show
    /// which ones open without a security key.
    #[serde(default)]
    pub unlocked: bool,
}

impl From<&SiloEntry> for SiloView {
    fn from(entry: &SiloEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            name: entry.name.clone(),
            path: entry.path.to_string_lossy().to_string(),
            last_opened: entry.last_opened,
            present: entry.is_present(),
            // Whether a silo is open is state, not registry data, so the
            // conversion cannot know it — callers that care fill it in.
            unlocked: false,
        }
    }
}

/// What adding a folder back needs from this machine.
///
/// The condition this replaces asked whether a `credentials.json` sat in the
/// folder, which is close to the opposite of the right question: credentials
/// live in the OS keyring and that file is deleted the moment the keyring
/// accepts them, so the folders it rejected were the ones in good order.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum AddPlan {
    /// This machine already has what it needs.
    Ready,
    /// Openable with a security key, so a fresh device secret restores
    /// access — the unlock path takes one and ignores it, unwrapping the DEK
    /// with the key instead.
    MintSecret,
    /// No key enrolled and no secret: the secret *was* the only way in, and
    /// a new one unwraps nothing. Only the backup storage has this silo now.
    Refuse,
}

impl AddPlan {
    pub(crate) fn decide(has_credentials: bool, fido_enrolled: bool) -> Self {
        match (has_credentials, fido_enrolled) {
            (true, _) => Self::Ready,
            (false, true) => Self::MintSecret,
            (false, false) => Self::Refuse,
        }
    }
}

/// Every silo this machine knows about, most recently used first.
#[tauri::command]
pub fn silo_list(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<SiloView>, String> {
    let registry = load_registry(&app_data_dir(&app)?);
    let open = state.open_silo_ids();
    let mut silos: Vec<SiloView> = registry
        .silos
        .iter()
        .map(|entry| SiloView {
            unlocked: open.contains(&entry.id),
            ..SiloView::from(entry)
        })
        .collect();
    silos.sort_by_key(|s| std::cmp::Reverse(s.last_opened));
    Ok(silos)
}

/// The consumer sync service a location belongs to, if any. Advisory: a
/// silo in a synced folder is a fine single-computer backup, since
/// everything there is ciphertext, but not a second computer's copy, since
/// two machines rewriting one snapshot file produce a conflict copy rather
/// than a merge. That is what the operation log in a bucket is for.
#[tauri::command]
pub fn silo_sync_provider_at(path: String) -> Option<String> {
    silentsilo_vault::detect_sync_provider(&PathBuf::from(path)).map(str::to_string)
}

/// Where a new silo would go unless the user says otherwise.
#[tauri::command]
pub fn silo_default_location(app: AppHandle, name: String) -> Result<String, String> {
    Ok(available_path(&default_silo_parent(&app), &name)
        .to_string_lossy()
        .to_string())
}

/// Creates a silo and makes it the open one.
///
/// The vault id and the bootstrap secret are generated here — there is no
/// server to issue them — and a security key still has to be enrolled before
/// the silo can be locked and reopened.
#[tauri::command]
pub async fn silo_create(
    app: AppHandle,
    name: String,
    location: Option<String>,
) -> Result<SiloView, String> {
    // Provisioning is disk work end to end, and the shell-queue drain at the
    // bottom can be an import of whole files: blocking pool, not an async
    // worker.
    crate::commands::fido::run_blocking(move || silo_create_impl(&app, name, location)).await
}

fn silo_create_impl(
    app: &AppHandle,
    name: String,
    location: Option<String>,
) -> Result<SiloView, String> {
    let app = app.clone();
    let state = app.state::<AppState>();
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the silo a name.".into());
    }

    let app_data = app_data_dir(&app)?;
    let mut registry = load_registry(&app_data);
    if registry.name_taken(&name, None) {
        return Err(format!("You already have a silo called “{name}”."));
    }

    let path = match location {
        Some(chosen) => PathBuf::from(chosen),
        None => available_path(&default_silo_parent(&app), &name),
    };
    if VaultPaths::new(path.clone()).exists() {
        return Err(format!(
            "{} already holds a silo. Pick an empty folder.",
            path.display()
        ));
    }

    let vault_id = Uuid::new_v4();
    // 32 bytes of OS randomness: this feeds key derivation, so it needs full
    // entropy rather than being a readable identifier.
    let device_secret = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };

    std::fs::create_dir_all(&path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    silentsilo_vault::write_marker(&path, vault_id).map_err(|e| e.to_string())?;
    silentsilo_vault::save_credentials(&LocalVaultAuth {
        vault_id,
        device_secret: device_secret.clone(),
    })
    .map_err(|e| e.to_string())?;

    let session = VaultSession::provision(path.clone(), vault_id, &device_secret)
        .map_err(|e| e.to_string())?;
    let vfs = Vfs::new(&session);
    vfs.ensure_initialized().map_err(|e| e.to_string())?;
    // `provision` snapshots before any schema exists, so without this the
    // next open would decrypt that empty snapshot back over the initialised
    // database and the silo would read as "not found".
    session.backup_locally().map_err(|e| e.to_string())?;

    let entry = SiloEntry {
        id: vault_id,
        name,
        path,
        last_opened: 0,
        auto_lock_minutes: None,
    };
    registry.upsert(entry.clone());
    registry.active = Some(entry.id);
    save_registry(&app_data, &registry).map_err(|e| e.to_string())?;
    mark_silo_opened(&app, &entry)?;

    *state.active_silo.lock().map_err(|e| e.to_string())? = Some(entry.clone());
    crate::state::open_focused_session(&app, session)?;
    // Anything the Explorer verbs queued while no silo was open lands here,
    // rather than being dropped because there was nowhere to put it.
    let _ = crate::commands::vault::process_shell_upload_queue(&app);
    Ok(SiloView::from(&entry))
}

/// Points the app at a silo. It still has to be unlocked afterwards.
#[tauri::command]
pub async fn silo_open(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<SiloView, String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let registry = load_registry(&app_data_dir(&app)?);
    let entry = registry
        .get(id)
        .cloned()
        .ok_or_else(|| "That silo is no longer in the list.".to_string())?;

    if !entry.is_present() {
        return Err(format!(
            "Nothing at {}. If it's on a removable drive, plug it in first.",
            entry.path.display()
        ));
    }

    // Whatever was open stays open. Pointing at a silo is not the same act
    // as unlocking it: if this one is already unlocked, focusing it is all
    // that happens and the user is not asked for a key they have already
    // presented.
    mark_silo_opened(&app, &entry)?;
    *state.active_silo.lock().map_err(|e| e.to_string())? = Some(entry.clone());
    crate::state::touch(&state, entry.id);
    Ok(SiloView::from(&entry))
}

/// Which silos are unlocked right now.
///
/// The switcher needs this to know which ones it can move to without asking
/// for a key, and the Explorer verbs need it to know whether to ask where
/// something should go.
#[tauri::command]
pub fn silo_open_list(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<SiloView>, String> {
    let registry = load_registry(&app_data_dir(&app)?);
    let mut open: Vec<SiloView> = state
        .open_silo_ids()
        .into_iter()
        .filter_map(|id| {
            registry.get(id).map(|entry| SiloView {
                unlocked: true,
                ..SiloView::from(entry)
            })
        })
        .collect();
    open.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(open)
}

/// How long each open silo has gone unused, against its own timeout.
///
/// The frontend runs the clock — it is the side that knows about pointers
/// and keystrokes — but the elapsed time is measured here, where the work
/// actually happens, so a silo being driven by the Explorer verbs while the
/// window sits untouched still counts as in use.
#[derive(serde::Serialize)]
pub struct SiloIdleView {
    pub id: String,
    pub idle_seconds: u64,
    /// The silo's own setting, or `None` to follow the app-wide default.
    pub auto_lock_minutes: Option<u32>,
}

#[tauri::command]
pub fn silo_idle_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SiloIdleView>, String> {
    let registry = load_registry(&app_data_dir(&app)?);
    Ok(crate::state::idle_seconds(&state)
        .into_iter()
        .map(|(id, idle_seconds)| SiloIdleView {
            id: id.to_string(),
            idle_seconds,
            auto_lock_minutes: registry.get(id).and_then(|e| e.auto_lock_minutes),
        })
        .collect())
}

/// Marks the focused silo as being used right now. Commands cover real
/// work but not reading: someone scrolling a folder for ten minutes issues
/// none and would be locked out mid-read. Only the silo on screen counts;
/// the others go on counting down.
#[tauri::command]
pub fn silo_touch(state: State<'_, AppState>) -> Result<(), String> {
    crate::state::touch(&state, crate::state::focused_id(&state)?);
    Ok(())
}

/// Sets how long a silo may sit unused before it locks itself.
#[tauri::command]
pub fn silo_set_auto_lock(app: AppHandle, id: String, minutes: Option<u32>) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let app_data = app_data_dir(&app)?;
    let mut registry = load_registry(&app_data);
    let mut entry = registry
        .get(id)
        .cloned()
        .ok_or_else(|| "That silo is no longer in the list.".to_string())?;
    entry.auto_lock_minutes = minutes.filter(|m| *m > 0);
    registry.upsert(entry);
    save_registry(&app_data, &registry).map_err(|e| e.to_string())
}

/// Steps back to the picker without locking anything.
///
/// The counterpart to keeping silos open: leaving the one on screen is not
/// a decision to lock it, or switching between two silos would cost a key
/// tap in each direction. Locking is what the Lock button and each silo's
/// own timeout are for.
#[tauri::command]
pub fn silo_blur(state: State<'_, AppState>) -> Result<(), String> {
    *state.active_silo.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

fn close_session(state: &State<'_, AppState>) -> Result<(), String> {
    match crate::state::focused_id(state) {
        Ok(id) => state.close_session(id),
        // Nothing in focus is nothing to close.
        Err(_) => Ok(()),
    }
}

#[tauri::command]
pub fn silo_rename(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the silo a name.".into());
    }

    let app_data = app_data_dir(&app)?;
    let mut registry = load_registry(&app_data);
    if registry.name_taken(&name, Some(id)) {
        return Err(format!("You already have a silo called “{name}”."));
    }
    let mut entry = registry
        .get(id)
        .cloned()
        .ok_or_else(|| "That silo is no longer in the list.".to_string())?;
    // The folder keeps its original name. Renaming it would break the path
    // every backup and shortcut already points at, to fix nothing.
    entry.name = name;
    registry.upsert(entry.clone());
    save_registry(&app_data, &registry).map_err(|e| e.to_string())?;

    if let Ok(mut guard) = app.state::<AppState>().active_silo.lock()
        && guard.as_ref().is_some_and(|s| s.id == id)
    {
        *guard = Some(entry);
    }
    Ok(())
}

/// Removes a silo from the list, and optionally deletes its files.
///
/// Forgetting is the safe default and is reversible — the folder is still
/// there and can be added back. Deleting is neither, which is why it is a
/// separate flag rather than a consequence.
#[tauri::command]
pub async fn silo_forget(app: AppHandle, id: String, delete_files: bool) -> Result<(), String> {
    // Deleting a silo's folder removes every blob it holds, which for a
    // large silo is minutes of filesystem work: blocking pool.
    crate::commands::fido::run_blocking(move || silo_forget_impl(&app, id, delete_files)).await
}

fn silo_forget_impl(app: &AppHandle, id: String, delete_files: bool) -> Result<(), String> {
    let app = app.clone();
    let state = app.state::<AppState>();
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let app_data = app_data_dir(&app)?;
    let mut registry = load_registry(&app_data);
    let entry = registry
        .get(id)
        .cloned()
        .ok_or_else(|| "That silo is no longer in the list.".to_string())?;

    // Closed first: deleting the files of a silo whose database is open
    // would fail on Windows and corrupt it everywhere else.
    if active_silo(&app).is_ok_and(|s| s.id == id) {
        close_session(&state)?;
        *state.active_silo.lock().map_err(|e| e.to_string())? = None;
    }

    registry.remove(id);
    save_registry(&app_data, &registry).map_err(|e| e.to_string())?;

    // Whatever this machine decrypted for the silo goes with it. Closing the
    // session above already does this when the silo was the open one, and
    // forgetting one that was not open used to leave its working copy behind
    // for whatever silo occupied that folder next.
    silentsilo_vault::wipe_work_dir(&entry.path);

    // Secrets go only when the files do. Removing a silo from the list while
    // keeping its folder says "not on this list", not "not mine" — and
    // taking this machine's way into a folder the user deliberately kept is
    // a surprise in the one direction that costs them something.
    if delete_files {
        clear_credentials(entry.id);
        clear_s3_config(entry.id);
        // The blob bookkeeping describes a silo that is about to stop
        // existing. Kept when the folder is kept, because re-adding the same
        // silo should not have to re-check every blob against the bucket.
        silentsilo_vault::wipe_cache_dir(&entry.path);
    }

    if delete_files && entry.path.exists() {
        std::fs::remove_dir_all(&entry.path).map_err(|e| {
            format!("removed from the list, but the files could not be deleted: {e}")
        })?;
    }
    Ok(())
}

/// Adds a silo folder that this machine has forgotten, or that came from
/// somewhere else.
///
/// The credentials travel inside the folder, so a silo copied from another
/// machine or restored from a backup can be picked up as-is.
#[tauri::command]
pub fn silo_add_existing(
    app: AppHandle,
    path: String,
    name: Option<String>,
) -> Result<SiloView, String> {
    let path = PathBuf::from(path);
    if !VaultPaths::new(path.clone()).exists() {
        return Err(format!("{} doesn't look like a silo.", path.display()));
    }

    let app_data = app_data_dir(&app)?;
    let mut registry = load_registry(&app_data);
    if let Some(existing) = registry.silos.iter().find(|s| s.path == path) {
        return Ok(SiloView::from(existing));
    }

    // The id comes from the silo itself. Inventing one here would file this
    // machine's secrets under a key that nothing in the folder refers to.
    //
    // This used to demand a `credentials.json` beside it, which rejected the
    // normal case: credentials live in the OS keyring and that file is
    // deleted as soon as the keyring accepts them, so the folders failing
    // the check were the healthy ones.
    let id = silentsilo_vault::read_marker(&path)
        .map_err(|e| format!("could not identify that silo: {e}"))?
        .vault_id;

    // This machine may hold no credentials for it — the folder came from
    // another computer, or this one forgot the silo and cleared them.
    let plan = AddPlan::decide(
        silentsilo_vault::load_credentials(id).is_ok(),
        silentsilo_vault::is_fido_enrolled(&path),
    );
    if plan == AddPlan::Refuse {
        return Err(
            "That silo has no security key enrolled, and this computer no longer holds the \
             secret it was opened with — set it up again from its backup storage instead."
                .into(),
        );
    }
    if plan == AddPlan::MintSecret {
        let device_secret = {
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            rand::rng().fill_bytes(&mut bytes);
            hex::encode(bytes)
        };
        silentsilo_vault::save_credentials(&LocalVaultAuth {
            vault_id: id,
            device_secret,
        })
        .map_err(|e| format!("could not record this computer's access to that silo: {e}"))?;
    }
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Silo")
                .to_string()
        });
    let name = if registry.name_taken(&name, None) {
        format!("{name} (added)")
    } else {
        name
    };

    let entry = SiloEntry {
        id,
        name,
        path,
        last_opened: 0,
        auto_lock_minutes: None,
    };
    registry.upsert(entry.clone());
    save_registry(&app_data, &registry).map_err(|e| e.to_string())?;
    Ok(SiloView::from(&entry))
}

/// Registers a silo for a vault that already exists elsewhere.
///
/// Used by both join paths. The id is not ours to choose — it comes from the
/// bucket's manifest, and inventing one here would produce a silo that never
/// recognises anything already in storage.
pub(crate) fn register_joined_silo(
    app: &AppHandle,
    vault_id: Uuid,
    name: &str,
    location: Option<String>,
) -> Result<SiloEntry, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the silo a name.".into());
    }

    let app_data = app_data_dir(app)?;
    let registry = load_registry(&app_data);
    if registry.silos.iter().any(|s| s.id == vault_id) {
        return Err("That silo is already on this machine.".into());
    }
    if registry.name_taken(name, None) {
        return Err(format!("You already have a silo called “{name}”."));
    }

    let path = match location {
        Some(chosen) => PathBuf::from(chosen),
        None => available_path(&default_silo_parent(app), name),
    };
    if VaultPaths::new(path.clone()).exists() {
        return Err(format!(
            "{} already holds a silo. Pick an empty folder.",
            path.display()
        ));
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    silentsilo_vault::write_marker(&path, vault_id).map_err(|e| e.to_string())?;

    Ok(SiloEntry {
        id: vault_id,
        name: name.to_string(),
        path,
        last_opened: 0,
        auto_lock_minutes: None,
    })
}

/// Records a freshly joined silo and makes it the open one.
pub(crate) fn adopt_joined_silo(
    app: &AppHandle,
    state: &State<'_, AppState>,
    entry: SiloEntry,
) -> Result<(), String> {
    let app_data = app_data_dir(app)?;
    let mut registry = load_registry(&app_data);
    registry.upsert(entry.clone());
    registry.active = Some(entry.id);
    save_registry(&app_data, &registry).map_err(|e| e.to_string())?;
    mark_silo_opened(app, &entry)?;
    *state.active_silo.lock().map_err(|e| e.to_string())? = Some(entry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_this_machine_already_has_credentials_for_is_simply_added() {
        assert_eq!(AddPlan::decide(true, true), AddPlan::Ready);
        // Credentials present is enough on its own; enrolment is the silo's
        // business, not this machine's.
        assert_eq!(AddPlan::decide(true, false), AddPlan::Ready);
    }

    #[test]
    fn a_folder_with_a_key_enrolled_is_added_even_with_no_credentials_here() {
        // The case hit after removing a silo from the list while keeping its
        // files, and the one the old check got backwards: holding no
        // credentials is the normal state of a silo whose secrets live in
        // the keyring, not a reason to refuse the folder.
        assert_eq!(AddPlan::decide(false, true), AddPlan::MintSecret);
    }

    #[test]
    fn a_folder_with_no_key_and_no_credentials_cannot_be_recovered_here() {
        // Minting a secret here would produce a silo that looks added and
        // then fails to open, which is worse than saying so now.
        assert_eq!(AddPlan::decide(false, false), AddPlan::Refuse);
    }
}
