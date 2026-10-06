//! Signing in to OneDrive, Dropbox and Google Drive.
//!
//! The provider's page opens in the user's own browser, never in this
//! window, so the password and the second factor only ever reach the
//! provider. The tokens stay in core: the frontend gets the account to show
//! and an id that names the sign-in when a target is saved with it.

use std::sync::Mutex;

use silentsilo_vault::{CloudProvider, CloudSignIn};
use tauri::{AppHandle, Manager};

/// The sign-in in progress, so Cancel can stop it and free its port. One at
/// a time: starting another stops the first.
#[derive(Default)]
pub struct SignInSlot(Mutex<Option<tokio::sync::oneshot::Sender<()>>>);

fn provider(kind: &str) -> Result<CloudProvider, String> {
    CloudProvider::from_kind(kind).ok_or_else(|| format!("Not a provider this app knows: {kind}"))
}

/// The providers this build can sign in to, as storage kinds.
#[tauri::command(async)]
pub fn cloud_providers() -> Vec<&'static str> {
    [
        CloudProvider::OneDrive,
        CloudProvider::Dropbox,
        CloudProvider::GoogleDrive,
    ]
    .into_iter()
    .filter(|p| p.available())
    .map(|p| p.kind())
    .collect()
}

/// Opens the provider's sign-in page and waits for it to come back, for at
/// most five minutes.
#[tauri::command]
pub async fn cloud_sign_in(app: AppHandle, kind: String) -> Result<CloudSignIn, String> {
    let provider = provider(&kind)?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    {
        let slot = app.state::<SignInSlot>();
        let mut current = slot.0.lock().map_err(|e| e.to_string())?;
        if let Some(previous) = current.replace(stop) {
            let _ = previous.send(());
        }
    }
    let opener = app.clone();
    let signing_in = silentsilo_vault::cloud_sign_in(provider, move |url| {
        use tauri_plugin_opener::OpenerExt;
        opener
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    });
    // Dropping the sign-in closes its listener.
    tokio::select! {
        result = signing_in => result.map_err(|e| e.to_string()),
        // Not "cancelled": the error mapping reads that word as a key prompt.
        _ = stopped => Err("The sign-in was stopped.".into()),
    }
}

#[tauri::command(async)]
pub fn cloud_cancel_sign_in(slot: tauri::State<'_, SignInSlot>) -> Result<(), String> {
    if let Some(stop) = slot.0.lock().map_err(|e| e.to_string())?.take() {
        let _ = stop.send(());
    }
    Ok(())
}

/// Lets go of a finished sign-in nothing will save: the form was cancelled,
/// or signed in again. A Dropbox one is ended at Dropbox too.
#[tauri::command]
pub async fn cloud_discard_sign_in(sign_in: String) -> Result<(), String> {
    if let Ok(id) = uuid::Uuid::parse_str(sign_in.trim()) {
        silentsilo_vault::cancel_cloud_sign_in(id).await;
    }
    Ok(())
}

/// Every sign-in not saved yet goes when the last silo locks: nothing
/// should be able to use its tokens with no silo open.
pub fn forget_sign_ins_when_all_locked(app: &AppHandle) {
    if app
        .state::<crate::state::AppState>()
        .open_silo_ids()
        .is_empty()
    {
        tauri::async_runtime::spawn(silentsilo_vault::forget_cloud_sign_ins());
    }
}

/// The silo folders a sign-in can see, for setting up from backup storage.
#[tauri::command]
pub async fn cloud_list_silos(sign_in: String) -> Result<Vec<String>, String> {
    let id = uuid::Uuid::parse_str(sign_in.trim()).map_err(|_| "Sign in again.".to_string())?;
    silentsilo_vault::cloud_silo_folders(id)
        .await
        .map_err(|e| e.to_string())
}

/// Gives a copy a new sign-in after the old one stopped working. Refused for
/// another account: the copy would then point at an empty folder.
#[tauri::command]
pub async fn backup_target_reconnect(
    app: AppHandle,
    id: String,
    sign_in: String,
) -> Result<(), String> {
    let silo = crate::state::active_silo(&app)?;
    let target = silentsilo_vault::load_targets(silo.id)
        .into_iter()
        .find(|t| t.config.target_id().to_string() == id)
        .ok_or_else(|| "That copy is not set up for this silo.".to_string())?;
    let sign_in =
        uuid::Uuid::parse_str(sign_in.trim()).map_err(|_| "Sign in again.".to_string())?;
    silentsilo_vault::adopt_cloud_sign_in(sign_in, &target.config)
        .await
        .map_err(|e| e.to_string())
}
