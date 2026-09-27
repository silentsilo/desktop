//! End-to-end test builds (`--features e2e`): a constant stands in for the
//! security key, and everything the app writes goes under the folder the
//! runner names in `SILENTSILO_E2E_DIR`. Without it the build refuses to
//! start, so it cannot reach the silos of whoever runs it. Debug builds
//! only: the test authenticator does not compile otherwise.

use std::path::PathBuf;

const DIR: &str = "SILENTSILO_E2E_DIR";

pub fn dir() -> PathBuf {
    match std::env::var_os(DIR).filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => panic!("an e2e build needs {DIR}, so it never touches real silos"),
    }
}

/// Points the working copies and their scratch at the test folder. Called
/// first in `run`: the start-up sweep would otherwise clear what the real
/// app has open.
pub fn isolate() {
    let local = dir().join("local");
    // SAFETY: first thing in `run`, before any other thread exists.
    unsafe {
        std::env::set_var("LOCALAPPDATA", &local);
        std::env::set_var("XDG_CACHE_HOME", &local);
        std::env::set_var("SILENTSILO_TEST_WORK_BASE", local.join("work"));
    }
}

/// WebDriver asks WebView2 for a debugging port through
/// `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`. On some machines, the CI runner
/// among them, the window's own arguments replace that variable instead of
/// adding to it, and the driver never finds the port. So the driver's go
/// after ours.
pub fn driver_args<R: tauri::Runtime>(context: &mut tauri::Context<R>) {
    let Some(extra) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
        .ok()
        .filter(|args| !args.trim().is_empty())
    else {
        return;
    };
    for window in &mut context.config_mut().app.windows {
        window.additional_browser_args = Some(match window.additional_browser_args.take() {
            Some(own) => format!("{own} {extra}"),
            None => extra.clone(),
        });
    }
}
