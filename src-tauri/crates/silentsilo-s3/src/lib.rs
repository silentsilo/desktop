//! Object-storage client for any S3-compatible provider.
//!
//! Everything this crate uploads is already ciphertext — encryption happens
//! before it gets here (see `silentsilo-crypto`). Its only job is putting
//! opaque bytes at a key and getting them back.

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use silentsilo_core::S3Config;

mod error;
pub use error::S3Error;

/// An object as returned by [`S3Client::list`].
#[derive(Debug, Clone)]
pub struct ObjectEntry {
    /// Key relative to the configured prefix.
    pub key: String,
    pub size: i64,
    pub etag: Option<String>,
}

/// What a bucket does about deletion, as the bucket reports it.
///
/// Two separate facts, because they fail differently. Versioning alone means
/// a DELETE leaves the old version readable to anyone with
/// `s3:GetObjectVersion`, which makes revoking a key look like it worked when
/// it did not. Object lock means a DELETE is refused outright until the
/// retention expires.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BucketProtection {
    pub versioning: bool,
    pub object_lock: bool,
}

pub struct S3Client {
    client: Client,
    config: S3Config,
}

impl S3Client {
    /// Builds a client for `config`. Each setting exists for a specific
    /// non-AWS incompatibility: checksums `WhenRequired` because several
    /// providers reject the SDK's checksum headers; path-style addressing
    /// as a user switch because guessing wrong produces DNS failures; and
    /// static credentials rather than the default chain, whose IMDS probe
    /// stalls for seconds on a desktop machine.
    pub fn new(config: S3Config) -> Result<Self, S3Error> {
        if config.bucket.trim().is_empty() {
            return Err(S3Error::Config("bucket is required".into()));
        }
        if config.endpoint.trim().is_empty() {
            return Err(S3Error::Config("endpoint is required".into()));
        }

        let credentials = Credentials::new(
            config.access_key_id.trim(),
            config.secret_access_key.trim(),
            None,
            None,
            "silentsilo-user-config",
        );

        // Providers disagree on whether region means anything; several
        // accept any non-empty string. An empty one makes SigV4 signing fail
        // with an error that says nothing useful, so fall back rather than
        // surface that.
        let region = if config.region.trim().is_empty() {
            "us-east-1".to_string()
        } else {
            config.region.trim().to_string()
        };

        let sdk_config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .endpoint_url(config.endpoint.trim().trim_end_matches('/'))
            .credentials_provider(credentials)
            .force_path_style(config.path_style)
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .response_checksum_validation(
                aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired,
            )
            .build();

        Ok(Self {
            client: Client::from_conf(sdk_config),
            config,
        })
    }

    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// Round-trips a small object to prove the endpoint, credentials, bucket
    /// and addressing style all actually work together.
    ///
    /// A read-only check would pass with credentials that can list but not
    /// write, which would then fail on the first real sync — so this writes,
    /// reads back and deletes.
    pub async fn test_connection(&self) -> Result<(), S3Error> {
        const PROBE: &str = ".silentsilo-connection-test";
        let payload = b"silentsilo".to_vec();

        self.put(PROBE, payload.clone()).await?;
        let cold = self.probe_storage_class(PROBE).await;
        let read_back = self.get(PROBE).await?;
        let _ = self.delete(PROBE).await;

        if read_back != payload {
            return Err(S3Error::Config(
                "the bucket returned different bytes than were written".into(),
            ));
        }
        // Found while the user is still looking at the settings, rather than
        // as a sync pass that fails confusingly weeks later. Retrieval from
        // an archive class takes hours, and every read this app makes assumes
        // an object comes back now: opening a file, replaying the log,
        // checking whether a blob is already up there.
        if let Some(class) = cold {
            return Err(S3Error::Config(format!(
                "this bucket stores new objects as {class}, which has to be restored \
                 before it can be read. SilentSilo needs objects it can read straight \
                 away. Use a standard or infrequent-access class."
            )));
        }
        Ok(())
    }

    /// The storage class of a just-written object, if it has to be thawed
    /// before it can be read. Asked of an object rather than the bucket,
    /// because the class comes from the request or a lifecycle rule.
    /// `None` for anything immediately readable.
    async fn probe_storage_class(&self, key: &str) -> Option<String> {
        let class = self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(self.config.key(key))
            .send()
            .await
            .ok()?
            .storage_class?;
        let name = class.as_str().to_string();
        // Only the classes that have to be thawed. Glacier Instant Retrieval
        // reads in milliseconds despite the name, and refusing it would turn
        // down a cheap tier that works perfectly well here.
        matches!(name.as_str(), "GLACIER" | "DEEP_ARCHIVE").then_some(name)
    }

