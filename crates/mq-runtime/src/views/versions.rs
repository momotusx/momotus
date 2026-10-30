//! Versions (stream picker): queries every stream addon for one video,
//! normalizes and ranks the union on each arrival, and publishes the pick
//! with its reasons, intent groups and hidden rows.
//!
//! The recommendation is stable once the user starts moving focus
//! (`freeze`), so late addon results never swap the row under the cursor.

use crate::Core;
use crate::learn::LearnedNote;
use crate::live::{FRAME, coalesce};
use crate::speed::Network;
use crate::views::addons_changed;
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines, InstalledAddon};
use mq_library::{Policy, Table};
use mq_protocol::{ResourcePath, decode_streams};
use mq_streams::{
    Conditions, DeviceCaps, Group, HideReason, ParsedStream, Playability, RankInput, RankKey,
    Ranked, RankedRow, Reason, ReasonLine, SourceClass, SourceKind, Speed, StreamPrefs, Target,
    kind, normalize, rank, screen_class,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionRow {
    pub key: String,
    /// "4K", "1440p", "1080p", "720p" or "SD"; when the release names no
    /// resolution, its source ("Blu-ray", "WEB-DL") or "Quality not stated".
    pub quality: String,
    /// Words, never emoji: "Dolby Vision", "HDR10", "Atmos", "5.1", "Remux",
    /// and "AD" for audio described for blind and low-vision viewers.
    pub features: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_bytes: Option<u64>,
    pub status: VersionStatus,
    /// Service ("Real-Debrid") or addon name.
    pub via: String,
    pub release_name: String,
    pub languages: Vec<String>,
    pub reasons: Vec<Reason>,
    pub group: Group,
    pub failover_count: u16,
    /// What the row tells the user, with its numbers (3.7).
    pub line: ReasonLine,
    /// How it starts (3.2).
    pub source_class: SourceClass,
    /// How it plays, as the viewer groups versions: the kind chips
    /// (plans/plugins-and-marketplace.md 4.2).
    pub kind: SourceKind,
    /// Size over runtime after the pack guard; null when either is unknown.
    pub bitrate_mbps: Option<f32>,
    /// The sort key, for a details panel and tests.
    pub score: RankKey,
    /// "On this device": the version last played for this video, and this
    /// device holds its start at the resume point, so it starts with no
    /// peer (plans/player-polish.md 2b). Show it as the row's line.
    pub on_device: Option<String>,
    /// Where the row comes from (plans/plugins-and-marketplace.md 4.4).
    pub origin: RowOrigin,
    /// The host an Archive row plays from, in punycode; null for an
    /// addon's row.
    pub host: Option<String>,
    /// "Internet Archive" on an Archive row; null for an addon's row.
    pub source_name: Option<String>,
}

/// Where a row comes from: an installed addon, ranked for the pick, or a
/// built-in source listed in a section of its own and never picked
/// (plans/plugins-and-marketplace.md 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum RowOrigin {
    Addon,
    Archive,
    Plugin,
}

/// The rows of a source that ranks in a pass of its own, in that pass's
/// order, and whether it is still working (an item being read).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SourceRows {
    pub rows: Vec<VersionRow>,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VersionStatus {
    Ready { service: Option<String> },
    Torrent { seeds: Option<u32> },
    NeedsDownload { service: Option<String> },
    Direct,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HiddenSummary {
    pub count: u32,
    pub by_reason: Vec<(HideReason, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionsSnapshot {
    /// What the ranking read: the screen, the connection, the disk, the
    /// title and the preferences in effect (4.2).
    pub conditions: VersionsConditions,
    /// Every addon answered, nothing is listed, and the hidden rows say
    /// why (3.11).
    pub blocked: Option<BlockedVersions>,
    /// Nothing can play, and why, named by the core with the one action
    /// the apps offer (plans/play-every-version.md 2a). None while anything
    /// plays or the view is still asking.
    pub empty: Option<EmptyVersions>,
    /// The one-time note when a pick taught Marquee something (3.9).
    pub note: Option<LearnedNote>,
    pub recommended: Option<VersionRow>,
    pub rows: Vec<VersionRow>,
    pub hidden: HiddenSummary,
    pub pending_addons: Vec<String>,
    pub slow_addons: Vec<String>,
    pub failed_addons: Vec<String>,
    pub done: bool,
    pub frozen: bool,
    /// The "Smaller file" shortcut (plans/play-every-version.md 2f): the
    /// first row in the core's order under half the pick's size that can
    /// start and is in the viewer's language. Frozen with the pick.
    pub smaller: Option<VersionRow>,
    /// The "Sharper" shortcut: the first row in the core's order sharper
    /// than the pick, up to this screen's class, that can start and is in
    /// the viewer's language. Frozen with the pick.
    pub sharper: Option<VersionRow>,
    /// The quality sections of `rows`, in this order and only those with
    /// rows: 4K, 1080p (1080p and 1440p), 720p and below, other (quality
    /// not stated); each lists its rows' keys in the core's order.
    pub sections: Vec<VersionSection>,
    /// Every listed version is a swarm the engine would fetch whose
    /// trackers count one seeder or none: almost nobody is sharing this
    /// title right now, so say so before Play. Its attempt keeps the stall
    /// windows and the full deadline, but a swarm where no peer unchoked by
    /// the end of the first window (30 s) is let go then.
    pub nobody_sharing: bool,
    /// "From the Internet Archive" (plans/plugins-and-marketplace.md 4.4,
    /// 5): its own pass, after the addons' rows, never the pick, never a
    /// failover; `done` never waits for it.
    pub archive: SourceRows,
    /// "From plugins" (plans/plugins-and-marketplace.md 4.4, 7.8): the
    /// Nuvio providers' rows, a pass of their own like the Archive's.
    pub plugins: SourceRows,
    /// The kind chips (plans/plugins-and-marketplace.md 4.2): each kind the
    /// listed rows hold, with how many rows hold a source of it. The apps
    /// show them from two kinds on.
    pub kinds: Vec<KindChip>,
    /// The kind `setKind` chose: `recommended`, `rows` and `sections` are
    /// then that kind's own pass, and `smaller` and `sharper` are null.
    pub selected_kind: Option<SourceKind>,
}

/// A kind chip: the kind, the rows holding a source of it, and for debrid
/// the one service every such source resolves on ("Real-Debrid": the one it
/// names, or the only account; null otherwise, and the chip says "Debrid").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KindChip {
    pub kind: SourceKind,
    pub service: Option<String>,
    pub count: u32,
}

/// What the ranking read for this view, for the header "This TV: 4K HDR,
/// about 42 Mbit/s on Wi-Fi, 61 GB free".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionsConditions {
    /// 2160, 1080, 720 or 480 (3.3).
    pub screen_class: u32,
    /// The screen shows HDR and the user wants it.
    pub hdr: bool,
    pub network: Network,
    pub http: Speed,
    pub p2p: Speed,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub free_bytes: Option<u64>,
    pub runtime_min: Option<u32>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_limit_bytes: Option<u64>,
    pub min_seeders: u32,
    /// This build has the torrent engine.
    pub engine: bool,
    /// The profile has a streaming service account.
    pub account: bool,
}

/// Why nothing is listed (3.11), how many versions were found, and those
/// rows ranked as they would be once the reason is fixed ("Show them").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct BlockedVersions {
    pub reason: BlockedReason,
    pub count: u32,
    pub rows: Vec<VersionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum BlockedReason {
    /// Torrents this build opens only through a paid streaming service.
    NeedsAccount,
    /// Nothing found plays on this device.
    NothingPlaysHere,
}

/// One quality section of the Versions page (plans/play-every-version.md
/// 2f).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionSection {
    pub quality: SectionQuality,
    /// Keys of `rows`, in the core's order.
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum SectionQuality {
    /// 2160p and above.
    Uhd,
    /// 1080p and 1440p.
    Fhd,
    /// 720p and below.
    Hd,
    /// The release states no resolution.
    Other,
}

/// Why nothing can play (plans/play-every-version.md 2a), with its one
/// action. The words are `empty.<kind>` in apps/shared/stream-choice-words.json.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct EmptyVersions {
    pub kind: EmptyKind,
    pub action: EmptyAction,
    /// The versions found, for the states that have some.
    pub count: u32,
    /// Those versions ranked as they would play once the reason is gone:
    /// shown dimmed for needsAccount, startable each for nothingPlaysHere
    /// ("Choose a version").
    pub rows: Vec<VersionRow>,
    /// The version `playFromDevice` starts (`pick` takes it).
    pub key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum EmptyKind {
    /// No stream addon is installed.
    NoSource,
    /// Every addon failed (offline included: the network has no offline
    /// kind).
    Unreachable,
    /// Every version plays only through a streaming account the viewer has
    /// not set up.
    NeedsAccount,
    /// A kids profile: nothing here is on its list.
    KidsNothing,
    /// Every addon answered with nothing, or only with what is not a
    /// version of this video (another title or episode, an excluded one).
    NoResults,
    /// Versions exist, none plays on this device.
    NothingPlaysHere,
    /// The player only: its start budget (90 s) ran out with nothing
    /// playing (plans/play-every-version.md 2e).
    NotStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum EmptyAction {
    /// A link to Settings, Addons.
    SettingsAddons,
    /// Open the view again.
    TryAgain,
    /// Pick `key`: the last played source, kept on this device, starts with
    /// no addon.
    PlayFromDevice,
    /// The rows, each startable.
    ChooseVersion,
    None,
}

/// How long `done` waits for the meta, for each cache check (4.2, r4 N4)
/// and for the live counts once the rest is in: the apps play at `done`.
const SIDE_WAIT: Duration = Duration::from_secs(3);
/// How long Versions looks for the title page's TMDB answer: the page asks
/// once it has shown for a second.
const ORIGINAL_WAIT: Duration = Duration::from_secs(30);

struct State {
    conditions: VersionsConditions,
    /// A playback of a pick failed in this view: no later pick teaches
    /// (an offer taken after a failed hand pick included).
    recommendation_failed: bool,
    /// What the rows rank with, for a rank again outside the view's task
    /// (a playback's failure, `PickHandle::failed`). None when denied.
    ranking: Option<Arc<Ranking>>,
    /// The recommendation was picked and has not started or failed yet:
    /// a pick now could still turn out to follow its failure (code review
    /// r1 P3), so it teaches nothing.
    recommendation_starting: bool,
    /// A pick already counted in this view (only the first does).
    learned: bool,
    note: Option<LearnedNote>,
    /// What the ranking reads beyond the device and the user; the meta and
    /// the cache checks fill it in.
    rank: Conditions,
    /// Cache answers by store and lowercase info hash, and those asked:
    /// with two accounts one store's answer never relabels the other's
    /// rows (code review r1 P3).
    cache: HashMap<CacheKey, bool>,
    asked: HashSet<CacheKey>,
    /// Live seeders by lowercase info hash, and the hashes asked: a swarm's
    /// count from its trackers stands over the addon's number.
    live: HashMap<String, u32>,
    scraped: HashSet<String>,
    blocked: Option<BlockedVersions>,
    streams: Vec<ParsedStream>,
    ranked: Ranked,
    recommended: Option<RankedRow>,
    frozen: bool,
    /// The shortcuts' keys (`smaller`, `sharper`), chosen with the pick and
    /// frozen with it.
    smaller: Option<String>,
    sharper: Option<String>,
    /// Installs by transport URL, with their names: two installs may share a
    /// manifest id, and each keeps its own progress.
    pending: Vec<(Url, String)>,
    slow: Vec<(Url, String)>,
    failed: Vec<(Url, String)>,
    done: bool,
    /// The pick's debrid link was warmed (once per view).
    prefetched: bool,
    /// Live seeder asks still on the way.
    counting: usize,
    /// The key of the version that starts from what this device keeps.
    on_device: Option<String>,
    /// The version an offline download keeps here (plans/offline-downloads.md
    /// 4.12): the pick, frozen from the start, before any addon answers.
    download: Option<RankedRow>,
    /// That version's stream from the stream memory, startable with no
    /// addon when every source failed ("Play from this device").
    kept: Option<ParsedStream>,
    /// Why nothing is listed, for `empty`: `blocked`'s reasons, a device
    /// that plays nothing found before free space (its rows stay
    /// startable).
    why: Option<BlockedVersions>,
    /// The stream addons asked, and those that never answered (no
    /// connection, or none in time): a status is an answer.
    addons: usize,
    unreached: usize,
    /// Whether this device keeps the version last played is known
    /// (`kept_start`): until then every source failing says nothing, so
    /// Try again never turns into Play from this device.
    kept_checked: bool,
    /// A kids profile: what keeps it from playing is said in a child's
    /// words.
    kids: bool,
    /// The Internet Archive's streams for this film, their own pass, and
    /// whether it is still asking (plans/plugins-and-marketplace.md 4.4).
    archive: Vec<ParsedStream>,
    archive_ranked: Ranked,
    archive_pending: bool,
    /// The plugins' streams, their own pass, and whether a provider is
    /// still running (7.8).
    plugins: Vec<ParsedStream>,
    plugins_ranked: Ranked,
    plugins_pending: bool,
    /// The kind chip the viewer chose (`setKind`), its own pass over that
    /// kind's streams, and the card of that pass, which follows its source
    /// as the pick does (plans/plugins-and-marketplace.md 4.2).
    kind: Option<SourceKind>,
    kind_ranked: Ranked,
    kind_pick: Option<RankedRow>,
    /// The streaming accounts' stores: with exactly one, a debrid source
    /// that names no service resolves on it, which names the debrid chip.
    stores: Vec<mq_debrid::StoreId>,
}

pub struct VersionsView {
    core: Core,
    /// The playlist formats the device plays, from its caps.
    plays: Vec<mq_server::Format>,
    episode: (Option<u32>, Option<u32>),
    snapshot: SharedObservable<VersionsSnapshot>,
    state: Arc<Mutex<State>>,
    /// A kids profile's checked target; playback ids come from it.
    verified: Option<Verified>,
    /// A kids policy refused the target: the view does no work.
    denied: bool,
    /// What counting a pick reads (3.9).
    learning: Box<Learning>,
    _tasks: JoinSet<()>,
}

/// What a pick needs to count: the screen class it teaches, the device
/// (for HDR), the title, and the profile's preferences before learning.
struct Learning {
    on: bool,
    screen_class: u32,
    caps: DeviceCaps,
    item_id: String,
    prefs: StreamPrefs,
    device_language: Option<String>,
}

/// A row the user picked: its stream and failover chain for playback, the
/// handle playback reports a failure through, and the learning note if
/// this pick crossed one.
pub struct Pick {
    pub candidates: Vec<ParsedStream>,
    pub handle: PickHandle,
    pub note: Option<LearnedNote>,
    /// The audio languages this view ranked with: the profile's, else the
    /// device's language, then English (the bridge fills them in). The
    /// playback orders subtitles with them too.
    pub languages: Vec<String>,
    /// The device's language (`VersionsParams::device_language`).
    pub device_language: Option<String>,
    /// The meta runtime the view ranked with: a torrent's window is sized
    /// with the same rate (plans/streaming-window.md 2.2).
    pub runtime_s: Option<u32>,
}

/// Playback reports through this that the picked row failed; when it was
/// the recommendation, no later pick in the view teaches anything (3.9).
#[derive(Clone)]
pub struct PickHandle {
    state: std::sync::Weak<Mutex<State>>,
    core: Core,
    snapshot: SharedObservable<VersionsSnapshot>,
    recommended: bool,
    /// The device the view ranked for.
    pub caps: DeviceCaps,
}

impl PickHandle {
    /// The sources of version `key` the addons list now, failover chain and
    /// all: a download picked before they answered goes on to them when its
    /// own file fails (plans/offline-downloads.md 4.12.3).
    pub fn online(&self, key: &str) -> Vec<ParsedStream> {
        let Some(state) = self.state.upgrade() else {
            return Vec::new();
        };
        let s = lock(&state);
        holding(&s.ranked.rows, |st| st.key == key)
            .map(|r| std::iter::once(r.stream).chain(r.failover).collect())
            .unwrap_or_default()
    }

    /// The source `key` of the pick failed. No later pick in this view
    /// teaches (3.9), and the view learns what the device recorded of the
    /// failure: its rows rank again, and a frozen pick that failed is let
    /// go (code review 2026-10-20 P2-1). Called outside the playback's lock.
    pub fn failed(&self, key: &str) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        s.recommendation_failed = true;
        s.recommendation_starting = false;
        relearn(&self.core, &mut s, Some(key));
        self.snapshot.set(render(&s));
    }

    /// The device recorded something since (a swarm proven dead once
    /// another source reached a peer): the rows rank again.
    pub fn relearn(&self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        relearn(&self.core, &mut s, None);
        self.snapshot.set(render(&s));
    }

    /// The picked row started playing.
    pub fn ready(&self) {
        if let (true, Some(state)) = (self.recommended, self.state.upgrade()) {
            lock(&state).recommendation_starting = false;
        }
    }

    /// The pick was the recommendation.
    pub fn recommended(&self) -> bool {
        self.recommended
    }

    /// The trackers' live count for `stream`'s swarm said no seeder before
    /// Play (plans/play-every-version.md 2e). False with no count (DHT only,
    /// or none asked) or once the view is gone.
    pub fn none_counted(&self, stream: &ParsedStream) -> bool {
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let s = lock(&state);
        info_hash(stream).is_some_and(|h| s.live.get(&h) == Some(&0))
    }

    /// The version the ranking picks after the sources `tried` (stream
    /// keys): the first listed row that can start and holds none of them,
    /// with its failover chain. A row a folded pick played through is a
    /// tried row (code review 2026-10-20 r3 P3-6). After a version that
    /// did not play here (`unlike`), the row that differs most from it in
    /// what failed among those on the same footing as that first one (all
    /// but the resolution, `same_footing`), the first of them on a tie, so
    /// two moves do not die on the same E-AC-3 or 10-bit HEVC (pick audit
    /// 8.3) and difference alone never lifts a worse row. None once the
    /// view is gone.
    pub fn next_version(&self, tried: &[String], unlike: Option<&FailedOn>) -> Option<NextVersion> {
        let state = self.state.upgrade()?;
        let s = lock(&state);
        let untried = |r: &&RankedRow| {
            !std::iter::once(&r.stream)
                .chain(&r.failover)
                .any(|st| tried.contains(&st.key))
        };
        // Under a kind chip the next version is of that kind, from its own pass.
        let rows = match s.kind {
            Some(_) => &s.kind_ranked.rows,
            None => &s.ranked.rows,
        };
        let mut open = rows.iter().filter(|r| r.key.viable).filter(untried);
        let first = open.next()?;
        let row = match unlike {
            None => first,
            Some(failed) => std::iter::once(first)
                .chain(open.filter(|r| same_footing(&r.key, &first.key)))
                .enumerate()
                .max_by_key(|(i, r)| (failed.apart(&r.stream.release), std::cmp::Reverse(*i)))
                .map_or(first, |(_, r)| r),
        };
        Some(NextVersion {
            key: row.stream.key.clone(),
            label: version_label(&row.stream.release),
            candidates: std::iter::once(&row.stream)
                .chain(&row.failover)
                .cloned()
                .collect(),
        })
    }
}

/// A ready file the user prefers with every field before the start tier at
/// its best: a swarm passes it only through one of those, so no live count
/// can move it from the lead.
fn unbeatable(k: &RankKey) -> bool {
    k.tier == 3
        && k.language
        && k.legit
        && k.year_ok
        && k.original
        && k.sustains
        && k.size_ok
        && k.playability == Playability::Native
}

/// Two rows the ranking holds equal on everything but the resolution and
/// what breaks ties under it: the same language, a real release, the
/// year, the original or a dub, keeping up, pace and, past the speed, how
/// far past it (`covered`), how it plays, the size and the start tier.
fn same_footing(a: &RankKey, b: &RankKey) -> bool {
    let footing = |k: &RankKey| {
        (
            k.language,
            k.legit,
            k.year_ok,
            k.original,
            k.sustains,
            k.pace,
            k.covered,
            k.playability,
            k.size_ok,
            k.tier,
        )
    };
    footing(a) == footing(b)
}

/// What a version failed on when it did not play here, which the next
/// version should not share.
#[derive(Debug, Clone)]
pub enum FailedOn {
    /// The player could not decode it; it does not say whether the video or
    /// the audio, so both count.
    Decode(mq_relparse::ParsedRelease),
    /// Its audio output failed.
    Audio(mq_relparse::ParsedRelease),
}

impl FailedOn {
    /// How far `next` is from the version that failed: 2 a part surely
    /// apart, 1 one not stated, 0 the same, summed over what failed.
    fn apart(&self, next: &mq_relparse::ParsedRelease) -> u8 {
        match self {
            FailedOn::Decode(failed) => video_apart(failed, next) + audio_apart(failed, next),
            FailedOn::Audio(failed) => audio_apart(failed, next),
        }
    }
}

fn video_apart(failed: &mq_relparse::ParsedRelease, next: &mq_relparse::ParsedRelease) -> u8 {
    use mq_relparse::VideoCodec::Unknown;
    match (failed.codec, next.codec) {
        (Unknown, _) | (_, Unknown) => 1,
        (a, b) if a != b => 2,
        // The same codec: a stated bit depth apart is apart.
        _ => match (failed.bit_depth, next.bit_depth) {
            (Some(a), Some(b)) if a != b => 2,
            (Some(_), None) | (None, Some(_)) => 1,
            _ => 0,
        },
    }
}

/// Atmos rides on TrueHD or E-AC-3, so it is not an audio codec of its
/// own; DTS, DTS-HD, DTS-HD MA and DTS:X are one family, each carrying the
/// core the others decode.
fn audio_apart(failed: &mq_relparse::ParsedRelease, next: &mq_relparse::ParsedRelease) -> u8 {
    use mq_relparse::Audio::{Atmos, Dts, DtsHd, DtsHdMa, DtsX};
    let codecs = |r: &mq_relparse::ParsedRelease| -> Vec<mq_relparse::Audio> {
        r.audio
            .iter()
            .filter(|a| **a != Atmos)
            .map(|a| match a {
                DtsHd | DtsHdMa | DtsX => Dts,
                a => *a,
            })
            .collect()
    };
    let (failed, next) = (codecs(failed), codecs(next));
    if failed.is_empty() || next.is_empty() {
        1
    } else if next.iter().any(|a| failed.contains(a)) {
        0
    } else {
        2
    }
}

/// A version a playback can move on to.
pub struct NextVersion {
    pub key: String,
    pub label: String,
    pub candidates: Vec<ParsedStream>,
}

/// "1080p WEB", "4K Remux", "Blu-ray": the row's quality, then its source.
pub(crate) fn version_label(rel: &mq_relparse::ParsedRelease) -> String {
    use mq_relparse::Quality as Q;
    let quality = quality_word(rel.resolution.height(), rel.quality);
    let source = match rel.quality {
        Q::WebDl | Q::WebRip => "WEB",
        Q::BluRay => "Blu-ray",
        Q::Remux => "Remux",
        Q::Hdtv => "HDTV",
        Q::Dvd => "DVD",
        _ => return quality,
    };
    if rel.resolution.height() == 0 {
        return quality;
    }
    format!("{quality} {source}")
}

/// A kids profile's target as the list allows it (plans/m11-kids.md 4.3):
/// the title is on the list as this kind and the video is one of its own.
/// Only [`verify_play`] makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Verified {
    kind: String,
    item_id: String,
    video_id: String,
    /// The allowlist entry's name, then its other names.
    names: Vec<String>,
}

