//! Reads the file/folder paths from the OS clipboard (populated by Ctrl+C in
//! Windows Explorer), so the vault can offer a matching Ctrl+V "paste to
//! upload" — mirroring the existing "Upload to SilentSilo" context-menu flow
//! but triggered from inside the app instead of from Explorer.

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

#[cfg(not(windows))]
pub fn read_file_paths() -> Vec<String> {
    Vec::new()
}
