//! `silentsilo-browser-host`: the native messaging host the browser starts
//! for the SilentSilo extension.
//!
//! Chrome, Edge and Brave run it as `silentsilo-browser-host <extension
//! origin> --parent-window=<n>`; Firefox as `silentsilo-browser-host
//! <manifest path> <add-on id>`. It refuses any extension not on the list
//! for its form and, in a release build, any start that did not come from a
//! browser of that kind. Then it opens the app's pipe, checks the app is
//! what serves it, and relays whole frames between stdio and the pipe until
//! either side closes. When the pipe is not there, or is not the app's, it
//! answers `app-not-running` itself. It never starts the app.
//!
//! `silentsilo-browser-host --write-manifest` writes both manifests the
//! browsers read, beside the executable. On Linux, `--install-manifests`
//! writes them where each browser of this user looks, naming this
//! executable, and `--remove-manifests` takes them away; the app runs both
//! from its Settings toggle. `--registers <chrome|edge|firefox>`
//! exits 0 when that browser's registry key should point at them, 1 when its
//! list is empty. The installer runs both. `--check-release` says whether
//! this build is fit to ship.

use std::process::ExitCode;

use silentsilo_browser_host::{
    FIREFOX_MANIFEST_FILE, MANIFEST_FILE, allowed_caller, allowed_firefox_ids, allowed_origins,
    firefox_manifest, manifest, registers, this_build_release_problems,
};

fn main() -> ExitCode {
    silentsilo_shell::harden_process();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--write-manifest") => return write_manifests(),
        Some("--check-release") => return check_release(),
        Some("--registers") => return registers_key(args.get(1).map(String::as_str)),
        #[cfg(unix)]
        Some("--install-manifests") => return user_manifests::install(),
        #[cfg(unix)]
        Some("--remove-manifests") => return user_manifests::remove(),
        _ => {}
    }
    let Some(engine) = allowed_caller(&args, &allowed_origins(), &allowed_firefox_ids()) else {
        eprintln!("silentsilo-browser-host: this extension is not allowed");
        return ExitCode::from(2);
    };
    // Debug builds are started by tests, not by a browser.
    #[cfg(all(windows, not(debug_assertions)))]
    if let Err(reason) = silentsilo_browser_host::started_by_browser(engine) {
        eprintln!("silentsilo-browser-host: refused: {reason}");
        return ExitCode::from(3);
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    let _ = engine;
    relay::run()
}

fn write_manifests() -> ExitCode {
    let written = std::env::current_exe().and_then(|exe| {
        std::fs::write(
            exe.with_file_name(MANIFEST_FILE),
            manifest(&exe, &allowed_origins()),
        )?;
        std::fs::write(
            exe.with_file_name(FIREFOX_MANIFEST_FILE),
            firefox_manifest(&exe, &allowed_firefox_ids()),
        )
    });
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("silentsilo-browser-host: could not write the manifests: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
mod user_manifests {
    use std::path::PathBuf;
    use std::process::ExitCode;

    use silentsilo_browser_host::{
        USER_MANIFEST_FILE, allowed_firefox_ids, allowed_origins, firefox_manifest, manifest,
        registers, user_manifest_places,
    };

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    }

    /// Each browser of this user whose list has an id gets a manifest naming
    /// this executable; one whose list is empty gets none.
    pub fn install() -> ExitCode {
        let (Some(home), Ok(exe)) = (home(), std::env::current_exe()) else {
            eprintln!("silentsilo-browser-host: no home directory or executable path");
            return ExitCode::FAILURE;
        };
        let mut failed = false;
        for (key, dir) in user_manifest_places(&home) {
            let path = dir.join(USER_MANIFEST_FILE);
            if registers(key) != Some(true) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let body = if key == "firefox" {
                firefox_manifest(&exe, &allowed_firefox_ids())
            } else {
                manifest(&exe, &allowed_origins())
            };
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, body))
            {
                eprintln!("silentsilo-browser-host: {}: {e}", path.display());
                failed = true;
            }
        }
        if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }

    /// Takes away every manifest `install` may have written.
    pub fn remove() -> ExitCode {
        let Some(home) = home() else {
            return ExitCode::FAILURE;
        };
        for (_, dir) in user_manifest_places(&home) {
            let _ = std::fs::remove_file(dir.join(USER_MANIFEST_FILE));
        }
        ExitCode::SUCCESS
    }
}

