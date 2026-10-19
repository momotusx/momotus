//! Holds (plans/m11-kids.md 4.4): a torrent paused because its title left a
//! kids list wakes for no range and no file choice until an authorization
//! newer than the hold lifts it, whichever of the two calls comes first.

mod support;

use std::time::Duration;

use mq_engine::{Engine, EngineError, FileHint, TorrentHandle};
use tokio::io::AsyncReadExt;

async fn read(engine: &Engine, t: TorrentHandle, file: usize) -> Result<Vec<u8>, EngineError> {
    let reader = engine.open_reader(t, file).await?;
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
            .select_file(t, &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        assert_eq!(read(engine, t, file).await.unwrap(), s.movie[..64 * 1024]);

        // Taken off the list at generation 5: paused at once and held.
        assert!(engine.pause_now(t, 5).await.unwrap());
        let stats = engine.stats(t).await.unwrap();
        assert!(stats.paused && stats.held, "{stats:?}");
        assert!(matches!(
            read(engine, t, file).await,
            Err(EngineError::Held)
        ));
        assert!(matches!(
            engine.select_file(t, &FileHint::Largest).await,
            Err(EngineError::Held)
        ));
        // An authorization read before the removal lifts nothing.
        assert!(!engine.release_hold(t, 5).await.unwrap());
        assert!(held(engine, t).await);
        // Allowed again (generation 6): the next range wakes it.
        assert!(engine.release_hold(t, 6).await.unwrap());
        assert_eq!(read(engine, t, file).await.unwrap(), s.movie[..64 * 1024]);

        // The other order: a pause read at 6, before the re-allow that the
        // authorization at 7 saw, lands late and is ignored.
        assert!(engine.release_hold(t, 7).await.unwrap());
        assert!(!engine.pause_now(t, 6).await.unwrap());
        assert!(!held(engine, t).await);
        assert_eq!(read(engine, t, file).await.unwrap(), s.movie[..64 * 1024]);
        // A removal after it holds again.
        assert!(engine.pause_now(t, 8).await.unwrap());
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
        assert!(engine.pause_now(t, 3).await.unwrap());
        assert!(matches!(
            engine.select_file(t, &FileHint::Largest).await,
            Err(EngineError::Held)
        ));
        assert!(matches!(
            engine.open_reader(t, 1).await,
            Err(EngineError::Held)
        ));
        assert_eq!(engine.stats(t).await.unwrap().downloaded_bytes, 0);
    };
    tokio::time::timeout(Duration::from_secs(60), run)
        .await
        .expect("finished in time");
}
