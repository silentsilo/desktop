//! Replacing a file with a temporary one that holds its new contents.
//!
//! Writing through a temp file and renaming is what makes a write atomic: a
//! crash leaves either the old file or the new one, never half of either.
//! On Windows that rename is `MoveFileEx`, and it fails outright with
//! `ERROR_ACCESS_DENIED` while *any* other process holds the destination
//! open, even for reading. A real-time antivirus scanner does exactly that,
//! for a few milliseconds, on the files an app has just written; so does a
//! second instance of the app reading the same settings. A plain in-place
//! write survives that, which is why swapping one for the other turned
//! "silo created" into "Access is denied" on machines with a strict
//! scanner.
//!
//! So the rename is retried for as long as a scanner plausibly holds a
//! handle, and only then, for callers that ask, falls back to writing in
//! place. The fallback gives up atomicity for that one write, which is the
//! lesser loss: refusing the operation outright is a guaranteed failure
//! now, while a torn write needs a crash inside the same handful of
//! milliseconds.

use std::path::Path;

/// Roughly a second in total, in growing steps. A scanner's handle is gone
/// in well under that; a genuinely locked file is not going to free up by
/// waiting longer, and the caller has a better answer than a frozen window.
const RETRY_DELAYS_MS: [u64; 7] = [1, 2, 5, 10, 50, 200, 700];

/// Whether this failure is the kind another process causes by holding the
/// destination open, rather than something waiting will not fix.
fn is_transient(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ResourceBusy
    ) || err.raw_os_error() == Some(32) // ERROR_SHARING_VIOLATION
}

/// Renames `temp` onto `dest`, retrying while another process is holding
/// `dest` open. Returns the last error if the window expires.
pub fn rename_with_retry(temp: &Path, dest: &Path) -> std::io::Result<()> {
    let mut last = match std::fs::rename(temp, dest) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    for delay in RETRY_DELAYS_MS {
        if !is_transient(&last) {
            return Err(last);
        }
        std::thread::sleep(std::time::Duration::from_millis(delay));
        match std::fs::rename(temp, dest) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Writes `bytes` to `dest` atomically where the filesystem allows it.
///
/// The bytes reach a sibling temp file, are flushed to the platter, and
/// take the destination's name by rename. `open_temp` lets the caller
/// create that temp file with permissions of its own; the default opens it
/// like any other file.
///
/// When the rename cannot go through because something else is holding the
/// destination, the contents are written in place instead: an app that
/// refuses to save while an antivirus reads the file it just wrote is worse
/// than one that saves the ordinary way.
pub fn write_replacing(
    dest: &Path,
    bytes: &[u8],
    open_temp: impl Fn(&Path) -> std::io::Result<std::fs::File>,
) -> std::io::Result<()> {
    use std::io::Write;

    let temp = {
        let mut name = dest.as_os_str().to_os_string();
        name.push(".tmp");
        std::path::PathBuf::from(name)
    };

    let staged = (|| -> std::io::Result<()> {
        let mut file = open_temp(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();

    // Writing the temp file can fail on its own: creating a file needs
    // permission on the directory, which protection software withholds
    // separately from permission to change a file already there. So the
    // fallback covers that too, rather than only a refused rename.
    let outcome = match staged {
        Ok(()) => rename_with_retry(&temp, dest),
        Err(e) => Err(e),
    };

    match outcome {
        Ok(()) => Ok(()),
        Err(e) if is_transient(&e) => {
            let _ = std::fs::remove_file(&temp);
            let mut file = open_temp(dest)?;
            file.write_all(bytes)?;
            file.sync_all()
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::File::create(path)
    }

    #[test]
    fn an_ordinary_write_replaces_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("settings.json");
        std::fs::write(&dest, b"old").unwrap();

        write_replacing(&dest, b"new", plain).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(
            !dir.path().join("settings.json.tmp").exists(),
            "the temp file outlived the write"
        );
    }

    /// The regression this module exists for. A reader holding the
    /// destination open is what an antivirus scanner does to a file the app
    /// has just written, and on Windows it makes the rename fail outright.
    /// The write still has to land.
    #[test]
    fn a_reader_holding_the_destination_does_not_stop_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("silos.json");
        std::fs::write(&dest, b"old").unwrap();

        let reader = std::fs::File::open(&dest).unwrap();
        write_replacing(&dest, b"new", plain).expect("a concurrent reader must not fail the write");
        drop(reader);

        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!dir.path().join("silos.json.tmp").exists());
    }

    /// The temp file is not always creatable: protection software can
    /// withhold "create a file here" while still allowing an existing file
    /// to be changed. The write has to take the second route rather than
    /// giving up.
    #[test]
    fn a_temp_file_that_cannot_be_created_falls_back_to_writing_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("silos.json");
        std::fs::write(&dest, b"old").unwrap();
        // A directory sitting where the temp file wants to be: creating the
        // temp file is refused, deterministically.
        std::fs::create_dir(dir.path().join("silos.json.tmp")).unwrap();

        write_replacing(&dest, b"new", plain).expect("the write must still land");

        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    }

    #[test]
    fn a_missing_directory_is_still_an_error_rather_than_a_retry_loop() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nowhere").join("settings.json");

        assert!(write_replacing(&dest, b"x", plain).is_err());
    }
}
