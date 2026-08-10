//! Driving a sync pass from the app.
//!
//! Every command here is a no-op when no bucket is configured: sync is
//! optional, and a purely local vault should never see an error about
//! storage it deliberately doesn't use.

use silentsilo_core::VaultMeta;
use silentsilo_store::ObjectStore;
use silentsilo_sync as sync;
use silentsilo_vault::{
    LocalVaultAuth, SiloEntry, StoredFidoKeys, VaultSession, load_fido_keys, save_credentials,
};
use silentsilo_vfs::{
    OpRecord, Vfs, highest_applied_lamport, mark_delivered, pending_count, pending_ops_for,
    record_target_failure, record_target_success, replay, settle_delivery, target_last_success,
    target_retry_in,
};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::fido::{emit_fido_progress, run_fido};
use crate::state::{AppState, vault_dir};

/// What the last sync pass did, for the status bar.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SyncReport {
    pub configured: bool,
    pub ops_pushed: usize,
    pub ops_fetched: usize,
    pub ops_applied: usize,
    pub blobs_uploaded: usize,
    /// Blobs pulled down because this device keeps a full copy. Reported so
    /// the status line can say why a quiet pass moved gigabytes.
    pub blobs_fetched: usize,
    pub blobs_failed: usize,
    /// Entries another device's changes forced a rename on, so the UI can
    /// tell the user rather than letting a file quietly change name.
    pub renamed: Vec<String>,
    /// This device fell behind a compaction and has to be rebuilt from the
    /// current state before it can sync again. A field rather than an
    /// error: the silo is healthy, this copy is stale.
    pub needs_rebuild: bool,
    /// Records dropped from the bucket by a compaction this pass ran, so the
    /// status line can say the log got shorter rather than leaving the user
    /// wondering what the extra work was.
    pub compacted: usize,
    /// One entry per configured target, so a second copy that is falling
    /// behind can be seen rather than averaged away.
    #[serde(default)]
    pub targets: Vec<TargetStatus>,
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

/// The open silo's backup storage, whichever kind it is.
fn client_for(app: &AppHandle) -> Result<Option<Box<dyn ObjectStore>>, String> {
    Ok(crate::state::silo_store(app))
}

/// Cheap enough to poll: reads local state only, never touches the network.
#[tauri::command]
pub fn sync_status(app: AppHandle, state: State<AppState>) -> Result<SyncStatus, String> {
    let configured = crate::state::silo_store_config(&app).is_some();
    let guard = state.focused_session()?;
    let pending = match guard.as_ref() {
        Some(session) => pending_count(&session.conn).map_err(|e| e.to_string())?,
        None => 0,
    };
    // Read from the saved list rather than by opening anything: this is
    // polled, and a role is a setting, not a question for the network.
    let archive_targets = crate::state::active_silo(&app)
        .map(|silo| {
            silentsilo_vault::load_targets(silo.id)
                .iter()
                .filter(|t| !t.role.allows_delete())
                .count()
        })
        .unwrap_or(0);
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
    run_sync_pass(&app, &silo).await
}

/// Guards a pass so only one runs at a time.
///
/// Released on drop, so an error or an early return can't leave sync wedged
/// off for the rest of the session.
struct SyncGuard(AppHandle);

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
/// and the background pass has to reach all of them.
/// How one target fared this pass, so "the second one is behind" can be said
/// rather than hidden behind a single green tick.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetStatus {
    pub id: String,
    /// What the user called it, or what the store says about itself.
    pub label: String,
    pub ops_pushed: usize,
    pub blobs_uploaded: usize,
    /// Why this target got nothing this pass. `None` means it kept up.
    pub failed: Option<String>,
    /// Unix seconds of the last pass this target accepted everything, or 0
    /// if it never has. The screen turns this into "last written 47 days
    /// ago", which is a fact someone can act on, unlike a red dot that has
    /// been red since a disk went in a drawer.
    pub last_success: i64,
    /// Seconds until this target is worth trying again, 0 when it is due
    /// now. Non-zero means this pass deliberately left it alone.
    pub retry_in: i64,
    /// True when the pass skipped it rather than tried and failed.
    pub waiting: bool,
    /// Records this target is still owed. Counted per target, because "12
    /// changes not backed up" is only true of the target that is missing
    /// them.
    pub ops_behind: usize,
}

