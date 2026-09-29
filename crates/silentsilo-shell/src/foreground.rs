//! The window another program had in front, remembered so focus can go back
//! to it. A fill raises SilentSilo over the browser to ask for confirmation;
//! once confirmed, the person wants the page that was filled, not the app.

/// A top-level window of another process. Only Windows remembers one;
/// elsewhere nothing is taken and nothing is given back.
#[derive(Clone, Copy, Debug)]
pub struct ForegroundWindow {
    #[cfg(windows)]
    hwnd: isize,
}

/// The window in front now, when it belongs to another process. Taken by its
/// root owner, so an extension popup that closes when SilentSilo comes up
/// resolves to the browser window it belongs to.
pub fn foreground_window() -> Option<ForegroundWindow> {
    #[cfg(windows)]
    {
        use ::windows::Win32::UI::WindowsAndMessaging::{
            GA_ROOTOWNER, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId,
        };
        // SAFETY: plain queries on a window handle; a stale one only yields
        // an invalid result, checked below.
        unsafe {
            let front = GetForegroundWindow();
            if front.is_invalid() {
                return None;
            }
            let root = GetAncestor(front, GA_ROOTOWNER);
            let hwnd = if root.is_invalid() { front } else { root };
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return None;
            }
            Some(ForegroundWindow {
                hwnd: hwnd.0 as isize,
            })
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

impl ForegroundWindow {
    /// Gives this window the foreground back from `ours`, the app's own
    /// window (a raw handle), which held it above everything while it asked.
    ///
    /// `ours` loses always-on-top here, synchronously: the toolkit's own call
    /// lands later, and a browser brought forward under a window still on top
    /// stays hidden. Windows then grants the foreground only while this
    /// process holds it, which it gets back a moment after the Windows Hello
    /// prompt closes, so the handover is retried briefly on its own thread.
    /// If it never takes, `ours` goes to the bottom so the page is at least
    /// in view.
    pub fn take_back_from(self, ours: Option<isize>) {
        #[cfg(windows)]
        {
            let browser = self.hwnd;
            std::thread::spawn(move || hand_over(browser, ours));
        }
        #[cfg(not(windows))]
        {
            let _ = ours;
        }
    }
}

#[cfg(windows)]
fn hand_over(browser: isize, ours: Option<isize>) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, HWND_BOTTOM, HWND_NOTOPMOST, IsWindow, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetForegroundWindow, SetWindowPos,
    };
    let browser = HWND(browser as *mut core::ffi::c_void);
    let ours = ours.map(|h| HWND(h as *mut core::ffi::c_void));
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    // SAFETY: both handles are only passed to window calls, which fail
    // harmlessly on a window that has gone; the browser's is checked first.
    unsafe {
        if !IsWindow(Some(browser)).as_bool() {
            return;
        }
        if let Some(ours) = ours {
            let _ = SetWindowPos(ours, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
        }
        for _ in 0..30 {
            let _ = SetForegroundWindow(browser);
            if GetForegroundWindow() == browser {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if let Some(ours) = ours {
            let _ = SetWindowPos(ours, Some(HWND_BOTTOM), 0, 0, 0, 0, flags);
        }
    }
}
