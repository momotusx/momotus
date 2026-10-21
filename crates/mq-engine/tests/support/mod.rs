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
    #[allow(dead_code)] // the keeper's tests start leechers of their own
    pub leecher: Engine,
    pub info_hash: String,
    pub seeder_addr: SocketAddr,
    pub movie: Vec<u8>,
    /// A pack's other videos, after `movie` (`swarm_pack`).
    #[allow(dead_code)]
    pub others: Vec<Vec<u8>>,
    /// The torrent (the window tests' extra seeders).
    #[allow(dead_code)]
    pub torrent: Vec<u8>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl Swarm {
    /// The leecher's data folder.
    #[allow(dead_code)]
    pub fn leech_dir(&self) -> &std::path::Path {
        self._dirs.1.path()
    }
}

/// One more seeder of a swarm's torrent, on `127.0.0.1`.
#[allow(dead_code)] // only the window tests use it
pub struct Seeder {
    pub _engine: Engine,
    pub addr: SocketAddr,
    _dir: tempfile::TempDir,
}

#[allow(dead_code)]
impl Swarm {
    /// Another seeder with the same data, for several peers at once.
    pub async fn another_seeder(&self) -> Seeder {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("engine").join(&self.info_hash);
        std::fs::create_dir_all(&data).unwrap();
        let from = self._dirs.0.path().join("engine").join(&self.info_hash);
        for e in std::fs::read_dir(&from).unwrap().flatten() {
            std::fs::copy(e.path(), data.join(e.file_name())).unwrap();
        }
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let ip: IpAddr = Ipv4Addr::LOCALHOST.into();
        let config = EngineConfig {
            discovery: false,
            ..EngineConfig::new(dir.path().join("engine"), http.torrent_scope(&[ip]))
        };
        let engine = Engine::start(config).await.unwrap();
        let t = engine
            .add_torrent_bytes(self.torrent.clone(), &[])
            .await
            .unwrap();
        engine
            .select_file(t, "default", &FileHint::Largest)
            .await
            .unwrap();
        wait_complete(&engine, t).await;
        let addr = SocketAddr::new(ip, engine.listen_addr().unwrap().port());
        Seeder {
            _engine: engine,
            addr,
            _dir: dir,
        }
    }
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
    swarm_of(
        ip,
        seeder,
        leecher,
        piece_len,
        dwh,
        true,
        vec![("movie.mkv", movie)],
    )
    .await
}

/// The swarm on `127.0.0.1` with a `len`-byte video in `piece_len` pieces
/// (the window keeper's tests); the leecher uploads nothing, as the app's.
#[allow(dead_code)]
pub async fn swarm_big(len: usize, piece_len: u32) -> Swarm {
    let movie: Vec<u8> = (0..len as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let ip = Ipv4Addr::LOCALHOST.into();
    swarm_of(
        ip,
        &[ip],
        &[ip],
        piece_len,
        true,
        false,
        vec![("movie.mkv", movie)],
    )
    .await
}

/// A season pack on `127.0.0.1`: the `.nfo`, then a video of each
/// `(name, len)` in turn, in `piece_len` pieces. `movie` is the first video,
/// `others` the rest; the leecher uploads nothing, as the app's.
#[allow(dead_code)]
pub async fn swarm_pack(videos: &[(&str, usize)], piece_len: u32) -> Swarm {
    let videos = videos
        .iter()
        .enumerate()
        .map(|(n, &(name, len))| {
            let bytes = (0..len as u32)
                .map(|i| (i.wrapping_add(n as u32 * 7_919).wrapping_mul(2_654_435_761) >> 13) as u8)
                .collect();
            (name, bytes)
        })
        .collect();
    let ip = Ipv4Addr::LOCALHOST.into();
    swarm_of(ip, &[ip], &[ip], piece_len, true, false, videos).await
}

/// The swarm on `127.0.0.1` with `movie` as the video file, named `name`.
#[allow(dead_code)] // the server's MP4 tests use it
pub async fn swarm_movie(name: &str, movie: Vec<u8>) -> Swarm {
    let ip = Ipv4Addr::LOCALHOST.into();
    swarm_of(
        ip,
        &[ip],
        &[ip],
        256 * 1024,
        true,
        true,
        vec![(name, movie)],
    )
    .await
}

async fn swarm_of(
    ip: IpAddr,
    seeder: &[IpAddr],
    leecher: &[IpAddr],
    piece_len: u32,
    dwh: bool,
    leech_upload: bool,
    videos: Vec<(&str, Vec<u8>)>,
) -> Swarm {
    let (seed_dir, leech_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let src = seed_dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.nfo"), vec![b'n'; 37_000]).unwrap();
    for (name, bytes) in &videos {
        std::fs::write(src.join(name), bytes).unwrap();
    }
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
    for f in std::iter::once("a.nfo").chain(videos.iter().map(|v| v.0)) {
        std::fs::copy(src.join(f), data.join(f)).unwrap();
    }
    let mut videos = videos.into_iter().map(|v| v.1);
    let movie = videos.next().unwrap_or_default();
    let others = videos.collect();
    let config = |dir: &Path, scope: &[IpAddr], upload: bool| {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        EngineConfig {
            discovery: false,
            dwh,
            upload,
            ..EngineConfig::new(dir, http.torrent_scope(scope))
        }
    };
    let seeder = Engine::start(config(&seed_dir.path().join("engine"), seeder, true))
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
    let leecher = Engine::start(config(leech_dir.path(), leecher, leech_upload))
        .await
        .unwrap();
    Swarm {
        _seeder: seeder,
        leecher,
        info_hash,
        seeder_addr: SocketAddr::new(ip, port),
        movie,
        others,
        torrent: created.as_bytes().unwrap().to_vec(),
        _dirs: (seed_dir, leech_dir),
    }
}

/// The seeder's check of its data: up to a minute, since a test binary's
/// swarms hash at once and a loaded machine slows a debug build's SHA-1.
async fn wait_complete(engine: &Engine, t: TorrentHandle) {
    for _ in 0..1200 {
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
/// it. Counts accepted connections and requests, and keeps each request's
/// first line.
#[allow(dead_code)] // only the tracker tests use it
pub struct HttpTracker {
    pub url: String,
    connections: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    lines: Arc<std::sync::Mutex<Vec<String>>>,
}

#[allow(dead_code)]
impl HttpTracker {
    pub fn start(answer: Vec<u8>, keep_alive: bool) -> HttpTracker {
        let listener = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let url = format!("http://{}/announce", listener.local_addr().unwrap());
        let (connections, requests) =
            (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let lines = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (c, r, l) = (connections.clone(), requests.clone(), lines.clone());
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                c.fetch_add(1, Ordering::SeqCst);
                let (r, l, answer) = (r.clone(), l.clone(), answer.clone());
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
                        let first = head.split(|&b| b == b'\r').next().unwrap_or_default();
                        l.lock()
                            .unwrap()
                            .push(String::from_utf8_lossy(first).into_owned());
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
            lines,
        }
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// Every request's first line so far, in order.
    pub fn request_lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }
}
