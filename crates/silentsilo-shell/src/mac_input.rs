//! macOS pieces of auto-type and of giving focus back: the global shortcut
//! (Carbon's `RegisterEventHotKey`, which needs no permission), the
//! application in front, and typing with `CGEventPost`, which macOS lets
//! through only once the person allows SilentSilo under Privacy & Security,
//! Accessibility.

use std::ffi::c_void;
use std::sync::Mutex;

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

type OsStatus = i32;

#[repr(C)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

type EventHandler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> OsStatus;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandler,
        count: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut *mut c_void,
    ) -> OsStatus;
    fn RemoveEventHandler(handler: *mut c_void) -> OsStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> OsStatus;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> OsStatus;
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(event: *mut c_void, length: usize, text: *const u16);
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: CFStringRef,
        value: *mut *const c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

unsafe extern "C" {
    fn pthread_main_np() -> i32;
    static _dispatch_main_q: u8;
    fn dispatch_sync_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const KEY_A: u32 = 0;
const KEY_TAB: u16 = 48;
const KEY_RETURN: u16 = 36;
const CONTROL_KEY: u32 = 0x1000;
const OPTION_KEY: u32 = 0x0800;
const HOT_KEY_PRESSED: u32 = 5;
const HID_EVENT_TAP: u32 = 0;
const HID_SYSTEM_STATE: i32 = 1;
const MODIFIER_FLAGS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

/// Runs `work` on the main thread and waits for it: Carbon's hot keys
/// belong to the main run loop.
fn on_main<F: FnOnce() -> R + Send, R: Send>(work: F) -> R {
    // SAFETY: a plain query.
    if unsafe { pthread_main_np() } != 0 {
        return work();
    }
    struct Job<F, R> {
        work: Option<F>,
        result: Option<R>,
    }
    extern "C" fn run<F: FnOnce() -> R, R>(context: *mut c_void) {
        // SAFETY: `context` is the `Job` below, alive until dispatch_sync returns.
        let job = unsafe { &mut *context.cast::<Job<F, R>>() };
        if let Some(work) = job.work.take() {
            job.result = Some(work());
        }
    }
    let mut job = Job {
        work: Some(work),
        result: None,
    };
    // SAFETY: the main queue is a static; dispatch_sync returns only after
    // `run` has, so `job` outlives the call.
    unsafe {
        dispatch_sync_f(
            (&raw const _dispatch_main_q).cast(),
            (&raw mut job).cast(),
            run::<F, R>,
        );
    }
    job.result.expect("the main queue ran the job")
}

type Callback = Box<dyn Fn() + Send>;

/// The registered shortcut: Carbon's two references and the callback they
/// point at, as addresses so the handle can move between threads.
pub struct MacHotkey {
    hotkey: usize,
    handler: usize,
    callback: usize,
}

extern "C" fn hotkey_pressed(
    _next: *mut c_void,
    _event: *mut c_void,
    data: *mut c_void,
) -> OsStatus {
    // SAFETY: `data` is the callback `register` leaked; it is freed only
    // after the handler is removed.
    let callback = unsafe { &*data.cast::<Mutex<Callback>>() };
    if let Ok(callback) = callback.lock() {
        callback();
    }
    0
}

/// Registers Control-Option-A, the same keys as Ctrl+Alt+A on Windows.
/// `None` when another program holds them.
pub fn register(on_press: impl Fn() + Send + 'static) -> Option<MacHotkey> {
    // The press arrives on the main thread; the app's handler moves windows
    // and waits on them, so it runs on a thread of its own.
    let on_press = std::sync::Arc::new(Mutex::new(on_press));
    let callback: Callback = Box::new(move || {
        let on_press = on_press.clone();
        std::thread::spawn(move || {
            if let Ok(on_press) = on_press.lock() {
                on_press();
            }
        });
    });
    let callback = Box::into_raw(Box::new(Mutex::new(callback))) as usize;
    on_main(move || {
        let spec = EventTypeSpec {
            event_class: four_cc(b"keyb"),
            event_kind: HOT_KEY_PRESSED,
        };
        let mut handler = std::ptr::null_mut();
        let mut hotkey = std::ptr::null_mut();
        // SAFETY: Carbon calls on the main thread, which this is; the
        // callback stays alive until `unregister` frees it.
        unsafe {
            let target = GetApplicationEventTarget();
            if InstallEventHandler(
                target,
                hotkey_pressed,
                1,
                &spec,
                callback as *mut c_void,
                &mut handler,
            ) != 0
            {
                drop(Box::from_raw(callback as *mut Mutex<Callback>));
                return None;
            }
            let id = EventHotKeyId {
                signature: four_cc(b"SSAT"),
                id: 1,
            };
            if RegisterEventHotKey(KEY_A, CONTROL_KEY | OPTION_KEY, id, target, 0, &mut hotkey) != 0
            {
                RemoveEventHandler(handler);
                drop(Box::from_raw(callback as *mut Mutex<Callback>));
                return None;
            }
        }
        Some(MacHotkey {
            hotkey: hotkey as usize,
            handler: handler as usize,
            callback,
        })
    })
}

impl Drop for MacHotkey {
    fn drop(&mut self) {
        let (hotkey, handler, callback) = (self.hotkey, self.handler, self.callback);
        on_main(move || {
            // SAFETY: the references `register` made, released once; after
            // the handler is gone nothing can reach the callback.
            unsafe {
                UnregisterEventHotKey(hotkey as *mut c_void);
                RemoveEventHandler(handler as *mut c_void);
                drop(Box::from_raw(callback as *mut Mutex<Callback>));
            }
        });
    }
}

/// Whether macOS lets SilentSilo type into other applications.
pub fn access_granted() -> bool {
    // SAFETY: a plain query.
    unsafe { AXIsProcessTrusted() }
}

/// Puts SilentSilo in the Accessibility list and shows macOS's own prompt
/// that leads there. The person still has to switch it on.
pub fn ask_for_access() {
    // SAFETY: the key is a constant the framework exports; the dictionary
    // lives across the call.
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::true_value().as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef().cast());
    }
}

