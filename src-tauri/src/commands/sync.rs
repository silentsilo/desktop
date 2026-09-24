//! Driving a sync pass from the app.
//!
//! Every command here is a no-op when no bucket is configured: sync is
//! optional, and a purely local vault should never see an error about
//! storage it deliberately doesn't use.

use silentsilo_core::VaultMeta;
use silentsilo_store::ObjectStore;
use silentsilo_sync as sync;
use silentsilo_vault::{LocalVaultAuth, SiloEntry, StoredFidoKeys, VaultSession, save_credentials};
use silentsilo_vfs::{Vfs, pending_count, replay};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::fido::{emit_fido_progress, run_fido};
use crate::state::AppState;

pub use silentsilo_app::SyncReport;

/// This app as core's pass sees it: events go to the window under the
/// names it always had, warnings to the diagnostics log, and the copies
/// come from the stored target list.
struct DesktopHost<'a> {
    app: &'a AppHandle,
}

impl silentsilo_app::Host for DesktopHost<'_> {
    fn emit(&self, event: silentsilo_app::AppEvent) {
        use silentsilo_app::AppEvent;
        let _ = match event {
            AppEvent::SyncReport(report) => self.app.emit("sync-report", report),
            AppEvent::VaultChanged => self.app.emit("vault-changed", ()),
            AppEvent::SyncProgress(progress) => self.app.emit("sync-progress", progress),
        };
    }

    fn warn(&self, area: &str, detail: &str) {
        crate::diagnostics::warn(area, detail);
    }

    fn targets(&self, silo_id: Uuid) -> Vec<silentsilo_vault::BackupTarget> {
        silentsilo_vault::load_targets(silo_id)
    }
}

/// What a purge leaves of the content it was the last reference to: core's
/// rule, with this app's copies.
pub(crate) fn release_purged_blobs(
    app: &AppHandle,
    silo_id: Uuid,
    root: &std::path::Path,
    blob_ids: &[Uuid],
) {
    silentsilo_app::files::release_purged_blobs(&DesktopHost { app }, silo_id, root, blob_ids);
}

/// How far through the download a join is.
///
/// `total` is known before the first object comes down, because the listing
/// arrives whole, so this is a real proportion rather than a count going up
/// towards an unknown end.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JoinProgress {
    pub fetched: usize,
    pub total: usize,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SyncStatus {
    pub configured: bool,
    /// Locally authored changes not yet in the bucket.
    pub pending_ops: usize,
    /// How many targets never accept a delete. Polled with the rest so the
    /// delete-for-good dialogs can stop saying "permanently": on one of
    /// these the bytes stay until the storage's own retention lets them go,
    /// and a confirmation that promises otherwise is the app lying at the
    /// exact moment someone is counting on it.
    pub archive_targets: usize,
}

/// Never touches the network, but it does read the stored settings (a
/// keyring round trip per target) and count rows behind the sessions lock,
/// and the window polls it every twenty seconds. On the blocking pool, so a
/// poll that lands while a pass holds the lock waits there rather than in
/// front of the message loop.
#[tauri::command]
pub async fn sync_status(app: AppHandle) -> Result<SyncStatus, String> {
    crate::commands::fido::run_blocking(move || sync_status_impl(&app)).await
}

fn sync_status_impl(app: &AppHandle) -> Result<SyncStatus, String> {
    let configured = crate::state::silo_store_config(app).is_some();
    // Read from the saved list rather than by opening anything: this is
    // polled, and a role is a setting, not a question for the network.
    // Before the lock, because it is disk and keyring work that has nothing
    // to ask the open silo.
    let archive_targets = crate::state::active_silo(app)
        .map(|silo| {
            silentsilo_vault::load_targets(silo.id)
                .iter()
                .filter(|t| !t.role.allows_delete())
                .count()
        })
        .unwrap_or(0);
    let pending = {
        let state = app.state::<AppState>();
        let guard = state.focused_session()?;
        match guard.as_ref() {
            Some(session) => pending_count(&session.conn).map_err(|e| e.to_string())?,
            None => 0,
        }
    };
    Ok(SyncStatus {
        configured,
        pending_ops: pending,
        archive_targets,
    })
}

/// One full pass: operations out, operations in, then blob content.
/// Operations before blobs: a visible file that cannot open yet
/// self-corrects on the next pass, while unreferenced content looks like an
/// orphan and may get cleaned up.
#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<SyncReport, String> {
    // The silo on screen: pressing Sync is about the one being looked at,
    // unlike the background pass, which is about all of them.
    let silo = crate::state::active_silo(&app)?;
    // Pressing the button clears every backoff timer first. The wait exists
    // to stop this device hammering a target that is not answering, which is
    // not the situation when someone is sitting there asking for a pass now.
    let configured = silentsilo_vault::load_targets(silo.id);
    if let Ok(state) = app.state::<crate::state::AppState>().sessions.lock()
        && let Some(session) = state.get(&silo.id)
    {
        for target in &configured {
            let _ = silentsilo_vfs::reset_target_backoff(&session.conn, target.config.target_id());
        }
    }
    // Someone pressed a button and is owed an answer about their own silo, so
    // a background pass already running is waited out rather than reported as
    // nothing to do. Bounded: a pass that never finishes must not leave the
    // button spinning for the rest of the session.
    wait_for_pass_to_finish(&app, WAIT_FOR_PASS).await;
    run_sync_pass(&app, &silo).await
}

/// How long a pressed Sync waits for a pass already running.
const WAIT_FOR_PASS: std::time::Duration = std::time::Duration::from_secs(90);

