//! The connection's speed as the core measures it from playbacks
//! (plans/m13-stream-choice.md 5): one estimate per network type and kind of
//! source, kept on this device only (`device_state`).
//!
//! A sample is capacity: bytes received while at least one upstream request
//! was in flight, over that time, in a playback's first 30 s. Reader wait
//! time is never used, so a small file does not lock the estimate low.

use mq_library::Library;
use mq_streams::{Conditions, Speed, key_digest, target_digest};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The network type the app reports.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Network {
    Wifi,
    Ethernet,
    Cellular,
    #[default]
    Other,
}

impl Network {
    fn word(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Ethernet => "ethernet",
            Self::Cellular => "cellular",
            Self::Other => "other",
        }
    }
}

/// What the core knows of this device's connection and disk, for Settings
/// ("This device") and the `streamConditions` op.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamConditions {
    pub network: Network,
    /// Debrid and direct links; `samples` 0 is the prior.
    pub http: Speed,
    /// The torrent engine.
    pub p2p: Speed,
    /// Free space where torrents are kept; null without the engine.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub free_bytes: Option<u64>,
    /// This build has the torrent engine.
    pub engine: bool,
}

/// Debrid and direct links, or the torrent engine: a dead swarm never lowers
/// the debrid estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Http,
    P2p,
    /// The engine on a swarm listed under the wanted seeders (or with none
    /// listed): such a swarm measures itself, not the line, so it is kept
    /// apart (buffer-and-bitrate research R2).
    P2pThin,
}

/// Bytes and time in flight a sample needs at least (r3 P2 1: a fast line
/// reading 4 MiB ahead is in flight well under 2 s).
pub const MIN_BYTES: u64 = 4 << 20;
pub const MIN_BUSY: Duration = Duration::from_millis(250);
/// Samples the estimate keeps: the harmonic mean of the last 5, as MPC and
/// Puffer predict (research R5).
const KEPT_SAMPLES: usize = 5;
/// A sample older than this no longer counts; with none left the estimate
/// is its prior.
const EXPIRES_MS: u64 = 30 * 86_400_000;

/// One playback's capacity sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub kind: Kind,
    pub bytes: u64,
    /// Time with at least one upstream request in flight.
    pub busy: Duration,
    /// The seeders the playing torrent was listed with.
    pub seeders: Option<u32>,
}

/// The last samples of one estimate: (Mbit/s, ms), oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Stored {
    last: Vec<(f32, u64)>,
}

/// What the estimate kept before the harmonic mean: one value, read as one
/// sample taken when it was last moved.
#[derive(Deserialize)]
struct Before {
    mbps: f32,
    at: u64,
}

fn key(network: Network, kind: Kind) -> String {
    let kind = match kind {
        Kind::Http => "http",
        Kind::P2p => "p2p",
        Kind::P2pThin => "p2p_thin",
    };
    format!("speed.{}.{kind}", network.word())
}

fn prior(kind: Kind) -> Speed {
    let c = Conditions::default();
    match kind {
        Kind::Http => c.http,
        Kind::P2p => c.p2p,
        Kind::P2pThin => c.p2p_thin,
    }
}

/// The samples of `kind` on `network` taken in the last 30 days.
fn stored(lib: &Library, network: Network, kind: Kind, now_ms: u64) -> Stored {
    let Some(value) = lib.device_value(&key(network, kind)).ok().flatten() else {
        return Stored::default();
    };
    let mut stored = serde_json::from_str::<Stored>(&value)
        .or_else(|_| {
            serde_json::from_str::<Before>(&value).map(|b| Stored {
                last: vec![(b.mbps, b.at)],
            })
        })
        .unwrap_or_default();
    stored
        .last
        .retain(|(mbps, at)| *mbps > 0.0 && now_ms.saturating_sub(*at) <= EXPIRES_MS);
    stored
}

/// The estimate for `kind` on `network`: the harmonic mean of its last 5
/// samples from the last 30 days (one sample is itself, so the first
/// replaces the prior: a measured line beats a guess), else the prior. The
/// harmonic mean leans to the slow samples when they spread, a margin of
/// its own, and barely moves on a steady line.
pub fn estimate(lib: &Library, network: Network, kind: Kind, now_ms: u64) -> Speed {
    let last = stored(lib, network, kind, now_ms).last;
    if last.is_empty() {
        return prior(kind);
    }
    let inverse: f32 = last.iter().map(|(mbps, _)| 1.0 / mbps).sum();
    Speed {
        mbps: last.len() as f32 / inverse,
        samples: last.len() as u32,
    }
}

