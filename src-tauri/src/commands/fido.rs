use std::time::Duration;

use silentsilo_fido::Authenticator;
use silentsilo_sync as sync;
use silentsilo_vault::{
    StoredFidoCredential, StoredFidoKeys, VaultSession, dek_path, is_fido_enrolled, load_fido_keys,
    save_fido_keys, wrap_dek_bytes,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, vault_dir};
use hex::encode as hex_encode;

/// The instruction has to name the right gesture, or the user waits for a
/// prompt that never comes.
///
/// No step count: how many ceremonies an enrolment needs is not known until
/// the first one answers. Windows Hello hands back the wrap key with the
/// credential, and then there is only this one.
fn step_one_message(authenticator: Authenticator) -> &'static str {
    match authenticator {
        Authenticator::ThisDevice => "Confirm with Windows Hello to secure the silo.",
        Authenticator::SecurityKey => {
            "Touch your security key to create the enrollment credential."
        }
    }
}

/// Only emitted when the first ceremony did not produce the wrap key.
fn step_two_message(authenticator: Authenticator) -> &'static str {
    match authenticator {
        Authenticator::ThisDevice => "Confirm once more to secure the silo's encryption key.",
        Authenticator::SecurityKey => {
            "Touch the SAME key again to secure the silo's encryption key."
        }
    }
}

pub fn emit_fido_progress(app: &AppHandle, message: &str) {
    let _ = app.emit("fido-progress", message);
}

