//! The channel between the browser extension's native host and the app.
//!
//! The browser starts `silentsilo-browser-host`, which relays native
//! messaging frames to a named pipe this module serves:
//! `\\.\pipe\silentsilo-browser-<user SID>`, reachable by the current user
//! only. Both sides frame the same way the browser does (a 32-bit
//! little-endian length, then UTF-8 JSON), so the host copies frames without
//! reading them. The protocol itself is the app's business; this module only
//! moves frames. The contract is `docs/PROTOCOL.md` in silentsilo/browser.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest frame either side accepts, in bytes of JSON.
pub const MAX_FRAME: usize = 64 * 1024;

/// One frame as it came off the wire.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    Message(Vec<u8>),
    /// Over [`MAX_FRAME`]. Its bytes were read and dropped, so the stream is
    /// still in step; the sender gets a refusal rather than a hang.
    TooLarge,
}

/// Reads one frame. `None` means the other side closed between frames.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Frame>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as u64;
    if len > MAX_FRAME as u64 {
        let skipped = tokio::io::copy(&mut reader.take(len), &mut tokio::io::sink()).await?;
        if skipped < len {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        return Ok(Some(Frame::TooLarge));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).await?;
    Ok(Some(Frame::Message(body)))
}

/// Writes one frame and flushes it. Refuses anything over [`MAX_FRAME`].
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame over the size limit",
        ));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Whether the user turned the browser extension on. Off by default: the
/// pipe exists only while this says yes.
pub fn extension_enabled() -> bool {
    std::fs::read_to_string(setting_path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("enabled").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

pub fn set_extension_enabled(enabled: bool) -> io::Result<()> {
    let path = setting_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::json!({ "enabled": enabled }).to_string())
}

/// Beside `shell-integration.json`, so the uninstaller's "delete the
/// application data" box takes it and an update keeps it.
fn setting_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SilentSilo")
        .join("browser-extension.json")
}

#[cfg(windows)]
pub use imp::{ClientCheck, HOST_EXE, PipeServer, current_user_sid, pipe_name};

#[cfg(windows)]
mod imp {
    use std::future::Future;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    use ::windows::Win32::Foundation::{HLOCAL, LocalFree};
    use ::windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use ::windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use ::windows::core::HSTRING;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::sync::{Semaphore, mpsc, watch};
    use tokio::task::JoinSet;
    use zeroize::Zeroize;

    use super::{Frame, read_frame, write_frame};
    use crate::win_process;

    pub use crate::win_process::current_user_sid;

    /// At most this many connections at once. The browser starts one host
    /// per port, and an extension needs one or two.
    const MAX_INSTANCES: usize = 16;

    /// Requests one connection may have in progress. Past that the server
    /// stops reading from it until one finishes.
    const MAX_IN_FLIGHT: usize = 4;

    /// How long the server waits before trying again to create an instance
    /// (all of them taken, say), doubling up to the second value.
    const RETRY_FIRST: Duration = Duration::from_millis(100);
    const RETRY_MAX: Duration = Duration::from_secs(5);

    /// The host's file name, installed beside the app.
    pub const HOST_EXE: &str = "silentsilo-browser-host.exe";

    /// `\\.\pipe\silentsilo-browser-<SID>`. Per user, so two people signed
    /// in to one machine never reach each other's app.
    pub fn pipe_name() -> io::Result<String> {
        Ok(format!(
            r"\\.\pipe\silentsilo-browser-{}",
            current_user_sid()?
        ))
    }

    /// Which process may talk to the app over the pipe: this user's, running
    /// one executable, and in release builds signed with the same
    /// certificate as the app itself.
    #[derive(Clone, Debug)]
    pub struct ClientCheck {
        pub image: PathBuf,
        pub same_signer: bool,
    }

    impl ClientCheck {
        /// The host installed beside this executable. Its signature is
        /// checked in release builds only: a debug build is unsigned.
        pub fn host_beside_this_exe() -> io::Result<Self> {
            Ok(Self {
                image: std::env::current_exe()?.with_file_name(HOST_EXE),
                same_signer: cfg!(not(debug_assertions)),
            })
        }

