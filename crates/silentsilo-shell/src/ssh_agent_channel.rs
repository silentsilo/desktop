//! The SSH agent's channel: where `ssh` finds the agent, and who is on the
//! other end. The agent itself (which keys, what to sign) is the app's.
//!
//! On Windows the pipe Windows' own `ssh.exe` opens,
//! `\\.\pipe\openssh-ssh-agent`, created as its first instance and open to
//! this user only. On Linux `$XDG_RUNTIME_DIR/silentsilo/ssh-agent.sock`,
//! beside the browser's socket in the same private directory. Messages are
//! the agent protocol's (RFC 9987): a 32-bit big-endian length, then the
//! message. One request at a time per connection, as clients send them.

use std::io;
use std::path::PathBuf;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest message read. OpenSSH's own agent stops at 256 KB; nothing a
/// client sends to be signed comes near it.
pub const MAX_MESSAGE: usize = 256 * 1024;

/// Who opened a connection, as far as the system can say. Shown to the
/// person; never a reason to sign.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    /// The program, when the system names it.
    pub exe: Option<PathBuf>,
    /// The program that started it: a terminal, an editor, Git.
    pub parent: Option<PathBuf>,
}

/// Reads one message. `None` means the other side closed between messages.
/// A length over [`MAX_MESSAGE`] is an error, and the connection ends.
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 || len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent message length out of range",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

