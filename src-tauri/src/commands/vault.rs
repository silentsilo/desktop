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

use crate::commands::fido::{emit_fido_progress, run_blocking, run_fido};
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
}

#[tauri::command]
pub fn app_bootstrap(app: AppHandle, state: State<AppState>) -> Result<AppBootstrap, String> {
    // "Provisioned" is now per-silo: the app can know about several and have
    // none of them open, which is the state the picker exists for.
    let silo = crate::state::active_silo(&app).ok();
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
        return Err(format!("not a file: {}", source.display()));
    }

    let file_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid file name".to_string())?
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
        size_bytes: source.metadata().map_err(|e| e.to_string())?.len() as i64,
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
}

fn emit_import_progress(app: &AppHandle, progress: ImportProgress) {
    let _ = app.emit("import-progress", progress);
}

#[tauri::command]
pub fn cancel_import(state: State<AppState>) {
    state.import_cancelled.store(true, Ordering::Relaxed);
}

/// Clears any previous cancellation before starting a new cancellable batch
/// (folder import, paste). Separate from `cancel_import` so nested calls —
/// e.g. `vault_paste_paths` importing a folder as one of its items — don't
/// clobber a cancellation the outer call is still checking for.
#[tauri::command]
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

fn import_folder_recursive(
    app: &AppHandle,
    snapshot: &crate::state::SessionSnapshot,
    parent_folder_id: Uuid,
    source_dir: &std::path::Path,
    visited: &mut HashSet<std::path::PathBuf>,
    counters: &mut (u32, u32),
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    let canonical = std::fs::canonicalize(source_dir).unwrap_or_else(|_| source_dir.to_path_buf());
    if !visited.insert(canonical) {
        return Ok(());
    }

    let entries = std::fs::read_dir(source_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        if cancelled.load(Ordering::Relaxed) {
            return Err(IMPORT_CANCELLED.to_string());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
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
            counters.0 = counters.0.saturating_add(1);
            emit_import_progress(
                app,
                ImportProgress {
                    phase: "encrypting".into(),
                    current: counters.0,
                    total: counters.1,
                    name: display_name,
                },
            );
            // Keep going if a single file fails (locked/system files, etc.).
            if let Err(e) = import_one(app, snapshot, parent_folder_id, &path) {
                crate::diagnostics::warn(
                    "import",
                    format_args!("skipped a file ({}): {e}", path.display()),
                );
            }
        } else if meta.is_dir() {
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            emit_import_progress(
                app,
                ImportProgress {
                    phase: "folders".into(),
                    current: counters.0,
                    total: counters.1,
                    name: display_name,
                },
            );
            // A short lock for the row; the descent itself holds nothing.
            let created =
                crate::state::with_session_id(&app.state::<AppState>(), snapshot.id, |_s, vfs| {
                    vfs.create_folder(parent_folder_id, dir_name)
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
                    crate::diagnostics::warn(
                        "import",
                        format_args!("skipped a folder ({}): {e}", path.display()),
                    );
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
fn import_folder_impl(app: &AppHandle, folder_id: Uuid, source: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!("not a directory: {}", source.display()));
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
        },
    );

    let state = app.state::<AppState>();
    let snapshot = crate::state::snapshot_focused_session(&state)?;

    let dir_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid dir name".to_string())?;

    let top_folder = crate::state::with_session_id(&state, snapshot.id, |_s, vfs| {
        vfs.create_folder(folder_id, dir_name)
    })?;

    let mut visited = HashSet::new();
    let mut counters = (0u32, total_files);
    import_folder_recursive(
        app,
        &snapshot,
        top_folder.id,
        source,
        &mut visited,
        &mut counters,
        &state.import_cancelled,
    )?;

    emit_import_progress(
        app,
        ImportProgress {
            phase: "done".into(),
            current: counters.0,
            total: total_files,
            name: dir_name.to_string(),
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn vault_import_folder(
    app: AppHandle,
    folder_id: String,
    source_path: String,
) -> Result<(), String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let source = PathBuf::from(&source_path);
    run_blocking(move || import_folder_impl(&app, folder_id, &source)).await
}

pub(crate) fn process_shell_upload_queue(app: &AppHandle) -> Result<u32, String> {
    let paths = silentsilo_shell::drain_upload_queue().map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Ok(0);
    }

    let state = app.state::<AppState>();
    let snapshot = crate::state::snapshot_focused_session(&state)?;
    let inbox_id = crate::state::with_session_id(&state, snapshot.id, |_s, vfs| {
        vfs.inbox_folder().map(|f| f.id)
    })?;

    let mut imported = 0u32;
    for path in paths {
        let source = PathBuf::from(&path);
        if import_one(app, &snapshot, inbox_id, &source).is_ok() {
            imported += 1;
        }
    }
    Ok(imported)
}

#[tauri::command]
pub async fn vault_unlock(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<VaultMeta, String> {
    let creds = crate::state::silo_credentials(&app)?;
    let root = vault_dir(&app)?;

    if !is_fido_enrolled(&root) {
        return Err("Enroll a security key before unlocking".into());
    }

    let keys = silentsilo_vault::load_fido_keys(&root).map_err(|e| e.to_string())?;
    let cred_ids = keys.credential_ids_bytes().map_err(|e| e.to_string())?;
    let vault_id = creds.vault_id.to_string();

    emit_fido_progress(&app, "Touch any enrolled security key to unlock the silo.");
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id, None)
    })
    .await?;

    let wrapped_dek = keys
        .find_by_credential_id(&unlock.credential_id)
        .or_else(|| keys.primary())
        .ok_or_else(|| "No matching enrolled security key".to_string())?
        .wrapped_dek
        .clone();

    // Opening the session restores the working copy from its snapshot when
    // one is newer, which on a large silo is real disk work: blocking-pool
    // territory, not something to run on an async worker.
    run_blocking(move || {
        let session =
            VaultSession::open_with_fido_wrapped(root.clone(), &unlock.wrap_key, &wrapped_dek)
                .map_err(|e| e.to_string())?;

        if session.vault_id != creds.vault_id {
            return Err("these credentials belong to a different silo".into());
        }

        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().map_err(|e| e.to_string())?;
        let meta = vfs.meta().map_err(|e| e.to_string())?;

        // A crash between opening a file and locking would have left plaintext
        // behind; this is the first moment it is safe to clear it.
        wipe_open_scratch(&session.paths.root);

        crate::state::open_focused_session(&app, session)?;
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
        take_back_clipboard(&app);
        let state = app.state::<AppState>();
        let ids = match id {
            Some(id) => vec![Uuid::parse_str(&id).map_err(|e| e.to_string())?],
            None => state.open_silo_ids(),
        };
        // Only ciphertext should remain on disk for each of these once this
        // returns; `close_session` snapshots, drops the connection and then
        // clears the working copy, in that order.
        for id in ids {
            state.close_session(id)?;
        }
        Ok(())
    })
    .await
}

/// Locks every open silo, used when the workstation locks or suspends:
/// whoever walks up next should find nothing open. Failures are swallowed;
/// a silo that cannot close right now is closed by its idle timer soon
/// after.
pub fn lock_all_silos(app: &AppHandle) {
    take_back_clipboard(app);
    let state = app.state::<AppState>();
    for id in state.open_silo_ids() {
        let _ = state.close_session(id);
    }
    let _ = app.emit("silos-locked", ());
}

/// The focused silo's metadata, for a silo that is already unlocked, so
/// switching to it does not ask again for a key already presented.
#[tauri::command]
pub fn vault_meta(state: State<AppState>) -> Result<VaultMeta, String> {
    with_vfs(&state, |_session, vfs| vfs.meta())
}

#[tauri::command]
pub fn vault_list_folder(
    folder_id: String,
    state: State<AppState>,
) -> Result<Vec<VaultEntry>, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.list_folder(folder_id))
}