/// Counts `sample` into its estimate; false when it does not count (under
/// 4 MiB or 0.25 s in flight). A torrent listed with fewer than
/// `min_seeders`, or with none listed, counts into the small-swarm
/// estimate: a thin swarm measures itself, not the line.
pub fn record(
    lib: &Library,
    network: Network,
    sample: &Sample,
    min_seeders: u32,
    now_ms: u64,
) -> bool {
    if sample.bytes < MIN_BYTES || sample.busy < MIN_BUSY {
        return false;
    }
    let thin = sample.kind == Kind::P2p && sample.seeders.is_none_or(|n| n < min_seeders);
    let kind = if thin { Kind::P2pThin } else { sample.kind };
    let mbps = sample.bytes as f32 * 8.0 / sample.busy.as_secs_f32() / 1e6;
    let mut next = stored(lib, network, kind, now_ms);
    next.last.push((mbps, now_ms));
    let over = next.last.len().saturating_sub(KEPT_SAMPLES);
    next.last.drain(..over);
    let value = serde_json::to_string(&next).unwrap_or_default();
    lib.set_device_value(&key(network, kind), &value).is_ok()
}

/// "Measure again": the estimates of `network` go back to their priors.
pub fn reset(lib: &Library, network: Network) {
    for kind in [Kind::Http, Kind::P2p, Kind::P2pThin] {
        // A stored null reads as no estimate.
        let _ = lib.set_device_value(&key(network, kind), "null");
    }
}

const NETWORK_KEY: &str = "network";

