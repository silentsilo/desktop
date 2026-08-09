//! What SilentSilo needs from a place to keep a backup: five operations,
//! nothing a plain file server cannot do, so a bucket, a folder, a WebDAV
//! share and an SFTP account are all the same thing to the layer above.
//!
//! Two properties a future backend could break silently: `list` returns
//! keys in lexicographic order (operation keys are zero-padded, so that
//! *is* logical order), and a key is written once and never rewritten.

use std::path::PathBuf;

use async_trait::async_trait;
use uuid::Uuid;

/// Namespace for target ids. A fixed UUID so the same bucket keeps the same
/// id across builds and machines.
const TARGET_NAMESPACE: Uuid = Uuid::from_u128(0x5115_e175_107a_4e70_0000_0000_0000_0001);

mod folder;
mod s3;
mod sftp;
mod webdav;

pub use folder::FolderStore;
pub use sftp::{SftpAuth, SftpConfig, SftpStore, probe_host_key};
pub use webdav::{WebDavConfig, WebDavStore};

/// One stored object, as `list` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// Key relative to the configured prefix.
    pub key: String,
    pub size: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0} is not there")]
    NotFound(String),
    #[error("storage rejected the request: {0}")]
    Denied(String),
    #[error("could not reach storage: {0}")]
    Unreachable(String),
    #[error("storage error: {0}")]
    Other(String),
}

/// What a place a backup lives in does about deletion. Two facts rather
/// than one flag: object lock refuses a delete outright, while versioning
/// accepts it and keeps the old version readable, which makes revoking a
/// key look like it worked when it did not.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Protection {
    pub versioning: bool,
    pub object_lock: bool,
}

/// Somewhere a silo's backup can live.
///
/// Object-safe on purpose: the app holds one of these behind a pointer,
/// chosen at runtime from the user's settings, and every caller above is
/// written once against the trait rather than once per backend.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Writes an object, replacing any existing one at that key.
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError>;

    /// Reads an object whole. Everything stored here is small enough for
    /// that — blobs are chunk-encrypted files, not streams.
    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError>;

    /// The object's size, or `None` when it isn't there.
    ///
    /// Separate from `get` because the commonest question about a blob is
    /// "is it already up there", and answering it should not move bytes.
    async fn head(&self, key: &str) -> Result<Option<i64>, StoreError>;

    async fn delete(&self, key: &str) -> Result<(), StoreError>;

    /// Every object under `prefix`, in lexicographic order by key.
    async fn list(&self, prefix: &str) -> Result<Vec<StoredObject>, StoreError>;

    /// A short phrase naming where this writes, for the status bar.
    fn describe(&self) -> String;

    /// What this storage does about deletion, as it reports itself.
    ///
    /// The default is "nothing", which is the truth for a folder, a WebDAV
    /// share and an SFTP account: none of them can be made to refuse a
    /// delete, and pretending otherwise would let the app promise a
    /// tamper-resistant copy it cannot deliver. Only S3 answers differently,
    /// and only when the bucket itself says so.
    async fn protection(&self) -> Protection {
        Protection::default()
    }

    /// Round-trips a probe object, so a misconfiguration is found while the
    /// user is still looking at the settings that caused it.
    async fn check(&self) -> Result<(), StoreError> {
        const PROBE: &str = ".silentsilo-write-test";
        self.put(PROBE, b"ok".to_vec()).await?;
        let read = self.get(PROBE).await?;
        // Deleted whether or not the contents matched: leaving a probe
        // behind on a failed check is litter in the user's own storage.
        let removed = self.delete(PROBE).await;
        if read != b"ok" {
            return Err(StoreError::Other(
                "storage returned different bytes than were written".into(),
            ));
        }
        removed
    }
}

/// Everything needed to reach one backend.
///
/// An enum rather than a trait object at the config layer, because this is
/// what gets serialised into the silo's settings and round-tripped through
/// the UI — and a closed set is exactly what lets the UI show the right
/// fields for the chosen kind.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StoreConfig {
    S3(silentsilo_core::S3Config),
    Folder { path: PathBuf },
    WebDav(WebDavConfig),
    Sftp(SftpConfig),
}

impl StoreConfig {
    /// A stable name for the place this config points at, keying the
    /// per-target bookkeeping. Derived from where the target is, so
    /// changing the bucket or path makes a different target, correctly.
    /// Credentials are deliberately not part of it: rotating an access key
    /// does not make it a different bucket, and hashing a secret into a
    /// database is a way to leak one.
    pub fn target_id(&self) -> Uuid {
        let location = match self {
            Self::S3(config) => format!(
                "s3|{}|{}|{}|{}",
                config.endpoint.trim_end_matches('/'),
                config.region,
                config.bucket,
                config.prefix.trim_matches('/')
            ),
            Self::Folder { path } => format!("folder|{}", path.display()),
            Self::WebDav(config) => {
                format!("webdav|{}", config.url.trim_end_matches('/'))
            }
            Self::Sftp(config) => format!(
                "sftp|{}|{}|{}",
                config.host,
                config.port,
                config.path.trim_end_matches('/')
            ),
        };
        Uuid::new_v5(&TARGET_NAMESPACE, location.as_bytes())
    }

    pub fn open(&self) -> Result<Box<dyn ObjectStore>, StoreError> {
        match self {
            Self::S3(config) => Ok(Box::new(
                silentsilo_s3::S3Client::new(config.clone())
                    .map_err(|e| StoreError::Other(e.to_string()))?,
            )),
            Self::Folder { path } => Ok(Box::new(FolderStore::new(path.clone()))),
            Self::WebDav(config) => Ok(Box::new(WebDavStore::new(config.clone())?)),
            Self::Sftp(config) => Ok(Box::new(SftpStore::new(config.clone())?)),
        }
    }
}

#[cfg(test)]
mod target_id_tests {
    use super::*;

    fn s3(bucket: &str, prefix: &str, key: &str) -> StoreConfig {
        StoreConfig::S3(silentsilo_core::S3Config {
            endpoint: "https://s3.example.com".into(),
            region: "eu-central-1".into(),
            bucket: bucket.into(),
            prefix: prefix.into(),
            access_key_id: key.into(),
            secret_access_key: "secret".into(),
            path_style: false,
        })
    }

    #[test]
    fn the_same_bucket_keeps_the_same_name() {
        assert_eq!(
            s3("vault", "silo", "AAA").target_id(),
            s3("vault", "silo", "AAA").target_id()
        );
    }

    #[test]
    fn rotating_a_key_does_not_make_it_a_different_target() {
        // Everything learned about a bucket stays true after the credentials
        // change, and hashing a secret into a database row is a way to leak
        // one.
        assert_eq!(
            s3("vault", "silo", "AAA").target_id(),
            s3("vault", "silo", "BBB").target_id()
        );
    }

    #[test]
    fn a_different_bucket_or_prefix_is_a_different_target() {
        // A different pile of objects, so nothing observed about the old one
        // is true of it.
        assert_ne!(
            s3("vault", "silo", "AAA").target_id(),
            s3("other", "silo", "AAA").target_id()
        );
        assert_ne!(
            s3("vault", "silo", "AAA").target_id(),
            s3("vault", "second", "AAA").target_id()
        );
    }

    #[test]
    fn kinds_pointing_somewhere_similar_do_not_collide() {
        let folder = StoreConfig::Folder {
            path: PathBuf::from("/mnt/backup"),
        };
        assert_ne!(folder.target_id(), s3("mnt", "backup", "AAA").target_id());
    }
}
