//! Tells the app when the user leaves the machine. Auto-lock measures idle
//! time, which does not answer the common case: the user locks the screen
//! and walks off, leaving the silo open for minutes until the timer
//! expires. Windows announces that through session notifications, macOS
//! through a distributed notification when the screen locks and a
//! workspace one when the machine sleeps, and Linux through logind on the
//! system bus, so the app can lock at the same moment the workstation does.

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

#[cfg(target_os = "macos")]
mod imp {
    use super::SessionEvent;
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};

    /// Posted system-wide the moment the screen locks. Not in Apple's
    /// documentation, but stable across every release since 10.x, and what
    /// every "lock when the screen does" utility listens for.
    const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";

    static HANDLER: OnceLock<Box<dyn Fn(SessionEvent) + Send + Sync>> = OnceLock::new();

    fn user_left() {
        if let Some(handler) = HANDLER.get() {
            // Off the delivering thread, which for a workspace notification
            // is the main thread. Locking every silo does file work, and the
            // screen is already locked so nothing needs to stay responsive,
            // but there is no reason to stall AppKit either.
            std::thread::spawn(|| handler(SessionEvent::UserLeft));
        }
    }

    /// Registers the two observers and returns. Unlike Windows there is no
    /// loop to run here: AppKit delivers workspace notifications on the
    /// main thread's run loop, and the distributed centre delivers to the
    /// thread that registered, so being called from the main thread during
    /// setup is what makes both arrive.
    pub fn watch(handler: Box<dyn Fn(SessionEvent) + Send + Sync>) {
        if HANDLER.set(handler).is_err() {
            return; // Already watching.
        }
        let block = RcBlock::new(|_notification: NonNull<NSNotification>| user_left());
        // SAFETY: the names are valid strings, the block captures nothing and
        // reads only a static, and the observer objects are leaked on
        // purpose: the centre keeps delivering for as long as they exist,
        // which is meant to be the rest of the process.
        unsafe {
            let sleeping = NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceWillSleepNotification),
                    None,
                    None,
                    &block,
                );
            std::mem::forget(sleeping);
            let locked = NSDistributedNotificationCenter::defaultCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(&NSString::from_str(SCREEN_LOCKED)),
                    None,
                    None,
                    &block,
                );
            std::mem::forget(locked);
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::SessionEvent;
    use std::sync::Arc;

    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedObjectPath;

    type Handler = Arc<dyn Fn(SessionEvent) + Send + Sync>;

    const LOGIN1: &str = "org.freedesktop.login1";
    const MANAGER_PATH: &str = "/org/freedesktop/login1";
    const MANAGER: &str = "org.freedesktop.login1.Manager";
    const SESSION: &str = "org.freedesktop.login1.Session";
    const USER: &str = "org.freedesktop.login1.User";
    const USER_SELF: &str = "/org/freedesktop/login1/user/self";

    /// Three listeners, each on a thread of its own since each blocks for
    /// good: the machine about to sleep, this session asked to lock, and
    /// this session's lock screen up. A desktop without logind (no systemd
    /// and no elogind) has none of them, and the idle timer is what closes
    /// the silo there.
    pub fn watch(handler: Box<dyn Fn(SessionEvent) + Send + Sync>) {
        let handler: Handler = Arc::from(handler);
        let sleep = handler.clone();
        std::thread::spawn(move || {
            if let Err(e) = on_sleep(&sleep) {
                eprintln!("[session] not told of sleep: {e}");
            }
        });
        let lock = handler.clone();
        std::thread::spawn(move || {
            if let Err(e) = on_lock_request(&lock) {
                eprintln!("[session] not told of a lock request: {e}");
            }
        });
        std::thread::spawn(move || {
            if let Err(e) = on_locked_hint(&handler) {
                eprintln!("[session] not told of the lock screen: {e}");
            }
        });
    }

    /// `PrepareForSleep(true)`, sent before a suspend or a hibernation.
    fn on_sleep(handler: &Handler) -> zbus::Result<()> {
        let connection = Connection::system()?;
        let manager = Proxy::new(&connection, LOGIN1, MANAGER_PATH, MANAGER)?;
        for signal in manager.receive_signal("PrepareForSleep")? {
            if signal.body().deserialize::<bool>()? {
                handler(SessionEvent::UserLeft);
            }
        }
        Ok(())
    }

    /// `Lock` on this session: `loginctl lock-session`, or the desktop
    /// asking every screen to lock.
    fn on_lock_request(handler: &Handler) -> zbus::Result<()> {
        let connection = Connection::system()?;
        let session = Proxy::new(&connection, LOGIN1, session_path(&connection)?, SESSION)?;
        for _ in session.receive_signal("Lock")? {
            handler(SessionEvent::UserLeft);
        }
        Ok(())
    }

    /// `LockedHint` turning true: GNOME and KDE set it while their lock
    /// screen is up, however it was locked.
    fn on_locked_hint(handler: &Handler) -> zbus::Result<()> {
        let connection = Connection::system()?;
        let session = Proxy::new(&connection, LOGIN1, session_path(&connection)?, SESSION)?;
        for change in session.receive_property_changed::<bool>("LockedHint") {
            if change.get()? {
                handler(SessionEvent::UserLeft);
            }
        }
        Ok(())
    }

    /// This process's login session. Signals come from its real path, never
    /// from the `auto` alias, so it is asked for by id, or by process when
    /// the environment does not name it. An app started at sign-in by
    /// systemd's autostart generator, or by GNOME Shell, runs in a scope of
    /// the user's manager rather than in the session, and neither finds one:
    /// then the user's graphical session, which logind names `Display`.
    fn session_path(connection: &Connection) -> zbus::Result<OwnedObjectPath> {
        let manager = Proxy::new(connection, LOGIN1, MANAGER_PATH, MANAGER)?;
        if let Ok(id) = std::env::var("XDG_SESSION_ID")
            && let Ok(path) = manager.call("GetSession", &(id.as_str(),))
        {
            return Ok(path);
        }
        let by_process: zbus::Result<OwnedObjectPath> =
            manager.call("GetSessionByPID", &(std::process::id(),));
        if by_process.is_ok() {
            return by_process;
        }
        let user = Proxy::new(connection, LOGIN1, USER_SELF, USER)?;
        let (_, display): (String, OwnedObjectPath) = user.get_property("Display")?;
        if display.as_str() == "/" {
            return by_process;
        }
        Ok(display)
    }
}

/// Calls `handler` whenever the user leaves the machine.
///
/// On Windows this spawns the thread that runs the message loop and never
/// returns work to the caller. On macOS it registers observers with the
/// system and returns; call it from the main thread. On Linux it starts the
/// threads that listen to logind and returns. On platforms without an
/// implementation it does nothing, and the idle timer remains the only
/// thing closing an unattended silo.
pub fn on_user_left(handler: impl Fn(SessionEvent) + Send + Sync + 'static) {
    #[cfg(windows)]
    std::thread::spawn(move || imp::watch(Box::new(handler)));
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    imp::watch(Box::new(handler));
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let _ = handler;
}
