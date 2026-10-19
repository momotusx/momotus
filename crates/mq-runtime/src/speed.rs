//! The connection's speed as the core measures it from playbacks
//! (plans/m13-stream-choice.md 5): one estimate per network type and kind of
//! source, kept on this device only (`device_state`).
//!
//! A sample is capacity: bytes received while at least one upstream request
//! was in flight, over that time, in a playback's first 30 s. Reader wait
//! time is never used, so a small file does not lock the estimate low.

use mq_library::Library;
use mq_streams::{Conditions, Speed};
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
}

/// Bytes and time in flight a sample needs at least (r3 P2 1: a fast line
/// reading 4 MiB ahead is in flight well under 2 s).
pub const MIN_BYTES: u64 = 4 << 20;
pub const MIN_BUSY: Duration = Duration::from_millis(250);
/// Weight of a new sample once two samples set the estimate.
const ALPHA: f32 = 0.3;
/// A last sample older than this returns the estimate to its prior.
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Stored {
    mbps: f32,
    n: u32,
    at: u64,
}

fn key(network: Network, kind: Kind) -> String {
    let kind = match kind {
        Kind::Http => "http",
        Kind::P2p => "p2p",
    };
    format!("speed.{}.{kind}", network.word())
}

fn prior(kind: Kind) -> Speed {
    let c = Conditions::default();
    match kind {
        Kind::Http => c.http,
        Kind::P2p => c.p2p,
    }
}

fn stored(lib: &Library, network: Network, kind: Kind, now_ms: u64) -> Option<Stored> {
    let value = lib.device_value(&key(network, kind)).ok()??;
    serde_json::from_str::<Stored>(&value)
        .ok()
        .filter(|s| now_ms.saturating_sub(s.at) <= EXPIRES_MS)
}

/// The estimate for `kind` on `network`: what the samples measured, from
/// the first one on (a measured line beats a guess; the second sample
/// averages with it and later ones move it by 30%), else the prior, also
/// when the last sample is older than 30 days.
pub fn estimate(lib: &Library, network: Network, kind: Kind, now_ms: u64) -> Speed {
    match stored(lib, network, kind, now_ms) {
        Some(s) if s.n >= 1 => Speed {
            mbps: s.mbps,
            samples: s.n,
        },
        _ => prior(kind),
    }
}

/// Counts `sample` into its estimate; false when it does not count: under
/// 4 MiB or 0.25 s in flight, or a torrent listed with fewer than
/// `min_seeders` (a thin swarm measures itself, not the line). The first
/// two samples average, then each moves the estimate by 30%.
pub fn record(
    lib: &Library,
    network: Network,
    sample: &Sample,
    min_seeders: u32,
    now_ms: u64,
) -> bool {
    let thin = sample.kind == Kind::P2p && sample.seeders.is_none_or(|n| n < min_seeders);
    if sample.bytes < MIN_BYTES || sample.busy < MIN_BUSY || thin {
        return false;
    }
    let mbps = sample.bytes as f32 * 8.0 / sample.busy.as_secs_f32() / 1e6;
    let next = match stored(lib, network, sample.kind, now_ms) {
        None => Stored {
            mbps,
            n: 1,
            at: now_ms,
        },
        Some(s) if s.n < 2 => Stored {
            mbps: (s.mbps * s.n as f32 + mbps) / (s.n + 1) as f32,
            n: s.n + 1,
            at: now_ms,
        },
        Some(s) => Stored {
            mbps: ALPHA * mbps + (1.0 - ALPHA) * s.mbps,
            n: s.n.saturating_add(1),
            at: now_ms,
        },
    };
    let value = serde_json::to_string(&next).unwrap_or_default();
    lib.set_device_value(&key(network, sample.kind), &value)
        .is_ok()
}