pub async fn run_fido<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, silentsilo_fido::FidoError> + Send + 'static,
{
    bind_fido_parent_hwnd(app);
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

async fn sleep_ms(ms: u64) {
    let _ =
        tauri::async_runtime::spawn_blocking(move || std::thread::sleep(Duration::from_millis(ms)))
            .await;
}

/// Waited out between ceremonies when an enrolment needs two, which is
/// every enrolment except Windows Hello's single-ceremony path. Windows runs
/// one WebAuthn ceremony at a time, and starting the second too early makes
/// it fail *inside its own prompt*, where the retry below cannot reach: the
/// person has to press Try again themselves, during setup. So the wait goes
/// in front of the second ceremony, and waits for the platform rather than
/// for a number, the backend watching for the dialog to actually close. The
/// cap only applies if it never does.
async fn settle_between_ceremonies() {
    let _ = tauri::async_runtime::spawn_blocking(|| {
        silentsilo_fido::wait_for_ceremony_teardown(4_000);
    })
    .await;
}

/// Like `run_fido`, but retries a ceremony that failed for a reason the
/// user did not choose, backing off further each time. Never after a
/// cancellation: re-opening the dialog of someone who just pressed Cancel
/// is worse than the error was.
async fn run_fido_settling<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Fn() -> Result<T, silentsilo_fido::FidoError> + Send + Clone + 'static,
{
    let mut last = match run_fido(app, f.clone()).await {
        Ok(value) => return Ok(value),
        Err(e) => e,
    };

    for delay in [800, 1800] {
        if last.to_lowercase().contains("cancel") {
            break;
        }
        sleep_ms(delay).await;
        match run_fido(app, f.clone()).await {
            Ok(value) => return Ok(value),
            Err(e) => last = e,
        }
    }

    Err(last)
}

/// Runs blocking work (filesystem walks, encryption over large files) on a
/// dedicated thread rather than the async task the Tauri command runs in,
/// so a long operation can't stall the runtime.
pub async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

pub fn bind_fido_parent_hwnd(app: &AppHandle) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Some(window) = app.get_webview_window("main")
            && let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(win) = handle.as_raw()
        {
            silentsilo_fido::set_parent_hwnd(win.hwnd.get());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

fn ensure_session_for_enrollment(app: &AppHandle, state: &State<AppState>) -> Result<(), String> {
    if state.focused_session()?.is_some() {
        return Ok(());
    }
    let creds = crate::state::silo_credentials(app)?;
    let root = vault_dir(app)?;
    if is_fido_enrolled(&root) {
        return Err("A security key is already enrolled on this silo".into());
    }
    let session = VaultSession::open_with_device_secret(root, &creds.device_secret)
        .map_err(|e| e.to_string())?;
    crate::state::open_focused_session(app, session)
}

#[tauri::command]
pub async fn fido_enroll_primary(
    app: AppHandle,
    state: State<'_, AppState>,
    authenticator: Option<Authenticator>,
) -> Result<(), String> {
    let authenticator = authenticator.unwrap_or(Authenticator::SecurityKey);
    silentsilo_fido::require_fido_ready().map_err(|e| e.to_string())?;

    let root = vault_dir(&app)?;
    if is_fido_enrolled(&root) {
        return Err("A security key is already enrolled on this silo".into());
    }

    ensure_session_for_enrollment(&app, &state)?;

    let (vault_id, dek) = {
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| "Open a silo first, then enroll a security key".to_string())?;
        (session.vault_id.to_string(), session.dek.clone())
    };

    let challenge = silentsilo_fido::begin_enrollment(&vault_id, 0, authenticator)
        .map_err(|e| e.to_string())?;

    emit_fido_progress(&app, step_one_message(authenticator));
    let enrolled = run_fido(&app, move || {
        silentsilo_fido::complete_enrollment(&challenge)
    })
    .await?;
    let cred = enrolled.credential;

    let unlock = match enrolled.unlock {
        // The platform evaluated the PRF while creating the credential, so
        // the wrap key is already here. Asking for a second ceremony would
        // only be a second prompt and a race to lose.
        Some(unlock) => unlock,
        None => {
            emit_fido_progress(&app, step_two_message(authenticator));
            settle_between_ceremonies().await;
            let vault_id2 = vault_id.clone();
            let cred_ids = vec![cred.credential_id.clone()];
            run_fido_settling(&app, move || {
                silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id2, Some(authenticator))
            })
            .await?
        }
    };

    // Compute the FIDO-wrapped DEK purely in memory first — nothing local is
    // mutated yet, so a failure part-way leaves the vault untouched and still
    // openable with the device secret rather than half-enrolled.
    let envelope_bytes = wrap_dek_bytes(&dek, &unlock.wrap_key).map_err(|e| e.to_string())?;

    // Commit: from here on the vault is FIDO-only — the device secret alone
    // no longer decrypts it.
    std::fs::write(dek_path(&root), &envelope_bytes).map_err(|e| e.to_string())?;
    save_fido_keys(
        &root,
        &StoredFidoKeys {
            keys: vec![StoredFidoCredential {
                kind: silentsilo_vault::KIND_FIDO2.to_string(),
                credential_id: hex_encode(&cred.credential_id),
                public_key: hex_encode(&cred.public_key),
                key_slot: cred.key_slot,
                rp_id: cred.rp_id.clone(),
                label: "Primary".into(),
                wrapped_dek: hex::encode(&envelope_bytes),
                platform: authenticator == Authenticator::ThisDevice,
                revoked: false,
            }],
        },
    )
    .map_err(|e| e.to_string())?;

    // Bring the encrypted snapshot up to date, but keep the session. The silo
    // had to be open to enrol at all, and the assertion above already proved
    // this credential derives the key that unlocks it, so closing here only
    // buys a third PIN prompt in a row. A failed snapshot is worth saying out
    // loud and no reason to fail the command: the vault is already committed
    // to FIDO by this point, and reporting the enrolment as failed would be
    // the one answer that is certainly wrong.
    {
        let session_guard = state.focused_session()?;
        if let Some(session) = session_guard.as_ref()
            && let Err(e) = session.backup_locally()
        {
            crate::diagnostics::warn("enroll", format_args!("local snapshot failed: {e}"));
        }
    }
    Ok(())
}

