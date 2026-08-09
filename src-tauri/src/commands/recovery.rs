//! The written-down code that opens the vault when every key is gone.
//!
//! Deliberately the only alternative to a security key. A user-chosen
//! passphrase would be roughly forty bits and would silently become the real
//! strength of the vault; a generated code is a hundred and sixty and cannot
//! be guessed, whatever the storage provider does with the envelope.

use silentsilo_store::ObjectStore;
use silentsilo_sync as sync;
use silentsilo_vault::{
    VaultSession, clear_recovery_envelope, create_recovery_envelope, has_recovery_code,
    load_recovery_envelope, save_recovery_envelope, unwrap_with_code,
};
use silentsilo_vfs::Vfs;
use tauri::{AppHandle, Manager, State};

use crate::state::{AppState, vault_dir};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct RecoveryStatus {
    pub enabled: bool,
    /// Unix seconds, so the UI can say how old the written-down code is.
    pub created_at: Option<i64>,
}

fn client_for(app: &AppHandle) -> Option<Box<dyn ObjectStore>> {
    crate::state::silo_store(app)
}

#[tauri::command]
pub fn recovery_status(app: AppHandle) -> Result<RecoveryStatus, String> {
    let root = vault_dir(&app)?;
    if !has_recovery_code(&root) {
        return Ok(RecoveryStatus::default());
    }
    let created_at = load_recovery_envelope(&root).ok().map(|e| e.created_at);
    Ok(RecoveryStatus {
        enabled: true,
        created_at,
    })
}

