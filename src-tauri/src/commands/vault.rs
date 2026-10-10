use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use silentsilo_core::{
    CoreError, DeviceInfo, FileEntry, FolderEntry, SearchHit, TrashItem, VaultEntry, VaultMeta,
};
use silentsilo_crypto::{decrypt_blob, encrypt_file};
use silentsilo_vault::{
    VaultSession, has_backup_key, is_fido_enrolled, list_local_blob_ids, touch_blob_access,
};
use silentsilo_vfs::Vfs;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::fido::{Prompt, emit_fido_progress, run_blocking, run_fido};
use crate::state::{AppState, vault_dir, with_vfs};

#[derive(serde::Serialize)]
pub struct AppBootstrap {
    provisioned: bool,
    locked: bool,
    fido_available: bool,
    fido_key_present: bool,
    fido_enrolled: bool,
    fido_backup_enrolled: bool,
    /// Whether this machine can enrol its built-in authenticator, so the UI
    /// only offers Windows Hello where it would actually work.
    platform_authenticator: bool,
    /// Whether any enrolled key is a removable one. The unlock screen words
    /// its instruction around what is actually enrolled: telling a Windows
    /// Hello household to insert a key sends them looking for hardware they
    /// do not own.
    portable_enrolled: bool,
    /// Whether any enrolled key is the machine's built-in authenticator.
    platform_enrolled: bool,
    /// The silo currently open, if any. `None` means show the picker.
    silo: Option<crate::commands::silo::SiloView>,
    /// Which platform's words the window uses: "windows", "linux" or
    /// "macos". Without it the window took every build for Windows.
    os: &'static str,
}

/// The platform this build is for, in the window's terms.
fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "windows"
    }
}

/// Asks the authenticators what they can do, which walks the USB bus and can
/// take seconds with a key that is slow to answer. On the blocking pool, not
/// the async workers: a sync pass is often in flight while the picker loads.
#[tauri::command]
pub async fn app_bootstrap(app: AppHandle) -> Result<AppBootstrap, String> {
    run_blocking(move || app_bootstrap_impl(&app)).await
}

fn app_bootstrap_impl(app: &AppHandle) -> Result<AppBootstrap, String> {
    let state = app.state::<AppState>();
    // "Provisioned" is now per-silo: the app can know about several and have
    // none of them open, which is the state the picker exists for.
    let silo = crate::state::active_silo(app).ok();
    let provisioned = silo
        .as_ref()
        .is_some_and(|s| silentsilo_vault::is_provisioned(s.id));
    let locked = state.focused_session()?.is_none();
    let fido_enrolled = silo.as_ref().is_some_and(|s| is_fido_enrolled(&s.path));
    // Which kinds of key are enrolled, not just whether any is. Tombstones
    // count as no key at all, the same as everywhere else, and so does a key
    // whose kind this build cannot unlock with: both drive what the enrolment
    // screens offer, and offering nothing because another machine's Touch ID
    // is on the silo would leave this one with no way in.
    let (portable_enrolled, platform_enrolled) = silo
        .as_ref()
        .and_then(|s| silentsilo_vault::load_fido_keys(&s.path).ok())
        .map(|keys| {
            let mut portable = false;
            let mut platform = false;
            for key in keys.usable() {
                if key.platform {
                    platform = true;
                } else {
                    portable = true;
                }
            }
            (portable, platform)
        })
        .unwrap_or((false, false));
    let fido = silentsilo_fido::status();
    Ok(AppBootstrap {
        provisioned,
        locked,
        fido_available: fido.fido_accessible,
        fido_key_present: fido.key_present,
        fido_enrolled,
        fido_backup_enrolled: fido_enrolled
            && silo.as_ref().is_some_and(|s| has_backup_key(&s.path)),
        platform_authenticator: silentsilo_fido::platform_authenticator_available(),
        portable_enrolled,
        platform_enrolled,
        silo: silo.as_ref().map(crate::commands::silo::SiloView::from),
        os: os_name(),
    })
}

/// The long half of an import: one file sealed into the blob store. Needs
/// only the keys and the root, so it runs without the sessions lock;
/// encrypting a large file while holding it froze the window.
struct EncryptedImport {
    file_name: String,
    blob_id: Uuid,
    /// Plaintext length, which is what the row records; the encrypted length
    /// went to the blob cache bookkeeping already.
    size_bytes: i64,
    hash_hex: String,
    mime: Option<String>,
    blob_key: String,
}

fn encrypt_import(
    root: &Path,
    kek: &silentsilo_crypto::ContentKek,
    source: &Path,
) -> Result<EncryptedImport, String> {
    if !source.is_file() {
        return Err(crate::err::coded_with(
            "err.not_a_file_path",
            format!("Not a file: {}", source.display()),
            &[("path", &source.display())],
        ));
    }

    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            crate::err::coded!("err.file_name_unreadable", "That file name cannot be read.")
                .to_string()
        })?
        .to_string();

    let file_id = Uuid::now_v7();
    let blob_id = Uuid::new_v4();
    let blob_path = silentsilo_vault::VaultPaths::new(root.to_path_buf()).blob_path(blob_id);

    // A key for this blob alone, wrapped under the vault DEK and stored in
    // the record rather than in the file. That is what lets a later key
    // rotation leave every byte of content where it is.
    let content_key = silentsilo_crypto::generate_content_key();
    let blob_key =
        silentsilo_crypto::wrap_content_key(&content_key, kek).map_err(|e| e.to_string())?;
    let result = encrypt_file(source, &blob_path, &content_key, file_id, blob_id)
        .map_err(|e| e.to_string())?;
    let _ = silentsilo_vault::record_blob_present(root, blob_id, result.size_bytes as i64, false);

    Ok(EncryptedImport {
        file_name,
        blob_id,
        // Counted during encryption, so the row matches the sealed bytes
        // even when the source changed while it was being read.
        size_bytes: result.plain_bytes as i64,
        hash_hex: hex::encode(result.header.content_hash),
        mime: silentsilo_vfs::guess_mime(source),
        blob_key,
    })
}

/// The short half: the row naming what was just sealed. Runs under the
/// sessions lock, which it holds for one insert.
fn commit_import(
    vfs: &Vfs,
    folder_id: Uuid,
    encrypted: &EncryptedImport,
) -> silentsilo_core::CoreResult<FileEntry> {
    vfs.add_file(
        folder_id,
        &encrypted.file_name,
        encrypted.blob_id,
        encrypted.size_bytes,
        &encrypted.hash_hex,
        encrypted.mime.as_deref(),
        &encrypted.blob_key,
    )
}

/// Both halves, against a pinned silo: encrypt with no lock held, then a
/// short lock for the row. The pin is what keeps a batch import writing
/// into the silo it started on if the user switches silos part-way.
fn import_one(
    app: &AppHandle,
    snapshot: &crate::state::SessionSnapshot,
    folder_id: Uuid,
    source: &Path,
) -> Result<FileEntry, String> {
    let encrypted = encrypt_import(&snapshot.root, &snapshot.kek, source)?;
    crate::state::with_session_id(&app.state::<AppState>(), snapshot.id, |_session, vfs| {
        commit_import(vfs, folder_id, &encrypted)
    })
}

#[derive(Clone, serde::Serialize)]
struct ImportProgress {
    phase: String,
    current: u32,
    total: u32,
    name: String,
    /// Files and folders that could not be read or imported so far. Final
    /// on the "done" report.
    skipped: u32,
}

/// Where a folder import has got to: files attempted, files found by the
/// scan, and what could not be read or imported.
#[derive(Default)]
struct ImportCounters {
    done: u32,
    total: u32,
    skipped: u32,
    /// Files now in the silo, for the activity log.
    added: u32,
}

fn emit_import_progress(app: &AppHandle, progress: ImportProgress) {
    let _ = app.emit("import-progress", progress);
}

#[tauri::command(async)]
pub fn cancel_import(state: State<AppState>) {
    state.import_cancelled.store(true, Ordering::Relaxed);
}

/// Clears any previous cancellation before starting a new cancellable batch
/// (folder import, paste). Separate from `cancel_import` so nested calls —
/// e.g. `vault_paste_paths` importing a folder as one of its items — don't
/// clobber a cancellation the outer call is still checking for.
#[tauri::command(async)]
pub fn reset_import_cancel(state: State<AppState>) {
    state.import_cancelled.store(false, Ordering::Relaxed);
}

/// Count files under a directory (no symlink follow / cycle-safe).
fn count_importable_files(root: &std::path::Path) -> u32 {
    let mut total = 0u32;
    let mut visited = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(canon) = std::fs::canonicalize(&dir) else {
            continue;
        };
        if !visited.insert(canon) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_file() {
                total = total.saturating_add(1);
            } else if meta.is_dir() {
                stack.push(path);
            }
        }
    }
    total
}

/// Returned by `import_folder_recursive` when cancelled partway through, so
/// callers can tell "stopped on request" apart from a real I/O failure.
const IMPORT_CANCELLED: &str = "cancelled";

/// Returned when the silo closed while an import was running.
///
/// Distinct from a per-file failure on purpose. A file that will not open is
/// skipped and the walk carries on, which is right for a file another program
/// has locked; a silo that has gone away fails every remaining file for the
/// same reason, and carrying on produced a half-imported folder that the app
/// then reported as "Folder imported." Locking the workstation mid-import is
/// an ordinary thing to do, so this was an ordinary way to lose half a
/// folder without being told.
const SILO_CLOSED: &str = crate::err::coded!(
    "err.locked_during_import",
    "The silo was locked before the import finished."
);

fn import_folder_recursive(
    app: &AppHandle,
    snapshot: &crate::state::SessionSnapshot,
    parent_folder_id: Uuid,
    source_dir: &std::path::Path,
    visited: &mut HashSet<std::path::PathBuf>,
    counters: &mut ImportCounters,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    let canonical = std::fs::canonicalize(source_dir).unwrap_or_else(|_| source_dir.to_path_buf());
    if !visited.insert(canonical) {
        return Ok(());
    }

    // A subfolder that will not open is skipped and counted, like a file
    // that will not: it used to stop the whole import half way.
    let entries = match std::fs::read_dir(source_dir) {
        Ok(entries) => entries,
        Err(e) => {
            counters.skipped = counters.skipped.saturating_add(1);
            crate::diagnostics::warn("import", format_args!("skipped a folder: {e}"));
            return Ok(());
        }
    };

    for entry in entries {
        if cancelled.load(Ordering::Relaxed) {
            return Err(IMPORT_CANCELLED.to_string());
        }
        if !crate::state::session_is_open(&app.state::<AppState>(), snapshot.id) {
            return Err(SILO_CLOSED.to_string());
        }
        let Ok(entry) = entry else {
            counters.skipped = counters.skipped.saturating_add(1);
            continue;
        };
        let path = entry.path();

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                counters.skipped = counters.skipped.saturating_add(1);
                continue;
            }
        };
        // Skip symlinks / junctions — following them can recurse forever on Windows.
        if meta.file_type().is_symlink() {
            continue;
        }

        let display_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("…")
            .to_string();

        if meta.is_file() {
            counters.done = counters.done.saturating_add(1);
            emit_import_progress(
                app,
                ImportProgress {
                    phase: "encrypting".into(),
                    current: counters.done,
                    total: counters.total,
                    name: display_name,
                    skipped: counters.skipped,
                },
            );
            // Keep going if a single file fails (locked/system files, etc.).
            match import_one(app, snapshot, parent_folder_id, &path) {
                Ok(_) => counters.added = counters.added.saturating_add(1),
                Err(e) => {
                    counters.skipped = counters.skipped.saturating_add(1);
                    crate::diagnostics::warn("import", format_args!("skipped a file: {e}"));
                }
            }
        } else if meta.is_dir() {
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                counters.skipped = counters.skipped.saturating_add(1);
                continue;
            };
            emit_import_progress(
                app,
                ImportProgress {
                    phase: "folders".into(),
                    current: counters.done,
                    total: counters.total,
                    name: display_name,
                    skipped: counters.skipped,
                },
            );
            // A short lock for the row; the descent itself holds nothing.
            let created =
                crate::state::with_session_id(&app.state::<AppState>(), snapshot.id, |_s, vfs| {
                    vfs.create_or_get_folder(parent_folder_id, dir_name)
                });
            match created {
                Ok(subfolder) => {
                    import_folder_recursive(
                        app,
                        snapshot,
                        subfolder.id,
                        &path,
                        visited,
                        counters,
                        cancelled,
                    )?;
                }
                Err(e) => {
                    counters.skipped = counters.skipped.saturating_add(1);
                    crate::diagnostics::warn("import", format_args!("skipped a folder: {e}"));
                }
            }
        }
    }
    Ok(())
}

/// The synchronous body of a folder import, shared with the paste path.
///
/// Runs on the blocking pool, never on the main thread: the walk and the
/// per-file encryption are the longest work in the app. The sessions lock
/// is taken per row, not for the duration, so the rest of the app keeps
/// answering while a large folder comes in.
///
/// Returns how many files and folders could not be read or imported.
fn import_folder_impl(app: &AppHandle, folder_id: Uuid, source: &Path) -> Result<u32, String> {
    if !source.is_dir() {
        return Err(crate::err::coded_with(
            "err.not_a_folder_path",
            format!("Not a folder: {}", source.display()),
            &[("path", &source.display())],
        ));
    }

    let total_files = count_importable_files(source);
    emit_import_progress(
        app,
        ImportProgress {
            phase: "scanning".into(),
            current: 0,
            total: total_files,
            name: source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("folder")
                .to_string(),
            skipped: 0,
        },
    );

    let state = app.state::<AppState>();
    let snapshot = crate::state::snapshot_focused_session(&state)?;

    let dir_name = source.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        crate::err::coded!(
            "err.folder_name_unreadable",
            "That folder name cannot be read."
        )
        .to_string()
    })?;

    // Merged into a folder already carrying the name, as a drop onto an
    // existing tree means; uploading the same folder twice used to fail
    // outright on the name conflict.
    let top_folder = crate::state::with_session_id(&state, snapshot.id, |_s, vfs| {
        vfs.create_or_get_folder(folder_id, dir_name)
    })?;

    let mut visited = HashSet::new();
    let mut counters = ImportCounters {
        total: total_files,
        ..ImportCounters::default()
    };
    let walked = import_folder_recursive(
        app,
        &snapshot,
        top_folder.id,
        source,
        &mut visited,
        &mut counters,
        &state.import_cancelled,
    );
    // Cancelled or not, what landed is in the silo.
    log_added(app, snapshot.id, top_folder.id, &[], counters.added);
    walked?;

    emit_import_progress(
        app,
        ImportProgress {
            phase: "done".into(),
            current: counters.done,
            total: total_files,
            name: dir_name.to_string(),
            skipped: counters.skipped,
        },
    );
    Ok(counters.skipped)
}

