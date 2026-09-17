//! Connecting a silo to somewhere its backup can live. Backup is opt-in,
//! so every command here tolerates an absent config rather than treating
//! it as an error. The UI sends whichever fields its chosen backend needs
//! and everything else is identical, because the layers below talk to
//! `ObjectStore` and do not know which one they got.

use tauri::{AppHandle, Emitter, Manager};

/// What the UI sends when saving, and what it may see of the stored
/// settings. Core's: a blank secret keeps the stored one only for the same
/// server and account, and an SFTP server answering with another key needs
/// the password again.
pub use silentsilo_app::{StoreConfigInput, StoreConfigView};

/// The attach-time gate: a place that already holds a different silo is
/// refused before anything is saved. The first pass against it would
/// overwrite the other vault's manifest and key envelopes, killing that
/// backup, and then choke forever on records this silo's key cannot open.
async fn refuse_foreign_vault(
    app: &AppHandle,
    silo: &silentsilo_vault::SiloEntry,
    store: &dyn silentsilo_store::ObjectStore,
) -> Result<(), String> {
    let vault_id = {
        let state = app.state::<crate::state::AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(&silo.id)
            .ok_or_else(|| "The silo is locked.".to_string())?
            .vault_id
    };
    silentsilo_sync::refuse_foreign_vault(store, vault_id)
        .await
        .map_err(|e| e.to_string())
}

/// Asks a server for its host key so the user can confirm it.
///
/// Separate from saving on purpose: this is the step where a person decides
/// whether the server answering on that address is theirs. Nothing is stored
/// and no credentials are sent — the key is offered before authentication.
#[tauri::command]
pub async fn sftp_probe_host_key(host: String, port: u16) -> Result<String, String> {
    silentsilo_store::probe_host_key(host.trim(), port)
        .await
        .map_err(|e| e.to_string())
}

/// `None` when backup hasn't been set up.
#[tauri::command(async)]
pub fn s3_get_config(app: AppHandle) -> Result<Option<StoreConfigView>, String> {
    Ok(crate::state::silo_store_config(&app).map(|c| StoreConfigView::from(&c)))
}

/// Verifies the details actually work before storing them, so a typo can't
/// leave backup permanently but silently broken.
#[tauri::command]
pub async fn s3_save_config(
    app: AppHandle,
    config: StoreConfigInput,
) -> Result<StoreConfigView, String> {
    let silo = crate::state::active_silo(&app)?;
    let config = config.into_config(crate::state::silo_store_config(&app))?;
    let store = config.open().map_err(|e| e.to_string())?;
    // Another silo's place is refused before the test write lands in it.
    refuse_foreign_vault(&app, &silo, &*store).await?;
    store.check().await.map_err(|e| e.to_string())?;
    // Replaces the first target and leaves any others alone. This screen
    // edits one connection, and a silo may now have more than one: saving
    // through the list is what stops the two views of the same thing
    // drifting apart.
    let mut targets = silentsilo_vault::load_targets(silo.id);
    // Editing the first connection into the same place a second one already
    // points at would leave two targets with one id. Everything that reports
    // per target then matches on that id and answers about whichever came
    // first, so one copy's backlog and failures would be shown against the
    // other. Refused here for the same reason `backup_target_add` refuses
    // it: the same place twice is not a second copy.
    if targets
        .iter()
        .skip(1)
        .any(|t| t.config.target_id() == config.target_id())
    {
        return Err("This silo already backs up there as a second copy.".into());
    }
    match targets.first_mut() {
        Some(first) => first.config = config.clone(),
        None => targets.push(silentsilo_vault::BackupTarget {
            config: config.clone(),
            label: String::new(),
            // The connection this screen edits is the one the app keeps
            // tidy: it empties the trash, sweeps unreferenced content and
            // prunes the log. A silo whose only target refused deletes would
            // grow for ever without anyone having asked for that.
            role: silentsilo_vault::TargetRole::Working,
        }),
    }
    silentsilo_vault::save_targets(silo.id, &targets).map_err(|e| e.to_string())?;
    Ok(StoreConfigView::from(&config))
}

