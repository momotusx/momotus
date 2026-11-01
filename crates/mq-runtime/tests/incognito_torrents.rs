//! Torrents Incognito created (plans/batch-10-incognito.md 2.5 and 7.1):
//! recorded before their add, removed at the end unless a stream still
//! claims them (then on a later pass, or at the next launch), never a
//! cached one and never one a download names. A real engine over the
//! device's own torrent folder, on loopback.

#[path = "../../mq-engine/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use mq_engine::{Engine, EngineConfig, TorrentHandle};
use mq_library::{DownloadRecord, OWNER};
use mq_runtime::incognito::Record;
use mq_runtime::{Core, CoreConfig};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

fn open(dir: &Path) -> Core {
    let core = Core::open(CoreConfig {
        data_dir: dir.to_path_buf(),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap();
    let row = mq_library::AddonRow {
        transport_url: "http://127.0.0.1:9/manifest.json".into(),
        manifest_json: r#"{"id":"x","version":"1.0.0","name":"X","resources":[],"types":[]}"#
            .into(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(OWNER, &[row], core.clock())
        .unwrap();
    core
}

/// The device's engine over `<data_dir>/torrents`, on loopback.
async fn engine(core: &Core) -> Engine {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = EngineConfig {
        discovery: false,
        dwh: true,
        upload: false,
        ..EngineConfig::new(
            core.torrent_dir(),
            http.torrent_scope(&[Ipv4Addr::LOCALHOST.into()]),
        )
    };
    let engine = Engine::start(config).await.unwrap();
    core.set_torrent_engine(engine.clone());
    engine
}

fn lines(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("incognito").join("torrents")).unwrap_or_default()
}

/// A torrent's folder, `.have` and `.torrent`, as the engine leaves them.
fn leave_files(core: &Core, hash: &str) -> PathBuf {
    let folder = core.torrent_dir().join(hash);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("movie.mkv"), "bytes").unwrap();
    std::fs::write(folder.with_extension("have"), "x").unwrap();
    std::fs::write(folder.with_extension("torrent"), "x").unwrap();
    folder
}

fn files_of(core: &Core, hash: &str) -> [bool; 3] {
    let folder = core.torrent_dir().join(hash);
    [
        folder.exists(),
        folder.with_extension("have").exists(),
        folder.with_extension("torrent").exists(),
    ]
}

/// The swarm's torrent added from inside, its metadata from the seeder,
/// noted for the end as a start's own add notes it.
async fn add_inside(core: &Core, engine: &Engine, s: &support::Swarm) -> TorrentHandle {
    let hash = s.info_hash.to_ascii_lowercase();
    assert_eq!(core.record_torrent(&hash), Record::Recorded);
    let h = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    core.incognito_handles().insert(hash, h);
    h
}

/// A torrent this device had before is never Incognito's; outside
/// Incognito nothing is recorded; after the end nothing is (the late
/// look-ahead) and nothing is added.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_new_torrents_inside_a_live_scope_are_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let host = open(dir.path());
    let core = host.open_incognito().unwrap();
    let (cached, new, late) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
    leave_files(&host, &cached);
    assert_eq!(core.record_torrent(&cached), Record::Not);
    assert_eq!(host.record_torrent(&new), Record::Not);
    assert_eq!(core.record_torrent(&new), Record::Recorded);
    assert!(lines(dir.path()).contains(&new));
    let before = lines(dir.path());
    core.end_incognito();
    assert_eq!(core.record_torrent(&late), Record::Ended);
    assert!(!lines(dir.path()).contains(&late));
    assert!(before.contains(&new));
}

/// Leave while a stream still claims the torrent: the line stays for the
/// next pass, which removes the torrent once the claim is gone. A download
/// that names a torrent keeps its files; only its line goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_claimed_torrent_goes_on_a_later_pass() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let host = open(dir.path());
    let engine = engine(&host).await;
    let core = host.open_incognito().unwrap();
    let h = add_inside(&core, &engine, &s).await;
    let hash = s.info_hash.to_ascii_lowercase();
    leave_files(&host, &hash);
    let kept = "d".repeat(40);
    assert_eq!(core.record_torrent(&kept), Record::Recorded);
    leave_files(&host, &kept);
    host.library().add_download(&download(&kept)).unwrap();
    let claim = engine.claim(h, "incognito").unwrap();
    core.end_incognito();
    // The end's own pass: the claimed one stays, the download's line goes.
    while lines(dir.path()).contains(&kept) {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(files_of(&host, &kept), [true; 3]);
    assert!(lines(dir.path()).contains(&hash));
    assert_eq!(host.sweep_torrents().await, 1);
    drop(claim);
    assert_eq!(host.sweep_torrents().await, 0);
    assert_eq!(lines(dir.path()), "");
    assert_eq!(files_of(&host, &hash), [false; 3]);
    assert!(engine.stats(h).await.is_err(), "the engine let it go");
}

/// Two scopes: ending one removes only its own torrent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ending_one_scope_keeps_the_others_torrents() {
    let dir = tempfile::tempdir().unwrap();
    let host = open(dir.path());
    let (a, b) = (
        host.open_incognito().unwrap(),
        host.open_incognito().unwrap(),
    );
    let (mine, theirs) = ("e".repeat(40), "f".repeat(40));
    assert_eq!(a.record_torrent(&mine), Record::Recorded);
    assert_eq!(b.record_torrent(&theirs), Record::Recorded);
    leave_files(&host, &mine);
    leave_files(&host, &theirs);
    a.end_incognito();
    assert_eq!(host.sweep_torrents().await, 0);
    assert_eq!(files_of(&host, &mine), [false; 3]);
    assert_eq!(files_of(&host, &theirs), [true; 3]);
    assert!(lines(dir.path()).contains(&theirs));
    b.end_incognito();
}

