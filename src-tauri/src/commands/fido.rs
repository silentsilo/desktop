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

/// Which kind of authenticator to ask Windows for when unlocking.
///
/// Left unset, Windows offers the whole passkey menu: security key, phone,
/// this device. That is the right question only when the silo genuinely has
/// both kinds enrolled. A silo whose only key is Windows Hello should go
/// straight to Hello, and one with only removable keys should not be
/// offered a phone. Mixed silos keep the menu, because there the choice is
/// real.
pub(crate) fn preferred_authenticator(keys: &StoredFidoKeys) -> Option<Authenticator> {
    let mut platform = false;
    let mut portable = false;
    for key in keys.usable() {
        if key.platform {
            platform = true;
        } else {
            portable = true;
        }
    }
    match (platform, portable) {
        (true, false) => Some(Authenticator::ThisDevice),
        (false, true) => Some(Authenticator::SecurityKey),
        _ => None,
    }
}

/// The kind of one named credential, for a ceremony that already knows
/// which key it wants. Same reasoning as `preferred_authenticator`: asking
/// for "any" puts the whole passkey menu in front of someone who has one
/// specific key to present.
pub(crate) fn authenticator_of(
    keys: &StoredFidoKeys,
    credential_id: &str,
) -> Option<Authenticator> {
    keys.usable()
        .find(|k| k.credential_id == credential_id)
        .map(|k| {
            if k.platform {
                Authenticator::ThisDevice
            } else {
                Authenticator::SecurityKey
            }
        })
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

/// Proves that whoever is asking holds one of this silo's organisation keys.
///
/// The escrow rules all reduce to this question, and it is asked at the moment
/// of the change rather than remembered from the unlock: an employee's silo is
/// unlocked all day, and "somebody proved something earlier" is not the same
/// claim as "the company's key is in this machine now".
///
/// Verified rather than assumed from a successful ceremony: the derived key has
/// to actually unwrap that organisation key's DEK envelope, which is what makes
/// this proof of the enrolled credential rather than of any credential the
/// platform was willing to offer.
pub(crate) async fn prove_organisation_key(
    app: &AppHandle,
    keys: &silentsilo_vault::StoredFidoKeys,
    what: &str,
) -> Result<silentsilo_vault::OrgProof, String> {
    let cred_ids = keys.managed_credential_ids_bytes();
    if cred_ids.is_empty() {
        return Err(format!(
            "This silo's organisation keys cannot be used on this computer, so the {what} \
             cannot be changed here."
        ));
    }
    let vault_id = crate::state::silo_credentials(app)?.vault_id.to_string();

    emit_fido_progress(
        app,
        "Touch the organisation's security key to confirm this change.",
    );
    let unlock = run_fido(app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id, None)
    })
    .await?;

    // Both halves of the question, membership and material, are asked by the
    // proof's own constructor, so this and the write path cannot drift apart
    // about what counts as proof.
    silentsilo_vault::OrgProof::verify(keys, &unlock.credential_id, &unlock.wrap_key)
        .map_err(|_| "That is not one of this silo's organisation keys.".to_string())
}

/// The authority a command writes under, having asked for a key if the silo
/// needs one.
///
/// Returns the proof rather than a flag so the caller hands the write path
/// evidence instead of an assertion. On an ordinary silo it asks for nothing
/// and answers `None`, which is what keeps every personal silo working exactly
/// as before.
async fn organisation_proof_if_needed(
    app: &AppHandle,
    keys: &silentsilo_vault::StoredFidoKeys,
    what: &str,
) -> Result<Option<silentsilo_vault::OrgProof>, String> {
    if !keys.is_org_controlled() {
        return Ok(None);
    }
    let proof = prove_organisation_key(app, keys, what).await?;
    settle_between_ceremonies().await;
    Ok(Some(proof))
}