#[tauri::command]
pub fn vault_root_folder(state: State<AppState>) -> Result<FolderEntry, String> {
    with_vfs(&state, |_session, vfs| {
        let id = vfs.root_folder_id()?;
        vfs.get_folder(id)
    })
}

#[tauri::command]
pub fn vault_get_folder(folder_id: String, state: State<AppState>) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.get_folder(folder_id))
}

#[tauri::command]
pub fn vault_folder_by_path(path: String, state: State<AppState>) -> Result<FolderEntry, String> {
    with_vfs(&state, |_session, vfs| vfs.folder_by_path(&path))
}

/// Names only, across the whole vault.
///
/// Capped rather than paginated: nobody scrolls past fifty results, and a
/// query matching thousands means the user should type more, not that the
/// UI should render them all.
#[tauri::command]
pub fn vault_search(query: String, state: State<AppState>) -> Result<Vec<SearchHit>, String> {
    with_vfs(&state, |_, vfs| vfs.search_entries(&query, 50))
}

#[tauri::command]
pub fn vault_list_all_folders(state: State<AppState>) -> Result<Vec<FolderEntry>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_all_folders())
}

/// Stars or unstars one entry. `kind` is what the explorer already knows
/// about the row, rather than something to be guessed by looking the id up
/// in both tables.
#[tauri::command]
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

