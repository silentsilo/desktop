//! Reads the file/folder paths from the OS clipboard (populated by Ctrl+C in
//! Windows Explorer, Cmd+C in Finder, or Ctrl+C in a Linux file manager), so
//! the vault can offer a matching paste-to-upload: the same flow as the
//! "Add to SilentSilo" shell verb, triggered from inside the app instead of
//! from the file manager.

#[cfg(windows)]
mod imp {
    // Fully-qualified (leading `::`) to disambiguate from this crate's own
    // `windows` module (the Explorer context-menu integration) — a bare
    // `windows::` path here would resolve to that local module instead of
    // the `windows` (windows-rs) dependency.
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use ::windows::Win32::System::Ole::CF_HDROP;
    use ::windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    /// Returns the paths held in the clipboard as a Windows "drop" (CF_HDROP)
    /// — i.e. whatever a Ctrl+C in Explorer put there. Empty if the clipboard
    /// holds something else (plain text, an image, etc.) or nothing at all.
    pub fn read_file_paths() -> Vec<String> {
        unsafe {
            if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_err() {
                return Vec::new();
            }
            if OpenClipboard(Some(HWND::default())).is_err() {
                return Vec::new();
            }
            let paths = read_hdrop().unwrap_or_default();
            let _ = CloseClipboard();
            paths
        }
    }

    unsafe fn read_hdrop() -> Option<Vec<String>> {
        unsafe {
            let handle = GetClipboardData(CF_HDROP.0 as u32).ok()?;
            let hdrop = HDROP(handle.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            let mut paths = Vec::with_capacity(count as usize);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                let path = String::from_utf16_lossy(&buf[..len]);
                if !path.is_empty() {
                    paths.push(path);
                }
            }
            Some(paths)
        }
    }
}

#[cfg(windows)]
pub use imp::read_file_paths;

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
    use objc2_foundation::NSURL;

    /// Whatever a Cmd+C in Finder put there: one item per file, each
    /// carrying a `public.file-url`. Anything else on the pasteboard (text,
    /// an image) has no such item and reads as empty.
    pub fn read_file_paths() -> Vec<String> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let Some(items) = pasteboard.pasteboardItems() else {
            return Vec::new();
        };
        // SAFETY: a static AppKit exports for the life of the process.
        let file_url = unsafe { NSPasteboardTypeFileURL };
        items
            .iter()
            .filter_map(|item| item.stringForType(file_url))
            .filter_map(|url| NSURL::URLWithString(&url))
            .filter_map(|url| url.path())
            .map(|path| path.to_string())
            .filter(|path| !path.is_empty())
            .collect()
    }
}

#[cfg(target_os = "macos")]
pub use mac::read_file_paths;

/// Linux has no clipboard in the kernel or in a library everyone links: it
/// is a protocol between the display server and whichever process owns the
/// selection right now. So the answer is a helper program, and which one
/// depends on the display server. Shelling out is the same choice the macOS
/// code makes with `scutil`, for the same reason: the alternative is
/// linking a display-server client into a vault.
#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    /// `text/uri-list` is what every Linux file manager puts on the
    /// clipboard for a copied file: one `file://` URI per line. Nautilus
    /// also writes a private type for its own cut and paste state, and
    /// Dolphin another; the uri-list is the one they agree on.
    const URI_LIST: &str = "text/uri-list";

    /// A session can export `DISPLAY` for XWayland while being Wayland
    /// natively, so `WAYLAND_DISPLAY` is the stronger signal and is read
    /// first.
    pub(crate) fn is_wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    pub fn read_file_paths() -> Vec<String> {
        let Some(raw) = read_clipboard() else {
            return Vec::new();
        };
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(super::uri::path_from_uri)
            .collect()
    }

    fn read_clipboard() -> Option<String> {
        let out = if is_wayland() {
            Command::new("wl-paste")
                .args(["--no-newline", "--type", URI_LIST])
                .output()
        } else {
            Command::new("xclip")
                .args(["-selection", "clipboard", "-t", URI_LIST, "-o"])
                .output()
        }
        .ok()?;
        // A clipboard holding text rather than files makes the tool exit
        // non-zero. That is an answer, not a failure worth reporting.
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// Turning a `text/uri-list` line into a path. No platform in it, so it
/// compiles and is tested everywhere, which matters because the suite
/// only runs in full on Windows.
///
/// Only the Linux side calls it, and the tests below are the whole point of
/// compiling it elsewhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod uri {
    /// `file:///home/a%20b/x.txt` becomes `/home/a b/x.txt`. Only a local
    /// `file://` URI is accepted: a URL copied from a browser is also a
    /// uri-list, and uploading whatever it points at is not what the user
    /// asked for.
    pub fn path_from_uri(uri: &str) -> Option<String> {
        let rest = uri.strip_prefix("file://")?;
        // The authority is empty for a local file, so the path starts at the
        // first slash. A URI naming another host is not ours to open.
        let stripped = rest.strip_prefix('/')?;
        Some(percent_decode(&format!("/{stripped}")))
    }

    /// Decodes `%XX` and leaves anything else alone, including a stray `%`.
    /// Bytes rather than chars, because a percent-encoded name is UTF-8 one
    /// byte at a time and decoding per character would split it.
    fn percent_decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let (Some(hi), Some(lo)) = (
                    (bytes[i + 1] as char).to_digit(16),
                    (bytes[i + 2] as char).to_digit(16),
                )
            {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_local_file_uri_becomes_a_path() {
            assert_eq!(
                path_from_uri("file:///home/a%20b/x.txt").as_deref(),
                Some("/home/a b/x.txt")
            );
        }

        #[test]
        fn a_percent_encoded_name_decodes_as_utf8() {
            // What a file manager writes for a name carrying a diacritic.
            // Decoded per byte, because the character is two of them.
            assert_eq!(
                path_from_uri("file:///home/a/%C4%83.txt").as_deref(),
                Some("/home/a/\u{103}.txt")
            );
        }

        #[test]
        fn anything_that_is_not_a_local_file_is_refused() {
            assert!(path_from_uri("https://example.com/x").is_none());
            assert!(path_from_uri("file://other-host/x").is_none());
            assert!(path_from_uri("/plain/path").is_none());
        }

        #[test]
        fn a_stray_percent_is_left_alone() {
            assert_eq!(percent_decode("100%"), "100%");
            assert_eq!(percent_decode("%zz"), "%zz");
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::read_file_paths;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn read_file_paths() -> Vec<String> {
    Vec::new()
}