impl Verified {
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn video_id(&self) -> &str {
        &self.video_id
    }

    /// The allowlist entry's name.
    pub fn name(&self) -> &str {
        &self.names[0]
    }

    /// Season and episode from the video id itself (`tt1:2:5`), never from
    /// what an app passed.
    fn episode(&self) -> (Option<u32>, Option<u32>) {
        let rest = self
            .video_id
            .strip_prefix(&self.item_id)
            .unwrap_or_default();
        let numbers: Vec<u32> = rest
            .split(':')
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect();
        match numbers[..] {
            [season, episode] => (Some(season), Some(episode)),
            [episode] => (None, Some(episode)),
            _ => (None, None),
        }
    }
}

/// Checks a kids profile's target against its list: `item_id` is given and
/// allowed as `kind`, and `video_id` belongs to it. `None` refuses.
pub fn verify_play(
    core: &Core,
    kind: &str,
    item_id: Option<&str>,
    video_id: &str,
) -> Option<Verified> {
    let item_id = item_id?;
    if !core.policy().allows(kind, item_id) || !Policy::belongs(item_id, video_id) {
        return None;
    }
    let entry = core
        .library()
        .allowed_list(core.profile())
        .ok()?
        .counted
        .into_iter()
        .find(|r| r.item_id == item_id && r.kind == kind)?;
    Some(Verified {
        kind: kind.to_owned(),
        item_id: item_id.to_owned(),
        video_id: video_id.to_owned(),
        names: std::iter::once(entry.name).chain(entry.names).collect(),
    })
}

pub struct VersionsParams {
    pub kind: String,
    pub video_id: String,
    pub target: Target,
    pub caps: DeviceCaps,
    pub prefs: StreamPrefs,
    pub last_binge_group: Option<String>,
    /// What the app knows of the title; the meta fills the rest (4.3).
    pub facts: TitleFacts,
    /// The device's language as the core's tag ("no" for "nb"), as the
    /// app sends it: a profile with no subtitle languages starts subtitles
    /// in it (plans/onboarding-language-picker.md).
    pub device_language: Option<String>,
}

impl VersionsParams {
    /// From a `versions` subscription's params (plans/core-protocol.md), and
    /// from a download's stored request (plans/offline-downloads.md 4.5).
    pub fn from_json(params: &Value, mut prefs: StreamPrefs) -> Result<Self, String> {
        let num = |key: &str| params.get(key).and_then(Value::as_u64).map(|n| n as u32);
        let device: Option<String> = field(params, "language")?;
        let device = device.map(|code| crate::languages::tag(&code));
        if prefs.languages.is_empty() {
            prefs.languages = device.iter().cloned().chain(["en".to_owned()]).collect();
            prefs.languages.dedup();
        }
        let mut caps = DeviceCaps::basic_1080p();
        if let Some(height) = num("displayHeight") {
            caps.display_height = height;
        }
        if let Some(hdr) = field(params, "hdr")? {
            caps.display_hdr = hdr;
        }
        if let Some(decoders) = field(params, "decoders")? {
            caps.decoders = decoders;
        }
        if let Some(ten_bit) = field(params, "tenBitHevc")? {
            caps.ten_bit_hevc = ten_bit;
        }
        if let Some(ten_bit) = field(params, "tenBitH264")? {
            caps.ten_bit_h264 = ten_bit;
        }
        if let Some(passthrough) = field(params, "passthrough")? {
            caps.passthrough = passthrough;
        }
        if let Some(fallback) = field(params, "hasFallbackEngine")? {
            caps.has_fallback_engine = fallback;
        }
        if let Some(hvc1_only) = field(params, "hvc1Only")? {
            caps.hvc1_only = hvc1_only;
        }
        if let Some(audio) = field(params, "audioDecoders")? {
            caps.audio_decoders = audio;
        }
        if let Some(containers) = field::<Vec<String>>(params, "containers")? {
            caps.containers = containers.iter().map(|c| c.to_ascii_lowercase()).collect();
        }
        Ok(Self {
            kind: param(params, "kind")?,
            video_id: param(params, "videoId")?,
            target: Target {
                title: param(params, "title").unwrap_or_default(),
                year: None,
                season: num("season"),
                episode: num("episode"),
            },
            caps,
            prefs,
            last_binge_group: params
                .get("bingeGroup")
                .and_then(Value::as_str)
                .map(str::to_owned),
            facts: TitleFacts::read(
                field::<String>(params, "runtime")?.as_deref(),
                field::<String>(params, "releaseInfo")?.as_deref(),
                field::<String>(params, "released")?.as_deref(),
                num("episodesInSeason"),
            ),
            device_language: device,
        })
    }
}

fn field<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>, String> {
    params
        .get(key)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad {key}: {e}")))
        .transpose()
}

fn param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing parameter: {key}"))
}

/// The title's runtime and years and the season's size, as the app sends
/// them with `versions` (4.3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TitleFacts {
    pub runtime_s: Option<u32>,
    pub title_year: Option<u16>,
    /// The episode's air date's year.
    pub episode_year: Option<u16>,
    pub episodes_in_season: Option<u32>,
}

impl TitleFacts {
    /// From the strings the apps and Cinemeta use: a runtime of "142 min"
    /// or "2h 22min", a release info of "2002", "2022-", "2005-2013", or the
    /// core's own "Since 2022" and "2005 to 2013", and an air date of
    /// "2013-09-16".
    pub fn read(
        runtime: Option<&str>,
        release_info: Option<&str>,
        released: Option<&str>,
        episodes_in_season: Option<u32>,
    ) -> Self {
        Self {
            runtime_s: runtime.and_then(minutes).map(|m| m * 60),
            title_year: release_info.and_then(year),
            episode_year: released.and_then(year),
            episodes_in_season: episodes_in_season.filter(|n| *n > 0),
        }
    }
}