/// Add another FIDO2 security key (any vendor). Vault must be unlocked.
#[tauri::command]
pub async fn fido_add_key(
    app: AppHandle,
    state: State<'_, AppState>,
    label: Option<String>,
    authenticator: Option<Authenticator>,
) -> Result<StoredFidoCredential, String> {
    let authenticator = authenticator.unwrap_or(Authenticator::SecurityKey);
    silentsilo_fido::require_fido_ready().map_err(|e| e.to_string())?;

    let root = vault_dir(&app)?;
    if !is_fido_enrolled(&root) {
        return Err("Enroll the first security key before adding more".into());
    }

    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let slot = keys.next_slot();
    let (vault_id, dek) = {
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| "Unlock the silo first to add a security key".to_string())?;
        (session.vault_id.to_string(), session.dek.clone())
    };

    let challenge = silentsilo_fido::begin_enrollment(&vault_id, slot, authenticator)
        .map_err(|e| e.to_string())?;

    emit_fido_progress(&app, step_one_message(authenticator));
    let enrolled = run_fido(&app, move || {
        silentsilo_fido::complete_enrollment(&challenge)
    })
    .await?;
    let cred = enrolled.credential;

    let new_id_hex = hex_encode(&cred.credential_id);
    if keys.active().any(|k| k.credential_id == new_id_hex) {
        return Err("This credential is already enrolled".into());
    }
    // Re-enrolling a key that was removed while offline: drop the tombstone,
    // or the pass that publishes the new envelope would delete it again in
    // the same breath.
    keys.keys.retain(|k| k.credential_id != new_id_hex);

    let unlock = match enrolled.unlock {
        // Already derived while the credential was created; see the primary
        // enrolment for why that is worth taking.
        Some(unlock) => unlock,
        None => {
            emit_fido_progress(&app, step_two_message(authenticator));
            settle_between_ceremonies().await;
            let vault_id2 = vault_id.clone();
            let cred_ids = vec![cred.credential_id.clone()];
            run_fido_settling(&app, move || {
                silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id2, Some(authenticator))
            })
            .await?
        }
    };

    let envelope_bytes = wrap_dek_bytes(&dek, &unlock.wrap_key).map_err(|e| e.to_string())?;
    let label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Key {}", keys.active().count() + 1));

    let stored = StoredFidoCredential {
        kind: silentsilo_vault::KIND_FIDO2.to_string(),
        credential_id: hex_encode(&cred.credential_id),
        public_key: hex_encode(&cred.public_key),
        key_slot: slot,
        rp_id: cred.rp_id.clone(),
        label,
        wrapped_dek: hex::encode(&envelope_bytes),
        platform: authenticator == Authenticator::ThisDevice,
        revoked: false,
    };

    keys.keys.push(stored.clone());
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;

    Ok(stored)
}

#[tauri::command]
pub fn fido_list_keys(app: AppHandle) -> Result<Vec<StoredFidoCredential>, String> {
    let root = vault_dir(&app)?;
    if !is_fido_enrolled(&root) {
        return Ok(Vec::new());
    }
    let keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    // Tombstones are bookkeeping, not keys: showing a removed key in the list
    // would read as the removal having failed.
    Ok(keys.active().cloned().collect())
}

/// Renames one enrolled key.
///
/// The label is the only way to tell two keys apart at a glance: the list
/// otherwise offers a slot number and twelve hex characters. Local only,
/// as far as this command goes; the next sync pass republishes the key
/// envelopes, and the label rides along with them.
#[tauri::command]
pub fn fido_rename_key(app: AppHandle, credential_id: String, label: String) -> Result<(), String> {
    let root = vault_dir(&app)?;
    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let label = label.trim();
    if label.is_empty() {
        return Err("Give the key a name.".into());
    }
    if label.chars().count() > 64 {
        return Err("That name is too long.".into());
    }

    let key = keys
        .keys
        .iter_mut()
        .find(|k| k.credential_id == credential_id.trim())
        .ok_or_else(|| "Security key not found".to_string())?;
    key.label = label.to_string();
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;
    Ok(())
}