#[tauri::command]
pub fn vault_list_favorites(state: State<AppState>) -> Result<Vec<SearchHit>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_favorites())
}

#[tauri::command]
pub fn vault_list_devices(state: State<AppState>) -> Result<Vec<DeviceInfo>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_devices())
}

/// What has happened to this silo, newest first.
///
/// Read straight from the local operation log, so it costs one query and
/// nothing is stored for it. See `silentsilo_vfs::activity` for what this is
/// and, more importantly, what it is not.
#[tauri::command]
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

#[tauri::command]
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
            let source = PathBuf::from(&path);
            imported.push(import_one(&app, &snapshot, folder_id, &source)?);
        }
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

            if source.is_dir() {
                match import_folder_impl(&app, folder_uuid, &source) {
                    Ok(()) => result.imported_folders += 1,
                    // A cancelled sub-folder import means cancellation was
                    // requested — stop the whole paste rather than recording the
                    // folder as a failure.
                    Err(e) if e == IMPORT_CANCELLED => break,
                    Err(e) => result.failed.push(format!("{name}: {e}")),
                }
            } else if source.is_file() {
                match import_one(&app, &snapshot, folder_uuid, &source) {
                    Ok(_) => result.imported_files += 1,
                    Err(e) => result.failed.push(format!("{name}: {e}")),
                }
            }
            // Neither a file nor a directory anymore (e.g. moved/deleted since
            // it was copied) — silently skipped, same as a no-op paste of it.
        }

        Ok(result)
    })
    .await
}

#[tauri::command]
pub fn vault_create_folder(
    parent_id: String,
    name: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let parent_id = Uuid::parse_str(&parent_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.create_folder(parent_id, &name))
}

#[tauri::command]
pub fn vault_rename_file(
    file_id: String,
    new_name: String,
    state: State<AppState>,
) -> Result<FileEntry, String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.rename_file(file_id, &new_name))
}

#[tauri::command]
pub fn vault_rename_folder(
    folder_id: String,
    new_name: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| {
        vfs.rename_folder(folder_id, &new_name)
    })
}

#[tauri::command]
pub fn vault_trash_file(file_id: String, state: State<AppState>) -> Result<(), String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.trash_file(file_id))
}

#[tauri::command]
pub fn vault_trash_folder(folder_id: String, state: State<AppState>) -> Result<(), String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.trash_folder(folder_id))
}

#[tauri::command]
pub fn vault_list_trash(state: State<AppState>) -> Result<Vec<TrashItem>, String> {
    with_vfs(&state, |_session, vfs| vfs.list_trash())
}

#[tauri::command]
pub fn vault_restore_file(file_id: String, state: State<AppState>) -> Result<FileEntry, String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.restore_file(file_id))
}

#[tauri::command]
pub fn vault_restore_folder(
    folder_id: String,
    state: State<AppState>,
) -> Result<FolderEntry, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| vfs.restore_folder(folder_id))
}

