//! A security key's PIN, asked in the window.
//!
//! On Linux and macOS the app talks to a key itself, so the key's PIN is
//! asked here rather than by the system (Windows asks in its own dialog and
//! never calls this). The ceremony runs on a blocking thread; it emits
//! `fido-pin-request` and waits for `fido_pin_answer`, up to two minutes.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use zeroize::Zeroizing;

type Answer = Option<Zeroizing<String>>;

/// The question waiting for its answer, if one is.
static WAITING: Mutex<Option<Sender<Answer>>> = Mutex::new(None);

const WAIT: Duration = Duration::from_secs(120);

pub fn install(app: AppHandle) {
    silentsilo_fido::set_pin_prompt(Box::new(move |question| {
        let (tx, rx) = channel();
        if let Ok(mut waiting) = WAITING.lock() {
            *waiting = Some(tx);
        }
        let _ = app.emit("fido-pin-request", question);
        let answer = rx.recv_timeout(WAIT).ok().flatten();
        if let Ok(mut waiting) = WAITING.lock() {
            *waiting = None;
        }
        let _ = app.emit("fido-pin-done", ());
        answer
    }));
}

/// The person's answer: the PIN, or nothing for a cancel.
#[tauri::command]
pub fn fido_pin_answer(pin: Option<String>) {
    let pin = pin.map(Zeroizing::new);
    if let Ok(mut waiting) = WAITING.lock()
        && let Some(tx) = waiting.take()
    {
        let _ = tx.send(pin);
    }
}
