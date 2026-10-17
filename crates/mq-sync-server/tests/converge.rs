//! Three devices make random edits and sync in random order through a real
//! relay; after two full rounds every device holds the same registers.

use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::hlc::{HlcClock, now_ms};
use mq_library::{AddonRow, Library, LibraryItemRow, ProgressRow, SyncCursor, coll};
use mq_sync::wire::{AccountResponse, MAX_CLOCK_AHEAD_MS};
use mq_sync::{SyncConfig, pull_changes, push_changes, sync_once};
use proptest::prelude::*;
use std::collections::HashMap;

const PROFILE: &str = "default";
const URLS: [&str; 3] = [
    "https://a.example/manifest.json",
    "https://b.example/manifest.json",
    "https://c.example/manifest.json",
];

#[derive(Debug, Clone)]
enum Op {
    Progress {
        device: usize,
        video: u8,
        position: i64,
    },
    Watched {
        device: usize,
        video: u8,
        watched: bool,
    },
    InLibrary {
        device: usize,
        item: u8,
        added: bool,
    },
    Setting {
        device: usize,
        key: u8,
        value: u8,
    },
    Addons {
        device: usize,
        order: Vec<usize>,
    },
    /// App restart: a fresh clock seeded from the stored registers.
    Restart {
        device: usize,
    },
    /// The device clock jumps (a user fixes the time, NTP corrects it).
    ClockJump {
        device: usize,
        skew_s: i64,
    },
    /// Takes a timestamp, pulls, then writes with it: a pull that lands
    /// between an edit's clock and its write.
    StalePullWrite {
        device: usize,
        video: u8,
        position: i64,
    },
    Memory {
        device: usize,
        item: u8,
        group: u8,
    },
    Sync {
        device: usize,
    },
    Push {
        device: usize,
    },
    Pull {
        device: usize,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let device = 0..3usize;
    prop_oneof![
        (device.clone(), 0..2u8, 0..1000i64).prop_map(|(device, video, position)| Op::Progress {
            device,
            video,
            position
        }),
        (device.clone(), 0..2u8, any::<bool>()).prop_map(|(device, video, watched)| Op::Watched {
            device,
            video,
            watched
        }),
        (device.clone(), 0..2u8, any::<bool>()).prop_map(|(device, item, added)| Op::InLibrary {
            device,
            item,
            added
        }),
        (device.clone(), 0..2u8, any::<u8>()).prop_map(|(device, key, value)| Op::Setting {
            device,
            key,
            value
        }),
        (
            device.clone(),
            proptest::sample::subsequence(vec![0usize, 1, 2], 0..=3).prop_shuffle()
        )
            .prop_map(|(device, order)| Op::Addons { device, order }),
        (device.clone(), 0..2u8, 0..3u8).prop_map(|(device, item, group)| Op::Memory {
            device,
            item,
            group
        }),
        device.clone().prop_map(|device| Op::Sync { device }),
        device.clone().prop_map(|device| Op::Restart { device }),
        (device.clone(), 0..2u8, 0..1000i64).prop_map(|(device, video, position)| {
            Op::StalePullWrite {
                device,
                video,
                position,
            }
        }),
        (device.clone(), -7200..540i64)
            .prop_map(|(device, skew_s)| Op::ClockJump { device, skew_s }),
        device.clone().prop_map(|device| Op::Push { device }),
        device.prop_map(|device| Op::Pull { device }),
    ]
}

struct Device {
    library: Library,
    clock: HlcClock,
    /// Wall clock offset, so devices disagree about the time.
    skew_ms: i64,
}

impl Device {
    fn hlc(&self) -> String {
        self.clock.tick(self.clock.wall_now()).encode()
    }