/// A look-ahead's own cleanup lets the engine forget a torrent with its
/// folder kept; the end then finds it unknown and deletes the files as
/// recovery does (A r4 P3-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forgotten_look_ahead_torrent_leaves_no_folder() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let host = open(dir.path());
    let engine = engine(&host).await;
    let core = host.open_incognito().unwrap();
    let h = add_inside(&core, &engine, &s).await;
    let hash = s.info_hash.to_ascii_lowercase();
    engine.remove(h, true).await.unwrap();
    leave_files(&host, &hash);
    core.end_incognito();
    assert_eq!(host.sweep_torrents().await, 0);
    assert_eq!(files_of(&host, &hash), [false; 3]);
}

/// Leave from the player with the stream's claim held, then the process
/// dies before any later pass: the next launch removes the files and the
/// line, and a download's torrent in the same list keeps its files (A r3
/// P1-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kill_after_leave_is_cleared_at_the_next_launch() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let hash = s.info_hash.to_ascii_lowercase();
    let kept = "d".repeat(40);
    {
        let host = open(dir.path());
        let engine = engine(&host).await;
        let core = host.open_incognito().unwrap();
        let h = add_inside(&core, &engine, &s).await;
        assert_eq!(core.record_torrent(&kept), Record::Recorded);
        leave_files(&host, &kept);
        host.library().add_download(&download(&kept)).unwrap();
        leave_files(&host, &hash);
        let claim = engine.claim(h, "incognito").unwrap();
        core.end_incognito();
        assert!(lines(dir.path()).contains(&hash));
        // The kill: the claim never let go, no pass after this.
        std::mem::forget(claim);
        drop((engine, core, host));
    }
    // The end's own pass holds the device while it runs; a kill would not
    // wait for it, the test does, so the next open is a new launch.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let host = Core::open(CoreConfig {
        data_dir: dir.path().to_path_buf(),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap();
    assert_eq!(files_of(&host, &hash), [false; 3]);
    assert_eq!(files_of(&host, &kept), [true; 3]);
    assert_eq!(lines(dir.path()), "");
}

fn download(hash: &str) -> DownloadRecord {
    let version = serde_json::json!({"key": "k", "chain": [],
        "torrent": {"hash": hash, "file": 0, "name": "movie.mkv"}});
    DownloadRecord {
        id: format!("d-{hash}"),
        added_by: OWNER.into(),
        kind: "movie".into(),
        item_id: "tt1".into(),
        video_id: "tt1".into(),
        season: None,
        episode: None,
        facts: "{}".into(),
        request: "{}".into(),
        version: Some(version.to_string()),
        state: "done".into(),
        reason: None,
        mobile_data: false,
        hand_picked: true,
        bytes_done: 0,
        bytes_total: None,
        moves: 0,
        subtitles: "[]".into(),
        subtitles_state: "done".into(),
        position: 0,
        created_ms: 1,
        done_ms: Some(1),
        smart: None,
    }
}
