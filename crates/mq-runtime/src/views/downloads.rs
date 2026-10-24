//! Downloads: what this device keeps for watching with no connection
//! (plans/offline-downloads.md 3.4, 4.15). Every standard profile sees the
//! device's downloads; a kids profile sees those of its allowed titles.

use crate::Core;
use crate::downloads::{Board, DownloadRow, DownloadSettings, DownloadState};
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::pin::Pin;
use tokio::task::JoinHandle;

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsSnapshot {
    pub storage: DownloadStorage,
    pub settings: DownloadSettings,
    /// What the platform's keeper shows (a notification, the system's
    /// progress) and whether it keeps the app running.
    pub active: DownloadActivity,
    /// Queued, running, paused and waiting, in queue order.
    pub downloading: Vec<DownloadRow>,
    /// Done, failed and missing, by title, newest first.
    pub titles: Vec<DownloadTitle>,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadStorage {
    /// What downloads hold on disk.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub downloads_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub free_bytes: Option<u64>,
    /// More space a download waiting for it needs.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub needed_bytes: Option<u64>,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadActivity {
    pub running: u32,
    pub queued: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes_done: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes_total: u64,
    /// The title the first running download belongs to.
    pub title: Option<String>,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTitle {
    pub item_id: String,
    pub kind: String,
    pub name: String,
    pub poster: Option<String>,
    pub count: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes: u64,
    pub rows: Vec<DownloadRow>,
}

pub struct DownloadsView {
    snapshot: SharedObservable<DownloadsSnapshot>,
    task: JoinHandle<()>,
}

impl DownloadsView {
    pub fn open(core: &Core) -> Self {
        let downloads = core.downloads();
        let mut boards = downloads.board().subscribe();
        let snapshot = SharedObservable::new(render(core, &downloads.board().get()));
        let (core2, out) = (core.clone(), snapshot.clone());
        let mut changes = core.library().subscribe();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    board = boards.next() => {
                        let Some(board) = board else { return };
                        out.set(render(&core2, &board));
                    }
                    // A kids list changed: the rows it allows change.
                    tables = crate::views::next_change(&mut changes) => {
                        let Some(tables) = tables else { return };
                        if tables.contains(&mq_library::Table::Access) {
                            out.set(render(&core2, &core2.downloads().board().get()));
                        }
                    }
                }
            }
        });
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> DownloadsSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        DownloadsSnapshot,
        Pin<Box<dyn Stream<Item = Vec<DownloadsSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for DownloadsView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The rows this profile may see: a kids profile, those of its list.
pub(crate) fn visible(core: &Core, board: &Board) -> Vec<DownloadRow> {
    let policy = core.policy();
    board
        .rows
        .iter()
        .filter(|r| policy.allows(&r.kind, &r.item_id))
        .cloned()
        .collect()
}

fn render(core: &Core, board: &Board) -> DownloadsSnapshot {
    let rows = visible(core, board);
    let is_kept = |r: &&DownloadRow| {
        matches!(
            r.state,
            DownloadState::Done | DownloadState::Failed | DownloadState::Missing
        )
    };
    let downloading: Vec<DownloadRow> = rows.iter().filter(|r| !is_kept(r)).cloned().collect();
    let mut titles: Vec<DownloadTitle> = Vec::new();
    // Newest first: the queue's order is oldest first.
    for row in rows.iter().filter(is_kept).rev() {
        match titles.iter_mut().find(|t| t.item_id == row.item_id) {
            Some(title) => {
                title.count += 1;
                title.bytes += row.bytes_done;
                title.rows.push(row.clone());
            }
            None => titles.push(DownloadTitle {
                item_id: row.item_id.clone(),
                kind: row.kind.clone(),
                name: row.name.clone(),
                poster: row.poster.clone(),
                count: 1,
                bytes: row.bytes_done,
                rows: vec![row.clone()],
            }),
        }
    }
    for title in &mut titles {
        title
            .rows
            .sort_by_key(|r| (r.season.unwrap_or(0), r.episode.unwrap_or(0)));
    }
    let running: Vec<&DownloadRow> = downloading
        .iter()
        .filter(|r| r.state == DownloadState::Running)
        .collect();
    let active_rows = downloading
        .iter()
        .filter(|r| matches!(r.state, DownloadState::Running | DownloadState::Queued));
    let (bytes_done, bytes_total) = active_rows.fold((0, 0), |(d, t), r| {
        (d + r.bytes_done, t + r.bytes_total.unwrap_or(0))
    });
    DownloadsSnapshot {
        storage: DownloadStorage {
            downloads_bytes: rows.iter().map(|r| r.bytes_done).sum(),
            free_bytes: core.free_bytes(),
            needed_bytes: board.needed_bytes,
        },
        settings: board.settings,
        active: DownloadActivity {
            running: running.len() as u32,
            queued: downloading
                .iter()
                .filter(|r| r.state == DownloadState::Queued)
                .count() as u32,
            bytes_done,
            bytes_total,
            title: running.first().map(|r| r.name.clone()),
        },
        downloading,
        titles,
    }
}
