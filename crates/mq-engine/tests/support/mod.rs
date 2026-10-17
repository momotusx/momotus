//! A seeder engine with a small two-file torrent on loopback, shared by the
//! engine and server integration tests.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use mq_engine::{Engine, EngineConfig, FileHint, TorrentHandle};

pub struct Swarm {
    pub _seeder: Engine,
    pub leecher: Engine,
    pub info_hash: String,
    pub seeder_addr: SocketAddr,
    pub movie: Vec<u8>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

/// A 37,000-byte `.nfo` then a video file (so pieces straddle files) in
/// 256 KiB pieces, seeded on loopback with discovery off.
pub async fn swarm(dwh: bool) -> Swarm {
    let (seed_dir, leech_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let src = seed_dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let movie: Vec<u8> = (0..5 * 1024 * 1024 + 12_345u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    std::fs::write(src.join("a.nfo"), vec![b'n'; 37_000]).unwrap();
    std::fs::write(src.join("movie.mkv"), &movie).unwrap();
    let spawner = mq_engine::librqbit::spawn_utils::BlockingSpawner::new(2);
    let created = mq_engine::librqbit::create_torrent(
        &src,
        mq_engine::librqbit::CreateTorrentOptions {
            name: Some("Movie"),
            trackers: vec![],
            piece_length: Some(256 * 1024),
        },
        &spawner,
    )
    .await
    .unwrap();
    let info_hash = created.info_hash().as_string();
    // The seeder finds the data where it keeps every torrent.
    let data = seed_dir.path().join("engine").join(&info_hash);
    std::fs::create_dir_all(&data).unwrap();
    for f in ["a.nfo", "movie.mkv"] {
        std::fs::copy(src.join(f), data.join(f)).unwrap();
    }
    let config = |dir: &Path| EngineConfig {
        discovery: false,
        dwh,
        ..EngineConfig::new(dir)
    };
    let seeder = Engine::start(config(&seed_dir.path().join("engine")))
        .await
        .unwrap();
    let t = seeder
        .add_torrent_bytes(created.as_bytes().unwrap().to_vec(), &[])
        .await
        .unwrap();
    seeder.select_file(t, &FileHint::Largest).await.unwrap();
    wait_complete(&seeder, t).await;
    let port = seeder.listen_addr().unwrap().port();
    let leecher = Engine::start(config(leech_dir.path())).await.unwrap();
    Swarm {
        _seeder: seeder,
        leecher,
        info_hash,
        seeder_addr: SocketAddr::from(([127, 0, 0, 1], port)),
        movie,
        _dirs: (seed_dir, leech_dir),
    }
}

async fn wait_complete(engine: &Engine, t: TorrentHandle) {
    for _ in 0..200 {
        let s = engine.stats(t).await.unwrap();
        if s.pieces_total > 0 && s.pieces_have == s.pieces_total {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("seeder never verified its data");
}