/// "142 min" is 142, "2h 22min" is 142, "1 h" is 60; a bare number is
/// minutes.
fn minutes(text: &str) -> Option<u32> {
    let mut total = 0;
    let mut found = false;
    let mut rest = text.trim();
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let digits: String = rest[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        rest = &rest[start + digits.len()..];
        let n: u32 = digits.parse().ok()?;
        let hours = rest.trim_start().starts_with(['h', 'H']);
        total += if hours { n * 60 } else { n };
        found = true;
    }
    (found && total > 0).then_some(total)
}

/// The first year in `text`: its first group of four or more digits, so
/// the core's own year line ("Since 2023", "2008 to 2013", core backlog
/// C37) reads as the addon's "2023-" did.
fn year(text: &str) -> Option<u16> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|group| group.len() >= 4)?
        .get(..4)?
        .parse()
        .ok()
        .filter(|y| (1870..=2200).contains(y))
}

impl VersionsView {
    /// `open_checked` without an item id: a kids policy refuses it.
    pub fn open(core: &Core, params: VersionsParams) -> Self {
        Self::open_checked(core, params, None)
    }

    /// Opens Versions for a video of `item_id`. A kids profile needs the
    /// item id: the target is verified against the list, and the ranking
    /// takes the allowlist entry's names and the video's own season and
    /// episode, not the app's. A refused target opens denied: no work, an
    /// empty snapshot, and `denied()` says so.
    pub fn open_checked(core: &Core, params: VersionsParams, item_id: Option<&str>) -> Self {
        Self::open_with(core, params, item_id, false)
    }

    /// `open_checked` ranked to download (plans/offline-downloads.md 4.5):
    /// no speed rule, the size limit on every row, only what fits whole,
    /// the smaller file first among versions as good, and no row for a
    /// download already kept.
    pub fn open_to_download(core: &Core, params: VersionsParams, item_id: Option<&str>) -> Self {
        Self::open_with(core, params, item_id, true)
    }