    pub async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), S3Error> {
        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(self.config.key(key))
            .body(ByteStream::from(body))
            .send()
            .await
            .map_err(|e| S3Error::from_sdk("upload", e))?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Vec<u8>, S3Error> {
        let output = self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(self.config.key(key))
            .send()
            .await
            .map_err(|e| S3Error::from_sdk("download", e))?;

        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| S3Error::Transport(e.to_string()))?;
        Ok(bytes.to_vec())
    }

    /// `Ok(None)` when the object simply isn't there, which is an expected
    /// state (nothing synced yet) rather than a failure.
    pub async fn head(&self, key: &str) -> Result<Option<i64>, S3Error> {
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(self.config.key(key))
            .send()
            .await
        {
            Ok(output) => Ok(Some(output.content_length().unwrap_or(0))),
            Err(e) if head_says_absent(&e) => Ok(None),
            Err(e) => Err(S3Error::from_sdk("head", e)),
        }
    }

    /// What the bucket itself says about resisting deletion. Both
    /// questions answer "no" when the provider will not say, because
    /// claiming protection that is not there is the failure this exists to
    /// prevent.
    pub async fn protection(&self) -> BucketProtection {
        let versioning = self
            .client
            .get_bucket_versioning()
            .bucket(&self.config.bucket)
            .send()
            .await
            .ok()
            .and_then(|out| out.status)
            .is_some_and(|s| s.as_str() == "Enabled");

        let object_lock = self
            .client
            .get_object_lock_configuration()
            .bucket(&self.config.bucket)
            .send()
            .await
            .ok()
            .and_then(|out| out.object_lock_configuration)
            .and_then(|c| c.object_lock_enabled)
            .is_some_and(|e| e.as_str() == "Enabled");

        BucketProtection {
            versioning,
            object_lock,
        }
    }

    pub async fn delete(&self, key: &str) -> Result<(), S3Error> {
        self.client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(self.config.key(key))
            .send()
            .await
            .map_err(|e| S3Error::from_sdk("delete", e))?;
        Ok(())
    }

    /// Lists everything under `relative_prefix`, following continuation
    /// tokens — a vault can hold far more than the 1000 keys one page
    /// returns, and a truncated listing would read as "these blobs don't
    /// exist remotely".
    pub async fn list(&self, relative_prefix: &str) -> Result<Vec<ObjectEntry>, S3Error> {
        let full_prefix = self.config.key(relative_prefix);
        let strip = self.config.key("");
        let mut entries = Vec::new();
        let mut continuation: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.config.bucket)
                .prefix(&full_prefix);
            if let Some(token) = continuation.take() {
                request = request.continuation_token(token);
            }

            let output = request
                .send()
                .await
                .map_err(|e| S3Error::from_sdk("list", e))?;

            for object in output.contents() {
                let Some(key) = object.key() else { continue };
                entries.push(ObjectEntry {
                    key: key.strip_prefix(&strip).unwrap_or(key).to_string(),
                    size: object.size().unwrap_or(0),
                    etag: object.e_tag().map(str::to_string),
                });
            }

            if output.is_truncated().unwrap_or(false) {
                continuation = output.next_continuation_token().map(str::to_string);
                if continuation.is_none() {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(entries)
    }
}

/// Whether a failed HEAD means "there is nothing here". S3 answers a HEAD
/// for a missing object with 403 rather than 404 when the caller cannot
/// list that location, which is exactly what a prefix-scoped credential
/// gets. Answering "not there" is safe because every caller uses HEAD to
/// decide whether to write and the writes are idempotent; a genuinely
/// wrong credential still fails loudly on the PUT.
fn head_says_absent<E>(err: &aws_sdk_s3::error::SdkError<E, HttpResponse>) -> bool {
    match err {
        aws_sdk_s3::error::SdkError::ServiceError(context) => {
            status_says_absent(context.raw().status().as_u16())
        }
        _ => false,
    }
}

/// Split out from the SDK error so the decision itself can be tested.
fn status_says_absent(status: u16) -> bool {
    matches!(status, 404 | 403)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> S3Config {
        S3Config {
            endpoint: "http://localhost:9000".into(),
            region: "us-east-1".into(),
            bucket: "vault".into(),
            prefix: "silentsilo".into(),
            access_key_id: "key".into(),
            secret_access_key: "secret".into(),
            path_style: true,
        }
    }

    #[test]
    fn builds_from_a_complete_config() {
        assert!(S3Client::new(config()).is_ok());
    }

    /// A probe that cannot list gets 403 for a key that is not there, which
    /// is what a prefix-scoped credential sees on a prefix nothing has been
    /// written to yet. Treating it as a failure stopped the first pass
    /// against hosted storage before it could write anything, and since
    /// nothing was written the next pass saw the same 403.
    #[test]
    fn a_refused_probe_counts_as_nothing_there() {
        assert!(status_says_absent(404));
        assert!(status_says_absent(403));
    }

    /// Only the two answers that mean "no object". A broken endpoint or a
    /// provider having a bad day must still reach the user.
    #[test]
    fn other_failures_are_still_failures() {
        for status in [400, 401, 429, 500, 503] {
            assert!(!status_says_absent(status), "{status} should surface");
        }
    }

    #[test]
    fn rejects_a_missing_bucket() {
        let mut c = config();
        c.bucket = "   ".into();
        assert!(matches!(S3Client::new(c), Err(S3Error::Config(_))));
    }

    #[test]
    fn rejects_a_missing_endpoint() {
        let mut c = config();
        c.endpoint = String::new();
        assert!(matches!(S3Client::new(c), Err(S3Error::Config(_))));
    }

    #[test]
    fn an_empty_region_does_not_prevent_construction() {
        // Several providers ignore region entirely, but SigV4 signing needs
        // *something*; an empty value would otherwise fail later with an
        // error that points nowhere near the actual cause.
        let mut c = config();
        c.region = "  ".into();
        assert!(S3Client::new(c).is_ok());
    }

    #[test]
    fn a_trailing_slash_on_the_endpoint_is_tolerated() {
        let mut c = config();
        c.endpoint = "http://localhost:9000/".into();
        assert!(S3Client::new(c).is_ok());
    }
}
