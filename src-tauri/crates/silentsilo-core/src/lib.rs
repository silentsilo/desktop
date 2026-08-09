//! Domain types and errors for SilentSilo.

mod error;
mod types;

pub use error::{CoreError, CoreResult};
pub use types::{
    DeviceInfo, FileEntry, FolderEntry, S3Config, S3ConfigView, SearchHit, TransferJob, TrashItem,
    VaultEntry, VaultMeta,
};
