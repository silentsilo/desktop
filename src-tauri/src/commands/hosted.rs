//! Connecting a silo to hosted storage, via device authorization (the
//! shape `gh auth login` uses): the app shows a short code, the person
//! approves it in a browser, and the credentials come back here. Card
//! details never touch this process and no password is asked for.
//!
//! The service is a convenience, not a dependency: everything here fills
//! in the same `StoreConfigInput::S3` a person could type by hand, and a
//! connected silo keeps working whether or not the service is reachable.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::commands::storage::StoreConfigInput;

/// Where the pairing endpoints live.
///
/// Overridable at build time so a development build can point at a local
/// service. Only ever a public address: nothing about the deployment belongs
/// in this repository.
fn service_base() -> String {
    std::env::var("SILENTSILO_STORAGE_URL").unwrap_or_else(|_| {
        option_env!("SILENTSILO_STORAGE_URL")
            .unwrap_or("https://storage.silentsilo.com")
            .to_string()
    })
}

#[derive(Debug, Serialize)]
pub struct PairingStart {
    /// Shown to the person so they can type it into the browser.
    pub user_code: String,
    /// Kept by this process and never displayed. The frontend holds it only
    /// to hand back on the next poll.
    pub device_code: String,
    pub verification_url: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Deserialize)]