/// Permanently removes trashed items from the local index, then best-effort
/// deletes their blobs from local disk cache and cloud storage. Blob IDs are
/// never shared between files (each upload gets a fresh `Uuid::new_v4`), so
/// deleting them here can never affect another live file. Cloud/local blob
/// cleanup failures are logged but don't fail the command — the local index
/// change (what the user sees in Trash) already succeeded and is the part
/// that must be atomic; a stray orphaned blob can be cleaned up later.
#[tauri::command]
pub async fn vault_empty_trash(app: AppHandle) -> Result<u64, String> {
    let app2 = app.clone();
    let (removed, blob_ids, root) = run_blocking(move || {
        let state = app2.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let (removed, blob_ids) = Vfs::new(session).empty_trash().map_err(|e| e.to_string())?;
        Ok((removed, blob_ids, session.paths.root.clone()))
    })
    .await?;

    // The bucket copy too, best-effort: the index change already committed
    // and an orphaned object costs storage, not correctness. Append-only
    // targets are skipped rather than asked and refused.
    for target in crate::state::silo_targets(&app) {
        if !target.role.allows_delete() {
            continue;
        }
        for blob_id in &blob_ids {
            if let Err(e) = silentsilo_sync::delete_blob(&*target.store, *blob_id).await {
                crate::diagnostics::warn(
                    "purge",
                    format_args!("blob {blob_id} left in storage: {e}"),
                );
            }
        }
    }

    let _ = run_blocking(move || {
        for blob_id in &blob_ids {
            if let Err(e) = silentsilo_vault::remove_blob_from_cache(&root, *blob_id) {
                crate::diagnostics::warn(
                    "purge",
                    format_args!("blob {blob_id} left in the local cache: {e}"),
                );
            }
        }
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
#[tauri::command]
pub async fn vault_purge_items(app: AppHandle, ids: Vec<String>) -> Result<u64, String> {
    let ids: Vec<Uuid> = ids
        .iter()
        .map(|id| Uuid::parse_str(id).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;

    let app2 = app.clone();
    let (removed, blob_ids, root) = run_blocking(move || {
        let state = app2.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let (removed, blob_ids) = Vfs::new(session)
            .purge_items(&ids)
            .map_err(|e| e.to_string())?;
        Ok((removed, blob_ids, session.paths.root.clone()))
    })
    .await?;

    // Best-effort, as when emptying the whole trash: the index change
    // already committed. Append-only targets are skipped rather than asked
    // and refused.
    for target in crate::state::silo_targets(&app) {
        if !target.role.allows_delete() {
            continue;
        }
        for blob_id in &blob_ids {
            if let Err(e) = silentsilo_sync::delete_blob(&*target.store, *blob_id).await {
                crate::diagnostics::warn(
                    "purge",
                    format_args!("blob {blob_id} left in storage: {e}"),
                );
            }
        }
    }

    let _ = run_blocking(move || {
        for blob_id in &blob_ids {
            if let Err(e) = silentsilo_vault::remove_blob_from_cache(&root, *blob_id) {
                crate::diagnostics::warn(
                    "purge",
                    format_args!("blob {blob_id} left in the local cache: {e}"),
                );
            }
        }
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
        import_one(&app, &snapshot, folder_id, &source)
    })
    .await
}

/// Downloads any of `blob_ids` this device doesn't hold.
///
/// Export is the one place a missing blob is fatal rather than cosmetic: the
/// user asked for the bytes, and there is no partial answer. A vault with no
/// storage connected gets a message about *that*, not a decryption failure.
async fn ensure_blobs_local(app: &AppHandle, blob_ids: &[Uuid]) -> Result<(), String> {
    let root = vault_dir(app)?;
    let missing: Vec<Uuid> = blob_ids
        .iter()
        .copied()
        .filter(|id| !root.join("blobs").join(format!("{id}.sslo")).is_file())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let Some(store) = crate::state::silo_store(app) else {
        return Err(
            "Some of this content isn't on this device, and no backup storage is connected.".into(),
        );
    };
    for id in missing {
        silentsilo_sync::fetch_blob(&*store, &root, id)
            .await
            .map_err(|e| format!("could not download the file content: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_export_file(
    app: AppHandle,
    file_id: String,
    dest_path: String,
) -> Result<(), String> {
    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    let dest = PathBuf::from(&dest_path);

    // The row and its wrapped key under one short lock, before the network
    // and before the decrypt: neither of those may hold the sessions mutex,
    // or every small command in the app queues behind a large file.
    let (blob_id, wrapped_key) = {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let vfs = Vfs::new(session);
        let file = vfs.get_file(file_id).map_err(|e| e.to_string())?;
        let wrapped = vfs.blob_key(file_id).map_err(|e| e.to_string())?;
        (file.blob_id, wrapped)
    };
    ensure_blobs_local(&app, &[blob_id]).await?;

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        let key = unwrap_export_key(&wrapped_key, &snapshot.kek)?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string())?;
        let _ = touch_blob_access(&snapshot.root, blob_id);
        Ok(())
    })
    .await
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
        return Err("This file has no content key recorded, so it cannot be opened.".into());
    }
    silentsilo_crypto::unwrap_content_key(wrapped, kek)
        .map_err(|_| "This content's key could not be read, so it cannot be opened.".to_string())
}

/// Recursively decrypt a folder (and all its subfolders/files) into
/// `dest_dir`, downloading any cloud-only blobs on demand. The folder
/// itself is recreated as a subdirectory of `dest_dir` (matching normal
/// "download folder" behavior — you pick a destination, not the final
/// path). Returns the number of files exported.
#[tauri::command]
pub async fn vault_export_folder(
    app: AppHandle,
    folder_id: String,
    dest_dir: String,
) -> Result<u32, String> {
    let folder_id = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;

    // The whole subtree as a plan — destination paths, blob ids, wrapped
    // keys — read under one short lock at database speed. The download and
    // the decrypts run against the plan, holding nothing, so listing a
    // folder while a large export runs stays instant.
    let plan = {
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
        plan
    };

    let blob_ids: Vec<Uuid> = plan
        .iter()
        .filter_map(|item| match item {
            ExportItem::File { blob_id, .. } => Some(*blob_id),
            ExportItem::Dir(_) => None,
        })
        .collect();
    ensure_blobs_local(&app, &blob_ids).await?;

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
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
                } => {
                    let key = unwrap_export_key(wrapped_key, &snapshot.kek)?;
                    decrypt_blob(&paths.blob_path(*blob_id), dest, &key, *blob_id)
                        .map_err(|e| e.to_string())?;
                    let _ = touch_blob_access(&snapshot.root, *blob_id);
                    exported += 1;
                    let _ = app.emit("export-progress", exported);
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
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(format!("refusing to export unsafe entry name: {name:?}"));
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
#[tauri::command]
pub async fn vault_open_file(app: AppHandle, file_id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let file_id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;

    let (blob_id, name, wrapped_key) = {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        let session = session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        let vfs = Vfs::new(session);
        let file = vfs.get_file(file_id).map_err(|e| e.to_string())?;
        // Read here, while the session is in hand, because the decrypt below
        // runs on the blocking pool with only what it was given.
        let wrapped = vfs.blob_key(file_id).map_err(|e| e.to_string())?;
        (file.blob_id, file.name, wrapped)
    };
    ensure_blobs_local(&app, &[blob_id]).await?;

    let app2 = app.clone();
    let path = run_blocking(move || {
        // The keys under a lock held for the copy; the decrypt of an
        // arbitrarily large file holds nothing, so the rest of the app keeps
        // answering while it runs.
        let snapshot = crate::state::snapshot_focused_session(&app2.state::<AppState>())?;

        let dir = open_scratch_dir(&snapshot.root);
        silentsilo_vault::create_private_dir(&dir).map_err(|e| e.to_string())?;
        // The original name, so the OS picks the right application and the
        // title bar says something the user recognises.
        let dest = safe_join(&dir, &name)?;
        let _ = std::fs::remove_file(&dest);

        let key =
            silentsilo_crypto::unwrap_content_key(&wrapped_key, &snapshot.kek).map_err(|_| {
                "This content's key could not be read, so it cannot be opened.".to_string()
            })?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string())?;

        silentsilo_vault::seal_readonly(&dest);
        let _ = touch_blob_access(&snapshot.root, blob_id);
        Ok(dest)
    })
    .await?;

    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Every password entry, as a JSON array.
///
/// Entries live as rows in the encrypted index rather than as a file in the
/// tree. Nothing decrypted touches the disk on this path: the rows come
/// straight out of the open database.
#[tauri::command]
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

/// Creates or replaces one entry, keyed by the id the panel generated.
#[tauri::command]
pub fn vault_upsert_password(
    id: String,
    json: String,
    state: State<AppState>,
) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("invalid entry id: {e}"))?;

    // Parsed to reject anything that would not survive a round trip, and to
    // make sure the id in the record matches the one being written. A row
    // whose body disagrees with its key is the kind of thing that only
    // surfaces on another device, months later.
    let parsed: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("invalid JSON: {e}"))?;
    match parsed.get("id").and_then(|v| v.as_str()) {
        Some(inner) if inner == id.to_string() => {}
        Some(_) => return Err("entry id does not match the record".into()),
        None => return Err("entry has no id".into()),
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
#[tauri::command]
pub fn copy_secret_to_clipboard(app: AppHandle, text: String) -> Result<(), String> {
    silentsilo_shell::set_secret_clipboard(&text)?;

    // Cleared only if it is still ours: by the time this fires the user has
    // often copied something else, and wiping that would be the app reaching
    // into something that stopped being its business.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(
            silentsilo_shell::SECRET_CLIPBOARD_TTL_SECS,
        ));
        if silentsilo_shell::clear_expired_secret(&text) {
            let _ = app.emit("clipboard-cleared", ());
        }
    });
    Ok(())
}