/// A target opened and ready to talk to, with what it is called.
struct OpenTarget {
    id: uuid::Uuid,
    label: String,
    store: Box<dyn ObjectStore>,
    /// What this target in particular has not received yet. Read per target
    /// rather than shared, so an unplugged disk's backlog is not offered to
    /// a bucket that already took it.
    owed: Vec<OpRecord>,
    /// Whether the app may send this target a delete at all. Checked before
    /// every deletion rather than relying on the storage to refuse: a target
    /// meant to be append-only should not depend on a bucket policy being
    /// configured correctly for that to hold.
    role: silentsilo_vault::TargetRole,
}

pub(crate) async fn run_sync_pass(app: &AppHandle, silo: &SiloEntry) -> Result<SyncReport, String> {
    let configured = silentsilo_vault::load_targets(silo.id);
    if configured.is_empty() {
        return Ok(SyncReport::default());
    }

    // Two passes at once would each read the same pending queue and send it
    // twice. Skipping is right rather than queueing: the pass already
    // running will pick up whatever this one would have sent.
    let Some(_guard) = SyncGuard::acquire(app) else {
        return Ok(SyncReport {
            configured: true,
            ..SyncReport::default()
        });
    };
    let app = app.clone();
    let app = &app;
    let root = silo.path.clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let every_target: Vec<Uuid> = configured.iter().map(|t| t.config.target_id()).collect();

    // Everything the network phase needs is read up front, then the lock is
    // released, so a slow upload never blocks the UI. What each target is
    // owed is read per target: one shared queue would let a disk in a
    // drawer decide what the bucket is offered.
    let (due, dek, kek, vault_id, applied_through, base) = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Unlock the silo before syncing".to_string())?;
        let conn = &session.conn;

        let mut due = Vec::new();
        for id in &every_target {
            due.push((
                *id,
                pending_ops_for(conn, *id).map_err(|e| e.to_string())?,
                target_last_success(conn, *id).map_err(|e| e.to_string())?,
                target_retry_in(conn, *id, now).map_err(|e| e.to_string())?,
            ));
        }
        (
            due,
            session.dek.clone(),
            session.kek.clone(),
            session.vault_id,
            highest_applied_lamport(conn).map_err(|e| e.to_string())?,
            silentsilo_vfs::snapshot::read_base(conn).map_err(|e| e.to_string())?,
        )
    };

    // A target whose settings will not open is reported, not skipped in
    // silence. A target inside its backoff window is left alone and says
    // so.
    let mut targets: Vec<OpenTarget> = Vec::new();
    let mut resting: Vec<TargetStatus> = Vec::new();
    for (target, (id, owed, last_success, retry_in)) in configured.iter().zip(due) {
        let mut status = TargetStatus {
            id: id.to_string(),
            label: target.label.clone(),
            ops_pushed: 0,
            blobs_uploaded: 0,
            failed: None,
            last_success,
            retry_in,
            waiting: retry_in > 0,
            ops_behind: owed.len(),
        };
        if retry_in > 0 {
            resting.push(status);
            continue;
        }
        match target.config.open() {
            Ok(store) => {
                let label = if target.label.is_empty() {
                    store.describe()
                } else {
                    target.label.clone()
                };
                targets.push(OpenTarget {
                    id,
                    label,
                    store,
                    owed,
                    role: target.role,
                });
            }
            Err(e) => {
                status.failed = Some(e.to_string());
                resting.push(status);
            }
        }
    }
    if targets.is_empty() {
        // Nothing was written, so nothing settles. Failures are still
        // recorded: a target whose settings will not open should back off
        // like any other, or a broken config means a pass every ten seconds
        // for as long as it stays broken.
        record_target_outcomes(app, silo, &resting, now)?;
        return Ok(SyncReport {
            configured: true,
            targets: resting,
            ..SyncReport::default()
        });
    }

    // Asked before anything is sent: a device behind the horizon would push
    // records nobody will ever apply, and tell this user their work was
    // saved. The lowest horizon across targets decides it.
    let stores: Vec<&dyn ObjectStore> = targets.iter().map(|t| &*t.store).collect();
    let horizon = sync::lowest_snapshot_horizon(&stores)
        .await
        .map_err(|e| e.to_string())?;
    if horizon > 0 && applied_through <= horizon {
        return Ok(SyncReport {
            configured: true,
            needs_rebuild: true,
            ..SyncReport::default()
        });
    }

    // ── Out, to every target ────────────────────────────────────────
    // Each target is pushed to on its own: `push_ops` asks whether a record
    // is already there, and a wrapper answering for all of them would skip
    // the target that is missing it.
    let mut statuses = resting;
    let mut ops_pushed = 0;
    let mut blobs_uploaded = 0;
    let mut blobs_failed = 0;

    for target in &targets {
        let store = &*target.store;
        let mut failure: Option<String> = None;
        let mut pushed_here = 0;
        let mut uploaded_here = 0;

        // Publish who this prefix belongs to, so another device can recognise
        // the vault before it can decrypt anything in it. Written only when
        // the target does not already say it: this runs on every pass, and
        // the contents change roughly never.
        if let Err(e) = sync::ensure_manifest(store, vault_id).await {
            failure = Some(e.to_string());
        }

        // Beside the manifest because it answers the same question: what a
        // device joining here needs before it can read anything at all.
        if failure.is_none()
            && let Ok(envelope) = silentsilo_vault::wrap_kek_bytes(&kek, &dek)
            && let Err(e) = sync::publish_content_kek(store, &envelope).await
        {
            failure = Some(e.to_string());
        }

        // And the recovery envelope, for the same reason. Publishing it once
        // when the code was generated only reached the storage configured at
        // that moment: a target added later never got it, and nothing said
        // so. The code looked set up and the recovery would not have worked.
        if failure.is_none()
            && let Ok(recovery) = silentsilo_vault::load_recovery_envelope(&root)
            && let Err(e) = sync::ensure_recovery_envelope(store, &recovery).await
        {
            failure = Some(e.to_string());
        }

        // The base snapshot, before any push: a log compacted locally must
        // never start mid-stream in a bucket a device could join from.
        if failure.is_none()
            && let Some(base) = &base
            && let Err(e) = sync::publish_base_if_missing(store, &dek, base).await
        {
            failure = Some(e.to_string());
        }

        // The wrapped DEKs, which are what let a second device unlock at all,
        // plus any revocation still owed to storage from a pass made offline.
        if failure.is_none()
            && let Ok(mut keys) = load_fido_keys(&root)
        {
            match sync::publish_key_envelopes(store, &keys, target.role.allows_delete()).await {
                Ok(report) if !report.revoked.is_empty() => {
                    keys.keys
                        .retain(|k| !report.revoked.contains(&k.credential_id));
                    silentsilo_vault::save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;
                }
                Ok(_) => {}
                Err(e) => failure = Some(e.to_string()),
            }
        }

        if failure.is_none() {
            match sync::push_ops(store, &dek, &target.owed).await {
                Ok(count) => pushed_here = count,
                Err(e) => failure = Some(e.to_string()),
            }
        }

        if failure.is_none() {
            match sync::push_pending_blobs(store, &root).await {
                Ok(outcome) => {
                    uploaded_here = outcome.uploaded;
                    blobs_failed += outcome.failed.len();
                }
                Err(e) => failure = Some(e.to_string()),
            }
        }

        ops_pushed += pushed_here;
        blobs_uploaded += uploaded_here;
        statuses.push(TargetStatus {
            id: target.id.to_string(),
            label: target.label.clone(),
            ops_pushed: pushed_here,
            blobs_uploaded: uploaded_here,
            // Everything it was owed either arrived this pass or was already
            // there; `push_ops` skips what the target already holds.
            ops_behind: if failure.is_some() {
                target.owed.len()
            } else {
                0
            },
            failed: failure,
            last_success: 0,
            retry_in: 0,
            waiting: false,
        });
    }

    // ── In, from every target ───────────────────────────────────────
    //
    // A record may exist on one target and not another, so all of them are
    // read and the results replayed together. Replay is idempotent and order
    // independent, which is what makes reading the same record twice free.
    let mut incoming = Vec::new();
    for target in &targets {
        match sync::fetch_ops_from(&*target.store, &dek, applied_through).await {
            Ok(mut records) => incoming.append(&mut records),
            Err(e) => {
                if let Some(status) = statuses.iter_mut().find(|s| s.id == target.id.to_string())
                    && status.failed.is_none()
                {
                    status.failed = Some(e.to_string());
                }
            }
        }
    }
    let fetched = incoming.len();

    let replayed = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(&silo.id)
            .ok_or_else(|| "Vault locked mid-sync".to_string())?;
        let report = replay(&session.conn, incoming).map_err(|e| e.to_string())?;

        // Written down per target, after the write and never before: a
        // record noted as delivered without having arrived is one this
        // device will never offer again.
        for target in &targets {
            let reached = statuses
                .iter()
                .find(|s| s.id == target.id.to_string())
                .is_some_and(|s| s.failed.is_none());
            if reached {
                mark_delivered(&session.conn, target.id, &target.owed)
                    .map_err(|e| e.to_string())?;
            }
        }
        report
    };

    // `pushed` gates local pruning as well as re-sending, so it may only be
    // set once every configured target holds the record. Compaction dropping
    // something one target never received would leave it permanently short,
    // with no local copy left to offer. A target removed from the list stops
    // being waited for, which is what makes this terminate.
    {
        let state = app.state::<AppState>();
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.get_mut(&silo.id) {
            settle_delivery(&mut session.conn, &every_target).map_err(|e| e.to_string())?;
        }
    }
    record_target_outcomes(app, silo, &statuses, now)?;

    // ── Housekeeping, on the first target that will have it ─────────
    //
    // Content is fetched from wherever answers first: a blob is the same
    // bytes everywhere, so there is nothing to choose between them.
    let primary = &*targets[0].store;
    let pulled = fetch_missing_for_full_copy(app, silo, primary).await;

    // Compaction publishes to each target before pruning it, which
    // `publish_compaction` guarantees for the target it is given. A target
    // that fails keeps its whole log, which is safe: it simply has more
    // history than it needs.
    let compacted = run_compaction(app, silo, &targets, &dek, vault_id).await?;

    // The sweep exists to delete, so an append-only target is skipped
    // outright rather than swept and refused. Content that nothing
    // references staying there for ever is what that role means, and the
    // Copies panel says so instead of the sweep pretending to run.
    for target in targets.iter().filter(|t| t.role.allows_delete()) {
        run_blob_sweep(app, silo, &*target.store, target.id).await?;
    }

    let report = SyncReport {
        configured: true,
        ops_pushed,
        ops_fetched: fetched,
        ops_applied: replayed.applied,
        blobs_uploaded,
        blobs_fetched: pulled,
        blobs_failed,
        renamed: replayed
            .renamed
            .iter()
            .map(|(from, to)| format!("{from} → {to}"))
            .collect(),
        needs_rebuild: false,
        compacted,
        targets: statuses,
    };

    // The file list is stale the moment remote changes land.
    if report.ops_applied > 0 {
        let _ = app.emit("vault-changed", ());
    }

    // Emitted here rather than by the caller, so a screen showing what each
    // copy is doing hears about every pass and not only the ones a person
    // started. Anything that reports per-target state goes stale the instant
    // a background pass finishes, and the only clue was that leaving the
    // page and coming back fixed it.
    let _ = app.emit("sync-report", &report);
    Ok(report)
}