/// Round-trips a probe object against the given details without saving them,
/// so the user can check a change before committing to it.
#[tauri::command]
pub async fn s3_test_config(app: AppHandle, config: StoreConfigInput) -> Result<(), String> {
    // Only with the silo open: a blank secret here stands for the stored
    // one, and nothing should send that anywhere while the silo is locked.
    let silo = crate::state::active_silo(&app)?;
    {
        let state = app.state::<crate::state::AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        if !sessions.contains_key(&silo.id) {
            return Err("The silo is locked.".into());
        }
    }
    let store = config
        .into_config(crate::state::silo_store_config(&app))?
        .open()
        .map_err(|e| e.to_string())?;
    // The same gate saving applies, so "Test connection" cannot pass a place
    // that saving is about to refuse; asked before the test write.
    refuse_foreign_vault(&app, &silo, &*store).await?;
    store.check().await.map_err(|e| e.to_string())
}

/// Forgets the connection details. What is already in storage is left alone
/// — it is the user's storage, and deleting their data because they
/// disconnected would be the wrong default.
#[tauri::command(async)]
pub fn s3_disconnect(app: AppHandle) -> Result<(), String> {
    let silo = crate::state::unlocked_silo(&app)?;
    // Every target, not just the first: "disconnect" means this silo stops
    // backing up, and leaving a second one configured would keep it doing
    // precisely what the user asked it to stop. Saving an empty list clears
    // the single-target slot as part of the same write, so there is nothing
    // left to clear afterwards.
    silentsilo_vault::save_targets(silo.id, &[]).map_err(|e| e.to_string())?;
    Ok(())
}

// ── More than one target ────────────────────────────────────────────

/// One configured target, as the UI lists it.
#[derive(serde::Serialize)]
pub struct BackupTargetView {
    /// Stable name of the place it points at, used as the row key and to
    /// match a target against what the last sync pass said about it.
    pub id: String,
    pub label: String,
    pub config: StoreConfigView,
    /// True for the one this silo treats as its main connection, which is
    /// the one the Backup screen edits.
    pub primary: bool,
    /// Unix seconds of the last pass this target accepted everything, 0 if
    /// it never has. A copy is not a light that is on or off, it has an age,
    /// and the age is what tells someone whether to go and find the disk.
    pub last_success: i64,
    /// Changes this target has not received. Per target, because a disk in a
    /// drawer being twelve changes behind says nothing about the bucket.
    pub ops_behind: usize,
    /// Seconds until sync tries this target again, 0 when it is due now.
    pub retry_in: i64,
    /// True when the app never sends this target a delete. It grows for
    /// ever by design, and the panel says so rather than letting the bill
    /// arrive as a surprise.
    pub archive: bool,
}

/// Every place this silo backs up to, with how each one is doing.
///
/// Reads local state only. Asking the targets themselves would make opening
/// the screen a network round trip per copy, and the answer would still be
/// about this moment rather than about the backlog, which is what the user
/// is actually asking.
/// On the blocking pool: reading the saved list is a keyring round trip per
/// target, and the numbers come from the database behind the sessions mutex,
/// which a sync pass holds. The panel refreshes on every pass, so this is
/// asked most often at exactly the moment the lock is busiest.
#[tauri::command]
pub async fn backup_targets_list(app: AppHandle) -> Result<Vec<BackupTargetView>, String> {
    crate::commands::fido::run_blocking(move || backup_targets_list_impl(&app)).await
}