pub async fn write_message<W: AsyncWrite + Unpin>(writer: &mut W, body: &[u8]) -> io::Result<()> {
    writer.write_all(&(body.len() as u32).to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Whether the person turned the SSH agent on. Off by default: nothing
/// listens until this says yes.
pub fn agent_enabled() -> bool {
    std::fs::read_to_string(setting_path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("enabled").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

pub fn set_agent_enabled(enabled: bool) -> io::Result<()> {
    let path = setting_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::json!({ "enabled": enabled }).to_string())
}

/// Beside the browser extension's setting.
fn setting_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SilentSilo")
        .join("ssh-agent.json")
}

/// One admitted connection: each message in, the handler's answer out,
/// until either side or `stop` ends it. An empty answer closes the
/// connection.
#[cfg(any(windows, unix))]
async fn serve<T, S, H, F>(
    stream: T,
    peer: std::sync::Arc<Peer>,
    handler: H,
    mut stop: tokio::sync::watch::Receiver<bool>,
) where
    T: AsyncRead + AsyncWrite + Unpin,
    S: Default,
    H: Fn(std::sync::Arc<Peer>, std::sync::Arc<S>, Vec<u8>) -> F,
    F: std::future::Future<Output = Vec<u8>>,
{
    use zeroize::Zeroize;

    let state = std::sync::Arc::new(S::default());
    let (mut reader, mut writer) = tokio::io::split(stream);
    loop {
        let message = tokio::select! {
            message = read_message(&mut reader) => message,
            _ = stop.changed() => return,
        };
        let Ok(Some(mut message)) = message else {
            return;
        };
        let answer = tokio::select! {
            answer = handler(peer.clone(), state.clone(), std::mem::take(&mut message)) => answer,
            _ = stop.changed() => return,
        };
        message.zeroize();
        if answer.is_empty() || write_message(&mut writer, &answer).await.is_err() {
            return;
        }
    }
}

/// Why the agent could not start listening.
#[derive(Debug)]
pub enum BindError {
    /// Another program holds the name: on Windows usually the OpenSSH
    /// Authentication Agent service, named here when it can be.
    Taken {
        holder: Option<PathBuf>,
    },
    Other(io::Error),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Taken { holder: Some(path) } => {
                write!(f, "the agent's address is held by {}", path.display())
            }
            BindError::Taken { holder: None } => {
                write!(f, "the agent's address is held by another program")
            }
            BindError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for BindError {
    fn from(e: io::Error) -> Self {
        BindError::Other(e)
    }
}

#[cfg(windows)]
pub use imp::{AgentServer, PIPE_NAME};

#[cfg(unix)]
pub use unix::{AgentServer, socket_path};

#[cfg(windows)]
mod imp {
    use std::future::Future;
    use std::os::windows::io::AsRawHandle;
    use std::sync::Arc;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer};
    use tokio::sync::watch;

    use super::{BindError, Peer};
    use crate::win_pipe::UserOnly;
    use crate::win_process;

    /// The name Windows' OpenSSH looks for. Not per user: a second person
    /// signed in to the same machine cannot have an agent here at the same
    /// time, which is OpenSSH's choice, not ours.
    pub const PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

    /// At most this many connections at once.
    const MAX_INSTANCES: usize = 16;

    pub struct AgentServer {
        name: String,
        descriptor: UserOnly,
        first: NamedPipeServer,
        stop: watch::Receiver<bool>,
    }

    impl AgentServer {
        pub async fn bind(stop: watch::Receiver<bool>) -> Result<Self, BindError> {
            Self::bind_at(PIPE_NAME.to_string(), stop).await
        }

        /// [`bind`](Self::bind) under another name, for tests. A name already
        /// served is refused, never joined: whoever holds it would get the
        /// connections meant for this agent.
        pub async fn bind_at(name: String, stop: watch::Receiver<bool>) -> Result<Self, BindError> {
            let descriptor = UserOnly::new(MAX_INSTANCES)?;
            let mut last = None;
            // A moment for this app's own previous server to let go.
            for _ in 0..5 {
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
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let last = last.unwrap_or_else(|| std::io::ErrorKind::AddrInUse.into());
            match holder(&name) {
                Some(holder) => Err(BindError::Taken {
                    holder: Some(holder),
                }),
                None if last.kind() == std::io::ErrorKind::PermissionDenied => {
                    Err(BindError::Taken { holder: None })
                }
                None => Err(BindError::Other(last)),
            }
        }

        /// Serves until `stop` turns true. Each connection runs as its own
        /// task; a client of another user is dropped unread.
        pub async fn run<S, H, F, W>(self, handler: H, warn: W) -> std::io::Result<()>
        where
            S: Default + Send + Sync + 'static,
            H: Fn(Arc<Peer>, Arc<S>, Vec<u8>) -> F + Clone + Send + Sync + 'static,
            F: Future<Output = Vec<u8>> + Send + 'static,
            W: Fn(String) + Clone + Send + Sync + 'static,
        {
            let Self {
                name,
                descriptor,
                first,
                mut stop,
            } = self;
            let mut waiting = Some(first);
            loop {
                let pipe = match waiting.take() {
                    Some(pipe) => pipe,
                    None => match descriptor.next_instance(&name, &mut stop, &warn).await? {
                        Some(pipe) => pipe,
                        None => return Ok(()),
                    },
                };
                tokio::select! {
                    _ = pipe.connect() => {}
                    _ = stop.changed() => return Ok(()),
                }
                let (handler, warn, stop) = (handler.clone(), warn.clone(), stop.clone());
                tokio::spawn(async move {
                    let peer = match tokio::task::spawn_blocking({
                        let pid = win_process::pipe_client_pid(pipe.as_raw_handle());
                        move || pid.and_then(peer_of)
                    })
                    .await
                    {
                        Ok(Ok(peer)) => peer,
                        Ok(Err(e)) => {
                            warn(format!("refused a connection: {e}"));
                            return;
                        }
                        Err(e) => {
                            warn(format!("refused a connection: {e}"));
                            return;
                        }
                    };
                    super::serve::<_, S, H, F>(pipe, Arc::new(peer), handler, stop).await;
                });
            }
        }
    }

    /// The client process, which must be this user's. The pipe's DACL lets
    /// no one else open it; this says so again before a byte is read.
    fn peer_of(pid: u32) -> std::io::Result<Peer> {
        if win_process::user_sid(pid)? != win_process::current_user_sid()? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the client runs as another user",
            ));
        }
        Ok(Peer {
            pid,
            exe: win_process::image_path(pid).ok(),
            parent: win_process::parent_pid(pid)
                .and_then(win_process::image_path)
                .ok(),
        })
    }

    /// The program serving `name`, found by connecting to it as a client.
    fn holder(name: &str) -> Option<std::path::PathBuf> {
        let client = ClientOptions::new().open(name).ok()?;
        let pid = win_process::pipe_server_pid(client.as_raw_handle()).ok()?;
        win_process::image_path(pid).ok()
    }
}

