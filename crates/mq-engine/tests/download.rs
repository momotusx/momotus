//! Offline downloads in the engine (plans/offline-downloads.md 4.6, 6.1): a
//! file fetched whole with no reader, kept from every sweep, and deleted
//! without touching its pack's other files.

#![cfg(feature = "test-hooks")]

#[allow(dead_code)]
mod support;

use std::path::Path;
use std::time::Duration;

use mq_engine::{
    DownloadHandle, Engine, EngineConfig, FileHint, RESERVE_BYTES, TorrentHandle, Window,
    WindowPlan, complete_on_disk, test_hooks,
};

const MIB: u64 = 1 << 20;
const PIECE: u32 = 1 << 20;

fn config(dir: &Path, kept: Vec<(String, usize)>, budget: u64) -> EngineConfig {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    EngineConfig {
        discovery: false,
        upload: false,
        cache_budget_bytes: budget,
        kept,
        ..EngineConfig::new(
            dir,
            http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]),
        )
    }
}

async fn engine(dir: &Path, kept: Vec<(String, usize)>, budget: u64) -> Engine {
    let cfg = config(dir, kept, budget);
    for _ in 0..40 {
        if let Ok(engine) = Engine::start(cfg.clone()).await {
            return engine;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Engine::start(cfg).await.unwrap()
}

async fn until<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..600 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never: {what}");
}

/// Downloads `file` of the swarm into `engine` and waits until it is whole.
async fn download(
    engine: &Engine,
    s: &support::Swarm,
    name: &str,
) -> (TorrentHandle, usize, DownloadHandle) {
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let file = engine
        .files(t)
        .unwrap()
        .iter()
        .position(|f| f.path.ends_with(name))
        .unwrap();
    let handle = engine.download(t, file).await.unwrap();
    until("the file is whole", || async {
        let p = handle.progress().await;
        p.have == p.len
    })
    .await;
    (t, file, handle)
}

fn read(dir: &Path, hash: &str, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(hash).join(name)).unwrap()
}

/// With no reader, a download brings the whole file, and once it stops its
/// torrent's bitfield says so on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_whole_file_downloads_with_no_reader() {
    let s = support::swarm_big(12 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), u64::MAX).await;
    let (t, file, handle) = download(&e, &s, "movie.mkv").await;
    assert_eq!(e.stats(t).await.unwrap().readers, 0);
    handle.stop().await;
    assert!(read(root.path(), &s.info_hash, "movie.mkv") == s.movie);
    let dir = root.path().to_owned();
    let hash = s.info_hash.clone();
    until("the bitfield is on disk", || {
        let (dir, hash) = (dir.clone(), hash.clone());
        async move { complete_on_disk(&dir, &hash, file) }
    })
    .await;
}

/// A kept torrent is no cache: a fresh engine with a zero budget sweeps an
/// idle leftover but not the kept folder, and counts neither the kept
/// folder as reclaimable nor against its budget (engine r2 P1-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_file_survives_every_sweep() {
    let s = support::swarm_big(12 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let file = {
        let e = engine(root.path(), Vec::new(), u64::MAX).await;
        let (_, file, handle) = download(&e, &s, "movie.mkv").await;
        handle.stop().await;
        file
    };
    // An idle leftover of 100 KiB beside it.
    let idle = root.path().join("1".repeat(40));
    std::fs::create_dir_all(&idle).unwrap();
    std::fs::write(idle.join("x.mkv"), vec![7u8; 100 << 10]).unwrap();
    let kept = vec![(s.info_hash.clone(), file)];
    // A 1 MiB budget: the kept 12 MiB would be over it, the leftover not.
    let e = engine(root.path(), kept, MIB).await;
    until("a maintenance round", || async { e.counted().is_some() }).await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(
        root.path().join(&s.info_hash).exists(),
        "the kept folder went"
    );
    assert!(
        idle.exists(),
        "the idle leftover went though under the budget"
    );
    let counted = e.counted().unwrap();
    assert!(counted.reclaimable < 2 * MIB, "{}", counted.reclaimable);
    let held = &counted.held[&s.info_hash.to_ascii_lowercase()];
    assert!(!held.idle);
    // Loaded and idle (no reader, no claim), it is no cache either: it
    // stays, and the leftover with it.
    let t = e.add_magnet(&s.info_hash, &[], &[]).await.unwrap();
    e.select_file(t, "default", &FileHint::Index(file))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(e.stats(t).await.is_ok(), "the kept torrent was evicted");
    assert!(
        idle.exists(),
        "the idle leftover went though under the budget"
    );
}