/// Whether the removal is finished, or still owed to storage.
///
/// The difference matters to the person doing it: until the envelope is gone
/// from the bucket, the key they just removed still opens the vault from any
/// other machine, and telling them it is done would be false.
#[derive(serde::Serialize)]
pub struct RemoveKeyOutcome {
    pub published: bool,
    /// Targets the envelope was left on because they are append-only. The
    /// key still opens the vault from anything that can read them, and the
    /// only real revocation there is rotating the vault key. Named rather
    /// than counted, so the user knows which storage to think about.
    pub withheld: Vec<String>,
}

#[tauri::command]
pub async fn fido_remove_key(
    app: AppHandle,
    credential_id: String,
) -> Result<RemoveKeyOutcome, String> {
    let root = vault_dir(&app)?;
    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let credential_id = credential_id.trim().to_string();

    // "Do not leave this machine without a way in", narrower than "do not
    // leave the silo with one key": revoking a lost laptop's Touch ID key
    // must stay possible from here. The count is of *usable* keys, or an
    // unusable one would stand in for the key this machine needs.
    let removing_usable = keys.usable().any(|k| k.credential_id == credential_id);
    if removing_usable && keys.usable().count() <= 1 {
        return Err("Keep at least one security key enrolled".into());
    }
    let Some(key) = keys
        .keys
        .iter_mut()
        .find(|k| !k.revoked && k.credential_id == credential_id)
    else {
        return Err("Security key not found".into());
    };

    // Marked rather than dropped, so a delete that never reaches storage is
    // retried by every later sync pass instead of being forgotten. The key
    // stops opening this silo on this machine either way: `active()` is what
    // unlock consults.
    key.revoked = true;
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;

    // Removing it locally is not revocation once envelopes are shared: the
    // copy in the bucket would still let anyone holding the physical key
    // unlock the vault from another device.
    // Every target, not just the first: an envelope left on the second one
    // is a key that still unlocks the vault from any device that can read
    // it, which is precisely what removing it was meant to stop.
    let targets = crate::state::silo_targets(&app);
    if targets.is_empty() {
        return Ok(RemoveKeyOutcome {
            published: false,
            withheld: Vec::new(),
        });
    }

    let mut withheld = Vec::new();
    for target in &targets {
        // An append-only target is not asked. Trying and being refused would
        // read as a transient failure and be retried for ever, and the
        // outcome is the same either way: the envelope stays readable there.
        if !target.role.allows_delete() {
            withheld.push(target.label.clone());
            continue;
        }
        if let Err(e) = silentsilo_sync::revoke_key_envelope(&*target.store, &credential_id).await {
            crate::diagnostics::warn(
                "revoke",
                format_args!("the key envelope is still in storage: {e}"),
            );
            return Ok(RemoveKeyOutcome {
                published: false,
                withheld,
            });
        }
    }

    // Every target that could be asked confirmed it, so the tombstone has
    // done its job. It is dropped even when a target withheld the delete:
    // keeping it would make the app retry a deletion it will never issue.
    keys.keys.retain(|k| k.credential_id != credential_id);
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;
    Ok(RemoveKeyOutcome {
        published: true,
        withheld,
    })
}

/// What a rotation did, and what it cost.
#[derive(Debug, serde::Serialize)]
pub struct RotateOutcome {
    /// Objects re-sealed across every target that accepts writes.
    pub resealed: usize,
    /// Security keys that still open the silo. Everything else enrolled
    /// before now does not.
    pub kept: Vec<String>,
    /// Keys that stopped working, by label, so the screen can name them
    /// rather than say "some".
    pub retired: Vec<String>,
    /// The new recovery code, shown once. The old one stopped working the
    /// moment the key changed, and leaving a silo with fewer keys and no
    /// recovery code is the wrong direction to fail in.
    pub recovery_code: String,
    /// Targets the app never deletes from, so their old objects could not be
    /// re-sealed and stay readable with the old key.
    pub unchanged_targets: Vec<String>,
}