fn registers_key(key: Option<&str>) -> ExitCode {
    match key.and_then(registers) {
        Some(true) => ExitCode::SUCCESS,
        Some(false) => ExitCode::from(1),
        None => {
            eprintln!("silentsilo-browser-host: --registers takes chrome, edge or firefox");
            ExitCode::from(2)
        }
    }
}

fn check_release() -> ExitCode {
    let problems = this_build_release_problems();
    for problem in &problems {
        eprintln!("silentsilo-browser-host: not fit to ship: {problem}");
    }
    if problems.is_empty() {
        println!("allowed (Chromium): {}", allowed_origins().join(" "));
        println!("allowed (Firefox): {}", allowed_firefox_ids().join(" "));
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(windows)]
mod relay {
    use std::os::windows::io::AsRawHandle;
    use std::process::ExitCode;
    use std::time::Duration;

    use silentsilo_browser_host::{NOT_OURS, NOT_RUNNING, expected_server, verify_server};
    use silentsilo_shell::browser_pipe::pipe_name;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

    use super::frames::{answer_alone, bridge};

    const ERROR_PIPE_BUSY: i32 = 231;
    /// The server may identify this client but never act as it.
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;

    pub fn run() -> ExitCode {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return ExitCode::FAILURE;
        };
        runtime.block_on(async {
            match connect().await {
                Some(pipe) => match check(&pipe) {
                    Ok(()) => bridge(pipe).await,
                    Err(reason) => {
                        eprintln!("silentsilo-browser-host: not the app's pipe: {reason}");
                        drop(pipe);
                        answer_alone(NOT_OURS).await;
                    }
                },
                None => answer_alone(NOT_RUNNING).await,
            }
        });
        // Exited outright: the stdin reader sits in a blocking read the
        // runtime would otherwise wait for.
        std::process::exit(0)
    }

    /// The app's pipe, or `None` when nothing listens on it. A busy pipe
    /// (every instance taken for a moment) is waited out briefly.
    async fn connect() -> Option<NamedPipeClient> {
        let name = pipe_name().ok()?;
        for _ in 0..40 {
            match ClientOptions::new()
                .security_qos_flags(SECURITY_IDENTIFICATION)
                .open(&name)
            {
                Ok(pipe) => return Some(pipe),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(_) => return None,
            }
        }
        None
    }

    /// Nothing is written to the pipe before this passes: whoever made a
    /// pipe of this name first would otherwise read the requests.
    fn check(pipe: &NamedPipeClient) -> Result<(), String> {
        let expected = expected_server().map_err(|e| e.to_string())?;
        verify_server(pipe.as_raw_handle(), &expected)
    }
}

/// Frames between the browser's stdio and the app, whatever carries them.
#[cfg(any(windows, unix))]
mod frames {
    use silentsilo_browser_host::{MALFORMED, TOO_LARGE, error_answer, not_running_answer};
    use silentsilo_shell::browser_pipe::{Frame, read_frame, write_frame};
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio::sync::mpsc;
    use zeroize::Zeroize;

    /// No app to relay to: each request gets `app-not-running` with
    /// `message`, until the browser closes stdin.
    pub async fn answer_alone(message: &str) {
        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        while let Ok(Some(frame)) = read_frame(&mut stdin).await {
            let answer = match frame {
                Frame::Message(mut request) => {
                    let answer = not_running_answer(&request, message);
                    request.zeroize();
                    answer
                }
                Frame::TooLarge => error_answer("", "bad-request", TOO_LARGE),
            };
            if write_frame(&mut stdout, &answer).await.is_err() {
                break;
            }
        }
    }