fn backup_targets_list_impl(app: &AppHandle) -> Result<Vec<BackupTargetView>, String> {
    let silo = crate::state::active_silo(app)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // The settings first, with no lock held. Reading them means decrypting a
    // stored secret per target, which has nothing to ask the open silo, and
    // doing it under the mutex held the lock for the length of the keyring.
    let targets = silentsilo_vault::load_targets(silo.id);

    // Then one short locked section for the numbers, three cheap queries per
    // target and nothing else in it.
    //
    // A locked silo still lists its targets: the question "where does this
    // back up to" is worth answering without a password. What cannot be
    // answered is how far behind each one is, and 0 is the honest stand-in
    // for "not known right now".
    let numbers: Vec<(i64, usize, i64)> = {
        let state = app.state::<crate::state::AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        match sessions.get(&silo.id) {
            Some(s) => targets
                .iter()
                .map(|target| {
                    let id = target.config.target_id();
                    (
                        silentsilo_vfs::target_last_success(&s.conn, id).unwrap_or(0),
                        silentsilo_vfs::pending_count_for(&s.conn, id).unwrap_or(0),
                        silentsilo_vfs::target_retry_in(&s.conn, id, now).unwrap_or(0),
                    )
                })
                .collect(),
            None => vec![(0, 0, 0); targets.len()],
        }
    };

    Ok(targets
        .into_iter()
        .zip(numbers)
        .enumerate()
        .map(
            |(index, (target, (last_success, ops_behind, retry_in)))| BackupTargetView {
                id: target.config.target_id().to_string(),
                label: target.label,
                config: StoreConfigView::from(&target.config),
                primary: index == 0,
                last_success,
                ops_behind,
                retry_in,
                archive: !target.role.allows_delete(),
            },
        )
        .collect())
}

/// Adds a second place for this silo to back up to.
///
/// Checked before it is saved, like the first one: a target that cannot be
/// written to is not a copy, and finding that out on the next sync pass means
/// believing you have two for however long that takes.
#[tauri::command]
pub async fn backup_target_add(
    app: AppHandle,
    config: StoreConfigInput,
    label: String,
    archive: bool,
) -> Result<(), String> {
    let config = config.into_config(None)?;
    let store = config.open().map_err(|e| e.to_string())?;
    store.check().await.map_err(|e| e.to_string())?;

    let silo = crate::state::active_silo(&app)?;
    refuse_foreign_vault(&app, &silo, &*store).await?;
    let mut targets = silentsilo_vault::load_targets(silo.id);
    // The same place twice is not a second copy, however it was typed.
    if targets
        .iter()
        .any(|t| t.config.target_id() == config.target_id())
    {
        return Err("This silo already backs up there.".into());
    }
    targets.push(silentsilo_vault::BackupTarget {
        config,
        label: label.trim().to_string(),
        role: if archive {
            silentsilo_vault::TargetRole::Archive
        } else {
            silentsilo_vault::TargetRole::Working
        },
    });
    silentsilo_vault::save_targets(silo.id, &targets).map_err(|e| e.to_string())
}