struct StartResponse {
    #[serde(rename = "deviceCode")]
    device_code: String,
    #[serde(rename = "userCode")]
    user_code: String,
    #[serde(rename = "expiresIn")]
    expires_in: i64,
    interval: i64,
    #[serde(rename = "verificationUrl")]
    verification_url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct HostedStorage {
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    #[serde(rename = "accessKeyId")]
    access_key_id: String,
    #[serde(rename = "secretAccessKey")]
    secret_access_key: String,
    #[serde(rename = "pathStyle")]
    path_style: bool,
    /// Reads what this silo occupies and what the account bought. Absent
    /// from a service older than the endpoint that answers it, which is why
    /// it defaults rather than failing the pairing.
    #[serde(rename = "usageToken", default)]
    usage_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HostedDisplay {
    /// Masked, e.g. `a***@example.com`. Enough for the person to recognise
    /// their own account, not enough to learn anyone else's.
    pub account: String,
    pub label: String,
    pub plan: String,
    pub region: String,
    #[serde(rename = "portalUrl")]
    pub portal_url: String,
}

#[derive(Debug, Deserialize)]
struct PollBody {
    status: String,
    result: Option<PollResult>,
}

#[derive(Debug, Deserialize)]
struct PollResult {
    storage: HostedStorage,
    display: HostedDisplay,
}

/// What the frontend gets back while waiting.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PairingPoll {
    Pending,
    SlowDown,
    Expired,
    Cancelled,
    /// Approved, but not applied yet. The person has to confirm the account
    /// is theirs first: see `hosted_confirm`.
    Approved {
        display: HostedDisplay,
    },
}

/// The offer waiting for a yes or no, held in memory only.
///
/// Never written to disk. If the app closes between approval and
/// confirmation, the pairing is simply lost and the person starts again,
/// which is the safe direction to fail in.
#[derive(Default)]
pub struct PendingPairing(pub std::sync::Mutex<Option<PendingConfig>>);

pub struct PendingConfig {
    device_code: String,
    storage: HostedStorage,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

/// How much of the bought space is in use, for the silo on screen.
///
/// `None` whenever the answer is not available: the silo is not on bought
/// space, it was paired before the service issued tokens, or the service is
/// unreachable. All three are the same thing to the screen, which is a line
/// that does not appear, and none of them is an error worth showing: the
/// figure is a courtesy, and the vault does not depend on it.
#[derive(Debug, serde::Serialize)]
pub struct HostedUsage {
    /// What this silo holds.
    pub silo_bytes: u64,
    /// What everything on the account holds. The allowance is per account,
    /// so a second silo eats into the same one and headroom cannot be
    /// worked out from this silo alone.
    pub account_bytes: u64,
    pub quota_bytes: u64,
    /// active | past_due | grace | readonly | cancelled
    pub status: String,
    /// When the service last recomputed the figure, ISO 8601. It trails
    /// reality by up to a few hours, so the screen says as much rather than
    /// presenting it as live.
    pub measured_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct UsageResponse {
    #[serde(rename = "siloBytes")]
    silo_bytes: u64,
    #[serde(rename = "accountBytes")]
    account_bytes: u64,
    #[serde(rename = "quotaBytes")]
    quota_bytes: u64,
    status: String,
    #[serde(rename = "measuredAt")]
    measured_at: Option<String>,
}

#[tauri::command]
pub async fn hosted_usage(app: AppHandle) -> Result<Option<HostedUsage>, String> {
    let silo = crate::state::active_silo(&app)?;
    let Some(token) = silentsilo_vault::load_targets(silo.id)
        .into_iter()
        .find(|t| t.hosted)
        .and_then(|t| t.usage_token)
    else {
        return Ok(None);
    };

    let response = client()?
        .get(format!("{}/api/silos/usage", service_base()))
        .bearer_auth(token)
        .send()
        .await;

    // Every failure is the same answer. The figure is decoration on a screen
    // and this call is the one place the app talks to the service outside
    // pairing, so it must never turn into an error the user has to dismiss.
    let Ok(response) = response else {
        return Ok(None);
    };
    if !response.status().is_success() {
        return Ok(None);
    }
    let Ok(body) = response.json::<UsageResponse>().await else {
        return Ok(None);
    };

    Ok(Some(HostedUsage {
        silo_bytes: body.silo_bytes,
        account_bytes: body.account_bytes,
        quota_bytes: body.quota_bytes,
        status: body.status,
        measured_at: body.measured_at,
    }))
}

#[tauri::command]
pub async fn hosted_pair_start() -> Result<PairingStart, String> {
    let response = client()?
        .post(format!("{}/api/pair/start", service_base()))
        .send()
        .await
        .map_err(|_| "Could not reach SilentSilo storage. Check your connection.".to_string())?;

    if !response.status().is_success() {
        return Err("SilentSilo storage is not answering right now. Try again shortly.".into());
    }

    let body: StartResponse = response
        .json()
        .await
        .map_err(|e| format!("Unexpected reply from SilentSilo storage: {e}"))?;

    Ok(PairingStart {
        user_code: body.user_code,
        device_code: body.device_code,
        verification_url: body.verification_url,
        expires_in: body.expires_in,
        interval: body.interval,
    })
}

/// Asks whether the browser side is done.
///
/// On approval the credentials are parked in memory and only the masked
/// account is handed to the UI. Nothing is written until the person has
/// looked at that account and said it is theirs.
#[tauri::command]
pub async fn hosted_pair_poll(app: AppHandle, device_code: String) -> Result<PairingPoll, String> {
    use tauri::Manager;

    let response = client()?
        .post(format!("{}/api/pair/poll", service_base()))
        .json(&serde_json::json!({ "deviceCode": device_code }))
        .send()
        .await
        .map_err(|_| "Could not reach SilentSilo storage. Check your connection.".to_string())?;

    let body: PollBody = response
        .json()
        .await
        .map_err(|e| format!("Unexpected reply from SilentSilo storage: {e}"))?;

    match body.status.as_str() {
        "pending" => Ok(PairingPoll::Pending),
        "slow_down" => Ok(PairingPoll::SlowDown),
        "cancelled" => Ok(PairingPoll::Cancelled),
        "approved" => {
            let result = body.result.ok_or_else(|| {
                "SilentSilo storage approved the pairing but sent nothing".to_string()
            })?;
            let display = result.display;
            *app.state::<PendingPairing>()
                .0
                .lock()
                .map_err(|e| e.to_string())? = Some(PendingConfig {
                device_code,
                storage: result.storage,
            });
            Ok(PairingPoll::Approved { display })
        }
        _ => Ok(PairingPoll::Expired),
    }
}

/// The person said the account is theirs. Only now does anything get
/// written, and a prefix already holding a different vault is refused
/// before the config is stored. `additional` adds the target alongside
/// what the silo already backs up to rather than replacing it, which is
/// the whole point of the Copies panel.
#[tauri::command]
pub async fn hosted_pair_confirm(app: AppHandle, additional: bool) -> Result<(), String> {
    use tauri::Manager;

    // Left in place until the write succeeds. `backup_target_add` resolves
    // `Hosted` from here, and it is what marks the target as bought space so
    // the screen stops offering to sell a second allocation.
    if additional {
        crate::commands::storage::backup_target_add(
            app.clone(),
            StoreConfigInput::Hosted {},
            "SilentSilo storage".to_string(),
            false,
        )
        .await?;
        clear_pending(&app);
        return Ok(());
    }

    let pending = {
        let state = app.state::<PendingPairing>();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.take()
    };
    let pending = pending.ok_or_else(|| "There is nothing waiting to be connected.".to_string())?;
    // Read before the pairing is consumed below, because that is the last
    // moment it exists.
    let token = Some(pending.storage.usage_token.clone()).filter(|t| !t.is_empty());
    crate::commands::storage::s3_save_config(app.clone(), input_for(pending.storage)).await?;
    // The connection this replaced may have been the user's own storage, so
    // the flag is set rather than assumed, and it is what the Copies panel
    // reads to know an allocation is already in use.
    mark_first_target_hosted(&app, token);
    Ok(())
}

fn clear_pending(app: &AppHandle) {
    use tauri::Manager;
    if let Ok(mut guard) = app.state::<PendingPairing>().0.lock() {
        *guard = None;
    }
}

fn mark_first_target_hosted(app: &AppHandle, usage_token: Option<String>) {
    let Ok(silo) = crate::state::active_silo(app) else {
        return;
    };
    let mut targets = silentsilo_vault::load_targets(silo.id);
    if let Some(first) = targets.first_mut() {
        first.hosted = true;
        first.label = "SilentSilo storage".to_string();
        first.usage_token = usage_token;
        let _ = silentsilo_vault::save_targets(silo.id, &targets);
    }
}

fn input_for(storage: HostedStorage) -> StoreConfigInput {
    StoreConfigInput::S3 {
        endpoint: storage.endpoint,
        region: storage.region,
        bucket: storage.bucket,
        prefix: storage.prefix,
        access_key_id: storage.access_key_id,
        secret_access_key: Some(storage.secret_access_key),
        path_style: storage.path_style,
    }
}

/// The usage token from the pairing that is waiting, without consuming it.
///
/// Separate from `pending_input` because the token is not part of how to
/// reach the bucket: it is about the account, and it is stored on the target
/// rather than in the storage settings.
pub(crate) fn pending_usage_token(app: &AppHandle) -> Option<String> {
    use tauri::Manager;
    let guard = app.state::<PendingPairing>();
    let held = guard.0.lock().ok()?;
    held.as_ref()
        .map(|p| p.storage.usage_token.clone())
        .filter(|t| !t.is_empty())
}

/// The pairing that is waiting, as storage settings, without consuming it:
/// setting a computer up from a backup needs the same details twice, once
/// to look and once to pull. Returned to the backend only, never the
/// frontend: it carries the secret access key.
pub fn pending_input(app: &AppHandle) -> Option<StoreConfigInput> {
    use tauri::Manager;
    let state = app.state::<PendingPairing>();
    let guard = state.0.lock().ok()?;
    let pending = guard.as_ref()?;
    Some(input_for(pending.storage.clone()))
}

/// The person said no, or closed the dialog.
///
/// Best effort by contract: the service is told so it can revoke the key it
/// just issued, but a failure here is not worth surfacing. The connection
/// stays visible in the account portal either way, which is the path that
/// always works.
#[tauri::command]
pub async fn hosted_pair_cancel(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let pending = {
        let state = app.state::<PendingPairing>();
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.take()
    };
    let Some(pending) = pending else {
        return Ok(());
    };

    let _ = client()?
        .post(format!("{}/api/pair/cancel", service_base()))
        .json(&serde_json::json!({ "deviceCode": pending.device_code }))
        .send()
        .await;

    Ok(())
}

/// Where to send someone who wants to change their plan or revoke a device.
#[tauri::command]
pub fn hosted_portal_url() -> String {
    format!("{}/account", service_base())
}