/// The network type the app last reported on this device; "other" before
/// the first report.
pub fn last_network(lib: &Library) -> Network {
    lib.device_value(NETWORK_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// Keeps `network` as this device's network; written only when it changed.
pub fn remember_network(lib: &Library, network: Network) {
    if last_network(lib) != network {
        let value = serde_json::to_string(&network).unwrap_or_default();
        let _ = lib.set_device_value(NETWORK_KEY, &value);
    }
}

/// How long a swarm that stalled here stays demoted.
// ponytail: one day, whatever the network; per network if the race shows a
// swarm that stalls on cellular and starts on Wi-Fi.
const DEAD_FOR_MS: u64 = 86_400_000;
/// Entries a stamped list keeps at most (the oldest go first).
const KEPT: usize = 200;
const DEAD_KEY: &str = "dead_swarms";
/// A file does not change: kept until the list is full.
const NO_PICTURE_KEY: &str = "no_picture";
const DID_NOT_PLAY_KEY: &str = "did_not_play";
/// A torrent's file that was not the version listed, for the target it
/// was played for; kept as long as a decode failure.
const WRONG_FILE_KEY: &str = "wrong_file";
/// How long a version the player could not decode stays demoted.
// ponytail: 30 days, as a decoder rarely changes; a one-off decode error
// demotes a version for that long, never hides it.
const DID_NOT_PLAY_FOR_MS: u64 = 30 * 86_400_000;

/// The entries of the stamped list `store_key` younger than `keep_ms`.
fn stamped(lib: &Library, store_key: &str, keep_ms: u64, now_ms: u64) -> Vec<String> {
    stamped_map(lib, store_key)
        .into_iter()
        .filter(|(_, at)| now_ms.saturating_sub(*at) < keep_ms)
        .map(|(entry, _)| entry)
        .collect()
}

fn stamped_map(lib: &Library, store_key: &str) -> std::collections::BTreeMap<String, u64> {
    lib.device_value(store_key)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

fn stamp(lib: &Library, store_key: &str, entry: String, keep_ms: u64, now_ms: u64) {
    let mut map = stamped_map(lib, store_key);
    map.retain(|_, at| now_ms.saturating_sub(*at) < keep_ms);
    map.insert(entry, now_ms);
    while map.len() > KEPT {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, at)| **at)
            .map(|(e, _)| e.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
    let value = serde_json::to_string(&map).unwrap_or_default();
    let _ = lib.set_device_value(store_key, &value);
}

/// Info hashes (lowercase) whose swarm stalled on this device in the last
/// day: the ranking does not start them first (seeders an addon lists
/// are a hint).
pub fn dead_swarms(lib: &Library, now_ms: u64) -> Vec<String> {
    stamped(lib, DEAD_KEY, DEAD_FOR_MS, now_ms)
}

/// The swarm of `info_hash` sent almost nothing after its metadata.
pub fn record_dead(lib: &Library, info_hash: &str, now_ms: u64) {
    let hash = info_hash.to_ascii_lowercase();
    stamp(lib, DEAD_KEY, hash, DEAD_FOR_MS, now_ms);
}

/// Digests (`key_digest`) of the stream keys whose file showed no picture
/// on this device.
pub fn no_picture(lib: &Library) -> Vec<String> {
    stamped(lib, NO_PICTURE_KEY, u64::MAX, 0)
}

/// The file of `stream_key` showed no picture on this device. Only its
/// digest is kept: a key can carry an account's API key.
pub fn record_no_picture(lib: &Library, stream_key: &str, now_ms: u64) {
    stamp(
        lib,
        NO_PICTURE_KEY,
        key_digest(stream_key),
        u64::MAX,
        now_ms,
    );
}

/// Digests of the stream keys this device's player could not decode in
/// the last 30 days.
pub fn did_not_play(lib: &Library, now_ms: u64) -> Vec<String> {
    stamped(lib, DID_NOT_PLAY_KEY, DID_NOT_PLAY_FOR_MS, now_ms)
}

/// The player could not decode the stream `stream_key`; its digest is kept.
pub fn record_did_not_play(lib: &Library, stream_key: &str, now_ms: u64) {
    let digest = key_digest(stream_key);
    stamp(lib, DID_NOT_PLAY_KEY, digest, DID_NOT_PLAY_FOR_MS, now_ms);
}

/// Digests (`target_digest`) of the stream keys whose torrent file was
/// not the version listed for the target it played for, in the last 30
/// days (code review 2026-10-20 r2 P3-3, r3 P3-3).
pub fn wrong_file(lib: &Library, now_ms: u64) -> Vec<String> {
    stamped(lib, WRONG_FILE_KEY, DID_NOT_PLAY_FOR_MS, now_ms)
}

/// The torrent file `stream_key` played for an episode (`season`,
/// `episode`) or a film was not the version it listed.
pub fn record_wrong_file(
    lib: &Library,
    stream_key: &str,
    (season, episode): (Option<u32>, Option<u32>),
    now_ms: u64,
) {
    let digest = target_digest(stream_key, season, episode);
    stamp(lib, WRONG_FILE_KEY, digest, DID_NOT_PLAY_FOR_MS, now_ms);
}

/// Lists written before digests kept raw stream keys: each becomes its
/// digest (code review 2026-10-20 P2-5). Run when the core opens.
pub fn digest_version_lists(lib: &Library) {
    for store_key in [NO_PICTURE_KEY, DID_NOT_PLAY_KEY] {
        let map = stamped_map(lib, store_key);
        if map.keys().all(|k| is_digest(k)) {
            continue;
        }
        let digested: std::collections::BTreeMap<String, u64> = map
            .into_iter()
            .map(|(k, at)| {
                if is_digest(&k) {
                    (k, at)
                } else {
                    (key_digest(&k), at)
                }
            })
            .collect();
        let value = serde_json::to_string(&digested).unwrap_or_default();
        let _ = lib.set_device_value(store_key, &value);
    }
}

fn is_digest(entry: &str) -> bool {
    entry.len() == 33 && entry.starts_with('h') && entry[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1_000_000;

    fn sample(kind: Kind, mbps: u64, secs: f32) -> Sample {
        Sample {
            kind,
            bytes: (mbps as f64 * secs as f64 / 8.0 * MB as f64) as u64,
            busy: Duration::from_secs_f32(secs),
            seeders: Some(500),
        }
    }

    fn mbps(lib: &Library, network: Network, kind: Kind, now: u64) -> f32 {
        estimate(lib, network, kind, now).mbps
    }

    /// Research R5 (plans/research/buffer-and-bitrate-decisions.md): the
    /// harmonic mean of the last 5 samples. The first sample replaces the
    /// prior; 40, 25, 60, 35, 20 give 31.2 (the EWMA gave 33.3); 40, 40,
    /// 40, 40, 8 give 22.2 (the EWMA gave 30.4); a sixth drops the oldest.
    #[test]
    fn the_first_sample_replaces_the_prior_then_the_last_five_make_a_harmonic_mean() {
        let lib = Library::temporary().unwrap();
        let wifi = Network::Wifi;
        assert_eq!(estimate(&lib, wifi, Kind::Http, 0), prior(Kind::Http));
        assert!(record(&lib, wifi, &sample(Kind::Http, 100, 2.0), 50, 0));
        let one = estimate(&lib, wifi, Kind::Http, 0);
        assert!(
            (one.mbps - 100.0).abs() < 0.5 && one.samples == 1,
            "{one:?}"
        );
        let run = |network: Network, samples: &[u64]| {
            for mbps in samples {
                assert!(record(
                    &lib,
                    network,
                    &sample(Kind::Http, *mbps, 10.0),
                    50,
                    0
                ));
            }
            estimate(&lib, network, Kind::Http, 0)
        };
        let spread = run(Network::Ethernet, &[40, 25, 60, 35, 20]);
        assert!((spread.mbps - 31.2).abs() < 0.05, "{spread:?}");
        assert_eq!(spread.samples, 5);
        let dip = run(Network::Other, &[40, 40, 40, 40, 8]);
        assert!((dip.mbps - 22.2).abs() < 0.05, "{dip:?}");
        // A sixth sample: 40, 40, 40, 8, 40.
        let next = run(Network::Other, &[40]);
        assert!(
            (next.mbps - 22.2).abs() < 0.05 && next.samples == 5,
            "{next:?}"
        );
        let more = run(Network::Other, &[40]);
        assert!((more.mbps - 22.2).abs() < 0.05, "{more:?}");
        let gone = run(Network::Other, &[40, 40, 40]);
        assert!((gone.mbps - 40.0).abs() < 0.05, "the 8 left: {gone:?}");
        // Kinds and networks keep their own.
        assert_eq!(mbps(&lib, wifi, Kind::P2p, 0), 10.0);
        assert_eq!(mbps(&lib, Network::Cellular, Kind::Http, 0), 15.0);
    }

    /// An estimate stored before the harmonic mean reads as one sample.
    #[test]
    fn an_estimate_from_before_reads_as_one_sample() {
        let lib = Library::temporary().unwrap();
        let net = Network::Wifi;
        let old = r#"{"mbps":30.0,"n":7,"at":0}"#;
        lib.set_device_value(&key(net, Kind::Http), old).unwrap();
        let read = estimate(&lib, net, Kind::Http, 0);
        assert!(
            (read.mbps - 30.0).abs() < 0.05 && read.samples == 1,
            "{read:?}"
        );
        assert!(record(&lib, net, &sample(Kind::Http, 60, 2.0), 50, 0));
        let two = estimate(&lib, net, Kind::Http, 0);
        assert!(
            (two.mbps - 40.0).abs() < 0.05 && two.samples == 2,
            "{two:?}"
        );
    }

    #[test]
    fn a_fast_line_with_a_small_file_still_samples() {
        let lib = Library::temporary().unwrap();
        // 4 MiB read ahead at 100 Mbit/s is 0.34 s in flight.
        let fast = Sample {
            kind: Kind::Http,
            bytes: MIN_BYTES,
            busy: Duration::from_millis(340),
            seeders: None,
        };
        assert!(record(&lib, Network::Wifi, &fast, 50, 0));
        let short = Sample {
            busy: Duration::from_millis(200),
            ..fast
        };
        assert!(!record(&lib, Network::Wifi, &short, 50, 0), "under 0.25 s");
        let small = Sample {
            bytes: MIN_BYTES - 1,
            ..fast
        };
        assert!(!record(&lib, Network::Wifi, &small, 50, 0), "under 4 MiB");
    }

    /// Research R2: a thin swarm measures itself, not the line. Its samples
    /// (under the wanted seeders, or with none listed) go to the
    /// small-swarm estimate, whose prior is 1.4 Mbit/s; the line's stays.
    #[test]
    fn a_thin_swarm_measures_itself_not_the_line() {
        let lib = Library::temporary().unwrap();
        let net = Network::Ethernet;
        assert_eq!(estimate(&lib, net, Kind::P2pThin, 0).mbps, 1.4);
        for _ in 0..2 {
            assert!(record(&lib, net, &sample(Kind::P2p, 40, 3.0), 50, 0));
        }
        let thin = Sample {
            seeders: Some(12),
            ..sample(Kind::P2p, 2, 20.0)
        };
        assert!(record(&lib, net, &thin, 50, 0));
        let unknown = Sample {
            seeders: None,
            ..sample(Kind::P2p, 3, 20.0)
        };
        assert!(record(&lib, net, &unknown, 50, 0));
        assert!((mbps(&lib, net, Kind::P2p, 0) - 40.0).abs() < 0.5);
        let small = estimate(&lib, net, Kind::P2pThin, 0);
        assert!(
            (small.mbps - 2.4).abs() < 0.05 && small.samples == 2,
            "{small:?}"
        );
    }

    /// Code review 2026-10-20 P2-5: a stream key can be a URL with an
    /// account's API key; the device keeps only its digest, and lists from
    /// before are digested when the core opens.
    #[test]
    fn version_lists_keep_digests_of_stream_keys() {
        let lib = Library::temporary().unwrap();
        let key = "url:https://torrentio.example/resolve/realdebrid/SECRETKEY/aaaa/0/x.mkv";
        record_did_not_play(&lib, key, 1_000);
        record_no_picture(&lib, key, 1_000);
        for store_key in [DID_NOT_PLAY_KEY, NO_PICTURE_KEY] {
            let stored = lib.device_value(store_key).unwrap().unwrap();
            assert!(!stored.contains("SECRETKEY"), "{stored}");
        }
        assert_eq!(did_not_play(&lib, 2_000), [key_digest(key)]);
        assert_eq!(no_picture(&lib), [key_digest(key)]);

        let old = Library::temporary().unwrap();
        let raw = serde_json::json!({ key: 1_000 }).to_string();
        old.set_device_value(NO_PICTURE_KEY, &raw).unwrap();
        old.set_device_value(DID_NOT_PLAY_KEY, &raw).unwrap();
        digest_version_lists(&old);
        assert_eq!(no_picture(&old), [key_digest(key)]);
        assert_eq!(did_not_play(&old, 2_000), [key_digest(key)]);
        let stored = old.device_value(NO_PICTURE_KEY).unwrap().unwrap();
        assert!(!stored.contains("SECRETKEY"), "{stored}");
    }

    /// Code review 2026-10-20 r3 P3-3: a wrong file ages out like a decode
    /// failure, after 30 days.
    #[test]
    fn a_wrong_file_ages_out_in_30_days() {
        let lib = Library::temporary().unwrap();
        record_wrong_file(&lib, "bt:cccc:auto", (Some(1), Some(5)), 1_000);
        assert_eq!(wrong_file(&lib, 2_000).len(), 1);
        assert!(wrong_file(&lib, 1_000 + DID_NOT_PLAY_FOR_MS).is_empty());
    }

    #[test]
    fn a_stalled_swarm_stays_demoted_for_a_day() {
        let lib = Library::temporary().unwrap();
        record_dead(&lib, "AAAA", 1_000);
        record_dead(&lib, "bbbb", 2_000);
        assert_eq!(dead_swarms(&lib, 2_000), ["aaaa", "bbbb"]);
        assert_eq!(dead_swarms(&lib, 1_000 + DEAD_FOR_MS), ["bbbb"]);
        assert!(dead_swarms(&lib, 2_000 + DEAD_FOR_MS).is_empty());
    }

    #[test]
    fn an_old_estimate_expires_and_measure_again_resets() {
        let lib = Library::temporary().unwrap();
        let net = Network::Wifi;
        for _ in 0..2 {
            assert!(record(&lib, net, &sample(Kind::Http, 90, 2.0), 50, 0));
        }
        assert!((mbps(&lib, net, Kind::Http, EXPIRES_MS) - 90.0).abs() < 0.5);
        assert_eq!(mbps(&lib, net, Kind::Http, EXPIRES_MS + 1), 15.0);
        // An expired estimate starts over from the new sample.
        assert!(record(
            &lib,
            net,
            &sample(Kind::Http, 30, 2.0),
            50,
            EXPIRES_MS + 1
        ));
        assert!((mbps(&lib, net, Kind::Http, EXPIRES_MS + 1) - 30.0).abs() < 0.5);

        for _ in 0..2 {
            assert!(record(&lib, net, &sample(Kind::P2p, 40, 3.0), 50, 0));
        }
        let thin = Sample {
            seeders: Some(3),
            ..sample(Kind::P2p, 2, 20.0)
        };
        assert!(record(&lib, net, &thin, 50, 0));
        reset(&lib, net);
        assert_eq!(estimate(&lib, net, Kind::P2p, 0), prior(Kind::P2p));
        assert_eq!(estimate(&lib, net, Kind::Http, 0), prior(Kind::Http));
        assert_eq!(estimate(&lib, net, Kind::P2pThin, 0), prior(Kind::P2pThin));
    }
}