#[cfg(unix)]
mod unix {
    use std::future::Future;
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::{Semaphore, watch};

    use super::{BindError, Peer};

    const MAX_CONNECTIONS: usize = 16;

    fn own_uid() -> u32 {
        // SAFETY: getuid cannot fail and touches no memory.
        unsafe { libc::getuid() }
    }

    /// `$XDG_RUNTIME_DIR/silentsilo/ssh-agent.sock`. No fallback to a
    /// directory other users share.
    pub fn socket_path() -> io::Result<PathBuf> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no XDG_RUNTIME_DIR"))?;
        Ok(PathBuf::from(runtime)
            .join("silentsilo")
            .join("ssh-agent.sock"))
    }

    pub struct AgentServer {
        listener: UnixListener,
        path: PathBuf,
        /// The socket file this server made: on its way out it removes that
        /// one only, never a newer server's at the same path.
        inode: u64,
        stop: watch::Receiver<bool>,
    }

    impl AgentServer {
        pub async fn bind(stop: watch::Receiver<bool>) -> Result<Self, BindError> {
            Self::bind_at(socket_path()?, stop).await
        }

        /// [`bind`](Self::bind) at another path, for tests. A socket there
        /// that answers is another program's and is refused; one that does
        /// not is left from a crash and goes.
        pub async fn bind_at(
            path: PathBuf,
            stop: watch::Receiver<bool>,
        ) -> Result<Self, BindError> {
            let dir = path
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no directory"))?;
            private_dir(dir)?;
            // A moment for this app's own previous server to let go, as on
            // Windows: turned off and straight back on, it may still answer.
            let mut tries = 0;
            while std::fs::symlink_metadata(&path).is_ok() {
                if UnixStream::connect(&path).await.is_err() {
                    // Gone already if the old server was just leaving.
                    match std::fs::remove_file(&path) {
                        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                        _ => break,
                    }
                }
                tries += 1;
                if tries == 5 {
                    return Err(BindError::Taken { holder: None });
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            let listener = UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            let inode = std::fs::symlink_metadata(&path)?.ino();
            Ok(Self {
                listener,
                path,
                inode,
                stop,
            })
        }

        /// Serves until `stop` turns true, then removes the socket.
        pub async fn run<S, H, F, W>(self, handler: H, warn: W) -> io::Result<()>
        where
            S: Default + Send + Sync + 'static,
            H: Fn(Arc<Peer>, Arc<S>, Vec<u8>) -> F + Clone + Send + Sync + 'static,
            F: Future<Output = Vec<u8>> + Send + 'static,
            W: Fn(String) + Clone + Send + Sync + 'static,
        {
            let Self {
                listener,
                path,
                inode,
                mut stop,
            } = self;
            let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    _ = stop.changed() => break,
                };
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        warn(format!("a connection failed: {e}"));
                        continue;
                    }
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    warn("too many connections at once".into());
                    continue;
                };
                let (handler, warn, stop) = (handler.clone(), warn.clone(), stop.clone());
                tokio::spawn(async move {
                    match peer_of(&stream) {
                        Ok(peer) => {
                            super::serve::<_, S, H, F>(stream, Arc::new(peer), handler, stop).await
                        }
                        Err(reason) => warn(format!("refused a connection: {reason}")),
                    }
                    drop(slot);
                });
            }
            drop(listener);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.ino() == inode) {
                let _ = std::fs::remove_file(&path);
            }
            Ok(())
        }
    }

    /// The directory, made if missing, must be this user's and private.
    fn private_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() || meta.uid() != own_uid() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the socket's directory is not this user's",
            ));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }

    fn peer_of(stream: &UnixStream) -> Result<Peer, String> {
        let cred = stream.peer_cred().map_err(|e| e.to_string())?;
        if cred.uid() != own_uid() {
            return Err("the client runs as another user".into());
        }
        let pid = cred.pid().unwrap_or(0).max(0) as u32;
        let exe = |pid: u32| std::fs::read_link(format!("/proc/{pid}/exe")).ok();
        let parent = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| parent_from_stat(&stat))
            .and_then(exe);
        Ok(Peer {
            pid,
            exe: exe(pid),
            parent,
        })
    }

    /// The fourth field of `/proc/<pid>/stat`, after the name in parentheses
    /// (which may itself hold spaces and parentheses).
    fn parent_from_stat(stat: &str) -> Option<u32> {
        let rest = &stat[stat.rfind(')')? + 1..];
        rest.split_whitespace().nth(1)?.parse().ok()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_parent_is_read_past_a_name_with_spaces() {
            assert_eq!(
                super::parent_from_stat("1234 (my (odd) prog) S 987 1234 1234 0"),
                Some(987)
            );
            assert_eq!(super::parent_from_stat("garbage"), None);
        }

        #[tokio::test]
        async fn turned_off_and_straight_back_on_binds_again() {
            let dir = std::env::temp_dir().join(format!("ss-agent-{}", std::process::id()));
            let path = dir.join("ssh-agent.sock");
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let server = super::AgentServer::bind_at(path.clone(), stopped)
                .await
                .unwrap();
            let running =
                tokio::spawn(server.run::<(), _, _, _>(|_, _, _| async { Vec::new() }, |_| {}));
            stop.send(true).unwrap();
            let (_again, quiet) = tokio::sync::watch::channel(false);
            let rebound = super::AgentServer::bind_at(path, quiet).await;
            assert!(rebound.is_ok(), "{:?}", rebound.as_ref().err());
            let _ = running.await;
            // The old server left the new one's socket where it was.
            assert!(dir.join("ssh-agent.sock").exists());
            drop(rebound);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn messages_are_big_endian_and_bounded() {
        let mut wire = Vec::new();
        write_message(&mut wire, b"\x0b").await.unwrap();
        assert_eq!(wire, [0, 0, 0, 1, 11]);
        let mut reader = &wire[..];
        assert_eq!(read_message(&mut reader).await.unwrap(), Some(vec![11]));
        assert_eq!(read_message(&mut reader).await.unwrap(), None);

        let too_long = ((MAX_MESSAGE + 1) as u32).to_be_bytes();
        assert!(read_message(&mut &too_long[..]).await.is_err());
        assert!(read_message(&mut &[0u8, 0, 0, 0][..]).await.is_err());
    }

    #[cfg(any(windows, unix))]
    #[tokio::test]
    async fn a_connection_is_served_in_turn_and_an_empty_answer_closes_it() {
        let (client, server) = tokio::io::duplex(1024);
        let (_stop_tx, stop) = tokio::sync::watch::channel(false);
        let handler = |_peer: std::sync::Arc<Peer>,
                       _state: std::sync::Arc<()>,
                       message: Vec<u8>| async move {
            if message == b"bye" {
                Vec::new()
            } else {
                [b"re:".as_slice(), &message].concat()
            }
        };
        let served = tokio::spawn(serve(
            server,
            std::sync::Arc::new(Peer::default()),
            handler,
            stop,
        ));
        let (mut r, mut w) = tokio::io::split(client);
        write_message(&mut w, b"one").await.unwrap();
        assert_eq!(read_message(&mut r).await.unwrap().unwrap(), b"re:one");
        write_message(&mut w, b"bye").await.unwrap();
        assert_eq!(read_message(&mut r).await.unwrap(), None);
        served.await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_name_already_served_is_refused_and_its_holder_named() {
        let name = format!(r"\\.\pipe\silentsilo-agent-test-{}", std::process::id());
        let (_tx, stop) = tokio::sync::watch::channel(false);
        let first = AgentServer::bind_at(name.clone(), stop.clone())
            .await
            .unwrap();
        match AgentServer::bind_at(name, stop).await {
            Err(BindError::Taken { holder: Some(path) }) => {
                assert_eq!(path, std::env::current_exe().unwrap());
            }
            Err(other) => panic!("{other}"),
            Ok(_) => panic!("bound twice"),
        }
        drop(first);
    }
}