/// Writes down how each target the pass actually tried came out. Success
/// is a timestamp, failure lengthens the backoff. Targets the pass
/// deliberately skipped are left alone: counting a skip as a failure would
/// leave a disk plugged back in unwritten for hours.
fn record_target_outcomes(
    app: &AppHandle,
    silo: &SiloEntry,
    statuses: &[TargetStatus],
    now: i64,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let Some(session) = sessions.get(&silo.id) else {
        return Ok(());
    };
    for status in statuses {
        if status.waiting {
            continue;
        }
        let Ok(id) = Uuid::parse_str(&status.id) else {
            continue;
        };
        if status.failed.is_some() {
            record_target_failure(&session.conn, id, now).map_err(|e| e.to_string())?;
        } else {
            record_target_success(&session.conn, id, now).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The silo's content KEK, or a refusal naming what is missing. A joining
/// device must never mint one: a split KEK is a split no later sync
/// repairs.
async fn fetch_kek_or_refuse(
    store: &dyn ObjectStore,
    dek: &silentsilo_crypto::MasterDek,
) -> Result<silentsilo_crypto::ContentKek, String> {
    let envelope = sync::fetch_content_kek(store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "This storage has no content key yet. Sync once from the computer that has              the silo, then set this one up."
                .to_string()
        })?;
    silentsilo_vault::unwrap_kek_bytes(&envelope, dek).map_err(|e| e.to_string())
}

/// Downloads whatever this device is missing, when it is meant to hold a
/// full copy. Bounded per pass so a hundred-gigabyte silo does not hold
/// the sync loop for hours; every following pass carries on where this one
/// stopped. Failures are counted rather than raised.
async fn fetch_missing_for_full_copy(
    app: &AppHandle,
    silo: &SiloEntry,
    store: &dyn ObjectStore,
) -> usize {
    if !silentsilo_vault::keep_full_copy(&silo.path) {
        return 0;
    }

    let missing: Vec<Uuid> = {
        let state = app.state::<AppState>();
        let Ok(sessions) = state.sessions.lock() else {
            return 0;
        };
        let Some(session) = sessions.get(&silo.id) else {
            return 0;
        };
        let here: std::collections::HashSet<Uuid> =
            silentsilo_vault::list_local_blob_ids(&silo.path)
                .into_iter()
                .collect();
        match silentsilo_vfs::referenced_blobs(&session.conn) {
            Ok(referenced) => referenced.difference(&here).copied().collect(),
            Err(_) => return 0,
        }
    };

    let mut fetched = 0;
    for blob_id in missing.into_iter().take(FULL_COPY_FETCH_PER_PASS) {
        match sync::fetch_blob(store, &silo.path, blob_id).await {
            Ok(_) => fetched += 1,
            Err(_) => break,
        }
    }
    fetched
}

/// How many blobs one pass will pull when catching a full copy up.
///
/// Enough that a small silo completes in one go, small enough that a large
/// one does not hold the loop for an hour before anything else gets a turn.
const FULL_COPY_FETCH_PER_PASS: usize = 50;

/// Shortens the log if it has grown enough to be worth it. Run only at the
/// end of a pass that just succeeded, which is what entitles this device to
/// declare records deletable. Three phases because the vault connection
/// cannot be held across an await. Failures are swallowed: compaction is
/// housekeeping and the next pass proposes the same work again.
async fn run_compaction(
    app: &AppHandle,
    silo: &SiloEntry,
    targets: &[OpenTarget],
    dek: &silentsilo_crypto::MasterDek,
    vault_id: uuid::Uuid,
) -> Result<usize, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let policy = silentsilo_vfs::CompactionPolicy::default();

    let planned = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let Some(session) = sessions.get(&silo.id) else {
            return Ok(0);
        };
        sync::plan_compaction(&session.conn, vault_id, &policy, now)
    };
    let Ok(Some(snapshot)) = planned else {
        return Ok(0);
    };

    // Every target gets the snapshot, and each prunes only after its own
    // copy of it has landed, which `publish_compaction` guarantees for the
    // target it is handed. A target that fails keeps its whole log: more
    // history than it needs, which is the harmless direction to fail in.
    let mut published_anywhere = false;
    for target in targets {
        // An append-only target takes the snapshot and keeps its whole log.
        // The snapshot still helps: it is a PUT on a fresh key and it is
        // what a joining device replays from instead of the log's start.
        if sync::publish_compaction(&*target.store, dek, &snapshot, target.role.allows_delete())
            .await
            .is_ok()
        {
            published_anywhere = true;
        }
    }
    if !published_anywhere {
        return Ok(0);
    }

    let dropped = {
        let state = app.state::<AppState>();
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        // Locked or closed while the upload ran. The snapshot is in the
        // bucket and the records it covers are gone from it, which is a
        // complete and consistent state; this device simply keeps a longer
        // log than it needs until the next pass.
        let Some(session) = sessions.get_mut(&silo.id) else {
            return Ok(0);
        };
        match sync::finish_compaction(&mut session.conn, &snapshot) {
            Ok(report) => report.dropped,
            Err(_) => 0,
        }
    };

    Ok(dropped)
}

