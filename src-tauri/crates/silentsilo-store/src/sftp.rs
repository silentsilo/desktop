//! A silo's backup over SFTP. The protocol is the easy part; the host key
//! is not. A client that accepts whatever key a server presents has no
//! authentication of the server at all, so the fingerprint is pinned in
//! the silo's settings and a mismatch fails loudly. Learning it is a
//! separate explicit step, [`probe_host_key`], shown to a person before
//! anything is stored.

use std::sync::Arc;

use async_trait::async_trait;
use russh::client;
use russh::keys::PublicKey;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{ObjectStore, StoreError, StoredObject};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum SftpAuth {
    Password {
        password: String,
    },
    /// The private key itself rather than a path to it.
    ///
    /// The silo's settings are already stored in the OS keyring, and keeping
    /// the key there means the silo keeps working when the file is moved or
    /// the machine is rebuilt — a path would be a second thing to get right.
    Key {
        private_key: String,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SftpAuth,
    /// Directory on the server that holds the silo.
    pub path: String,
    /// The server key this silo expects, as `SHA256:…`.
    ///
    /// `None` means the key has never been confirmed by a person, and no
    /// connection will be made — see the module note.
    pub host_fingerprint: Option<String>,
}

/// Accepts whatever the server offers and reports it, without transferring
/// anything.
///
/// Deliberately separate from the store: this exists to be shown to a human
/// for confirmation, and having it in the same type that reads and writes
/// data would make "connect without checking" a single wrong argument away.
struct LearningHandler {
    seen: Arc<std::sync::Mutex<Option<String>>>,
}

impl client::Handler for LearningHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(key.fingerprint(Default::default()).to_string());
        }
        Ok(true)
    }
}

/// Refuses any key but the one pinned in the silo's settings.
struct PinnedHandler {
    expected: String,
    /// What was actually offered, so a mismatch can name both.
    offered: Arc<std::sync::Mutex<Option<String>>>,
}

impl client::Handler for PinnedHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(Default::default()).to_string();
        let matches = fingerprint == self.expected;
        if let Ok(mut offered) = self.offered.lock() {
            *offered = Some(fingerprint);
        }
        Ok(matches)
    }
}

/// An SFTP session and the SSH connection carrying it.
///
/// These have to be held together. Dropping the russh handle tears down the
/// transport, and the SFTP session on top of it then fails mid-operation with
/// nothing but "disconnected" to say why — which is exactly what happened
/// when only the session was returned: small writes completed before the
/// teardown landed and larger ones did not.
struct Connection {
    ssh: client::Handle<PinnedHandler>,
    sftp: SftpSession,
}

impl std::ops::Deref for Connection {
    type Target = SftpSession;

    fn deref(&self) -> &Self::Target {
        &self.sftp
    }
}

/// Opens an SSH connection, retrying briefly if the handshake is dropped:
/// sshd refuses connections at random past `MaxStartups`, and on a shared
/// host that refusal is indistinguishable from the server being down. A
/// handler is built per attempt because `connect` consumes it.
async fn dial<H, F>(host: &str, port: u16, mut handler: F) -> Result<client::Handle<H>, H::Error>
where
    H: client::Handler + 'static,
    F: FnMut() -> H,
{
    let mut wait = std::time::Duration::from_millis(150);
    let mut last;
    loop {
        match client::connect(Arc::new(client::Config::default()), (host, port), handler()).await {
            Ok(handle) => return Ok(handle),
            Err(e) => last = e,
        }
        if wait > std::time::Duration::from_millis(600) {
            return Err(last);
        }
        tokio::time::sleep(wait).await;
        wait *= 2;
    }
}

/// Asks a server for its host key so a person can confirm it.
///
/// Nothing is stored and no credentials are sent: the key is offered during
/// the handshake, before authentication.
pub async fn probe_host_key(host: &str, port: u16) -> Result<String, StoreError> {
    let seen = Arc::new(std::sync::Mutex::new(None));

    dial(host, port, || LearningHandler { seen: seen.clone() })
        .await
        .map_err(|e| StoreError::Unreachable(format!("{host}:{port}: {e}")))?;

    seen.lock()
        .ok()
        .and_then(|s| s.clone())
        .ok_or_else(|| StoreError::Other("the server offered no host key".into()))
}

