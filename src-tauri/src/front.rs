//! A question from outside the window (a fill or a save from the browser, a
//! signature for an SSH client) brings the window forward, above the others
//! while it is open, and puts things back once it is answered.

use tauri::{AppHandle, Manager};

/// The main window as a question found it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowBefore {
    Hidden,
    Minimised,
    /// On screen, in front or not. Left where the person puts it next.
    Shown,
}

fn window_before(app: &AppHandle) -> WindowBefore {
    let Some(window) = app.get_webview_window("main") else {
        return WindowBefore::Shown;
    };
    if !window.is_visible().unwrap_or(true) {
        WindowBefore::Hidden
    } else if window.is_minimized().unwrap_or(false) {
        WindowBefore::Minimised
    } else {
        WindowBefore::Shown
    }
}

/// How the window and the program in front were when a question came in.
pub struct Front {
    before: WindowBefore,
    caller: Option<silentsilo_shell::ForegroundWindow>,
}

impl Front {
    /// Taken before the window moves.
    pub fn capture(app: &AppHandle) -> Self {
        Self {
            before: window_before(app),
            caller: silentsilo_shell::foreground_window(),
        }
    }

    /// Puts the window in front of the caller, above other windows while the
    /// question is open.
    pub fn raise(app: &AppHandle) {
        crate::commands::shell::show_main_window(app);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_always_on_top(true);
            let _ = window.set_focus();
        }
    }

    /// The question is over. The window stops being on top; after a yes it
    /// goes back to hidden or minimised if that is how the question found
    /// it, and the program that was in front gets the focus back, so the
    /// person is left where they were.
    pub fn settle(&self, app: &AppHandle, answered: bool) {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let _ = window.set_always_on_top(false);
        if !answered {
            return;
        }
        match self.before {
            WindowBefore::Hidden => {
                let _ = window.hide();
            }
            WindowBefore::Minimised => {
                let _ = window.minimize();
            }
            WindowBefore::Shown => {}
        }
        if let Some(caller) = self.caller {
            #[cfg(windows)]
            let ours = window.hwnd().ok().map(|h| h.0 as isize);
            #[cfg(not(windows))]
            let ours = None;
            caller.take_back_from(ours);
        }
    }
}