/// Returns how many files and folders were skipped because they could not be
/// read or imported; the "done" progress report carries the same number.
#[tauri::command]
pub async fn vault_import_folder(
    app: AppHandle,
    folder_id: String,
    source_path: String,
) -> Result<u32, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let source = PathBuf::from(&source_path);
    run_blocking(move || import_folder_impl(&app, folder_id, &source)).await
}

/// Imports what the Explorer verbs queued into Inbox.
///
/// Nothing is dropped. What would not import goes back in the queue, so the
/// next unlock offers it again, and the window hears about it through
/// `shell-upload-failed` (a list of "name: why"). A queue that cannot be
/// read into the silo at all goes back whole.
pub(crate) fn process_shell_upload_queue(app: &AppHandle) -> Result<u32, String> {
    let paths = silentsilo_shell::drain_upload_queue().map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Ok(0);
    }

    let state = app.state::<AppState>();
    let target = crate::state::snapshot_focused_session(&state).and_then(|snapshot| {
        crate::state::with_session_id(&state, snapshot.id, |_s, vfs| {
            vfs.inbox_folder().map(|f| f.id)
        })
        .map(|inbox| (snapshot, inbox))
    });
    let (snapshot, inbox_id) = match target {
        Ok(target) => target,
        Err(e) => {
            requeue_uploads(&paths);
            return Err(e);
        }
    };

    let mut imported = 0u32;
    let mut added = Vec::new();
    let mut failed = Vec::new();
    let mut requeue = Vec::new();
    for path in paths {
        let source = PathBuf::from(&path);
        match import_one(app, &snapshot, inbox_id, &source) {
            Ok(file) => {
                imported += 1;
                added.push(file);
            }
            Err(e) => {
                let name = source
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path)
                    .to_string();
                failed.push(format!("{name}: {e}"));
                requeue.push(path);
            }
        }
    }
    if !failed.is_empty() {
        requeue_uploads(&requeue);
        let _ = app.emit("shell-upload-failed", &failed);
    }
    log_added(app, snapshot.id, inbox_id, &added, imported);
    Ok(imported)
}

fn requeue_uploads(paths: &[String]) {
    for path in paths {
        if let Err(e) = silentsilo_shell::queue_upload(path) {
            crate::diagnostics::warn("import", format_args!("a queued file was lost: {e}"));
        }
    }
}

/// The DEK envelope to open the silo with, given the credential that
/// answered the prompt.
///
/// The credential is matched first, because only its own envelope unwraps
/// under the key that ceremony produced. Falling back to the first enrolled
/// key covers the silo whose envelopes were published by another device and
/// carry ids this one has not seen; it fails a moment later with a clearer
/// message than "no matching key" would give here.
fn wrapped_dek_for(
    keys: &silentsilo_vault::StoredFidoKeys,
    credential_id: &[u8],
) -> Result<String, String> {
    keys.find_by_credential_id(credential_id)
        .or_else(|| keys.primary())
        .map(|key| key.wrapped_dek.clone())
        .filter(|wrapped| !wrapped.is_empty())
        .ok_or_else(|| {
            crate::err::coded!(
                "err.key_not_enrolled",
                "That key is not enrolled on this silo."
            )
            .to_string()
        })
}

/// What to tell the user to do, matching what was actually asked of the
/// platform: naming a security key to a Hello-only silo sends its owner
/// looking for hardware they do not have.
fn unlock_prompt(wanted: Option<silentsilo_fido::Authenticator>) -> Prompt {
    let built_in = crate::commands::fido::BUILT_IN;
    match wanted {
        Some(silentsilo_fido::Authenticator::ThisDevice) => Prompt::new(
            "unlock_built_in",
            format!("Confirm with {built_in} to unlock the silo."),
        ),
        Some(silentsilo_fido::Authenticator::SecurityKey) => {
            Prompt::new("unlock_key", "Touch your security key to unlock the silo.")
        }
        None => Prompt::new(
            "unlock_any",
            format!("Touch an enrolled security key, or confirm with {built_in}, to unlock."),
        ),
    }
}

/// What a presence check is for, said in the prompt that asks for it.
pub(crate) enum Presence {
    ShowEntry,
    ExportLogins,
    FillLogin { label: String, site: String },
    FillAny,
    SshSign { key: String },
    AutoType { label: String, program: String },
}

impl Presence {
    fn prompt(&self, built_in: bool) -> Prompt {
        let (code, purpose) = match self {
            Presence::ShowEntry => ("show_entry", "show this entry".to_string()),
            Presence::ExportLogins => ("export", "export your logins".to_string()),
            Presence::FillLogin { label, site } => {
                ("fill", format!("fill your {label} login on {site}"))
            }
            Presence::FillAny => ("fill_any", "fill a login in your browser".to_string()),
            Presence::SshSign { key } => ("ssh", format!("sign with your {key} SSH key")),
            Presence::AutoType { label, program } => (
                "autotype",
                format!("type your {label} login into {program}"),
            ),
        };
        let prompt = if built_in {
            Prompt::new(
                "verify_built_in",
                format!(
                    "Confirm with {} to {purpose}.",
                    crate::commands::fido::BUILT_IN
                ),
            )
        } else {
            Prompt::new(
                "verify_key",
                format!("Touch your security key to {purpose}."),
            )
        };
        let prompt = prompt.with("purpose", code);
        match self {
            Presence::FillLogin { label, site } => prompt.with("label", label).with("site", site),
            Presence::SshSign { key } => prompt.with("key", key),
            Presence::AutoType { label, program } => {
                prompt.with("label", label).with("program", program)
            }
            _ => prompt,
        }
    }
}

#[tauri::command]
pub async fn vault_unlock(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<VaultMeta, String> {
    let creds = crate::state::silo_credentials(&app)?;
    let root = vault_dir(&app)?;

    if !is_fido_enrolled(&root) {
        return Err(
            crate::err::coded!("err.enrol_before_unlock", "Enrol a key before unlocking.").into(),
        );
    }

    let keys = silentsilo_vault::load_fido_keys(&root).map_err(|e| e.to_string())?;
    let cred_ids = keys.credential_ids_bytes().map_err(|e| e.to_string())?;
    let vault_id = creds.vault_id.to_string();

    // Asked for by kind, so a silo whose only key is Windows Hello opens
    // with Hello rather than through the whole passkey menu.
    let wanted = crate::commands::fido::preferred_authenticator(&keys);
    emit_fido_progress(&app, unlock_prompt(wanted));
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id, wanted)
    })
    .await?;

    let wrapped_dek = wrapped_dek_for(&keys, &unlock.credential_id)?;
    // The touch is done. Opening can take a while (after an update the
    // tree is rebuilt from the history), and the touch line must not stay.
    emit_fido_progress(
        &app,
        Prompt::new(
            "opening",
            "Opening the silo. Right after an update a large one takes a minute.",
        ),
    );

    // Opening the session adopts a working copy a crash left behind, or
    // restores one from the encrypted snapshot: real disk work on a large
    // silo, so blocking-pool territory rather than an async worker.
    run_blocking(move || {
        // Kept by the scratch sweep another silo's lock runs meanwhile.
        let _opening = crate::state::opening(&app, &root);
        let session =
            VaultSession::open_with_fido_wrapped(root.clone(), &unlock.wrap_key, &wrapped_dek)
                .map_err(|e| e.to_string())?;

        if session.vault_id != creds.vault_id {
            return Err(crate::err::coded!(
                "err.key_other_silo",
                "This key opens a different silo."
            )
            .into());
        }

        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().map_err(|e| e.to_string())?;
        let meta = vfs.meta().map_err(|e| e.to_string())?;

        // A crash between opening a file and locking would have left plaintext
        // behind; this is the first moment it is safe to clear it.
        wipe_open_scratch(&session.paths.root);

        crate::state::open_focused_session(&app, session)?;
        let label = keys
            .find_by_credential_id(&unlock.credential_id)
            .map(crate::audit::key_name)
            .unwrap_or_default();
        crate::audit::set_unlocked_with(creds.vault_id, Some(label.clone()));
        crate::audit::record(
            &app,
            crate::audit::event(crate::audit::codes::UNLOCKED).with("key", label),
        )?;
        // Any shell-upload paths queued while locked are left in the queue for
        // the frontend to fetch (via shell_upload_queue_pending) and offer a
        // destination-folder picker, instead of silently landing in Inbox.
        Ok(meta)
    })
    .await
}

/// Locks one silo, or every open one when no id is given.
///
/// Both are real actions rather than one being a convenience: the idle timer
/// locks a single silo whose own timeout expired, while leaving the machine
/// or pressing Lock is a statement about all of them.
#[tauri::command]
pub async fn vault_lock(app: AppHandle, id: Option<String>) -> Result<(), String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        let all = id.is_none();
        let ids = match id {
            Some(id) => {
                let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
                // Only a silo that is open has anything to give back.
                state
                    .open_silo_ids()
                    .into_iter()
                    .filter(|open| *open == id)
                    .collect()
            }
            None => state.open_silo_ids(),
        };
        take_back_clipboard(&app, if all { None } else { Some(&ids) });
        // Only ciphertext should remain on disk for each of these once this
        // returns; `close_session` snapshots, drops the connection and then
        // clears the working copy, in that order.
        for id in ids {
            state.close_session(id)?;
        }
        tell_if_scratch_survived(&app, state.sweep_scratch());
        crate::commands::cloud::forget_sign_ins_when_all_locked(&app);
        // Every silo closing is news to the screen, which the caller may not
        // own: the updater locks all of them before installing, and when the
        // install then failed, the window went on showing a silo that no
        // longer answered.
        if all {
            let _ = app.emit("silos-locked", ());
        }
        Ok(())
    })
    .await
}

/// A file opened from a silo is still held by another application, so its
/// decrypted copy could not be deleted. Every later lock and the next start
/// try again; the user is the one who can close it now.
fn tell_if_scratch_survived(app: &AppHandle, left: usize) {
    if left > 0 {
        let _ = app.emit("scratch-still-open", left);
    }
}

/// Locks every open silo, used when the workstation locks or suspends:
/// whoever walks up next should find nothing open. Failures are swallowed;
/// a silo that cannot close right now is closed by its idle timer soon
/// after.
pub fn lock_all_silos(app: &AppHandle) {
    take_back_clipboard(app, None);
    let state = app.state::<AppState>();
    for id in state.open_silo_ids() {
        let _ = state.close_session(id);
    }
    tell_if_scratch_survived(app, state.sweep_scratch());
    crate::commands::cloud::forget_sign_ins_when_all_locked(app);
    let _ = app.emit("silos-locked", ());
}

/// How far past its limit an idle silo is locked from here rather than by
/// the window. The window's own sweep locks first, every 5 seconds; this
/// is for a window that has crashed or hung, which left the silo open until
/// the app quit, and for one throttled while hidden.
const IDLE_BACKSTOP_MARGIN_SECS: u64 = 120;
/// Short enough for the minute's warning below to come close to a minute.
const IDLE_BACKSTOP_TICK: std::time::Duration = std::time::Duration::from_secs(10);
/// How long before a silo locks the system notification comes.
const LOCK_NOTICE_SECS: u64 = 60;

/// A silo's idle limit in seconds: its own if it has one, else the
/// app-wide one. `None` when it never locks by itself.
fn idle_limit_secs(own: Option<u32>, default_minutes: u32) -> Option<u64> {
    let limit = own.filter(|m| *m > 0).unwrap_or(default_minutes);
    (limit > 0).then(|| u64::from(limit) * 60)
}

/// Whether a silo idle for `idle_secs` is past the backstop: its limit plus
/// the margin.
fn past_idle_backstop(idle_secs: u64, own: Option<u32>, default_minutes: u32) -> bool {
    idle_limit_secs(own, default_minutes)
        .is_some_and(|limit| idle_secs >= limit + IDLE_BACKSTOP_MARGIN_SECS)
}

/// Whether a silo idle for `idle_secs` locks within the notice's minute.
fn locks_within_notice(idle_secs: u64, own: Option<u32>, default_minutes: u32) -> bool {
    idle_limit_secs(own, default_minutes)
        .is_some_and(|limit| idle_secs < limit && limit - idle_secs <= LOCK_NOTICE_SECS)
}

/// The notification's words, set by the window in the language in use:
/// a title and a body where `{name}` is the silo's. `None` when the user
/// turned the notice off, and until the window has said.
static LOCK_NOTICE: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
/// Silos already told about, so a minute's countdown is one notification.
static LOCK_NOTICED: std::sync::Mutex<Vec<Uuid>> = std::sync::Mutex::new(Vec::new());

#[tauri::command(async)]
pub fn app_set_lock_notice(title: Option<String>, body: Option<String>) {
    if let Ok(mut notice) = LOCK_NOTICE.lock() {
        *notice = title.zip(body);
    }
}

