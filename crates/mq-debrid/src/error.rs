use mq_http::HttpError;
use std::time::Duration;

#[derive(Debug, Clone, thiserror::Error)]
pub enum DebridError {
    #[error("authentication failed")]
    Auth,
    #[error("account is not premium")]
    NotPremium,
    #[error("torrent is not cached")]
    NotCached,
    #[error("rate limited (retry after {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },
    #[error("quota or active limit reached")]
    Quota,
    #[error("provider error {code}: {message}")]
    Provider { code: String, message: String },
    #[error("http: {0}")]
    Http(HttpError),
    #[error("timed out")]
    Timeout,
    #[error("no playable file in torrent")]
    NoPlayableFile,
}

impl DebridError {
    pub(crate) fn provider(code: impl Into<String>, message: impl Into<String>) -> Self {
        DebridError::Provider {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<HttpError> for DebridError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Timeout(_) => DebridError::Timeout,
            other => DebridError::Http(other),
        }
    }
}
