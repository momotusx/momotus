//! Holds (plans/m11-kids.md 4.4): a torrent paused because its title left a
//! kids list wakes for none of that profile's ranges or file choices until
//! an authorization newer than the hold lifts it, whichever of the two calls
//! comes first. Other profiles' reads and choices ignore the hold, and a
//! torrent another profile has a claim on keeps running.

mod support;

use std::time::Duration;

use mq_engine::{Engine, EngineConfig, EngineError, FileHint, TorrentHandle};
use tokio::io::AsyncReadExt;

const KID: &str = "kid";
const OWNER: &str = "default";

async fn read(
    engine: &Engine,
    t: TorrentHandle,
    profile: &str,
    file: usize,
) -> Result<Vec<u8>, EngineError> {
    let reader = engine.open_reader(t, profile, file).await?;
    let mut range = reader.range(0, Some(64 * 1024)).await?;
    range.ready().await?;
    let mut out = Vec::new();
    range.read_to_end(&mut out).await?;
    Ok(out)
}

async fn held(engine: &Engine, t: TorrentHandle) -> bool {
    engine.stats(t).await.unwrap().held
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_torrent_wakes_only_for_a_newer_authorization() {
    let s = support::swarm(true).await;
    let run = async {
        let engine = &s.leecher;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let file = engine
            .select_file(t, KID, &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        assert_eq!(
            read(engine, t, KID, file).await.unwrap(),
            s.movie[..64 * 1024]
        );

        // Taken off the list at generation 5: paused at once and held.
        assert!(engine.pause_now(t, KID, 5).await.unwrap());
        let stats = engine.stats(t).await.unwrap();
        assert!(stats.paused && stats.held, "{stats:?}");
        assert!(matches!(
            read(engine, t, KID, file).await,
            Err(EngineError::Held)
        ));
        assert!(matches!(
            engine.select_file(t, KID, &FileHint::Largest).await,
            Err(EngineError::Held)
        ));
        // An authorization read before the removal lifts nothing.
        assert!(!engine.release_hold(t, KID, 5).await.unwrap());
        assert!(held(engine, t).await);
        // Allowed again (generation 6): the next range wakes it.
        assert!(engine.release_hold(t, KID, 6).await.unwrap());
        assert_eq!(
            read(engine, t, KID, file).await.unwrap(),
            s.movie[..64 * 1024]
        );

        // The other order: a pause read at 6, before the re-allow that the
        // authorization at 7 saw, lands late and is ignored.
        assert!(engine.release_hold(t, KID, 7).await.unwrap());
        assert!(!engine.pause_now(t, KID, 6).await.unwrap());
        assert!(!held(engine, t).await);
        assert_eq!(
            read(engine, t, KID, file).await.unwrap(),
            s.movie[..64 * 1024]
        );
        // A removal after it holds again.
        assert!(engine.pause_now(t, KID, 8).await.unwrap());
        assert!(held(engine, t).await);
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("finished in time");
}

/// A handle held before its file was ever chosen (a removal during a
/// playback's setup) never starts.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_held_during_setup_never_starts() {
    let s = support::swarm(true).await;
    let run = async {
        let engine = &s.leecher;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        assert!(engine.pause_now(t, KID, 3).await.unwrap());
        assert!(matches!(
            engine.select_file(t, KID, &FileHint::Largest).await,
            Err(EngineError::Held)
        ));
        assert!(matches!(
            engine.open_reader(t, KID, 1).await,
            Err(EngineError::Held)
        ));
        assert_eq!(engine.stats(t).await.unwrap().downloaded_bytes, 0);
    };
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("finished in time");
}

/// The kid's hold binds the kid only: the owner, playing the same release
/// at the same generation, chooses its file and reads; while the owner has
/// a claim the torrent is not paused at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hold_binds_only_its_own_profile() {
    let s = support::swarm(true).await;
    let run = async {
        let engine = &s.leecher;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let movie = FileHint::Name("movie.mkv".into());
        let file = engine.select_file(t, KID, &movie).await.unwrap();
        let claim = engine.claim(t, OWNER).unwrap();
        assert_eq!(
            read(engine, t, OWNER, file).await.unwrap(),
            s.movie[..64 * 1024]
        );

        // The owner is playing: the kid's hold leaves the torrent running.
        assert!(engine.pause_now(t, KID, 5).await.unwrap());
        let stats = engine.stats(t).await.unwrap();
        assert!(stats.held && !stats.paused, "{stats:?}");
        assert!(matches!(
            read(engine, t, KID, file).await,
            Err(EngineError::Held)
        ));
        let via_claim = claim.open_reader(file).await.unwrap();
        let mut range = via_claim.range(0, Some(1024)).await.unwrap();
        range.ready().await.unwrap();
        // The owner's own check at the same generation is not refused.
        assert!(engine.release_hold(t, OWNER, 5).await.unwrap());
        assert!(held(engine, t).await, "the kid's hold stays");
        drop((range, via_claim, claim));

        // No other claim: a hold pauses it at once; the owner's next read
        // wakes it all the same, the kid's does not.
        assert!(engine.pause_now(t, KID, 6).await.unwrap());
        assert!(engine.stats(t).await.unwrap().paused);
        assert_eq!(engine.select_file(t, OWNER, &movie).await.unwrap(), file);
        assert_eq!(
            read(engine, t, OWNER, file).await.unwrap(),
            s.movie[..64 * 1024]
        );
        assert!(!engine.stats(t).await.unwrap().paused);
        assert!(matches!(
            engine.select_file(t, KID, &movie).await,
            Err(EngineError::Held)
        ));
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("finished in time");
}

/// Up Next opens the next episode from the pack the current one plays, and
/// gives up on it (its head was late): the torrent and its files stay, and
/// the playing episode reads on. Once it ends, the removal goes through.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_open_that_gives_up_leaves_a_torrent_another_playback_claims() {
    let s = support::swarm(true).await;
    let run = async {
        let engine = &s.leecher;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let playing = engine.claim(t, OWNER).unwrap();
        let movie = engine
            .select_file(t, OWNER, &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        let reader = playing.open_reader(movie).await.unwrap();
        assert_eq!(
            read(engine, t, OWNER, movie).await.unwrap(),
            s.movie[..64 * 1024]
        );

        let next = engine.claim(t, OWNER).unwrap();
        let other = engine
            .select_file(t, OWNER, &FileHint::Name("a.nfo".into()))
            .await
            .unwrap();
        assert_ne!(other, movie);
        drop(next);
        assert!(matches!(
            engine.remove(t, false).await,
            Err(EngineError::Claimed)
        ));

        let (a, b) = (3 << 20, (3 << 20) + 64 * 1024);
        let mut range = reader.range(a as u64, Some(b as u64)).await.unwrap();
        range.ready().await.unwrap();
        let mut out = Vec::new();
        range.read_to_end(&mut out).await.unwrap();
        assert!(out == s.movie[a..b], "the playing episode reads on");

        drop((range, reader, playing));
        engine.remove(t, false).await.unwrap();
        assert!(matches!(
            engine.stats(t).await,
            Err(EngineError::UnknownTorrent)
        ));
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("finished in time");
}

/// The disk budget evicts idle torrents, never a claimed one: a playback
/// paused with no range open keeps its torrent. Unclaimed, the next
/// maintenance round (every 5 s) evicts it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_disk_budget_never_evicts_a_claimed_torrent() {
    let s = support::swarm(true).await;
    let run = async {
        let dir = tempfile::tempdir().unwrap();
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let ip = std::net::Ipv4Addr::LOCALHOST.into();
        let engine = Engine::start(EngineConfig {
            discovery: false,
            cache_budget_bytes: 1,
            ..EngineConfig::new(dir.path(), http.torrent_scope(&[ip]))
        })
        .await
        .unwrap();
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let claim = engine.claim(t, OWNER).unwrap();
        let movie = engine
            .select_file(t, OWNER, &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        assert_eq!(
            read(&engine, t, OWNER, movie).await.unwrap(),
            s.movie[..64 * 1024]
        );

        tokio::time::sleep(Duration::from_secs(11)).await;
        assert!(
            engine.stats(t).await.is_ok(),
            "a claimed torrent was evicted"
        );
        // Its folder is no leftover either (engine review r3, P3-4).
        assert!(
            dir.path().join(&s.info_hash).exists(),
            "a claimed torrent's folder went as a leftover"
        );

        drop(claim);
        for _ in 0..30 {
            if engine.stats(t).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(matches!(
            engine.stats(t).await,
            Err(EngineError::UnknownTorrent)
        ));
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("finished in time");
}

/// A claim that lands while the disk budget is about to evict the torrent
/// keeps it (engine review r1, P3): the eviction stops after its first
/// check (the test hook), the torrent is claimed, then it goes on, and the
/// check under the torrents lock sees the claim.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_claim_during_an_eviction_keeps_the_torrent() {
    let s = support::swarm(true).await;
    let run = async {
        let dir = tempfile::tempdir().unwrap();
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let ip = std::net::Ipv4Addr::LOCALHOST.into();
        let engine = Engine::start(EngineConfig {
            discovery: false,
            cache_budget_bytes: 1,
            ..EngineConfig::new(dir.path(), http.torrent_scope(&[ip]))
        })
        .await
        .unwrap();
        // No round stops before the range closes: the file is read, or has
        // no bytes yet.
        let evicting = mq_engine::test_hooks::hold_evictions(&engine);
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let movie = engine
            .select_file(t, OWNER, &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        assert_eq!(
            read(&engine, t, OWNER, movie).await.unwrap(),
            s.movie[..64 * 1024]
        );
        tokio::time::timeout(Duration::from_secs(15), evicting.reached())
            .await
            .expect("no eviction round came");
        let claim = engine.claim(t, OWNER).unwrap();
        evicting.open();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            engine.stats(t).await.is_ok(),
            "a torrent claimed during its eviction was removed"
        );
        drop(claim);
    };
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("finished in time");
}

/// Torrent folders an earlier launch left count against the disk budget
/// from the start, the least recently changed first; a folder that is no
/// torrent's stays (engine review r2, P2-4: they were never evicted). A
/// torrent folder an interrupted round set aside goes, a set-aside name
/// that is no torrent's stays (engine review r3, P3-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_disk_budget_evicts_what_earlier_launches_left() {
    let dir = tempfile::tempdir().unwrap();
    let older = dir.path().join("1".repeat(40));
    let newer = dir.path().join("2".repeat(40));
    let other = dir.path().join("notes");
    let now = std::time::SystemTime::now();
    let trash = dir.path().join(format!("{}.evicted", "3".repeat(40)));
    let not_trash = dir.path().join("notes.evicted");
    for folder in [&trash, &not_trash] {
        std::fs::create_dir_all(folder).unwrap();
    }
    for (folder, age) in [(&older, 7200), (&newer, 3600), (&other, 9000)] {
        std::fs::create_dir_all(folder).unwrap();
        let file = folder.join("movie.mkv");
        std::fs::write(&file, vec![7u8; 1 << 20]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(now - Duration::from_secs(age))
            .unwrap();
    }
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let engine = Engine::start(EngineConfig {
        discovery: false,
        // One leftover fits, two do not.
        cache_budget_bytes: 3 << 19,
        ..EngineConfig::new(dir.path(), http.torrent_scope(&[]))
    })
    .await
    .unwrap();
    for _ in 0..40 {
        if !older.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!older.exists(), "the older leftover stayed");
    assert!(newer.exists(), "the newer leftover went too");
    assert!(other.exists(), "a folder that is no torrent's went");
    for _ in 0..40 {
        if !trash.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!trash.exists(), "a set-aside torrent folder stayed");
    assert!(
        not_trash.exists(),
        "a set-aside name that is no torrent's went"
    );
    drop(engine);
}