/// A window on a pack's other file: the kept file is no part of it, so its
/// held bytes leave it out, and asking for a window on the kept file plays
/// it whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_file_is_no_part_of_a_window() {
    let s = support::swarm_pack(&[("e01.mkv", 16 << 20), ("e02.mkv", 16 << 20)], PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), u64::MAX).await;
    let (t, e01, handle) = download(&e, &s, "e01.mkv").await;
    let e02 = e
        .select_file(t, "default", &FileHint::Name("e02.mkv".into()))
        .await
        .unwrap();
    let claim = e.claim(t, "default").unwrap();
    let plan = WindowPlan {
        budget: 8 * MIB,
        behind: 2 * MIB,
        slack: MIB,
        ahead: 2 * MIB,
        open_floor: 2 * MIB,
        open_cap: 4 * MIB,
        floor: 4 * MIB,
        playing: 2 * MIB,
        memory: true,
    };
    let w = Window {
        plan,
        bitrate: 64 << 10,
        metered: false,
        resume_at: None,
    };
    e.window(t, e01, w).await.unwrap();
    assert_eq!(
        e.stats(t).await.unwrap().window_bytes,
        None,
        "e01 plays whole"
    );
    e.window(t, e02, w).await.unwrap();
    assert_eq!(e.stats(t).await.unwrap().window_bytes, Some(8 * MIB));
    let held = e.stats(t).await.unwrap().held_bytes.unwrap_or(0);
    assert!(
        held < 2 * MIB,
        "the kept file counted in the window: {held}"
    );
    // The stream stops while e01's download still runs: the download holds
    // no playback claim, so the window ends (engine r1 P1-1).
    drop(claim);
    until("the window ends", || async {
        e.stats(t).await.unwrap().window_bytes.is_none()
    })
    .await;
    handle.stop().await;
}

/// A delete in a pack after a relaunch: the torrent loads from its details,
/// the file's own pieces leave the disk, and the other kept file's bytes
/// and the shared boundary pieces stay as they were.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_delete_in_a_pack_keeps_the_other_file() {
    // Sizes off the piece grid, so the two files share a boundary piece.
    let s = support::swarm_pack(
        &[
            ("e01.mkv", (12 << 20) + 300_000),
            ("e02.mkv", (12 << 20) + 200_000),
        ],
        PIECE,
    )
    .await;
    let root = tempfile::tempdir().unwrap();
    let (e01, e02) = {
        let e = engine(root.path(), Vec::new(), u64::MAX).await;
        let (_, e01, a) = download(&e, &s, "e01.mkv").await;
        let (_, e02, b) = download(&e, &s, "e02.mkv").await;
        a.stop().await;
        b.stop().await;
        (e01, e02)
    };
    let dir = root.path().join(&s.info_hash);
    let before = read(root.path(), &s.info_hash, "e02.mkv");
    assert!(before == s.others[0]);
    let size = |p: &Path| {
        let m = std::fs::metadata(p).unwrap();
        std::os::unix::fs::MetadataExt::blocks(&m) * 512
    };
    let e01_before = size(&dir.join("e01.mkv"));
    let kept = vec![(s.info_hash.clone(), e01), (s.info_hash.clone(), e02)];
    let e = engine(root.path(), kept, u64::MAX).await;
    e.delete_file(&s.info_hash, e01).await.unwrap();
    let e01_after = size(&dir.join("e01.mkv"));
    assert!(
        e01_after + 4 * MIB < e01_before,
        "e01 kept its bytes: {e01_before} then {e01_after}"
    );
    assert!(read(root.path(), &s.info_hash, "e02.mkv") == before);
    // Its shared boundary piece too: e02 is still whole by its bitfield.
    assert!(complete_on_disk(root.path(), &s.info_hash, e02));
    // The last kept file goes with the folder.
    e.delete_file(&s.info_hash, e02).await.unwrap();
    until("the folder is gone", || async { !dir.exists() }).await;
}

