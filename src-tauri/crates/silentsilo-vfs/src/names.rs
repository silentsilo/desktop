//! What a file or folder may be called, decided once for every platform:
//! the intersection of what Windows, macOS and Linux accept, applied when
//! an operation is *replayed* rather than when it is typed. Every device
//! must reach the same name from the same record, or two of them disagree
//! about what a file is called with no way to notice.
//!
//! Two silent problems in particular. macOS hands back decomposed names
//! (NFD) where the others give composed ones, so without normalising, a
//! round trip through a Mac creates a second entry that looks identical on
//! screen. And Linux allows two names differing only in case, which Windows
//! cannot export, so uniqueness is judged case-insensitively even though
//! names are stored exactly as typed.

use unicode_normalization::UnicodeNormalization;

use silentsilo_core::{CoreError, CoreResult};

/// Longest name in bytes, matching the limit on ext4, APFS and NTFS. Measured
/// in bytes rather than characters because that is what those filesystems
/// count.
pub const MAX_NAME_BYTES: usize = 255;

/// Illegal on Windows, legal everywhere else. Allowing them would produce
/// silos that simply cannot be exported on one of the three platforms.
const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Windows refuses these whatever the extension, so `nul.txt` is out too.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// What a name shown as `raw` is stored as.
///
/// Total by design: replay cannot reject a record, because the record is in
/// the log forever and refusing it would wedge every future pass. So anything
/// unacceptable is repaired rather than refused, and [`check`] is what stops
/// the repair being needed in the first place for names typed locally.
pub fn sanitize(raw: &str) -> String {
    let composed: String = raw.nfc().collect();

    let mut cleaned: String = composed
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Windows drops trailing dots and spaces when creating a file, so a name
    // ending in one is a name that does not survive a round trip to disk.
    cleaned = cleaned
        .trim_start()
        .trim_end_matches([' ', '.', '\t'])
        .to_string();

    cleaned = truncate_bytes(&cleaned, MAX_NAME_BYTES);

    if is_reserved(&cleaned) {
        cleaned.insert(0, '_');
    }

    if cleaned.is_empty() {
        return "unnamed".to_string();
    }
    cleaned
}

/// The key two names are compared on to decide whether they collide.
///
/// Case-folded and composed, so `Report.pdf` and `report.pdf` cannot both
/// exist in one folder, and neither can the composed and decomposed spellings
/// of the same word.
pub fn fold(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// Whether a name typed by the user is acceptable as-is.
///
/// Separate from [`sanitize`] because the two situations are different: a
/// record from another device must be accepted and repaired, while a name
/// someone just typed should come back as an error they can act on. Quietly
/// storing something other than what was typed is the worst of both.
pub fn check(raw: &str) -> CoreResult<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CoreError::InvalidName("the name is empty".into()));
    }
    if let Some(bad) = trimmed.chars().find(|c| FORBIDDEN.contains(c)) {
        return Err(CoreError::InvalidName(format!(
            "a name cannot contain {bad}"
        )));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(CoreError::InvalidName(
            "a name cannot contain control characters".into(),
        ));
    }
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        return Err(CoreError::InvalidName(
            "a name cannot end in a dot or a space".into(),
        ));
    }
    if trimmed.len() > MAX_NAME_BYTES {
        return Err(CoreError::InvalidName(format!(
            "a name cannot be longer than {MAX_NAME_BYTES} bytes"
        )));
    }
    if is_reserved(trimmed) {
        return Err(CoreError::InvalidName(format!(
            "{trimmed} is a reserved name on Windows"
        )));
    }
    Ok(())
}

fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Cuts to at most `limit` bytes without splitting a character in half.
fn truncate_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_spellings_of_one_word_collide() {
        // The macOS case: the same filename, composed and decomposed. They
        // render identically, so two entries would look like one duplicated
        // for no reason.
        let composed = "ștampilă.pdf";
        let decomposed: String = composed.nfd().collect();
        assert_ne!(composed, decomposed.as_str(), "the test needs both forms");

        assert_eq!(sanitize(composed), sanitize(&decomposed));
        assert_eq!(fold(composed), fold(&decomposed));
    }

    #[test]
    fn names_differing_only_in_case_collide() {
        // Legal on Linux, impossible to export on Windows.
        assert_eq!(fold("Raport.pdf"), fold("raport.PDF"));
        assert_ne!(fold("raport.pdf"), fold("raportul.pdf"));
    }

    #[test]
    fn characters_windows_refuses_are_replaced() {
        assert_eq!(sanitize("a:b?c*d|e"), "a_b_c_d_e");
        assert_eq!(sanitize("with/slash"), "with_slash");
        assert_eq!(sanitize("tab\there"), "tab_here");
    }

    #[test]
    fn a_trailing_dot_or_space_is_removed() {
        // Windows silently drops these on create, so keeping them would mean
        // the vault and the disk disagree about the name.
        assert_eq!(sanitize("report."), "report");
        assert_eq!(sanitize("report   "), "report");
        assert_eq!(sanitize("report. . ."), "report");
    }

    #[test]
    fn a_leading_dot_is_kept() {
        // `.gitignore` is a real name, not a mistake.
        assert_eq!(sanitize(".gitignore"), ".gitignore");
    }

    #[test]
    fn reserved_device_names_are_escaped() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("com9.log"), "_com9.log");
        // Not reserved, merely similar.
        assert_eq!(sanitize("console.log"), "console.log");
        assert_eq!(sanitize("COM10"), "COM10");
    }

    #[test]
    fn a_name_too_long_is_cut_on_a_character_boundary() {
        let long = "ă".repeat(300);
        let cut = sanitize(&long);
        assert!(cut.len() <= MAX_NAME_BYTES);
        // The real assertion: it is still valid UTF-8 with whole characters,
        // which `String` guarantees only if the cut landed correctly.
        assert!(cut.chars().all(|c| c == 'ă'));
    }

    #[test]
    fn nothing_usable_still_produces_a_name() {
        // An entry with no name at all has no row to show and no way to be
        // renamed back into existence.
        assert_eq!(sanitize(""), "unnamed");
        assert_eq!(sanitize("   "), "unnamed");
        assert_eq!(sanitize("..."), "unnamed");
    }

    #[test]
    fn sanitizing_twice_changes_nothing() {
        // Replay applies this on every device and every rebuild. If it were
        // not idempotent, a name would drift a little further each pass.
        for raw in [
            "report.",
            "a:b",
            "CON",
            "  spaced  ",
            "",
            &"x".repeat(400),
            "ștampilă.pdf",
        ] {
            let once = sanitize(raw);
            assert_eq!(sanitize(&once), once, "not idempotent for {raw:?}");
        }
    }

    #[test]
    fn what_check_accepts_sanitize_leaves_alone() {
        // The two must agree, or the app rejects names it would have stored
        // fine, or accepts names it then quietly rewrites.
        for good in ["report.pdf", ".gitignore", "Ștampilă 2026.pdf", "a b c"] {
            assert!(check(good).is_ok(), "{good:?} should be accepted");
            assert_eq!(sanitize(good), good, "{good:?} should be stored as typed");
        }
    }

    #[test]
    fn check_says_what_is_wrong() {
        for bad in ["", "   ", "a/b", "a:b", "report.", "CON", "nul.txt"] {
            assert!(check(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(check(&"x".repeat(256)).is_err());
    }
}
