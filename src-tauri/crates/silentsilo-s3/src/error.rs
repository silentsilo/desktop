use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum S3Error {
    /// Bad or incomplete connection details — the user can fix these.
    #[error("{0}")]
    Config(String),

    /// Couldn't reach the endpoint, or the response was unusable.
    #[error("storage unreachable: {0}")]
    Transport(String),

    /// The provider answered, and said no.
    #[error("{0}")]
    Service(String),
}

impl S3Error {
    /// Turns an SDK error into something worth showing a user.
    ///
    /// The SDK's own Display is a wrapper like "service error" with the
    /// useful part buried in the source chain, so this walks to the innermost
    /// cause and leads with the HTTP status where there is one — the
    /// difference between 403 and 404 is exactly what tells someone whether
    /// their keys or their bucket name is wrong.
    pub fn from_sdk<E>(operation: &str, err: aws_sdk_s3::error::SdkError<E, HttpResponse>) -> Self
    where
        E: std::error::Error + 'static,
    {
        use aws_sdk_s3::error::SdkError;

        match err {
            SdkError::ServiceError(context) => {
                let status = context.raw().status().as_u16();
                let detail = innermost(context.err());
                let hint = match status {
                    403 => ": check the access key, secret and bucket permissions",
                    404 => ": check the bucket name and endpoint",
                    301 | 307 => ": wrong region for this bucket",
                    _ => "",
                };
                Self::Service(format!("{operation} failed ({status}): {detail}{hint}"))
            }
            SdkError::TimeoutError(_) => Self::Transport(format!("{operation} timed out")),
            SdkError::DispatchFailure(context) => Self::Transport(format!(
                "{operation} could not reach the endpoint: {}",
                describe(&context)
            )),
            other => Self::Transport(format!("{operation} failed: {other}")),
        }
    }
}

fn innermost(err: &(dyn std::error::Error + 'static)) -> String {
    let mut current: &(dyn std::error::Error + 'static) = err;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}

fn describe(context: &impl std::fmt::Debug) -> String {
    format!("{context:?}")
}