async fn wait_for_pass_to_finish(app: &AppHandle, limit: std::time::Duration) {
    let deadline = std::time::Instant::now() + limit;
    while app
        .state::<AppState>()
        .sync_in_flight
        .load(Ordering::SeqCst)
    {
        if std::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Keeps sync passes off while a key operation rewrites what a pass reads.
///
/// Waits for a running pass to end, then holds the same flag a pass takes,
/// so the background loop stands down until the guard drops. A pass that
/// loaded `fido.json` before a key change and saved it back after put the
/// old envelopes over the new ones, and one still pushing during a rotation
/// sealed records under the key being retired.
pub(crate) async fn hold_sync(app: &AppHandle) -> Result<SyncGuard, String> {
    let deadline = std::time::Instant::now() + WAIT_FOR_PASS;
    loop {
        if let Some(guard) = SyncGuard::acquire(app) {
            return Ok(guard);
        }
        if std::time::Instant::now() >= deadline {
            return Err("A sync is still running. Try again in a moment.".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Guards a pass so only one runs at a time.
///
/// Released on drop, so an error or an early return can't leave sync wedged
/// off for the rest of the session.
pub(crate) struct SyncGuard(AppHandle);

impl SyncGuard {
    fn acquire(app: &AppHandle) -> Option<Self> {
        app.state::<AppState>()
            .sync_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(app.clone()))
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        self.0
            .state::<AppState>()
            .sync_in_flight
            .store(false, Ordering::SeqCst);
    }
}

/// Syncs one silo, named rather than assumed: more than one can be open,
/// and the background pass has to reach all of them. Core's pass, with this
/// app as its host: one implementation, the one the tests exercise.
pub(crate) async fn run_sync_pass(app: &AppHandle, silo: &SiloEntry) -> Result<SyncReport, String> {
    let state = app.state::<AppState>();
    silentsilo_app::run_sync_pass(&state.core, &DesktopHost { app }, silo).await
}

/// The local half of a join: the silo folder, the content key it is opened
/// under, the published key envelopes and the recovery envelope.
///
/// Core's code, not a copy of it, and every door into a silo on this device
/// goes through here: the two joins and the repair. The reason is the
/// envelopes. Nothing signs the `policy` field one carries, so an `org`
/// planted on a key nobody can prove would refuse rotation and recovery-code
/// changes on this device for good. Core keeps that claim only on the key
/// that proved itself in this join and clears it everywhere else, and it
/// drops revoked keys, implausible credential ids and any key a sealed
/// marker names. A second implementation of that rule here would be a
/// second thing to get wrong.
///
/// It never mints a content key either: a joining device that did would
/// split the silo in a way no later sync repairs.
pub(crate) async fn provision_joined_silo(
    store: &dyn ObjectStore,
    join: &silentsilo_app::flows::RecoveryJoin,
    root: std::path::PathBuf,
    device_secret: &str,
) -> Result<VaultSession, String> {
    silentsilo_app::flows::recovery_join_provision(store, join, root, device_secret).await
}

#[tauri::command]
pub async fn vault_rebuild_from_snapshot(app: AppHandle, silo_id: String) -> Result<usize, String> {
    // Named, not focused. The dialog that asks for this can sit on screen
    // while the user steps back to the picker and opens something else, and
    // a rebuild resolved from the focus then replaced the wrong silo: new
    // device identity, local changes not yet pushed thrown away, on a silo
    // nobody asked about.
    let silo_id = Uuid::parse_str(&silo_id).map_err(|e| e.to_string())?;
    let silo = crate::state::silo_by_id(&app, silo_id)?;
    // A pass running alongside would replay records it fetched against the
    // old horizon onto the rebuilt tree.
    let _sync = hold_sync(&app).await?;
    let targets = crate::state::targets_for(silo.id);
    if targets.is_empty() {
        return Err("This silo has no backup storage configured.".into());
    }

    let dek = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(&silo.id)
            .ok_or("Unlock the silo first.")?
            .dek
            .clone()
    };

    // The most current copy serves the rebuild: the highest horizon, then
    // the most records above it. The first copy with any snapshot used to
    // win, and a stale disk first in the list restored old state and threw
    // away what the other copies had.
    let mut plan: Option<(
        silentsilo_vfs::snapshot::Snapshot,
        Vec<silentsilo_vfs::OpRecord>,
    )> = None;
    for target in &targets {
        if let Ok(Some(found)) = sync::fetch_rebuild(&*target.store, &dek).await {
            let better = plan.as_ref().is_none_or(|(best, best_ops)| {
                (found.0.horizon, found.1.len()) > (best.horizon, best_ops.len())
            });
            if better {
                plan = Some(found);
            }
        }
    }
    let (snapshot, incoming) =
        plan.ok_or("Backup storage holds nothing to rebuild this silo from.")?;

    // The one long operation that has to hold the sessions lock: a
    // half-rebuilt tree must not answer queries. Blocking pool regardless,
    // so the async runtime keeps serving everything else.
    let applied = crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(&silo.id)
            .ok_or("The silo was locked during the rebuild.")?;
        let applied = sync::apply_rebuild(&mut session.conn, &snapshot, incoming)
            .map_err(|e| e.to_string())?
            .replay
            .applied;
        drop(sessions);
        let _ = app.emit("vault-changed", ());
        Ok(applied)
    })
    .await?;

    Ok(applied)
}

/// Downloads a blob this device doesn't hold — either another device
/// uploaded it, or the local cache evicted it to reclaim space.
#[tauri::command]
pub async fn sync_fetch_blob(
    app: AppHandle,
    silo_id: String,
    blob_id: String,
) -> Result<(), String> {
    // The silo is named by the caller, because this is driven in a loop from
    // the UI: "download everything" is hundreds of calls, and switching silos
    // half way through sent the rest of them asking the new silo's storage
    // for the old silo's content. Nothing was corrupted, since no two silos
    // share a bucket, but every remaining fetch failed and the report blamed
    // the storage.
    let silo_id = Uuid::parse_str(&silo_id).map_err(|e| e.to_string())?;
    let silo = crate::state::silo_by_id(&app, silo_id)?;
    let blob_id = Uuid::parse_str(&blob_id).map_err(|e| e.to_string())?;
    let targets = crate::state::targets_for(silo.id);
    if targets.is_empty() {
        return Err(
            "That file is not on this computer, and no backup storage is connected.".into(),
        );
    }
    let stores: Vec<(Uuid, &dyn ObjectStore)> = targets.iter().map(|t| (t.id, &*t.store)).collect();
    // A copy that would not open is missing from `targets`, and may be the
    // one that holds it.
    let every_copy = targets.len() == silentsilo_vault::load_targets(silo.id).len();
    sync::fetch_blob_from_targets(&stores, &silo.path, blob_id, every_copy)
        .await
        .map_err(|e| e.to_string())?;
    settle_fetched(&silo);
    Ok(())
}

/// Content just downloaded is on the copy it came from: settled against every
/// configured target, so it stops counting as waiting to back up.
fn settle_fetched(silo: &SiloEntry) {
    let every_target: Vec<Uuid> = silentsilo_vault::load_targets(silo.id)
        .iter()
        .map(|t| t.config.target_id())
        .collect();
    let _ = silentsilo_vault::settle_blob_delivery(&silo.path, &every_target);
}

/// What a bucket looks like to a device that hasn't joined it yet.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct JoinPreview {
    /// The vault the bucket already holds, if any.
    pub vault_id: Option<String>,
    /// Labels of the security keys that can open it, so the user knows
    /// which one to reach for before being prompted to touch it.
    pub key_labels: Vec<String>,
}

/// Looks at a bucket without changing anything, local or remote.
///
/// Deliberately read-only: this runs before the user has committed to
/// anything, and a device that turns out to be pointed at the wrong bucket
/// should leave no trace in it.
#[tauri::command]
pub async fn vault_preview_join(
    config: crate::commands::storage::StoreConfigInput,
) -> Result<JoinPreview, String> {
    // The bucket details are passed in rather than read from a silo,
    // because at this point there is no silo — that is the whole situation.
    let store = config
        .into_config(None)?
        .open()
        .map_err(|e| e.to_string())?;
    let Some(manifest) = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(JoinPreview::default());
    };
    let labels = sync::fetch_key_envelopes(&*store)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|k| k.label)
        .collect();
    Ok(JoinPreview {
        vault_id: Some(manifest.vault_id.to_string()),
        key_labels: labels,
    })
}