pub struct SftpStore {
    config: SftpConfig,
    base: String,
    /// The connection in use, kept between operations.
    ///
    /// One handshake per object was the first design, and it was wrong twice
    /// over: key exchange costs more than the transfers a sync pass makes,
    /// and sshd caps how many connections may be mid-handshake at once
    /// (`MaxStartups`, ten by default) — so a pass with a few files in flight
    /// gets connections dropped at random by the server's own defence.
    live: tokio::sync::Mutex<Option<Arc<Connection>>>,
}

impl SftpStore {
    pub fn new(config: SftpConfig) -> Result<Self, StoreError> {
        if config.host.trim().is_empty() {
            return Err(StoreError::Other("An SFTP host is required".into()));
        }
        if config.host_fingerprint.is_none() {
            return Err(StoreError::Denied(
                "This server's identity hasn't been confirmed yet. Check its fingerprint before \
                 connecting."
                    .into(),
            ));
        }
        Ok(Self {
            base: config.path.trim().trim_end_matches('/').to_string(),
            config,
            live: tokio::sync::Mutex::new(None),
        })
    }

    fn path_for(&self, key: &str) -> String {
        if self.base.is_empty() {
            key.to_string()
        } else {
            format!("{}/{key}", self.base)
        }
    }

    /// The connection to use, dialling one if there isn't a live one.
    async fn session(&self) -> Result<Arc<Connection>, StoreError> {
        let mut live = self.live.lock().await;
        if let Some(conn) = live.as_ref().filter(|c| !c.ssh.is_closed()) {
            return Ok(conn.clone());
        }
        let conn = Arc::new(self.connect().await?);
        *live = Some(conn.clone());
        Ok(conn)
    }

    /// Forgets `dead` so the next call dials again — unless something else
    /// already replaced it, in which case the newer one is the good one.
    async fn retire(&self, dead: &Arc<Connection>) {
        let mut live = self.live.lock().await;
        if live.as_ref().is_some_and(|c| Arc::ptr_eq(c, dead)) {
            *live = None;
        }
    }

    /// Runs an operation, dialling again and retrying once if the connection
    /// turned out to be dead.
    ///
    /// A reused connection can be closed between the check and the call — by
    /// an idle timeout, a NAT table, a server restart — and that is a normal
    /// event rather than a failure to report. Only a second failure on a
    /// connection that was definitely fresh is worth telling the user about.
    async fn run<T, F, Fut>(&self, op: F) -> Result<T, StoreError>
    where
        F: Fn(Arc<Connection>) -> Fut,
        Fut: std::future::Future<Output = Result<T, StoreError>>,
    {
        let conn = self.session().await?;
        match op(conn.clone()).await {
            Err(err) if conn.ssh.is_closed() => {
                self.retire(&conn).await;
                let _ = err;
                let fresh = self.session().await?;
                op(fresh).await
            }
            result => result,
        }
    }