    /// Applies a local edit and returns the register it wrote and its hlc.
    fn apply(&self, op: &Op) -> ((u8, String), String) {
        let lib = &self.library;
        let hlc = self.hlc();
        let key = |parts: &[&str]| parts.join("\u{1f}");
        match op {
            Op::Progress {
                video, position, ..
            } => {
                let video_id = format!("tt1:1:{video}");
                let row = ProgressRow {
                    video_id: &video_id,
                    item_id: "tt1",
                    position_ms: *position,
                    duration_ms: 1000,
                    updated_ms: *position,
                };
                lib.set_progress(PROFILE, row, &hlc).unwrap();
                ((coll::PROGRESS, key(&[PROFILE, &video_id])), hlc)
            }
            Op::Watched { video, watched, .. } => {
                let video_id = format!("tt1:1:{video}");
                lib.set_watched(PROFILE, "tt1", &video_id, *watched, &hlc)
                    .unwrap();
                ((coll::WATCHED, key(&[PROFILE, &video_id])), hlc)
            }
            Op::InLibrary { item, added, .. } => {
                let row = LibraryItemRow {
                    item_id: format!("tt{item}"),
                    kind: "series".into(),
                    name: format!("Show {item}"),
                    poster: None,
                    poster_shape: None,
                    added_ms: self.skew_ms,
                };
                lib.upsert_item(PROFILE, &row, *added, &hlc).unwrap();
                ((coll::ITEMS, key(&[PROFILE, &row.item_id])), hlc)
            }
            Op::Setting { key: k, value, .. } => {
                let name = format!("k{k}");
                lib.set_setting(PROFILE, &name, &value.to_string(), &hlc)
                    .unwrap();
                ((coll::SETTINGS, key(&[PROFILE, &name])), hlc)
            }
            Op::Addons { order, .. } => {
                let rows: Vec<AddonRow> = order
                    .iter()
                    .map(|&i| AddonRow {
                        transport_url: URLS[i].into(),
                        manifest_json: format!("{{\"n\":{i}}}"),
                        position: 0,
                        enabled: i != 1,
                        allow_private: false,
                    })
                    .collect();
                lib.set_addons(PROFILE, &rows, &hlc).unwrap();
                ((coll::ADDONS, key(&[PROFILE])), hlc)
            }
            Op::Memory { item, group, .. } => {
                let item_id = format!("tt{item}");
                let binge = format!("g{group}");
                lib.set_stream_memory(PROFILE, &item_id, Some(&binge), "{}", &hlc)
                    .unwrap();
                ((coll::STREAM_MEMORY, key(&[PROFILE, &item_id])), hlc)
            }
            Op::Sync { .. }
            | Op::Push { .. }
            | Op::Pull { .. }
            | Op::Restart { .. }
            | Op::ClockJump { .. }
            | Op::StalePullWrite { .. } => {
                unreachable!("handled by the runner")
            }
        }
    }
}

/// Whether a register value holds what `op` wrote.
fn written(op: &Op, value: &serde_json::Value) -> bool {
    match op {
        Op::Progress { position, .. } => value["positionMs"] == *position,
        Op::Watched { watched, .. } => value["watched"] == *watched,
        Op::InLibrary { added, .. } => value["inLibrary"] == *added,
        Op::Setting { value: v, .. } => value["valueJson"].as_str() == Some(v.to_string().as_str()),
        Op::Addons { order, .. } => {
            let urls: Vec<&str> = order.iter().map(|&i| URLS[i]).collect();
            let stored: Vec<&str> = value["addons"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x["url"].as_str()).collect())
                .unwrap_or_default();
            urls == stored
        }
        Op::Memory { group, .. } => value["bingeGroup"] == format!("g{group}"),
        Op::StalePullWrite { position, .. } => value["positionMs"] == *position,
        _ => true,
    }
}