/// Joins the vault the configured bucket already holds. The security key
/// unwraps a key envelope published by another device, and that DEK is what
/// makes this device part of the same vault rather than a new one.
#[tauri::command]
pub async fn vault_join_from_storage(
    app: AppHandle,
    state: State<'_, AppState>,
    config: crate::commands::storage::StoreConfigInput,
    name: String,
    location: Option<String>,
) -> Result<VaultMeta, String> {
    silentsilo_fido::require_fido_ready().map_err(|e| e.to_string())?;

    let s3_config = config.into_config(None)?;
    let store = s3_config.open().map_err(|e| e.to_string())?;

    let manifest = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "That backup storage does not hold a silo yet. Create one first, and sync it once."
                .to_string()
        })?;
    let vault_id = manifest.vault_id;

    let envelopes = sync::fetch_key_envelopes(&*store)
        .await
        .map_err(|e| e.to_string())?;
    if envelopes.is_empty() {
        return Err(
            "No keys have reached this backup storage yet. Sync once from the computer that has the silo, then try again."
                .into(),
        );
    }
    let keys = StoredFidoKeys { keys: envelopes };
    // Refuse by name rather than fail in the middle. The silo has keys, this
    // computer simply cannot produce any of them: a Touch ID key from a Mac,
    // read by a Windows build. Saying so is the whole point of the kind
    // field, and the alternative is a security-key prompt that no key on the
    // desk can ever answer.
    if keys.usable().next().is_none() {
        return Err(
            "This silo's keys are all of a kind this computer cannot use. Enrol a security key on it from a computer that can already open the silo, then set up from backup storage with that key."
                .into(),
        );
    }
    let cred_ids = keys.credential_ids_bytes().map_err(|e| e.to_string())?;

    emit_fido_progress(&app, "Touch a security key already enrolled on this silo.");
    let vault_id_str = vault_id.to_string();
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id_str, None)
    })
    .await?;

    // Core opens the join with the key that was touched: only that key's
    // envelope can be unwrapped, and it is the one key whose `policy` claim
    // has been proven here. It also refuses a key a sealed revocation marker
    // names, which a device that had not heard of the removal can republish.
    let offer = silentsilo_app::flows::KeyJoinOffer {
        vault_id,
        keys: keys.keys.clone(),
    };
    let join = silentsilo_app::flows::key_join_open(
        &*store,
        &offer,
        &hex::encode(&unlock.credential_id),
        &unlock.wrap_key,
    )
    .await?;

    // From here on local state is created. Everything above could fail
    // without leaving anything behind, and everything below is undone if it
    // fails, so trying again is not refused over a half-made silo.
    let joined = crate::commands::silo::register_joined_silo(&app, vault_id, &name, location)?;
    let mut cleanup = crate::commands::silo::JoinCleanup::new(&app, &joined);
    let entry = joined.entry;
    let root = entry.path.clone();
    let _opening = crate::state::opening(&app, &root);

    let device_secret = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };
    save_credentials(&LocalVaultAuth {
        vault_id,
        device_secret: device_secret.clone(),
    })
    .map_err(|e| e.to_string())?;
    cleanup.wrote_credentials();
    // Kept on this machine, so this device syncs to the bucket it just joined
    // without the user entering the details a second time.
    silentsilo_vault::save_s3_config(vault_id, &s3_config).map_err(|e| e.to_string())?;
    cleanup.wrote_storage();

    let session = provision_joined_silo(&*store, &join, root.clone(), &device_secret).await?;

    let dek_for_fetch = session.dek.clone();
    crate::commands::silo::adopt_joined_silo(&app, &state, entry)?;

    // The tree is rebuilt rather than copied. Two ways in, decided by the
    // bucket: an uncompacted silo replays from the beginning, a compacted
    // one starts from the snapshot, because replaying a partial log would
    // hand the user a partial vault with no sign anything was missing.
    let mut report = |done: usize, total: usize| {
        let _ = app.emit(
            "join-progress",
            JoinProgress {
                fetched: done,
                total,
            },
        );
    };
    let plan = sync::fetch_join_plan_reporting(&*store, &dek_for_fetch, &mut report)
        .await
        .map_err(|e| e.to_string())?;

    // Replayed while the session is still owned here, before it enters the
    // shared map: it used to be inserted first and locked mutably for the
    // whole replay, which parked every other command behind a join and let
    // the background sync see a half-built silo. Blocking pool, because a
    // long history is sustained database work.
    let session = crate::commands::fido::run_blocking(move || {
        let mut session = session;
        plan.apply(&mut session.conn).map_err(|e| e.to_string())?;
        session.backup_locally().map_err(|e| e.to_string())?;
        Ok(session)
    })
    .await?;
    // Whole and saved: from here a failure leaves a silo that opens.
    cleanup.done();
    crate::commands::fido::run_blocking(move || {
        let meta = Vfs::new(&session).meta().map_err(|e| e.to_string())?;
        crate::state::open_focused_session(&app, session)?;
        Ok(meta)
    })
    .await
}

