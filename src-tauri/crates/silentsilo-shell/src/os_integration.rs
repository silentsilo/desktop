use std::path::{Path, PathBuf};

const INTEGRATION_FLAG: &str = "shell-integration.json";

/// Bump whenever the registered context-menu entries change shape, so an
/// existing install re-registers from a clean slate instead of mixing old
/// and new entries.
const MENU_VERSION: u32 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct IntegrationState {
    context_menu: bool,
    exe_path: String,
    #[serde(default)]
    menu_version: u32,
}

pub fn register_os_integration(exe_path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // Clear out whatever a previous version registered first — old and
        // new entries can otherwise coexist (different registry key names).
        let _ = crate::windows::unregister_context_menu();
        crate::windows::register_context_menu(exe_path)?;
        save_state(exe_path, true)?;
    }
    #[cfg(target_os = "macos")]
    {
        // Same clean slate as the Windows path above: a workflow left by an
        // earlier version keeps its old command until something removes it.
        let _ = crate::macos::unregister_context_menu();
        crate::macos::register_context_menu(exe_path)?;
        save_state(exe_path, true)?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = exe_path;
        save_state(exe_path, false)?;
    }
    Ok(())
}

pub fn ensure_os_integration() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    if integration_current(&exe)? {
        return Ok(());
    }
    register_os_integration(&exe)
}

fn integration_current(exe: &Path) -> std::io::Result<bool> {
    let flag = flag_path();
    if !flag.is_file() {
        return Ok(false);
    }
    let json = std::fs::read_to_string(flag)?;
    let state: IntegrationState = serde_json::from_str(&json).unwrap_or(IntegrationState {
        context_menu: false,
        exe_path: String::new(),
        menu_version: 0,
    });
    Ok(state.context_menu
        && state.exe_path == exe.display().to_string()
        && state.menu_version == MENU_VERSION)
}

fn save_state(exe: &Path, context_menu: bool) -> std::io::Result<()> {
    let flag = flag_path();
    if let Some(parent) = flag.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = IntegrationState {
        context_menu,
        exe_path: exe.display().to_string(),
        menu_version: MENU_VERSION,
    };
    std::fs::write(flag, serde_json::to_string_pretty(&state)?)?;
    Ok(())
}

fn flag_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SilentSilo")
        .join(INTEGRATION_FLAG)
}
