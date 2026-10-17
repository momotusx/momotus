//! Import from a Stremio account: device-code sign-in (no password typed
//! here), then addons, library, progress and watched marks.
//!
//! Series watched marks only decode against the series' episode list, so the
//! lists are fetched from the meta addons after the first pass and the stored
//! raw import is applied once more.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_import::{
    Endpoints, ImportError, ImportReport, LinkLogin, LinkPoll, SeriesVideos, Snapshot,
    StremioClient, apply_snapshot, import_account,
};
use serde::Serialize;
use std::pin::Pin;
use std::time::Duration;
use tokio::task::JoinHandle;

const POLL_EVERY: Duration = Duration::from_secs(2);
/// Episode lists fetched at once for the watched-marks pass.
const META_CONCURRENCY: usize = 6;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum ImportSnapshot {
    Starting,
    /// Show `code` (and the QR image) and ask the user to open `linkUrl`
    /// while signed in to Stremio.
    #[serde(rename_all = "camelCase")]
    WaitingForLink {
        code: String,
        link_url: String,
        qr_url: Option<String>,
    },
    Importing {
        step: String,
    },
    Done {
        summary: ImportSummary,
    },
    /// The code was not used in time; open the view again for a new one.
    Expired,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub addons_imported: u32,
    /// "Name: reason" for each addon that could not be imported.
    pub addons_skipped: Vec<String>,
    pub titles: u32,
    pub in_progress: u32,
    pub watched: u32,
    /// Series whose watched marks could not be matched to episodes.
    pub series_unmatched: u32,
}

pub struct ImportView {
    snapshot: SharedObservable<ImportSnapshot>,
    task: JoinHandle<()>,
}

impl ImportView {
    /// Starts the sign-in. `Endpoints::default()` is the real Stremio service.
    pub fn open(core: &Core, endpoints: Endpoints) -> Self {
        let snapshot = SharedObservable::new(ImportSnapshot::Starting);
        let task = tokio::spawn(run(core.clone(), endpoints, snapshot.clone()));
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> ImportSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        ImportSnapshot,
        Pin<Box<dyn Stream<Item = Vec<ImportSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for ImportView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(core: Core, endpoints: Endpoints, snapshot: SharedObservable<ImportSnapshot>) {
    let end = match import(&core, endpoints, &snapshot).await {
        Ok(Some(report)) => ImportSnapshot::Done {
            summary: summary(&report),
        },
        Ok(None) => ImportSnapshot::Expired,
        Err(e) => ImportSnapshot::Failed {
            reason: e.to_string(),
        },
    };
    snapshot.set(end);
}

/// `Ok(None)` when the link code expired.
async fn import(
    core: &Core,
    endpoints: Endpoints,
    snapshot: &SharedObservable<ImportSnapshot>,
) -> Result<Option<ImportReport>, ImportError> {
    let login = LinkLogin::start(core.http().clone(), endpoints.clone()).await?;
    snapshot.set(ImportSnapshot::WaitingForLink {
        code: login.code.clone(),
        link_url: login.link_url.clone(),
        qr_url: login.qr_url.clone(),
    });
    let auth_key = loop {
        tokio::time::sleep(POLL_EVERY).await;
        match login.poll().await {
            Ok(LinkPoll::Pending) => {}
            Ok(LinkPoll::Authorized { auth_key }) => break auth_key,
            Ok(LinkPoll::Expired) => return Ok(None),
            // A dropped connection is retried on the next tick.
            Err(ImportError::Http(e)) => tracing::debug!("link poll failed: {e}"),
            Err(e) => return Err(e),
        }
    };

    step(snapshot, "Reading your Stremio account");
    let client =
        StremioClient::login_with_auth_key(core.http().clone(), endpoints, auth_key).await?;
    let (library, profile) = (core.library(), core.profile());
    let mut report =
        import_account(&client, library, profile, profile, &SeriesVideos::new()).await?;
    let (Some(raw_id), false) = (
        report.raw_import_id,
        report.deferred_watched_items.is_empty(),
    ) else {
        return Ok(Some(report));
    };

    step(
        snapshot,
        &format!(
            "Matching watched episodes for {} series",
            report.deferred_watched_items.len()
        ),
    );
    let videos = series_videos(core, &report.deferred_watched_items).await;
    let stored = library
        .raw_import(raw_id)
        .map_err(ImportError::Store)?
        .ok_or_else(|| ImportError::Decode("stored import is missing".into()))?;
    let stored: Snapshot =
        serde_json::from_str(&stored).map_err(|e| ImportError::Decode(e.to_string()))?;
    let now = mq_library::hlc::now_ms();
    report =
        apply_snapshot(library, profile, profile, &stored, &videos, now).map_err(|source| {
            ImportError::Apply {
                raw_import_id: raw_id,
                source,
            }
        })?;
    report.raw_import_id = Some(raw_id);
    Ok(Some(report))
}

fn step(snapshot: &SharedObservable<ImportSnapshot>, text: &str) {
    snapshot.set(ImportSnapshot::Importing {
        step: text.to_owned(),
    });
}

/// Episode lists from the installed meta addons; titles that fail are left
/// out and stay counted as unmatched.
async fn series_videos(core: &Core, ids: &[String]) -> SeriesVideos {
    futures_util::stream::iter(ids.iter().cloned())
        .map(|id| async move {
            let meta = core.fetch_meta("series", &id).await.ok()?;
            Some((id, meta.0.videos))
        })
        .buffer_unordered(META_CONCURRENCY)
        .filter_map(|found| async move { found })
        .collect()
        .await
}

fn summary(report: &ImportReport) -> ImportSummary {
    ImportSummary {
        addons_imported: report.addons_imported as u32,
        addons_skipped: report
            .addons_skipped
            .iter()
            .map(|s| format!("{}: {}", s.name, s.reason))
            .collect(),
        titles: report.items_imported as u32,
        in_progress: report.in_progress as u32,
        watched: report.watched_marked as u32,
        series_unmatched: report.deferred_watched_items.len() as u32,
    }
}