/// "Measure again": both estimates of `network` go back to their priors.
pub fn reset(lib: &Library, network: Network) {
    for kind in [Kind::Http, Kind::P2p] {
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
/// Stalled swarms kept at most (the oldest go first).
const DEAD_KEPT: usize = 200;
const DEAD_KEY: &str = "dead_swarms";

fn dead_map(lib: &Library) -> std::collections::BTreeMap<String, u64> {
    lib.device_value(DEAD_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}

/// Info hashes (lowercase) whose swarm stalled on this device in the last
/// day: the ranking does not start them first (seeders an addon lists
/// are a hint).
pub fn dead_swarms(lib: &Library, now_ms: u64) -> Vec<String> {
    dead_map(lib)
        .into_iter()
        .filter(|(_, at)| now_ms.saturating_sub(*at) < DEAD_FOR_MS)
        .map(|(hash, _)| hash)
        .collect()
}

/// The swarm of `info_hash` sent almost nothing after its metadata.
pub fn record_dead(lib: &Library, info_hash: &str, now_ms: u64) {
    let mut dead = dead_map(lib);
    dead.retain(|_, at| now_ms.saturating_sub(*at) < DEAD_FOR_MS);
    dead.insert(info_hash.to_ascii_lowercase(), now_ms);
    while dead.len() > DEAD_KEPT {
        let Some(oldest) = dead
            .iter()
            .min_by_key(|(_, at)| **at)
            .map(|(h, _)| h.clone())
        else {
            break;
        };
        dead.remove(&oldest);
    }
    let value = serde_json::to_string(&dead).unwrap_or_default();
    let _ = lib.set_device_value(DEAD_KEY, &value);
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

    #[test]
    fn the_first_sample_replaces_the_prior_then_two_average_then_move_by_30_percent() {
        let lib = Library::temporary().unwrap();
        let wifi = Network::Wifi;
        assert_eq!(estimate(&lib, wifi, Kind::Http, 0), prior(Kind::Http));
        assert!(record(&lib, wifi, &sample(Kind::Http, 100, 2.0), 50, 0));
        let one = estimate(&lib, wifi, Kind::Http, 0);
        assert!(
            (one.mbps - 100.0).abs() < 0.5 && one.samples == 1,
            "{one:?}"
        );
        assert!(record(&lib, wifi, &sample(Kind::Http, 60, 2.0), 50, 0));
        let two = estimate(&lib, wifi, Kind::Http, 0);
        assert!((two.mbps - 80.0).abs() < 0.5, "{two:?}");
        assert_eq!(two.samples, 2);
        assert!(record(&lib, wifi, &sample(Kind::Http, 180, 2.0), 50, 0));
        let three = mbps(&lib, wifi, Kind::Http, 0);
        assert!((three - 110.0).abs() < 0.5, "0.3 x 180 + 0.7 x 80: {three}");
        // Kinds and networks keep their own.
        assert_eq!(mbps(&lib, wifi, Kind::P2p, 0), 10.0);
        assert_eq!(mbps(&lib, Network::Cellular, Kind::Http, 0), 15.0);
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

    #[test]
    fn a_thin_swarm_measures_itself_not_the_line() {
        let lib = Library::temporary().unwrap();
        let net = Network::Ethernet;
        for _ in 0..2 {
            assert!(record(&lib, net, &sample(Kind::P2p, 40, 3.0), 50, 0));
        }
        let thin = Sample {
            seeders: Some(12),
            ..sample(Kind::P2p, 2, 20.0)
        };
        assert!(!record(&lib, net, &thin, 50, 0));
        let unknown = Sample {
            seeders: None,
            ..thin
        };
        assert!(!record(&lib, net, &unknown, 50, 0));
        assert!((mbps(&lib, net, Kind::P2p, 0) - 40.0).abs() < 0.5);
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
        reset(&lib, net);
        assert_eq!(estimate(&lib, net, Kind::P2p, 0), prior(Kind::P2p));
        assert_eq!(estimate(&lib, net, Kind::Http, 0), prior(Kind::Http));
    }
}