/// The disk under the reserve stops a download before any write fails, and
/// its handle says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_stops_under_the_reserve() {
    let s = support::swarm_big(64 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), u64::MAX).await;
    test_hooks::set_free(&e, Some(RESERVE_BYTES - 1));
    let t = e
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let handle = e.download(t, 1).await.unwrap();
    until("the download stops for space", || async {
        handle.progress().await.out_of_space
    })
    .await;
    let have = handle.progress().await.have;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let later = handle.progress().await.have;
    assert!(later <= have + 2 * MIB, "it went on: {have} then {later}");
    test_hooks::set_free(&e, None);
}

/// A file is whole on disk only with every piece in its bitfield and its
/// data all there.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn complete_on_disk_needs_every_piece_and_its_data() {
    let s = support::swarm_big(12 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let file = {
        let e = engine(root.path(), Vec::new(), u64::MAX).await;
        let (_, file, handle) = download(&e, &s, "movie.mkv").await;
        handle.stop().await;
        let dir = root.path().to_owned();
        let hash = s.info_hash.clone();
        until("whole", || {
            let (dir, hash) = (dir.clone(), hash.clone());
            async move { complete_on_disk(&dir, &hash, file) }
        })
        .await;
        file
    };
    // The engine's last bitfield writes land before the test edits it.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let have = root.path().join(&s.info_hash).with_extension("have");
    let bits = std::fs::read(&have).unwrap();
    // A middle piece left out of the bitfield.
    let mut missing = bits.clone();
    missing[0] &= !0x08;
    std::fs::write(&have, &missing).unwrap();
    assert!(!complete_on_disk(root.path(), &s.info_hash, file));
    std::fs::write(&have, &bits).unwrap();
    assert!(complete_on_disk(root.path(), &s.info_hash, file));
    // Its last 4 MiB as a hole: the length stays, the data does not.
    let path = root.path().join(&s.info_hash).join("movie.mkv");
    let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let len = f.metadata().unwrap().len();
    f.set_len(len - 4 * MIB).unwrap();
    f.set_len(len).unwrap();
    assert!(!complete_on_disk(root.path(), &s.info_hash, file));
}

/// What a running download will still write is committed, for its own
/// torrent's window as for any other (engine r1 P1-4, r2 P2-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_download_is_committed() {
    let s = support::swarm_big(32 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        limits: mq_engine::librqbit::limits::LimitsConfig {
            download_bps: std::num::NonZeroU32::new(2 << 20),
            upload_bps: None,
        },
        ..config(root.path(), Vec::new(), u64::MAX)
    };
    let e = Engine::start(cfg).await.unwrap();
    let t = e
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let handle = e.download(t, 1).await.unwrap();
    until("bytes come", || async { handle.progress().await.have > 0 }).await;
    let p = handle.progress().await;
    let left = p.len - p.have;
    assert!(e.committed(None).await >= left - 8 * MIB, "{left}");
    assert!(
        e.committed(Some(t)).await >= left - 8 * MIB,
        "its own window's C"
    );
    drop(handle);
}

