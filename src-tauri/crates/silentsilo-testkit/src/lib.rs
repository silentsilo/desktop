//! What the suites need to test the two things a clean temporary directory
//! cannot show: real storage, and a hostile environment. Dev-dependency only.

use std::path::Path;

/// Set in the release lane, where a suite that skips itself is a failure:
/// green has to mean the tests ran.
const REQUIRE: &str = "SILENTSILO_TEST_REQUIRE_BACKENDS";

/// Reports a suite standing down for want of an endpoint, or fails when the
/// caller has declared that endpoints must be there.
pub fn skip_or_fail(what: &str) {
    if std::env::var(REQUIRE).is_ok() {
        panic!(
            "{what}, and {REQUIRE} is set: the release lane requires every backend to be \
             reachable. Start them with scripts/test-local.ps1, or unset {REQUIRE}."
        );
    }
    eprintln!("skipped: {what}");
}

/// Holds a file open the way a scanner does just after a write. On Windows
/// a rename onto a held destination fails outright, which is how every
/// durable write once became "Access is denied".
pub struct HeldOpen(#[allow(dead_code)] std::fs::File);

impl HeldOpen {
    /// Opens `path` for reading and keeps the handle until dropped. The
    /// share mode is the ordinary one: readers do not stop a plain write,
    /// only a rename onto the file.
    pub fn reading(path: &Path) -> Self {
        Self(std::fs::File::open(path).expect("the file to hold open must exist"))
    }
}

/// Puts a directory where a file is about to be written, making "cannot
/// create here" deterministic. It is a separate failure from "cannot change
/// what is already there".
pub struct BlockedPath(std::path::PathBuf);

impl BlockedPath {
    pub fn at(path: &Path) -> Self {
        std::fs::create_dir_all(path).expect("blocking the path");
        Self(path.to_path_buf())
    }
}

impl Drop for BlockedPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
