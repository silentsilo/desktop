//! Putting a secret on the clipboard without leaving it lying around.
//! `navigator.clipboard.writeText` is the wrong tool on Windows:
//! Clipboard History keeps the last 25 entries **on disk**, and Cloud
//! Clipboard syncs them to the user's other machines. Windows lets a
//! writer opt out of both through two documented formats. Neither is a
//! permission check (any process reading the clipboard still sees the
//! value); what they stop is the system *retaining* the copy.

use std::sync::Mutex;

use zeroize::Zeroizing;

/// How long a copied secret stays on the clipboard before it is cleared.
///
/// Long enough to switch window and paste, short enough that walking away
/// does not leave a password sitting there. The value matches what most
/// password managers settled on.
pub const SECRET_CLIPBOARD_TTL_SECS: u64 = 45;

/// The secret this app last put on the clipboard, until it is taken back.
/// The timer alone was not enough: locking a silo, or the workstation
/// locking, is the user saying they are gone, and the password copied a
/// moment earlier would otherwise stay readable for the rest of its
/// forty-five seconds.
static OUTSTANDING: Mutex<Option<Zeroizing<String>>> = Mutex::new(None);

/// Puts a secret on the clipboard and remembers it, so a lock can take it
/// back before the timer would.
pub fn set_secret_tracked(text: &str) -> Result<(), String> {
    set_secret(text)?;
    if let Ok(mut held) = OUTSTANDING.lock() {
        *held = Some(Zeroizing::new(text.to_string()));
    }
    Ok(())
}

/// Takes back whatever this app last copied, if the clipboard still holds
/// it. Called when a silo locks, and when the machine says the user left.
///
/// Returns whether anything was actually cleared, so the caller can say the
/// copy is gone rather than guess.
pub fn clear_outstanding() -> bool {
    let held = match OUTSTANDING.lock() {
        Ok(mut slot) => slot.take(),
        Err(_) => return false,
    };
    match held {
        Some(text) => clear_if_still(&text),
        None => false,
    }
}

/// One copy's own timer expiring.
///
/// Deliberately not [`clear_outstanding`]: by the time this fires the user
/// may have copied something newer, and the older timer taking the slot
/// would cut the newer copy's life short. It only forgets what it put there
/// itself, and only clears a clipboard that still holds it.
pub fn clear_expired(text: &str) -> bool {
    if let Ok(mut slot) = OUTSTANDING.lock()
        && slot.as_ref().map(|held| held.as_str()) == Some(text)
    {
        *slot = None;
    }
    clear_if_still(text)
}

#[cfg(windows)]
mod imp {
    use ::windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use ::windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use ::windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use ::windows::Win32::System::Ole::CF_UNICODETEXT;
    use ::windows::core::PCWSTR;

    /// Excludes the value from Clipboard History and from Cloud Clipboard.
    /// Documented under "Clipboard Formats" as the opt-out a writer of
    /// sensitive data is expected to set.
    const EXCLUDE_HISTORY: &str = "CanIncludeInClipboardHistory";
    const EXCLUDE_MONITORING: &str = "ExcludeClipboardContentFromMonitorProcessing";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Takes the clipboard, waiting for whoever has it. `OpenClipboard`
    /// fails outright while another process holds it, and something usually
    /// does, so Windows expects a retry rather than treating the first
    /// refusal as final. Bounded to a fifth of a second: better than
    /// hanging a click on a clipboard some other program wedged open.
    fn open_clipboard() -> Result<(), String> {
        let mut last = "the clipboard is in use by another program".to_string();
        for _ in 0..10 {
            match unsafe { OpenClipboard(Some(HWND::default())) } {
                Ok(()) => return Ok(()),
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        Err(last)
    }

    /// Allocates a moveable global block holding `data`, as the clipboard
    /// requires: ownership passes to the system on `SetClipboardData`, so
    /// this must not be freed here.
    unsafe fn global_from(data: &[u8]) -> Option<HGLOBAL> {
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, data.len()).ok()?;
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
            let _ = GlobalUnlock(handle);
            Some(handle)
        }
    }