    /// Frames both ways, unread, until either side closes. Every frame is
    /// wiped once passed on: an answer to a fill carries a password.
    pub async fn bridge<T: AsyncRead + AsyncWrite>(pipe: T) {
        let (mut from_app, mut to_app) = tokio::io::split(pipe);
        let (out, mut outgoing) = mpsc::channel::<Vec<u8>>(8);

        let write_out = async move {
            let mut stdout = tokio::io::stdout();
            while let Some(mut frame) = outgoing.recv().await {
                let written = write_frame(&mut stdout, &frame).await;
                frame.zeroize();
                if written.is_err() {
                    break;
                }
            }
            // Answers queued behind a failed write, a fill's among them.
            outgoing.close();
            while let Ok(mut frame) = outgoing.try_recv() {
                frame.zeroize();
            }
        };

        let to_browser = out.clone();
        let up = async move {
            let mut stdin = tokio::io::stdin();
            loop {
                match read_frame(&mut stdin).await {
                    Ok(Some(Frame::Message(mut request))) => {
                        let sent = write_frame(&mut to_app, &request).await;
                        request.zeroize();
                        if sent.is_err() {
                            break;
                        }
                    }
                    Ok(Some(Frame::TooLarge)) => {
                        let refusal = error_answer("", "bad-request", TOO_LARGE);
                        if to_browser.send(refusal).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        };

        let down = async move {
            loop {
                let frame = match read_frame(&mut from_app).await {
                    Ok(Some(Frame::Message(answer))) => answer,
                    Ok(Some(Frame::TooLarge)) => error_answer("", "bad-request", MALFORMED),
                    _ => break,
                };
                if let Err(mpsc::error::SendError(mut unsent)) = out.send(frame).await {
                    unsent.zeroize();
                    break;
                }
            }
        };

        // Whichever side closes first ends both directions; what is already
        // queued for the browser is still written before exit.
        let relay = async move {
            tokio::select! {
                _ = up => {}
                _ = down => {}
            }
        };
        tokio::join!(write_out, relay);
    }
}

#[cfg(unix)]
mod relay {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::process::ExitCode;

    use silentsilo_browser_host::{NOT_OURS, NOT_RUNNING};
    use silentsilo_shell::browser_pipe::socket_path;
    use tokio::net::UnixStream;

    use super::frames::{answer_alone, bridge};

    pub fn run() -> ExitCode {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return ExitCode::FAILURE;
        };
        runtime.block_on(async {
            match UnixStream::connect(match socket_path() {
                Ok(path) => path,
                Err(_) => return answer_alone(NOT_RUNNING).await,
            })
            .await
            {
                Ok(stream) => match check(&stream) {
                    Ok(()) => bridge(stream).await,
                    Err(reason) => {
                        eprintln!("silentsilo-browser-host: not the app's socket: {reason}");
                        drop(stream);
                        answer_alone(NOT_OURS).await;
                    }
                },
                Err(_) => answer_alone(NOT_RUNNING).await,
            }
        });
        // Exited outright: the stdin reader sits in a blocking read the
        // runtime would otherwise wait for.
        std::process::exit(0)
    }

    /// Nothing is written before this passes. The server must be this user,
    /// and the socket's directory this user's alone: nothing else can then
    /// have made the socket. Who started the host proves nothing here; a
    /// browser may start it through a portal or a sandbox helper.
    fn check(stream: &UnixStream) -> Result<(), String> {
        // SAFETY: getuid cannot fail and touches no memory.
        let me = unsafe { libc::getuid() };
        let cred = stream.peer_cred().map_err(|e| e.to_string())?;
        if cred.uid() != me {
            return Err("the server runs as another user".into());
        }
        let path = socket_path().map_err(|e| e.to_string())?;
        let dir = path.parent().ok_or("no socket directory")?;
        let meta = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.uid() != me || meta.permissions().mode() & 0o077 != 0 {
            return Err("the socket's directory is not this user's alone".into());
        }
        Ok(())
    }
}

#[cfg(not(any(windows, unix)))]
mod relay {
    use std::process::ExitCode;

    pub fn run() -> ExitCode {
        eprintln!("silentsilo-browser-host: only Windows is supported for now");
        ExitCode::FAILURE
    }
}