/// Takes back a copied secret when a silo closes: a password copied a
/// moment before the lock would otherwise stay readable for the rest of
/// its forty-five seconds.
fn take_back_clipboard(app: &AppHandle) {
    if silentsilo_shell::clear_secret_clipboard_now() {
        let _ = app.emit("clipboard-cleared", ());
    }
}

/// Removes one entry outright. There is no trash for logins.
#[tauri::command]
pub fn vault_delete_password(id: String, state: State<AppState>) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("invalid entry id: {e}"))?;
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
/// drawer. Used by entries marked "ask again before revealing".
#[tauri::command]
pub async fn fido_reverify(app: AppHandle) -> Result<(), String> {
    let root = vault_dir(&app)?;
    let creds = crate::state::silo_credentials(&app)?;

    if !is_fido_enrolled(&root) {
        return Err("No security key is enrolled on this silo.".into());
    }

    let keys = silentsilo_vault::load_fido_keys(&root).map_err(|e| e.to_string())?;
    let cred_ids = keys.credential_ids_bytes().map_err(|e| e.to_string())?;
    let vault_id = creds.vault_id.to_string();

    emit_fido_progress(&app, "Touch your security key to show this entry.");
    let unlock = run_fido(&app, move || {
        silentsilo_fido::derive_unlock_material(&cred_ids, &vault_id, None)
    })
    .await?;

    let stored = keys
        .find_by_credential_id(&unlock.credential_id)
        .or_else(|| keys.primary())
        .ok_or_else(|| "That security key isn't enrolled on this silo.".to_string())?;

    silentsilo_vault::unwrap_dek_hex(&stored.wrapped_dek, &unlock.wrap_key)
        .map(|_| ())
        .map_err(|_| "That security key could not verify this silo.".to_string())
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
#[tauri::command]
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
        return Err("Not a file.".into());
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
            .ok_or_else(|| "invalid file name".to_string())?
            .to_string();
        let size_bytes = source.metadata().map_err(|e| e.to_string())?.len() as i64;

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
            size_bytes,
            blob_key,
        })
    })
    .await
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
        // The name comes out of entry JSON, which is user data: joined
        // through the same guard as every export, so a traversal-shaped
        // name cannot walk out of the scratch directory.
        let dest = safe_join(&dir, &name)?;
        let _ = std::fs::remove_file(&dest);

        let key =
            silentsilo_crypto::unwrap_content_key(&blob_key, &snapshot.kek).map_err(|_| {
                "This content's key could not be read, so it cannot be opened.".to_string()
            })?;
        let blob_path = silentsilo_vault::VaultPaths::new(snapshot.root.clone()).blob_path(blob_id);
        decrypt_blob(&blob_path, &dest, &key, blob_id).map_err(|e| e.to_string())?;

        silentsilo_vault::seal_readonly(&dest);
        let _ = touch_blob_access(&snapshot.root, blob_id);
        Ok(dest)
    })
    .await?;

    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Removes an attachment's content, locally and from the backup.
