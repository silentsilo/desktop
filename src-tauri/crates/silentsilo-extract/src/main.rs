//! `silentsilo-extract`: get your files out with a recovery code and nothing
//! else.
//!
//! Arguments are parsed by hand rather than with a library. This binary is
//! the answer to "what if the project disappears", so its dependency list is
//! part of what it promises: the fewer things between a person and their
//! files, the more of that promise survives. Six flags do not need a parser.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
silentsilo-extract  ·  read a SilentSilo backup with only a recovery code

  silentsilo-extract list    --from <folder> --code <recovery code>
  silentsilo-extract extract --from <folder> --code <recovery code> --to <folder>

  --from   the backup: a folder holding vault.json, ops/ and blobs/.
           A bucket works too, once mirrored to a folder with rclone or
           the like. What is copied down is ciphertext throughout.
  --code   the 32-character code from your emergency kit. Dashes and case
           do not matter.
  --to     where to write the files. Created if it is not there.

Nothing is written anywhere else, no silo is created, and nothing is sent
over the network beyond reading the backup you named.
";

struct Args {
    command: String,
    from: Option<PathBuf>,
    code: Option<String>,
    to: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut raw = std::env::args().skip(1);
    let command = raw.next().unwrap_or_default();
    let mut args = Args {
        command,
        from: None,
        code: None,
        to: None,
    };
    while let Some(flag) = raw.next() {
        let mut value = || {
            raw.next()
                .ok_or_else(|| format!("{flag} needs a value after it"))
        };
        match flag.as_str() {
            "--from" => args.from = Some(PathBuf::from(value()?)),
            "--code" => args.code = Some(value()?),
            "--to" => args.to = Some(PathBuf::from(value()?)),
            other => return Err(format!("I do not know the option {other}")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    if !matches!(args.command.as_str(), "list" | "extract") {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }

    let (Some(from), Some(code)) = (args.from.clone(), args.code.clone()) else {
        eprintln!("Both --from and --code are needed.\n\n{USAGE}");
        return ExitCode::FAILURE;
    };
    if !from.is_dir() {
        eprintln!("There is no folder at {}.", from.display());
        return ExitCode::FAILURE;
    }
    if args.command == "extract" && args.to.is_none() {
        eprintln!("Extracting needs --to, a folder to write into.\n\n{USAGE}");
        return ExitCode::FAILURE;
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Could not start: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(args, from, code))
}

async fn run(args: Args, from: PathBuf, code: String) -> ExitCode {
    let store = silentsilo_extract::folder_store(&from);

    let backup = match silentsilo_extract::open(&*store, &code).await {
        Ok(backup) => backup,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "Silo {} · {} files, {} in the trash",
        backup.vault_id,
        backup.file_count(),
        backup.trashed_count()
    );

    if args.command == "list" {
        for line in silentsilo_extract::describe(&backup) {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    }

    let dest = args.to.expect("checked above");
    if let Err(e) = std::fs::create_dir_all(&dest) {
        eprintln!("Could not make {}: {e}", dest.display());
        return ExitCode::FAILURE;
    }

    // Progress on one rewritten line: this runs for a long time on a large
    // silo, and a page of scrolling filenames is not progress.
    let mut last = 0usize;
    let result = silentsilo_extract::extract(&backup, &*store, &dest, &mut |done, total, _| {
        if done == total || done >= last + 25 {
            last = done;
            println!("  {done} of {total}");
        }
    })
    .await;

    match result {
        Ok(out) => {
            println!("Wrote {} files to {}", out.written, dest.display());
            if out.failed.is_empty() {
                return ExitCode::SUCCESS;
            }
            // Named, not counted, and on stderr: this is the part someone
            // has to act on, and it should survive being piped to a file.
            eprintln!("\n{} could not be written:", out.failed.len());
            for (path, why) in &out.failed {
                eprintln!("  {path}: {why}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