/// Fills one target from another, so a large silo can be copied over a
/// cable instead of a home connection. Both targets must already be
/// configured: seeding an unconfigured place would be a copy the app then
/// forgets. Needs no vault key and decrypts nothing; interrupting is safe
/// and running it again carries on.
#[tauri::command]
pub async fn backup_target_seed(app: AppHandle, from: String, to: String) -> Result<usize, String> {
    if from == to {
        return Err("Choose two different places.".into());
    }
    let silo = crate::state::active_silo(&app)?;
    let targets = silentsilo_vault::load_targets(silo.id);
    let find = |id: &str| {
        targets
            .iter()
            .find(|t| t.config.target_id().to_string() == id)
            .ok_or_else(|| "That place is not set up for this silo.".to_string())
    };
    let source = find(&from)?.config.open().map_err(|e| e.to_string())?;
    let dest = find(&to)?.config.open().map_err(|e| e.to_string())?;

    // Progress goes out as an event rather than a return value: this runs for
    // hours on the volumes it exists for, and a spinner with no number on it
    // is what makes people pull the cable.
    let handle = app.clone();
    let state = app.state::<crate::state::AppState>();
    state
        .seed_cancelled
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let outcome = silentsilo_sync::seed_target(
        &*source,
        &*dest,
        &mut move |done, total| {
            let _ = handle.emit("seed-progress", (done, total));
        },
        &|| {
            state
                .seed_cancelled
                .load(std::sync::atomic::Ordering::Relaxed)
        },
    )
    .await
    .map_err(|e| match e {
        silentsilo_sync::SyncError::Cancelled => "cancelled".to_string(),
        other => other.to_string(),
    })?;

    if !outcome.failed.is_empty() {
        return Err(format!(
            "Copied {} of {}, and {} would not copy. The first was {}: {}",
            outcome.copied,
            outcome.copied + outcome.failed.len(),
            outcome.failed.len(),
            outcome.failed[0].0,
            outcome.failed[0].1
        ));
    }

    // A seed is a successful write to that target, so it counts as one. Without
    // this the Copies panel keeps saying "not written to yet" about a place
    // that was just filled, which is the panel contradicting the thing the
    // user watched happen.
    if let Ok(id) = uuid::Uuid::parse_str(&to) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let state = app.state::<crate::state::AppState>();
        if let Ok(sessions) = state.sessions.lock()
            && let Some(session) = sessions.get(&silo.id)
        {
            let _ = silentsilo_vfs::record_target_success(&session.conn, id, now);
        }
    }

    // The records are on that target now, but the local bookkeeping still
    // thinks it owes them, so the panel would report a backlog that is
    // already there. A pass settles it the ordinary way: `push_ops` asks
    // before writing, finds everything present and marks it delivered.
    // Reconciling by hand here would be a second implementation of that,
    // free to disagree with the first.
    let _ = crate::commands::sync::run_sync_pass(&app, &silo).await;

    Ok(outcome.copied)
}