/// Turns the optional proof into the authority the write path wants.
fn authority(proof: Option<&silentsilo_vault::OrgProof>) -> silentsilo_vault::Authority<'_> {
    match proof {
        Some(proof) => silentsilo_vault::Authority::Organisation(proof),
        None => silentsilo_vault::Authority::Machine,
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

/// Enrols the silo's first key.
///
/// `organisation` marks it as escrow: a company provisioning a silo for an
/// employee sets this, and from then on the employee cannot retire the key or
/// change the recovery code without it. Only available here, at the first
/// enrolment, and that is deliberate: a key nobody can remove must be part of
/// what the silo was set up as, never something added to a silo somebody is
/// already using.
#[tauri::command]
pub async fn fido_enroll_primary(
    app: AppHandle,
    state: State<'_, AppState>,
    authenticator: Option<Authenticator>,
    organisation: Option<bool>,
) -> Result<(), String> {
    let authenticator = authenticator.unwrap_or(Authenticator::SecurityKey);
    let policy = if organisation.unwrap_or(false) {
        // The company's way back in has to work from any machine, and Hello
        // is sealed to this one. An organisation whose escrow key dies with
        // one motherboard was never holding escrow at all.
        if authenticator == Authenticator::ThisDevice {
            return Err(
                "Windows Hello is sealed to this computer, and an organisation key \
                        has to open the silo from anywhere. Use a removable security key."
                    .into(),
            );
        }
        silentsilo_vault::POLICY_ORG.to_string()
    } else {
        String::new()
    };
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
                derivation: silentsilo_vault::DERIVATION_HMAC_V1.to_string(),
                // Set only here, at the first enrolment. A silo that starts as
                // somebody's own must never acquire keys they cannot retire.
                policy,
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
        // The silo is being created, so there is nothing to take away.
        silentsilo_vault::Authority::Machine,
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
///
/// `organisation` adds a second escrow key, which is what keeps a company from
/// depending on a single one it could lose. Two rules make that safe: the silo
/// has to have been created as organisation-administered, so a personal silo
/// can never acquire escrow it did not start with, and an existing
/// organisation key has to be present to authorise the new one.
#[tauri::command]
pub async fn fido_add_key(
    app: AppHandle,
    state: State<'_, AppState>,
    label: Option<String>,
    authenticator: Option<Authenticator>,
    organisation: Option<bool>,
) -> Result<StoredFidoCredential, String> {
    let authenticator = authenticator.unwrap_or(Authenticator::SecurityKey);
    silentsilo_fido::require_fido_ready().map_err(|e| e.to_string())?;

    let root = vault_dir(&app)?;
    if !is_fido_enrolled(&root) {
        return Err("Enroll the first security key before adding more".into());
    }

    let mut keys = load_fido_keys(&root).map_err(|e| e.to_string())?;
    let slot = keys.next_slot();

    // Asked before the new key is touched, so somebody who is about to be
    // refused is not first walked through a ceremony for nothing.
    let policy = if organisation.unwrap_or(false) {
        if authenticator == Authenticator::ThisDevice {
            return Err(
                "Windows Hello is sealed to this computer, and an organisation key \
                        has to open the silo from anywhere. Use a removable security key."
                    .into(),
            );
        }
        if !keys.is_org_controlled() {
            return Err(
                "This silo was not set up as organisation-administered, so it cannot \
                        take an organisation key. Create a new silo for that."
                    .into(),
            );
        }
        prove_organisation_key(&app, &keys, "organisation keys").await?;
        settle_between_ceremonies().await;
        silentsilo_vault::POLICY_ORG.to_string()
    } else {
        String::new()
    };
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
        derivation: silentsilo_vault::DERIVATION_HMAC_V1.to_string(),
        policy,
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
    // Adding never takes anything away, including when the key being added is
    // itself an organisation one: that already asked for an existing key above.
    save_fido_keys(&root, &keys, silentsilo_vault::Authority::Machine)
        .map_err(|e| e.to_string())?;

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
    // A label, so the set of keys that open the silo is unchanged.
    save_fido_keys(&root, &keys, silentsilo_vault::Authority::Machine)
        .map_err(|e| e.to_string())?;
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

    // An organisation's key is the company's way back into a silo it
    // provisioned, so retiring one takes another organisation key in hand.
    // Enforced here rather than hidden in the UI: this command is reachable
    // from anything that can call the backend. The key being retired counts as
    // proof of itself, which is what lets a company rotate its own key and
    // what lets it hand a silo over by retiring the last one.
    let proof = if keys
        .active()
        .any(|k| k.credential_id == credential_id && k.managed())
    {
        Some(prove_organisation_key(&app, &keys, "organisation key").await?)
    } else {
        None
    };

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
    save_fido_keys(&root, &keys, authority(proof.as_ref())).map_err(|e| e.to_string())?;

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
    save_fido_keys(&root, &keys, authority(proof.as_ref())).map_err(|e| e.to_string())?;
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

/// Puts a rotation's new key in force, together with the snapshot under it.
///
/// The two have to move as one. The plaintext working copy is wiped when the
/// silo locks, so `vault.db.enc` is all that survives, and a silo whose key
/// says one thing and whose snapshot says another opens exactly never.
/// Ordered so that the only gap a crash can land in is recoverable: the
/// staged snapshot is written first, the keys commit, then the staged file
/// is renamed into place. Dying between the last two leaves the new key in
/// force beside a snapshot under the old one, which is what
/// `VaultPaths::db_enc_staged_path` exists for and what unlock now finishes.
fn commit_keys_and_snapshot(
    app: &AppHandle,
    silo: &silentsilo_vault::SiloEntry,
    root: &std::path::Path,
    new_dek: &silentsilo_crypto::MasterDek,
) -> Result<(), String> {
    let paths = silentsilo_vault::VaultPaths::new(root.to_path_buf());
    let staged_db = paths.db_enc_staged_path();
    {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "The silo closed part way through the change".to_string())?;
        session
            .stage_local_backup(new_dek, &staged_db)
            .map_err(|e| e.to_string())?;
    }

    silentsilo_vault::rotation::commit_rotation(root).map_err(|e| e.to_string())?;
    silentsilo_core::rename_with_retry(&staged_db, &paths.db_enc_path())
        .map_err(|e| e.to_string())?;
    // The spare copy as well, or the fallback path opens the old one and
    // reports the database as corrupt.
    std::fs::copy(paths.db_enc_path(), paths.db_enc_backup_path()).map_err(|e| e.to_string())?;
    Ok(())
}

/// What to say when a rotation stops after storage has been touched.
///
/// The key is staged, some objects have moved to it, and going back is not
/// possible: only the staged key opens what has already been re-sealed. So
/// the message names the state and points at the one action that leads out
/// of it, rather than describing a failure that left nothing behind.
fn unfinished(target: &str, why: &str) -> String {
    format!(
        "The key change stopped part way through {target} ({why}). Nothing is lost: the new key \
         is saved and the objects already converted are readable with it. Finish the change from \
         Settings, with any enrolled key. Do not start another one."
    )
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

    // A rotation that stopped part way is finished, never started again.
    // Staging a second key writes over the first, and the objects the first
    // attempt already re-sealed then open under neither: not the old key,
    // which they have moved past, and not the second staged key, which never
    // saw them. That is unrecoverable, so the only way past this point is
    // `vault_rotate_resume`.
    if silentsilo_vault::rotation::rotation_pending(&root) {
        return Err(
            "A key change on this silo was started and never finished. Finish that one first: \
             starting another would strand everything the first attempt already re-sealed."
                .into(),
        );
    }

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

    // Rotation is retirement and a fresh recovery code in one operation, so
    // on an administered silo it answers to the same rule as each of them
    // alone. Without this, keeping only your own key was a one-screen way
    // around every other guard: the organisation's keys cannot be in the
    // keep list of someone who cannot touch them.
    let org_proof = organisation_proof_if_needed(&app, &keys, "encryption key").await?;

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
        let wanted = authenticator_of(&keys, id);
        let unlock = run_fido_settling(&app, move || {
            silentsilo_fido::derive_unlock_material(std::slice::from_ref(&raw), &vault, wanted)
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
        // Errors here leave a rotation staged and storage part-way through
        // it, which is exactly the state `vault_rotate_resume` exists for.
        // The message says so rather than claiming nothing happened: it once
        // said "the key was left unchanged", which sent people back to start
        // a second rotation over the objects the first had already moved.
        let outcome =
            sync::reseal_under_new_key(&*target.store, &old_dek, &new_dek, &mut |_, _| {})
                .await
                .map_err(|e| unfinished(&target.label, &e.to_string()))?;
        if !outcome.failed.is_empty() {
            return Err(unfinished(
                &target.label,
                &format!(
                    "{} objects would not re-seal, the first being {}: {}",
                    outcome.failed.len(),
                    outcome.failed[0].0,
                    outcome.failed[0].1
                ),
            ));
        }
        resealed += outcome.resealed;
    }

    // The switch. Everything above can be repeated and none of it is
    // visible to the next unlock until this runs.
    commit_keys_and_snapshot(&app, &silo, &root, &new_dek)?;

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
    save_fido_keys(&root, &keys, authority(org_proof.as_ref())).map_err(|e| e.to_string())?;

    // A new recovery code, because the old one unwraps the old key and would
    // fail at the worst moment. Generated rather than offered: the silo has
    // just lost keys, so leaving it without a way back is the wrong
    // direction to fail in.
    let (recovery_code, envelope) =
        silentsilo_vault::create_recovery_envelope(&new_dek).map_err(|e| e.to_string())?;
    silentsilo_vault::save_recovery_envelope(&root, &envelope).map_err(|e| e.to_string())?;
    publish_recovery_envelope(&app, &envelope).await;

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

/// What finishing an interrupted key change produced.
///
/// The recovery code is the reason this is a struct rather than a count.
/// Resuming mints a new one, because the old code unwraps the key that just
/// stopped being current; a resume that generated it and threw it away left
/// the silo reporting a recovery code nobody had ever seen.
#[derive(Debug, serde::Serialize)]
pub struct ResumeOutcome {
    pub resealed: usize,
    /// Shown once, exactly as a rotation started here shows it.
    pub recovery_code: String,
    /// Targets the app never deletes from, so their copy of the old
    /// recovery envelope stays readable with the old code.
    pub unchanged_targets: Vec<String>,
}

/// Carries an interrupted key change through to the end, always forward:
/// once an object in storage is re-sealed, only the staged key opens it.
/// Resuming needs one key touched, and any enrolled key will do, including
/// one that did not start the rotation, because the staged key is wrapped
/// under the old vault key that every unlock produces.
#[tauri::command]
pub async fn vault_rotate_resume(
    app: AppHandle,
    credential: String,
) -> Result<ResumeOutcome, String> {
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

    // Finishing drops every key but the chosen one, so on an administered
    // silo it is the same operation as starting and answers to the same
    // rule. An interrupted company rotation must not become an employee's
    // way to finish it with only their own key.
    let org_proof = organisation_proof_if_needed(&app, &keys, "encryption key").await?;

    // The touch first, as when starting: a key nobody can produce leaves this
    // exactly where it was rather than half further along.
    emit_fido_progress(&app, &format!("Touch “{label}” to finish the key change"));
    let raw =
        hex::decode(&credential).map_err(|_| "That credential id is not readable.".to_string())?;
    let wanted = authenticator_of(&keys, &credential);
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(std::slice::from_ref(&raw), &vault_id, wanted)
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

    // Wrapped for this credential's row in `fido.json` below; rotation
    // itself no longer writes an envelope to disk.
    let envelope =
        silentsilo_vault::wrap_dek_bytes(&new_dek, &unlock.wrap_key).map_err(|e| e.to_string())?;
    commit_keys_and_snapshot(&app, &silo, &root, &new_dek)?;

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
    save_fido_keys(&root, &keys, authority(org_proof.as_ref())).map_err(|e| e.to_string())?;

    // A new code, and one somebody actually gets to write down. The old code
    // unwraps the key that just stopped being current, so a resume that kept
    // the code to itself left the silo reporting a recovery it had no way
    // back through. Published as well as saved, for the same reason
    // `recovery_generate` publishes: the case this exists for is a machine
    // that has never seen the silo.
    let (recovery_code, recovery) =
        silentsilo_vault::create_recovery_envelope(&new_dek).map_err(|e| e.to_string())?;
    silentsilo_vault::save_recovery_envelope(&root, &recovery).map_err(|e| e.to_string())?;
    let unchanged_targets = publish_recovery_envelope(&app, &recovery).await;

    crate::commands::vault::lock_all_silos(&app);
    Ok(ResumeOutcome {
        resealed,
        recovery_code,
        unchanged_targets,
    })
}

/// Puts a fresh recovery envelope on every target that accepts a delete,
/// and names the ones that do not.
///
/// Best effort by design: the code is already in the caller's hand and is
/// about to be shown, so failing here must not swallow it. An append-only
/// target keeps whatever envelope it already has, which the caller says out
/// loud rather than leaving the user to find out.
pub(crate) async fn publish_recovery_envelope(
    app: &AppHandle,
    envelope: &silentsilo_vault::RecoveryEnvelope,
) -> Vec<String> {
    let mut unchanged = Vec::new();
    for target in crate::state::silo_targets(app) {
        if !target.role.allows_delete() {
            unchanged.push(target.label);
            continue;
        }
        if let Err(e) = sync::push_recovery_envelope(&*target.store, envelope).await {
            crate::diagnostics::warn(
                "recovery",
                format_args!("the new envelope did not reach {}: {e}", target.label),
            );
            unchanged.push(target.label);
        }
    }
    unchanged
}

#[cfg(test)]
mod authenticator_choice_tests {
    use super::preferred_authenticator;
    use silentsilo_fido::Authenticator;
    use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};

    /// A usable key of the given kind: `usable()` insists on the fido2 kind
    /// and the hmac derivation, so anything else would be filtered out and
    /// the test would pass for the wrong reason.
    fn key(platform: bool) -> StoredFidoCredential {
        StoredFidoCredential {
            kind: "fido2".into(),
            derivation: "hmac-secret-v1".into(),
            policy: String::new(),
            credential_id: "aa11".into(),
            public_key: "3059".into(),
            key_slot: 0,
            rp_id: "silentsilo.com".into(),
            label: "a key".into(),
            wrapped_dek: "ff".into(),
            platform,
            revoked: false,
        }
    }

    #[test]
    fn a_silo_that_only_has_windows_hello_asks_for_windows_hello() {
        // Left unset, Windows offers the whole passkey menu, and someone who
        // enrolled Hello is asked to find a security key they never had.
        let keys = StoredFidoKeys {
            keys: vec![key(true)],
        };
        assert_eq!(
            preferred_authenticator(&keys),
            Some(Authenticator::ThisDevice)
        );
    }

    #[test]
    fn a_silo_with_only_removable_keys_asks_for_one() {
        let keys = StoredFidoKeys {
            keys: vec![key(false)],
        };
        assert_eq!(
            preferred_authenticator(&keys),
            Some(Authenticator::SecurityKey)
        );
    }

    #[test]
    fn a_silo_with_both_keeps_the_choice() {
        let keys = StoredFidoKeys {
            keys: vec![key(true), key(false)],
        };
        assert_eq!(preferred_authenticator(&keys), None);
    }

    #[test]
    fn a_named_key_is_asked_for_by_its_own_kind() {
        // Rotation touches one key at a time and knows which: asking for
        // "any" would put the passkey menu in front of someone who has that
        // exact key in hand.
        let mut hello = key(true);
        hello.credential_id = "hello".into();
        let mut stick = key(false);
        stick.credential_id = "stick".into();
        let keys = StoredFidoKeys {
            keys: vec![hello, stick],
        };

        assert_eq!(
            super::authenticator_of(&keys, "hello"),
            Some(Authenticator::ThisDevice)
        );
        assert_eq!(
            super::authenticator_of(&keys, "stick"),
            Some(Authenticator::SecurityKey)
        );
        // A key this silo does not have decides nothing.
        assert_eq!(super::authenticator_of(&keys, "absent"), None);
    }

    #[test]
    fn a_revoked_key_does_not_decide_anything() {
        let mut revoked = key(false);
        revoked.revoked = true;
        let keys = StoredFidoKeys {
            keys: vec![key(true), revoked],
        };
        assert_eq!(
            preferred_authenticator(&keys),
            Some(Authenticator::ThisDevice)
        );
    }
}
