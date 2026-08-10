use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Marked for the Favourites list. Part of the entry rather than a local
    /// preference: a favourite that only exists on the machine that starred
    /// it would be a feature that lies on the second device.
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: Uuid,
    pub folder_id: Uuid,
    pub name: String,
    pub blob_id: Uuid,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub content_hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// See [`FolderEntry::favorite`].
    pub favorite: bool,
}

/// A device that has written to this silo's log.
///
/// Derived from the log rather than from a registry: there is nowhere to
/// register, and a device that has never written anything has left no trace
/// to list. Removing one is not offered here, because revoking access means
/// revoking its key, which Settings already does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: Uuid,
    /// What someone named it, if anyone has. Wins over `system_name`, which
    /// is why re-announcing a machine cannot overwrite it.
    pub label: Option<String>,
    /// What the machine calls itself: its computer name.
    pub system_name: Option<String>,
    /// The system it runs, e.g. "Windows 11 Pro".
    pub platform: Option<String>,
    /// The device this silo is open on right now.
    pub is_this_device: bool,
    /// How many operations of the log it wrote.
    pub operations: i64,
    /// That device's own clock when it last changed something. Zero for a
    /// log written before this was recorded.
    pub last_change_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VaultEntry {
    Folder(FolderEntry),
    File(FileEntry),
}

/// A search result plus the folder it lives in.
///
/// The path is what makes a hit useful: three files called "scan.pdf" are
/// indistinguishable without it, and the user's next action is almost always
/// to go to where it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub entry: VaultEntry,
    pub folder_path: String,
}

/// A trashed entry plus the path of the folder it was trashed out of, so
/// the Trash view can show where each item came from (its own name already
/// appears separately, so this is the *containing* folder's path, e.g.
/// "/Documents/Receipts" — not the item's own full path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    #[serde(flatten)]
    pub entry: VaultEntry,
    pub original_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: Uuid,
    pub job_type: String,
    pub file_id: Option<Uuid>,
    pub local_path: String,
    pub status: String,
    pub progress: f64,
    pub bytes_total: Option<i64>,
    pub bytes_done: Option<i64>,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultMeta {
    pub revision: i64,
    pub vault_id: Uuid,
}

/// Connection details for the user's own S3-compatible bucket. Sync is
/// optional: with no config stored the app is purely local. `path_style`
/// and the free-form `region` exist because non-AWS endpoints disagree on
/// both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// Key prefix inside the bucket, so one bucket can host other things.
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub path_style: bool,
}

impl S3Config {
    /// Full object key for `relative`, under the configured prefix.
    pub fn key(&self, relative: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        if prefix.is_empty() {
            relative.trim_start_matches('/').to_string()
        } else {
            format!("{prefix}/{}", relative.trim_start_matches('/'))
        }
    }
}

/// The same connection details minus the secret, for sending to the UI —
/// the secret access key is write-only from the frontend's perspective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ConfigView {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key_id: String,
    pub path_style: bool,
}

impl From<&S3Config> for S3ConfigView {
    fn from(c: &S3Config) -> Self {
        Self {
            endpoint: c.endpoint.clone(),
            region: c.region.clone(),
            bucket: c.bucket.clone(),
            prefix: c.prefix.clone(),
            access_key_id: c.access_key_id.clone(),
            path_style: c.path_style,
        }
    }
}
