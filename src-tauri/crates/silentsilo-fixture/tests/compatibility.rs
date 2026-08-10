//! Opens every silo any past release wrote and checks it still says the
//! same thing: the test the whole format freeze exists for. The answer to
//! a failure is never to update the fixture, which is the released format;
//! changing it only hides the break from the people who will hit it. A new
//! directory appears per format change, and old ones are never removed.

use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .expect("the fixtures directory must exist")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[tokio::test]
async fn every_released_format_still_opens() {
    let dirs = fixture_dirs();
    assert!(
        !dirs.is_empty(),
        "no fixtures found: the suite is passing by having nothing to check"
    );

    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let expected = std::fs::read_to_string(dir.join("expected.txt"))
            .unwrap_or_else(|e| panic!("{name}: cannot read expected.txt: {e}"));

        let actual = silentsilo_fixture::digest_from_store(&dir, silentsilo_fixture::FIXTURE_CODE)
            .await
            .unwrap_or_else(|e| panic!("{name}: cannot rebuild the silo from storage: {e}"));

        let expected: Vec<&str> = expected.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            expected, actual,
            "{name}: this build reads a silo written by {name} differently.\n\
             The fixture is the released format. Fix the code, not the fixture."
        );
    }
}

#[test]
fn the_silo_folder_an_old_release_left_behind_still_unlocks() {
    // The other test rebuilds from the bucket, the disaster path. This is
    // the everyday one: the app updates and the silo already on disk has to
    // unlock. The two must agree on what the vault holds.
    for dir in fixture_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let expected = std::fs::read_to_string(dir.join("expected.txt"))
            .unwrap_or_else(|e| panic!("{name}: cannot read expected.txt: {e}"));

        // A copy: unlocking writes the working copy for the path it opens,
        // and the committed silo stays exactly as the release wrote it.
        let scratch = tempfile::tempdir().unwrap();
        let silo = scratch.path().join("silo");
        copy_dir(&silentsilo_fixture::silo_dir(&dir), &silo)
            .unwrap_or_else(|e| panic!("{name}: cannot copy the silo: {e}"));

        let actual = silentsilo_fixture::digest_from_silo(&silo, silentsilo_fixture::FIXTURE_CODE)
            .unwrap_or_else(|e| panic!("{name}: the silo folder no longer unlocks: {e}"));

        // The op count is the one line the two halves are allowed to differ
        // on: compaction prunes the store, the local log keeps everything.
        // Never the other way around.
        let expected: Vec<&str> = expected
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with("ops count="))
            .collect();
        let (tree, local_ops): (Vec<&str>, Vec<&str>) = actual
            .iter()
            .map(String::as_str)
            .partition(|l| !l.starts_with("ops count="));
        assert_eq!(
            expected, tree,
            "{name}: this build reads a silo folder written by {name} differently.\n\
             The fixture is the released format. Fix the code, not the fixture."
        );
        let store_count = ops_count(&std::fs::read_to_string(dir.join("expected.txt")).unwrap());
        let local_count = ops_count(&local_ops.join("\n"));
        assert!(
            local_count >= store_count,
            "{name}: the local log holds {local_count} records, the store {store_count}"
        );

        assert!(
            silentsilo_fixture::digest_from_silo(&silo, "0000-0000-0000-0000-0000-0000-0000-0000")
                .is_err(),
            "{name}: the silo folder opened without the right code"
        );
    }
}

fn ops_count(digest: &str) -> u64 {
    digest
        .lines()
        .find_map(|l| l.strip_prefix("ops count=")?.parse().ok())
        .unwrap_or(0)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn the_wrong_code_opens_nothing() {
    // The fixtures are sealed with a known code, which is only safe because
    // they contain nothing real. Worth asserting anyway: a fixture that
    // opens without its code would mean the envelope is not doing its job.
    for dir in fixture_dirs() {
        assert!(
            silentsilo_fixture::digest_from_store(&dir, "0000-0000-0000-0000-0000-0000-0000-0000")
                .await
                .is_err(),
            "{}: opened without the right code",
            dir.display()
        );
    }
}