/// Changes the key everything in this silo is encrypted under. Removing a
/// security key is not always enough: on storage that keeps what it is
/// asked to delete, the envelope stays readable and the key keeps working,
/// and rotating is the only real revocation there.
///
/// `keep` names the credentials that should still open the silo, and every
/// one has to be touched during this call, because re-wrapping needs the
/// key itself. Anything not named stops working, which is the point.
/// Content is never re-encrypted: only the content KEK is re-wrapped, so a
/// terabyte rotates as fast as a megabyte.
#[tauri::command]
pub async fn vault_rotate_key(app: AppHandle, keep: Vec<String>) -> Result<RotateOutcome, String> {
    if keep.is_empty() {
        return Err(
            "Choose at least one security key to keep, or nothing would open this silo.".into(),
        );
    }

    let (old_dek, kek, root, vault_id, silo) = {
        let silo = crate::state::active_silo(&app)?;
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Unlock the silo before rotating its key".to_string())?;
        (
            session.dek.clone(),
            session.kek.clone(),
            session.paths.root.clone(),
            session.vault_id.to_string(),
            silo,
        )
    };

    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let labelled: std::collections::HashMap<String, String> = keys
        .active()
        .map(|k| (k.credential_id.clone(), k.label.clone()))
        .collect();
    for id in &keep {
        if !labelled.contains_key(id) {
            return Err(format!("{id} is not a key enrolled on this silo."));
        }
    }

    // Every touch first, before anything on disk or in storage changes. A key
    // the user cannot produce is a rotation that should not have started, and
    // finding that out half way through is how a silo ends up with a new key
    // nobody holds.
    let mut wrap_keys: Vec<(String, [u8; 32])> = Vec::new();
    for (nth, id) in keep.iter().enumerate() {
        let label = labelled.get(id).cloned().unwrap_or_default();
        emit_fido_progress(&app, &format!("Touch “{label}” to keep it working"));
        // Same platform constraint as enrolment: one ceremony at a time, and
        // the next one has to wait for the last to be torn down. Rotating two
        // keys is two ceremonies back to back, so every touch after the first
        // gets the same pause.
        if nth > 0 {
            settle_between_ceremonies().await;
        }
        let raw = hex::decode(id).map_err(|_| "That credential id is not readable.".to_string())?;
        let vault = vault_id.clone();
        let unlock = run_fido_settling(&app, move || {
            silentsilo_fido::derive_unlock_material(std::slice::from_ref(&raw), &vault, None)
        })
        .await?;
        wrap_keys.push((id.clone(), unlock.wrap_key));
    }

    // From here the silo changes. The new key is written beside the old one
    // rather than over it, so a crash before the commit below leaves the silo
    // opening exactly as it did.
    let new_dek = silentsilo_crypto::generate_dek();
    silentsilo_vault::rotation::stage_rotation(&root, &new_dek, &kek, &old_dek)
        .map_err(|e| e.to_string())?;

    // Storage second, and only now, because the staged key is on disk: an
    // object re-sealed under a key that exists only in memory is one no
    // surviving key opens.
    let mut resealed = 0;
    let mut unchanged_targets = Vec::new();
    for target in crate::state::silo_targets(&app) {
        if !target.role.allows_delete() {
            // Append-only, so its objects cannot be overwritten. Named rather
            // than skipped quietly: what is there stays readable with the old
            // key, which is the one thing rotation is meant to stop.
            unchanged_targets.push(target.label);
            continue;
        }
        let outcome =
            sync::reseal_under_new_key(&*target.store, &old_dek, &new_dek, &mut |_, _| {})
                .await
                .map_err(|e| e.to_string())?;
        if !outcome.failed.is_empty() {
            return Err(format!(
                "{} objects on {} could not be re-sealed, so the key was left unchanged. \
                 The first was {}: {}",
                outcome.failed.len(),
                target.label,
                outcome.failed[0].0,
                outcome.failed[0].1
            ));
        }
        resealed += outcome.resealed;
    }

    // The switch. Everything above can be repeated and none of it is
    // visible to the next unlock until this runs. The database is under the
    // vault key too, so it is staged first and moved into place right after
    // the keys: a silo whose key and database disagree opens exactly never.
    let staged_db = root.join("vault.db.enc.next");
    {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "The silo closed part way through the change".to_string())?;
        session
            .stage_local_backup(&new_dek, &staged_db)
            .map_err(|e| e.to_string())?;
    }

    silentsilo_vault::rotation::commit_rotation(&root).map_err(|e| e.to_string())?;
    let db_enc = root.join("vault.db.enc");
    std::fs::rename(&staged_db, &db_enc).map_err(|e| e.to_string())?;
    // The spare copy as well, or the fallback path opens the old one and
    // reports the database as corrupt.
    std::fs::copy(&db_enc, root.join("vault.db.enc.bak")).map_err(|e| e.to_string())?;

    let mut retired: Vec<String> = Vec::new();
    for credential in keys.keys.iter_mut() {
        match wrap_keys
            .iter()
            .find(|(id, _)| *id == credential.credential_id)
        {
            Some((_, wrap_key)) => {
                credential.wrapped_dek = hex::encode(
                    silentsilo_vault::wrap_dek_bytes(&new_dek, wrap_key)
                        .map_err(|e| e.to_string())?,
                );
            }
            // Not offered, so it no longer opens anything. The record is kept
            // and marked rather than deleted, so the next sync pass removes
            // its published envelope the same way an ordinary removal does.
            None => {
                if !credential.revoked {
                    retired.push(credential.label.clone());
                }
                credential.revoked = true;
            }
        }
    }
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;

    // A new recovery code, because the old one unwraps the old key and would
    // fail at the worst moment. Generated rather than offered: the silo has
    // just lost keys, so leaving it without a way back is the wrong
    // direction to fail in.
    let (recovery_code, envelope) =
        silentsilo_vault::create_recovery_envelope(&new_dek).map_err(|e| e.to_string())?;
    silentsilo_vault::save_recovery_envelope(&root, &envelope).map_err(|e| e.to_string())?;
    for target in crate::state::silo_targets(&app) {
        if target.role.allows_delete() {
            let _ = sync::push_recovery_envelope(&*target.store, &envelope).await;
        }
    }

    // Locked rather than kept open. The session in memory holds the old key,
    // and every read it makes from here would be against a silo that has
    // moved on.
    crate::commands::vault::lock_all_silos(&app);
    let _ = silo;

    Ok(RotateOutcome {
        resealed,
        kept: keep,
        retired,
        recovery_code,
        unchanged_targets,
    })
}

