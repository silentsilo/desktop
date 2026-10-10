//! Auto-type: the global shortcut that asks for it, and typing a login into
//! the window that was in front. Windows and macOS; elsewhere every call
//! says it is not available.
//!
//! The text goes as Unicode characters (`KEYEVENTF_UNICODE`, or a key event
//! carrying the character on macOS), not as keys, so the keyboard layout
//! cannot turn a password into something else. Tab and Enter go as keys,
//! since a form reads them as keys.

use zeroize::Zeroizing;

/// One thing to type.
pub enum Step {
    Text(Zeroizing<String>),
    Tab,
    Enter,
}

/// The shortcut while it is registered; dropping it unregisters it.
pub struct Hotkey {
    #[cfg(windows)]
    thread: u32,
    #[cfg(target_os = "macos")]
    _registered: crate::mac_input::MacHotkey,
}

/// Why a shortcut could not be registered.
#[derive(Debug, PartialEq, Eq)]
pub enum HotkeyError {
    /// Another program holds the same combination.
    Taken,
    NotSupported,
}

/// Registers Ctrl+Alt+A (Control-Option-A on a Mac) for the whole session
/// and calls `on_press` on each press, on a thread of its own:
/// `RegisterHotKey` delivers to the thread that registered, which must run a
/// message loop.
pub fn register_hotkey(on_press: impl Fn() + Send + 'static) -> Result<Hotkey, HotkeyError> {
    #[cfg(windows)]
    {
        use ::windows::Win32::System::Threading::GetCurrentThreadId;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{
            MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
        };
        use ::windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // SAFETY: the registration and the loop belong to this thread;
            // the message is read into a local.
            unsafe {
                let registered = RegisterHotKey(
                    None,
                    1,
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    u32::from(b'A'),
                );
                if registered.is_err() {
                    let _ = tx.send(None);
                    return;
                }
                let _ = tx.send(Some(GetCurrentThreadId()));
                let mut msg = MSG::default();
                // Ends on WM_QUIT, which `Hotkey::drop` posts.
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_HOTKEY {
                        on_press();
                    }
                }
                let _ = UnregisterHotKey(None, 1);
            }
        });
        match rx.recv() {
            Ok(Some(thread)) => Ok(Hotkey { thread }),
            _ => Err(HotkeyError::Taken),
        }
    }
    #[cfg(target_os = "macos")]
    {
        crate::mac_input::register(on_press)
            .map(|registered| Hotkey {
                _registered: registered,
            })
            .ok_or(HotkeyError::Taken)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = on_press;
        Err(HotkeyError::NotSupported)
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use ::windows::Win32::Foundation::{LPARAM, WPARAM};
            use ::windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            // SAFETY: posts to the loop's own thread; a thread that has
            // already ended makes this fail harmlessly.
            unsafe {
                let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Why nothing was typed.
#[derive(Debug, PartialEq, Eq)]
pub enum TypeError {
    /// A modifier key (Ctrl, Alt, Shift, Windows) was still down after the
    /// wait: typed characters would have become shortcuts.
    KeysHeld,
    /// Windows took fewer inputs than were sent.
    Refused,
    /// macOS: SilentSilo is not allowed under Privacy & Security,
    /// Accessibility, and typed events would go nowhere.
    NoAccess,
    /// macOS: another program has secure keyboard entry on.
    SecureInput,
    NotSupported,
}

/// Whether the system lets SilentSilo type into other programs. Only macOS
/// asks; elsewhere it is always yes.
pub fn access_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::mac_input::access_granted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// macOS: adds SilentSilo to the Accessibility list and shows the system's
/// prompt that leads there. Nothing elsewhere.
pub fn ask_for_access() {
    #[cfg(target_os = "macos")]
    crate::mac_input::ask_for_access();
}

/// macOS: opens Privacy & Security, Accessibility, with SilentSilo already
/// in its list. Nothing elsewhere.
pub fn open_access_settings() {
    #[cfg(target_os = "macos")]
    {
        crate::mac_input::ask_for_access();
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

/// Types `steps` into whatever window is in front, after the shortcut's own
/// keys are released. The caller checks first that the window in front is
/// the one meant.
pub fn type_steps(steps: &[Step]) -> Result<(), TypeError> {
    #[cfg(windows)]
    {
        wait_for_modifiers_up()?;
        for step in steps {
            let inputs = match step {
                Step::Text(text) => unicode_inputs(text),
                Step::Tab => key_inputs(::windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB),
                Step::Enter => key_inputs(::windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN),
            };
            send(&inputs)?;
            // A field that moves focus on Tab needs a moment before the next
            // characters, or they land in the field being left.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        use crate::mac_input as mac;
        if !mac::access_granted() {
            return Err(TypeError::NoAccess);
        }
        if mac::secure_input_on() {
            return Err(TypeError::SecureInput);
        }
        let mut released = false;
        for _ in 0..100 {
            if !mac::modifiers_held() {
                released = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !released {
            return Err(TypeError::KeysHeld);
        }
        for step in steps {
            let sent = match step {
                Step::Text(text) => mac::post_text(text),
                Step::Tab => mac::post_tab(),
                Step::Enter => mac::post_return(),
            };
            if !sent {
                return Err(TypeError::Refused);
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = steps;
        Err(TypeError::NotSupported)
    }
}

#[cfg(windows)]
fn wait_for_modifiers_up() -> Result<(), TypeError> {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let keys = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];
    for _ in 0..100 {
        // SAFETY: reads the key state; no pointers involved.
        let held = keys
            .iter()
            .any(|k| unsafe { GetAsyncKeyState(i32::from(k.0)) } as u16 & 0x8000 != 0);
        if !held {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(TypeError::KeysHeld)
}

#[cfg(windows)]
fn unicode_inputs(text: &str) -> Vec<::windows::Win32::UI::Input::KeyboardAndMouse::INPUT> {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    let event = |unit: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE
                    | if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    text.encode_utf16()
        .flat_map(|unit| [event(unit, false), event(unit, true)])
        .collect()
}

#[cfg(windows)]
fn key_inputs(
    key: ::windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
) -> Vec<::windows::Win32::UI::Input::KeyboardAndMouse::INPUT> {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let event = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    vec![event(KEYBD_EVENT_FLAGS(0)), event(KEYEVENTF_KEYUP)]
}

#[cfg(windows)]
fn send(inputs: &[::windows::Win32::UI::Input::KeyboardAndMouse::INPUT]) -> Result<(), TypeError> {
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, SendInput};
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: a slice of initialised INPUT structures and their size.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(TypeError::Refused)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn text_goes_as_characters_down_and_up() {
        let inputs = unicode_inputs("aé😀");
        // "😀" is two UTF-16 units, each pressed and released.
        assert_eq!(inputs.len(), 2 * 4);
    }

    #[test]
    fn a_key_is_pressed_then_released() {
        let inputs = key_inputs(::windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB);
        assert_eq!(inputs.len(), 2);
    }
}