/// Generates a code and returns it — the only time it exists in readable
/// form.
///
/// Nothing derived from it is kept except the envelope, so if the UI fails
/// to show it to the user it is gone, and regenerating is the only remedy.
/// Replacing an existing code is fine and invalidates the old one, which is
/// what someone who thinks their paper copy was seen would want.
#[tauri::command]
pub async fn recovery_generate(app: AppHandle) -> Result<String, String> {
    let (code, envelope) = {
        let state = app.state::<AppState>();
        let guard = state.focused_session()?;
        let session = guard
            .as_ref()
            .ok_or_else(|| "Unlock the silo before creating a recovery code".to_string())?;
        let (code, envelope) = create_recovery_envelope(&session.dek).map_err(|e| e.to_string())?;
        save_recovery_envelope(&session.paths.root, &envelope).map_err(|e| e.to_string())?;
        (code, envelope)
    };

    // Published to every target, because the situation this exists for is a
    // machine that has never seen the vault, and a copy that does not carry
    // the envelope is a copy the code cannot open. A local-only recovery
    // code would be useless in the one case it is for.
    for target in crate::state::silo_targets(&app) {
        sync::push_recovery_envelope(&*target.store, &envelope)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(code)
}

/// Turns recovery off, locally and in the bucket.
///
/// Deleting only the local copy would leave the written-down code working
/// from any other device — the opposite of what someone asking for this
/// wants.
/// Targets the envelope was left on, because they are append-only. Empty is
/// the ordinary answer and means the code is genuinely gone.
#[tauri::command]
pub async fn recovery_disable(app: AppHandle) -> Result<Vec<String>, String> {
    clear_recovery_envelope(&vault_dir(&app)?);
    let mut withheld = Vec::new();
    // Every target: a recovery envelope left on the second one is a written
    // down code that still opens the silo, which is the opposite of what was
    // just asked for.
    for target in crate::state::silo_targets(&app) {
        if !target.role.allows_delete() {
            withheld.push(target.label);
            continue;
        }
        sync::revoke_recovery_envelope(&*target.store)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(withheld)
}

/// Opens this device's vault with the code instead of a security key.
///
/// For the case where the vault is still on this machine but the key that
/// unlocks it is not.
#[tauri::command]
pub async fn vault_unlock_with_recovery(
    app: AppHandle,
    _state: State<'_, AppState>,
    code: String,
) -> Result<silentsilo_core::VaultMeta, String> {
    let root = vault_dir(&app)?;
    let envelope = if has_recovery_code(&root) {
        load_recovery_envelope(&root).map_err(|e| e.to_string())?
    } else {
        // The local copy can be missing on a device that joined after the
        // code was created — the bucket is then the only place it exists.
        let store = client_for(&app)
            .ok_or_else(|| "No recovery code is set up for this silo.".to_string())?;
        sync::fetch_recovery_envelope(&*store)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No recovery code is set up for this silo.".to_string())?
    };

    let dek = unwrap_with_code(&envelope, &code)
        .map_err(|_| "That recovery code doesn't match this silo.".to_string())?;

    // Disk work — the working copy may be restored from its snapshot — so
    // it runs on the blocking pool rather than an async worker.
    crate::commands::fido::run_blocking(move || {
        let creds = crate::state::silo_credentials(&app)?;
        let session = VaultSession::open_with_dek(root, dek).map_err(|e| e.to_string())?;
        if session.vault_id != creds.vault_id {
            return Err("vault id mismatch".into());
        }

        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().map_err(|e| e.to_string())?;
        let meta = vfs.meta().map_err(|e| e.to_string())?;
        crate::state::open_focused_session(&app, session)?;
        Ok(meta)
    })
    .await
}

/// Rebuilds the vault on a machine that has never seen it, using only the
/// code and the bucket.
///
/// This is what the recovery code is really for. Unlocking a vault that is
/// still on this disk is the mild case; the one people actually hit is a
/// dead laptop and a lost key, where nothing survives except what is in
/// storage and what was written on paper.
#[tauri::command]
pub async fn vault_join_with_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    config: crate::commands::storage::StoreConfigInput,
    code: String,
    name: String,
    location: Option<String>,
) -> Result<silentsilo_core::VaultMeta, String> {
    let s3_config = config.into_config(None)?;
    let store = s3_config.open().map_err(|e| e.to_string())?;

    let manifest = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That bucket doesn't hold a silo.".to_string())?;
    let envelope = sync::fetch_recovery_envelope(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No recovery code was set up for this silo.".to_string())?;

    let dek = unwrap_with_code(&envelope, &code)
        .map_err(|_| "That recovery code doesn't match this silo.".to_string())?;

    // Local state starts here. Everything above could fail without leaving
    // anything behind.
    let entry =
        crate::commands::silo::register_joined_silo(&app, manifest.vault_id, &name, location)?;
    let root = entry.path.clone();

    let device_secret = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };
    silentsilo_vault::save_credentials(&silentsilo_vault::LocalVaultAuth {
        vault_id: manifest.vault_id,
        device_secret: device_secret.clone(),
    })
    .map_err(|e| e.to_string())?;
    silentsilo_vault::save_s3_config(manifest.vault_id, &s3_config).map_err(|e| e.to_string())?;

    let kek_envelope = sync::fetch_content_kek(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "This storage has no content key yet, so there is nothing here to recover.".to_string()
        })?;
    let kek = silentsilo_vault::unwrap_kek_bytes(&kek_envelope, &dek).map_err(|e| e.to_string())?;
    let session = VaultSession::provision_with_dek(
        root.clone(),
        manifest.vault_id,
        &device_secret,
        dek.clone(),
        kek,
    )
    .map_err(|e| e.to_string())?;
    Vfs::new(&session)
        .ensure_initialized()
        .map_err(|e| e.to_string())?;

    // The key envelopes come down too, so the security keys still in the
    // user's possession keep working on this machine. Without them the vault
    // would be openable only by the recovery code from here on.
    if let Ok(keys) = sync::fetch_key_envelopes(&*store).await
        && !keys.is_empty()
    {
        // Re-wrap the local envelope under a security key, as joining from
        // storage does. `provision_with_dek` wrapped it under this machine's
        // device secret, which would leave a recovered silo with a second,
        // weaker door: the folder plus that secret, no key touched. Nothing
        // reads this file on the FIDO unlock path, so replacing it locks
        // nobody out. Only when an envelope exists: a vault with no enrolled
        // key still needs the device-secret door to open at all.
        if let Some(envelope) = keys
            .iter()
            .find(|key| !key.wrapped_dek.is_empty())
            .and_then(|key| hex::decode(&key.wrapped_dek).ok())
        {
            std::fs::write(silentsilo_vault::dek_path(&root), &envelope)
                .map_err(|e| e.to_string())?;
        }
        let _ = silentsilo_vault::save_fido_keys(&root, &silentsilo_vault::StoredFidoKeys { keys });
    }
    save_recovery_envelope(&root, &envelope).map_err(|e| e.to_string())?;

    session.backup_locally().map_err(|e| e.to_string())?;
    crate::commands::silo::adopt_joined_silo(&app, &state, entry)?;

    // Replayed while the session is still owned here, before it enters the
    // shared map. It used to be inserted first and re-borrowed for the
    // replay, which held the sessions mutex for the whole restore and let
    // the background sync see a half-replayed silo. On the blocking pool,
    // because replaying a long history is sustained database work.
    let ops = sync::fetch_ops_from(&*store, &dek, 0)
        .await
        .map_err(|e| e.to_string())?;
    crate::commands::fido::run_blocking(move || {
        silentsilo_vfs::replay(&session.conn, ops).map_err(|e| e.to_string())?;
        session.backup_locally().map_err(|e| e.to_string())?;
        let meta = Vfs::new(&session).meta().map_err(|e| e.to_string())?;
        crate::state::open_focused_session(&app, session)?;
        Ok(meta)
    })
    .await
}