/// Another program asked for secure keyboard entry (Terminal's option, a
/// password prompt): macOS then drops typed events from everyone else.
pub fn secure_input_on() -> bool {
    // SAFETY: a plain query.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Whether Control, Option, Shift or Command is still down.
pub fn modifiers_held() -> bool {
    // SAFETY: a plain query.
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & MODIFIER_FLAGS != 0 }
}

/// Posts one key press and release; with `text`, the event carries those
/// characters instead of the key's own, so the layout does not matter.
pub fn post_key(key: u16, text: Option<&[u16]>) -> bool {
    for down in [true, false] {
        // SAFETY: the event is created, used and released here.
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
            if event.is_null() {
                return false;
            }
            if let Some(text) = text {
                CGEventKeyboardSetUnicodeString(event, text.len(), text.as_ptr());
            }
            // Clear, so a modifier the system still counts as down does not
            // turn the character into a shortcut.
            CGEventSetFlags(event, 0);
            CGEventPost(HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
    true
}

pub fn post_text(text: &str) -> bool {
    let mut units = [0u16; 2];
    text.chars()
        .all(|c| post_key(KEY_A as u16, Some(c.encode_utf16(&mut units))))
}

pub fn post_tab() -> bool {
    post_key(KEY_TAB, None)
}

pub fn post_return() -> bool {
    post_key(KEY_RETURN, None)
}

/// The application in front, when it is not this one.
pub fn frontmost_pid() -> Option<i32> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = app.processIdentifier();
    (pid > 0 && pid as u32 != std::process::id()).then_some(pid)
}

/// The application's name as macOS shows it ("Terminal").
pub fn app_name(pid: i32) -> String {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .and_then(|app| {
            app.localizedName()
                .or_else(|| app.executableURL().and_then(|url| url.lastPathComponent()))
        })
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// The title of the application's focused window. Read through
/// Accessibility, so empty until SilentSilo is allowed there.
pub fn window_title(pid: i32) -> String {
    if !access_granted() {
        return String::new();
    }
    // SAFETY: every object copied here is released by its wrapper or below.
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return String::new();
        }
        let title = copy_attribute(app, "AXFocusedWindow")
            .and_then(|window| {
                let title = copy_attribute(window.as_CFTypeRef() as *mut c_void, "AXTitle");
                drop(window);
                title
            })
            .and_then(|value| value.downcast_into::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_default();
        CFRelease(app);
        title
    }
}

/// # Safety
/// `element` is a live AXUIElement.
unsafe fn copy_attribute(element: *mut c_void, name: &'static str) -> Option<CFType> {
    let attribute = CFString::from_static_string(name);
    let mut value = std::ptr::null();
    // SAFETY: the caller's element; the value comes back retained.
    let status = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    // SAFETY: a +1 reference from a Copy call.
    (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}

/// Brings the application forward again.
pub fn activate(pid: i32) {
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
}