/// A minute before the silo on screen locks, says so in a system
/// notification, but only when the window is not in front: there the window
/// counts down itself. The window cannot do this part: hidden in the tray,
/// its timers run late.
fn notify_before_lock(app: &AppHandle, registry: &silentsilo_vault::SiloRegistry) {
    let state = app.state::<AppState>();
    let Ok(focused) = crate::state::focused_id(&state) else {
        return;
    };
    let default_minutes = state.auto_lock_default_minutes.load(Ordering::Relaxed);
    let soon = crate::state::idle_seconds(&state)
        .into_iter()
        .any(|(id, idle)| {
            id == focused
                && locks_within_notice(
                    idle,
                    registry.get(id).and_then(|e| e.auto_lock_minutes),
                    default_minutes,
                )
        });
    let Ok(mut noticed) = LOCK_NOTICED.lock() else {
        return;
    };
    if !soon {
        noticed.retain(|id| *id != focused);
        return;
    }
    if noticed.contains(&focused) {
        return;
    }
    noticed.push(focused);
    let in_front = app.get_webview_window("main").is_some_and(|w| {
        w.is_visible().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
            && w.is_focused().unwrap_or(false)
    });
    let notice = LOCK_NOTICE.lock().ok().and_then(|n| n.clone());
    if let (false, Some((title, body))) = (in_front, notice) {
        let name = registry
            .get(focused)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let _ = crate::commands::silo::notify(app, &title, &body.replace("{name}", &name));
    }
}

/// Locks idle silos from Rust, for as long as the app runs. On its own task
/// rather than the sync loop's: a pass uploading for an hour holds that one.
pub fn spawn_idle_backstop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(IDLE_BACKSTOP_TICK).await;
            let handle = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || lock_idle_silos(&handle)).await;
        }
    });
}

fn lock_idle_silos(app: &AppHandle) {
    let Ok(app_data) = crate::state::app_data_dir(app) else {
        return;
    };
    let registry = silentsilo_vault::load_registry(&app_data);
    notify_before_lock(app, &registry);
    let state = app.state::<AppState>();
    let default_minutes = state.auto_lock_default_minutes.load(Ordering::Relaxed);
    let due: Vec<Uuid> = crate::state::idle_seconds(&state)
        .into_iter()
        .filter(|(id, idle)| {
            let own = registry.get(*id).and_then(|e| e.auto_lock_minutes);
            past_idle_backstop(*idle, own, default_minutes)
        })
        .map(|(id, _)| id)
        .collect();
    if due.is_empty() {
        return;
    }
    take_back_clipboard(app, Some(&due));
    for id in due {
        if state.close_session(id).is_ok() {
            let _ = app.emit("silo-idle-locked", id.to_string());
        }
    }
    tell_if_scratch_survived(app, state.sweep_scratch());
    crate::commands::cloud::forget_sign_ins_when_all_locked(app);
}

/// The focused silo's metadata, for a silo that is already unlocked, so
/// switching to it does not ask again for a key already presented.
#[tauri::command(async)]
pub fn vault_meta(state: State<AppState>) -> Result<VaultMeta, String> {
    with_vfs(&state, |_session, vfs| vfs.meta())
}

#[tauri::command(async)]
pub fn vault_list_folder(
    folder_id: String,
    state: State<AppState>,
) -> Result<Vec<VaultEntry>, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    crate::state::with_vfs_untouched(&state, |_session, vfs| vfs.list_folder(folder_id))
}

#[tauri::command(async)]
pub fn vault_root_folder(state: State<AppState>) -> Result<FolderEntry, String> {
    with_vfs(&state, |_session, vfs| {
        let id = vfs.root_folder_id()?;
        vfs.get_folder(id)
    })
}

#[tauri::command(async)]
pub fn vault_get_folder(folder_id: String, state: State<AppState>) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.get_folder(folder_id))
}

#[tauri::command(async)]
pub fn vault_folder_by_path(path: String, state: State<AppState>) -> Result<FolderEntry, String> {
    with_vfs(&state, |_session, vfs| vfs.folder_by_path(&path))
}

/// Names only, across the whole vault.
///
/// Capped rather than paginated: nobody scrolls past fifty results, and a
/// query matching thousands means the user should type more, not that the
/// UI should render them all.
#[tauri::command(async)]
pub fn vault_search(query: String, state: State<AppState>) -> Result<Vec<SearchHit>, String> {
    with_vfs(&state, |_, vfs| vfs.search_entries(&query, 50))
}

#[tauri::command(async)]
pub fn vault_list_all_folders(state: State<AppState>) -> Result<Vec<FolderEntry>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_all_folders())
}

/// Stars or unstars one entry. `kind` is what the explorer already knows
/// about the row, rather than something to be guessed by looking the id up
/// in both tables.
#[tauri::command(async)]
pub fn vault_set_favorite(
    id: String,
    kind: String,
    favorite: bool,
    state: State<AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| match kind.as_str() {
        "folder" => vfs.set_folder_favorite(id, favorite).map(|_| ()),
        "file" => vfs.set_file_favorite(id, favorite).map(|_| ()),
        _ => Err(silentsilo_core::CoreError::InvalidPath(kind.clone())),
    })
}

#[tauri::command(async)]
pub fn vault_list_favorites(state: State<AppState>) -> Result<Vec<SearchHit>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_favorites())
}

#[tauri::command(async)]
pub fn vault_list_devices(state: State<AppState>) -> Result<Vec<DeviceInfo>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_devices())
}

/// What has happened to this silo, newest first.
///
/// Read straight from the local operation log, so it costs one query and
/// nothing is stored for it. See `silentsilo_vfs::activity` for what this is
/// and, more importantly, what it is not.
#[tauri::command(async)]
pub fn vault_activity(
    query: silentsilo_vfs::activity::ActivityQuery,
    state: State<AppState>,
) -> Result<silentsilo_vfs::ActivityPage, String> {
    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    // The page size is clamped inside `page` rather than trusted from here:
    // this is a window onto a log that can hold hundreds of thousands of
    // records.
    silentsilo_vfs::activity::page(&session.conn, &query).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn vault_set_device_label(
    device_id: String,
    label: String,
    state: State<AppState>,
) -> Result<(), String> {
    let device_id = Uuid::parse_str(&device_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| {
        vfs.set_device_label(device_id, &label)
    })
}

#[tauri::command]
pub async fn vault_import_files_to_folder(
    app: AppHandle,
    folder_id: String,
    paths: Vec<String>,
) -> Result<Vec<FileEntry>, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;

    run_blocking(move || {
        let state = app.state::<AppState>();
        let snapshot = crate::state::snapshot_focused_session(&state)?;
        let mut imported = Vec::with_capacity(paths.len());
        for path in paths {
            if state.import_cancelled.load(Ordering::Relaxed) {
                break;
            }
            if !crate::state::session_is_open(&state, snapshot.id) {
                return Err(SILO_CLOSED.to_string());
            }
            let source = PathBuf::from(&path);
            match import_one(&app, &snapshot, folder_id, &source) {
                Ok(file) => imported.push(file),
                Err(e) => {
                    log_added(
                        &app,
                        snapshot.id,
                        folder_id,
                        &imported,
                        imported.len() as u32,
                    );
                    return Err(e);
                }
            }
        }
        log_added(
            &app,
            snapshot.id,
            folder_id,
            &imported,
            imported.len() as u32,
        );
        Ok(imported)
    })
    .await
}

#[derive(Clone, serde::Serialize)]
pub struct PasteResult {
    imported_files: u32,
    imported_folders: u32,
    failed: Vec<String>,
}

/// Imports whatever the OS clipboard held (Ctrl+C in Explorer, Ctrl+V here) —
/// a mix of files and folders is fine; each folder is imported the same way
/// as "Upload folder" (recursively, with its own progress events), each file
/// the same way as a regular file upload. Per-item failures are collected
/// rather than aborting the whole paste, since one bad item (e.g. something
/// deleted after it was copied) shouldn't sink the rest of the batch.
#[tauri::command]
pub async fn vault_paste_paths(
    app: AppHandle,
    folder_id: String,
    paths: Vec<String>,
) -> Result<PasteResult, String> {
    let folder_uuid = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;

    run_blocking(move || {
        let state = app.state::<AppState>();
        let snapshot = crate::state::snapshot_focused_session(&state)?;
        let mut result = PasteResult {
            imported_files: 0,
            imported_folders: 0,
            failed: Vec::new(),
        };
        let mut pasted = Vec::new();

        for path in paths {
            if state.import_cancelled.load(Ordering::Relaxed) {
                break;
            }
            let source = PathBuf::from(&path);
            let name = source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&path)
                .to_string();

            if !crate::state::session_is_open(&state, snapshot.id) {
                return Err(SILO_CLOSED.to_string());
            }
            if source.is_dir() {
                match import_folder_impl(&app, folder_uuid, &source) {
                    Ok(0) => result.imported_folders += 1,
                    // Imported, minus what could not be read; said rather
                    // than counted as a clean folder.
                    Ok(skipped) => {
                        result.imported_folders += 1;
                        result
                            .failed
                            .push(format!("{name}: {skipped} items could not be read"));
                    }
                    // A cancelled sub-folder import means cancellation was
                    // requested — stop the whole paste rather than recording the
                    // folder as a failure.
                    Err(e) if e == IMPORT_CANCELLED => break,
                    // The silo went away. Every remaining item would fail for
                    // the same reason, and counting this folder as imported
                    // would report work that did not happen.
                    Err(e) if e == SILO_CLOSED => return Err(e),
                    Err(e) => result.failed.push(format!("{name}: {e}")),
                }
            } else if source.is_file() {
                match import_one(&app, &snapshot, folder_uuid, &source) {
                    Ok(file) => {
                        result.imported_files += 1;
                        pasted.push(file);
                    }
                    Err(e) => result.failed.push(format!("{name}: {e}")),
                }
            }
            // Neither a file nor a directory anymore (e.g. moved/deleted since
            // it was copied) — silently skipped, same as a no-op paste of it.
        }

        log_added(
            &app,
            snapshot.id,
            folder_uuid,
            &pasted,
            result.imported_files,
        );
        Ok(result)
    })
    .await
}

#[tauri::command(async)]
pub fn vault_create_folder(
    app: AppHandle,
    parent_id: String,
    name: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let parent_id = Uuid::parse_str(&parent_id).map_err(|e| e.to_string())?;
    let folder = with_vfs(&state, |_session, vfs| vfs.create_folder(parent_id, &name))?;
    let _ = crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::FOLDER_CREATED)
            .on(folder.id.to_string(), folder.path.clone()),
    );
    Ok(folder)
}

#[tauri::command(async)]
pub fn vault_rename_file(
    app: AppHandle,
    file_id: String,
    new_name: String,
    state: State<AppState>,
) -> Result<FileEntry, String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let old = with_vfs(&state, |_session, vfs| {
        vfs.get_file(file_id).map(|f| f.name)
    })?;
    if old != new_name {
        crate::audit::record(
            &app,
            crate::audit::event(crate::audit::codes::FILE_RENAMED)
                .on(file_id.to_string(), new_name.clone())
                .with("from", old),
        )?;
    }
    with_vfs(&state, |_session, vfs| vfs.rename_file(file_id, &new_name))
}

#[tauri::command(async)]
pub fn vault_rename_folder(
    app: AppHandle,
    folder_id: String,
    new_name: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let old = with_vfs(&state, |_session, vfs| {
        vfs.get_folder(folder_id).map(|f| f.name)
    })?;
    if old != new_name {
        crate::audit::record(
            &app,
            crate::audit::event(crate::audit::codes::FILE_RENAMED)
                .on(folder_id.to_string(), new_name.clone())
                .with("from", old)
                .with("folder", true),
        )?;
    }
    with_vfs(&state, |_session, vfs| {
        vfs.rename_folder(folder_id, &new_name)
    })
}

#[tauri::command(async)]
pub fn vault_trash_file(
    app: AppHandle,
    file_id: String,
    state: State<AppState>,
) -> Result<(), String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let name = with_vfs(&state, |_session, vfs| {
        vfs.get_file(file_id).map(|f| f.name)
    })?;
    crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::FILE_TRASHED).on(file_id.to_string(), name),
    )?;
    with_vfs(&state, |_session, vfs| vfs.trash_file(file_id))
}

#[tauri::command(async)]
pub fn vault_trash_folder(
    app: AppHandle,
    folder_id: String,
    state: State<AppState>,
) -> Result<(), String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let path = with_vfs(&state, |_session, vfs| {
        vfs.get_folder(folder_id).map(|f| f.path)
    })?;
    crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::FILE_TRASHED).on(folder_id.to_string(), path),
    )?;
    with_vfs(&state, |_session, vfs| vfs.trash_folder(folder_id))
}

#[tauri::command(async)]
pub fn vault_list_trash(state: State<AppState>) -> Result<Vec<TrashItem>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_trash())
}

#[tauri::command(async)]
pub fn vault_restore_file(
    app: AppHandle,
    file_id: String,
    state: State<AppState>,
) -> Result<FileEntry, String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let file = with_vfs(&state, |_session, vfs| vfs.restore_file(file_id))?;
    let _ = crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::FILE_RESTORED)
            .on(file.id.to_string(), file.name.clone()),
    );
    Ok(file)
}

#[tauri::command(async)]
pub fn vault_restore_folder(
    app: AppHandle,
    folder_id: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let folder = with_vfs(&state, |_session, vfs| vfs.restore_folder(folder_id))?;
    let _ = crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::FILE_RESTORED)
            .on(folder.id.to_string(), folder.path.clone()),
    );
    Ok(folder)
}

/// Permanently removes trashed items from the local index, and their blobs
/// from the local cache once a copy holds them. The bucket copies are left to the orphan sweep: it
/// deletes only what stayed unreferenced across two passes, so a restore or
/// an edit made concurrently on another device cannot lose content it still
/// points at. The blob ids come back already filtered against what the
/// surviving rows still reference, because a conflict copy is a second row
/// carrying the same blob id.
#[tauri::command]
pub async fn vault_empty_trash(app: AppHandle) -> Result<u64, String> {
    log_purge(
        &app,
        crate::audit::event(crate::audit::codes::FILE_PURGED).with("what", "trash"),
    )
    .await?;
    let app2 = app.clone();
    let (removed, blob_ids, root, silo_id) = run_blocking(move || {
        let state = app2.state::<AppState>();
        let silo_id = crate::state::focused_id(&state)?;
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let (removed, blob_ids) = Vfs::new(session).empty_trash().map_err(|e| e.to_string())?;
        Ok((removed, blob_ids, session.paths.root.clone(), silo_id))
    })
    .await?;

    let _ = run_blocking(move || {
        crate::commands::sync::release_purged_blobs(&app, silo_id, &root, &blob_ids);
        Ok(())
    })
    .await;

    Ok(removed)
}

