//! Offline downloads where they cannot run (plans/webos.md 3.4: web apps on
//! webOS cannot reach a service's files). The surface of `downloads` with
//! nothing in it: the board is empty, settings read as the defaults, and
//! every change answers "not on this device". The apps already hide the
//! controls (`downloadsAllowed` is false there).

use crate::core::WeakCore;
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use mq_protocol::MetaItem;
use mq_streams::ParsedStream;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

fn off() -> CoreError {
    CoreError::Downloads("downloads are not on this device".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Queued,
    Running,
    Paused,
    Waiting,
    Done,
    Failed,
    Missing,
}

impl DownloadState {
    pub(crate) fn holds_part(self) -> bool {
        !matches!(self, Self::Done | Self::Failed | Self::Missing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadReason {
    Wifi,
    LowData,
    Space,
    Network,
    Playing,
    App,
    NoVersion,
    Source,
    Retries,
    NotVideo,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Facts {
    pub name: String,
    pub episode_title: Option<String>,
    pub poster: Option<String>,
    pub still: Option<String>,
    pub runtime: Option<String>,
    pub release_info: Option<String>,
    pub released: Option<String>,
    pub episodes_in_season: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    pub id: String,
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub name: String,
    pub episode_title: Option<String>,
    pub poster: Option<String>,
    pub still: Option<String>,
    pub runtime: Option<String>,
    pub released: Option<String>,
    pub state: DownloadState,
    pub reason: Option<DownloadReason>,
    pub mobile_data: bool,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: Option<u64>,
    pub eta_secs: Option<u64>,
    pub quality: Option<String>,
    pub subtitles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBadge {
    pub id: String,
    pub state: DownloadState,
    pub reason: Option<DownloadReason>,
    pub mobile_data: bool,
    pub progress: Option<f32>,
    pub bytes_total: Option<u64>,
}

impl DownloadRow {
    pub fn badge(&self) -> DownloadBadge {
        DownloadBadge {
            id: self.id.clone(),
            state: self.state,
            reason: self.reason,
            mobile_data: self.mobile_data,
            progress: None,
            bytes_total: self.bytes_total,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    pub wifi_only: bool,
    pub smart: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            wifi_only: true,
            smart: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Link {
    pub offline: bool,
    pub metered: bool,
    pub low_data: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    pub rows: Vec<DownloadRow>,
    pub settings: DownloadSettings,
    pub link: LinkWords,
    pub needed_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkWords {
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
    Retry { mobile_data: bool },
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Video,
    Season,
    Series,
}

pub struct AddRequest {
    pub kind: String,
    pub item_id: String,
    pub scope: Scope,
    pub video_id: Option<String>,
    pub season: Option<u32>,
    pub params: serde_json::Value,
    pub pick: Option<Vec<ParsedStream>>,
}

/// Held by a playback streaming from the network; nothing waits on it here.
pub struct Streaming;

/// Held by a playback of a download's own version; there is none here.
pub struct InUse;

/// A download's file, for a play; there is none here.
pub(crate) struct Local {
    pub(crate) path: PathBuf,
    pub(crate) len: u64,
    pub(crate) held: Option<tokio::sync::watch::Receiver<u64>>,
    pub(crate) file_name: String,
}

pub struct Downloads {
    board: SharedObservable<Board>,
    link: std::sync::Mutex<Link>,
}

impl Downloads {
    pub(crate) fn new(_core: WeakCore, _dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            board: SharedObservable::new(Board::default()),
            link: std::sync::Mutex::default(),
        })
    }

    pub(crate) fn start(self: &Arc<Self>) {}

    pub fn board(&self) -> &SharedObservable<Board> {
        &self.board
    }

    pub fn settings(&self) -> DownloadSettings {
        DownloadSettings::default()
    }

    pub fn set_settings(&self, _: Option<bool>, _: Option<bool>) -> Result<(), CoreError> {
        Err(off())
    }

    /// Kept for `link()`: playback reads whether the device is offline.
    pub fn set_link(&self, link: Link) {
        *self.link.lock().unwrap_or_else(|p| p.into_inner()) = link;
    }

    pub fn link(&self) -> Link {
        *self.link.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn set_allowed(&self, _: bool, _: Option<u64>) {}

    pub fn streaming(self: &Arc<Self>, _: Option<&str>) -> Streaming {
        Streaming
    }

    pub fn act(&self, _: &[String], _: Action) -> Result<(), CoreError> {
        Err(off())
    }

    pub(crate) fn bad_file(&self, _: &str, _: &str, _: bool) {}

    pub(crate) fn playing(self: &Arc<Self>, _: &str, _: &str, _: &str) -> Option<InUse> {
        None
    }

    pub(crate) fn finished(self: &Arc<Self>, _: &Core, _: &str, _: &str) {}

    pub(crate) fn kept(&self, _: &str, _: &str) -> Option<(ParsedStream, DownloadRow)> {
        None
    }

    pub(crate) fn has_peers(&self, _: &str) -> bool {
        false
    }

    pub fn ids(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn ids_of(&self, _: &str) -> Vec<String> {
        Vec::new()
    }

    pub(crate) fn downloaded_meta(&self, _: &str, _: &str) -> Option<MetaItem> {
        None
    }

    pub(crate) fn local(&self, _: &str, _: &str) -> Option<(String, Local)> {
        None
    }

    pub fn relaunch(&self) {}

    pub fn report_free(&self, _: u64, _: std::time::Duration) {}

    pub(crate) fn free(&self, _: &Core) -> Option<u64> {
        None
    }
}

pub fn owner(core: &Core) -> Result<Arc<Downloads>, CoreError> {
    let _ = core;
    Err(off())
}

pub async fn add(_: &Core, _: AddRequest) -> Result<Vec<String>, CoreError> {
    Err(off())
}

pub(crate) fn names_torrent(_: &mq_library::Library, _: &str) -> bool {
    false
}
