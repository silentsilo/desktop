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
pub use imp::{PipeServer, current_user_sid, pipe_name};

#[cfg(windows)]
mod imp {
    use std::future::Future;
    use std::io;
    use std::time::Duration;

    use ::windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use ::windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use ::windows::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use ::windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use ::windows::core::{HSTRING, PWSTR};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::sync::{mpsc, watch};
    use tokio::task::JoinSet;
    use zeroize::Zeroize;

    use super::{Frame, read_frame, write_frame};

    /// At most this many connections at once. The browser starts one host
    /// per port, and an extension needs one or two.
    const MAX_INSTANCES: usize = 16;

    /// The SID of the user this process runs as, `S-1-5-21-…`.
    pub fn current_user_sid() -> io::Result<String> {
        // SAFETY: plain Win32 calls on this process's own token. The buffer
        // is u64-aligned for TOKEN_USER, sized by the first call, and the
        // SID it points into lives inside it until the string is copied out.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let got = GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                len,
                &mut len,
            );
            let _ = CloseHandle(token);
            got?;
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text)?;
            let sid = text.to_string();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            sid.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }

    /// `\\.\pipe\silentsilo-browser-<SID>`. Per user, so two people signed
    /// in to one machine never reach each other's app.
    pub fn pipe_name() -> io::Result<String> {
        Ok(format!(
            r"\\.\pipe\silentsilo-browser-{}",
            current_user_sid()?
        ))
    }

    /// A security descriptor granting the current user, and nobody else,
    /// full access. `P` protects the DACL from inheriting anything.
    struct UserOnly(PSECURITY_DESCRIPTOR);

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
            Ok(Self(sd))
        }

        fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0.0,
                bInheritHandle: false.into(),
            };
            // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose
            // descriptor lives as long as `self`.
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .max_instances(MAX_INSTANCES)
                    .create_with_security_attributes_raw(
                        name,
                        (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                    )
            }
        }
    }

    impl Drop for UserOnly {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }

    /// The pipe, bound and waiting for its first client.
    pub struct PipeServer {
        name: String,
        descriptor: UserOnly,
        first: NamedPipeServer,
        stop: watch::Receiver<bool>,
    }

    impl PipeServer {
        /// Creates the first instance. Fails when the name is already taken,
        /// which is either this app still closing a previous server (retried
        /// for a moment) or another program squatting on it (refused: the
        /// extension would be talking to it).
        pub async fn bind(stop: watch::Receiver<bool>) -> io::Result<Self> {
            let name = pipe_name()?;
            let descriptor = UserOnly::new(&current_user_sid()?)?;
            let mut last = None;
            for _ in 0..10 {
                match descriptor.create(&name, true) {
                    Ok(first) => {
                        return Ok(Self {
                            name,
                            descriptor,
                            first,
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
        /// user does not hold up a status on the same connection. Every frame
        /// written is wiped afterwards: a fill answer carries a password.
        pub async fn run<H, F>(self, handler: H) -> io::Result<()>
        where
            H: Fn(Frame) -> F + Clone + Send + Sync + 'static,
            F: Future<Output = Vec<u8>> + Send + 'static,
        {
            let Self {
                name,
                descriptor,
                first,
                mut stop,
            } = self;
            let mut waiting = first;
            loop {
                // A connect error is a client that left before it was
                // accepted. The instance is spent either way, and the
                // connection task below ends on its first read.
                tokio::select! {
                    _ = waiting.connect() => {}
                    _ = stop.changed() => return Ok(()),
                }
                let next = descriptor.create(&name, false)?;
                let pipe = std::mem::replace(&mut waiting, next);
                tokio::spawn(connection(pipe, handler.clone(), stop.clone()));
            }
        }
    }

    async fn connection<H, F>(pipe: NamedPipeServer, handler: H, mut stop: watch::Receiver<bool>)
    where
        H: Fn(Frame) -> F + Clone + Send + Sync + 'static,
        F: Future<Output = Vec<u8>> + Send + 'static,
    {
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
                let frame = tokio::select! {
                    frame = read_frame(&mut reader) => frame,
                    _ = stop.changed() => break,
                };
                let Ok(Some(frame)) = frame else { break };
                let handler = handler.clone();
                let tx = tx.clone();
                requests.spawn(async move {
                    let answer = handler(frame).await;
                    let _ = tx.send(answer).await;
                });
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
        let Ok(server) = PipeServer::bind(stop_rx.clone()).await else {
            eprintln!("skipped: the app's browser pipe is open on this machine");
            return;
        };
        assert!(
            PipeServer::bind(stop_rx).await.is_err(),
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
}
