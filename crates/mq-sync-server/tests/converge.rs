//! Three devices make random edits, rotate keys, lose stored records and
//! sync in random order through a real relay that may be restored from a
//! snapshot; after two full rounds every device holds the same registers.

use mq_http::{ClientConfig, HttpClient};
use mq_library::hlc::{HlcClock, now_ms};
use mq_library::{AddonRow, Library, LibraryItemRow, ProgressRow, SyncCursor, coll};
use mq_sync::wire::MAX_CLOCK_AHEAD_MS;
use mq_sync::{SyncConfig, SyncReport, pull_changes, push_changes, sync_once};
use proptest::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

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
    /// Another app's history imported on this device: device 0, stamped
    /// with when it happened there.
    Import {
        device: usize,
        video: u8,
        position: i64,
        age_s: u64,
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
    /// A plain key rotation (no one dropped).
    Rotate {
        device: usize,
    },
    /// A sync round whose changed record the app never stores.
    DropRecord {
        device: usize,
    },
    /// Reads the roster only (the devices screen).
    CatchUp {
        device: usize,
    },
    /// The operator backs up the relay database.
    Snapshot,
    /// The operator restores the last backup (the roster journal survives)
    /// and starts the relay with a new generation.
    Restore,
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
        (device.clone(), 0..2u8, 0..1000i64, 0..7200u64).prop_map(
            |(device, video, position, age_s)| Op::Import {
                device,
                video,
                position,
                age_s,
            }
        ),
        device.clone().prop_map(|device| Op::Push { device }),
        device.clone().prop_map(|device| Op::Pull { device }),
        device.clone().prop_map(|device| Op::Rotate { device }),
        device.clone().prop_map(|device| Op::DropRecord { device }),
        device.prop_map(|device| Op::CatchUp { device }),
        Just(Op::Snapshot),
        Just(Op::Restore),
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
            _ => unreachable!("handled by the runner"),
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

/// A relay on a database in its own directory, with the roster journal
/// next to it.
struct Relay {
    url: String,
    db: PathBuf,
    offset: Arc<AtomicI64>,
}

impl Relay {
    async fn start(db: PathBuf, offset: Arc<AtomicI64>, new_generation: bool) -> Self {
        let app = mq_sync_server::router(
            &db,
            mq_sync_server::Config {
                clock_offset_ms: offset.clone(),
                new_generation,
                ..Default::default()
            },
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        Self { url, db, offset }
    }

    async fn new(offset: Arc<AtomicI64>) -> Self {
        Self::start(fresh_dir().join("relay.sqlite"), offset, false).await
    }

    fn snapshot(&self) -> PathBuf {
        let path = self.db.with_file_name("snapshot.sqlite");
        let _ = std::fs::remove_file(&path);
        rusqlite::Connection::open(&self.db)
            .unwrap()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        path
    }

    /// A relay started from `snapshot` with this relay's journal: what an
    /// operator gets restoring the database alone.
    async fn restored(&self, snapshot: &PathBuf) -> Self {
        let db = fresh_dir().join("relay.sqlite");
        std::fs::copy(snapshot, &db).unwrap();
        std::fs::copy(
            self.db.with_extension("roster.jsonl"),
            db.with_extension("roster.jsonl"),
        )
        .unwrap();
        Self::start(db, self.offset.clone(), true).await
    }
}

fn fresh_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mq-converge-{}", rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Stores a round's changed record, as the app does (unless it drops it).
fn keep(config: &mut SyncConfig, report: SyncReport) {
    if let Some(record) = report.record {
        *config = record;
    }
}

/// Registers so far written by one device only (no imports), with its last
/// edit; `None` once another device or an import touched it.
type Single = HashMap<(u8, String), Option<(usize, Op)>>;

fn note(single: &mut Single, register: (u8, String), edit: Option<(usize, &Op)>) {
    let next = match (single.get(&register), edit) {
        (Some(None), _) | (_, None) => None,
        (Some(Some((other, _))), Some((device, _))) if *other != device => None,
        (_, Some((device, op))) => Some((device, op.clone())),
    };
    single.insert(register, next);
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

/// `slow`: the relay's clock is 30 minutes behind every device until the
/// final rounds.
async fn run(ops: Vec<Op>, slow: bool) {
    let offset = Arc::new(AtomicI64::new(if slow { -30 * 60 * 1000 } else { 0 }));
    let mut relay = Relay::new(offset.clone()).await;
    let mut snapshot: Option<PathBuf> = None;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let mut configs: Vec<SyncConfig> = Vec::new();
    let mut devices = Vec::new();
    // One device runs two hours slow; the fast one stays inside the relay's
    // 10 minutes (restamping is covered in tests/engine.rs).
    for (i, skew) in [0i64, -7_200_000, 540_000].into_iter().enumerate() {
        let library = Library::temporary().unwrap();
        let clock = HlcClock::new(i as u64 + 1);
        clock.set_offset_ms(skew);
        library
            .ensure_profile(PROFILE, PROFILE, &clock.tick(now_ms()).encode())
            .unwrap();
        // The first device makes the account; the others join it by QR,
        // each as its own member.
        let record = if i == 0 {
            mq_sync::create_account(&http, &relay.url, None, "first")
                .await
                .unwrap()
        } else {
            let first: &Device = &devices[0];
            let joining = mq_sync::pair::start_pairing(&http, &relay.url)
                .await
                .unwrap();
            let (_, record) = mq_sync::pair::approve_pairing(
                &first.library,
                &http,
                &configs[0],
                &first.clock,
                &joining.qr,
            )
            .await
            .unwrap();
            configs[0] = record;
            let offer = joining.poll(&http).await.unwrap().expect("approved");
            joining.claim(&http, offer, "next").await.unwrap()
        };
        configs.push(record);
        devices.push(Device {
            library,
            clock,
            skew_ms: skew,
        });
    }
    // The newest write of every register, across all devices: the oracle.
    let mut newest: HashMap<(u8, String), String> = HashMap::new();
    let mut single = Single::new();
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
                let report = pull_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                keep(&mut configs[*device], report);
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
                note(
                    &mut single,
                    (coll::PROGRESS, key.clone()),
                    Some((*device, op)),
                );
                let slot = newest.entry((coll::PROGRESS, key)).or_default();
                if stored.hlc > *slot {
                    *slot = stored.hlc;
                }
            }
            Op::Import {
                device,
                video,
                position,
                age_s,
            } => {
                let hlc = mq_library::Hlc {
                    wall_ms: now_ms() - age_s * 1000,
                    counter: 0,
                    device: 0,
                }
                .encode();
                let video_id = format!("tt1:1:{video}");
                let row = ProgressRow {
                    video_id: &video_id,
                    item_id: "tt1",
                    position_ms: *position,
                    duration_ms: 1000,
                    updated_ms: *position,
                };
                let d = &devices[*device];
                d.library.set_progress(PROFILE, row, &hlc).unwrap();
                // An import never overrides a newer edit, so only the newest
                // version is checked.
                let key = [PROFILE, &video_id].join("\u{1f}");
                let stored = d.library.register(coll::PROGRESS, &key).unwrap().unwrap();
                note(&mut single, (coll::PROGRESS, key.clone()), None);
                let slot = newest.entry((coll::PROGRESS, key)).or_default();
                if stored.hlc > *slot {
                    *slot = stored.hlc;
                }
            }
            Op::Sync { device } => {
                let d = &devices[*device];
                let report = sync_once(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                keep(&mut configs[*device], report);
            }
            Op::DropRecord { device } => {
                let d = &devices[*device];
                sync_once(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
            }
            Op::Push { device } => {
                let d = &devices[*device];
                let report = push_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                keep(&mut configs[*device], report);
            }
            Op::Pull { device } => {
                let d = &devices[*device];
                let report = pull_changes(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                keep(&mut configs[*device], report);
            }
            Op::Rotate { device } => {
                let d = &devices[*device];
                configs[*device] =
                    mq_sync::remove_devices(&d.library, &http, &configs[*device], &d.clock, vec![])
                        .await
                        .unwrap();
            }
            Op::CatchUp { device } => {
                let d = &devices[*device];
                let seen = mq_sync::devices(&d.library, &http, &configs[*device], &d.clock)
                    .await
                    .unwrap();
                assert_eq!(seen.members.len(), 3, "{seen:?}");
                if let Some(record) = seen.record {
                    configs[*device] = record;
                }
            }
            Op::Snapshot => snapshot = Some(relay.snapshot()),
            Op::Restore => {
                if let Some(snapshot) = &snapshot {
                    relay = relay.restored(snapshot).await;
                    for config in &mut configs {
                        config.server = relay.url.clone();
                    }
                }
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
                note(&mut single, register.clone(), Some((*device, op)));
                let slot = newest.entry(register).or_default();
                if stored.hlc > *slot {
                    *slot = stored.hlc;
                }
            }
        }
    }
    // The operator fixes the relay's clock, so what waited goes through.
    offset.store(0, Ordering::Relaxed);
    for _ in 0..2 {
        for (d, config) in devices.iter().zip(configs.iter_mut()) {
            // A round that changed the record publishes nothing: store it
            // and go again, as the app does.
            for _ in 0..3 {
                let report = sync_once(&d.library, &http, config, &d.clock)
                    .await
                    .unwrap();
                let again = report.again;
                keep(config, report);
                if !again {
                    break;
                }
            }
        }
    }
    let keys = &configs[0].keys;
    assert!(
        configs.iter().all(|c| c.keys == *keys),
        "devices hold different keys after {ops:?}"
    );
    let first = registers(&devices[0].library);
    for d in &devices[1..] {
        assert_eq!(
            registers(&d.library),
            first,
            "devices diverged after {ops:?}"
        );
    }
    // Equal is not enough: every register must hold its newest write. (A
    // relay with a slow clock restamps fresh edits at its own time, so there
    // last-writer-wins falls back to push order; equality still holds.)
    let synced: Vec<_> = first.iter().filter(|r| r.coll != coll::PROFILES).collect();
    for r in synced.iter().filter(|_| !slow) {
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
    // A register one device wrote alone ends with its last edit, also under
    // a slow relay: restamps keep each device's own order.
    for ((coll, key), edit) in &single {
        let Some((_, op)) = edit else { continue };
        let r = synced
            .iter()
            .find(|r| r.coll == *coll && r.key == *key)
            .expect("written registers are synced");
        assert!(
            written(op, &r.value),
            "lost update in {coll}:{key} after {ops:?}"
        );
    }
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
    fn three_devices_converge(ops in proptest::collection::vec(op(), 1..40), slow in any::<bool>()) {
        tokio::runtime::Runtime::new().unwrap().block_on(run(ops, slow));
    }
}

/// A device that pulls an older version of a register it changed but has
/// not pushed yet must keep its own change.
#[tokio::test]
async fn pulling_an_older_version_keeps_the_unpushed_newer_write() {
    run(
        vec![
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
        ],
        false,
    )
    .await;
}