    unsafe fn set_flag(name: &str) {
        unsafe {
            let format = RegisterClipboardFormatW(PCWSTR(wide(name).as_ptr()));
            if format == 0 {
                return;
            }
            // A single zero byte is the documented "no" for both formats.
            if let Some(handle) = global_from(&[0u8]) {
                let _ = SetClipboardData(format, Some(HANDLE(handle.0)));
            }
        }
    }

    /// Copies `text` as a secret: readable by a paste, but kept out of
    /// clipboard history and cloud sync.
    pub fn set_secret(text: &str) -> Result<(), String> {
        open_clipboard()?;
        unsafe {
            let result = (|| {
                EmptyClipboard().map_err(|e| e.to_string())?;

                let payload = wide(text);
                let bytes =
                    std::slice::from_raw_parts(payload.as_ptr() as *const u8, payload.len() * 2);
                let handle = global_from(bytes)
                    .ok_or_else(|| "could not allocate clipboard memory".to_string())?;
                SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0)))
                    .map_err(|e| e.to_string())?;

                // After the value, so an early failure above leaves nothing
                // on the clipboard at all rather than a bare set of flags.
                set_flag(EXCLUDE_HISTORY);
                set_flag(EXCLUDE_MONITORING);
                Ok(())
            })();
            let _ = CloseClipboard();
            result
        }
    }

    /// Clears the clipboard, but only if it still holds `expected`.
    ///
    /// The check matters: by the time the timer fires the user may well have
    /// copied something else, and wiping that would be the app reaching into
    /// something that is no longer its business.
    pub fn clear_if_still(expected: &str) -> bool {
        if open_clipboard().is_err() {
            return false;
        }
        unsafe {
            let cleared = (|| {
                let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
                let ptr = GlobalLock(HGLOBAL(handle.0)) as *const u16;
                if ptr.is_null() {
                    return None;
                }
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let current = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                let _ = GlobalUnlock(HGLOBAL(handle.0));

                if current != expected {
                    return Some(false);
                }
                EmptyClipboard().ok()?;
                Some(true)
            })()
            .unwrap_or(false);
            let _ = CloseClipboard();
            cleared
        }
    }
}

#[cfg(windows)]
pub use imp::{clear_if_still, set_secret};

#[cfg(not(windows))]
pub fn set_secret(_text: &str) -> Result<(), String> {
    Err("copying secrets is only implemented on Windows so far".into())
}

#[cfg(not(windows))]
pub fn clear_if_still(_expected: &str) -> bool {
    false
}

/// These use the real clipboard, like the autostart tests use the real
/// registry: there is no seam to fake, and the property being checked is
/// exactly what the operating system does with the value. They leave the
/// clipboard empty, which is the safe direction to leave it in.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    static LOCK: Mutex<()> = Mutex::new(());
    fn serialised() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn locking_takes_the_secret_back_before_the_timer_would() {
        // The whole point: the timer is not the only thing that ends a copy.
        let _guard = serialised();
        set_secret_tracked("hunter2-lock").expect("set");
        assert!(
            clear_outstanding(),
            "a copy this app made was not taken back"
        );
        assert!(
            !clear_outstanding(),
            "clearing twice must not claim a second success"
        );
    }

    #[test]
    fn an_expiring_timer_does_not_cut_short_a_newer_copy() {
        // Copy A, copy B, then A's timer fires. B has to survive with its own
        // full life ahead of it, and stay tracked so a lock still takes it.
        let _guard = serialised();
        set_secret_tracked("first-secret").expect("set A");
        set_secret_tracked("second-secret").expect("set B");

        assert!(
            !clear_expired("first-secret"),
            "A's timer cleared a clipboard that no longer held A"
        );
        assert!(
            clear_outstanding(),
            "B stopped being tracked when A's timer fired"
        );
    }

    #[test]
    fn nothing_copied_means_nothing_to_take_back() {
        let _guard = serialised();
        let _ = clear_outstanding();
        assert!(!clear_outstanding());
    }
}