/// Permanently deletes the named trashed entries, and the content they
/// were the last reference to.
///
/// The same purge the trash-emptying path runs, aimed at a selection: the
/// operation carries explicit ids either way, so a device replaying it
/// removes exactly these and nothing that happened to be in its own trash.
/// Bucket copies are left to the orphan sweep, as when emptying the trash.
#[tauri::command]
pub async fn vault_purge_items(app: AppHandle, ids: Vec<String>) -> Result<u64, String> {
    let ids: Vec<Uuid> = ids
        .iter()
        .map(|id| Uuid::parse_str(id).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    // Named while they still exist: once purged there is nothing to ask.
    let names: Vec<String> = with_vfs(&app.state::<AppState>(), |_session, vfs| {
        Ok(ids
            .iter()
            .take(LOGGED_NAMES)
            .filter_map(|id| {
                vfs.get_file(*id)
                    .map(|f| f.name)
                    .or_else(|_| vfs.get_folder(*id).map(|f| f.name))
                    .ok()
            })
            .collect())
    })
    .unwrap_or_default();
    let mut event = crate::audit::event(crate::audit::codes::FILE_PURGED);
    event = match (ids.len(), names.as_slice()) {
        (1, [one]) => event.on(ids[0].to_string(), one.clone()),
        _ => event.with("count", ids.len()).with("names", names),
    };
    log_purge(&app, event).await?;

    let app2 = app.clone();
    let (removed, blob_ids, root, silo_id) = run_blocking(move || {
        let state = app2.state::<AppState>();
        let silo_id = crate::state::focused_id(&state)?;
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let (removed, blob_ids) = Vfs::new(session)
            .purge_items(&ids)
            .map_err(|e| e.to_string())?;
        Ok((removed, blob_ids, session.paths.root.clone(), silo_id))
    })
    .await?;

    let _ = run_blocking(move || {
        crate::commands::sync::release_purged_blobs(&app, silo_id, &root, &blob_ids);
        Ok(())
    })
    .await;

    Ok(removed)
}

#[tauri::command]
pub async fn vault_import_file(
    app: AppHandle,
    folder_id: String,
    source_path: String,
) -> Result<FileEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let source = PathBuf::from(&source_path);

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        let file = import_one(&app, &snapshot, folder_id, &source)?;
        log_added(&app, snapshot.id, folder_id, std::slice::from_ref(&file), 1);
        Ok(file)
    })
    .await
}

/// Whether an import replaced the content of a file already there rather
/// than adding one. `add_file` keeps the file's id and creation time when the
/// name was taken, so a creation time older than the change is a
/// replacement. One made in the same millisecond reads as added.
fn replaced(file: &FileEntry) -> bool {
    file.created_at < file.updated_at
}

/// How many names an event about several files carries.
const LOGGED_NAMES: usize = 10;

/// Files added, logged once per import rather than once per file: by name
/// when it is one, otherwise the count and the first names, with the folder
/// they went into. After the fact and never refused: the files are already
/// in the silo.
fn log_added(app: &AppHandle, silo_id: Uuid, folder_id: Uuid, files: &[FileEntry], count: u32) {
    if count == 0 {
        return;
    }
    let mut event = crate::audit::event(crate::audit::codes::FILE_ADDED);
    match files {
        [one] if count == 1 => {
            event = event.on(one.id.to_string(), one.name.clone());
            if replaced(one) {
                event = event.with("replaced", true);
            }
        }
        _ => {
            event = event.with("count", count);
            let over = files.iter().filter(|f| replaced(f)).count();
            if over > 0 {
                event = event.with("replaced", over);
            }
            let names: Vec<String> = files
                .iter()
                .take(LOGGED_NAMES)
                .map(|f| f.name.clone())
                .collect();
            if !names.is_empty() {
                event = event.with("names", names);
            }
        }
    }
    if let Ok(folder) =
        crate::state::with_session_id(&app.state::<AppState>(), silo_id, |_session, vfs| {
            vfs.get_folder(folder_id).map(|f| f.path)
        })
    {
        event = event.with("folder", folder);
    }
    let _ = crate::audit::record_in(app, silo_id, event);
}

/// A purge about to happen, in the focused silo's log.
async fn log_purge(app: &AppHandle, event: silentsilo_audit::Event) -> Result<(), String> {
    let app = app.clone();
    run_blocking(move || crate::audit::record(&app, event)).await
}

/// Downloads any of `blob_ids` this device doesn't hold.
///
/// Export is the one place a missing blob is fatal rather than cosmetic: the
/// user asked for the bytes, and there is no partial answer. A vault with no
/// storage connected gets a message about *that*, not a decryption failure.
/// Every copy is tried: the first target being unreachable must not fail an
/// export the second could serve.
pub(crate) async fn ensure_blobs_local(app: &AppHandle, blob_ids: &[Uuid]) -> Result<(), String> {
    ensure_blobs_local_watched(app, blob_ids, None).await
}

/// A download shown on the progress card of `item` (a file or a folder) and
/// stopped when that item is cancelled. `sizes` gives each blob's size, so
/// the card counts bytes across all of them.
struct Watched<'a> {
    item: Uuid,
    sizes: &'a std::collections::HashMap<Uuid, u64>,
}

async fn ensure_blobs_local_watched(
    app: &AppHandle,
    blob_ids: &[Uuid],
    watched: Option<Watched<'_>>,
) -> Result<(), String> {
    let root = vault_dir(app)?;
    let missing: Vec<Uuid> = blob_ids
        .iter()
        .copied()
        .filter(|id| !root.join("blobs").join(format!("{id}.sslo")).is_file())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let targets = crate::state::silo_targets(app);
    if targets.is_empty() {
        return Err(
            "Some of these files are not on this computer, and no backup storage is connected."
                .into(),
        );
    }
    let stores: Vec<(Uuid, &dyn silentsilo_store::ObjectStore)> =
        targets.iter().map(|t| (t.id, &*t.store)).collect();
    let every_copy = crate::state::active_silo(app)
        .is_ok_and(|silo| silentsilo_vault::load_targets(silo.id).len() == targets.len());
    let size = |id: &Uuid| {
        watched
            .as_ref()
            .and_then(|w| w.sizes.get(id).copied())
            .unwrap_or(0)
    };
    let total: u64 = missing.iter().map(size).sum();
    let mut base = 0u64;
    let mut outcome = Ok(());
    for id in missing {
        let fetch = silentsilo_sync::fetch_blob_from_targets(&stores, &root, id, every_copy);
        let fetched = match &watched {
            None => Some(fetch.await),
            Some(w) => {
                let watch = ProgressWatch::start_from(
                    app,
                    w.item,
                    "downloading",
                    root.join("blobs").join(format!("{id}.sslo.part")),
                    base,
                    total,
                );
                let fetched = until_cancelled(app, w.item, fetch).await;
                watch.stop();
                fetched
            }
        };
        base += size(&id);
        match fetched {
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                outcome = Err(crate::err::coded_with(
                    "err.download_failed",
                    format!("Could not download the file: {e}"),
                    &[("detail", &e)],
                ));
                break;
            }
            None => {
                outcome = Err(OPEN_CANCELLED.to_string());
                break;
            }
        }
    }
    // On the copy it came from, so no longer waiting to back up there. Also
    // after a stop: what came down before it stays.
    if let Ok(silo) = crate::state::active_silo(app) {
        let every_target: Vec<Uuid> = silentsilo_vault::load_targets(silo.id)
            .iter()
            .map(|t| t.config.target_id())
            .collect();
        let _ = silentsilo_vault::settle_blob_delivery(&root, &every_target);
    }
    outcome
}

/// `work`, or `None` once `item` is cancelled.
async fn until_cancelled<T>(
    app: &AppHandle,
    item: Uuid,
    work: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            done = &mut work => return Some(done),
            _ = tokio::time::sleep(std::time::Duration::from_millis(150)) => {
                if is_cancelled(app, item) {
                    return None;
                }
            }
        }
    }
}

fn is_cancelled(app: &AppHandle, item: Uuid) -> bool {
    crate::state::lock_recovering(&app.state::<AppState>().open_cancelled).contains(&item)
}

#[tauri::command]
pub async fn vault_export_file(
    app: AppHandle,
    file_id: String,
    dest_path: String,
) -> Result<(), String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&dest_path);
    crate::state::lock_recovering(&app.state::<AppState>().open_cancelled).remove(&file_id);

    // The row and its wrapped key under one short lock, before the network
    // and before the decrypt: neither of those may hold the sessions mutex,
    // or every small command in the app queues behind a large file.
    let (blob_id, name, size, wrapped_key) = {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let vfs = Vfs::new(session);
        let file = vfs.get_file(file_id).map_err(|e| e.to_string())?;
        let wrapped = vfs.blob_key(file_id).map_err(|e| e.to_string())?;
        (
            file.blob_id,
            file.name,
            file.size_bytes.max(0) as u64,
            wrapped,
        )
    };
    // On the same card as opening a file, Cancel included.
    let sizes = std::collections::HashMap::from([(blob_id, size)]);
    let watched = Watched {
        item: file_id,
        sizes: &sizes,
    };
    ensure_blobs_local_watched(&app, &[blob_id], Some(watched)).await?;
    if is_cancelled(&app, file_id) {
        return Err(OPEN_CANCELLED.into());
    }

    let watch = ProgressWatch::start(&app, file_id, "decrypting", part_path(&dest), size);
    let written = run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        crate::audit::record_in(
            &app,
            snapshot.id,
            crate::audit::event(crate::audit::codes::FILE_SAVED_OUTSIDE)
                .on(file_id.to_string(), name),
        )?;
        let key = unwrap_export_key(&wrapped_key, &snapshot.kek)?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string())?;
        let _ = touch_blob_access(&snapshot.root, blob_id);
        Ok(())
    })
    .await;
    watch.stop();
    written
}

/// The key a file's content is encrypted under, unwrapped for use.
///
/// Refused rather than guessed when the row carries no key: handing the
/// wrong key to a decrypt produces an authentication failure that reads as
/// corrupted content, and sends someone looking for a damaged disk. Takes
/// the wrapped key rather than a session, because its callers carry the key
/// out of the lock and unwrap where no lock is held.
fn unwrap_export_key(
    wrapped: &str,
    kek: &silentsilo_crypto::ContentKek,
) -> Result<silentsilo_crypto::ContentKey, String> {
    if wrapped.is_empty() {
        return Err(crate::err::coded!(
            "err.file_no_key",
            "This file has no key recorded, so it cannot be opened."
        )
        .into());
    }
    silentsilo_crypto::unwrap_content_key(wrapped, kek).map_err(|_| {
        crate::err::coded!(
            "err.file_key_unreadable",
            "This file's key could not be read, so it cannot be opened."
        )
        .to_string()
    })
}

/// Which of `names` are already in `dest_dir`.
///
/// Saving a single file goes through the system's save dialog, which asks
/// about an existing name itself. Saving several does not, and used to
/// replace whatever had the same name without a word. Asked once, before
/// the first write: answering per file mid-run would put a dialog between
/// the user and a save they already started.
///
/// A name the export would refuse anyway (`safe_join`) fails here too,
/// rather than being reported as clash-free and then rejected.
#[tauri::command(async)]
pub fn export_clashes(dest_dir: String, names: Vec<String>) -> Result<Vec<String>, String> {
    let dest = PathBuf::from(&dest_dir);
    let mut clashes = Vec::new();
    for name in names {
        if safe_join(&dest, &name)?.try_exists().unwrap_or(false) {
            clashes.push(name);
        }
    }
    Ok(clashes)
}

/// The files a folder export would write over, relative to `dest_dir`.
///
/// The whole subtree, because the export merges into a directory of the
/// same name rather than replacing it: only the files that collide are at
/// risk, and naming the directory alone would overstate what is lost.
#[tauri::command(async)]
pub fn vault_export_folder_clashes(
    app: AppHandle,
    folder_id: String,
    dest_dir: String,
) -> Result<Vec<String>, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let root = PathBuf::from(&dest_dir);
    let state = app.state::<AppState>();
    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    let vfs = Vfs::new(session);
    let folder = vfs.get_folder(folder_id).map_err(|e| e.to_string())?;
    let dest_base = safe_join(&root, &folder.name)?;
    let mut plan = Vec::new();
    plan_export(&vfs, folder_id, &dest_base, &mut plan)?;
    Ok(plan
        .iter()
        .filter_map(|item| match item {
            ExportItem::File { dest, .. } => Some(dest),
            ExportItem::Dir(_) => None,
        })
        .filter(|dest| dest.try_exists().unwrap_or(false))
        .map(|dest| {
            dest.strip_prefix(&root)
                .unwrap_or(dest)
                .to_string_lossy()
                .to_string()
        })
        .collect())
}

