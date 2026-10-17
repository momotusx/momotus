//! One-way import of a Stremio account into the local library.
//!
//! Three steps, in this order, so a failure always leaves something to
//! recover from:
//!
//! 1. `StremioClient::fetch`: network reads only, nothing is written.
//! 2. The raw responses are stored with `store_raw_import("stremio", ..)`.
//! 3. `apply_snapshot` maps them into the profile. It is idempotent and takes
//!    the stored payload, so a failed or partial run is simply run again.
//!
//! Sign-in is the device-code flow (`LinkLogin`), so no password is typed.
//! The session key leaves this crate only through `AuthKey::expose`; the
//! caller passes it to its keystore. This crate never logs or stores it.

mod api;
mod map;

pub use api::{AuthKey, Endpoints, LinkLogin, LinkPoll, StremioClient};
pub use map::{ImportReport, SeriesVideos, SkippedAddon, Snapshot, apply_snapshot};

use mq_library::{Library, LibraryError, hlc::now_ms};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the Stremio session is no longer valid, link the account again")]
    InvalidAuthKey,
    #[error("stremio api error {code}: {message}")]
    Api { code: u64, message: String },
    #[error(transparent)]
    Http(#[from] mq_http::HttpError),
    #[error("unexpected response: {0}")]
    Decode(String),
    #[error("could not keep the raw import: {0}")]
    Store(LibraryError),
    #[error(
        "mapping failed, the raw import is kept as #{raw_import_id} and can be applied again: {source}"
    )]
    Apply {
        raw_import_id: i64,
        source: LibraryError,
    },
}

/// Fetch, keep the raw import, map. `videos` holds the series video lists the
/// caller already has (can be empty; series that need one are listed in
/// `deferred_watched_items`).
pub async fn import_account(
    client: &StremioClient,
    lib: &Library,
    profile_id: &str,
    profile_name: &str,
    videos: &SeriesVideos,
) -> Result<ImportReport, ImportError> {
    let snapshot = client.fetch().await?;
    let now = now_ms();
    let payload =
        serde_json::to_string(&snapshot).map_err(|e| ImportError::Decode(e.to_string()))?;
    let raw_import_id = lib
        .store_raw_import("stremio", now as i64, &payload)
        .map_err(ImportError::Store)?;
    let mut report = apply_snapshot(lib, profile_id, profile_name, &snapshot, videos, now)
        .map_err(|source| ImportError::Apply {
            raw_import_id,
            source,
        })?;
    report.raw_import_id = Some(raw_import_id);
    Ok(report)
}