/// Where the volume cannot punch, a ring may keep a stream's pieces in RAM;
/// never on a torrent with a download: a window opened while one runs
/// leaves the download's pieces on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ring_never_takes_a_downloads_pieces() {
    use mq_engine::librqbit::sync_log::{Fail, fail};
    let s = support::swarm_pack(&[("e01.mkv", 12 << 20), ("e02.mkv", 12 << 20)], PIECE).await;
    let root = tempfile::tempdir().unwrap();
    fail(root.path(), Fail::PunchRefused, usize::MAX);
    let cfg = EngineConfig {
        limits: mq_engine::librqbit::limits::LimitsConfig {
            download_bps: std::num::NonZeroU32::new(4 << 20),
            upload_bps: None,
        },
        ..config(root.path(), Vec::new(), u64::MAX)
    };
    let e = Engine::start(cfg).await.unwrap();
    let t = e
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let files = e.files(t).unwrap();
    let at = |name: &str| files.iter().position(|f| f.path.ends_with(name)).unwrap();
    let (e01, e02) = (at("e01.mkv"), at("e02.mkv"));
    let handle = e.download(t, e01).await.unwrap();
    e.select_file(t, "default", &FileHint::Index(e02))
        .await
        .unwrap();
    let _claim = e.claim(t, "default").unwrap();
    let plan = WindowPlan {
        budget: 4 * MIB,
        behind: MIB,
        slack: MIB,
        ahead: MIB,
        open_floor: MIB,
        open_cap: 2 * MIB,
        floor: 2 * MIB,
        playing: MIB,
        memory: true,
    };
    let w = Window {
        plan,
        bitrate: 64 << 10,
        metered: false,
        resume_at: None,
    };
    e.window(t, e02, w).await.unwrap();
    until("e01 is whole", || async {
        let p = handle.progress().await;
        p.have == p.len
    })
    .await;
    handle.stop().await;
    let dir = root.path().to_owned();
    let hash = s.info_hash.clone();
    until("e01 is whole on disk", || {
        let (dir, hash) = (dir.clone(), hash.clone());
        async move { complete_on_disk(&dir, &hash, e01) }
    })
    .await;
}