/// Recursively decrypt a folder (and all its subfolders/files) into
/// `dest_dir`, downloading any cloud-only blobs on demand. The folder
/// itself is recreated as a subdirectory of `dest_dir` (matching normal
/// "download folder" behavior — you pick a destination, not the final
/// path). Returns the number of files exported.
///
/// `skip_existing` is the answer to the question `vault_export_folder_clashes`
/// fed: files already on disk are left as they are, and the count that comes
/// back counts what was actually written.
#[tauri::command]
pub async fn vault_export_folder(
    app: AppHandle,
    folder_id: String,
    dest_dir: String,
    skip_existing: bool,
) -> Result<u32, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    crate::state::lock_recovering(&app.state::<AppState>().open_cancelled).remove(&folder_id);

    // The whole subtree as a plan — destination paths, blob ids, wrapped
    // keys — read under one short lock at database speed. The download and
    // the decrypts run against the plan, holding nothing, so listing a
    // folder while a large export runs stays instant.
    let (plan, folder_path) = {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let vfs = Vfs::new(session);

        let folder = vfs.get_folder(folder_id).map_err(|e| e.to_string())?;
        let dest_base = safe_join(&PathBuf::from(&dest_dir), &folder.name)?;
        let mut plan = vec![ExportItem::Dir(dest_base.clone())];
        plan_export(&vfs, folder_id, &dest_base, &mut plan)?;
        (plan, folder.path)
    };

    let sizes: std::collections::HashMap<Uuid, u64> = plan
        .iter()
        .filter_map(|item| match item {
            ExportItem::File { blob_id, size, .. } => Some((*blob_id, *size)),
            ExportItem::Dir(_) => None,
        })
        .collect();
    let blob_ids: Vec<Uuid> = sizes.keys().copied().collect();
    let file_count = plan
        .iter()
        .filter(|item| matches!(item, ExportItem::File { .. }))
        .count() as u64;
    let watched = Watched {
        item: folder_id,
        sizes: &sizes,
    };
    ensure_blobs_local_watched(&app, &blob_ids, Some(watched)).await?;

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        crate::audit::record_in(
            &app,
            snapshot.id,
            crate::audit::event(crate::audit::codes::FILE_SAVED_OUTSIDE)
                .on(folder_id.to_string(), folder_path)
                .with("files", blob_ids.len()),
        )?;
        let paths = silentsilo_vault::VaultPaths::new(snapshot.root.clone());
        let mut exported = 0u32;
        for item in &plan {
            match item {
                ExportItem::Dir(dir) => {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                ExportItem::File {
                    dest,
                    blob_id,
                    wrapped_key,
                    ..
                } => {
                    if is_cancelled(&app, folder_id) {
                        return Err(OPEN_CANCELLED.to_string());
                    }
                    if skip_existing && dest.try_exists().unwrap_or(false) {
                        continue;
                    }
                    // Stops with the silo. The files already written were
                    // asked for; the rest were not decrypted after a lock.
                    if !crate::state::session_is_open(&app.state::<AppState>(), snapshot.id) {
                        return Err(CoreError::VaultLocked.to_string());
                    }
                    let key = unwrap_export_key(wrapped_key, &snapshot.kek)?;
                    decrypt_blob(&paths.blob_path(*blob_id), dest, &key, *blob_id)
                        .map_err(|e| e.to_string())?;
                    let _ = touch_blob_access(&snapshot.root, *blob_id);
                    exported += 1;
                    open_progress(&app, folder_id, "decrypting", exported.into(), file_count);
                }
            }
        }
        Ok(exported)
    })
    .await
}

/// One step of a folder export, resolved while the session was in hand.
enum ExportItem {
    Dir(PathBuf),
    File {
        dest: PathBuf,
        blob_id: Uuid,
        size: u64,
        wrapped_key: String,
    },
}

/// Walks the subtree under the lock, producing the plan the lock-free half
/// executes. Parents land before their children, so the create-then-write
/// order takes care of itself.
fn plan_export(
    vfs: &Vfs,
    folder_id: Uuid,
    dest: &Path,
    plan: &mut Vec<ExportItem>,
) -> Result<(), String> {
    for entry in vfs.list_folder(folder_id).map_err(|e| e.to_string())? {
        match entry {
            VaultEntry::Folder(sub) => {
                let sub_dest = safe_join(dest, &sub.name)?;
                plan.push(ExportItem::Dir(sub_dest.clone()));
                plan_export(vfs, sub.id, &sub_dest, plan)?;
            }
            VaultEntry::File(file) => {
                plan.push(ExportItem::File {
                    dest: safe_join(dest, &file.name)?,
                    blob_id: file.blob_id,
                    size: file.size_bytes.max(0) as u64,
                    wrapped_key: vfs.blob_key(file.id).map_err(|e| e.to_string())?,
                });
            }
        }
    }
    Ok(())
}

/// Defense-in-depth against a traversal-shaped name (`..`, a path
/// separator) reaching this point despite `silentsilo-vfs`'s own
/// validation at creation/rename time — e.g. a vault.db written by an
/// older client version, or one tampered with outside the normal app.
/// `Path::join` does not sanitize `..` components, so without this check
/// a bad name could write outside the directory the user chose to export
/// into.
fn safe_join(dest: &Path, name: &str) -> Result<PathBuf, String> {
    // A colon belongs here with the separators: on Windows `C:evil` is a
    // drive-relative path, and `Path::join` lets it replace the destination
    // outright rather than landing inside it.
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(format!("This name cannot be used as a file name: {name:?}"));
    }
    Ok(dest.join(name))
}

/// Where a file goes when the user opens it rather than exporting it.
///
/// Under the vault's own directory, not the shared OS temp dir, which many
/// other processes read routinely — indexers, backup agents, antivirus.
pub fn open_scratch_dir(vault_root: &Path) -> PathBuf {
    silentsilo_vault::work_dir_for(vault_root).join("open")
}

/// Removes every decrypted copy left behind by opening files.
///
/// Called on lock and on exit, so plaintext never outlives the session that
/// asked for it. Best-effort: an application still holding a file open will
/// block the delete on Windows, and failing the lock over that would be
/// worse than one file surviving until the next attempt — which is why this
/// also runs on unlock, cleaning up whatever a crash left.
pub fn wipe_open_scratch(vault_root: &Path) {
    let dir = open_scratch_dir(vault_root);
    if !dir.exists() {
        return;
    }
    // Read-only is set on every file written here, and Windows refuses to
    // delete a read-only file, so the bit has to come off first.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = std::fs::metadata(&path) {
                let mut perms = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&path, perms);
            }
            let _ = std::fs::remove_file(&path);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Opens a file in whatever application the OS associates with it.
///
/// The copy is written read-only on purpose. Editing and saving would go to
/// a scratch file that gets wiped at lock, and the user would lose the work
/// with nothing on screen to warn them — an application complaining that it
/// cannot save is a far better outcome than silence.
///
/// Each step reports to the window (`open-progress`), since a large file
/// takes seconds to fetch and decrypt, and can be cancelled. A copy left as
/// it was written is opened again without decrypting it again.
#[tauri::command]
pub async fn vault_open_file(app: AppHandle, file_id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    crate::state::lock_recovering(&state.open_cancelled).remove(&file_id);
    let cancelled = || {
        crate::state::lock_recovering(&app.state::<AppState>().open_cancelled).contains(&file_id)
    };

    let (blob_id, name, size, wrapped_key) = {
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let vfs = Vfs::new(session);
        let file = vfs.get_file(file_id).map_err(|e| e.to_string())?;
        // Read here, while the session is in hand, because the decrypt below
        // runs on the blocking pool with only what it was given.
        let wrapped = vfs.blob_key(file_id).map_err(|e| e.to_string())?;
        (
            file.blob_id,
            file.name,
            file.size_bytes.max(0) as u64,
            wrapped,
        )
    };
    let root = vault_dir(&app)?;
    open_progress(&app, file_id, "preparing", 0, 0);

    // Fetched first when only backup storage has it, watched by the size of
    // the file it streams into, and given up on when the person cancels.
    let sizes = std::collections::HashMap::from([(blob_id, size)]);
    let watched = Watched {
        item: file_id,
        sizes: &sizes,
    };
    ensure_blobs_local_watched(&app, &[blob_id], Some(watched)).await?;

    // The original name, so the OS picks the right application and the
    // title bar says something the user recognises.
    let dir = open_scratch_dir(&root);
    silentsilo_vault::create_private_dir(&dir).map_err(|e| e.to_string())?;
    let dest = safe_join(&dir, &name)?;
    crate::audit::record_off_thread(
        &app,
        crate::state::focused_id(&state)?,
        crate::audit::event(crate::audit::codes::FILE_OPENED).on(file_id.to_string(), &name),
    )
    .await?;

    // Decrypted already in this unlock and left as it was written: opened
    // again as it is. Anything else, changed or gone, is decrypted afresh.
    let kept = crate::state::lock_recovering(&state.opened_copies)
        .get(&dest)
        .cloned();
    let reusable = kept.is_some_and(|kept| {
        kept.blob == blob_id && copy_of(&dest, blob_id).as_ref() == Some(&kept)
    });
    if !reusable {
        let watch = ProgressWatch::start(&app, file_id, "decrypting", part_path(&dest), size);
        let app2 = app.clone();
        let target = dest.clone();
        let written = run_blocking(move || {
            // The keys under a lock held for the copy; the decrypt of an
            // arbitrarily large file holds nothing, so the rest of the app
            // keeps answering while it runs.
            let snapshot = crate::state::snapshot_focused_session(&app2.state::<AppState>())?;
            remove_opened(&target);
            let key = silentsilo_crypto::unwrap_content_key(&wrapped_key, &snapshot.kek).map_err(
                |_| {
                    crate::err::coded!(
                        "err.file_key_unreadable",
                        "This file's key could not be read, so it cannot be opened."
                    )
                    .to_string()
                },
            )?;
            let blob_path =
                silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
            decrypt_blob(&blob_path, &target, &key, blob_id).map_err(|e| e.to_string())?;
            crate::state::discard_if_locked(&app2.state::<AppState>(), snapshot.id, &target)?;
            silentsilo_vault::seal_readonly(&target);
            let _ = touch_blob_access(&snapshot.root, blob_id);
            Ok(())
        })
        .await;
        watch.stop();
        written?;
        // Cancelled while it decrypted: the decrypt cannot be stopped half
        // way, so what it wrote goes now, and nothing opens.
        if cancelled() {
            remove_opened(&dest);
            return Err(OPEN_CANCELLED.into());
        }
        if let Some(copy) = copy_of(&dest, blob_id) {
            crate::state::lock_recovering(&state.opened_copies).insert(dest.clone(), copy);
        }
    }

    open_progress(&app, file_id, "opening", size, size);
    app.opener()
        .open_path(dest.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// What a cancelled opening answers with; the window shows nothing for it.
const OPEN_CANCELLED: &str = "Cancelled.";

/// Stops a file being made ready to open: a download is given up at once, a
/// decrypt is thrown away when it ends.
#[tauri::command(async)]
pub fn vault_open_cancel(app: AppHandle, file_id: String) {
    if let Ok(id) = Uuid::parse_str(&file_id) {
        crate::state::lock_recovering(&app.state::<AppState>().open_cancelled).insert(id);
    }
}

#[derive(Clone, serde::Serialize)]
struct OpenProgress {
    file_id: String,
    phase: &'static str,
    done: u64,
    total: u64,
}

fn open_progress(app: &AppHandle, file_id: Uuid, phase: &'static str, done: u64, total: u64) {
    let _ = app.emit(
        "open-progress",
        OpenProgress {
            file_id: file_id.to_string(),
            phase,
            done,
            total,
        },
    );
}

/// Reports a step by the size of the file it is writing, a few times a
/// second, until stopped. A download and a decrypt both write to a `.part`
/// beside their result, so neither needs to report on its own.
struct ProgressWatch(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl ProgressWatch {
    fn start(
        app: &AppHandle,
        file_id: Uuid,
        phase: &'static str,
        path: PathBuf,
        total: u64,
    ) -> Self {
        Self::start_from(app, file_id, phase, path, 0, total)
    }

    /// `base` bytes done before this file, for a step over several.
    fn start_from(
        app: &AppHandle,
        file_id: Uuid,
        phase: &'static str,
        path: PathBuf,
        base: u64,
        total: u64,
    ) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watching = stop.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while !watching.load(Ordering::Relaxed) {
                let done = base + std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                open_progress(&app, file_id, phase, done.min(total), total);
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        });
        Self(stop)
    }

    fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// `path.part`, where `decrypt_blob` writes before it renames.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// The copy at `path` as it is now, to compare with how it was left.
fn copy_of(path: &Path, blob: Uuid) -> Option<crate::state::OpenedCopy> {
    let meta = std::fs::metadata(path).ok()?;
    Some(crate::state::OpenedCopy {
        blob,
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Removes a decrypted copy, read-only as it is left: Windows will not
/// delete a read-only file, so the bit comes off first.
fn remove_opened(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
    let _ = std::fs::remove_file(path);
}

/// Every password entry, as a JSON array.
///
/// Entries live as rows in the encrypted index rather than as a file in the
/// tree. Nothing decrypted touches the disk on this path: the rows come
/// straight out of the open database.
#[tauri::command(async)]
pub fn vault_read_passwords(state: State<AppState>) -> Result<String, String> {
    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    let entries = Vfs::new(session)
        .list_passwords()
        .map_err(|e| e.to_string())?;

    // Each row is already JSON, so the array is assembled rather than
    // re-serialised through a struct this layer would have to know.
    Ok(format!("[{}]", entries.join(",")))
}

/// What a save was, for the activity log. The window knows: whether the
/// entry existed, and whether this save restores or clears its history.
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum EntryChange {
    Created,
    Edited,
    Restored,
    HistoryCleared,
    /// One of many in an import, logged once for the whole import.
    Imported,
    /// Starred, unstarred or moved to another category: what it says did
    /// not change, so nothing is logged.
    Arranged,
}

impl EntryChange {
    fn code(self) -> Option<u16> {
        use crate::audit::codes;
        match self {
            EntryChange::Created => Some(codes::ENTRY_CREATED),
            EntryChange::Edited => Some(codes::ENTRY_EDITED),
            EntryChange::Restored => Some(codes::ENTRY_RESTORED),
            EntryChange::HistoryCleared => Some(codes::HISTORY_CLEARED),
            EntryChange::Imported | EntryChange::Arranged => None,
        }
    }
}

/// The name an entry is shown under, for the log.
fn entry_label(parsed: &serde_json::Value) -> String {
    parsed
        .get("service")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Creates or replaces one entry, keyed by the id the panel generated.
#[tauri::command(async)]
pub fn vault_upsert_password(
    app: AppHandle,
    id: String,
    json: String,
    change: Option<EntryChange>,
    state: State<AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("That entry id is not valid: {e}"))?;

    // Parsed to reject anything that would not survive a round trip, and to
    // make sure the id in the record matches the one being written. A row
    // whose body disagrees with its key is the kind of thing that only
    // surfaces on another device, months later.
    let parsed: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("The entry could not be read: {e}"))?;
    match parsed.get("id").and_then(|v| v.as_str()) {
        Some(inner) if inner == id.to_string() => {}
        Some(_) => return Err("The entry id does not match the entry.".into()),
        None => return Err("The entry has no id.".into()),
    }

    // The category list is a row of its own, not an entry anyone edited.
    let is_entry = !parsed
        .get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|t| t.starts_with("meta:"));
    if let Some(code) = change.unwrap_or(EntryChange::Edited).code()
        && is_entry
    {
        crate::audit::record(
            &app,
            crate::audit::event(code).on(id.to_string(), entry_label(&parsed)),
        )?;
    }

    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    Vfs::new(session)
        .upsert_password(id, &json)
        .map_err(|e| e.to_string())
}

/// Copies a secret to the clipboard, kept out of Windows Clipboard History
/// and cloud sync, and cleared shortly afterwards. The webview's own
/// clipboard API is the wrong tool: what it writes is retained by
/// Clipboard History, which persists to disk, and by Cloud Clipboard,
/// which syncs it to the user's other machines.
///
/// On the blocking pool because taking the clipboard means waiting for
/// whoever holds it, which `silentsilo-shell` does by sleeping between
/// retries for up to a fifth of a second.
///
/// `audit` says what the secret was, for the silo's activity log; it is
/// written before the clipboard holds anything. The recovery code on its
/// way to paper is the one copy without it.
#[tauri::command]
pub async fn copy_secret_to_clipboard(
    app: AppHandle,
    text: String,
    audit: Option<crate::audit::CopiedSecret>,
) -> Result<(), String> {
    let copied = text.clone();
    let owner = crate::state::focused_id(&app.state::<AppState>()).ok();
    if let (Some(audit), Some(owner)) = (audit, owner) {
        let app = app.clone();
        run_blocking(move || crate::audit::record_in(&app, owner, audit.event())).await?;
    }
    run_blocking(move || silentsilo_shell::set_secret_clipboard(&copied)).await?;
    if let Ok(mut held) = CLIPBOARD_OWNER.lock() {
        *held = owner;
    }

    // Cleared only if it is still ours: by the time this fires the user has
    // often copied something else, and wiping that would be the app reaching
    // into something that stopped being its business.
    //
    // On the async runtime rather than an OS thread of its own. Every copy
    // arms one of these and each sleeps for the best part of a minute, so a
    // few minutes of ordinary use spawned dozens of threads whose whole job
    // was to wait.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(
            silentsilo_shell::SECRET_CLIPBOARD_TTL_SECS,
        ))
        .await;
        if silentsilo_shell::clear_expired_secret(&text) {
            let _ = app.emit("clipboard-cleared", ());
        }
    });
    Ok(())
}