/// How often the loop wakes up to look. The look is one SQLite count and
/// touches no network: waking often and acting rarely.
const AUTO_SYNC_TICK: std::time::Duration = std::time::Duration::from_secs(10);

/// How long an idle silo goes between pulls. Receiving has to poll:
/// without a server, nothing local knows another device has written until
/// someone asks.
const PULL_INTERVAL_SECS: i64 = 120;

/// Runs sync in the background for as long as the app is open. Errors are
/// quiet: the network being down is the normal state of a laptop. Remote
/// changes landing are emitted as events.
pub fn spawn_auto_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // When each silo last reached its storage, so a silo with nothing to
        // say still pulls on a schedule. In memory: after a restart the first
        // tick pulls, which is what someone opening the app wants anyway.
        let mut last_pull: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
        // Silos whose last pass failed, or stopped before any target could
        // record an outcome (rebuild, rejoin, replaced key), and when to try
        // again. Their targets' own backoff never moved, so without this a
        // silo with changes waiting ran a full pass every tick.
        let mut held_until: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();

        loop {
            tokio::time::sleep(AUTO_SYNC_TICK).await;

            // A locked silo has no key to decrypt anything with, and one
            // with no storage configured has nowhere to sync to.
            let Ok(app_data) = crate::state::app_data_dir(&app) else {
                continue;
            };
            let registry = silentsilo_vault::load_registry(&app_data);
            let open: Vec<SiloEntry> = app
                .state::<AppState>()
                .open_silo_ids()
                .into_iter()
                .filter_map(|id| registry.get(id).cloned())
                .collect();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            for silo in open {
                // Two local reads, no network. A silo with nothing waiting
                // that pulled recently costs this loop a count and a
                // subtraction.
                let waiting = pending_locally(&app, silo.id);
                if !pass_due(
                    waiting,
                    now,
                    last_pull.get(&silo.id).copied(),
                    held_until.get(&silo.id).copied(),
                ) {
                    continue;
                }
                held_until.remove(&silo.id);

                match run_sync_pass(&app, &silo).await {
                    // Stood down for a pass already running. Nothing was
                    // reached, so recording a pull here would push the next
                    // one two minutes out on the strength of work this tick
                    // did not do.
                    Ok(report) if report.skipped => {}
                    Ok(report) if report.configured => {
                        last_pull.insert(silo.id, now);
                        if report.needs_rebuild
                            || report.needs_rejoin
                            || report.key_material_replaced
                        {
                            held_until.insert(silo.id, now + PULL_INTERVAL_SECS);
                        }
                    }
                    // Not configured means no storage: nothing was attempted
                    // and nothing is owed, so it should not be retried on
                    // every tick either. It still compacts its own log.
                    Ok(_) => {
                        last_pull.insert(silo.id, now);
                        maybe_compact_local(&app, silo.id, now).await;
                    }
                    Err(e) => {
                        // Recorded as an attempt, and held off even with
                        // changes waiting, so a silo whose pass keeps failing
                        // does not retry every ten seconds.
                        last_pull.insert(silo.id, now);
                        held_until.insert(silo.id, now + PULL_INTERVAL_SECS);
                        crate::diagnostics::warn(
                            "sync",
                            format_args!("background pass for {} failed: {e}", silo.name),
                        );
                    }
                }
            }
        }
    });
}

