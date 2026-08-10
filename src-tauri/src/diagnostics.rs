//! The one place this app writes a diagnostic, so where they go in a
//! shipped build stays a single edit. Whatever answers that has to reckon
//! with what these lines contain: an import failure names the file it
//! skipped, and a file name is vault content. Until a sink is chosen
//! deliberately these stay on stderr, where they vanish with the process.

use std::fmt::Display;

/// Something went wrong in a place that carries on regardless.
///
/// `area` is the operation, not the file: "sync", "trash", "import". It is
/// what makes a line searchable without putting the user's own words in it.
pub fn warn(area: &str, detail: impl Display) {
    eprintln!("[{area}] {detail}");
}