    async fn connect(&self) -> Result<Connection, StoreError> {
        let expected = self
            .config
            .host_fingerprint
            .clone()
            .ok_or_else(|| StoreError::Denied("no confirmed host key".into()))?;
        let offered = Arc::new(std::sync::Mutex::new(None));

        let mut session = dial(&self.config.host, self.config.port, || PinnedHandler {
            expected: expected.clone(),
            offered: offered.clone(),
        })
        .await
        .map_err(|e| {
            // A rejected key surfaces here as a generic handshake failure,
            // so the specific message has to be reconstructed — otherwise
            // the most security-relevant error in the whole backend reads as
            // "connection closed".
            let seen = offered.lock().ok().and_then(|o| o.clone());
            match seen {
                Some(actual) if actual != expected => StoreError::Denied(format!(
                    "the server's identity has changed: expected {expected}, got {actual}. \
                     Do not continue unless you know why."
                )),
                _ => StoreError::Unreachable(e.to_string()),
            }
        })?;

        let authenticated = match &self.config.auth {
            SftpAuth::Password { password } => session
                .authenticate_password(&self.config.username, password)
                .await
                .map_err(|e| StoreError::Denied(e.to_string()))?,
            SftpAuth::Key {
                private_key,
                passphrase,
            } => {
                let key = russh::keys::decode_secret_key(private_key, passphrase.as_deref())
                    .map_err(|e| StoreError::Denied(format!("unusable private key: {e}")))?;
                let hash = session
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| StoreError::Other(e.to_string()))?
                    .flatten();
                session
                    .authenticate_publickey(
                        &self.config.username,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(|e| StoreError::Denied(e.to_string()))?
            }
        };
        if !authenticated.success() {
            return Err(StoreError::Denied(
                "the server rejected those credentials".into(),
            ));
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| StoreError::Other(e.to_string()))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| StoreError::Other(format!("the server has no SFTP subsystem: {e}")))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| StoreError::Other(e.to_string()))?;
        Ok(Connection { ssh: session, sftp })
    }

    /// Creates every directory a key needs, outermost first.
    ///
    /// Like WebDAV and unlike a bucket, directories are real here: writing
    /// `ops/000001.op` into a missing `ops/` fails.
    async fn ensure_parents(&self, sftp: &SftpSession, key: &str) -> Result<(), StoreError> {
        let full = self.path_for(key);
        let Some((dir, _)) = full.rsplit_once('/') else {
            return Ok(());
        };

        let mut built = if dir.starts_with('/') {
            String::from("/")
        } else {
            String::new()
        };
        for segment in dir.split('/').filter(|s| !s.is_empty()) {
            if !built.is_empty() && !built.ends_with('/') {
                built.push('/');
            }
            built.push_str(segment);
            // An existing directory answers with a failure that is
            // indistinguishable from other errors here, so the check is
            // "does it exist afterwards" rather than "did this succeed".
            if sftp.metadata(built.clone()).await.is_err() {
                let _ = sftp.create_dir(built.clone()).await;
            }
        }
        Ok(())
    }
}

fn missing(err: &russh_sftp::client::error::Error) -> bool {
    matches!(
        err,
        russh_sftp::client::error::Error::Status(status)
            if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile
    )
}

#[async_trait]
impl ObjectStore for SftpStore {
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError> {
        self.run(|sftp| {
            // Cloned because the retry may run this a second time.
            let body = body.clone();
            async move {
                self.ensure_parents(&sftp, key).await?;

                // Written under a temporary name and renamed, so another
                // device reading the directory never sees a partial object —
                // the same reasoning as the folder backend, and it matters
                // more here because the reader is on a different machine.
                let final_path = self.path_for(key);
                let temp_path = format!("{final_path}.part");
                let mut file = sftp
                    .open_with_flags(
                        temp_path.clone(),
                        OpenFlags::CREATE | OpenFlags::WRITE | OpenFlags::TRUNCATE,
                    )
                    .await
                    .map_err(|e| StoreError::Other(format!("{key}: {e}")))?;
                file.write_all(&body)
                    .await
                    .map_err(|e| StoreError::Other(format!("{key}: {e}")))?;
                file.shutdown()
                    .await
                    .map_err(|e| StoreError::Other(format!("{key}: {e}")))?;

                // SFTP rename fails if the target exists, and re-writing a
                // key is legitimate for the manifest and the key envelopes.
                let _ = sftp.remove_file(final_path.clone()).await;
                sftp.rename(temp_path, final_path)
                    .await
                    .map_err(|e| StoreError::Other(format!("{key}: {e}")))?;
                Ok(())
            }
        })
        .await
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        self.run(|sftp| async move {
            let mut file = sftp.open(self.path_for(key)).await.map_err(|e| {
                if missing(&e) {
                    StoreError::NotFound(key.to_string())
                } else {
                    StoreError::Other(format!("{key}: {e}"))
                }
            })?;

            let mut out = Vec::new();
            file.read_to_end(&mut out)
                .await
                .map_err(|e| StoreError::Other(format!("{key}: {e}")))?;
            Ok(out)
        })
        .await
    }