/// Whether a key change was started on this silo and never finished.
///
/// Cheap and local: the presence of a staged key is the whole answer. The UI
/// asks on unlock, because a silo in this state syncs against storage that is
/// part-way converted and every pass will look broken until it is finished.
#[tauri::command]
pub fn vault_rotation_pending(app: AppHandle) -> Result<bool, String> {
    Ok(silentsilo_vault::rotation::rotation_pending(&vault_dir(
        &app,
    )?))
}

/// Carries an interrupted key change through to the end, always forward:
/// once an object in storage is re-sealed, only the staged key opens it.
/// Resuming needs one key touched, and any enrolled key will do, including
/// one that did not start the rotation, because the staged key is wrapped
/// under the old vault key that every unlock produces.
#[tauri::command]
pub async fn vault_rotate_resume(app: AppHandle, credential: String) -> Result<usize, String> {
    let (old_dek, root, vault_id, silo) = {
        let silo = crate::state::active_silo(&app)?;
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Unlock the silo before finishing the key change".to_string())?;
        (
            session.dek.clone(),
            session.paths.root.clone(),
            session.vault_id.to_string(),
            silo,
        )
    };
    let _ = silo;

    if !silentsilo_vault::rotation::rotation_pending(&root) {
        return Err("There is no key change waiting to be finished.".into());
    }
    let new_dek = silentsilo_vault::rotation::load_staged_dek(&root, &old_dek)
        .map_err(|_| "The staged key cannot be read with this silo's current key.".to_string())?;

    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let label = keys
        .active()
        .find(|k| k.credential_id == credential)
        .map(|k| k.label.clone())
        .ok_or_else(|| "That key is not enrolled on this silo.".to_string())?;

    // The touch first, as when starting: a key nobody can produce leaves this
    // exactly where it was rather than half further along.
    emit_fido_progress(&app, &format!("Touch “{label}” to finish the key change"));
    let raw =
        hex::decode(&credential).map_err(|_| "That credential id is not readable.".to_string())?;
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(std::slice::from_ref(&raw), &vault_id, None)
    })
    .await?;

    // Idempotent, so whatever the interrupted pass managed is kept and only
    // the rest is done.
    let mut resealed = 0;
    for target in crate::state::silo_targets(&app) {
        if !target.role.allows_delete() {
            continue;
        }
        let outcome =
            sync::reseal_under_new_key(&*target.store, &old_dek, &new_dek, &mut |_, _| {})
                .await
                .map_err(|e| e.to_string())?;
        if !outcome.failed.is_empty() {
            return Err(format!(
                "{} objects on {} still cannot be re-sealed, so the key change is unfinished.                  The first was {}: {}",
                outcome.failed.len(),
                target.label,
                outcome.failed[0].0,
                outcome.failed[0].1
            ));
        }
        resealed += outcome.resealed;
    }

    // The encrypted copy of the database is under the vault key as well, so it
    // has to move with it. Staged first and put in place immediately after the
    // keys: a silo whose key says one thing and whose database says another
    // opens exactly never, because the plaintext working copy is wiped when
    // the silo locks and the shadow backup is all that is left.
    let staged_db = root.join("vault.db.enc.next");
    {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "The silo closed part way through the change".to_string())?;
        session
            .stage_local_backup(&new_dek, &staged_db)
            .map_err(|e| e.to_string())?;
    }

    // Wrapped for this credential's row in `fido.json` below; rotation
    // itself no longer writes an envelope to disk.
    let envelope =
        silentsilo_vault::wrap_dek_bytes(&new_dek, &unlock.wrap_key).map_err(|e| e.to_string())?;
    silentsilo_vault::rotation::commit_rotation(&root).map_err(|e| e.to_string())?;
    let db_enc = root.join("vault.db.enc");
    std::fs::rename(&staged_db, &db_enc).map_err(|e| e.to_string())?;
    // The spare copy as well, or the fallback path opens the old one and
    // reports the database as corrupt.
    std::fs::copy(&db_enc, root.join("vault.db.enc.bak")).map_err(|e| e.to_string())?;

    // Only this key is known to work now. The others were never re-wrapped,
    // and there is no way to tell from here whether the interrupted pass
    // meant to keep them, so they are marked rather than guessed about.
    for key in keys.keys.iter_mut() {
        if key.credential_id != credential {
            key.revoked = true;
        } else {
            key.wrapped_dek = hex::encode(&envelope);
        }
    }
    save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;

    let (_code, recovery) =
        silentsilo_vault::create_recovery_envelope(&new_dek).map_err(|e| e.to_string())?;
    silentsilo_vault::save_recovery_envelope(&root, &recovery).map_err(|e| e.to_string())?;

    crate::commands::vault::lock_all_silos(&app);
    Ok(resealed)
}
