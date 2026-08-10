//! The S3 client, seen through the common contract.
//!
//! A thin adaptation rather than a rewrite: `S3Client` already had exactly
//! these five operations, which is what made the trait worth extracting —
//! the shape was right before it had a name.

use async_trait::async_trait;
use silentsilo_s3::{S3Client, S3Error};

use crate::{ObjectStore, StoreError, StoredObject};

/// Keeps the distinctions the caller can act on.
///
/// "Wrong key" and "no network" lead to different fixes in different places,
/// and flattening both into one message is how a user ends up checking their
/// credentials because their wifi dropped.
fn map(err: S3Error) -> StoreError {
    let text = err.to_string();
    let lower = text.to_lowercase();
    // A lifecycle rule moved this object to an archive class after it was
    // written, so it exists but cannot be read until a restore finishes,
    // which takes hours. Left as the raw provider error it reads as a
    // corrupt or missing object, which is the wrong thing to go and check.
    if lower.contains("invalidobjectstate") {
        return StoreError::Other(format!(
            "this object is in cold storage and has to be restored before it can be \
             read, which takes hours. SilentSilo cannot use an archive storage class. \
             Check the bucket's lifecycle rules. ({text})"
        ));
    }
    if lower.contains("nosuchkey") || lower.contains("not found") || lower.contains("404") {
        StoreError::NotFound(text)
    } else if lower.contains("403")
        || lower.contains("accessdenied")
        || lower.contains("signaturedoesnotmatch")
        || lower.contains("invalidaccesskeyid")
    {
        StoreError::Denied(text)
    } else if lower.contains("dispatch failure")
        || lower.contains("connect")
        || lower.contains("timeout")
        || lower.contains("dns")
    {
        StoreError::Unreachable(text)
    } else {
        StoreError::Other(text)
    }
}

#[async_trait]
impl ObjectStore for S3Client {
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), StoreError> {
        S3Client::put(self, key, body).await.map_err(map)
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
        S3Client::get(self, key).await.map_err(map)
    }

    async fn put_from_file(&self, key: &str, path: &std::path::Path) -> Result<(), StoreError> {
        S3Client::put_file(self, key, path).await.map_err(map)
    }

    async fn get_to_file(&self, key: &str, dest: &std::path::Path) -> Result<(), StoreError> {
        S3Client::get_file(self, key, dest).await.map_err(map)
    }

    async fn head(&self, key: &str) -> Result<Option<i64>, StoreError> {
        S3Client::head(self, key).await.map_err(map)
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        S3Client::delete(self, key).await.map_err(map)
    }

    async fn protection(&self) -> crate::Protection {
        let found = S3Client::protection(self).await;
        crate::Protection {
            versioning: found.versioning,
            object_lock: found.object_lock,
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<StoredObject>, StoreError> {
        let entries = S3Client::list(self, prefix).await.map_err(map)?;
        Ok(entries
            .into_iter()
            .map(|e| StoredObject {
                key: e.key,
                size: e.size,
            })
            .collect())
    }

    fn describe(&self) -> String {
        let config = self.config();
        if config.prefix.is_empty() {
            config.bucket.clone()
        } else {
            format!("{}/{}", config.bucket, config.prefix.trim_matches('/'))
        }
    }

    async fn check(&self) -> Result<(), StoreError> {
        // The S3 client's own probe already covers write, read and delete,
        // and reports provider-specific failures better than a generic
        // round trip would.
        self.test_connection().await.map_err(map)
    }
}