/// Stops a running seed between objects.
///
/// Safe by construction: what already landed stays landed, and the next run
/// skips it and carries on. The flag is reset by `backup_target_seed` itself
/// at the start of each run.
#[tauri::command(async)]
pub fn cancel_seed(state: tauri::State<crate::state::AppState>) {
    state
        .seed_cancelled
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

/// What a place would do about deletion, asked before it is trusted to.
///
/// Separate from adding it, so the screen can say what it found while the
/// user is still choosing a role. Nothing is saved: this is a question, not
/// a change.
#[tauri::command]
pub async fn backup_target_protection(
    config: StoreConfigInput,
) -> Result<silentsilo_store::Protection, String> {
    let config = config.into_config(None)?;
    Ok(config.open().map_err(|e| e.to_string())?.protection().await)
}

/// Stops backing up to one target. What is already there is left alone: it is
/// the user's storage, and deleting their copy because they stopped writing
/// to it would be the wrong default.
#[tauri::command(async)]
pub fn backup_target_remove(app: AppHandle, id: String) -> Result<(), String> {
    let silo = crate::state::unlocked_silo(&app)?;
    let targets: Vec<_> = silentsilo_vault::load_targets(silo.id)
        .into_iter()
        .filter(|t| t.config.target_id().to_string() != id)
        .collect();
    silentsilo_vault::save_targets(silo.id, &targets).map_err(|e| e.to_string())
}

#[cfg(test)]
mod deserialisation_tests {
    use super::*;
    use silentsilo_store::{SftpAuth, StoreConfig};

    /// The exact JSON `storeDraftPayload` produces, for every kind.
    ///
    /// This is the seam where a rename on either side goes unnoticed until a
    /// user tries to save: the command still compiles, the form still
    /// submits, and the only symptom is an error naming a field the UI has
    /// never heard of.
    #[test]
    fn every_shape_the_ui_sends_is_a_shape_this_reads() {
        let payloads = [
            serde_json::json!({
                "kind": "s3",
                "endpoint": "http://127.0.0.1:9000",
                "region": "us-east-1",
                "bucket": "vault",
                "prefix": "",
                "accessKeyId": "silentsilo",
                "secretAccessKey": "silentsilo123",
                "pathStyle": true
            }),
            serde_json::json!({ "kind": "folder", "path": "/srv/backups/silo" }),
            serde_json::json!({
                "kind": "web-dav",
                "url": "https://cloud.example.com/remote.php/dav/files/me/silo",
                "username": "me",
                "password": "app-password"
            }),
            serde_json::json!({
                "kind": "sftp",
                "host": "nas.example.com",
                "port": 22,
                "username": "alex",
                "path": "backups/silo",
                "auth": { "method": "password", "password": "hunter2" },
                "hostFingerprint": "SHA256:abc"
            }),
            serde_json::json!({
                "kind": "sftp",
                "host": "nas.example.com",
                "port": 22,
                "username": "alex",
                "path": "backups/silo",
                "auth": {
                    "method": "key",
                    "privateKey": "-----BEGIN OPENSSH PRIVATE KEY-----",
                    "passphrase": null
                },
                "hostFingerprint": "SHA256:abc"
            }),
        ];

        for payload in payloads {
            let kind = payload["kind"].clone();
            serde_json::from_value::<StoreConfigInput>(payload)
                .unwrap_or_else(|e| panic!("the UI payload for {kind} must deserialise: {e}"));
        }
    }

    #[test]
    fn an_sftp_server_whose_fingerprint_was_never_confirmed_is_refused() {
        let json = serde_json::json!({
            "kind": "sftp",
            "host": "nas.example.com",
            "port": 22,
            "username": "alex",
            "path": "backups/silo",
            "auth": { "method": "password", "password": "hunter2" },
            "hostFingerprint": null
        });
        let input: StoreConfigInput = serde_json::from_value(json).unwrap();

        let Err(err) = input.into_config(None) else {
            panic!("a server nobody has vouched for must not be saved");
        };
        assert!(err.contains("fingerprint"), "got: {err}");
    }

    #[test]
    fn editing_an_sftp_connection_keeps_the_password_and_the_fingerprint() {
        // Blank secrets mean "unchanged", the same rule the other backends
        // follow — otherwise changing the directory would mean pasting a
        // private key back in.
        let stored = StoreConfig::Sftp(silentsilo_store::SftpConfig {
            host: "nas.example.com".into(),
            port: 22,
            username: "alex".into(),
            auth: SftpAuth::Password {
                password: "hunter2".into(),
            },
            path: "backups/silo".into(),
            host_fingerprint: Some("SHA256:abc".into()),
        });
        let json = serde_json::json!({
            "kind": "sftp",
            "host": "nas.example.com",
            "port": 22,
            "username": "alex",
            "path": "backups/silo-2",
            "auth": { "method": "password", "password": null },
            "hostFingerprint": null
        });
        let input: StoreConfigInput = serde_json::from_value(json).unwrap();

        let StoreConfig::Sftp(config) = input.into_config(Some(stored)).unwrap() else {
            panic!("kind changed");
        };
        assert_eq!(config.path, "backups/silo-2");
        assert_eq!(config.host_fingerprint.as_deref(), Some("SHA256:abc"));
        assert!(matches!(
            config.auth,
            SftpAuth::Password { password } if password == "hunter2"
        ));
    }

    #[test]
    fn switching_backends_does_not_inherit_the_other_ones_secret() {
        // A stored bucket secret must not stand in for a missing SFTP
        // password: the two are unrelated credentials that happen to sit in
        // the same slot.
        let stored = StoreConfig::S3(silentsilo_core::S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            bucket: "vault".into(),
            prefix: String::new(),
            access_key_id: "silentsilo".into(),
            secret_access_key: "silentsilo123".into(),
            path_style: true,
        });
        let json = serde_json::json!({
            "kind": "sftp",
            "host": "nas.example.com",
            "port": 22,
            "username": "alex",
            "path": "backups/silo",
            "auth": { "method": "password", "password": null },
            "hostFingerprint": "SHA256:abc"
        });
        let input: StoreConfigInput = serde_json::from_value(json).unwrap();

        assert!(input.into_config(Some(stored)).is_err());
    }
}