/// Whether the background loop runs a pass for a silo this tick: changes
/// waiting, or a pull due, and not held off after a pass that failed.
fn pass_due(waiting: usize, now: i64, last_pull: Option<i64>, held_until: Option<i64>) -> bool {
    if held_until.is_some_and(|until| now < until) {
        return false;
    }
    waiting > 0 || now - last_pull.unwrap_or(0) >= PULL_INTERVAL_SECS
}

const LOCAL_COMPACT_INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Daily log compaction for a silo with no storage configured. With targets
/// the sync pass owns compaction; without them the log would otherwise grow
/// for ever.
async fn maybe_compact_local(app: &AppHandle, silo_id: Uuid, now: i64) {
    let app = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let Ok(mut sessions) = state.sessions.lock() else {
            return;
        };
        let Some(session) = sessions.get_mut(&silo_id) else {
            return;
        };
        let last: i64 = session
            .conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM vault_meta WHERE key = 'local_compacted_at'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if now - last < LOCAL_COMPACT_INTERVAL_SECS {
            return;
        }

        let policy = silentsilo_vfs::CompactionPolicy::default();
        if let Ok(Some(snapshot)) =
            sync::plan_compaction(&session.conn, session.vault_id, &policy, now)
        {
            let _ = silentsilo_vfs::snapshot::compact_covered(&mut session.conn, &snapshot);
        }
        let _ = session.conn.execute(
            "INSERT INTO vault_meta(key, value) VALUES ('local_compacted_at', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [now.to_string()],
        );
    })
    .await;
}

/// Changes this device has authored and not yet sent, read from its own
/// database. Zero for a silo that is locked or gone, which is also "nothing
/// to do".
fn pending_locally(app: &AppHandle, silo_id: Uuid) -> usize {
    let state = app.state::<AppState>();
    let Ok(sessions) = state.sessions.lock() else {
        return 0;
    };
    sessions
        .get(&silo_id)
        .and_then(|session| pending_count(&session.conn).ok())
        .unwrap_or(0)
}

/// What a check of one target found.
#[derive(Debug, serde::Serialize)]
pub struct VerifyTargetResult {
    pub id: String,
    pub label: String,
    pub records_read: usize,
    pub blobs_checked: usize,
    pub bytes_read: u64,
    /// Content this silo believes it has and the target does not.
    pub missing: usize,
    /// What is there but wrong, with why, ready to show.
    pub damaged: Vec<String>,
    /// Content nothing references. Normal, and counted so a number that keeps
    /// climbing can be noticed.
    pub unreferenced: usize,
    /// Set when the target could not be read at all.
    pub failed: Option<String>,
}

/// Reads every copy of this silo and reports what is wrong with it.
/// `deep` reads every file back and authenticates it, the only way to see
/// bit rot, at the cost of downloading the whole silo; without it the check
/// is a listing. Every target is checked, because "verified" is a claim
/// about a copy, not about a silo.
#[tauri::command]
pub async fn vault_verify(app: AppHandle, deep: bool) -> Result<Vec<VerifyTargetResult>, String> {
    let silo = crate::state::active_silo(&app)?;

    // Read once, up front: the connection cannot be held across the network
    // work below, and asking again per target would let the answer change
    // half way through a report.
    let (dek, kek, expected, keys) = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Unlock the silo before checking it.".to_string())?;
        let vfs = Vfs::new(session);
        // Attachments count: the silo believes it holds them, so a check
        // that skipped them would call a copy sound while they rotted.
        let mut keys = vfs.blob_keys().map_err(|e| e.to_string())?;
        for attachment in vfs.attachment_blobs().map_err(|e| e.to_string())? {
            keys.entry(attachment.blob_id)
                .or_insert(attachment.blob_key);
        }
        (
            session.dek.clone(),
            session.kek.clone(),
            vfs.referenced_blobs_with_attachments()
                .map_err(|e| e.to_string())?,
            keys,
        )
    };

    let depth = if deep {
        sync::VerifyDepth::Content
    } else {
        sync::VerifyDepth::Listing
    };

    // Reset here rather than by a separate command, so a stale press of stop
    // from an earlier run cannot kill the one the user just started.
    {
        let state = app.state::<AppState>();
        state
            .verify_cancelled
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    let mut out = Vec::new();
    for configured in silentsilo_vault::load_targets(silo.id) {
        // A copy whose settings will not open is reported as not reached.
        // Left out, the report read as if the silo had one copy fewer.
        let store = match configured.config.open() {
            Ok(store) => store,
            Err(e) => {
                let label = if configured.label.is_empty() {
                    "A backup storage".to_string()
                } else {
                    configured.label.clone()
                };
                out.push(not_checked(
                    configured.config.target_id().to_string(),
                    label,
                    format!("Could not be reached: {e}"),
                ));
                continue;
            }
        };
        let target_label = if configured.label.is_empty() {
            store.describe()
        } else {
            configured.label.clone()
        };
        let id = store.describe();
        let mut open = |blob_id: uuid::Uuid| {
            keys.get(&blob_id)
                .and_then(|wrapped| silentsilo_crypto::unwrap_content_key(wrapped, &kek).ok())
        };
        let handle = app.clone();
        let label = target_label.clone();
        let state = app.state::<AppState>();
        let result = sync::verify_against(
            &*store,
            &dek,
            &expected,
            depth,
            &mut open,
            &mut move |done, total| {
                let _ = handle.emit("verify-progress", (label.clone(), done, total));
            },
            &|| {
                state
                    .verify_cancelled
                    .load(std::sync::atomic::Ordering::Relaxed)
            },
        )
        .await;

        // Stopping stops the whole report, not just this target: a report
        // with later targets silently absent would read as them being fine.
        if matches!(result, Err(sync::SyncError::Cancelled)) {
            return Err("cancelled".into());
        }

        out.push(match result {
            Ok(report) => VerifyTargetResult {
                id,
                label: target_label,
                records_read: report.records_read,
                blobs_checked: report.blobs_checked,
                bytes_read: report.bytes_read,
                missing: report.missing.len(),
                damaged: report
                    .damaged
                    .iter()
                    .map(|(key, why)| format!("{key}: {why}"))
                    .collect(),
                unreferenced: report.unreferenced,
                failed: None,
            },
            // A target that cannot be reached is reported as unchecked rather
            // than as sound. "Could not look" and "looked and found nothing"
            // are the two answers it would be worst to confuse.
            Err(e) => not_checked(id, target_label, e.to_string()),
        });
    }

    if out.is_empty() {
        return Err("This silo has no backup storage to check.".into());
    }
    Ok(out)
}

