//! The one place this app writes a diagnostic, so where they go in a
//! shipped build stays a single edit. Whatever answers that has to reckon
//! with what these lines contain: an import failure names the file it
//! skipped, and a file name is vault content. Until a sink is chosen
//! deliberately these stay on stderr, where they vanish with the process.
//! On Linux stderr can end up in the journal, so the home folder is written
//! as `~` and callers pass errors rather than paths.

use std::fmt::Display;
use std::io::Write;

/// Something went wrong in a place that carries on regardless.
///
/// `area` is the operation, not the file: "sync", "trash", "import". It is
/// what makes a line searchable without putting the user's own words in it.
pub fn warn(area: &str, detail: impl Display) {
    let line = without_home(&detail.to_string(), home().as_deref());
    // Never `eprintln!`: it panics when the write fails, a closed pipe for
    // one, and a diagnostic must not take the app down.
    let _ = writeln!(std::io::stderr(), "[{area}] {line}");
}

fn home() -> Option<String> {
    std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok()
        .filter(|h| h.len() > 1)
}

/// The line with the user's home folder shortened to `~`.
fn without_home(line: &str, home: Option<&str>) -> String {
    match home {
        Some(home) => line.replace(home, "~"),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::without_home;

    #[test]
    fn the_home_folder_is_not_written_out() {
        assert_eq!(
            without_home(
                "skipped: /home/alex/Documents/tax.pdf is locked",
                Some("/home/alex")
            ),
            "skipped: ~/Documents/tax.pdf is locked"
        );
        assert_eq!(without_home("no path here", None), "no path here");
    }
}
