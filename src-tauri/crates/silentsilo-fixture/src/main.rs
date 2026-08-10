//! Writes a fixture, or reads one back.
//!
//! ```text
//! cargo run -p silentsilo-fixture -- create crates/silentsilo-fixture/fixtures/v1.0.0
//! cargo run -p silentsilo-fixture -- digest crates/silentsilo-fixture/fixtures/v1.0.0
//! ```
//!
//! Paths are relative to `src-tauri/`. The compacted twin of a fixture is
//! written with `create-compacted` into the directory of the same name with
//! a `-compacted` suffix.
//!
//! `create` is run once per release that changes a persisted format, and its
//! output is committed. `digest` prints what a fixture contains, which is
//! what `expected.txt` next to it holds and what the compatibility test
//! compares against.

use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let dir: PathBuf = args
        .next()
        .ok_or_else(|| usage("a directory is required"))?
        .into();

    match command.as_str() {
        "create" | "create-compacted" => {
            if dir.exists() {
                // Overwriting would produce a fixture that is half one
                // release and half another, which is worse than no fixture.
                return Err(format!("{} already exists", dir.display()));
            }
            if command == "create-compacted" {
                silentsilo_fixture::create_compacted(&dir).await?;
            } else {
                silentsilo_fixture::create(&dir).await?;
            }
            let lines =
                silentsilo_fixture::digest_from_store(&dir, silentsilo_fixture::FIXTURE_CODE)
                    .await?;
            std::fs::write(dir.join("expected.txt"), lines.join("\n") + "\n")
                .map_err(|e| e.to_string())?;
            println!("wrote {} ({} lines)", dir.display(), lines.len());
        }
        "digest" => {
            let lines =
                silentsilo_fixture::digest_from_store(&dir, silentsilo_fixture::FIXTURE_CODE)
                    .await?;
            println!("{}", lines.join("\n"));
        }
        other => return Err(usage(&format!("unknown command {other:?}"))),
    }
    Ok(())
}

fn usage(problem: &str) -> String {
    format!("{problem}\n\nusage: silentsilo-fixture <create|create-compacted|digest> <directory>")
}