    async fn head(&self, key: &str) -> Result<Option<i64>, StoreError> {
        self.run(|sftp| async move {
            match sftp.metadata(self.path_for(key)).await {
                Ok(meta) => Ok(Some(meta.size.unwrap_or(0) as i64)),
                Err(e) if missing(&e) => Ok(None),
                Err(e) => Err(StoreError::Other(format!("{key}: {e}"))),
            }
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.run(|sftp| async move {
            match sftp.remove_file(self.path_for(key)).await {
                Ok(()) => Ok(()),
                // Already gone is the desired end state.
                Err(e) if missing(&e) => Ok(()),
                Err(e) => Err(StoreError::Other(format!("{key}: {e}"))),
            }
        })
        .await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<StoredObject>, StoreError> {
        self.run(|sftp| async move {
            let mut out = Vec::new();
            let mut queue = vec![prefix.trim_end_matches('/').to_string()];

            while let Some(relative) = queue.pop() {
                let dir = self.path_for(&relative);
                let entries = match sftp.read_dir(dir).await {
                    Ok(entries) => entries,
                    // A prefix nothing has been written to yet is an empty
                    // listing, which is the normal state of fresh storage.
                    Err(e) if missing(&e) => continue,
                    Err(e) => return Err(StoreError::Other(format!("{relative}: {e}"))),
                };

                for entry in entries {
                    let name = entry.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let key = if relative.is_empty() {
                        name.clone()
                    } else {
                        format!("{relative}/{name}")
                    };

                    if entry.metadata().is_dir() {
                        queue.push(key);
                    } else if !name.ends_with(".part") {
                        // A rename that never completed. Reporting it as an
                        // object would hand the caller a truncated file.
                        out.push(StoredObject {
                            key,
                            size: entry.metadata().size.unwrap_or(0) as i64,
                        });
                    }
                }
            }

            // Operation keys encode the Lamport counter, so callers read this
            // order as chronological. `readdir` promises nothing.
            out.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(out)
        })
        .await
    }

    fn describe(&self) -> String {
        format!(
            "{}@{}:{}",
            self.config.username,
            self.config.host,
            if self.base.is_empty() {
                "/"
            } else {
                &self.base
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SftpConfig {
        SftpConfig {
            host: "nas.example.com".into(),
            port: 22,
            username: "alex".into(),
            auth: SftpAuth::Password {
                password: "hunter2".into(),
            },
            path: "/backups/silo".into(),
            host_fingerprint: Some("SHA256:abc".into()),
        }
    }

    #[test]
    fn a_server_whose_key_was_never_confirmed_is_refused() {
        // The property the whole module exists for: no connection is made
        // to a host whose identity a person has not checked.
        let mut unconfirmed = config();
        unconfirmed.host_fingerprint = None;

        let Err(err) = SftpStore::new(unconfirmed) else {
            panic!("an unconfirmed host key must not be usable");
        };
        assert!(err.to_string().contains("fingerprint"), "got: {err}");
    }

    #[test]
    fn keys_are_placed_under_the_configured_directory() {
        let store = SftpStore::new(config()).unwrap();
        assert_eq!(
            store.path_for("ops/000001.op"),
            "/backups/silo/ops/000001.op"
        );
    }

    #[test]
    fn an_empty_directory_puts_objects_at_the_login_root() {
        let mut at_root = config();
        at_root.path = String::new();
        let store = SftpStore::new(at_root).unwrap();
        assert_eq!(store.path_for("ops/000001.op"), "ops/000001.op");
    }

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        let mut trailing = config();
        trailing.path = "/backups/silo/".into();
        let store = SftpStore::new(trailing).unwrap();
        assert_eq!(store.path_for("vault.json"), "/backups/silo/vault.json");
    }

    #[test]
    fn the_description_says_where_it_writes() {
        assert_eq!(
            SftpStore::new(config()).unwrap().describe(),
            "alex@nas.example.com:/backups/silo"
        );
    }
}