        /// Whether the process `pid` passes, or why not.
        pub fn admit(&self, pid: u32) -> Result<(), String> {
            let fail = |what: &str, e: io::Error| format!("{what}: {e}");
            let user = win_process::user_sid(pid).map_err(|e| fail("client user", e))?;
            if user != current_user_sid().map_err(|e| fail("own user", e))? {
                return Err("the client runs as another user".into());
            }
            let image = win_process::image_path(pid).map_err(|e| fail("client image", e))?;
            if !win_process::same_file(&image, &self.image) {
                return Err(format!(
                    "the client is {}, not {}",
                    image.display(),
                    self.image.display()
                ));
            }
            if self.same_signer {
                let theirs =
                    win_process::signer(&image).map_err(|e| fail("client signature", e))?;
                if own_signer()? != &theirs.certificate {
                    return Err(format!(
                        "the client is signed by {}, not with this app's certificate",
                        theirs.name
                    ));
                }
            }
            Ok(())
        }
    }

    /// This executable's signing certificate, read once.
    fn own_signer() -> Result<&'static Vec<u8>, String> {
        static OWN: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();
        OWN.get_or_init(|| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            win_process::signer(&exe)
                .map(|s| s.certificate)
                .map_err(|e| format!("this app's own signature: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
    }

    /// A security descriptor granting the current user, and nobody else,
    /// full access. `P` protects the DACL from inheriting anything.
    struct UserOnly {
        sd: PSECURITY_DESCRIPTOR,
        sid: String,
    }

    // SAFETY: the descriptor is immutable once built and only read by
    // CreateNamedPipe; LocalFree on drop is the only other use.
    unsafe impl Send for UserOnly {}
    unsafe impl Sync for UserOnly {}

    impl UserOnly {
        fn new(sid: &str) -> io::Result<Self> {
            let sddl = HSTRING::from(format!("O:{sid}D:P(A;;GA;;;{sid})"));
            let mut sd = PSECURITY_DESCRIPTOR::default();
            // SAFETY: the SDDL string outlives the call; the descriptor is
            // allocated by the system and freed in Drop.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &sddl,
                    SDDL_REVISION_1,
                    &mut sd,
                    None,
                )?;
            }
            Ok(Self {
                sd,
                sid: sid.to_string(),
            })
        }

        fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.sd.0,
                bInheritHandle: false.into(),
            };
            // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose
            // descriptor lives as long as `self`.
            let pipe = unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .max_instances(MAX_INSTANCES)
                    .create_with_security_attributes_raw(
                        name,
                        (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                    )?
            };
            // A later instance joins whatever pipe holds the name. Had every
            // instance of ours closed and another user created the name
            // meanwhile, this one would be theirs.
            if !first && win_process::owner_sid(pipe.as_raw_handle())? != self.sid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the pipe name is held by another user",
                ));
            }
            Ok(pipe)
        }
    }

    impl Drop for UserOnly {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.sd.0)));
            }
        }
    }

    /// The pipe, bound and waiting for its first client.
    pub struct PipeServer {
        name: String,
        descriptor: UserOnly,
        first: NamedPipeServer,
        check: ClientCheck,
        stop: watch::Receiver<bool>,
    }

    impl PipeServer {
        /// Creates the first instance under this user's name. Fails when
        /// the name is already taken, which is either this app still closing
        /// a previous server (retried for a moment) or another program
        /// squatting on it (refused: the extension would be talking to it).
        pub async fn bind(stop: watch::Receiver<bool>, check: ClientCheck) -> io::Result<Self> {
            Self::bind_at(pipe_name()?, stop, check).await
        }

        /// [`bind`](Self::bind) under another name, for tests.
        pub async fn bind_at(
            name: String,
            stop: watch::Receiver<bool>,
            check: ClientCheck,
        ) -> io::Result<Self> {
            let descriptor = UserOnly::new(&current_user_sid()?)?;
            let mut last = None;
            for _ in 0..10 {
                match descriptor.create(&name, true) {
                    Ok(first) => {
                        return Ok(Self {
                            name,
                            descriptor,
                            first,
                            check,
                            stop,
                        });
                    }
                    Err(e) => last = Some(e),
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(last.unwrap_or_else(|| io::ErrorKind::AddrInUse.into()))
        }

        /// Serves until `stop` turns true. Each connection runs as its own
        /// task, and each request on it as another, so a fill waiting for the
        /// user does not hold up a status on the same connection. A client
        /// that fails the [`ClientCheck`] is disconnected unread. Every frame
        /// written is wiped afterwards: a fill answer carries a password.
        ///
        /// `S` is state kept per connection and handed to every request on
        /// it, for limits that belong to one client. `warn` hears about
        /// refused clients and instances that could not be created.
        pub async fn run<S, H, F, W>(self, handler: H, warn: W) -> io::Result<()>
        where
            S: Default + Send + Sync + 'static,
            H: Fn(Arc<S>, Frame) -> F + Clone + Send + Sync + 'static,
            F: Future<Output = Vec<u8>> + Send + 'static,
            W: Fn(String) + Clone + Send + Sync + 'static,
        {
            let Self {
                name,
                descriptor,
                first,
                check,
                mut stop,
            } = self;
            let mut waiting = Some(first);
            loop {
                let pipe = match waiting.take() {
                    Some(pipe) => pipe,
                    None => match next_instance(&descriptor, &name, &mut stop, &warn).await? {
                        Some(pipe) => pipe,
                        None => return Ok(()),
                    },
                };
                // A connect error is a client that left before it was
                // accepted. The instance is spent either way, and the
                // connection task below ends on its first read.
                tokio::select! {
                    _ = pipe.connect() => {}
                    _ = stop.changed() => return Ok(()),
                }
                tokio::spawn(connection::<S, H, F, W>(
                    pipe,
                    handler.clone(),
                    check.clone(),
                    warn.clone(),
                    stop.clone(),
                ));
            }
        }
    }

    /// The next instance to wait on. Creating one fails while every
    /// instance is taken; the server tries again, more slowly each time,
    /// rather than giving the name up. `None` when stopped, an error when
    /// the name turned out to be another user's.
    async fn next_instance<W: Fn(String)>(
        descriptor: &UserOnly,
        name: &str,
        stop: &mut watch::Receiver<bool>,
        warn: &W,
    ) -> io::Result<Option<NamedPipeServer>> {
        let mut delay = RETRY_FIRST;
        let mut warned = false;
        loop {
            match descriptor.create(name, false) {
                Ok(pipe) => return Ok(Some(pipe)),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(e),
                Err(e) => {
                    if !warned {
                        warn(format!("waiting to open another connection: {e}"));
                        warned = true;
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = stop.changed() => return Ok(None),
            }
            delay = (delay * 2).min(RETRY_MAX);
        }
    }

    async fn connection<S, H, F, W>(
        pipe: NamedPipeServer,
        handler: H,
        check: ClientCheck,
        warn: W,
        mut stop: watch::Receiver<bool>,
    ) where
        S: Default + Send + Sync + 'static,
        H: Fn(Arc<S>, Frame) -> F + Clone + Send + Sync + 'static,
        F: Future<Output = Vec<u8>> + Send + 'static,
        W: Fn(String) + Clone + Send + Sync + 'static,
    {
        // Who is on the other end, before a single byte is read.
        let admitted = match win_process::pipe_client_pid(pipe.as_raw_handle()) {
            Ok(pid) => tokio::task::spawn_blocking(move || check.admit(pid))
                .await
                .unwrap_or_else(|e| Err(e.to_string())),
            Err(e) => Err(format!("no client process: {e}")),
        };
        if let Err(reason) = admitted {
            warn(format!("refused a connection: {reason}"));
            return;
        }

        let state = Arc::new(S::default());
        let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
        let (mut reader, mut writer) = tokio::io::split(pipe);
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
        let mut requests = JoinSet::new();
        let write = async move {
            while let Some(mut frame) = rx.recv().await {
                let written = write_frame(&mut writer, &frame).await;
                frame.zeroize();
                if written.is_err() {
                    break;
                }
            }
        };
        let read = async move {
            loop {
                // At most MAX_IN_FLIGHT requests at once: the next frame is
                // read only when one of them has finished.
                let permit = tokio::select! {
                    permit = in_flight.clone().acquire_owned() => permit,
                    _ = stop.changed() => break,
                };
                let Ok(permit) = permit else { break };
                let frame = tokio::select! {
                    frame = read_frame(&mut reader) => frame,
                    _ = stop.changed() => break,
                };
                let Ok(Some(frame)) = frame else { break };
                let handler = handler.clone();
                let state = state.clone();
                let tx = tx.clone();
                requests.spawn(async move {
                    let answer = handler(state, frame).await;
                    let _ = tx.send(answer).await;
                    drop(permit);
                });
                while requests.try_join_next().is_some() {}
            }
            // The other side left, or the server is stopping: a fill still
            // waiting for confirmation has nobody left to answer to.
            requests.abort_all();
            while requests.join_next().await.is_some() {}
            // The writer ends once this last sender is gone.
            drop(tx);
        };
        tokio::join!(write, read);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn frames_of(bytes: Vec<u8>) -> Vec<Frame> {
        let mut reader = std::io::Cursor::new(bytes);
        let mut out = Vec::new();
        while let Some(frame) = read_frame(&mut reader).await.unwrap() {
            out.push(frame);
        }
        out
    }

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn a_frame_round_trips() {
        let mut buf = Vec::new();
        write_frame(&mut buf, br#"{"id":"1","type":"status"}"#)
            .await
            .unwrap();
        assert_eq!(&buf[..4], &26u32.to_le_bytes());
        assert_eq!(
            frames_of(buf).await,
            vec![Frame::Message(br#"{"id":"1","type":"status"}"#.to_vec())]
        );
    }

    #[tokio::test]
    async fn frames_follow_one_another() {
        let mut bytes = framed(b"one");
        bytes.extend(framed(b""));
        bytes.extend(framed(b"three"));
        assert_eq!(
            frames_of(bytes).await,
            vec![
                Frame::Message(b"one".to_vec()),
                Frame::Message(Vec::new()),
                Frame::Message(b"three".to_vec()),
            ]
        );
    }

    #[tokio::test]
    async fn an_oversized_frame_is_skipped_and_the_stream_stays_in_step() {
        let mut bytes = framed(&vec![b'x'; MAX_FRAME + 1]);
        bytes.extend(framed(b"after"));
        assert_eq!(
            frames_of(bytes).await,
            vec![Frame::TooLarge, Frame::Message(b"after".to_vec())]
        );
    }

    #[tokio::test]
    async fn a_frame_at_the_limit_is_accepted() {
        let body = vec![b'x'; MAX_FRAME];
        assert_eq!(frames_of(framed(&body)).await, vec![Frame::Message(body)]);
    }

    #[tokio::test]
    async fn writing_over_the_limit_is_refused() {
        let mut buf = Vec::new();
        let err = write_frame(&mut buf, &vec![0u8; MAX_FRAME + 1])
            .await
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(buf.is_empty(), "nothing goes out for a refused frame");
    }

    #[tokio::test]
    async fn a_close_between_frames_is_the_end_not_an_error() {
        assert!(frames_of(Vec::new()).await.is_empty());
    }

    #[tokio::test]
    async fn a_close_inside_a_frame_is_an_error() {
        let mut bytes = framed(b"complete");
        bytes.truncate(6);
        let mut reader = std::io::Cursor::new(bytes);
        assert!(read_frame(&mut reader).await.is_err());
    }

    /// The descriptor as Windows reports it on a connected handle, and a
    /// second server refused the name. One test: both need the real pipe.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_pipe_admits_this_user_only_and_cannot_be_taken_twice() {
        use ::windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
        use ::windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_KERNEL_OBJECT,
        };
        use ::windows::Win32::Security::{
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        };
        use ::windows::core::PWSTR;
        use std::os::windows::io::AsRawHandle;

        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let Ok(server) = PipeServer::bind(stop_rx.clone(), this_process()).await else {
            eprintln!("skipped: the app's browser pipe is open on this machine");
            return;
        };
        assert!(
            PipeServer::bind(stop_rx, this_process()).await.is_err(),
            "a second server took the name"
        );

        let client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(pipe_name().unwrap())
            .unwrap();
        let wanted = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        // SAFETY: reads the descriptor of a handle this test owns, and frees
        // what the two calls allocate.
        let sddl = unsafe {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            let got = GetSecurityInfo(
                HANDLE(client.as_raw_handle()),
                SE_KERNEL_OBJECT,
                wanted,
                None,
                None,
                None,
                None,
                Some(&mut sd),
            );
            assert!(got.is_ok(), "GetSecurityInfo: {got:?}");
            let mut text = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                wanted,
                &mut text,
                None,
            )
            .unwrap();
            let sddl = text.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            sddl
        };
        let sid = current_user_sid().unwrap();
        assert_eq!(sddl, format!("O:{sid}D:P(A;;FA;;;{sid})"));

        drop(client);
        drop(server);
        let _ = stop.send(true);
    }

    #[cfg(windows)]
    #[test]
    fn the_pipe_is_named_for_this_user() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
        assert_eq!(
            pipe_name().unwrap(),
            format!(r"\\.\pipe\silentsilo-browser-{sid}")
        );
    }
    /// A check that admits this test process and nothing else.
    #[cfg(windows)]
    fn this_process() -> ClientCheck {
        ClientCheck {
            image: std::env::current_exe().unwrap(),
            same_signer: false,
        }
    }

    #[cfg(windows)]
    fn test_pipe_name(what: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!(
            r"\\.\pipe\silentsilo-test-{what}-{}-{nanos}",
            std::process::id()
        )
    }

    type Warnings = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

    /// Echoes each request back, counting them.
    #[cfg(windows)]
    async fn serve_echo(
        name: &str,
        check: ClientCheck,
    ) -> (
        tokio::sync::watch::Sender<bool>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        Warnings,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let server = PipeServer::bind_at(name.to_string(), stop_rx, check)
            .await
            .unwrap();
        let handled = Arc::new(AtomicUsize::new(0));
        let warnings: Warnings = Arc::new(Mutex::new(Vec::new()));
        let (count, heard) = (handled.clone(), warnings.clone());
        tokio::spawn(server.run(
            move |_: Arc<()>, frame: Frame| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    match frame {
                        Frame::Message(body) => body,
                        Frame::TooLarge => Vec::new(),
                    }
                }
            },
            move |warning: String| heard.lock().unwrap().push(warning),
        ));
        (stop, handled, warnings)
    }

    #[cfg(windows)]
    fn open_client(
        name: &str,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        tokio::net::windows::named_pipe::ClientOptions::new().open(name)
    }

    /// Opens a client, waiting out the moment between instances.
    #[cfg(windows)]
    async fn connect(name: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
        for _ in 0..300 {
            match open_client(name) {
                Ok(client) => return client,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
            }
        }
        panic!("the server never took a client");
    }

    #[cfg(windows)]
    async fn round_trip(
        client: &mut tokio::net::windows::named_pipe::NamedPipeClient,
        body: &[u8],
    ) -> std::io::Result<Option<Frame>> {
        write_frame(client, body).await?;
        tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(client))
            .await
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
    }

    #[cfg(windows)]
    fn host_elsewhere() -> ClientCheck {
        ClientCheck {
            image: std::env::current_exe().unwrap().with_file_name(HOST_EXE),
            same_signer: false,
        }
    }

    /// A process that is not the host is disconnected before its request
    /// is read.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_client_that_is_not_the_host_is_turned_away_unread() {
        let name = test_pipe_name("refuse");
        let (stop, handled, warnings) = serve_echo(&name, host_elsewhere()).await;
        let mut client = connect(&name).await;
        let answer = round_trip(&mut client, br#"{"id":"1","type":"search","query":"ba"}"#).await;
        assert!(
            !matches!(answer, Ok(Some(Frame::Message(_)))),
            "a refused client got an answer: {answer:?}"
        );
        assert_eq!(handled.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            warnings
                .lock()
                .unwrap()
                .iter()
                .any(|w| w.contains("refused")),
            "the refusal is reported"
        );
        let _ = stop.send(true);
    }

    #[cfg(windows)]
    #[test]
    fn the_host_check_names_the_reason() {
        let reason = host_elsewhere().admit(std::process::id()).unwrap_err();
        assert!(reason.contains(HOST_EXE), "{reason}");
        assert_eq!(this_process().admit(std::process::id()), Ok(()));
    }

    /// With every instance taken the server cannot create the next one. It
    /// keeps trying instead of giving the name up, and serves again once a
    /// client leaves.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_server_outlasts_a_full_house() {
        let name = test_pipe_name("full");
        let (stop, _, _) = serve_echo(&name, this_process()).await;
        let mut clients = Vec::new();
        for _ in 0..16 {
            let mut client = connect(&name).await;
            let echoed = round_trip(&mut client, b"hello").await.unwrap();
            assert_eq!(echoed, Some(Frame::Message(b"hello".to_vec())));
            clients.push(client);
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            open_client(&name).is_err(),
            "a seventeenth connection got in"
        );

        drop(clients.pop());
        let mut late = connect(&name).await;
        let echoed = round_trip(&mut late, b"again").await.unwrap();
        assert_eq!(echoed, Some(Frame::Message(b"again".to_vec())));
        let _ = stop.send(true);
    }

    /// A connection has at most four requests in progress; the fifth is not
    /// read until one of them ends.
    #[cfg(windows)]
    #[tokio::test]
    async fn one_connection_has_at_most_four_requests_in_progress() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let name = test_pipe_name("inflight");
        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let server = PipeServer::bind_at(name.clone(), stop_rx, this_process())
            .await
            .unwrap();
        let started = Arc::new(AtomicUsize::new(0));
        let (release, released) = tokio::sync::watch::channel(false);
        let count = started.clone();
        tokio::spawn(server.run(
            move |_: Arc<()>, _frame: Frame| {
                let count = count.clone();
                let mut released = released.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    let _ = released.wait_for(|r| *r).await;
                    b"done".to_vec()
                }
            },
            |_| {},
        ));
        let mut client = connect(&name).await;
        for _ in 0..6 {
            write_frame(&mut client, b"wait").await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(started.load(Ordering::SeqCst), 4);
        let _ = release.send(true);
        for _ in 0..6 {
            assert_eq!(
                read_frame(&mut client).await.unwrap(),
                Some(Frame::Message(b"done".to_vec()))
            );
        }
        assert_eq!(started.load(Ordering::SeqCst), 6);
        let _ = stop.send(true);
    }
}
