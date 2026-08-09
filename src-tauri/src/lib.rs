mod commands;
mod diagnostics;
mod state;

/// The fixture builder's escape hatch must not be in a shipped binary.
/// `seal_under_code_for_fixtures` wraps the DEK under any string, which is
/// the passphrase this design does not have, and Cargo unifies features
/// across a `--workspace` build. Release only: `cargo test --all` builds
/// both crates together in debug on purpose.
#[cfg(not(debug_assertions))]
const _: () = assert!(
    !silentsilo_vault::FIXTURE_SUPPORT,
    "fixture-support is enabled in a release build of the app: build the app on \
     its own rather than with --workspace, or the DEK can be wrapped under an \
     arbitrary string"
);

use std::collections::HashMap;
use std::sync::Mutex;

use commands::shell::{handle_shell_action, show_main_window};
use silentsilo_shell::ensure_os_integration;
use state::{AppState, flush_vault_snapshot};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, RunEvent};

fn setup_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "tray-open", "Open SilentSilo", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let icon = app
        .default_window_icon()
        .ok_or("missing default window icon")?
        .clone();

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("SilentSilo")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "tray-open" => show_main_window(app),
            "tray-quit" => {
                flush_vault_snapshot(app.state::<AppState>());
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything else loads. These policies are inherited and cannot be
    // undone, so applying them first is the whole point: a DLL that gets in
    // ahead of this call has already won.
    silentsilo_shell::harden_process();

    let startup_args: Vec<String> = std::env::args().collect();

    let builder = tauri::Builder::default();

    // Without this, every "Upload/Download from SilentSilo" shell-verb
    // click spawns a new process and re-prompts for the security key
    // instead of forwarding to the running app. Once release-only over a
    // single-instance plugin panic on `tauri dev` relaunch, fixed upstream
    // well before the version in the lockfile.
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        handle_shell_action(app, args);
    }));

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Size, position and maximised state, and nothing else. The plugin
        // also tracks visibility by default, and this window is hidden at
        // startup on purpose: restoring a saved "visible" would pop it open
        // on sign-in, or keep it hidden when the user asked for it.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .manage(commands::hosted::PendingPairing::default())
        .manage(AppState {
            active_silo: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            last_touched: Mutex::new(HashMap::new()),
            import_cancelled: std::sync::atomic::AtomicBool::new(false),
            verify_cancelled: std::sync::atomic::AtomicBool::new(false),
            seed_cancelled: std::sync::atomic::AtomicBool::new(false),
            sync_in_flight: std::sync::atomic::AtomicBool::new(false),
        })
        .setup(move |app| {
            let _ = ensure_os_integration();
            // Before the tray, so a first run that is also the first boot
            // has the Run entry in place whatever happens next.
            let _ = silentsilo_shell::ensure_autostart();
            setup_tray(app.handle())?;
            commands::fido::bind_fido_parent_hwnd(app.handle());
            handle_shell_action(app.handle(), startup_args);
            commands::sync::spawn_auto_sync(app.handle().clone());

            // Locking the workstation and walking away is the common way a
            // silo is left unattended, and the idle timer only notices
            // minutes later. Lock every open silo the moment the OS says the
            // user has gone.
            let lock_handle = app.handle().clone();
            silentsilo_shell::on_user_left(move |_| {
                commands::vault::lock_all_silos(&lock_handle);
            });

            // Clicking the window's own close (X) button hides it instead of
            // quitting — the app keeps running in the tray so shell "Upload to
            // SilentSilo" actions don't cold-start it (and re-prompt for the
            // security key). Real exit goes through the tray's Quit or the OS
            // shutting down (ExitRequested below).
            if let Some(window) = app.get_webview_window("main") {
                let window_to_hide = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_to_hide.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::silo::silo_list,
            commands::silo::silo_create,
            commands::silo::silo_open,
            commands::silo::silo_open_list,
            commands::silo::silo_idle_status,
            commands::silo::silo_set_auto_lock,
            commands::silo::silo_touch,
            commands::silo::silo_blur,
            commands::silo::silo_rename,
            commands::silo::silo_forget,
            commands::silo::silo_add_existing,
            commands::silo::silo_default_location,
            commands::silo::silo_sync_provider_at,
            commands::vault::app_bootstrap,
            commands::vault::vault_unlock,
            commands::vault::vault_lock,
            commands::vault::vault_meta,
            commands::fido::fido_enroll_primary,
            commands::fido::fido_add_key,
            commands::fido::fido_list_keys,
            commands::fido::fido_remove_key,
            commands::fido::fido_rename_key,
            commands::vault::vault_list_folder,
            commands::vault::vault_root_folder,
            commands::vault::vault_get_folder,
            commands::vault::vault_folder_by_path,
            commands::vault::vault_list_all_folders,
            commands::vault::vault_set_favorite,
            commands::vault::vault_list_favorites,
            commands::vault::vault_list_devices,
            commands::vault::vault_activity,
            commands::vault::vault_disk_space,
            commands::vault::full_copy_status,
            commands::vault::set_full_copy,
            commands::vault::protected_folders_list,
            commands::vault::protected_folders_add,
            commands::vault::protected_folders_remove,
            commands::vault::protected_folders_scan,
            commands::vault::vault_set_device_label,
            commands::vault::vault_search,
            commands::shell::shell_upload_queue_pending,
            commands::shell::shell_download_queue_pending,
            commands::shell::clipboard_file_paths,
            commands::shell::autostart_status,
            commands::shell::autostart_set,
            commands::vault::vault_import_files_to_folder,
            commands::vault::vault_paste_paths,
            commands::vault::cancel_import,
            commands::vault::reset_import_cancel,
            commands::vault::vault_create_folder,
            commands::vault::vault_rename_file,
            commands::vault::vault_rename_folder,
            commands::vault::vault_trash_file,
            commands::vault::vault_trash_folder,
            commands::vault::vault_list_trash,
            commands::vault::vault_restore_file,
            commands::vault::vault_restore_folder,
            commands::vault::vault_empty_trash,
            commands::vault::vault_purge_items,
            commands::vault::vault_import_file,
            commands::vault::vault_import_folder,
            commands::vault::vault_export_file,
            commands::vault::vault_open_file,
            commands::vault::vault_export_folder,
            commands::vault::vault_read_passwords,
            commands::vault::vault_upsert_password,
            commands::vault::vault_delete_password,
            commands::vault::fido_reverify,
            commands::vault::ssh_generate_keypair,
            commands::vault::password_attach_file,
            commands::vault::password_open_attachment,
            commands::vault::password_delete_attachment,
            commands::vault::copy_secret_to_clipboard,
            commands::vault::passwords_read_import_csv,
            commands::vault::passwords_write_export_csv,
            commands::vault::vault_blob_status,
            commands::hosted::hosted_pair_start,
            commands::hosted::hosted_pair_poll,
            commands::hosted::hosted_pair_confirm,
            commands::hosted::hosted_pair_cancel,
            commands::hosted::hosted_portal_url,
            commands::hosted::hosted_usage,
            commands::storage::s3_get_config,
            commands::storage::s3_save_config,
            commands::storage::s3_test_config,
            commands::storage::s3_disconnect,
            commands::storage::backup_targets_list,
            commands::storage::backup_target_add,
            commands::storage::backup_target_remove,
            commands::storage::backup_target_protection,
            commands::storage::backup_target_seed,
            commands::storage::cancel_seed,
            commands::sync::cancel_verify,
            commands::fido::vault_rotate_key,
            commands::fido::vault_rotation_pending,
            commands::fido::vault_rotate_resume,
            commands::sync::vault_verify,
            commands::sync::vault_test_restore,
            commands::storage::sftp_probe_host_key,
            commands::sync::sync_status,
            commands::sync::sync_now,
            commands::sync::sync_fetch_blob,
            commands::sync::vault_rebuild_from_snapshot,
            commands::sync::vault_preview_join,
            commands::sync::vault_join_from_storage,
            commands::recovery::recovery_status,
            commands::recovery::recovery_generate,
            commands::recovery::recovery_disable,
            commands::recovery::vault_unlock_with_recovery,
            commands::recovery::vault_join_with_recovery,
        ])
        .build(tauri::generate_context!())
        .expect("error while running SilentSilo")
        .run(|app_handle, event| {
            if matches!(event, RunEvent::ExitRequested { .. }) {
                let state = app_handle.state::<AppState>();
                flush_vault_snapshot(state.clone());
                // Every silo that is open, not just the one on screen: each
                // has a decrypted working copy, and leaving any of them
                // behind is the thing this exists to prevent.
                for id in state.open_silo_ids() {
                    let _ = state.close_session(id);
                }
            }
        });
}