/// The silo the secret on the clipboard came from, if one was focused.
static CLIPBOARD_OWNER: std::sync::Mutex<Option<Uuid>> = std::sync::Mutex::new(None);

/// Takes back a copied secret when a silo closes: a password copied a
/// moment before the lock would otherwise stay readable for the rest of
/// its forty-five seconds.
///
/// `closing` names the silos going away, `None` meaning all of them. A
/// secret another silo copied stays; one whose silo is not known goes.
pub(crate) fn take_back_clipboard(app: &AppHandle, closing: Option<&[Uuid]>) {
    let owner = CLIPBOARD_OWNER.lock().map(|o| *o).unwrap_or(None);
    if !clipboard_goes(owner, closing) {
        return;
    }
    if silentsilo_shell::clear_secret_clipboard_now() {
        let _ = app.emit("clipboard-cleared", ());
    }
}

fn clipboard_goes(owner: Option<Uuid>, closing: Option<&[Uuid]>) -> bool {
    match (owner, closing) {
        (_, None) => true,
        // Nothing is closing, so nothing is taken back.
        (_, Some([])) => false,
        (None, Some(_)) => true,
        (Some(owner), Some(closing)) => closing.contains(&owner),
    }
}

/// Removes one entry outright. There is no trash for logins. `label` is
/// the name it had, for the activity log.
#[tauri::command(async)]
pub fn vault_delete_password(
    app: AppHandle,
    id: String,
    label: Option<String>,
    state: State<AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("That entry id is not valid: {e}"))?;
    crate::audit::record(
        &app,
        crate::audit::event(crate::audit::codes::ENTRY_DELETED)
            .on(id.to_string(), label.unwrap_or_default()),
    )?;
    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;
    Vfs::new(session)
        .delete_password(id)
        .map_err(|e| e.to_string())
}

/// Proves the user is still at the keyboard, without touching the open
/// session. The touch only counts if the key it produces actually unwraps
/// this silo's DEK; a bare presence check would accept any FIDO key in the
/// drawer. Used by entries marked "ask again before revealing". `purpose`
/// picks the prompt's wording from a fixed list, never free text.
#[tauri::command]
pub async fn fido_reverify(app: AppHandle, purpose: Option<String>) -> Result<(), String> {
    let why = match purpose.as_deref() {
        Some("export") => Presence::ExportLogins,
        _ => Presence::ShowEntry,
    };
    verify_presence(&app, why).await
}

/// Whether the focused silo has a security key or Windows Hello enrolled,
/// the only things [`verify_presence`] can ask.
pub(crate) fn presence_check_enrolled(app: &AppHandle) -> bool {
    vault_dir(app).is_ok_and(|root| is_fido_enrolled(&root))
}

/// The ceremony behind `fido_reverify`, shared with the browser fill so both
/// ask the same thing before a secret leaves the app. `purpose` completes
/// "Confirm with Windows Hello to …".
pub(crate) async fn verify_presence(app: &AppHandle, purpose: Presence) -> Result<(), String> {
    let root = vault_dir(app)?;
    let creds = crate::state::silo_credentials(app)?;

    if !is_fido_enrolled(&root) {
        return Err(
            crate::err::coded!("err.no_key_enrolled", "No key is enrolled on this silo.").into(),
        );
    }

    let keys = silentsilo_vault::load_fido_keys(&root).map_err(|e| e.to_string())?;
    let cred_ids = keys.credential_ids_bytes().map_err(|e| e.to_string())?;
    let vault_id = creds.vault_id.to_string();

    let wanted = crate::commands::fido::preferred_authenticator(&keys);
    emit_fido_progress(
        app,
        purpose.prompt(matches!(
            wanted,
            Some(silentsilo_fido::Authenticator::ThisDevice)
        )),
    );
    let unlock = run_fido(app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id, wanted)
    })
    .await?;

    let stored = keys
        .find_by_credential_id(&unlock.credential_id)
        .or_else(|| keys.primary())
        .ok_or_else(|| {
            crate::err::coded!(
                "err.key_not_enrolled",
                "That key is not enrolled on this silo."
            )
            .to_string()
        })?;

    silentsilo_vault::unwrap_dek_hex(&stored.wrapped_dek, &unlock.wrap_key)
        .map(|_| ())
        .map_err(|_| {
            crate::err::coded!(
                "err.key_cannot_verify",
                "That key could not verify this silo."
            )
            .to_string()
        })
}

#[derive(serde::Serialize)]
pub struct SshKeypair {
    private_key: String,
    public_key: String,
    fingerprint: String,
}

/// Generates an ed25519 keypair in OpenSSH format for an SSH-key entry. In
/// Rust because WebCrypto has no OpenSSH serialisation and the ssh-key
/// crate is already in the tree. Ed25519 only.
#[tauri::command(async)]
pub fn ssh_generate_keypair() -> Result<SshKeypair, String> {
    use ssh_key::private::{Ed25519Keypair, KeypairData};
    use ssh_key::{HashAlg, LineEnding, PrivateKey};

    // Seeded from the app's own RNG rather than ssh-key's generic
    // constructor, whose rand_core bound tracks a different release train
    // than the rand already in this tree.
    let seed: [u8; 32] = rand::random();
    let key = PrivateKey::new(KeypairData::Ed25519(Ed25519Keypair::from_seed(&seed)), "")
        .map_err(|e| e.to_string())?;
    Ok(SshKeypair {
        private_key: key
            .to_openssh(LineEnding::LF)
            .map_err(|e| e.to_string())?
            .to_string(),
        public_key: key.public_key().to_openssh().map_err(|e| e.to_string())?,
        fingerprint: key.public_key().fingerprint(HashAlg::Sha256).to_string(),
    })
}

/// One file kept with a password entry, as the panel stores it inside the
/// entry's JSON. The blob is real; the file row is deliberately absent.
#[derive(serde::Serialize)]
pub struct PasswordAttachment {
    blob_id: String,
    name: String,
    size_bytes: i64,
    /// This attachment's content key, wrapped under the vault DEK.
    ///
    /// Kept in the entry rather than in a table because an attachment has no
    /// row anywhere: the sealed entry is its only reference. The entry is
    /// sealed under the vault key before it is stored, so this is never at
    /// rest in the clear.
    blob_key: String,
}

impl PasswordAttachment {
    pub(crate) fn blob_id(&self) -> &str {
        &self.blob_id
    }
}

/// Encrypts a picked file into the blob store for a password entry:
/// everything an import does except the index row, so the attachment
/// appears exactly where the password does and nowhere else. The cache
/// bookkeeping is what gets the blob uploaded by the next sync pass.
#[tauri::command]
pub async fn password_attach_file(
    app: AppHandle,
    path: String,
) -> Result<PasswordAttachment, String> {
    let source = PathBuf::from(&path);
    if !source.is_file() {
        return Err(crate::err::coded!("err.not_a_file", "Not a file.").into());
    }

    // The keys under a lock held for the copy; the encryption of an
    // arbitrarily large attachment holds nothing at all. Nothing lands in
    // the database here: the attachment lives in the entry's own JSON, so
    // there is no commit half.
    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;

        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                crate::err::coded!("err.file_name_unreadable", "That file name cannot be read.")
                    .to_string()
            })?
            .to_string();

        let blob_id = Uuid::new_v4();
        let content_key = silentsilo_crypto::generate_content_key();
        let blob_key = silentsilo_crypto::wrap_content_key(&content_key, &snapshot.kek)
            .map_err(|e| e.to_string())?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        let result = encrypt_file(&source, &blob_path, &content_key, Uuid::now_v7(), blob_id)
            .map_err(|e| e.to_string())?;
        let _ = silentsilo_vault::record_blob_present(
            &snapshot.root,
            blob_id,
            result.size_bytes as i64,
            false,
        );

        Ok(PasswordAttachment {
            blob_id: blob_id.to_string(),
            name,
            size_bytes: result.plain_bytes as i64,
            blob_key,
        })
    })
    .await
}

/// An attachment made from bytes already in memory, as an import has them:
/// encrypted straight into the silo, never written in clear.
pub(crate) fn encrypt_attachment_bytes(
    snapshot: &crate::state::SessionSnapshot,
    name: &str,
    bytes: &[u8],
) -> Result<PasswordAttachment, String> {
    let blob_id = Uuid::new_v4();
    let content_key = silentsilo_crypto::generate_content_key();
    let blob_key = silentsilo_crypto::wrap_content_key(&content_key, &snapshot.kek)
        .map_err(|e| e.to_string())?;
    let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
    let result = silentsilo_crypto::encrypt_stream(
        &mut std::io::Cursor::new(bytes),
        &blob_path,
        &content_key,
        Uuid::now_v7(),
        blob_id,
    )
    .map_err(|e| e.to_string())?;
    let _ = silentsilo_vault::record_blob_present(
        &snapshot.root,
        blob_id,
        result.size_bytes as i64,
        false,
    );
    Ok(PasswordAttachment {
        blob_id: blob_id.to_string(),
        name: name.to_string(),
        size_bytes: result.plain_bytes as i64,
        blob_key,
    })
}

/// An attachment's content, for an export that writes it into another file.
/// Decrypted through the scratch directory, which a lock sweeps, and removed
/// as soon as it is read.
pub(crate) fn decrypt_attachment_bytes(
    snapshot: &crate::state::SessionSnapshot,
    blob_id: &str,
    blob_key: &str,
) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    let blob_id = Uuid::parse_str(blob_id).map_err(|e| e.to_string())?;
    let key = silentsilo_crypto::unwrap_content_key(blob_key, &snapshot.kek).map_err(|_| {
        crate::err::coded!(
            "err.attachment_key_unreadable",
            "An attached file's key could not be read."
        )
        .to_string()
    })?;
    let dir = open_scratch_dir(&snapshot.root);
    silentsilo_vault::create_private_dir(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("export-{}", Uuid::new_v4()));
    let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
    let decrypted = decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string());
    let bytes = decrypted.and_then(|_| std::fs::read(&dest).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&dest);
    bytes.map(zeroize::Zeroizing::new)
}

/// Opens an attachment the way a vault file opens: decrypted into the
/// scratch directory, read-only, handed to the OS. Fetches the blob from
/// backup first when this device does not hold it.
#[tauri::command]
pub async fn password_open_attachment(
    app: AppHandle,
    blob_id: String,
    name: String,
    blob_key: String,
    entry_label: Option<String>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let blob_id = Uuid::parse_str(&blob_id).map_err(|e| e.to_string())?;
    ensure_blobs_local(&app, &[blob_id]).await?;

    let app2 = app.clone();
    let path = run_blocking(move || {
        // Same shape as opening a vault file: keys under a short lock, the
        // decrypt itself under none.
        let snapshot = crate::state::snapshot_focused_session(&app2.state::<AppState>())?;

        let dir = open_scratch_dir(&snapshot.root);
        silentsilo_vault::create_private_dir(&dir).map_err(|e| e.to_string())?;
        // The name comes out of entry JSON, which an import fills from a
        // file someone else made: repaired as a file name in the silo is
        // (a device name such as CON, a trailing dot), then joined through
        // the same guard as every export, so it cannot walk out of the
        // scratch directory.
        let dest = safe_join(&dir, &silentsilo_vfs::sanitize_name(&name))?;
        let _ = std::fs::remove_file(&dest);
        crate::audit::record_in(
            &app2,
            snapshot.id,
            crate::audit::event(crate::audit::codes::FILE_OPENED)
                .on(blob_id.to_string(), &name)
                .with("entry", entry_label.unwrap_or_default()),
        )?;

        let key =
            silentsilo_crypto::unwrap_content_key(&blob_key, &snapshot.kek).map_err(|_| {
                crate::err::coded!(
                    "err.file_key_unreadable",
                    "This file's key could not be read, so it cannot be opened."
                )
                .to_string()
            })?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string())?;
        crate::state::discard_if_locked(&app2.state::<AppState>(), snapshot.id, &dest)?;

        silentsilo_vault::seal_readonly(&dest);
        let _ = touch_blob_access(&snapshot.root, blob_id);
        Ok(dest)
    })
    .await?;

    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Removes an attachment's local content. The backup copy is left to the
