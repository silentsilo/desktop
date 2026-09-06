//! Tells the app when the user leaves the machine. Auto-lock measures idle
//! time, which does not answer the common case: the user locks the screen
//! and walks off, leaving the silo open for minutes until the timer
//! expires. Windows announces that through session notifications, so the
//! app can lock at the same moment the workstation does.

/// What the OS just told us about the user's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    /// The workstation locked, the session was disconnected, or the machine
    /// is suspending. Every one of them means the same thing here.
    UserLeft,
}

#[cfg(windows)]
mod imp {
    use super::SessionEvent;
    use std::sync::OnceLock;

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use ::windows::Win32::System::Power::RegisterSuspendResumeNotification;
    use ::windows::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DispatchMessageW,
        GetMessageW, HWND_MESSAGE, MSG, PBT_APMSUSPEND, RegisterClassW, TranslateMessage,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
    };
    use ::windows::core::PCWSTR;

    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_CONSOLE_DISCONNECT: usize = 0x3;
    const WTS_REMOTE_DISCONNECT: usize = 0x5;

    static HANDLER: OnceLock<Box<dyn Fn(SessionEvent) + Send + Sync>> = OnceLock::new();

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let left = match msg {
            WM_WTSSESSION_CHANGE => matches!(
                wparam.0,
                WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT
            ),
            WM_POWERBROADCAST => wparam.0 as u32 == PBT_APMSUSPEND,
            _ => false,
        };
        if left && let Some(handler) = HANDLER.get() {
            handler(SessionEvent::UserLeft);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Runs the message loop. Blocks, so the caller gives it a thread.
    ///
    /// A message-only window (`HWND_MESSAGE`) rather than a hidden one: it
    /// never appears anywhere, takes part in no z-order, and exists purely to
    /// receive these notifications.
    pub fn watch(handler: Box<dyn Fn(SessionEvent) + Send + Sync>) {
        if HANDLER.set(handler).is_err() {
            return; // Already watching.
        }
        unsafe {
            let Ok(instance) = GetModuleHandleW(None) else {
                return;
            };
            let class_name = wide("SilentSiloSessionWatcher");
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            ) else {
                return;
            };

            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let _ = RegisterSuspendResumeNotification(
                ::windows::Win32::Foundation::HANDLE(hwnd.0),
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// Calls `handler` whenever the user leaves the machine.
///
/// Spawns its own thread and never returns work to the caller. On platforms
/// without an implementation this does nothing, and the idle timer remains
/// the only thing closing an unattended silo.
pub fn on_user_left(handler: impl Fn(SessionEvent) + Send + Sync + 'static) {
    #[cfg(windows)]
    std::thread::spawn(move || imp::watch(Box::new(handler)));
    #[cfg(not(windows))]
    let _ = handler;
}