///
/// Best-effort on the remote side for the same reason emptying the trash
/// is: an orphaned object costs storage, while failing the removal would
/// leave the entry claiming a file that the user asked to be gone.
#[tauri::command]
pub async fn password_delete_attachment(app: AppHandle, blob_id: String) -> Result<(), String> {
    let blob_id = Uuid::parse_str(&blob_id).map_err(|e| e.to_string())?;
    let root = vault_dir(&app)?;
    silentsilo_vault::remove_blob_from_cache(&root, blob_id).map_err(|e| e.to_string())?;
    for target in crate::state::silo_targets(&app) {
        if target.role.allows_delete() {
            let _ = silentsilo_sync::delete_blob(&*target.store, blob_id).await;
        }
    }
    Ok(())
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
            return Err("Not a file.".into());
        }
        if meta.len() > MAX_IMPORT_CSV_BYTES {
            return Err("That file is too large to be a password export.".into());
        }

        std::fs::read_to_string(&path)
            .map_err(|_| "Couldn't read that file as text. Is it really a CSV?".to_string())
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
) -> Result<(), String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        let session_guard = state.focused_session()?;
        session_guard
            .as_ref()
            .ok_or_else(|| CoreError::VaultLocked.to_string())?;
        drop(session_guard);

        let path = PathBuf::from(path);

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| e.to_string())?;
            file.write_all(contents.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        #[cfg(not(unix))]
        std::fs::write(&path, contents.as_bytes()).map_err(|e| e.to_string())?;

        Ok(())
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
    usage: silentsilo_vault::CacheUsage,
}

