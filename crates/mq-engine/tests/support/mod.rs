//! A seeder engine with a small two-file torrent on loopback, shared by the
//! engine and server integration tests.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// The swarm on `127.0.0.1`, which both engines' policies allow.
pub async fn swarm(dwh: bool) -> Swarm {
    swarm_on(Ipv4Addr::LOCALHOST.into(), dwh).await
}

/// The seeder addressed on `ip`, both engines scoped to `[ip]`.
pub async fn swarm_on(ip: IpAddr, dwh: bool) -> Swarm {
    swarm_scoped(ip, &[ip], &[ip], dwh).await
}

/// A 37,000-byte `.nfo` then a video file (so pieces straddle files) in
/// 256 KiB pieces, seeded on loopback with discovery off. The seeder is
/// addressed on `ip`; each engine's policy allows its own list.
pub async fn swarm_scoped(ip: IpAddr, seeder: &[IpAddr], leecher: &[IpAddr], dwh: bool) -> Swarm {
    swarm_with(ip, seeder, leecher, 256 * 1024, dwh).await
}

/// [`swarm_scoped`] in `piece_len` pieces.
pub async fn swarm_with(
    ip: IpAddr,
    seeder: &[IpAddr],
    leecher: &[IpAddr],
    piece_len: u32,
    dwh: bool,
) -> Swarm {
    let movie: Vec<u8> = (0..5 * 1024 * 1024 + 12_345u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    swarm_of(ip, seeder, leecher, piece_len, dwh, ("movie.mkv", movie)).await
}

/// The swarm on `127.0.0.1` with `movie` as the video file, named `name`.
#[allow(dead_code)] // the server's MP4 tests use it
pub async fn swarm_movie(name: &str, movie: Vec<u8>) -> Swarm {
    let ip = Ipv4Addr::LOCALHOST.into();
    swarm_of(ip, &[ip], &[ip], 256 * 1024, true, (name, movie)).await
}

async fn swarm_of(
    ip: IpAddr,
    seeder: &[IpAddr],
    leecher: &[IpAddr],
    piece_len: u32,
    dwh: bool,
    (name, movie): (&str, Vec<u8>),
) -> Swarm {
    let (seed_dir, leech_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let src = seed_dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.nfo"), vec![b'n'; 37_000]).unwrap();
    std::fs::write(src.join(name), &movie).unwrap();
    let spawner = mq_engine::librqbit::spawn_utils::BlockingSpawner::new(2);
    let created = mq_engine::librqbit::create_torrent(
        &src,
        mq_engine::librqbit::CreateTorrentOptions {
            name: Some("Movie"),
            trackers: vec![],
            piece_length: Some(piece_len),
        },
        &spawner,
    )
    .await
    .unwrap();
    let info_hash = created.info_hash().as_string();
    // The seeder finds the data where it keeps every torrent.
    let data = seed_dir.path().join("engine").join(&info_hash);
    std::fs::create_dir_all(&data).unwrap();
    for f in ["a.nfo", name] {
        std::fs::copy(src.join(f), data.join(f)).unwrap();
    }
    let config = |dir: &Path, scope: &[IpAddr]| {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        EngineConfig {
            discovery: false,
            dwh,
            ..EngineConfig::new(dir, http.torrent_scope(scope))
        }
    };
    let seeder = Engine::start(config(&seed_dir.path().join("engine"), seeder))
        .await
        .unwrap();
    let t = seeder
        .add_torrent_bytes(created.as_bytes().unwrap().to_vec(), &[])
        .await
        .unwrap();
    seeder
        .select_file(t, "default", &FileHint::Largest)
        .await
        .unwrap();
    wait_complete(&seeder, t).await;
    let port = seeder.listen_addr().unwrap().port();
    let leecher = Engine::start(config(leech_dir.path(), leecher))
        .await
        .unwrap();
    Swarm {
        _seeder: seeder,
        leecher,
        info_hash,
        seeder_addr: SocketAddr::new(ip, port),
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

/// An HTTP tracker fixture on `[::1]`: every request gets `answer`, then the
/// connection closes, or with `keep_alive` waits for the next request on
/// it. Counts accepted connections and requests.
#[allow(dead_code)] // only the tracker tests use it
pub struct HttpTracker {
    pub url: String,
    connections: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
}

#[allow(dead_code)]
impl HttpTracker {
    pub fn start(answer: Vec<u8>, keep_alive: bool) -> HttpTracker {
        let listener = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let url = format!("http://{}/announce", listener.local_addr().unwrap());
        let (connections, requests) =
            (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (c, r) = (connections.clone(), requests.clone());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                c.fetch_add(1, Ordering::SeqCst);
                let (r, answer) = (r.clone(), answer.clone());
                std::thread::spawn(move || {
                    loop {
                        let mut head = Vec::new();
                        let mut buf = [0u8; 1024];
                        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => return,
                                Ok(n) => head.extend_from_slice(&buf[..n]),
                            }
                        }
                        r.fetch_add(1, Ordering::SeqCst);
                        if stream.write_all(&answer).is_err() || !keep_alive {
                            return;
                        }
                    }
                });
            }
        });
        HttpTracker {
            url,
            connections,
            requests,
        }
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}
