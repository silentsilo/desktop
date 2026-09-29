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
    /// Puts the window back in front, if it still exists. Windows allows this
    /// only while this process holds the foreground, which it does right after
    /// the person confirmed in it.
    pub fn activate(self) {
        #[cfg(windows)]
        {
            use ::windows::Win32::Foundation::HWND;
            use ::windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
            let hwnd = HWND(self.hwnd as *mut core::ffi::c_void);
            // SAFETY: the handle is checked with IsWindow before use.
            unsafe {
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        }
    }
}