/// A copy the check could not look at, with why.
fn not_checked(id: String, label: String, why: String) -> VerifyTargetResult {
    VerifyTargetResult {
        id,
        label,
        records_read: 0,
        blobs_checked: 0,
        bytes_read: 0,
        missing: 0,
        damaged: Vec::new(),
        unreferenced: 0,
        failed: Some(why),
    }
}

/// Stops a running check between objects.
///
/// Read-only work, so stopping loses nothing but the answer. The flag is
/// reset by `vault_verify` itself at the start of each run.
#[tauri::command(async)]
pub fn cancel_verify(state: State<AppState>) {
    state.verify_cancelled.store(true, Ordering::Relaxed);
}

/// What a trial restore produced.
#[derive(Debug, serde::Serialize)]
pub struct RestoreTest {
    /// True when the rebuilt silo matches this one exactly.
    pub matches: bool,
    /// Records replayed out of storage, so a report that matches on an empty
    /// silo cannot be mistaken for one that matched on a full one.
    pub records: usize,
    /// Folders and files the rebuild produced.
    pub entries: usize,
    /// Where the two disagree, named rather than counted. Empty when they
    /// do not.
    pub differences: Vec<String>,
    /// One file pulled out of the rebuild and opened, named so the report
    /// says what was proved. The tree matching alone would pass a recovery
    /// with a perfect file list and no readable content; opening one file
    /// walks the whole key chain.
    pub checked_file: Option<String>,
    /// Why the file could not be opened, when one was tried and failed.
    pub content_error: Option<String>,
}

/// Rebuilds this silo from its storage and a recovery code, in a scratch
/// directory, and compares the result with what is on screen. Deliberately
/// the recovery path rather than a folder copy: it is the only route that
/// still exists when the computer does not. Nothing local is touched; the
/// rebuild is deleted afterwards and never becomes the open session.
#[tauri::command]
pub async fn vault_test_restore(app: AppHandle, code: String) -> Result<RestoreTest, String> {
    let silo = crate::state::active_silo(&app)?;

    let live = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Unlock the silo before testing a recovery.".to_string())?;
        silentsilo_vfs::digest(&session.conn).map_err(|e| e.to_string())?
    };

    let store = crate::state::silo_store(&app)
        .ok_or_else(|| "This silo has no backup storage to restore from.".to_string())?;

    let manifest = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That backup storage does not hold a silo.".to_string())?;

    let envelope = sync::fetch_recovery_envelope(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "This backup storage has no recovery code yet, so a recovery from paper alone would \
             not work. Create a recovery code and sync once."
                .to_string()
        })?;

    // Argon2id: on the blocking pool, not an async worker.
    let dek = crate::commands::fido::run_blocking(move || {
        silentsilo_vault::unwrap_with_code(&envelope, &code)
            .map_err(|_| "That recovery code does not open this backup storage.".to_string())
    })
    .await?;

    let kek_envelope = sync::fetch_content_kek(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "This backup storage has no key file for the silo, so nothing could be opened."
                .to_string()
        })?;
    let kek = silentsilo_vault::unwrap_kek_bytes(&kek_envelope, &dek).map_err(|e| e.to_string())?;

    // Somewhere that is not a silo and is never registered as one. A trial
    // restore that left an extra entry in the picker would be a test with a
    // side effect, which is a test people stop running.
    let scratch = tempfile::Builder::new()
        .prefix("silentsilo-restore-test")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let root = scratch.path().join("silo");
    // The temporary directory is not the whole of what a session writes.
    // Opening one puts the plaintext working copy under the machine's own
    // scratch base, keyed by the silo's path, and nothing under this
    // directory reaches it. Left alone, every trial restore dropped a fully
    // decrypted `vault.db` — the entire tree of folder and file names — in
    // `work/open`, under a fresh name each run, and none of them were ever
    // cleaned up. Guarded so it happens whichever way this function leaves.
    let _plaintext = ScratchWorkDir(root.clone());
    // Kept by the sweep a lock runs meanwhile, which would take the working
    // copy of a session that is in no map.
    let _opening = crate::state::opening(&app, &root);

    let device_secret = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };

    // The download is the long half of a trial restore, and it used to run
    // with nothing on screen but a disabled button. The counts already
    // existed; they were simply never handed out.
    let snapshot = sync::latest_snapshot(&*store, &dek)
        .await
        .map_err(|e| e.to_string())?;
    let horizon = snapshot.as_ref().map(|s| s.horizon).unwrap_or(0);
    let handle = app.clone();
    let ops =
        sync::fetch_all_ops_above_reporting(&*store, &dek, horizon, &mut move |done, total| {
            let _ = handle.emit("restore-progress", (done, total));
        })
        .await
        .map_err(|e| e.to_string())?;

    // The rebuild takes its own copy: the check afterwards opens a file with
    // the same key, once the blocking work has finished with it.
    let rebuild_kek = kek.clone();
    let restored = crate::commands::fido::run_blocking(move || {
        let session = VaultSession::provision_with_dek(
            root,
            manifest.vault_id,
            &device_secret,
            dek,
            rebuild_kek,
        )
        .map_err(|e| e.to_string())?;
        Vfs::new(&session)
            .ensure_initialized()
            .map_err(|e| e.to_string())?;

        // A compacted silo starts from its snapshot, exactly as a real
        // recovery would: the records below the horizon are gone from
        // storage, and replaying what is left onto an empty tree would
        // rebuild only the tail of the history.
        if let Some(snapshot) = snapshot {
            silentsilo_vfs::snapshot::restore(&session.conn, &snapshot)
                .map_err(|e| e.to_string())?;
        }
        let count = ops.len();
        replay(&session.conn, ops).map_err(|e| e.to_string())?;
        let digest = silentsilo_vfs::digest(&session.conn).map_err(|e| e.to_string())?;

        let sample = session
            .conn
            .query_row(
                "SELECT fo.path || '/' || f.name, f.blob_id, f.blob_key
                   FROM files f JOIN folders fo ON fo.id = f.folder_id
                  WHERE f.deleted_at IS NULL AND f.size_bytes > 0
                        AND f.blob_key IS NOT NULL AND f.blob_key <> ''
                  ORDER BY f.size_bytes
                  LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .ok()
            .and_then(|(name, blob, key)| Uuid::parse_str(&blob).ok().map(|id| (name, id, key)));

        Ok::<(Vec<String>, usize, Option<(String, Uuid, String)>), String>((digest, count, sample))
    })
    .await?;

    let (restored_digest, records, sample) = restored;
    let differences = silentsilo_vfs::digest_difference(&live, &restored_digest);

    // One file, opened for real. The smallest with content, because this runs
    // while someone waits and the point is to prove the chain rather than to
    // move bytes.
    let mut checked_file = None;
    let mut content_error = None;
    if let Some((name, blob_id, wrapped)) = sample {
        checked_file = Some(name);
        content_error = open_one(&*store, &kek, blob_id, &wrapped).await.err();
    }

    Ok(RestoreTest {
        matches: differences.is_empty() && content_error.is_none(),
        records,
        entries: restored_digest
            .iter()
            .filter(|l| l.starts_with("folder ") || l.starts_with("file "))
            .count(),
        differences,
        checked_file,
        content_error,
    })
}