async fn relay() -> String {
    let dir = std::env::temp_dir().join(format!("mq-converge-{}-{}", std::process::id(), now_ms()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(
        &dir.join(format!("{}.sqlite", rand_suffix())),
        Default::default(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn rand_suffix() -> String {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    hex::encode(b)
}

fn registers(lib: &Library) -> Vec<mq_library::SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 1_000_000)
        .unwrap()
}

async fn run(ops: Vec<Op>) {
    let server = relay().await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let keys = mq_sync::seal::KeyBundle::generate();
    let mut configs = Vec::new();
    let mut devices = Vec::new();
    // One device runs two hours slow; the fast one stays inside the relay's
    // 10 minutes (restamping is covered in tests/engine.rs).
    for (i, skew) in [0i64, -7_200_000, 540_000].into_iter().enumerate() {
        // phase 1: each device is its own device token on one account; the
        // account is created by the first and the token reused by the rest.
        if i == 0 {
            let account: AccountResponse = http
                .post_json(
                    &format!("{server}/v1/accounts"),
                    &serde_json::json!({}),
                    &local,
                )
                .await
                .unwrap()
                .json()
                .unwrap();
            configs.push(SyncConfig {
                server: server.clone(),
                device_token: account.device_token,
                keys: keys.clone(),
            });
        } else {
            configs.push(configs[0].clone());
        }
        let library = Library::temporary().unwrap();
        let clock = HlcClock::new(i as u64 + 1);
        clock.set_offset_ms(skew);
        library
            .ensure_profile(PROFILE, PROFILE, &clock.tick(now_ms()).encode())
            .unwrap();
        devices.push(Device {
            library,
            clock,
            skew_ms: skew,
        });
    }
    // The newest write of every register, across all devices: the oracle.
    let mut newest: HashMap<(u8, String), String> = HashMap::new();
    for op in &ops {
        match op {
            Op::Restart { device } => {
                let d = &mut devices[*device];
                d.clock = HlcClock::new(*device as u64 + 1);
                d.clock.set_offset_ms(d.skew_ms);
                if let Some(newest) = d
                    .library
                    .newest_hlc()
                    .unwrap()
                    .as_deref()
                    .and_then(mq_library::Hlc::decode)
                {
                    d.clock.observe(newest, d.clock.wall_now());
                }
            }
            Op::ClockJump { device, skew_s } => {
                devices[*device].skew_ms = skew_s * 1000;
                devices[*device].clock.set_offset_ms(skew_s * 1000);
            }
            Op::StalePullWrite {
                device,
                video,
                position,
            } => {
                let d = &devices[*device];
                let early = d.hlc();
                pull_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                let video_id = format!("tt1:1:{video}");
                let row = ProgressRow {
                    video_id: &video_id,
                    item_id: "tt1",
                    position_ms: *position,
                    duration_ms: 1000,
                    updated_ms: *position,
                };
                d.library.set_progress(PROFILE, row, &early).unwrap();
                let key = [PROFILE, &video_id].join("\u{1f}");
                let stored = d.library.register(coll::PROGRESS, &key).unwrap().unwrap();
                assert!(
                    written(op, &stored.value),
                    "edit lost: {op:?} left {stored:?}"
                );
                let slot = newest.entry((coll::PROGRESS, key)).or_default();
                if stored.hlc > *slot {
                    *slot = stored.hlc;
                }
            }
            Op::Sync { device } => {
                let d = &devices[*device];
                sync_once(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
            }
            Op::Push { device } => {
                let d = &devices[*device];
                push_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
            }
            Op::Pull { device } => {
                let d = &devices[*device];
                pull_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
            }
            Op::Progress { device, .. }
            | Op::Watched { device, .. }
            | Op::InLibrary { device, .. }
            | Op::Setting { device, .. }
            | Op::Addons { device, .. }
            | Op::Memory { device, .. } => {
                let (register, _) = devices[*device].apply(op);
                // Read your own write: a local edit always lands, even over a
                // version pulled from a device whose clock is far ahead.
                let stored = devices[*device]
                    .library
                    .register(register.0, &register.1)
                    .unwrap()
                    .expect("the register exists after writing it");
                assert!(
                    written(op, &stored.value),
                    "edit lost: {op:?} left {stored:?}"
                );
                let slot = newest.entry(register).or_default();
                if stored.hlc > *slot {
                    *slot = stored.hlc;
                }
            }
        }
    }
    for _ in 0..2 {
        for (d, config) in devices.iter().zip(&configs) {
            sync_once(&d.library, &http, config, &d.clock)
                .await
                .unwrap();
        }
    }
    let first = registers(&devices[0].library);
    for d in &devices[1..] {
        assert_eq!(
            registers(&d.library),
            first,
            "devices diverged after {ops:?}"
        );
    }
    // Equal is not enough: every register must hold its newest write.
    let synced: Vec<_> = first.iter().filter(|r| r.coll != coll::PROFILES).collect();
    for r in &synced {
        assert_eq!(
            Some(&r.hlc),
            newest.get(&(r.coll, r.key.clone())),
            "lost update in {}:{} after {ops:?}",
            r.coll,
            r.key
        );
    }
    assert_eq!(
        synced.len(),
        newest.len(),
        "registers missing after {ops:?}"
    );
    // Syncing never drags a device's clock past the relay's 10 minutes.
    let ceiling = now_ms() + MAX_CLOCK_AHEAD_MS + 1000;
    for d in devices.iter().filter(|d| d.skew_ms <= 0) {
        let wall = d.clock.tick(d.clock.wall_now()).wall_ms;
        assert!(wall <= ceiling, "clock {wall} past {ceiling} after {ops:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]
    #[test]
    fn three_devices_converge(ops in proptest::collection::vec(op(), 1..40)) {
        tokio::runtime::Runtime::new().unwrap().block_on(run(ops));
    }
}

/// A device that pulls an older version of a register it changed but has
/// not pushed yet must keep its own change.
#[tokio::test]
async fn pulling_an_older_version_keeps_the_unpushed_newer_write() {
    run(vec![
        Op::Progress {
            device: 0,
            video: 0,
            position: 10,
        },
        Op::Push { device: 0 },
        Op::Progress {
            device: 2,
            video: 0,
            position: 20,
        },
        Op::Pull { device: 2 },
    ])
    .await;
}