/// orphan sweep: it only deletes what no entry has referenced across two
/// passes, so an edit the user then cancels, or a concurrent edit from
/// another device, cannot lose an attachment that is still referenced.
#[tauri::command]
pub async fn password_delete_attachment(app: AppHandle, blob_id: String) -> Result<(), String> {
    let blob_id = Uuid::parse_str(&blob_id).map_err(|e| e.to_string())?;
    let root = crate::state::unlocked_silo(&app)?.path;
    silentsilo_vault::remove_blob_from_cache(&root, blob_id).map_err(|e| e.to_string())
}

/// Upper bound on a password CSV the importer will read. A real export from
/// any manager is far under this; the cap exists so a mistakenly-picked
/// multi-gigabyte file fails fast instead of being slurped into memory and
/// handed to the webview.
const MAX_IMPORT_CSV_BYTES: u64 = 32 * 1024 * 1024;

/// Reads a user-chosen CSV for the password importer.
///
/// Deliberately narrow rather than a general "read any file" command: it
/// only ever returns UTF-8 text, refuses anything but a regular file, and is
/// size-capped. Requires an unlocked vault so it can't be driven while the
/// app is locked.
#[tauri::command]
pub async fn passwords_read_import_csv(app: AppHandle, path: String) -> Result<String, String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        drop(session_guard);

        let path = PathBuf::from(path);
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(crate::err::coded!("err.not_a_file", "Not a file.").into());
        }
        if meta.len() > MAX_IMPORT_CSV_BYTES {
            return Err(crate::err::coded!(
                "err.export_file_too_large",
                "That file is too large to be a password export."
            )
            .into());
        }

        std::fs::read_to_string(&path).map_err(|_| {
            "Could not read that file as text. Choose a CSV or a Bitwarden JSON export.".to_string()
        })
    })
    .await
}

/// Writes the plaintext password export to a user-chosen path.
///
/// The plaintext is the point — every other manager's importer expects an
/// unencrypted CSV, and a vault you can't leave is a trap. The UI warns
/// before calling this. On Unix the file is created 0600 so it isn't
/// world-readable for the window between writing it and the user moving or
/// deleting it.
#[tauri::command]
pub async fn passwords_write_export_csv(
    app: AppHandle,
    path: String,
    contents: String,
    count: Option<u32>,
) -> Result<(), String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        drop(session_guard);
        let mut event =
            crate::audit::event(crate::audit::codes::PASSWORDS_EXPORTED).with("format", "csv");
        if let Some(count) = count {
            event = event.with("count", count);
        }
        crate::audit::record(&app, event)?;

        write_owner_only(&PathBuf::from(path), contents.as_bytes())
    })
    .await
}

/// What the explorer needs to label a file: which blobs are on this disk,
/// which have not reached the backup yet, and how much room they take. One
/// command rather than three, so they cannot disagree on screen.
#[derive(serde::Serialize)]
pub struct BlobStatus {
    local: Vec<String>,
    unsynced: Vec<String>,
    /// Content the silo has that this disk does not. Answered from the
    /// index and the blobs directory, never the network: the index already
    /// names every blob the silo holds.
    missing: Vec<String>,
    missing_bytes: i64,
    /// Content the silo lists that no backup holds either: asked for and
    /// not found. Kept out of `missing`, which a download could still fix.
    absent: Vec<String>,
    usage: silentsilo_vault::CacheUsage,
}

#[tauri::command]
pub async fn vault_blob_status(app: AppHandle) -> Result<BlobStatus, String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        // The database's half under a short lock; the blobs directory —
        // thousands of entries on a full-copy silo, and this command runs on
        // a 20-second timer — is walked with no lock held at all.
        let (root, sizes, attachments) = {
            let session_guard = state.focused_session()?;
            let session = session_guard
                .as_ref()
                .ok_or_else(|| CoreError::VaultLocked.to_string())?;
            (
                session.paths.root.clone(),
                Vfs::new(session)
                    .list_blob_sizes()
                    .map_err(|e| e.to_string())?,
                attachment_sizes(session),
            )
        };

        let local = list_local_blob_ids(&root);
        let absent_ids: HashSet<Uuid> = silentsilo_vault::list_absent_blob_ids(&root)
            .into_iter()
            .collect();
        let mut absent = Vec::new();
        let here: HashSet<Uuid> = local.iter().copied().collect();

        let mut missing = Vec::new();
        let mut missing_bytes = 0i64;
        for (blob_id, size) in sizes {
            if here.contains(&blob_id) {
                continue;
            }
            if absent_ids.contains(&blob_id) {
                absent.push(blob_id.to_string());
            } else {
                missing.push(blob_id.to_string());
                missing_bytes += size;
            }
        }

        // Password attachments hold blobs the file tree never references, so
        // a recovery that only counted the tree would leave them behind.
        for (blob_id, size_bytes) in attachments {
            let id = blob_id.to_string();
            if here.contains(&blob_id) || missing.contains(&id) || absent.contains(&id) {
                continue;
            }
            if absent_ids.contains(&blob_id) {
                absent.push(id);
            } else {
                missing.push(id);
                missing_bytes += size_bytes;
            }
        }

        Ok(BlobStatus {
            local: local.into_iter().map(|id| id.to_string()).collect(),
            unsynced: silentsilo_vault::list_unsynced_blob_ids(&root)
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
            missing,
            missing_bytes,
            absent,
            usage: silentsilo_vault::cache_usage(&root),
        })
    })
    .await
}

/// Password attachments by silo, with a fingerprint of the sealed rows they
/// were read from.
type AttachmentCache = Option<(Uuid, u64, Vec<(Uuid, i64)>)>;
static ATTACHMENTS: std::sync::Mutex<AttachmentCache> = std::sync::Mutex::new(None);

/// Every attachment's blob and size. Finding them means opening every
/// password entry, secrets included, and the explorer asks every twenty
/// seconds, so the answer is kept until a sealed row changes. Sealing uses
/// a fresh nonce, so any edit changes the ciphertext the fingerprint reads.
fn attachment_sizes(session: &VaultSession) -> Vec<(Uuid, i64)> {
    let fingerprint = sealed_passwords_fingerprint(session);
    if let (Some(fingerprint), Ok(cache)) = (fingerprint, ATTACHMENTS.lock())
        && let Some((silo, seen, sizes)) = cache.as_ref()
        && *silo == session.vault_id
        && *seen == fingerprint
    {
        return sizes.clone();
    }
    let sizes: Vec<(Uuid, i64)> = Vfs::new(session)
        .attachment_blobs()
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.blob_id, a.size_bytes))
        .collect();
    if let (Some(fingerprint), Ok(mut cache)) = (fingerprint, ATTACHMENTS.lock()) {
        *cache = Some((session.vault_id, fingerprint, sizes.clone()));
    }
    sizes
}

/// A hash of the sealed password rows as stored, without opening any.
/// `None` when the table cannot be read, which means no caching.
fn sealed_passwords_fingerprint(session: &VaultSession) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let mut stmt = session
        .conn
        .prepare("SELECT id, data FROM passwords ORDER BY id")
        .ok()?;
    let mut rows = stmt.query([]).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    while let Some(row) = rows.next().ok()? {
        row.get::<_, String>(0).ok()?.hash(&mut hasher);
        row.get::<_, String>(1).ok()?.hash(&mut hasher);
    }
    Some(hasher.finish())
}

// ── Protected folders ───────────────────────────────────────────────

/// How far a scan is: files done of those found, 0 of 0 while walking.
#[derive(Clone, serde::Serialize)]
struct ScanProgress {
    done: usize,
    total: usize,
}

/// What one scan managed.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ProtectedScanReport {
    pub imported: usize,
    /// Files that could not be read or encrypted. Counted rather than
    /// aborted: a locked file in the middle of a folder should not cost the
    /// user the other nine hundred.
    pub skipped: usize,
}

/// The list and the ledger are sealed under the content KEK, so reading
/// either needs the silo open. An unlocked-only command rather than one that
/// answers with an empty list: a list that reads as empty while locked is the
/// app telling the user they protect nothing.
#[tauri::command(async)]
pub fn protected_folders_list(app: AppHandle) -> Result<Vec<ProtectedFolderView>, String> {
    let (silo, kek) = crate::state::unlocked_silo_with_kek(&app)?;
    Ok(silentsilo_vault::load_protected(&silo.path, &kek)
        .map_err(|e| e.to_string())?
        .folders
        .into_iter()
        .map(|f| ProtectedFolderView {
            path: f.path.to_string_lossy().to_string(),
            target: f.target,
        })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtectedFolderView {
    pub path: String,
    pub target: String,
}

/// Starts keeping a copy of a folder.
///
/// The target inside the silo is decided here rather than by the caller, so
/// two folders with the same name on different drives cannot land on top of
/// each other.
#[tauri::command(async)]
pub fn protected_folders_add(app: AppHandle, path: String) -> Result<(), String> {
    let (silo, kek) = crate::state::unlocked_silo_with_kek(&app)?;
    let source = PathBuf::from(&path);
    if !source.is_dir() {
        return Err(crate::err::coded!(
            "err.not_a_folder_here",
            "That is not a folder on this computer."
        )
        .into());
    }

    let mut list = silentsilo_vault::load_protected(&silo.path, &kek).map_err(|e| e.to_string())?;
    if list.folders.iter().any(|f| f.path == source) {
        return Ok(());
    }

    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Folder")
        .to_string();
    let mut target = format!("/{name}");
    let mut attempt = 2;
    while list.folders.iter().any(|f| f.target == target) {
        target = format!("/{name} ({attempt})");
        attempt += 1;
    }

    list.folders.push(silentsilo_vault::ProtectedFolder {
        path: source,
        target,
    });
    silentsilo_vault::save_protected(&silo.path, &kek, &list).map_err(|e| e.to_string())
}

/// Stops keeping a copy. What was already imported stays: this is an archive,
/// and removing a folder from the list is not a request to delete anything.
#[tauri::command(async)]
pub fn protected_folders_remove(app: AppHandle, path: String) -> Result<(), String> {
    let (silo, kek) = crate::state::unlocked_silo_with_kek(&app)?;
    let mut list = silentsilo_vault::load_protected(&silo.path, &kek).map_err(|e| e.to_string())?;
    let removing = Path::new(&path);
    list.folders.retain(|f| f.path != removing);
    silentsilo_vault::save_protected(&silo.path, &kek, &list).map_err(|e| e.to_string())
}

/// Walks every protected folder and imports what has changed.
///
/// Run on demand and after unlocking. Each file is marked as taken only once
/// it is in the silo, so an interrupted scan keeps what it managed and
/// retries the rest next time.
#[tauri::command]
pub async fn protected_folders_scan(app: AppHandle) -> Result<ProtectedScanReport, String> {
    // One scan at a time. Each reads the ledger of what it already took when
    // it starts, so a second scan started while the first was still walking
    // (the one unlock starts, then "Check now") imported the same files again.
    // The second waits for the first, then finds only what is left.
    static SCAN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _scanning = SCAN.lock().await;

    run_blocking(move || {
        let (silo, kek) = crate::state::unlocked_silo_with_kek(&app)?;
        let list = silentsilo_vault::load_protected(&silo.path, &kek).map_err(|e| e.to_string())?;
        if list.folders.is_empty() {
            return Ok(ProtectedScanReport::default());
        }

        let seen =
            silentsilo_vault::protected::load_seen(&silo.path, &kek).map_err(|e| e.to_string())?;
        let state = app.state::<AppState>();
        let snapshot = crate::state::snapshot_focused_session(&state)?;

        let mut report = ProtectedScanReport::default();
        // Counted on the settings page: a first scan of a large folder takes
        // minutes. Every folder is walked first, so the count has an end.
        let progress = |done: usize, total: usize| {
            let _ = app.emit("protected-scan-progress", ScanProgress { done, total });
        };
        progress(0, 0);
        let mut pending = Vec::new();
        for folder in &list.folders {
            // The walk itself reads the user's folders, never the silo, so
            // it holds no lock; only resolving a target path and committing
            // a row do, briefly, per file.
            pending.extend(
                silentsilo_vault::plan_scan(&folder.path, &folder.target, &seen)
                    .map_err(|e| e.to_string())?,
            );
        }
        let total = pending.len();
        for (done, item) in pending.into_iter().enumerate() {
            progress(done, total);
            let target = crate::state::with_session_id(&state, snapshot.id, |_s, vfs| {
                ensure_vault_path(vfs, &item.target_folder).map_err(CoreError::InvalidPath)
            });
            let Ok(target) = target else {
                report.skipped += 1;
                continue;
            };
            match import_one(&app, &snapshot, target, &item.source) {
                Ok(_) => {
                    // Marked after the import, never before: the other order
                    // would skip a file that never arrived and leave it
                    // missing until someone touched it again.
                    let _ = silentsilo_vault::protected::mark_seen(
                        &silo.path,
                        &kek,
                        &item.source,
                        item.stat,
                    );
                    report.imported += 1;
                }
                Err(_) => report.skipped += 1,
            }
        }

        if report.imported > 0 {
            let _ = app.emit("vault-changed", ());
        }
        Ok(report)
    })
    .await
}

/// Resolves a vault path, creating the folders that are missing.
///
/// A protected folder's tree has to exist inside the silo before its files
/// can land in it, and the first scan finds none of it there.
fn ensure_vault_path(vfs: &Vfs, path: &str) -> Result<Uuid, String> {
    if let Ok(folder) = vfs.folder_by_path(path) {
        return Ok(folder.id);
    }
    let mut current = vfs.root_folder_id().map_err(|e| e.to_string())?;
    let mut walked = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        walked.push('/');
        walked.push_str(segment);
        current = match vfs.folder_by_path(&walked) {
            Ok(folder) => folder.id,
            Err(_) => {
                vfs.create_folder(current, segment)
                    .map_err(|e| e.to_string())?
                    .id
            }
        };
    }
    Ok(current)
}

// ── Room on the disk ────────────────────────────────────────────────

/// What the silo's disk has left, and whether a given write would fit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpaceReport {
    /// `None` when the disk cannot be asked: an unplugged drive, or a
    /// platform this build does not query. Treated everywhere as "no
    /// warning", because inventing a reassuring number would be worse and
    /// inventing an alarming one would cry wolf.
    pub available_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    /// "fine", "tight" or "insufficient", against `wanted_bytes`.
    pub verdict: String,
    pub wanted_bytes: u64,
    /// What the app insists on leaving free beyond the write itself, so the
    /// interface can explain the number rather than assert it.
    pub headroom_bytes: u64,
}

