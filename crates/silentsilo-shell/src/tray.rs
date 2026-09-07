//! Whether this system has somewhere to put a tray icon.
//!
//! The app is built to keep running with its window closed: the shell verbs
//! forward to the live instance, the sync timer only ticks while a process
//! exists, and a cold start costs a security key prompt. Closing the window
//! therefore hides it, and the tray is how the user gets it back.
//!
//! That contract holds on Windows and macOS, where the notification area
//! and the menu bar are part of the system. It does not hold on Linux.
//! GNOME removed the tray in 2017 and shows nothing without the
//! AppIndicator extension, which is not installed by default. An app that
//! hides its only window there is gone: no icon, no window, and a running
//! process the user cannot reach.
//!
//! So the question is asked rather than assumed, and the caller turns
//! closing the window into quitting when the answer is no.

/// Whether a tray icon will actually be visible to the user.
///
/// Windows and macOS always answer yes. Linux is asked properly, and
/// answers no when it cannot tell: an app that quits when it did not need
/// to is an annoyance the user can undo, and an app that vanishes into a
/// tray nobody can see is not.
pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::status_notifier_host_present()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    /// The bus name a tray host owns. KDE's Plasma, GNOME's AppIndicator
    /// extension, the Xfce and Cinnamon panels and every other modern tray
    /// implement the StatusNotifierItem specification, and the watcher is
    /// the piece of it that has to exist for an icon to appear anywhere. Its
    /// name kept the `org.kde` prefix from where the spec was written.
    const WATCHER: &str = "org.kde.StatusNotifierWatcher";

    /// Asks the session bus whether anyone owns the watcher name.
    ///
    /// Through `gdbus`, which ships with glib and is therefore on every
    /// machine that has a desktop at all, rather than by linking a D-Bus
    /// client into this crate for one boolean. The same reasoning as the
    /// clipboard helpers and `scutil` on macOS.
    pub fn status_notifier_host_present() -> bool {
        let Ok(out) = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.DBus",
                "--object-path",
                "/org/freedesktop/DBus",
                "--method",
                "org.freedesktop.DBus.NameHasOwner",
                WATCHER,
            ])
            .output()
        else {
            // No gdbus, or no session bus. Either way this is not a desktop
            // that will show an icon.
            return false;
        };
        if !out.status.success() {
            return false;
        }
        // The reply is a one-element tuple, printed as `(true,)`.
        String::from_utf8_lossy(&out.stdout).contains("true")
    }
}