/// How often a silo's storage is swept for content nothing points at.
///
/// A sweep lists every blob, which is the most expensive request this app
/// makes of a metered bucket. What it reclaims is storage, and storage that
/// has been wasted for a day is no worse than storage wasted for a minute.
const BLOB_SWEEP_INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Deletes superseded content, at most once a day, and only what was
/// already unreferenced last time. Runs after a successful pass, when the
/// referenced set is trustworthy. Errors are swallowed: housekeeping.
async fn run_blob_sweep(
    app: &AppHandle,
    silo: &SiloEntry,
    store: &dyn ObjectStore,
    target: Uuid,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let plan = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let Some(session) = sessions.get(&silo.id) else {
            return Ok(());
        };
        // Every read here gives up rather than failing the pass: a missing
        // bookkeeping table once turned this `?` into a sync that failed a
        // day after first open.
        if !matches!(
            silentsilo_vfs::snapshot::sweep_due(
                &session.conn,
                target,
                now,
                BLOB_SWEEP_INTERVAL_SECS
            ),
            Ok(true)
        ) {
            return Ok(());
        }
        let (Ok(referenced), Ok(candidates)) = (
            silentsilo_vfs::referenced_blobs(&session.conn),
            silentsilo_vfs::snapshot::gc_candidates(&session.conn, target),
        ) else {
            return Ok(());
        };
        (referenced, candidates)
    };

    let Ok(outcome) = sync::sweep_orphan_blobs(store, &plan.0, &plan.1).await else {
        return Ok(());
    };

    let state = app.state::<AppState>();
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let Some(session) = sessions.get_mut(&silo.id) else {
        // Locked while the listing ran. The candidate set is not written, so
        // the next sweep starts these blobs over at first sighting, which is
        // the safe direction.
        return Ok(());
    };
    let seen = outcome.candidates.into_iter().collect();
    let _ = silentsilo_vfs::snapshot::set_gc_candidates(&mut session.conn, target, &seen);
    let _ = silentsilo_vfs::snapshot::record_sweep(&session.conn, target, now);
    Ok(())
}