/// Measures a proposed write against the free space where the silo lives.
///
/// `paths` is what is about to be imported, if anything: their sizes on disk
/// are close enough to what the encrypted copies will weigh, since encryption
/// adds a header and a tag per chunk rather than a multiple. Called with an
/// empty list this is simply "how full is the disk", which is what the Health
/// page asks.
#[tauri::command]
pub async fn vault_disk_space(app: AppHandle, paths: Vec<String>) -> Result<SpaceReport, String> {
    // `measure` walks whatever is about to be imported, which for a large
    // folder is a full directory-tree traversal: blocking-pool work, asked
    // for at exactly the moment the user is watching the window.
    run_blocking(move || {
        let silo = crate::state::active_silo(&app)?;
        let wanted: u64 = paths.iter().map(|p| measure(std::path::Path::new(p))).sum();

        let space = silentsilo_shell::disk_space::space_at(&silo.path);
        let verdict = match space {
            Some(space) => match silentsilo_shell::disk_space::verdict(space, wanted) {
                silentsilo_shell::disk_space::SpaceVerdict::Fine => "fine",
                silentsilo_shell::disk_space::SpaceVerdict::Tight => "tight",
                silentsilo_shell::disk_space::SpaceVerdict::Insufficient => "insufficient",
            },
            None => "unknown",
        };

        Ok(SpaceReport {
            available_bytes: space.map(|s| s.available),
            total_bytes: space.map(|s| s.total),
            verdict: verdict.into(),
            wanted_bytes: wanted,
            headroom_bytes: silentsilo_shell::disk_space::HEADROOM_BYTES,
        })
    })
    .await
}

/// Bytes a path would add, walking a folder rather than guessing at it.
///
/// Anything unreadable counts as nothing: this is an estimate used to warn,
/// and refusing to answer because one file in a tree could not be stat'ed
/// would replace a useful warning with none at all.
fn measure(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| measure(&entry.path()))
        .sum::<u64>()
}

// ── Keeping a full copy on this device ──────────────────────────────

/// Whether this device is meant to hold every blob, and how far off it is.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FullCopyStatus {
    pub enabled: bool,
    /// Blobs the index references that are not on this disk. Zero with the
    /// setting on means this device really is a complete copy.
    pub missing: usize,
    pub missing_bytes: i64,
}

/// On the blocking pool: the blob directory is walked whole, which on a silo
/// of any size is thousands of directory entries, and the listing it compares
/// against is read under the sessions lock.
#[tauri::command]
pub async fn full_copy_status(app: AppHandle) -> Result<FullCopyStatus, String> {
    run_blocking(move || full_copy_status_impl(&app)).await
}

fn full_copy_status_impl(app: &AppHandle) -> Result<FullCopyStatus, String> {
    let silo = crate::state::active_silo(app)?;
    // The walk first, with no lock held: it is the long half, and it asks the
    // filesystem rather than the silo.
    let here: HashSet<Uuid> = list_local_blob_ids(&silo.path).into_iter().collect();
    let sizes = {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        Vfs::new(session)
            .list_blob_sizes()
            .map_err(|e| e.to_string())?
    };

    let mut missing = 0usize;
    let mut missing_bytes = 0i64;
    for (blob_id, size) in sizes {
        if !here.contains(&blob_id) {
            missing += 1;
            missing_bytes += size;
        }
    }

    Ok(FullCopyStatus {
        enabled: silentsilo_vault::keep_full_copy(&silo.path),
        missing,
        missing_bytes,
    })
}

/// Turns the full copy on or off for this device.
///
/// Turning it off keeps whatever is already here. It stops promising, it does
/// not start deleting: a setting that emptied the disk when switched would be
/// a trap, and the space is reclaimed by the cache limit in its own time.
#[tauri::command(async)]
pub fn set_full_copy(app: AppHandle, enabled: bool) -> Result<(), String> {
    let silo = crate::state::active_silo(&app)?;
    silentsilo_vault::set_keep_full_copy(&silo.path, enabled).map_err(|e| e.to_string())
}

#[cfg(test)]
mod export_path_tests {
    use super::{safe_join, unwrap_export_key};
    use std::path::Path;

    /// Names come out of the vault index, which this guard treats as data
    /// rather than as something the app controls: a row written by another
    /// build, or by something hostile, must not be able to walk out of the
    /// folder the user chose.
    #[test]
    fn a_name_that_walks_out_of_the_destination_is_refused() {
        let dest = Path::new("C:/Users/alex/Exports");
        for bad in [
            "..",
            ".",
            "",
            "../secrets",
            "sub/dir",
            "sub\\dir",
            // Drive-relative on Windows: `join` would drop the destination.
            "C:evil",
            "C:/Windows/System32/evil.dll",
            "name\u{0}with-nul",
        ] {
            assert!(
                safe_join(dest, bad).is_err(),
                "{bad:?} should have been refused"
            );
        }
    }

    #[test]
    fn an_ordinary_name_lands_inside_the_destination() {
        let dest = Path::new("C:/Users/alex/Exports");
        for good in ["report.pdf", ".gitignore", "Ștampilă 2026.pdf", "a b c"] {
            let joined = safe_join(dest, good).expect("an ordinary name is fine");
            assert!(joined.starts_with(dest), "{good:?} landed at {joined:?}");
            assert!(joined.ends_with(good));
        }
    }

    /// Guessing a key produces an authentication failure that reads as
    /// damaged content, which sends someone looking for a broken disk.
    #[test]
    fn a_row_with_no_content_key_is_refused_rather_than_guessed() {
        let kek = silentsilo_crypto::generate_content_kek();
        let err = unwrap_export_key("", &kek).map(|_| ()).unwrap_err();
        assert!(err.contains("no key recorded"), "{err}");
    }
}

#[cfg(test)]
mod clipboard_and_status_tests {
    use super::{attachment_sizes, clipboard_goes, sealed_passwords_fingerprint};
    use silentsilo_vault::VaultSession;
    use silentsilo_vfs::Vfs;
    use uuid::Uuid;

    #[test]
    fn a_lock_takes_back_only_its_own_silos_secret() {
        let (a, b) = (Uuid::from_bytes([1; 16]), Uuid::from_bytes([2; 16]));
        assert!(clipboard_goes(Some(a), Some(&[a])));
        assert!(
            !clipboard_goes(Some(a), Some(&[b])),
            "another silo's copy stays"
        );
        assert!(
            !clipboard_goes(None, Some(&[])),
            "closing nothing clears nothing"
        );
        assert!(
            clipboard_goes(Some(a), None),
            "locking everything clears it"
        );
        assert!(clipboard_goes(None, Some(&[a])), "an unknown owner goes");
    }

    #[test]
    fn attachments_are_read_again_only_when_an_entry_changes() {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().join("silo"), Uuid::new_v4(), "s").unwrap();
        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().unwrap();
        let before = sealed_passwords_fingerprint(&session).expect("the table reads");

        let id = Uuid::new_v4();
        let blob = Uuid::new_v4();
        let entry = serde_json::json!({
            "id": id.to_string(), "service": "Bank", "username": "a", "password": "p",
            "attachments": [{ "blob_id": blob.to_string(), "name": "x.pdf",
                "size_bytes": 7, "blob_key": "k" }],
        });
        vfs.upsert_password(id, &entry.to_string()).unwrap();
        let after = sealed_passwords_fingerprint(&session).unwrap();
        assert_ne!(before, after, "a new entry changes the fingerprint");
        assert_eq!(attachment_sizes(&session), vec![(blob, 7)]);

        vfs.delete_password(id).unwrap();
        assert!(
            attachment_sizes(&session).is_empty(),
            "a removed entry is not served from the cache"
        );
    }
}

#[cfg(test)]
mod unlock_rule_tests {
    use super::wrapped_dek_for;
    use silentsilo_vault::{StoredFidoCredential, StoredFidoKeys};

    fn key(id: &str, wrapped: &str) -> StoredFidoCredential {
        StoredFidoCredential {
            kind: "fido2".into(),
            derivation: "hmac-secret-v1".into(),
            policy: String::new(),
            credential_id: id.into(),
            public_key: "3059".into(),
            key_slot: 0,
            rp_id: "silentsilo.com".into(),
            label: id.into(),
            wrapped_dek: wrapped.into(),
            platform: false,
            revoked: false,
        }
    }

    #[test]
    fn the_envelope_belongs_to_the_key_that_answered() {
        // Only that credential's envelope unwraps under the key its ceremony
        // produced, so picking any other one fails at the decrypt.
        let keys = StoredFidoKeys {
            keys: vec![key("aa11", "envelope-a"), key("bb22", "envelope-b")],
        };
        assert_eq!(
            wrapped_dek_for(&keys, &hex::decode("bb22").unwrap()).unwrap(),
            "envelope-b"
        );
    }

    #[test]
    fn a_credential_this_device_has_not_seen_falls_back_to_the_first() {
        // Envelopes published by another device carry ids this one may not
        // hold. Trying the first is better than refusing outright here; the
        // decrypt says so plainly a moment later if it is wrong.
        let keys = StoredFidoKeys {
            keys: vec![key("aa11", "envelope-a")],
        };
        assert_eq!(
            wrapped_dek_for(&keys, &hex::decode("cc33").unwrap()).unwrap(),
            "envelope-a"
        );
    }

    #[test]
    fn a_silo_with_no_usable_envelope_says_so() {
        // A row without an envelope cannot unlock anything, and handing an
        // empty string to the unwrap reads as corruption rather than as the
        // missing key it is.
        let keys = StoredFidoKeys {
            keys: vec![key("aa11", "")],
        };
        assert!(wrapped_dek_for(&keys, &hex::decode("aa11").unwrap()).is_err());
        assert!(
            wrapped_dek_for(&StoredFidoKeys { keys: vec![] }, b"whatever").is_err(),
            "a silo with no keys cannot unlock"
        );
    }

    #[test]
    fn a_revoked_key_is_not_offered() {
        let mut revoked = key("aa11", "envelope-a");
        revoked.revoked = true;
        let keys = StoredFidoKeys {
            keys: vec![revoked, key("bb22", "envelope-b")],
        };
        assert_eq!(
            wrapped_dek_for(&keys, &hex::decode("aa11").unwrap()).unwrap(),
            "envelope-b",
            "a retired key must not be what unlocks the silo"
        );
    }
}

/// Writes an export only its owner can read where the system has such a
/// thing: it is in clear. A file already there is made so too.
pub(crate) fn write_owner_only(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_decrypted_copy_is_known_as_long_as_it_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("film.mkv");
        let blob = Uuid::new_v4();
        std::fs::write(&path, b"plain").unwrap();
        silentsilo_vault::seal_readonly(&path);
        let left = copy_of(&path, blob).unwrap();
        assert!(copy_of(&path, blob).as_ref() == Some(&left));
        assert!(
            copy_of(&path, Uuid::new_v4()).as_ref() != Some(&left),
            "another file's content"
        );

        // Read-only, and still removed: Windows refuses that otherwise.
        remove_opened(&path);
        assert!(!path.exists());
        assert!(copy_of(&path, blob).is_none());
        assert_eq!(part_path(&path), dir.path().join("film.mkv.part"));
    }

    use super::*;

    #[test]
    fn the_backstop_waits_past_the_limit_the_window_uses() {
        // 30 minutes, the window's sweep would lock at 1800 s.
        assert!(!past_idle_backstop(1800, None, 30));
        assert!(!past_idle_backstop(
            1800 + IDLE_BACKSTOP_MARGIN_SECS - 1,
            None,
            30
        ));
        assert!(past_idle_backstop(
            1800 + IDLE_BACKSTOP_MARGIN_SECS,
            None,
            30
        ));
    }

    #[test]
    fn the_notice_comes_in_the_last_minute_only() {
        assert!(!locks_within_notice(1739, None, 30));
        assert!(locks_within_notice(1740, None, 30));
        assert!(locks_within_notice(1799, None, 30));
        assert!(
            !locks_within_notice(1800, None, 30),
            "past the limit it is locking, not about to"
        );
        assert!(
            !locks_within_notice(1790, None, 0),
            "a silo that never locks gives no notice"
        );
    }

    #[test]
    fn a_silo_of_its_own_setting_follows_it() {
        let five = 5 * 60 + IDLE_BACKSTOP_MARGIN_SECS;
        assert!(past_idle_backstop(five, Some(5), 360));
        assert!(!past_idle_backstop(five, Some(60), 5));
    }

    #[test]
    fn zero_never_locks_and_an_unset_silo_follows_the_default() {
        assert!(!past_idle_backstop(u64::MAX / 2, None, 0));
        // The registry never stores 0 for a silo; read as "follow the default".
        assert!(past_idle_backstop(
            15 * 60 + IDLE_BACKSTOP_MARGIN_SECS,
            Some(0),
            15
        ));
    }
}

#[cfg(test)]
mod entry_change_tests {
    use super::EntryChange;
    use crate::audit::codes;

    #[test]
    fn each_save_has_its_code_and_an_import_none() {
        let code = |s: &str| {
            serde_json::from_str::<EntryChange>(&format!("\"{s}\""))
                .unwrap()
                .code()
        };
        assert_eq!(code("created"), Some(codes::ENTRY_CREATED));
        assert_eq!(code("edited"), Some(codes::ENTRY_EDITED));
        assert_eq!(code("restored"), Some(codes::ENTRY_RESTORED));
        assert_eq!(code("history_cleared"), Some(codes::HISTORY_CLEARED));
        assert_eq!(code("imported"), None);
        assert_eq!(code("arranged"), None);
    }
}