    fn open_with(
        core: &Core,
        mut params: VersionsParams,
        item_id: Option<&str>,
        download: bool,
    ) -> Self {
        let verified = if core.policy().kids() {
            let Some(verified) = verify_play(core, &params.kind, item_id, &params.video_id) else {
                return Self::denied_view(core, &params);
            };
            params.target.title = verified.name().to_owned();
            (params.target.season, params.target.episode) = verified.episode();
            Some(verified)
        } else {
            None
        };
        let screen = screen_class(params.caps.display_height);
        let learning = Box::new(Learning {
            on: params.prefs.learn && verified.is_none(),
            screen_class: screen,
            caps: params.caps.clone(),
            item_id: item_id
                .unwrap_or_else(|| params.video_id.split(':').next().unwrap_or_default())
                .to_owned(),
            prefs: params.prefs.clone(),
            device_language: params.device_language.clone(),
        });
        params.prefs = core.learned_prefs(params.prefs, screen);
        if params.last_binge_group.is_none() {
            let item_id =
                item_id.unwrap_or_else(|| params.video_id.split(':').next().unwrap_or_default());
            params.last_binge_group = core
                .library()
                .last_binge_group(core.profile(), item_id)
                .ok()
                .flatten();
        }
        let episode = (params.target.season, params.target.episode);
        let plays = plays(&params.caps);
        // Subscribe before the first read so no addon or account change
        // slips in between.
        let changes = core.library().subscribe();
        let accounts = core.account_changes();
        let path = ResourcePath {
            resource: "stream".into(),
            r#type: params.kind.clone(),
            id: params.video_id.clone(),
            extra: vec![],
        };
        let eligible = eligible(core, &path);
        let facts = &params.facts;
        #[cfg(feature = "torrent")]
        let (reclaimable_bytes, committed_bytes, held) = core.room_counts();
        #[cfg(not(feature = "torrent"))]
        let (reclaimable_bytes, committed_bytes, held) = (None, 0, Default::default());
        let conditions = Conditions {
            runtime_s: facts.runtime_s,
            title_year: facts.title_year,
            episode_year: facts.episode_year,
            episodes_in_season: facts.episodes_in_season,
            http: core.speed(crate::speed::Kind::Http),
            p2p: core.speed(crate::speed::Kind::P2p),
            p2p_thin: core.speed(crate::speed::Kind::P2pThin),
            free_bytes: if download {
                core.downloads().free(core)
            } else {
                core.free_bytes()
            },
            engine: core.has_engine(),
            debrid: core.debrid().is_some(),
            dead_swarms: crate::speed::dead_swarms(core.library(), mq_library::hlc::now_ms()),
            limit_chosen: core.size_limit_chosen(),
            no_picture: crate::speed::no_picture(core.library()),
            did_not_play: crate::speed::did_not_play(core.library(), mq_library::hlc::now_ms()),
            wrong_file: crate::speed::wrong_file(core.library(), mq_library::hlc::now_ms()),
            containers: crate::speed::containers(core.library()),
            reclaimable_bytes,
            committed_bytes,
            metered: core.network() == crate::speed::Network::Cellular,
            held,
            download,
            // From TMDB once it answers, for a user who chose no audio
            // languages (`original`).
            original_language: None,
        };
        let item_id = item_id.map_or_else(
            || {
                params
                    .video_id
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            },
            str::to_owned,
        );
        let state = Arc::new(Mutex::new(State {
            conditions: view_conditions(core, &params, &conditions),
            recommendation_failed: false,
            recommendation_starting: false,
            learned: false,
            note: None,
            rank: conditions,
            cache: HashMap::new(),
            asked: HashSet::new(),
            live: HashMap::new(),
            scraped: HashSet::new(),
            blocked: None,
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            smaller: None,
            sharper: None,
            pending: eligible
                .iter()
                .map(|a| (a.transport_url.clone(), a.manifest.name.clone()))
                .collect(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: eligible.is_empty(),
            prefetched: false,
            counting: 0,
            ranking: None,
            on_device: None,
            download: None,
            kept: None,
            why: None,
            unreached: 0,
            kept_checked: !cfg!(feature = "torrent"),
            addons: eligible.len(),
            kids: verified.is_some(),
            archive: Vec::new(),
            archive_ranked: Ranked::default(),
            // Movies only (5.3), for a profile that may contact archive.org.
            archive_pending: params.kind == "movie" && crate::archive::wanted(core),
            // Titles with an IMDb id, for a profile with a repository on (7.8).
            plugins_pending: item_id.starts_with("tt")
                && verified.is_none()
                && core.plugins_wanted(),
            plugins: Vec::new(),
            plugins_ranked: Ranked::default(),
            kind: None,
            kind_ranked: Ranked::default(),
            kind_pick: None,
            stores: core.debrid().map(|r| r.stores()).unwrap_or_default(),
        }));
        let snapshot = SharedObservable::new(render(&lock(&state)));
        let mut tasks = JoinSet::new();
        let ranking = Arc::new(Ranking {
            kids: verified.as_ref().map(|v| v.names.clone()),
            remembered: resumes(core, &item_id, &params.video_id),
            params,
            item_id,
        });
        lock(&state).ranking = Some(ranking.clone());
        if !download {
            let mut s = lock(&state);
            if let Some(row) = downloaded(core, &s, &ranking) {
                s.download = Some(row.clone());
                s.recommended = Some(row);
                s.frozen = true;
                snapshot.set(render(&s));
            }
        }
        tasks.spawn(follow_network(
            core.clone(),
            core.library().subscribe(),
            state.clone(),
            snapshot.clone(),
        ));
        #[cfg(feature = "torrent")]
        tasks.spawn(kept_start(
            core.clone(),
            ranking.clone(),
            state.clone(),
            snapshot.clone(),
        ));
        if lock(&state).archive_pending {
            tasks.spawn(archive_pass(
                core.clone(),
                ranking.clone(),
                state.clone(),
                snapshot.clone(),
            ));
        }
        if lock(&state).plugins_pending {
            tasks.spawn(plugin_pass(
                core.clone(),
                ranking.clone(),
                state.clone(),
                snapshot.clone(),
            ));
        }
        tasks.spawn(supervise(
            core.clone(),
            (changes, accounts),
            eligible,
            path,
            ranking,
            state.clone(),
            snapshot.clone(),
        ));
        Self {
            core: core.clone(),
            plays,
            episode,
            snapshot,
            state,
            verified,
            denied: false,
            learning,
            _tasks: tasks,
        }
    }

    fn denied_view(core: &Core, params: &VersionsParams) -> Self {
        let state = State {
            conditions: view_conditions(core, params, &Conditions::default()),
            recommendation_failed: false,
            recommendation_starting: false,
            learned: false,
            note: None,
            rank: Conditions::default(),
            cache: HashMap::new(),
            asked: HashSet::new(),
            live: HashMap::new(),
            scraped: HashSet::new(),
            blocked: None,
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            smaller: None,
            sharper: None,
            pending: Vec::new(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: false,
            prefetched: true,
            counting: 0,
            ranking: None,
            on_device: None,
            download: None,
            kept: None,
            why: None,
            unreached: 0,
            kept_checked: true,
            addons: 0,
            kids: true,
            archive: Vec::new(),
            archive_ranked: Ranked::default(),
            archive_pending: false,
            plugins: Vec::new(),
            plugins_ranked: Ranked::default(),
            plugins_pending: false,
            kind: None,
            kind_ranked: Ranked::default(),
            kind_pick: None,
            stores: Vec::new(),
        };
        Self {
            core: core.clone(),
            plays: plays(&params.caps),
            episode: (None, None),
            snapshot: SharedObservable::new(render(&state)),
            state: Arc::new(Mutex::new(state)),
            verified: None,
            denied: true,
            learning: Box::new(Learning {
                on: false,
                screen_class: screen_class(params.caps.display_height),
                caps: params.caps.clone(),
                item_id: String::new(),
                prefs: params.prefs.clone(),
                device_language: params.device_language.clone(),
            }),
            _tasks: JoinSet::new(),
        }
    }

    /// A kids policy refused the target at open.
    pub fn denied(&self) -> bool {
        self.denied
    }

    /// A kids profile's verified target, for its playback.
    pub fn verified(&self) -> Option<&Verified> {
        self.verified.as_ref()
    }

    pub fn snapshot(&self) -> VersionsSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        VersionsSnapshot,
        Pin<Box<dyn Stream<Item = Vec<VersionsSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// Keeps the current recommendation from now on (the user moved focus).
    /// Before there is one, it does nothing: the first answer still picks.
    pub fn freeze(&self) {
        let mut state = lock(&self.state);
        if state.recommended.is_none() {
            return;
        }
        state.frozen = true;
        maybe_prefetch(&self.core, &mut state, self.episode);
        self.snapshot.set(render(&state));
    }

    /// Lists one kind of version only (`setKind`, plans/plugins-and-marketplace.md
    /// 4.2), or every one again (`None`): the pick freezes, and that kind's
    /// streams rank in a pass of their own, which gives the card, the rows
    /// and the sections. A kind no row holds, the Archive's and the plugins'
    /// included, lists everything.
    pub fn set_kind(&self, kind: Option<SourceKind>) {
        let mut state = lock(&self.state);
        if state.recommended.is_some() {
            state.frozen = true;
            maybe_prefetch(&self.core, &mut state, self.episode);
        }
        // The chip already on keeps its card.
        if state.kind != kind {
            state.kind = kind;
            state.kind_pick = None;
        }
        match state.ranking.clone() {
            Some(ranking) => rank_kind(&mut state, &ranking),
            None => state.kind = None,
        }
        self.snapshot.set(render(&state));
    }

    /// Live seeder counts are still on the way: the rows may rank again
    /// when they land.
    pub fn counting(&self) -> bool {
        lock(&self.state).counting > 0
    }

    /// The playlist formats the device plays, for `PlaybackView::open_for`.
    pub fn plays(&self) -> Vec<mq_server::Format> {
        self.plays.clone()
    }

    /// The stream for row `key` followed by its failover chain, for playback.
    pub fn candidates(&self, key: &str) -> Vec<ParsedStream> {
        let s = lock(&self.state);
        // An Archive or plugin row plays from its own pass alone (4.4).
        let own = if key.starts_with(crate::plugins::KEY_PREFIX) {
            Some(&s.plugins_ranked.rows)
        } else if key.starts_with(crate::archive::KEY_PREFIX) {
            Some(&s.archive_ranked.rows)
        } else {
            None
        };
        if let Some(rows) = own {
            return holding(rows, |st| st.key == key)
                .map(|r| {
                    std::iter::once(&r.stream)
                        .chain(&r.failover)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
        }
        // The kept file first, then the same version from the addons, so a
        // file that fails to play falls over to the network (4.12.3).
        if let Some(download) = s.download.as_ref().filter(|d| d.stream.key == key) {
            let online = holding(&s.ranked.rows, |st| st.key == key);
            return std::iter::once(download.stream.clone())
                .chain(
                    online
                        .into_iter()
                        .flat_map(|r| std::iter::once(r.stream).chain(r.failover)),
                )
                .collect();
        }
        // Under a kind chip a row plays its sources of that kind, from that
        // kind's pass (plans/plugins-and-marketplace.md 4.2).
        let of_kind = s.kind.is_some().then(|| {
            (s.kind_pick.clone().filter(|r| r.stream.key == key))
                .or_else(|| holding(&s.kind_ranked.rows, |st| st.key == key))
        });
        // The frozen pick plays its own source first, wherever a later
        // answer folded it (code review 2026-10-20 P2-3).
        let row = of_kind.flatten().or_else(|| {
            (s.recommended.clone().filter(|r| r.stream.key == key))
                .or_else(|| holding(&s.ranked.rows, |st| st.key == key))
        });
        row.map(|r| {
            std::iter::once(&r.stream)
                .chain(&r.failover)
                .cloned()
                .collect()
        })
        // The source kept on this device when no addon listed it ("Play from
        // this device"), or a version hidden as unplayable here: each stays
        // startable ("Choose a version").
        .unwrap_or_else(|| {
            let unplayable = s
                .ranked
                .hidden
                .iter()
                .filter(|h| h.reason == HideReason::Unplayable);
            s.kept
                .iter()
                .chain(unplayable.map(|h| &h.stream))
                .find(|st| st.key == key)
                .cloned()
                .into_iter()
                .collect()
        })
    }

    /// The user picked row `key` to play. A pick counts toward learning
    /// (3.9) when the view is frozen, it is not the recommendation nor the
    /// stream last played for the title, no playback of the recommendation
    /// failed here, learning is on, the profile is not a kids one, and no
    /// pick counted in this view yet.
    pub fn pick(&self, key: &str) -> Pick {
        let candidates = self.candidates(key);
        let mut s = lock(&self.state);
        let recommended = s.recommended.as_ref().is_some_and(|r| r.stream.key == key);
        let handle = PickHandle {
            state: Arc::downgrade(&self.state),
            core: self.core.clone(),
            snapshot: self.snapshot.clone(),
            recommended,
            caps: self.learning.caps.clone(),
        };
        if recommended {
            s.recommendation_starting = true;
        }
        let l = &self.learning;
        // A pick under a kind chip was made among one kind only: it teaches
        // nothing.
        let counts = l.on
            && s.frozen
            && s.kind.is_none()
            && !recommended
            && !s.recommendation_failed
            && !s.recommendation_starting
            && !s.learned;
        let chose = s.ranked.rows.iter().find(|r| r.stream.key == key);
        let taught = match (counts, chose, &s.recommended) {
            (true, Some(chose), Some(over)) if !self.replays(key) => {
                crate::learn::choice(chose, over, &l.caps)
            }
            _ => None,
        };
        let mut note = None;
        if let Some((choice, delta)) = taught {
            s.learned = true;
            note = self
                .core
                .learn_pick(l.screen_class, choice, delta, l.prefs.clone());
            if note.is_some() {
                s.note = note.clone();
                self.snapshot.set(render(&s));
            }
        }
        Pick {
            candidates,
            handle,
            note,
            languages: self.learning.prefs.languages.clone(),
            device_language: self.learning.device_language.clone(),
            runtime_s: s.rank.runtime_s,
        }
    }

    /// `key` is the stream last played for the title: a replay teaches
    /// nothing.
    fn replays(&self, key: &str) -> bool {
        let remembered = self
            .core
            .library()
            .stream_memory(self.core.profile(), &self.learning.item_id)
            .ok()
            .flatten();
        remembered
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .is_some_and(|m| m["key"] == key)
    }

    /// The row the addons listed first (addon order, then their own): the
    /// live race's other arm (plans/m13-stream-choice.md 6.4).
    pub fn first_listed(&self) -> Option<String> {
        let s = lock(&self.state);
        s.ranked
            .rows
            .iter()
            .min_by_key(|r| (r.stream.addon_rank, r.stream.index))
            .map(|r| r.stream.key.clone())
    }

    pub fn recommended(&self) -> Option<RankedRow> {
        lock(&self.state).recommended.clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn eligible(core: &Core, path: &ResourcePath) -> Vec<InstalledAddon> {
    let addons = core.installed_addons().unwrap_or_default();
    addons.into_iter().filter(|a| a.supports(path)).collect()
}

/// The version last played for this video says "On this device" when this
/// device holds its start (`playback::kept_start`).
#[cfg(feature = "torrent")]
async fn kept_start(
    core: Core,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let video = &ranking.params.video_id;
    let key = crate::views::playback::kept_start(&core, &ranking.item_id, video).await;
    let kept = (key.as_deref())
        .and_then(|k| crate::views::playback::kept_stream(&core, &ranking.item_id, k));
    let mut s = lock(&state);
    s.on_device = key;
    s.kept = kept;
    s.kept_checked = true;
    snapshot.set(render(&s));
}

/// Asks the installed stream addons, and asks again from the start whenever
/// they change, or a streaming account is added, replaced or removed: a
/// source installed, or an account connected, while a title is open shows
/// up without reopening it, on every screen that holds the view.
async fn supervise(
    core: Core,
    (mut changes, mut accounts): (
        tokio::sync::broadcast::Receiver<Vec<Table>>,
        crate::debrid::AccountChanges,
    ),
    mut addons: Vec<InstalledAddon>,
    path: ResourcePath,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    loop {
        let current = urls(&addons);
        let run = collect(
            core.clone(),
            addons.clone(),
            path.clone(),
            ranking.clone(),
            state.clone(),
            snapshot.clone(),
        );
        tokio::pin!(run);
        let mut running = true;
        // Dropping an unfinished run stops its writes at once.
        let moved = loop {
            tokio::select! {
                () = &mut run, if running => running = false,
                next = changed_set(&mut changes, &core, &path, &current) => {
                    let Some(next) = next else { return };
                    addons = next;
                    break false;
                }
                moved = accounts.changed() => {
                    if moved.is_err() {
                        return;
                    }
                    break true;
                }
            }
        };
        let mut s = lock(&state);
        if moved {
            accounts_moved(&core, &mut s);
        }
        restart(&mut s, &addons);
        snapshot.set(render(&s));
    }
}

/// The Internet Archive's rows for this film (plans/plugins-and-marketplace.md
/// 5.3), ranked in a pass of their own: the title the app sent (the meta's
/// name) and its year. An addon change leaves them (`restart`).
async fn archive_pass(
    core: Core,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let p = &ranking.params;
    let year = p.target.year.or(p.facts.title_year);
    let found = crate::archive::streams(&core, &[p.target.title.as_str()], year).await;
    let mut s = lock(&state);
    s.archive = found;
    s.archive_pending = false;
    rank_archive(&mut s, &ranking);
    snapshot.set(render(&s));
}

/// The Archive pass: its streams ranked alone, with the view's input.
fn rank_archive(s: &mut State, ranking: &Ranking) {
    let params = &ranking.params;
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &s.rank,
    };
    s.archive_ranked = rank(s.archive.clone(), &input);
}

/// The Nuvio providers' rows for this video (plans/plugins-and-marketplace.md
/// 7.8): every provider switched on runs in the device's pool, its rows
/// ranked in a pass of their own as each one answers. `done` and the pick
/// never wait for it; an addon change leaves it (`restart`).
async fn plugin_pass(
    core: Core,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let p = &ranking.params;
    let ask = crate::plugins::Ask {
        kind: p.kind.clone(),
        imdb: ranking.item_id.clone(),
        season: p.target.season,
        episode: p.target.episode,
        title: p.target.title.clone(),
        year: p.target.year.or(p.facts.title_year),
        video_id: p.video_id.clone(),
    };
    let providers = crate::plugins::providers_for(&core, &ask).await;
    let tmdb = if providers.is_empty() {
        None
    } else {
        core.tmdb_find(&ask.imdb, Some(ask.kind.as_str()))
            .await
            .map(|f| f.id)
    };
    let mut calls: futures_util::stream::FuturesUnordered<_> = (providers.iter())
        .map(|(repo, provider)| crate::plugins::streams(&core, repo, provider, &ask, tmdb))
        .collect();
    while let Some(found) = calls.next().await {
        if found.is_empty() {
            continue;
        }
        let mut s = lock(&state);
        s.plugins.extend(found);
        rank_plugins(&mut s, &ranking);
        snapshot.set(render(&s));
    }
    let mut s = lock(&state);
    s.plugins_pending = false;
    snapshot.set(render(&s));
}

/// The plugin pass: its streams ranked alone, with the view's input.
fn rank_plugins(s: &mut State, ranking: &Ranking) {
    let params = &ranking.params;
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &s.rank,
    };
    s.plugins_ranked = rank(s.plugins.clone(), &input);
}

/// A streaming account came, went or was replaced: the rows rank by the
/// accounts there are now (an account opens the rows `needsAccount` held)
/// and the old accounts' cache answers go. A pick the user made stays, as
/// through an addon change, while its source is still installed
/// (`restart`).
fn accounts_moved(core: &Core, s: &mut State) {
    let account = core.debrid().is_some();
    s.stores = core.debrid().map(|r| r.stores()).unwrap_or_default();
    s.rank.debrid = account;
    s.conditions.account = account;
    s.cache.clear();
    s.asked.clear();
}

/// The device moved between Wi-Fi and mobile data (`networkChanged`): a
/// window keeps its minutes ahead on one and only the device preset on the
/// other, so the rows rank again for the network a playback opens on
/// (plans/streaming-window.md 3.3, review r1 B P2-1).
async fn follow_network(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        if let Err(RecvError::Closed) = changes.recv().await {
            return;
        }
        let network = core.network();
        let mut s = lock(&state);
        if s.conditions.network == network {
            continue;
        }
        s.conditions.network = network;
        s.rank.metered = network == Network::Cellular;
        if let Some(ranking) = s.ranking.clone() {
            rerank(&mut s, &ranking);
        }
        snapshot.set(render(&s));
    }
}

fn urls(addons: &[InstalledAddon]) -> Vec<Url> {
    addons.iter().map(|a| a.transport_url.clone()).collect()
}

/// Waits until the stream addons themselves change (which ones, or their
/// order). A write that leaves them as they were, such as a sync or another
/// addon's switch, keeps the rows the user is moving through. `None` once
/// the library closes.
async fn changed_set(
    changes: &mut tokio::sync::broadcast::Receiver<Vec<Table>>,
    core: &Core,
    path: &ResourcePath,
    current: &[Url],
) -> Option<Vec<InstalledAddon>> {
    loop {
        if !addons_changed(changes).await {
            return None;
        }
        let next = eligible(core, path);
        if urls(&next) != current {
            return Some(next);
        }
    }
}

/// A fresh run over `addons`. A frozen pick stays while its addon is still
/// installed; otherwise the next answers pick again.
fn restart(s: &mut State, addons: &[InstalledAddon]) {
    let from_installed = s
        .recommended
        .as_ref()
        .and_then(|r| r.stream.addon_url.as_ref())
        .is_some_and(|url| addons.iter().any(|a| a.transport_url == *url));
    if !(s.frozen && from_installed) {
        s.frozen = false;
        s.recommended = None;
    }
    s.streams.clear();
    s.ranked = Ranked::default();
    // The kind pass ranked what is gone: every kind is listed again.
    s.kind = None;
    s.kind_ranked = Ranked::default();
    s.kind_pick = None;
    s.pending = addons
        .iter()
        .map(|a| (a.transport_url.clone(), a.manifest.name.clone()))
        .collect();
    s.slow.clear();
    s.failed.clear();
    s.blocked = None;
    s.why = None;
    s.unreached = 0;
    s.addons = addons.len();
    s.done = addons.is_empty();
}

async fn collect(
    core: Core,
    addons: Vec<InstalledAddon>,
    path: ResourcePath,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let params = &ranking.params;
    // Name and rank by transport URL: two installs may share a manifest id.
    let names: std::collections::HashMap<Url, (String, u16)> = addons
        .iter()
        .enumerate()
        .map(|(i, a)| (a.transport_url.clone(), (a.manifest.name.clone(), i as u16)))
        .collect();
    let name_of = |url: &Url, id: &str| {
        let name = names
            .get(url)
            .map_or_else(|| id.to_owned(), |(n, _)| n.clone());
        (url.clone(), name)
    };
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    let mut next_index = 0u32;
    // The meta and the cache checks: `done` waits for them (or 3 s each).
    let mut side: JoinSet<Side> = JoinSet::new();
    // Live seeders: they rank the rows again when they come, until the
    // view is frozen. `done` waits for them once the rest is in, at most
    // `SIDE_WAIT`: the apps play the pick at `done`, which came about a
    // second before the counts on every pick of the audit
    // (plans/research/pick-audit.md).
    let mut live: JoinSet<Side> = JoinSet::new();
    let mut hold: Option<tokio::time::Instant> = None;
    let mut held_out = false;
    if needs_meta(params, &lock(&state).rank) {
        side.spawn(meta_facts(
            core.clone(),
            params.kind.clone(),
            ranking.item_id.clone(),
            params.video_id.clone(),
            params.target.season,
        ));
    }
    // The original language ranks above a dub only for a user who chose no
    // audio languages: a choice wins. `done` never waits for it: a kept
    // answer is read now, and one that comes later ranks the rows again.
    let mut late: JoinSet<Side> = JoinSet::new();
    if core.prefs().streams.languages.is_empty() && core.tmdb_ready() {
        match core.original_language_held(&ranking.item_id, &params.kind) {
            Some(original) => lock(&state).rank.original_language = original,
            None => {
                late.spawn(original(
                    core.clone(),
                    params.kind.clone(),
                    ranking.item_id.clone(),
                ));
            }
        }
    }
    let mut answered = addons.is_empty();
    loop {
        let event = tokio::select! {
            event = fanout.next(), if !answered => match event {
                Some(event) => event,
                None => {
                    answered = true;
                    AddonEvent::Done
                }
            },
            Some(joined) = side.join_next() => {
                let mut s = lock(&state);
                if let Ok(found) = joined {
                    found.apply(&mut s);
                }
                let counting = awaited(answered && side.is_empty(), &live, &mut hold, held_out);
                settle(&core, &mut s, &ranking, answered && side.is_empty(), counting);
                snapshot.set(render(&s));
                continue;
            }
            Some(Ok(found)) = late.join_next() => {
                let mut s = lock(&state);
                found.apply(&mut s);
                let counting = awaited(answered && side.is_empty(), &live, &mut hold, held_out);
                settle(&core, &mut s, &ranking, answered && side.is_empty(), counting);
                snapshot.set(render(&s));
                continue;
            }
            Some(joined) = live.join_next() => {
                let mut s = lock(&state);
                s.counting = s.counting.saturating_sub(1);
                if let Ok(found) = joined {
                    found.apply(&mut s);
                }
                let counting = awaited(answered && side.is_empty(), &live, &mut hold, held_out);
                settle(&core, &mut s, &ranking, answered && side.is_empty(), counting);
                snapshot.set(render(&s));
                continue;
            }
            _ = tokio::time::sleep_until(hold.unwrap_or_else(tokio::time::Instant::now)),
                if hold.is_some() && !held_out && !live.is_empty() => {
                held_out = true;
                let mut s = lock(&state);
                settle(&core, &mut s, &ranking, true, false);
                snapshot.set(render(&s));
                continue;
            }
            else => break,
        };
        // Parsing is CPU work: keep it off the async threads that feed UIs.
        let parsed = match &event {
            AddonEvent::Arrived {
                addon_id,
                transport_url,
                body,
                ..
            } => {
                let (name, rank_index) = names
                    .get(transport_url)
                    .cloned()
                    .unwrap_or((addon_id.clone(), u16::MAX));
                let (addon_id, body, start) = (addon_id.clone(), body.clone(), next_index);
                let addon_url = transport_url.clone();
                let parsed = tokio::task::spawn_blocking(move || {
                    decode_streams(&body)
                        .map(|r| {
                            r.streams
                                .iter()
                                .enumerate()
                                .map(|(i, w)| {
                                    let mut stream = normalize(
                                        w,
                                        &addon_id,
                                        &name,
                                        rank_index,
                                        start + i as u32,
                                    );
                                    stream.addon_url = Some(addon_url.clone());
                                    stream
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                next_index += parsed.len() as u32;
                parsed
            }
            _ => Vec::new(),
        };
        let mut s = lock(&state);
        match event {
            AddonEvent::Arrived { transport_url, .. } => {
                for (store, hashes) in cache_asks(&core, &parsed, &mut s.asked) {
                    side.spawn(cache_check(core.clone(), store, hashes));
                }
                // Asked as each addon answers, alongside the others still
                // on the way.
                let swarms = live_asks(&core, &parsed, &mut s.scraped);
                if !swarms.is_empty() {
                    live.spawn(live_check(core.clone(), swarms));
                    s.counting += 1;
                }
                s.streams.extend(parsed);
                s.pending.retain(|(url, _)| *url != transport_url);
                s.slow.retain(|(url, _)| *url != transport_url);
            }
            AddonEvent::Slow {
                addon_ids,
                transport_urls,
            } => {
                s.slow = transport_urls
                    .iter()
                    .zip(&addon_ids)
                    .map(|(url, id)| name_of(url, id))
                    .collect();
            }
            AddonEvent::Failed {
                addon_id,
                transport_url,
                error,
            } => {
                s.pending.retain(|(url, _)| *url != transport_url);
                s.slow.retain(|(url, _)| *url != transport_url);
                s.failed.push(name_of(&transport_url, &addon_id));
                s.unreached += usize::from(crate::core::unanswered(&error));
            }
            AddonEvent::Done => answered = true,
        }
        let counting = awaited(answered && side.is_empty(), &live, &mut hold, held_out);
        settle(
            &core,
            &mut s,
            &ranking,
            answered && side.is_empty(),
            counting,
        );
        snapshot.set(render(&s));
    }
}

/// Live counts `done` still waits for: some are on the way and
/// `SIDE_WAIT` has not passed since every addon and side check was in
/// (`rest`); the wait starts here.
fn awaited(
    rest: bool,
    live: &JoinSet<Side>,
    hold: &mut Option<tokio::time::Instant>,
    held_out: bool,
) -> bool {
    if rest && hold.is_none() {
        *hold = Some(tokio::time::Instant::now() + SIDE_WAIT);
    }
    !live.is_empty() && !held_out
}

/// Ranks again with what is known now; once `done`, warms the pick's
/// link and says why nothing is listed when nothing is.
/// `done` once every addon and side check is in (`rest`) and no live count
/// is `counting`, or the lead cannot be overtaken by them.
fn settle(core: &Core, s: &mut State, ranking: &Ranking, rest: bool, counting: bool) {
    let stores = core.debrid().map(|r| r.stores()).unwrap_or_default();
    apply_cache(&mut s.streams, &s.cache, &stores);
    apply_live(&mut s.streams, &s.live);
    rerank(s, ranking);
    let settled = s.recommended.as_ref().is_some_and(|r| unbeatable(&r.key));
    s.done = rest && (!counting || settled);
    let target = &ranking.params.target;
    if s.done {
        maybe_prefetch(core, s, (target.season, target.episode));
    }
    reblock(s, ranking);
}

/// A cache answer's store (`None`: the default) and lowercase info hash.
type CacheKey = (Option<mq_debrid::StoreId>, String);

/// What a side task found.
enum Side {
    Meta(TitleFacts),
    /// The title's original language, if TMDB knows it.
    Original(Option<String>),
    Cache(Vec<(CacheKey, bool)>),
    Live(Vec<(String, u32)>),
}

impl Side {
    fn apply(self, s: &mut State) {
        match self {
            Side::Meta(f) => {
                let c = &mut s.rank;
                c.runtime_s = c.runtime_s.or(f.runtime_s);
                c.title_year = c.title_year.or(f.title_year);
                c.episode_year = c.episode_year.or(f.episode_year);
                c.episodes_in_season = c.episodes_in_season.or(f.episodes_in_season);
                s.conditions.runtime_min = c.runtime_s.map(|r| r / 60);
            }
            Side::Original(o) => s.rank.original_language = o,
            Side::Cache(found) => s.cache.extend(found),
            Side::Live(found) => s.live.extend(found),
        }
    }
}

/// The title's original language once the title page's TMDB answer is kept.
/// Versions asks TMDB nothing itself: the page opens it in a hidden tab too,
/// which asks nothing until it shows.
// ponytail: looks every 250 ms for `ORIGINAL_WAIT`; a signal from the lookup
// if a page shown later than that must still rank the original first.
async fn original(core: Core, kind: String, item_id: String) -> Side {
    let until = tokio::time::Instant::now() + ORIGINAL_WAIT;
    loop {
        if let Some(held) = core.original_language_held(&item_id, &kind) {
            return Side::Original(held);
        }
        if tokio::time::Instant::now() >= until {
            return Side::Original(None);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The app sent no runtime or year, or for an episode no air date or
/// season size: the meta has them.
fn needs_meta(params: &VersionsParams, c: &Conditions) -> bool {
    let episode = params.target.episode.is_some();
    c.runtime_s.is_none()
        || c.title_year.is_none()
        || (episode && (c.episode_year.is_none() || c.episodes_in_season.is_none()))
}

/// The title's runtime, year, the episode's air date and the season's
/// episode count from the first meta addon that answers, within 3 s.
async fn meta_facts(
    core: Core,
    kind: String,
    item_id: String,
    video_id: String,
    season: Option<u32>,
) -> Side {
    let Ok(Ok((meta, _))) = tokio::time::timeout(SIDE_WAIT, core.fetch_meta(&kind, &item_id)).await
    else {
        return Side::Meta(TitleFacts::default());
    };
    let p = &meta.preview;
    let video = meta.videos.iter().find(|v| v.id == video_id);
    let season = video
        .and_then(|v| v.series_info.as_ref())
        .map(|i| i.season)
        .or(season);
    let episodes = season.map(|n| {
        meta.videos
            .iter()
            .filter(|v| v.series_info.as_ref().is_some_and(|i| i.season == n))
            .count() as u32
    });
    let released = video
        .and_then(|v| v.released)
        .map(|d| d.date_naive().to_string());
    Side::Meta(TitleFacts::read(
        p.runtime.as_deref(),
        p.release_info.as_deref(),
        released.as_deref(),
        episodes,
    ))
}

/// A stream's info hash, lowercase, when a streaming service could have it.
fn info_hash(s: &ParsedStream) -> Option<String> {
    match &s.source {
        mq_protocol::Source::Torrent { info_hash, .. } => Some(info_hash.to_ascii_lowercase()),
        mq_protocol::Source::ClientResolve(r) => {
            r.info_hash.as_ref().map(|h| h.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The hashes of `streams` no one answered or asked about yet, by the
/// store each resolves on (the one its service names when that account is
/// here, else the default): one check per store and arrival.
fn cache_asks(
    core: &Core,
    streams: &[ParsedStream],
    asked: &mut HashSet<CacheKey>,
) -> Vec<(Option<mq_debrid::StoreId>, Vec<String>)> {
    let Some(resolver) = core.debrid() else {
        return Vec::new();
    };
    let stores = resolver.stores();
    let mut asks: Vec<(Option<mq_debrid::StoreId>, Vec<String>)> = Vec::new();
    for s in streams.iter().filter(|s| s.cached.is_none()) {
        let Some(hash) = info_hash(s) else {
            continue;
        };
        let store = store_of(s, &stores);
        if !asked.insert((store, hash.clone())) {
            continue;
        }
        match asks.iter_mut().find(|(st, _)| *st == store) {
            Some((_, hashes)) => hashes.push(hash),
            None => asks.push((store, vec![hash])),
        }
    }
    asks
}

/// How long one scrape may take, the engine's first start included; `done`
/// waits for the counts at most `SIDE_WAIT` of it.
const LIVE_WAIT: Duration = Duration::from_secs(10);
/// The pause before asking again when no tracker answered at all.
const LIVE_RETRY: Duration = Duration::from_secs(1);

/// Swarms asked at most per answer: one UDP scrape packet.
// ponytail: an addon's first 70 swarms, in its order; rank first and ask
// the top ones if an addon lists more and the live ones sit past 70.
const LIVE_MAX: usize = 70;

/// The swarms of `streams` the engine would fetch (no service has them
/// ready) and no one asked about yet, with their trackers.
fn live_asks(
    core: &Core,
    streams: &[ParsedStream],
    scraped: &mut HashSet<String>,
) -> Vec<(String, Vec<String>)> {
    if !core.counts_swarms() {
        return Vec::new();
    }
    let mut asks = Vec::new();
    for s in streams.iter().filter(|s| s.cached != Some(true)) {
        let Some(hash) = info_hash(s) else {
            continue;
        };
        if asks.len() == LIVE_MAX || !scraped.insert(hash.clone()) {
            continue;
        }
        let trackers = match &s.source {
            mq_protocol::Source::Torrent { trackers, .. } => trackers.clone(),
            _ => Vec::new(),
        };
        asks.push((hash, trackers));
    }
    asks
}

/// One scrape of the trackers: an ask takes at most 10 s, and one that
/// answers nothing at all is asked again a second later, so counts can come
/// up to about 21 s after an answer; `done` holds for them at most
/// `SIDE_WAIT` once the rest is in, and later ones rank the rows again. A
/// swarm no answer told anything about keeps the addon's number: the
/// engine gives no count for a default tracker's 0/0, the answer for a
/// swarm it does not track, and gives a zero that a tracker counting
/// leechers, or the stream's own tracker, reported (`Engine::scrape`).
async fn live_check(core: Core, swarms: Vec<(String, Vec<String>)>) -> Side {
    let ask = || async {
        let counts = tokio::time::timeout(LIVE_WAIT, core.live_seeders(&swarms)).await;
        counts.unwrap_or_else(|_| vec![None; swarms.len()])
    };
    // A cold first scrape (the engine starting, tracker names not resolved
    // yet) can come back with no answer at all: ask once more a moment
    // later (Android walk EW-1, the first open after an install).
    let mut counts = ask().await;
    if counts.iter().all(Option::is_none) {
        tokio::time::sleep(LIVE_RETRY).await;
        counts = ask().await;
    }
    Side::Live(
        swarms
            .into_iter()
            .zip(counts)
            .filter_map(|((hash, _), n)| Some((hash, n?)))
            .collect(),
    )
}

/// Live seeders over the addons' numbers, on every row of the swarm.
fn apply_live(streams: &mut [ParsedStream], live: &HashMap<String, u32>) {
    if live.is_empty() {
        return;
    }
    for stream in streams.iter_mut() {
        if let Some(n) = info_hash(stream).and_then(|h| live.get(&h)) {
            stream.seeders = Some(*n);
        }
    }
}

/// One cache check, within 3 s; a store without a reliable check, an error
/// or no answer in time leaves the rows as they were.
async fn cache_check(core: Core, store: Option<mq_debrid::StoreId>, hashes: Vec<String>) -> Side {
    use mq_debrid::CacheStatus;
    let Some(resolver) = core.debrid() else {
        return Side::Cache(Vec::new());
    };
    let answers = tokio::time::timeout(SIDE_WAIT, resolver.check(store, &hashes)).await;
    let Ok(Ok(answers)) = answers else {
        return Side::Cache(Vec::new());
    };
    Side::Cache(
        hashes
            .into_iter()
            .zip(answers)
            .filter_map(|(h, a)| match a {
                CacheStatus::Cached => Some(((store, h), true)),
                CacheStatus::NotCached => Some(((store, h), false)),
                CacheStatus::Unknown => None,
            })
            .collect(),
    )
}

/// The store a stream resolves on: the one its service names when that
/// account is here, else the default (`None`).
fn store_of(s: &ParsedStream, stores: &[mq_debrid::StoreId]) -> Option<mq_debrid::StoreId> {
    s.service
        .as_deref()
        .and_then(mq_debrid::StoreId::from_service)
        .filter(|id| stores.contains(id))
}

/// Our cache answers on the rows no addon said anything about, each from
/// the store it resolves on.
fn apply_cache(
    streams: &mut [ParsedStream],
    cache: &HashMap<CacheKey, bool>,
    stores: &[mq_debrid::StoreId],
) {
    if cache.is_empty() {
        return;
    }
    for stream in streams.iter_mut().filter(|st| st.cached.is_none()) {
        let key = info_hash(stream).map(|h| (store_of(stream, stores), h));
        if let Some(cached) = key.and_then(|k| cache.get(&k)) {
            stream.cached = Some(*cached);
        }
    }
}

/// `blocked` and `why`, from the ranking now: after every rerank, so a
/// later change (the network, a failure learned) never leaves them stale.
fn reblock(s: &mut State, ranking: &Ranking) {
    use BlockedReason::*;
    s.blocked = blocked(s, ranking, [NeedsAccount, NothingPlaysHere]);
    s.why = blocked(s, ranking, [NeedsAccount, NothingPlaysHere]);
}

/// Every addon answered, nothing is listed, and the hidden rows say why
/// (3.11): the first reason in `order` that hid any, how many, and those
/// rows ranked as they would be once it is fixed. `blocked` puts an account
/// first (it opens the most), then a device that plays nothing found. No
/// version is ever hidden for space (plans/no-space-warnings.md).
fn blocked(s: &State, ranking: &Ranking, order: [BlockedReason; 2]) -> Option<BlockedVersions> {
    if !s.done || !s.ranked.rows.is_empty() {
        return None;
    }
    let of = |reason| -> Vec<ParsedStream> {
        s.ranked
            .hidden
            .iter()
            .filter(|h| h.reason == reason)
            .map(|h| h.stream.clone())
            .collect()
    };
    let params = &ranking.params;
    let mut caps = params.caps.clone();
    let mut c = s.rank.clone();
    let (reason, streams) = order
        .into_iter()
        .map(|reason| {
            let hidden = match reason {
                BlockedReason::NeedsAccount => HideReason::NeedsDebrid,
                BlockedReason::NothingPlaysHere => HideReason::Unplayable,
            };
            (reason, of(hidden))
        })
        .find(|(_, streams)| !streams.is_empty())?;
    match reason {
        BlockedReason::NeedsAccount => c.debrid = true,
        BlockedReason::NothingPlaysHere => caps.has_fallback_engine = true,
    }
    let count = streams.len() as u32;
    let input = RankInput {
        caps: &caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &c,
    };
    let rows = rank(streams, &input)
        .rows
        .iter()
        .map(|r| row_dto(r, None, &c))
        .collect();
    Some(BlockedVersions {
        reason,
        count,
        rows,
    })
}

/// Once the pick settles (every addon answered, or the user moved focus),
/// warm its debrid link so Play starts from the cache.
fn maybe_prefetch(core: &Core, s: &mut State, (season, episode): (Option<u32>, Option<u32>)) {
    if s.prefetched {
        return;
    }
    if let Some(best) = &s.recommended {
        crate::views::playback::prefetch(core, &best.stream, season, episode);
        s.prefetched = true;
    }
}

/// The manifest formats caps list as containers: `m3u8` for HLS, `mpd`
/// for DASH.
fn plays(caps: &DeviceCaps) -> Vec<mq_server::Format> {
    let lists = |name: &str| caps.containers.iter().any(|c| c.eq_ignore_ascii_case(name));
    [
        ("m3u8", mq_server::Format::Hls),
        ("mpd", mq_server::Format::Dash),
    ]
    .into_iter()
    .filter(|(name, _)| lists(name))
    .map(|(_, format)| format)
    .collect()
}

/// What the ranking reads: the view's params, for a kids profile the
/// allowed title's names (plans/m11-kids.md 4.4), and the title's id for
/// its meta.
struct Ranking {
    params: VersionsParams,
    kids: Option<Vec<String>>,
    item_id: String,
    /// The release a Resume of this video plays: the title's remembered
    /// one, when the video has a resume point.
    remembered: Option<String>,
}

/// The title's remembered release when `video_id` resumes (it has a resume
/// point): a Resume keeps it first (plans/play-every-version.md 2f).
fn resumes(core: &Core, item_id: &str, video_id: &str) -> Option<String> {
    let progress = core.library().progress(core.profile(), video_id).ok()?;
    crate::views::playback::resume_point(progress)?;
    let memory = core
        .library()
        .stream_memory(core.profile(), item_id)
        .ok()??;
    let memory: serde_json::Value = serde_json::from_str(&memory).ok()?;
    memory["key"].as_str().map(str::to_owned)
}

/// The pick a Resume keeps first (2f): the release this video resumes,
/// when it can start; else none, and the ranking's best is the pick. Up
/// Next keeps the title's release line through the ranking itself (its
/// `binge` key, `last_binge_group`), which still puts a version that keeps
/// up with the connection first.
fn preferred(rows: &[RankedRow], ranking: &Ranking) -> Option<RankedRow> {
    let key = ranking.remembered.as_ref()?;
    holding(rows, |st| &st.key == key).filter(|r| r.key.viable)
}

/// A row a shortcut may offer beside `pick`: another version (not a
/// source of the pick's), that can start, in the viewer's language, and
/// with no download first.
fn shortcut(r: &RankedRow, pick: &RankedRow) -> bool {
    r.key.viable
        && !matches!(r.group, Group::OtherLanguages | Group::NeedsDownload)
        && std::iter::once(&r.stream)
            .chain(&r.failover)
            .all(|st| st.key != pick.stream.key)
}

/// "Smaller file": the first row under half the pick's size.
fn smaller(rows: &[RankedRow], pick: &RankedRow) -> Option<String> {
    let size = pick.stream.size_bytes?;
    rows.iter()
        .find(|r| shortcut(r, pick) && r.stream.size_bytes.is_some_and(|b| b * 2 < size))
        .map(|r| r.stream.key.clone())
}

/// "Sharper": the first row of a higher resolution than the pick's, up to
/// the screen's class.
fn sharper(rows: &[RankedRow], pick: &RankedRow, screen: u32) -> Option<String> {
    let height = |r: &RankedRow| r.stream.release.resolution.height();
    rows.iter()
        .find(|r| {
            // A pick of no stated resolution has nothing to be sharper than.
            height(pick) > 0 && shortcut(r, pick) && height(r) > height(pick) && height(r) <= screen
        })
        .map(|r| r.stream.key.clone())
}

/// A row's quality section (2f).
fn section(r: &RankedRow) -> SectionQuality {
    match r.stream.release.resolution.height() {
        0 => SectionQuality::Other,
        h if h >= 2000 => SectionQuality::Uhd,
        h if h >= 1000 => SectionQuality::Fhd,
        _ => SectionQuality::Hd,
    }
}

fn input<'a>(ranking: &'a Ranking, conditions: &'a Conditions) -> RankInput<'a> {
    let params = &ranking.params;
    RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions,
    }
}

fn rerank(s: &mut State, ranking: &Ranking) {
    let params = &ranking.params;
    s.ranked = rank(s.streams.clone(), &input(ranking, &s.rank));
    if let Some(download) = &s.download {
        // The kept file stays the pick, whatever the addons list.
        s.recommended = Some(download.clone());
    } else if !s.frozen {
        s.recommended = preferred(&s.ranked.rows, ranking).or_else(|| s.ranked.best().cloned());
        let screen = screen_class(params.caps.display_height);
        let pick = s.recommended.as_ref();
        s.smaller = pick.and_then(|p| smaller(&s.ranked.rows, p));
        s.sharper = pick.and_then(|p| sharper(&s.ranked.rows, p, screen));
    } else if let Some(row) = s
        .recommended
        .as_ref()
        .and_then(|p| follow(&s.ranked.rows, p))
    {
        // Frozen: the pick stays, its facts (live seeders, line) follow.
        s.recommended = Some(row);
    }
    rank_archive(s, ranking);
    rank_plugins(s, ranking);
    rank_kind(s, ranking);
    reblock(s, ranking);
}

/// The row of the version a download keeps for this video, when it plays
/// from disk now (plans/offline-downloads.md 4.12): done, or (offline) a
/// file still being written, which plays as far as it holds.
fn downloaded(core: &Core, s: &State, ranking: &Ranking) -> Option<RankedRow> {
    let downloads = core.downloads();
    let (stream, row) = downloads.kept(&ranking.item_id, &ranking.params.video_id)?;
    let partial = row.bytes_done > 0 && row.state.holds_part();
    // Online, a partial torrent plays at once when its start is on disk and
    // its swarm answers now (its reader fetches the rest first); a dead
    // swarm's never beats a ready version (apps r2 P2-2, code review r1
    // P2-5).
    let swarm_ready = partial
        && downloads.has_peers(&row.id)
        && torrent_start_on_disk(core, &ranking.item_id, &ranking.params.video_id);
    let plays = row.state == crate::downloads::DownloadState::Done
        || swarm_ready
        || (partial && downloads.link().offline);
    if !plays {
        return None;
    }
    let params = &ranking.params;
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &s.rank,
    };
    mq_streams::rank_kept(stream, &input)
}

/// A torrent download's start (its head and index) is verified on disk.
fn torrent_start_on_disk(core: &Core, item_id: &str, video_id: &str) -> bool {
    #[cfg(feature = "torrent")]
    if let Some((_, t, _)) = core.downloads().torrent_download(item_id, video_id) {
        return mq_engine::cached_on_disk(core.torrent_dir(), &t.hash, t.file, 0)
            .is_some_and(|c| c.ready);
    }
    let _ = (core, item_id, video_id);
    false
}

/// The row `pick` is now: its own row while it still leads with the same
/// source, else the row a later answer folded it into, still led by its
/// own source.
fn follow(rows: &[RankedRow], pick: &RankedRow) -> Option<RankedRow> {
    let same = |st: &ParsedStream| same_source(st, &pick.stream);
    rows.iter()
        .find(|r| same(&r.stream))
        .cloned()
        .or_else(|| holding(rows, same))
}

/// How a source plays, as the kind chips group it.
fn kind_of(s: &State, st: &ParsedStream) -> SourceKind {
    kind(st, s.rank.debrid, s.rank.engine)
}

/// Whether row `r` holds a source of kind `k`, its own or one in its chain.
fn holds_kind(s: &State, r: &RankedRow, k: SourceKind) -> bool {
    std::iter::once(&r.stream)
        .chain(&r.failover)
        .any(|st| kind_of(s, st) == k)
}

/// The chosen kind's own pass (plans/plugins-and-marketplace.md 4.2): its
/// streams ranked alone, so rows fold only among sources of that kind and
/// each row's class and line are its own, never a re-led row's. Its card
/// follows its source once shown; a kind with no rows left clears itself.
fn rank_kind(s: &mut State, ranking: &Ranking) {
    let Some(k) = s.kind else { return };
    let streams = (s.streams.iter())
        .filter(|st| kind_of(s, st) == k)
        .cloned()
        .collect();
    s.kind_ranked = rank(streams, &input(ranking, &s.rank));
    let own_pass = (s.archive_ranked.rows.iter())
        .chain(&s.plugins_ranked.rows)
        .any(|r| holds_kind(s, r, k));
    if s.kind_ranked.rows.is_empty() && !own_pass {
        s.kind = None;
        s.kind_pick = None;
        return;
    }
    let card = s
        .kind_pick
        .as_ref()
        .and_then(|p| follow(&s.kind_ranked.rows, p));
    s.kind_pick = card.or_else(|| s.kind_ranked.best().cloned());
}

/// The same source: one key from one install and one service.
fn same_source(a: &ParsedStream, b: &ParsedStream) -> bool {
    (&a.key, &a.addon_url, &a.service) == (&b.key, &b.addon_url, &b.service)
}

/// The row holding a source `is` matches, led by that source: a row's own
/// head, or a source folded into its failover chain.
fn holding(rows: &[RankedRow], is: impl Fn(&ParsedStream) -> bool) -> Option<RankedRow> {
    rows.iter().find_map(|r| {
        if is(&r.stream) {
            return Some(r.clone());
        }
        let at = r.failover.iter().position(&is)?;
        let mut led = r.clone();
        let source = led.failover.remove(at);
        let head = std::mem::replace(&mut led.stream, source);
        led.failover.insert(0, head);
        Some(led)
    })
}

/// What the device recorded since the view opened (a swarm that stalled,
/// a file with no picture, a version its player could not decode) is read
/// again and the rows rank again; a frozen pick that just failed is let
/// go, so the next one can lead (code review 2026-10-20 P2-1).
fn relearn(core: &Core, s: &mut State, failed: Option<&str>) {
    let (lib, now) = (core.library(), mq_library::hlc::now_ms());
    s.rank.dead_swarms = crate::speed::dead_swarms(lib, now);
    s.rank.no_picture = crate::speed::no_picture(lib);
    s.rank.did_not_play = crate::speed::did_not_play(lib, now);
    s.rank.wrong_file = crate::speed::wrong_file(lib, now);
    s.rank.containers = crate::speed::containers(lib);
    let pick_failed = s
        .recommended
        .as_ref()
        .zip(failed)
        .is_some_and(|(r, key)| r.stream.key == key);
    if pick_failed {
        s.frozen = false;
    }
    if let Some(ranking) = s.ranking.clone() {
        rerank(s, &ranking);
    }
}

/// What the snapshot says the ranking read (4.2).
fn view_conditions(core: &Core, params: &VersionsParams, c: &Conditions) -> VersionsConditions {
    let prefs = &params.prefs;
    VersionsConditions {
        screen_class: screen_class(params.caps.display_height),
        hdr: prefs.hdr && !params.caps.display_hdr.is_empty(),
        network: core.network(),
        http: c.http,
        p2p: c.p2p,
        free_bytes: c.free_bytes,
        runtime_min: c.runtime_s.map(|s| s / 60),
        size_limit_bytes: prefs.size_limit_bytes,
        min_seeders: prefs.min_seeders,
        engine: core.has_engine(),
        account: core.debrid().is_some(),
    }
}

fn names(installs: &[(Url, String)]) -> Vec<String> {
    installs.iter().map(|(_, name)| name.clone()).collect()
}

fn render(s: &State) -> VersionsSnapshot {
    let kept = s.on_device.as_deref();
    // Under a kind chip the card, rows and sections are that kind's pass
    // (plans/plugins-and-marketplace.md 4.2), with no shortcuts: they are
    // relative to the core's pick.
    let (pick, ranked) = match s.kind {
        Some(_) => (s.kind_pick.as_ref(), &s.kind_ranked.rows),
        None => (s.recommended.as_ref(), &s.ranked.rows),
    };
    let recommended_key = pick.map(|r| r.stream.key.clone());
    // A row whose chain holds the pick is the pick's own row, folded or
    // not: shown once, as the pick (code review 2026-10-20 r2 P3-2).
    let holds_pick = |r: &&RankedRow| {
        std::iter::once(&r.stream)
            .chain(&r.failover)
            .any(|st| Some(&st.key) == recommended_key.as_ref())
    };
    let listed: Vec<&RankedRow> = ranked.iter().filter(|r| !holds_pick(r)).collect();
    let rows = listed.iter().map(|r| row_dto(r, kept, &s.rank)).collect();
    let sections = [
        SectionQuality::Uhd,
        SectionQuality::Fhd,
        SectionQuality::Hd,
        SectionQuality::Other,
    ]
    .into_iter()
    .map(|quality| VersionSection {
        quality,
        keys: (listed.iter())
            .filter(|r| section(r) == quality)
            .map(|r| r.stream.key.clone())
            .collect(),
    })
    .filter(|section| !section.keys.is_empty())
    .collect();
    // A shortcut follows its row's facts, led by its own source, while its
    // key is listed.
    let shortcut_row = |key: &Option<String>| {
        let key = key.as_ref().filter(|_| s.kind.is_none())?;
        holding(&s.ranked.rows, |st| &st.key == key).map(|r| row_dto(&r, kept, &s.rank))
    };
    let mut by_reason: Vec<(HideReason, u32)> = Vec::new();
    for h in &s.ranked.hidden {
        match by_reason.iter_mut().find(|(r, _)| *r == h.reason) {
            Some((_, n)) => *n += 1,
            None => by_reason.push((h.reason, 1)),
        }
    }
    VersionsSnapshot {
        conditions: s.conditions.clone(),
        blocked: s.blocked.clone(),
        empty: empty(s),
        note: s.note.clone(),
        recommended: pick.map(|r| {
            let mut row = row_dto(r, kept, &s.rank);
            if s.kind.is_none() && s.download.is_some() {
                row.on_device = Some(crate::views::playback::DOWNLOADED.to_owned());
            }
            row
        }),
        rows,
        hidden: HiddenSummary {
            count: s.ranked.hidden.len() as u32,
            by_reason,
        },
        pending_addons: names(&s.pending),
        slow_addons: names(&s.slow),
        failed_addons: names(&s.failed),
        done: s.done,
        frozen: s.frozen,
        smaller: shortcut_row(&s.smaller),
        sharper: shortcut_row(&s.sharper),
        sections,
        nobody_sharing: nobody_sharing(s),
        archive: SourceRows {
            rows: (s.archive_ranked.rows.iter())
                .filter(|r| s.kind.is_none_or(|k| holds_kind(s, r, k)))
                .map(|r| row_dto(r, kept, &s.rank))
                .collect(),
            pending: s.archive_pending,
        },
        plugins: SourceRows {
            rows: (s.plugins_ranked.rows.iter())
                .filter(|r| s.kind.is_none_or(|k| holds_kind(s, r, k)))
                .map(|r| row_dto(r, kept, &s.rank))
                .collect(),
            pending: s.plugins_pending,
        },
        kinds: kinds(s),
        selected_kind: s.kind,
    }
}

/// Each kind the listed rows hold, in the chips' order (torrents, direct
/// links, debrid), with the rows holding a source of it, from the pass of
/// every kind, the Archive's and the plugins', so the label and the list
/// agree: a row of a torrent and a direct link counts under both.
fn kinds(s: &State) -> Vec<KindChip> {
    [SourceKind::Torrent, SourceKind::Direct, SourceKind::Debrid]
        .into_iter()
        .filter_map(|k| {
            let held: Vec<Vec<&ParsedStream>> = (s.ranked.rows.iter())
                .chain(&s.archive_ranked.rows)
                .chain(&s.plugins_ranked.rows)
                .map(|r| {
                    let all = std::iter::once(&r.stream).chain(&r.failover);
                    all.filter(|st| kind_of(s, st) == k).collect()
                })
                .filter(|v: &Vec<_>| !v.is_empty())
                .collect();
            let service = (k == SourceKind::Debrid)
                .then(|| one_service(held.iter().flatten().copied(), &s.stores))
                .flatten();
            (!held.is_empty()).then_some(KindChip {
                kind: k,
                service,
                count: held.len() as u32,
            })
        })
        .collect()
}

/// The one service every debrid source resolves on ("Real-Debrid"), else
/// none: a source names its service, or with exactly one account it
/// resolves on that account; with several accounts a source that names
/// none could resolve on any of them.
fn one_service<'a>(
    sources: impl Iterator<Item = &'a ParsedStream>,
    stores: &[mq_debrid::StoreId],
) -> Option<String> {
    let account = match stores {
        [only] => Some(service_name(only.name())),
        _ => None,
    };
    let mut names = sources.map(|st| {
        st.service
            .as_deref()
            .map(service_name)
            .or_else(|| account.clone())
    });
    let first = names.next()??;
    names
        .all(|n| n.as_deref() == Some(first.as_str()))
        .then_some(first)
}

/// Why nothing can play, once the view is done and nothing is listed
/// (plans/play-every-version.md 2a): no addon, no addon answered, a kids
/// profile, the hidden rows' reason (an account, room, this device), else
/// nothing that is a version of this video (another title or episode, an
/// excluded one).
fn empty(s: &State) -> Option<EmptyVersions> {
    let named = |kind, action| {
        Some(EmptyVersions {
            kind,
            action,
            count: 0,
            rows: Vec::new(),
            key: None,
        })
    };
    // The refused view asks no addon.
    if s.kids && s.addons == 0 {
        return named(EmptyKind::KidsNothing, EmptyAction::None);
    }
    // The Archive or a plugin may still list it, or lists it (4.4).
    if s.archive_pending
        || !s.archive_ranked.rows.is_empty()
        || s.plugins_pending
        || !s.plugins_ranked.rows.is_empty()
    {
        return None;
    }
    if !s.done || !s.ranked.rows.is_empty() || s.download.is_some() {
        return None;
    }
    if s.addons == 0 {
        return named(EmptyKind::NoSource, EmptyAction::SettingsAddons);
    }
    if s.unreached >= s.addons {
        // Settled first: no Try again that turns into Play from this device.
        if !s.kept_checked {
            return None;
        }
        let key = s.kept.as_ref().map(|k| k.key.clone());
        return Some(EmptyVersions {
            kind: EmptyKind::Unreachable,
            action: if key.is_some() {
                EmptyAction::PlayFromDevice
            } else {
                EmptyAction::TryAgain
            },
            count: 0,
            rows: Vec::new(),
            key,
        });
    }
    if s.kids {
        return named(EmptyKind::KidsNothing, EmptyAction::None);
    }
    if let Some(b) = &s.why {
        let (kind, action) = match b.reason {
            // Said quietly, with no action: an account is set up in
            // Settings only (the owner, 2026-10-23).
            BlockedReason::NeedsAccount => (EmptyKind::NeedsAccount, EmptyAction::None),
            BlockedReason::NothingPlaysHere => {
                (EmptyKind::NothingPlaysHere, EmptyAction::ChooseVersion)
            }
        };
        return Some(EmptyVersions {
            kind,
            action,
            count: b.count,
            rows: b.rows.clone(),
            key: None,
        });
    }
    named(EmptyKind::NoResults, EmptyAction::None)
}

/// Every listed version is a swarm the engine would fetch, and every
/// source of it a swarm whose trackers count one seeder or none. A source
/// without a live count, a link, or one a service may have ready can still
/// start.
fn nobody_sharing(s: &State) -> bool {
    let dead_source = |st: &ParsedStream| {
        let maybe_ready = st.cached == Some(true) || (s.rank.debrid && st.cached.is_none());
        !maybe_ready && info_hash(st).is_some_and(|h| s.live.get(&h).is_some_and(|n| *n <= 1))
    };
    let dead = |r: &RankedRow| {
        r.class == SourceClass::Swarm
            && std::iter::once(&r.stream)
                .chain(&r.failover)
                .all(dead_source)
    };
    !s.ranked.rows.is_empty() && s.ranked.rows.iter().all(dead)
}

/// The row for `r`; `kept` is the key of the version that starts from what
/// this device keeps, the row's own or one in its chain, said only while it
/// would play on the engine (with an account a streaming service may take
/// it instead, review P3-9); `c` says whether there is an account and an
/// engine.
fn row_dto(r: &RankedRow, kept: Option<&str>, c: &Conditions) -> VersionRow {
    let s = &r.stream;
    let on_device = std::iter::once(s).chain(&r.failover).any(|st| {
        Some(st.key.as_str()) == kept && crate::views::playback::by_engine(st, c.debrid, true)
    });
    let rel = &s.release;
    let archive = s.key.starts_with(crate::archive::KEY_PREFIX);
    let plugin = crate::plugins::row_source(s);
    VersionRow {
        key: s.key.clone(),
        quality: quality_word(rel.resolution.height(), rel.quality),
        features: features(rel),
        size_bytes: s.size_bytes,
        status: status(s),
        via: s
            .service
            .as_deref()
            .map(service_name)
            .unwrap_or_else(|| s.addon_name.clone()),
        release_name: s
            .filename
            .clone()
            .unwrap_or_else(|| first_line(&s.raw_description, &s.raw_name)),
        languages: rel.languages.clone(),
        reasons: r.reasons.clone(),
        group: r.group,
        failover_count: r.failover.len() as u16,
        line: r.line.clone(),
        source_class: r.class,
        kind: kind(s, c.debrid, c.engine),
        bitrate_mbps: r.bitrate_mbps,
        score: r.key,
        on_device: on_device.then(|| crate::views::playback::ON_DEVICE.to_owned()),
        origin: if archive {
            RowOrigin::Archive
        } else if plugin.is_some() {
            RowOrigin::Plugin
        } else {
            RowOrigin::Addon
        },
        host: if archive {
            Some(crate::archive::HOST.to_owned())
        } else {
            plugin.as_ref().map(|(_, host)| host.clone())
        },
        source_name: if archive {
            Some(crate::archive::NAME.to_owned())
        } else {
            plugin.map(|(name, _)| name)
        },
    }
}

pub(crate) fn quality_word(height: u32, source: mq_relparse::Quality) -> String {
    use mq_relparse::Quality as Q;
    match height {
        h if h >= 2160 => "4K",
        h if h >= 1440 => "1440p",
        h if h >= 1080 => "1080p",
        h if h >= 720 => "720p",
        // A headline never says "unknown": the source is the next best fact.
        0 => match source {
            Q::Remux => "Remux",
            Q::BluRay => "Blu-ray",
            Q::WebDl => "WEB-DL",
            Q::WebRip => "WEBRip",
            Q::Hdtv => "HDTV",
            Q::Dvd => "DVD",
            Q::Screener => "Screener",
            Q::Telesync => "Telesync",
            Q::Cam => "Cam",
            Q::Unknown => "Quality not stated",
        },
        _ => "SD",
    }
    .into()
}

fn features(rel: &mq_relparse::ParsedRelease) -> Vec<String> {
    use mq_relparse::{Audio, Hdr, Quality};
    let mut out = Vec::new();
    for h in &rel.hdr {
        out.push(
            match h {
                Hdr::DolbyVision => "Dolby Vision",
                Hdr::Hdr10Plus => "HDR10+",
                Hdr::Hdr10 => "HDR10",
                Hdr::Hlg => "HLG",
                Hdr::Sdr => continue,
            }
            .to_owned(),
        );
    }
    if rel.audio.contains(&Audio::Atmos) {
        out.push("Atmos".into());
    } else if rel.audio.iter().any(|a| matches!(a, Audio::TrueHd)) {
        out.push("TrueHD".into());
    } else if rel
        .audio
        .iter()
        .any(|a| matches!(a, Audio::DtsHdMa | Audio::DtsX))
    {
        out.push("DTS-HD".into());
    }
    if let Some(ch) = &rel.channels
        && ch != "stereo"
        && ch != "mono"
    {
        out.push(ch.clone());
    }
    if rel.quality == Quality::Remux || rel.remux {
        out.push("Remux".into());
    }
    // Audio described for blind and low-vision viewers (core backlog C35).
    if rel.audio_description {
        out.push("AD".into());
    }
    out
}

fn status(s: &ParsedStream) -> VersionStatus {
    use mq_protocol::Source;
    match (&s.source, s.cached) {
        (_, Some(true)) => VersionStatus::Ready {
            service: s.service.as_deref().map(service_name),
        },
        (Source::ClientResolve(_), _) => VersionStatus::NeedsDownload {
            service: s.service.as_deref().map(service_name),
        },
        (Source::Torrent { .. }, _) => VersionStatus::Torrent { seeds: s.seeders },
        _ => VersionStatus::Direct,
    }
}

fn service_name(id: &str) -> String {
    match id
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "")
        .as_str()
    {
        "realdebrid" | "rd" => "Real-Debrid",
        "torbox" | "tb" => "TorBox",
        "alldebrid" | "ad" => "AllDebrid",
        "premiumize" | "pm" => "Premiumize",
        "debridlink" => "Debrid-Link",
        "easydebrid" => "EasyDebrid",
        _ => return id.to_owned(),
    }
    .into()
}

fn first_line(description: &str, name: &str) -> String {
    description
        .lines()
        .next()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or(name)
        .trim()
        .to_owned()
}

/// Tests: a view whose ranking lists `rows`, nothing recommended. Their
/// sources are the view's streams, so a relearn ranks them again.
#[cfg(test)]
impl VersionsView {
    pub(crate) fn rows_for_tests(&self, rows: Vec<RankedRow>) {
        let mut s = lock(&self.state);
        s.streams = rows
            .iter()
            .flat_map(|r| std::iter::once(&r.stream).chain(&r.failover))
            .cloned()
            .collect();
        s.ranked = Ranked {
            rows,
            hidden: Vec::new(),
        };
        s.recommended = None;
    }

    /// The first listed row is the recommendation.
    #[cfg(feature = "torrent")]
    pub(crate) fn recommend_first_for_tests(&self) {
        let mut s = lock(&self.state);
        s.recommended = s.ranked.rows.first().cloned();
    }
}

/// Tests: the parameters of a film for a 1080p stick.
#[cfg(test)]
pub(crate) fn film_params() -> VersionsParams {
    VersionsParams {
        device_language: None,
        kind: "movie".into(),
        video_id: "tt1".into(),
        target: Target {
            title: "Movie".into(),
            year: Some(2020),
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        // Facts sent: no meta is asked, so nothing ranks the view again.
        facts: TitleFacts::read(Some("120 min"), Some("2020"), None, None),
    }
}

/// Tests: `streams` ranked for a 1080p stick with the engine.
#[cfg(test)]
pub(crate) fn rank_for_tests(streams: Vec<ParsedStream>) -> Vec<RankedRow> {
    let params = film_params();
    let conditions = Conditions {
        engine: true,
        free_bytes: Some(1 << 40),
        ..Conditions::default()
    };
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: None,
        kids: None,
        conditions: &conditions,
    };
    rank(streams, &input).rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Core backlog C35: a release with audio description carries the AD
    /// feature, after the picture and sound ones; one without carries none.
    #[test]
    fn a_described_release_carries_the_ad_feature() {
        let described =
            mq_relparse::parse("Movie.2020.2160p.WEB-DL.DV.Audio.Description.DDP5.1.H.265-GRP");
        assert_eq!(features(&described).last().map(String::as_str), Some("AD"));
        let plain = mq_relparse::parse("Movie.2020.2160p.WEB-DL.DV.DDP5.1.H.265-GRP");
        assert!(!features(&plain).iter().any(|f| f == "AD"));
    }

    /// Review 2026-10-23 P3: with no addon answering, nothing is said
    /// until the kept check settles, so Try again never turns into Play
    /// from this device.
    #[test]
    fn no_source_answering_waits_for_the_kept_check() {
        let core = Core::temporary("p").unwrap();
        let view = VersionsView::denied_view(&core, &film_params());
        let action = |checked: bool| {
            let mut s = lock(&view.state);
            (s.kids, s.addons, s.unreached, s.done) = (false, 1, 1, true);
            s.kept_checked = checked;
            render(&s).empty.map(|e| e.action)
        };
        assert_eq!(action(false), None);
        assert_eq!(action(true), Some(EmptyAction::TryAgain));
    }

    /// Review 2026-10-23 P3-8: a pick of no stated resolution offers no
    /// Sharper.
    #[test]
    fn no_sharper_than_an_unstated_resolution() {
        let rows = rank_for_tests(vec![
            wire(r#"{"title":"Movie.2020.WEB-DL.x264-AAA","url":"https://a.example/a.mkv"}"#),
            wire(r#"{"title":"Movie.2020.480p.WEB-DL.x264-BBB","url":"https://a.example/b.mkv"}"#),
        ]);
        let pick = (rows.iter())
            .find(|r| r.stream.release.resolution.height() == 0)
            .unwrap();
        assert_eq!(sharper(&rows, pick, 1080), None);
    }

    /// Core backlog C37: the year line the core now sends the apps, and
    /// they send back, still gives the title's year (review 2026-10-23 P2).
    #[test]
    fn title_facts_read_the_cores_year_line() {
        let year = |text: &str| TitleFacts::read(None, Some(text), None, None).title_year;
        assert_eq!(year("Since 2023"), Some(2023));
        assert_eq!(year("2008 to 2013"), Some(2008));
        assert_eq!(year("2023-"), Some(2023));
        assert_eq!(year("2002"), Some(2002));
        assert_eq!(year("TBA"), None);
        let aired = TitleFacts::read(None, None, Some("2013-09-16T00:00:00.000Z"), None);
        assert_eq!(aired.episode_year, Some(2013));
    }

    fn wire(json: &str) -> ParsedStream {
        let body = format!(r#"{{"streams":[{json}]}}"#);
        let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
        normalize(&wire.streams[0], "t", "T", 0, 0)
    }

    /// A torrent's kind follows its cache answer (plans/plugins-and-marketplace.md
    /// 4.1): under an account with the engine, uncached is the engine's. A
    /// selected kind follows: the counts move, the card falls to the kind's
    /// next row, and a kind with none left lists every kind again.
    #[tokio::test]
    async fn a_cache_answer_moves_a_torrent_between_kinds_while_one_is_selected() {
        let core = Core::temporary("p").unwrap();
        let view = VersionsView::open(&core, film_params());
        let torrent = |hash: char, seeds: u32, release: &str| {
            let hash = hash.to_string().repeat(40);
            wire(&format!(
                r#"{{"title":"{release}\n👤 {seeds} 💾 2.0 GB","infoHash":"{hash}"}}"#
            ))
        };
        let link =
            wire(r#"{"title":"Movie.2020.720p.WEB-DL.x264-L","url":"https://a.example/m.mkv"}"#);
        let ranking = Arc::new(Ranking {
            params: film_params(),
            kids: None,
            item_id: "tt1".into(),
            remembered: None,
        });
        let mut s = lock(&view.state);
        s.rank.debrid = true;
        s.rank.engine = true;
        s.rank.free_bytes = Some(1 << 40);
        s.streams = vec![
            torrent('a', 900, "Movie.2020.1080p.BluRay.x264-ALPHA"),
            torrent('b', 300, "Movie.2020.720p.WEBRip.x265-BETA"),
            link,
        ];
        s.ranking = Some(ranking.clone());
        s.kind = Some(SourceKind::Debrid);
        rerank(&mut s, &ranking);
        let counts = |s: &State| {
            kinds(s)
                .iter()
                .map(|k| (k.kind, k.count))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            counts(&s),
            [(SourceKind::Direct, 1), (SourceKind::Debrid, 2)]
        );
        let card = s.kind_pick.clone().unwrap().stream.key;
        // The card's torrent is not cached: the engine plays it, a torrent now.
        for st in s.streams.iter_mut().filter(|st| st.key == card) {
            st.cached = Some(false);
        }
        rerank(&mut s, &ranking);
        assert_eq!(
            counts(&s),
            [
                (SourceKind::Torrent, 1),
                (SourceKind::Direct, 1),
                (SourceKind::Debrid, 1)
            ]
        );
        assert_eq!(s.kind, Some(SourceKind::Debrid));
        let next = s.kind_pick.clone().unwrap();
        assert_ne!(
            next.stream.key, card,
            "the card falls to the kind's next row"
        );
        assert_eq!(kind_of(&s, &next.stream), SourceKind::Debrid);
        // The last debrid torrent goes the same way: no row of the kind is left.
        for st in &mut s.streams {
            st.cached = Some(false);
        }
        rerank(&mut s, &ranking);
        assert_eq!(s.kind, None);
    }

    /// The debrid chip names the one service every debrid source resolves on:
    /// the one it names, or with exactly one account that account.
    #[test]
    fn the_debrid_chip_names_the_one_service_its_sources_resolve_on() {
        let named = |service: Option<&str>| {
            let mut st = wire(r#"{"title":"Movie.2020.1080p","url":"https://a.example/m.mkv"}"#);
            st.service = service.map(str::to_owned);
            st
        };
        use mq_debrid::StoreId::{RealDebrid, TorBox};
        let rd = [named(Some("realdebrid")), named(Some("rd"))];
        assert_eq!(one_service(rd.iter(), &[]).as_deref(), Some("Real-Debrid"));
        let two = [named(Some("realdebrid")), named(Some("torbox"))];
        assert_eq!(one_service(two.iter(), &[RealDebrid]), None);
        // One account resolves a source that names nothing: its name.
        let unnamed = [named(Some("realdebrid")), named(None)];
        assert_eq!(
            one_service(unnamed.iter(), &[RealDebrid]).as_deref(),
            Some("Real-Debrid")
        );
        assert_eq!(
            one_service(unnamed.iter(), &[TorBox]),
            None,
            "the account is not the service named"
        );
        // With two accounts, or none, a source that names nothing could be any service.
        assert_eq!(one_service(unnamed.iter(), &[RealDebrid, TorBox]), None);
        assert_eq!(one_service([named(None)].iter(), &[]), None);
    }

    /// Code review 2026-10-20 r3 P3-8, r4 P3-8: nobodySharing reads every
    /// source of a row: a dead swarm with a link behind it can still play.
    #[tokio::test]
    async fn nobody_sharing_reads_every_source_of_a_row() {
        let core = Core::temporary("p").unwrap();
        let view = VersionsView::open(&core, film_params());
        let hash = "d".repeat(40);
        let swarm = wire(&format!(
            r#"{{"title":"Movie.2020.1080p.WEB-DL.x264-GRP\n👤 1 💾 2.0 GB","infoHash":"{hash}"}}"#
        ));
        let link =
            wire(r#"{"title":"Movie.2020.1080p.WEB-DL.x264-GRP","url":"https://a.example/m.mkv"}"#);
        let mut row = rank_for_tests(vec![swarm]).remove(0);
        assert_eq!(row.class, SourceClass::Swarm);
        let mut s = lock(&view.state);
        s.live.insert(hash, 1);
        s.ranked.rows = vec![row.clone()];
        assert!(nobody_sharing(&s), "a dead swarm alone");
        row.failover.push(link);
        s.ranked.rows = vec![row];
        assert!(!nobody_sharing(&s), "a link behind it can play");
    }

    /// Review P3-9: "On this device" only for a row that plays on the
    /// engine: with a streaming account and no proof the service lacks it,
    /// the service would take it.
    #[test]
    fn on_this_device_only_where_the_engine_plays_it() {
        let torrent = wire(&format!(
            r#"{{"title":"x","infoHash":"{}"}}"#,
            "a".repeat(40)
        ));
        let key = torrent.key.clone();
        let row = &rank_for_tests(vec![torrent])[0];
        let with = |debrid| Conditions {
            debrid,
            ..Conditions::default()
        };
        let said = |account| row_dto(row, Some(&key), &with(account)).on_device;
        assert_eq!(said(false).as_deref(), Some("On this device"));
        assert_eq!(said(true), None, "a service may take it");
        assert_eq!(row_dto(row, Some("another"), &with(false)).on_device, None);
    }

    /// A preferred ready file at its best on every field before the start
    /// tier.
    fn best_ready() -> RankKey {
        RankKey {
            language: true,
            legit: true,
            viable: true,
            year_ok: true,
            original: true,
            sustains: true,
            pace: 3,
            covered: 0,
            playability: Playability::Native,
            size_ok: true,
            tier: 3,
            binge: false,
            effective_height: 1080,
            start: 4,
            within: 500,
        }
    }

    /// Only a ready file at its best on every field a swarm could pass it
    /// through holds the lead against any live count.
    #[test]
    fn only_a_ready_file_at_its_best_is_unbeatable() {
        assert!(unbeatable(&best_ready()));
        let worse: [fn(&mut RankKey); 8] = [
            |k| k.tier = 2,
            |k| k.language = false,
            |k| k.legit = false,
            |k| k.year_ok = false,
            |k| k.original = false,
            |k| k.sustains = false,
            |k| k.size_ok = false,
            |k| k.playability = Playability::Unstated,
        ];
        for (i, worse) in worse.iter().enumerate() {
            let mut k = best_ready();
            worse(&mut k);
            assert!(!unbeatable(&k), "field {i}");
        }
    }

    /// Rows on the same footing differ only in the resolution and what
    /// breaks ties under it; past the speed, how far past it counts too.
    #[test]
    fn the_same_footing_leaves_out_only_the_resolution_and_ties() {
        let a = RankKey {
            tier: 2,
            pace: 0,
            covered: 500,
            ..best_ready()
        };
        let same = RankKey {
            effective_height: 720,
            start: 2,
            within: 900,
            binge: true,
            ..a
        };
        assert!(same_footing(&a, &same));
        assert!(!same_footing(&a, &RankKey { covered: 400, ..a }));
        assert!(!same_footing(&a, &RankKey { tier: 1, ..a }));
    }

    /// How far a version is from one that did not play here: 2 a part
    /// surely apart, 1 one not stated, 0 the same; after a decode failure
    /// the video and the audio, after an audio failure the audio.
    #[test]
    fn how_far_apart_a_version_is_from_what_failed() {
        let rel = mq_relparse::parse;
        let failed = rel("Show.S01E01.1080p.WEB-DL.DDP5.1.Atmos.HEVC.10bit-A");
        let decode = FailedOn::Decode(failed.clone());
        let audio = FailedOn::Audio(failed);
        for (name, after_decode, after_audio) in [
            ("Show.S01E01.1080p.WEB-DL.DDP5.1.HEVC.10bit-SAME", 0, 0),
            ("Show.S01E01.720p.WEB-DL.AAC.x264-BOTH", 4, 2),
            ("Show.S01E01.1080p.WEB-DL.AC3.x264-AC3", 4, 2),
            ("Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-VIDEO", 2, 0),
            ("Show.S01E01.1080p.WEB-DL.TrueHD.HEVC.10bit-AUDIO", 2, 2),
            // Atmos is no codec: TrueHD with Atmos is apart from E-AC-3
            // with Atmos.
            (
                "Show.S01E01.1080p.WEB-DL.TrueHD.Atmos.HEVC.10bit-ATMOS",
                2,
                2,
            ),
            ("Show.S01E01.1080p.WEB-DL.HEVC-UNSTATED", 2, 1),
            ("Show.S01E01.1080p.WEB-DL-NOTHING", 2, 1),
        ] {
            assert_eq!(decode.apart(&rel(name)), after_decode, "decode, {name}");
            assert_eq!(audio.apart(&rel(name)), after_audio, "audio, {name}");
        }
        // A stated bit depth apart is apart; one family of DTS is one codec.
        use mq_relparse::{Audio, ParsedRelease, VideoCodec};
        let file = |codec, bit_depth, audio: &[Audio]| ParsedRelease {
            codec,
            bit_depth,
            audio: audio.to_vec(),
            ..ParsedRelease::default()
        };
        let ten = FailedOn::Decode(file(VideoCodec::H265, Some(10), &[Audio::DtsHdMa]));
        assert_eq!(
            ten.apart(&file(VideoCodec::H265, Some(8), &[Audio::Dts])),
            2
        );
        assert_eq!(
            ten.apart(&file(VideoCodec::H265, Some(10), &[Audio::DtsX])),
            0
        );
        assert_eq!(ten.apart(&file(VideoCodec::Unknown, None, &[])), 2);
        let atmos_only = FailedOn::Audio(file(VideoCodec::H264, None, &[Audio::Atmos]));
        assert_eq!(
            atmos_only.apart(&file(VideoCodec::H264, None, &[Audio::Eac3])),
            1
        );
    }

    /// Code review r1 P3: with two accounts, one store's cache answer
    /// marks only the rows that resolve on it.
    #[test]
    fn a_cache_answer_marks_only_its_own_stores_rows() {
        use mq_debrid::StoreId;
        let row = |service: &str| {
            let body = format!(
                r#"{{"streams":[{{"name":"T","title":"x","infoHash":"{}","behaviorHints":{{"service":"{service}"}}}}]}}"#,
                "a".repeat(40)
            );
            let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
            normalize(&wire.streams[0], "t", "T", 0, 0)
        };
        let mut streams = vec![row("torbox"), row("premiumize")];
        let stores = [StoreId::TorBox, StoreId::Premiumize];
        let cache = HashMap::from([((Some(StoreId::TorBox), "a".repeat(40)), true)]);
        apply_cache(&mut streams, &cache, &stores);
        assert_eq!((streams[0].cached, streams[1].cached), (Some(true), None));
    }

    #[test]
    fn title_facts_read_the_strings_apps_and_cinemeta_send() {
        let f = TitleFacts::read(Some("142 min"), Some("2002"), None, None);
        assert_eq!((f.runtime_s, f.title_year), (Some(142 * 60), Some(2002)));
        let f = TitleFacts::read(
            Some("2h 22min"),
            Some("2005–2013"),
            Some("2013-09-16"),
            Some(16),
        );
        assert_eq!(f.runtime_s, Some(142 * 60));
        assert_eq!((f.title_year, f.episode_year), (Some(2005), Some(2013)));
        assert_eq!(f.episodes_in_season, Some(16));
        let f = TitleFacts::read(Some("45"), Some("2022-"), Some("soon"), Some(0));
        assert_eq!((f.runtime_s, f.title_year), (Some(45 * 60), Some(2022)));
        assert_eq!((f.episode_year, f.episodes_in_season), (None, None));
        assert_eq!(
            TitleFacts::read(Some("N/A"), None, None, None),
            TitleFacts::default()
        );
    }

    #[test]
    fn a_device_plays_hls_when_its_caps_list_m3u8() {
        let mut caps = DeviceCaps::basic_1080p();
        assert!(plays(&caps).is_empty());
        caps.containers.push("M3U8".into());
        assert_eq!(plays(&caps), [mq_server::Format::Hls]);
    }

    /// Apple's real container lists, read from its source (AVPlayer's, and the
    /// second engine's where it is on): HLS, never DASH.
    #[test]
    fn apples_caps_play_hls_and_refuse_dash() {
        let swift = include_str!("../../../../apps/apple/Marquee/Player/EngineChoice.swift");
        let mut caps = DeviceCaps::basic_1080p();
        caps.containers = [
            "static let avPlayerContainers",
            "static let engineContainers",
        ]
        .iter()
        .flat_map(|name| {
            let line = swift
                .lines()
                .find(|l| l.contains(name))
                .expect("Apple's containers");
            line[line.find('[').unwrap() + 1..line.rfind(']').unwrap()]
                .split(',')
                .map(|c| c.trim().trim_matches('"').to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
        assert!(caps.containers.contains(&"m3u8".to_owned()), "{caps:?}");
        assert_eq!(plays(&caps), [mq_server::Format::Hls]);
    }

    #[test]
    fn a_device_plays_dash_when_its_caps_list_mpd() {
        let mut caps = DeviceCaps::basic_1080p();
        caps.containers.push("MPD".into());
        assert_eq!(plays(&caps), [mq_server::Format::Dash]);
        caps.containers.push("m3u8".into());
        let both = [mq_server::Format::Hls, mq_server::Format::Dash];
        assert_eq!(plays(&caps), both);
    }
}