/// Rebuilds this device from the silo's current state.
///
/// The answer to `needs_rebuild`. Everything this device holds is replaced by
/// the newest snapshot plus the records above it, and it comes back with a
/// new identity, because the chain it was keeping died with the log.
#[tauri::command]
pub async fn vault_rebuild_from_snapshot(app: AppHandle) -> Result<usize, String> {
    let Some(store) = client_for(&app)? else {
        return Err("This silo has no backup storage configured.".into());
    };
    let silo = crate::state::active_silo(&app)?;

    let dek = {
        let state = app.state::<AppState>();
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(&silo.id)
            .ok_or("Unlock the silo first.")?
            .dek
            .clone()
    };

    let (snapshot, incoming) = sync::fetch_rebuild(&*store, &dek)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("This silo has no snapshot to rebuild from.")?;

    // The one long operation that has to hold the sessions lock: a
    // half-rebuilt tree must not answer queries. Blocking pool regardless,
    // so the async runtime keeps serving everything else.
    let applied = crate::commands::fido::run_blocking(move || {
        let state = app.state::<AppState>();
        let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(&silo.id)
            .ok_or("Vault locked mid-rebuild.")?;
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
pub async fn sync_fetch_blob(app: AppHandle, blob_id: String) -> Result<(), String> {
    let blob_id = uuid::Uuid::parse_str(&blob_id).map_err(|e| e.to_string())?;
    let Some(store) = client_for(&app)? else {
        return Err("That file isn't on this device, and no backup storage is connected.".into());
    };
    sync::fetch_blob(&*store, &vault_dir(&app)?, blob_id)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
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
    app: AppHandle,
    config: crate::commands::storage::StoreConfigInput,
) -> Result<JoinPreview, String> {
    // The bucket details are passed in rather than read from a silo,
    // because at this point there is no silo — that is the whole situation.
    let store = config
        .resolved(&app)?
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

    let s3_config = config.resolved(&app)?.into_config(None)?;
    let store = s3_config.open().map_err(|e| e.to_string())?;

    let manifest = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "That bucket doesn't hold a silo yet. Create one first, and sync it once.".to_string()
        })?;
    let vault_id = manifest.vault_id;

    let envelopes = sync::fetch_key_envelopes(&*store)
        .await
        .map_err(|e| e.to_string())?;
    if envelopes.is_empty() {
        return Err(
            "No security keys have been published to this bucket yet. Sync once from the computer that has the silo, then try again."
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
            "This silo's keys are all of a kind this computer cannot use. Enroll a security key on it from a device that can already open the silo, then join with that."
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

    // Only the envelope belonging to the key that was actually touched can
    // be unwrapped — falling back to another one would just fail later with
    // a worse message.
    let stored = keys
        .find_by_credential_id(&unlock.credential_id)
        .ok_or_else(|| "That security key isn't enrolled on this silo.".to_string())?
        .clone();
    let dek = silentsilo_vault::unwrap_dek_hex(&stored.wrapped_dek, &unlock.wrap_key)
        .map_err(|_| "That security key could not unlock the silo.".to_string())?;

    // From here on local state is created. Everything above could fail
    // without leaving anything behind.
    let entry = crate::commands::silo::register_joined_silo(&app, vault_id, &name, location)?;
    let root = entry.path.clone();

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
    // Kept on this machine, so this device syncs to the bucket it just joined
    // without the user entering the details a second time.
    silentsilo_vault::save_s3_config(vault_id, &s3_config).map_err(|e| e.to_string())?;

    // The silo's own content key wrapper, not one this device invents:
    // everything already there is wrapped under it.
    let kek = fetch_kek_or_refuse(&*store, &dek).await?;
    let session =
        VaultSession::provision_with_dek(root.clone(), vault_id, &device_secret, dek, kek)
            .map_err(|e| e.to_string())?;

    Vfs::new(&session)
        .ensure_initialized()
        .map_err(|e| e.to_string())?;

    // The DEK envelope on disk is the FIDO-wrapped one, matching a device
    // that enrolled its key here — so unlocking afterwards takes the normal
    // path with no special case for joined devices.
    std::fs::write(
        silentsilo_vault::dek_path(&root),
        hex::decode(&stored.wrapped_dek).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    silentsilo_vault::save_fido_keys(&root, &keys).map_err(|e| e.to_string())?;

    let dek_for_fetch = session.dek.clone();
    session.backup_locally().map_err(|e| e.to_string())?;
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
    let rebuild = sync::fetch_rebuild_reporting(&*store, &dek_for_fetch, &mut report)
        .await
        .map_err(|e| e.to_string())?;
    let from_scratch = match &rebuild {
        Some(_) => Vec::new(),
        None => sync::fetch_ops_reporting(&*store, &dek_for_fetch, 0, &mut report)
            .await
            .map_err(|e| e.to_string())?,
    };

    // Replayed while the session is still owned here, before it enters the
    // shared map: it used to be inserted first and locked mutably for the
    // whole replay, which parked every other command behind a join and let
    // the background sync see a half-built silo. Blocking pool, because a
    // long history is sustained database work.
    crate::commands::fido::run_blocking(move || {
        let mut session = session;
        match rebuild {
            Some((snapshot, incoming)) => {
                sync::apply_rebuild(&mut session.conn, &snapshot, incoming)
                    .map_err(|e| e.to_string())?;
            }
            None => {
                silentsilo_vfs::replay(&session.conn, from_scratch).map_err(|e| e.to_string())?;
            }
        }
        session.backup_locally().map_err(|e| e.to_string())?;
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
                let due = now - last_pull.get(&silo.id).copied().unwrap_or(0) >= PULL_INTERVAL_SECS;
                if waiting == 0 && !due {
                    continue;
                }

                match run_sync_pass(&app, &silo).await {
                    Ok(report) if report.configured => {
                        last_pull.insert(silo.id, now);
                    }
                    // Not configured means no storage: nothing was attempted
                    // and nothing is owed, so it should not be retried on
                    // every tick either. It still compacts its own log.
                    Ok(_) => {
                        last_pull.insert(silo.id, now);
                        maybe_compact_local(&app, silo.id, now).await;
                    }
                    Err(e) => {
                        // Recorded as an attempt so a silo that cannot reach
                        // its bucket does not retry ten times a minute for as
                        // long as the network is down.
                        last_pull.insert(silo.id, now);
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
            .ok_or_else(|| "Unlock the silo before checking it".to_string())?;
        let vfs = Vfs::new(session);
        (
            session.dek.clone(),
            session.kek.clone(),
            silentsilo_vfs::referenced_blobs(&session.conn).map_err(|e| e.to_string())?,
            vfs.blob_keys().map_err(|e| e.to_string())?,
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
    for target in crate::state::silo_targets(&app) {
        let id = target.store.describe();
        let mut open = |blob_id: uuid::Uuid| {
            keys.get(&blob_id)
                .and_then(|wrapped| silentsilo_crypto::unwrap_content_key(wrapped, &kek).ok())
        };
        let handle = app.clone();
        let label = target.label.clone();
        let state = app.state::<AppState>();
        let result = sync::verify_against(
            &*target.store,
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
                label: target.label,
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
            Err(e) => VerifyTargetResult {
                id,
                label: target.label,
                records_read: 0,
                blobs_checked: 0,
                bytes_read: 0,
                missing: 0,
                damaged: Vec::new(),
                unreferenced: 0,
                failed: Some(e.to_string()),
            },
        });
    }

    if out.is_empty() {
        return Err("This silo has no backup storage to check.".into());
    }
    Ok(out)
}

/// Stops a running check between objects.
///
/// Read-only work, so stopping loses nothing but the answer. The flag is
/// reset by `vault_verify` itself at the start of each run.
#[tauri::command]
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
            .ok_or_else(|| "Unlock the silo before testing a restore".to_string())?;
        silentsilo_vfs::digest(&session.conn).map_err(|e| e.to_string())?
    };

    let store = crate::state::silo_store(&app)
        .ok_or_else(|| "This silo has no backup storage to restore from.".to_string())?;

    let manifest = sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That storage does not hold a silo.".to_string())?;

    let envelope = sync::fetch_recovery_envelope(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "No recovery code is published to this storage, so a restore from paper alone \
             would not work. Turn the recovery code on and sync once."
                .to_string()
        })?;

    let dek = silentsilo_vault::unwrap_with_code(&envelope, &code)
        .map_err(|_| "That recovery code does not open this storage.".to_string())?;

    let kek_envelope = sync::fetch_content_kek(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "This storage has no content key, so nothing could be opened.".to_string()
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

    let device_secret = {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    };

    // The download is the long half of a trial restore, and it used to run
    // with nothing on screen but a disabled button. The counts already
    // existed; they were simply never handed out.
    let handle = app.clone();
    let ops = sync::fetch_ops_reporting(&*store, &dek, 0, &mut move |done, total| {
        let _ = handle.emit("restore-progress", (done, total));
    })
    .await
    .map_err(|e| e.to_string())?;
    let snapshot = sync::latest_snapshot(&*store, &dek)
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

/// Downloads one file from the backup and authenticates it.
async fn open_one(
    store: &dyn ObjectStore,
    kek: &silentsilo_crypto::ContentKek,
    blob_id: Uuid,
    wrapped: &str,
) -> Result<(), String> {
    let key = silentsilo_crypto::unwrap_content_key(wrapped, kek)
        .map_err(|_| "its content key could not be unwrapped".to_string())?;
    let bytes = sync::fetch_blob_bytes(store, blob_id)
        .await
        .map_err(|e| e.to_string())?;

    let dir = tempfile::Builder::new()
        .prefix("silentsilo-restore-check")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let path = dir.path().join("blob.sslo");
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    silentsilo_crypto::verify_blob(&path, &key, blob_id).map_err(|e| e.to_string())?;
    Ok(())
}
