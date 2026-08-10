//! Domain types and errors for SilentSilo.

pub mod durable;
mod error;
mod types;

pub use durable::{rename_with_retry, write_replacing};
pub use error::{CoreError, CoreResult};
pub use types::{
    DeviceInfo, FileEntry, FolderEntry, S3Config, S3ConfigView, SearchHit, TransferJob, TrashItem,
    VaultEntry, VaultMeta,
};