#[tauri::command]
pub async fn vault_blob_status(app: AppHandle) -> Result<BlobStatus, String> {
    run_blocking(move || {
        let state = app.state::<AppState>();
        // The database's half under a short lock; the blobs directory —
        // thousands of entries on a full-copy silo, and this command runs on
        // a 20-second timer — is walked with no lock held at all.
        let (root, sizes, password_rows) = {
            let session_guard = state.focused_session()?;
            let session = session_guard
                .as_ref()
                .ok_or_else(|| CoreError::VaultLocked.to_string())?;
            (
                session.paths.root.clone(),
                Vfs::new(session)
                    .list_blob_sizes()
                    .map_err(|e| e.to_string())?,
                Vfs::new(session).list_passwords().unwrap_or_default(),
            )
        };

        let local = list_local_blob_ids(&root);
        let here: HashSet<Uuid> = local.iter().copied().collect();

        let mut missing = Vec::new();
        let mut missing_bytes = 0i64;
        for (blob_id, size) in sizes {
            if !here.contains(&blob_id) {
                missing.push(blob_id.to_string());
                missing_bytes += size;
            }
        }

        // Password attachments hold blobs the file tree never references, so a
        // recovery that only counted the tree would leave them behind. This is
        // the one place the backend looks inside entry JSON, and it looks for
        // exactly one field: a row that does not parse, or has no attachments,
        // contributes nothing and fails nothing.
        for raw in password_rows {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(attachments) = value.get("attachments").and_then(|a| a.as_array()) else {
                continue;
            };
            for attachment in attachments {
                let Some(blob_id) = attachment
                    .get("blob_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                else {
                    continue;
                };
                if !here.contains(&blob_id) && !missing.contains(&blob_id.to_string()) {
                    missing.push(blob_id.to_string());
                    missing_bytes += attachment
                        .get("size_bytes")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                }
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
            usage: silentsilo_vault::cache_usage(&root),
        })
    })
    .await
}