/// A finished download that was played pauses after its keep-warm minute
/// like any torrent: kept is no reason to stay connected (engine r2 P2-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_played_download_pauses_after_keep_warm() {
    use tokio::io::AsyncReadExt;
    let s = support::swarm_big(4 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), u64::MAX).await;
    let (t, file, handle) = download(&e, &s, "movie.mkv").await;
    handle.stop().await;
    {
        let reader = e.open_reader(t, "default", file).await.unwrap();
        let mut range = reader.range(0, Some(64 << 10)).await.unwrap();
        range.read_to_end(&mut Vec::new()).await.unwrap();
    }
    assert!(!e.stats(t).await.unwrap().paused, "a read wakes it");
    for _ in 0..80 {
        if e.stats(t).await.unwrap().paused {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("a played download never paused");
}

/// The usual pack case after a relaunch: e01 downloaded yesterday, e02
/// streamed today in a small window on a fresh engine. The engine tells the
/// torrent's scheduler its kept file as it goes live, so the window never
/// counts e01 and its keeper never evicts it; e01's bytes are no room a
/// window could free either (code review r1, engine P1-1 and P2-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_of_a_packs_other_file_keeps_the_download() {
    use tokio::io::AsyncReadExt;
    let s = support::swarm_pack(&[("e01.mkv", 16 << 20), ("e02.mkv", 48 << 20)], PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e01 = {
        let e = engine(root.path(), Vec::new(), u64::MAX).await;
        let (_, e01, handle) = download(&e, &s, "e01.mkv").await;
        handle.stop().await;
        e01
    };
    let dir = root.path().to_owned();
    let hash = s.info_hash.clone();
    until("e01 is whole on disk", || {
        let (dir, hash) = (dir.clone(), hash.clone());
        async move { complete_on_disk(&dir, &hash, e01) }
    })
    .await;
    let before = read(root.path(), &s.info_hash, "e01.mkv");
    let e = engine(root.path(), vec![(s.info_hash.clone(), e01)], u64::MAX).await;
    let t = e
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let e02 = e
        .select_file(t, "default", &FileHint::Name("e02.mkv".into()))
        .await
        .unwrap();
    let claim = e.claim(t, "default").unwrap();
    let w = Window {
        // The keeper tests' plan: a 24 MiB window under a 48 MiB file.
        plan: WindowPlan {
            budget: 24 * MIB + mq_engine::APFS_HEAD_BYTES,
            behind: 2 * MIB,
            slack: 4 * MIB,
            ahead: 4 * MIB,
            open_floor: 2 * MIB,
            open_cap: 4 * MIB,
            floor: 12 * MIB,
            playing: 6 * MIB,
            memory: false,
        },
        bitrate: 64 << 10,
        metered: false,
        resume_at: None,
    };
    e.window(t, e02, w).await.unwrap();
    let held = e.stats(t).await.unwrap().held_bytes.unwrap_or(0);
    assert!(held < 2 * MIB, "e01 counted in the window: {held}");
    assert!(
        e.reclaimable(Some(t)).await < 2 * MIB,
        "e01 counted as room"
    );
    // e02 to its end through the window: the keeper runs its rounds.
    let reader = e.open_reader(t, "default", e02).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    tokio::time::timeout(Duration::from_secs(120), range.read_to_end(&mut got))
        .await
        .expect("e02 read in time")
        .unwrap();
    assert!(got == s.others[0]);
    drop(range);
    drop(reader);
    drop(claim);
    assert!(complete_on_disk(root.path(), &s.info_hash, e01));
    assert!(read(root.path(), &s.info_hash, "e01.mkv") == before);
}

/// A delete under a playback of the same file evicts nothing, even with
/// another file of the pack still kept: the reader's bytes stay on disk to
/// the end (code review r1, engine P2-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_delete_under_its_own_playback_evicts_nothing() {
    use tokio::io::AsyncReadExt;
    let s = support::swarm_pack(&[("e01.mkv", 12 << 20), ("e02.mkv", 8 << 20)], PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), u64::MAX).await;
    let (t, e01, first) = download(&e, &s, "e01.mkv").await;
    first.stop().await;
    let (_, _, second) = download(&e, &s, "e02.mkv").await;
    second.stop().await;
    let path = root.path().join(&s.info_hash).join("e01.mkv");
    let size = || std::os::unix::fs::MetadataExt::blocks(&std::fs::metadata(&path).unwrap()) * 512;
    let before = size();
    let claim = e.claim(t, "default").unwrap();
    let reader = e.open_reader(t, "default", e01).await.unwrap();
    // Playing: a first read wakes the torrent.
    let mut head = reader.range(0, Some(MIB)).await.unwrap();
    head.read_to_end(&mut Vec::new()).await.unwrap();
    e.delete_file(&s.info_hash, e01).await.unwrap();
    assert!(
        size() + MIB >= before,
        "evicted under its reader: {before} then {}",
        size()
    );
    let mut range = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    range.read_to_end(&mut got).await.unwrap();
    assert!(got == s.movie);
    drop(claim);
}

/// A playing torrent still counts in the cache's budget, though no round
/// deletes it: over the budget, the idle one goes (code review r1, engine
/// P2-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_playing_torrent_counts_in_the_budget() {
    use tokio::io::AsyncReadExt;
    let a = support::swarm_pack(&[("a.mkv", 8 << 20)], PIECE).await;
    let b = support::swarm_pack(&[("b.mkv", 8 << 20)], PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let e = engine(root.path(), Vec::new(), 12 * MIB).await;
    let read_whole = |s: &support::Swarm, name: &'static str| {
        let (e, hash, seeder) = (e.clone(), s.info_hash.clone(), s.seeder_addr);
        async move {
            let t = e.add_magnet(&hash, &[], &[seeder]).await.unwrap();
            let file = e
                .select_file(t, "default", &FileHint::Name(name.into()))
                .await
                .unwrap();
            let claim = e.claim(t, "default").unwrap();
            let reader = e.open_reader(t, "default", file).await.unwrap();
            let mut range = reader.range(0, None).await.unwrap();
            range.read_to_end(&mut Vec::new()).await.unwrap();
            claim
        }
    };
    // b played and left; a playing now.
    drop(read_whole(&b, "b.mkv").await);
    let _playing = read_whole(&a, "a.mkv").await;
    let idle = root.path().join(&b.info_hash);
    until("the idle torrent goes", || async { !idle.exists() }).await;
    assert!(root.path().join(&a.info_hash).exists());
}
