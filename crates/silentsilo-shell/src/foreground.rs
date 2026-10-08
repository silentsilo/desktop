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

/// What auto-type needs to know about the window it will type into.
impl ForegroundWindow {
    /// The window's title as the program sets it. Only a hint: any program
    /// can show any title.
    pub fn title(&self) -> String {
        #[cfg(windows)]
        {
            use ::windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
            let mut buf = [0u16; 512];
            // SAFETY: writes at most the buffer's length into it.
            let n = unsafe { GetWindowTextW(self.handle(), &mut buf) };
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
        #[cfg(not(windows))]
        {
            String::new()
        }
    }

    /// The process that owns the window, 0 when it has gone.
    pub fn process_id(&self) -> u32 {
        #[cfg(windows)]
        {
            use ::windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
            let mut pid = 0u32;
            // SAFETY: a query on a handle; a stale one leaves pid at 0.
            unsafe { GetWindowThreadProcessId(self.handle(), Some(&mut pid)) };
            pid
        }
        #[cfg(not(windows))]
        {
            0
        }
    }

    /// The owning program's file name ("putty.exe"), empty when it cannot be
    /// read.
    pub fn program(&self) -> String {
        #[cfg(windows)]
        {
            use ::windows::Win32::Foundation::CloseHandle;
            use ::windows::Win32::System::Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            };
            let pid = self.process_id();
            if pid == 0 {
                return String::new();
            }
            // SAFETY: the handle is closed below; the buffer and its length
            // go together.
            unsafe {
                let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                    return String::new();
                };
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                let read = QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    ::windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                );
                let _ = CloseHandle(process);
                if read.is_err() {
                    return String::new();
                }
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                path.rsplit(['\\', '/'])
                    .next()
                    .unwrap_or_default()
                    .to_string()
            }
        }
        #[cfg(not(windows))]
        {
            String::new()
        }
    }

    /// Whether the window's process runs elevated. Windows drops input sent
    /// to a process above ours without an error, so auto-type refuses there
    /// rather than type into nothing. A token that cannot be read counts as
    /// elevated: refusing is the safe mistake.
    pub fn is_elevated(&self) -> bool {
        #[cfg(windows)]
        {
            use ::windows::Win32::Foundation::{CloseHandle, HANDLE};
            use ::windows::Win32::Security::{
                GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
            };
            use ::windows::Win32::System::Threading::{
                OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            };
            let pid = self.process_id();
            if pid == 0 {
                return true;
            }
            // SAFETY: both handles are closed below; the elevation struct is
            // a local sized for the call.
            unsafe {
                let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                    return true;
                };
                let mut token = HANDLE::default();
                let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
                let _ = CloseHandle(process);
                if opened.is_err() {
                    return true;
                }
                let mut elevation = TOKEN_ELEVATION::default();
                let mut returned = 0u32;
                let read = GetTokenInformation(
                    token,
                    TokenElevation,
                    Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut returned,
                );
                let _ = CloseHandle(token);
                read.is_err() || elevation.TokenIsElevated != 0
            }
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Whether this window, in the same process as when it was taken, is in
    /// front now.
    pub fn is_in_front(&self, process_id: u32) -> bool {
        foreground_window().is_some_and(|front| {
            front.same_window(self) && front.process_id() == process_id && process_id != 0
        })
    }

    fn same_window(&self, other: &ForegroundWindow) -> bool {
        #[cfg(windows)]
        {
            self.hwnd == other.hwnd
        }
        #[cfg(not(windows))]
        {
            let _ = other;
            false
        }
    }

    #[cfg(windows)]
    fn handle(&self) -> ::windows::Win32::Foundation::HWND {
        ::windows::Win32::Foundation::HWND(self.hwnd as *mut core::ffi::c_void)
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