// ── Protected folders ───────────────────────────────────────────────

/// What one scan managed.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ProtectedScanReport {
    pub imported: usize,
    /// Files that could not be read or encrypted. Counted rather than
    /// aborted: a locked file in the middle of a folder should not cost the
    /// user the other nine hundred.
    pub skipped: usize,
}

#[tauri::command]
pub fn protected_folders_list(app: AppHandle) -> Result<Vec<ProtectedFolderView>, String> {
    let silo = crate::state::active_silo(&app)?;
    Ok(silentsilo_vault::load_protected(&silo.path)
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
#[tauri::command]
pub fn protected_folders_add(app: AppHandle, path: String) -> Result<(), String> {
    let silo = crate::state::active_silo(&app)?;
    let source = PathBuf::from(&path);
    if !source.is_dir() {
        return Err("That is not a folder on this computer.".into());
    }

    let mut list = silentsilo_vault::load_protected(&silo.path).map_err(|e| e.to_string())?;
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
    silentsilo_vault::save_protected(&silo.path, &list).map_err(|e| e.to_string())
}

/// Stops keeping a copy. What was already imported stays: this is an archive,
/// and removing a folder from the list is not a request to delete anything.
#[tauri::command]
pub fn protected_folders_remove(app: AppHandle, path: String) -> Result<(), String> {
    let silo = crate::state::active_silo(&app)?;
    let mut list = silentsilo_vault::load_protected(&silo.path).map_err(|e| e.to_string())?;
    let removing = Path::new(&path);
    list.folders.retain(|f| f.path != removing);
    silentsilo_vault::save_protected(&silo.path, &list).map_err(|e| e.to_string())
}

/// Walks every protected folder and imports what has changed.
///
/// Run on demand and after unlocking. Each file is marked as taken only once
/// it is in the silo, so an interrupted scan keeps what it managed and
/// retries the rest next time.
#[tauri::command]
pub async fn protected_folders_scan(app: AppHandle) -> Result<ProtectedScanReport, String> {
    run_blocking(move || {
        let silo = crate::state::active_silo(&app)?;
        let list = silentsilo_vault::load_protected(&silo.path).map_err(|e| e.to_string())?;
        if list.folders.is_empty() {
            return Ok(ProtectedScanReport::default());
        }

        let seen = silentsilo_vault::protected::load_seen(&silo.path).map_err(|e| e.to_string())?;
        let state = app.state::<AppState>();
        let snapshot = crate::state::snapshot_focused_session(&state)?;

        let mut report = ProtectedScanReport::default();
        for folder in &list.folders {
            // The walk itself reads the user's folders, never the silo, so
            // it holds no lock; only resolving a target path and committing
            // a row do, briefly, per file.
            let pending = silentsilo_vault::plan_scan(&folder.path, &folder.target, &seen)
                .map_err(|e| e.to_string())?;

            for item in pending {
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
                            &item.source,
                            item.stat,
                        );
                        report.imported += 1;
                    }
                    Err(_) => report.skipped += 1,
                }
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

#[tauri::command]
pub fn full_copy_status(app: AppHandle, state: State<AppState>) -> Result<FullCopyStatus, String> {
    let silo = crate::state::active_silo(&app)?;
    let session_guard = state.focused_session()?;
    let session = session_guard
        .as_ref()
        .ok_or_else(|| CoreError::VaultLocked.to_string())?;

    let here: HashSet<Uuid> = list_local_blob_ids(&silo.path).into_iter().collect();
    let mut missing = 0usize;
    let mut missing_bytes = 0i64;
    for (blob_id, size) in Vfs::new(session)
        .list_blob_sizes()
        .map_err(|e| e.to_string())?
    {
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
#[tauri::command]
pub fn set_full_copy(app: AppHandle, enabled: bool) -> Result<(), String> {
    let silo = crate::state::active_silo(&app)?;
    silentsilo_vault::set_keep_full_copy(&silo.path, enabled).map_err(|e| e.to_string())
}