/// Wipes the machine-local scratch space a throwaway session left behind.
///
/// A `VaultSession` keeps its plaintext working copy outside the silo
/// folder, so deleting the folder is not enough: the trial restore's
/// decrypted index would outlive the run, and the tempdir's own name is
/// different every time, so they accumulate. A guard rather than a call at
/// the end, because the interesting exits are the early ones.
struct ScratchWorkDir(std::path::PathBuf);

impl Drop for ScratchWorkDir {
    fn drop(&mut self) {
        silentsilo_vault::wipe_machine_state(&self.0);
    }
}

/// Downloads one file from the backup and authenticates it.
async fn open_one(
    store: &dyn ObjectStore,
    kek: &silentsilo_crypto::ContentKek,
    blob_id: Uuid,
    wrapped: &str,
) -> Result<(), String> {
    let key = silentsilo_crypto::unwrap_content_key(wrapped, kek)
        .map_err(|_| "Its key could not be read.".to_string())?;

    let dir = tempfile::Builder::new()
        .prefix("silentsilo-restore-check")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = dir.path().join("blob.sslo");
    sync::fetch_blob_to_file(store, blob_id, &path)
        .await
        .map_err(|e| e.to_string())?;
    silentsilo_crypto::verify_blob(&path, &key, blob_id).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod pass_rule_tests {
    use super::{PULL_INTERVAL_SECS, pass_due};

    #[test]
    fn a_failed_pass_is_not_repeated_every_tick() {
        let now = 1_000_000;
        // Changes waiting, nothing held: runs.
        assert!(pass_due(3, now, Some(now - 10), None));
        // The same, after a pass that failed a moment ago: waits.
        assert!(!pass_due(3, now, Some(now - 10), Some(now + 110)));
        // Once the hold runs out it tries again.
        assert!(pass_due(3, now + 120, Some(now - 10), Some(now + 110)));
        // Nothing waiting: only the pull interval counts.
        assert!(!pass_due(0, now, Some(now - 10), None));
        assert!(pass_due(0, now, Some(now - PULL_INTERVAL_SECS), None));
    }
}

/// Both doors into a silo, against storage that lies about what its key
/// envelopes may claim.
///
/// The `policy` on a published envelope is not signed by anything. Anyone who
/// can write to the bucket can put `org` on a key nobody is able to prove,
/// and a device that adopted it would refuse a key rotation and a new
/// recovery code for good, asking every time for a ceremony with a key that
/// does not exist. The rule lives in core; these hold this app to it.
#[cfg(test)]
mod join_tests {
    use super::*;
    use silentsilo_app::flows::{DeviceKey, KeyJoinOffer, key_join_open, recovery_join_begin};
    use silentsilo_store::{FolderStore, StoreConfig};
    use silentsilo_vault::{BackupTarget, TargetRole};

    /// The published silo, plus what opens it.
    struct Origin {
        _dirs: Vec<tempfile::TempDir>,
        storage: std::path::PathBuf,
        code: String,
    }

    /// A pass needs somewhere to send to and somewhere to say things.
    struct OneTarget(BackupTarget);

    impl silentsilo_app::Host for OneTarget {
        fn emit(&self, _event: silentsilo_app::AppEvent) {}
        fn warn(&self, _area: &str, _detail: &str) {}
        fn targets(&self, _silo_id: Uuid) -> Vec<BackupTarget> {
            vec![self.0.clone()]
        }
    }

    /// A silo with one key and one recovery code, synced to a folder.
    async fn origin() -> Origin {
        let storage = tempfile::tempdir().unwrap();
        let silo_dir = tempfile::tempdir().unwrap();
        let root = silo_dir.path().join("silo");
        let vault_id = Uuid::new_v4();
        let session = VaultSession::provision(root.clone(), vault_id, "secret-a").unwrap();
        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().unwrap();
        vfs.create_folder(vfs.root_folder_id().unwrap(), "Invoices")
            .unwrap();

        let (code, envelope) =
            silentsilo_vault::create_recovery_envelope(&session.dek, &session.kek).unwrap();
        silentsilo_vault::save_recovery_envelope(&root, &envelope).unwrap();
        // A FIDO2-shaped key with a known wrap key, so the join needs no
        // hardware and no ceremony.
        silentsilo_app::flows::enrol_device_key(
            &session,
            &DeviceKey {
                kind: silentsilo_vault::KIND_FIDO2.into(),
                derivation: silentsilo_vault::DERIVATION_HMAC_V1.into(),
                credential_id: "aa11".into(),
                public_key: String::new(),
                wrap_key: [7; 32],
                label: "YubiKey".into(),
            },
        )
        .unwrap();

        let host = OneTarget(BackupTarget {
            config: StoreConfig::Folder {
                path: storage.path().to_path_buf(),
            },
            label: String::new(),
            role: TargetRole::Working,
        });
        let state = silentsilo_app::AppState::default();
        let silo = SiloEntry {
            id: vault_id,
            name: "A".into(),
            path: root,
            last_opened: 0,
            auto_lock_minutes: None,
        };
        state.open_session(&host, silo.id, session).unwrap();
        let report = silentsilo_app::run_sync_pass(&state, &host, &silo)
            .await
            .unwrap();
        assert!(report.ops_pushed > 0, "{report:?}");

        Origin {
            storage: storage.path().to_path_buf(),
            code,
            _dirs: vec![storage, silo_dir],
        }
    }

    /// Writes `org` into a published envelope, or plants a whole envelope
    /// for a key that never existed. Both are one PUT for anyone who can
    /// write to the storage.
    async fn plant_org_policy(store: &FolderStore, id: &str) {
        let object = format!("keys/{id}.env");
        let mut envelope: serde_json::Value = match store.get(&object).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
            Err(_) => {
                let mut fake = serde_json::from_slice::<serde_json::Value>(
                    &store.get("keys/aa11.env").await.unwrap(),
                )
                .unwrap();
                fake["credential_id"] = id.into();
                fake["wrapped_dek"] = "00".repeat(60).into();
                fake
            }
        };
        envelope["policy"] = silentsilo_vault::POLICY_ORG.into();
        store
            .put(&object, serde_json::to_vec(&envelope).unwrap())
            .await
            .unwrap();
    }

    /// No key is touched on this path, so nothing has proved anything: every
    /// claim in storage goes.
    #[tokio::test]
    async fn a_recovery_join_keeps_no_policy_storage_planted() {
        let origin = origin().await;
        let store = FolderStore::new(origin.storage.clone());
        plant_org_policy(&store, "aa11").await;
        plant_org_policy(&store, "cc33").await;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("joined");
        let join = recovery_join_begin(&store, &origin.code).await.unwrap();
        let session = provision_joined_silo(&store, &join, root.clone(), "secret-b")
            .await
            .unwrap();
        drop(session);

        let keys = silentsilo_vault::load_fido_keys(&root).unwrap();
        assert!(
            keys.keys.iter().any(|k| k.credential_id == "aa11"),
            "the enrolled key still has to arrive: {:?}",
            keys.keys
        );
        assert!(
            !keys.is_org_controlled(),
            "a planted policy survived a recovery join: {:?}",
            keys.keys
        );
    }

    /// One key was touched here and produced its wrap key, so its own claim
    /// stands. Nothing else in the bucket gets to claim anything.
    #[tokio::test]
    async fn a_key_join_keeps_only_the_policy_of_the_key_that_proved_it() {
        let origin = origin().await;
        let store = FolderStore::new(origin.storage.clone());
        plant_org_policy(&store, "aa11").await;
        plant_org_policy(&store, "cc33").await;

        let offer = KeyJoinOffer {
            vault_id: Uuid::new_v4(),
            keys: sync::fetch_key_envelopes(&store).await.unwrap(),
        };
        let join = key_join_open(&store, &offer, "aa11", &[7; 32])
            .await
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("joined");
        let session = provision_joined_silo(&store, &join, root.clone(), "secret-b")
            .await
            .unwrap();
        drop(session);

        let keys = silentsilo_vault::load_fido_keys(&root).unwrap();
        let managed: Vec<String> = keys.managed().map(|k| k.credential_id.clone()).collect();
        assert_eq!(
            managed,
            vec!["aa11".to_string()],
            "only the key that was touched may keep its policy: {:?}",
            keys.keys
        );
    }
}
