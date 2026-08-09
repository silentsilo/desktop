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
