use mq_http::{ClientConfig, HttpClient, HttpError};
use mq_import::{
    AuthKey, Endpoints, ImportError, ImportReport, LinkLogin, LinkPoll, SeriesVideos, Snapshot,
    StremioClient, apply_snapshot, import_account,
};
use mq_library::{Hlc, Library};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stremio_core::types::resource::Video;
use stremio_core::types::watched_bitfield::{WatchedBitField, WatchedField};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const KEY: &str = "SESSION_KEY_0123456789abcdef";
const TRAKT_TOKEN: &str = "trakt-token-secret";
const PROFILE: &str = "p1";

// 2026-03-01T12:00:00Z, 2026-02-20T09:30:00Z and 2026-01-10T18:00:00Z in ms.
const M_NEW: i64 = 1_772_366_400_000;
const M_OLD: i64 = 1_771_579_800_000;
const CTIME: i64 = 1_768_068_000_000;
const T_NEW: &str = "2026-03-01T12:00:00.000Z";
const T_OLD: &str = "2026-02-20T09:30:00.000Z";
const T_CTIME: &str = "2026-01-10T18:00:00.000Z";

/// item_id, in_library, kind, poster, poster_shape, added_ms
type LibRow = (String, bool, String, Option<String>, Option<String>, i64);

// ---- local server -------------------------------------------------------

#[derive(Clone)]
struct Seen {
    path: String,
    body: Value,
}

struct Server {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    fn endpoints(&self) -> Endpoints {
        Endpoints {
            api: self.base.clone(),
            link: self.base.clone(),
        }
    }

    fn paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.path.clone())
            .collect()
    }

    fn body_of(&self, path: &str) -> Value {
        let seen = self.seen.lock().unwrap();
        seen.iter().find(|s| s.path == path).unwrap().body.clone()
    }
}

async fn read_request(socket: &mut TcpStream) -> (String, Value) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed before the request head");
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let path = head.split_whitespace().nth(1).unwrap().to_owned();
    let len: usize = head
        .to_ascii_lowercase()
        .lines()
        .find_map(|l| {
            l.strip_prefix("content-length:")
                .map(|v| v.trim().to_owned())
        })
        .map_or(0, |v| v.parse().unwrap());
    while buf.len() < head_end + len {
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed before the request body");
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = if len == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&buf[head_end..head_end + len]).unwrap()
    };
    (path, body)
}

/// One request per connection; `handler` maps (path, json body) to
/// (status, json response).
async fn serve(handler: impl Fn(&str, &Value) -> (u16, Value) + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let handler = handler.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let (path, body) = read_request(&mut socket).await;
                let (status, json) = handler(&path, &body);
                log.lock().unwrap().push(Seen { path, body });
                let payload = json.to_string();
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(payload.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    Server { base, seen }
}

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

// ---- local library with a SQL probe ---------------------------------------

struct Db {
    lib: Library,
    path: PathBuf,
}

impl Db {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mq-import-test-{}-{}.sqlite",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            lib: Library::open(&path).unwrap(),
            path,
        }
    }

    fn sql(&self) -> Connection {
        Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

impl Deref for Db {
    type Target = Library;
    fn deref(&self) -> &Library {
        &self.lib
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = self.path.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---- fixtures -------------------------------------------------------------

fn episode_ids(imdb: &str, season: u32, count: u32) -> Vec<String> {
    (1..=count)
        .map(|e| format!("{imdb}:{season}:{e}"))
        .collect()
}

/// Stremio's own encoder, so the fixture bytes are what a real client writes.
fn watched_field(video_ids: &[String], watched: &[usize]) -> String {
    let mut bits = WatchedBitField::construct_from_array(vec![], video_ids.to_vec());
    for &i in watched {
        bits.set(i, true);
    }
    WatchedField::from(bits).to_string()
}

/// Two seasons of Game of Thrones, handed over newest first to prove the
/// decoder sorts them the way Stremio does.
fn got_videos() -> Vec<Video> {
    let mut videos = Vec::new();
    for (season, count) in [(2, 3), (1, 10)] {
        for episode in (1..=count).rev() {
            videos.push(
                serde_json::from_value(json!({
                    "id": format!("tt0944947:{season}:{episode}"),
                    "title": format!("S{season}E{episode}"),
                    "season": season,
                    "episode": episode,
                    "released": format!("2011-0{season}-{:02}T00:00:00.000Z", episode),
                }))
                .unwrap(),
            );
        }
    }
    videos
}

fn state(
    offset: u64,
    duration: u64,
    video_id: Option<&str>,
    watched: Option<String>,
    times_watched: u32,
) -> Value {
    json!({
        "lastWatched": "2026-02-28T20:00:00.000Z",
        "timeWatched": offset,
        "timeOffset": offset,
        "overallTimeWatched": offset * 2,
        "timesWatched": times_watched,
        "flaggedWatched": u32::from(times_watched > 0),
        "duration": duration,
        "video_id": video_id,
        "watched": watched,
        "noNotif": false,
    })
}

fn item(
    id: &str,
    name: &str,
    kind: &str,
    removed: bool,
    temp: bool,
    mtime: &str,
    state: Value,
) -> Value {
    json!({
        "_id": id,
        "name": name,
        "type": kind,
        "poster": format!("https://images.example/{id}.jpg"),
        "posterShape": "poster",
        "removed": removed,
        "temp": temp,
        "_ctime": T_CTIME,
        "_mtime": mtime,
        "state": state,
        "behaviorHints": {"defaultVideoId": null, "featuredVideoId": null, "hasScheduledVideos": false},
    })
}

fn library_fixture() -> Vec<Value> {
    let sev_ids = episode_ids("tt11280740", 1, 9);
    let got_ids: Vec<String> = {
        let mut v = episode_ids("tt0944947", 1, 10);
        v.extend(episode_ids("tt0944947", 2, 3));
        v
    };
    let mut bbb = item(
        "bbb",
        "Big Buck Bunny",
        "movie",
        false,
        false,
        T_NEW,
        state(90_000, 596_000, Some("yt:bbb"), None, 0),
    );
    bbb["behaviorHints"]["defaultVideoId"] = json!("yt:bbb");
    let mut severance = item(
        "tt11280740",
        "Severance",
        "series",
        false,
        false,
        T_NEW,
        state(
            0,
            0,
            Some("tt11280740:1:4"),
            Some(watched_field(&sev_ids, &[0, 1, 2])),
            3,
        ),
    );
    severance["state"]["noNotif"] = json!(true);
    vec![
        // Series in the library, episodes 1,2,3,5 watched, pointer on episode 6.
        item(
            "tt0944947",
            "Game of Thrones",
            "series",
            false,
            false,
            T_NEW,
            state(
                1_320_000,
                3_600_000,
                Some("tt0944947:1:6"),
                Some(watched_field(&got_ids, &[0, 1, 2, 4])),
                4,
            ),
        ),
        // Series whose video list the caller does not have: deferred.
        severance,
        // Movie in progress.
        item(
            "tt1375666",
            "Inception",
            "movie",
            false,
            false,
            T_NEW,
            state(1_800_000, 8_880_000, Some("tt1375666"), None, 0),
        ),
        // Movie played through and marked watched, no progress.
        item(
            "tt0133093",
            "The Matrix",
            "movie",
            false,
            false,
            T_OLD,
            state(0, 8_160_000, Some("tt0133093"), None, 1),
        ),
        // Movie whose video id comes from the default video hint.
        bbb,
        // Temp item with progress: a progress row and a non-library row.
        item(
            "tt0083658",
            "Blade Runner",
            "movie",
            true,
            true,
            T_NEW,
            state(600_000, 7_000_000, Some("tt0083658"), None, 0),
        ),
        // Tombstone with a stale offset: skipped entirely.
        item(
            "tt0113277",
            "Heat",
            "movie",
            true,
            false,
            T_NEW,
            state(1_000, 10_000, Some("tt0113277"), None, 0),
        ),
        // Temp item that was only opened: nothing to keep.
        item(
            "tt0078748",
            "Alien",
            "movie",
            true,
            true,
            T_NEW,
            state(0, 0, None, None, 0),
        ),
        // Not a library item at all: counted as undecodable.
        json!({"_id": "broken", "name": "Broken", "type": "movie"}),
    ]
}

fn manifest(id: &str, name: &str, resources: Value) -> Value {
    json!({
        "id": id,
        "version": "1.2.3",
        "name": name,
        "description": format!("{name} addon"),
        "types": ["movie", "series"],
        "resources": resources,
        "idPrefixes": ["tt"],
        "catalogs": [],
    })
}

fn addons_fixture() -> Value {
    let secret_path = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut torrentio = manifest(
        "org.stremio.torrentio",
        "Torrentio",
        json!([{"name": "stream", "types": ["movie", "series"], "idPrefixes": ["tt"]}]),
    );
    torrentio["behaviorHints"] = json!({"configurable": true, "p2p": true});
    json!({
        "addons": [
            {
                "manifest": manifest("com.linvo.cinemeta", "Cinemeta", json!(["catalog", "meta"])),
                "transportUrl": "https://v3-cinemeta.strem.io/manifest.json",
                "flags": {"official": true, "protected": true},
            },
            {
                "manifest": torrentio,
                "transportUrl": format!("https://torrentio.strem.fun/realdebrid={secret_path}/manifest.json"),
                "flags": {"official": false, "protected": false},
            },
            {
                "manifest": manifest("org.lan.addon", "Home Library", json!(["catalog", "stream"])),
                "transportUrl": "http://192.168.1.20:7000/manifest.json",
                "flags": {},
            },
            {
                // Legacy descriptor: JSON-RPC transport, no flags, extra field.
                "manifest": manifest("org.legacy.subs", "Legacy Subs", json!(["subtitles"])),
                "transportUrl": "https://legacy.example.org/stremio/v1",
                "transportName": "legacy",
            },
            {
                "manifest": {"id": "org.broken", "name": "Broken"},
                "transportUrl": "https://broken.example.org/manifest.json",
            },
            {
                "manifest": manifest("com.linvo.cinemeta", "Cinemeta", json!(["catalog", "meta"])),
                "transportUrl": "https://v3-cinemeta.strem.io/manifest.json",
            },
            {
                "manifest": manifest("org.ftp.addon", "Ftp Addon", json!(["catalog"])),
                "transportUrl": format!("ftp://files.example.org/{secret_path}/manifest.json"),
            },
        ],
        "lastModified": "2026-03-02T10:00:00.000Z",
    })
}

fn user_fixture() -> Value {
    json!({
        "_id": "5f0000000000000000000001",
        "email": "user@example.com",
        "fbId": null,
        "avatar": null,
        "lastModified": "2026-03-01T00:00:00.000Z",
        "dateRegistered": "2020-01-01T00:00:00.000Z",
        "trakt": {"created_at": 1_700_000_000, "expires_in": 7_776_000, "access_token": TRAKT_TOKEN},
        "premium_expire": null,
        "gdpr_consent": {"tos": true, "privacy": true, "marketing": false, "from": "tos"},
    })
}

fn account(path: &str, body: &Value) -> (u16, Value) {
    if body["authKey"] != KEY {
        return (
            200,
            json!({"error": {"code": 1, "message": "Session does not exist"}}),
        );
    }
    match path {
        "/api/getUser" => (200, json!({"result": user_fixture()})),
        "/api/addonCollectionGet" => (200, json!({"result": addons_fixture()})),
        "/api/datastoreGet" => (200, json!({"result": library_fixture()})),
        _ => (404, json!({})),
    }
}

async fn client_for(server: &Server) -> StremioClient {
    StremioClient::login_with_auth_key(http(), server.endpoints(), AuthKey::new(KEY))
        .await
        .unwrap()
}

fn got_only() -> SeriesVideos {
    SeriesVideos::from([("tt0944947".to_owned(), got_videos())])
}

fn urls(db: &Db) -> Vec<String> {
    db.addons(PROFILE)
        .unwrap()
        .into_iter()
        .map(|a| a.transport_url)
        .collect()
}

// ---- tests ----------------------------------------------------------------

#[tokio::test]
async fn imports_addons_library_progress_and_watched() {
    let server = serve(account).await;
    let db = Db::new();
    let client = client_for(&server).await;

    let report = import_account(&client, &db, PROFILE, "Imported", &got_only())
        .await
        .unwrap();

    // Requests: read-only, one session key, the library fetched in full.
    assert_eq!(
        server.paths(),
        [
            "/api/getUser",
            "/api/addonCollectionGet",
            "/api/datastoreGet"
        ]
    );
    assert_eq!(server.body_of("/api/addonCollectionGet")["update"], false);
    let ds = server.body_of("/api/datastoreGet");
    assert_eq!(
        (ds["collection"].as_str(), ds["all"].as_bool()),
        (Some("libraryItem"), Some(true))
    );

    // Addons: Stremio's order, manifests kept verbatim, LAN addon allowed private.
    assert_eq!(report.addons_imported, 4);
    assert_eq!(
        urls(&db),
        [
            "https://v3-cinemeta.strem.io/manifest.json",
            "https://torrentio.strem.fun/realdebrid=ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/manifest.json",
            "http://192.168.1.20:7000/manifest.json",
            "https://legacy.example.org/stremio/v1",
        ]
    );
    let rows = db.addons(PROFILE).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.allow_private).collect::<Vec<_>>(),
        [false, false, true, false]
    );
    // Places with gaps, so another device's addon fits between two
    // (plans/accounts-social-sync.md 13).
    assert_eq!(
        rows.iter().map(|r| r.position).collect::<Vec<_>>(),
        [1024, 2048, 3072, 4096]
    );
    let stored: Value = serde_json::from_str(&rows[1].manifest_json).unwrap();
    assert_eq!(stored["behaviorHints"]["p2p"], true);
    assert_eq!(stored["resources"][0]["name"], "stream");

    // Skipped addons carry a reason and never a secret.
    let reasons: Vec<_> = report
        .addons_skipped
        .iter()
        .map(|s| (s.name.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), 3);
    assert!(reasons[0].0 == "Broken" && reasons[0].1.starts_with("invalid manifest"));
    assert_eq!(reasons[1], ("Cinemeta", "duplicate transport url"));
    assert_eq!(
        reasons[2],
        ("Ftp Addon", "unsupported transport scheme ftp")
    );
    let shown = format!("{report:?}");
    assert!(
        !shown.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
        "{shown}"
    );
    assert_eq!(
        report.addons_skipped[2].url,
        "ftp://files.example.org/<redacted>/manifest.json"
    );

    // Library: in-library titles only, tombstones and untouched temp items dropped.
    assert_eq!(report.items_imported, 5);
    let sql = db.sql();
    let rows: Vec<LibRow> = sql
        .prepare(
            "SELECT item_id, in_library, kind, poster, poster_shape, added_ms
             FROM library_items WHERE profile_id = ?1 ORDER BY item_id",
        )
        .unwrap()
        .query_map(params![PROFILE], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let ids: Vec<_> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "bbb",
            "tt0083658",
            "tt0133093",
            "tt0944947",
            "tt11280740",
            "tt1375666"
        ]
    );
    let flags: Vec<_> = rows.iter().map(|r| r.1).collect();
    assert_eq!(
        flags,
        [true, false, true, true, true, true],
        "Blade Runner is temp"
    );
    let got = rows.iter().find(|r| r.0 == "tt0944947").unwrap();
    assert_eq!(got.2, "series");
    assert_eq!(
        got.3.as_deref(),
        Some("https://images.example/tt0944947.jpg")
    );
    assert_eq!(got.4.as_deref(), Some("poster"));
    assert_eq!(got.5, CTIME);

    // Progress: offset, duration and video id, with _mtime as the update time.
    assert_eq!(report.in_progress, 4);
    assert_eq!(
        db.progress(PROFILE, "tt0944947:1:6").unwrap(),
        Some((1_320_000, 3_600_000))
    );
    assert_eq!(
        db.progress(PROFILE, "tt1375666").unwrap(),
        Some((1_800_000, 8_880_000))
    );
    assert_eq!(
        db.progress(PROFILE, "tt0083658").unwrap(),
        Some((600_000, 7_000_000))
    );
    assert_eq!(
        db.progress(PROFILE, "yt:bbb").unwrap(),
        Some((90_000, 596_000))
    );
    assert_eq!(
        db.progress(PROFILE, "tt0133093").unwrap(),
        None,
        "offset 0 is not progress"
    );
    assert_eq!(
        db.progress(PROFILE, "tt0113277").unwrap(),
        None,
        "tombstone"
    );
    let updated: i64 = sql
        .query_row(
            "SELECT updated_ms FROM progress WHERE video_id = 'tt1375666'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(updated, M_NEW);
    let resume: Vec<_> = db
        .continue_watching(PROFILE, 10)
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(resume.len(), 4);
    for name in [
        "Game of Thrones",
        "Inception",
        "Blade Runner",
        "Big Buck Bunny",
    ] {
        assert!(resume.iter().any(|n| n == name), "{name} in {resume:?}");
    }

    // Watched: GoT episodes 1,2,3,5 from the bitfield, the movie from its play count.
    assert_eq!(report.watched_marked, 5);
    assert_eq!(
        db.watched_videos(PROFILE, "tt0944947").unwrap(),
        [
            "tt0944947:1:1",
            "tt0944947:1:2",
            "tt0944947:1:3",
            "tt0944947:1:5"
        ]
    );
    assert_eq!(
        db.watched_videos(PROFILE, "tt0133093").unwrap(),
        ["tt0133093"]
    );
    assert_eq!(report.deferred_watched_items, ["tt11280740"]);
    assert!(db.watched_videos(PROFILE, "tt11280740").unwrap().is_empty());

    // Unsupported fields name what was seen and not mapped.
    for field in [
        "user.trakt",
        "addon.transport.legacy_jsonrpc",
        "libraryItem.behaviorHints",
        "libraryItem.state.noNotif",
        "libraryItem.state.lastWatched",
        "libraryItem.state.timeWatched",
        "libraryItem.state.overallTimeWatched",
        "libraryItem.state.flaggedWatched",
        "libraryItem: 1 undecodable items, kept in the raw import",
    ] {
        assert!(
            report.unsupported_fields.iter().any(|f| f == field),
            "{field}"
        );
    }
    for mapped in [
        "libraryItem.state.timeOffset",
        "libraryItem.state.watched",
        "libraryItem.poster",
    ] {
        assert!(
            !report.unsupported_fields.iter().any(|f| f == mapped),
            "{mapped}"
        );
    }

    // The raw import is stored before mapping and holds no secrets.
    assert_eq!(db.count("import_raw"), 1);
    assert_eq!(report.raw_import_id, Some(1));
    let (source, payload): (String, String) = sql
        .query_row("SELECT source, payload FROM import_raw", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(source, "stremio");
    for secret in [KEY, TRAKT_TOKEN, "user@example.com"] {
        assert!(!payload.contains(secret), "{secret}");
    }
    let raw: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(raw["library"].as_array().unwrap().len(), 9);
    assert_eq!(
        raw["user"],
        json!({"_id": "5f0000000000000000000001", "traktLinked": true})
    );
}

#[tokio::test]
async fn watched_bitfields_wait_for_video_lists_and_apply_later() {
    let server = serve(account).await;
    let db = Db::new();
    let client = client_for(&server).await;

    let first = import_account(&client, &db, PROFILE, "Imported", &SeriesVideos::new())
        .await
        .unwrap();
    assert_eq!(first.deferred_watched_items, ["tt0944947", "tt11280740"]);
    assert_eq!(first.watched_marked, 1, "only the movie");
    assert!(db.watched_videos(PROFILE, "tt0944947").unwrap().is_empty());

    // Recover from the stored raw import once the video list is known.
    let payload: String = db
        .sql()
        .query_row("SELECT payload FROM import_raw", [], |r| r.get(0))
        .unwrap();
    let snapshot: Snapshot = serde_json::from_str(&payload).unwrap();
    let second = apply_snapshot(&db, PROFILE, "Imported", &snapshot, &got_only(), 1).unwrap();
    assert_eq!(second.deferred_watched_items, ["tt11280740"]);
    assert_eq!(second.watched_marked, 5);
    assert_eq!(db.watched_videos(PROFILE, "tt0944947").unwrap().len(), 4);
    assert_eq!(second.items_imported, first.items_imported);
    assert_eq!(second.in_progress, first.in_progress);

    // Idempotent: a third run changes nothing.
    let third = apply_snapshot(&db, PROFILE, "Imported", &snapshot, &got_only(), 2).unwrap();
    assert_eq!(third, second);
    assert_eq!(db.count("progress"), 4);
    assert_eq!(db.count("watched"), 5);
    assert_eq!(db.count("library_items"), 6);
}

#[tokio::test]
async fn a_video_list_without_the_anchor_stays_deferred() {
    let server = serve(account).await;
    let db = Db::new();
    let client = client_for(&server).await;
    // The addon now lists different ids for the series than the field was written with.
    let other: Vec<Video> = (1..=5)
        .map(|e| {
            serde_json::from_value(json!({"id": format!("kitsu:1:{e}"), "season": 1, "episode": e}))
                .unwrap()
        })
        .collect();
    let videos = SeriesVideos::from([("tt0944947".to_owned(), other)]);
    let report = import_account(&client, &db, PROFILE, "Imported", &videos)
        .await
        .unwrap();
    assert!(
        report
            .deferred_watched_items
            .contains(&"tt0944947".to_owned())
    );
    assert!(db.watched_videos(PROFILE, "tt0944947").unwrap().is_empty());
}

#[tokio::test]
async fn newer_local_edits_win_and_older_ones_do_not() {
    let server = serve(account).await;
    let db = Db::new();
    let client = client_for(&server).await;
    let local = |wall_ms: i64| {
        Hlc {
            wall_ms: wall_ms as u64,
            counter: 0,
            device: 7,
        }
        .encode()
    };

    // Newer than the Stremio item: stays. Older than it: replaced.
    db.set_progress(
        PROFILE,
        mq_library::ProgressRow {
            video_id: "tt1375666",
            item_id: "tt1375666",
            position_ms: 5_000_000,
            duration_ms: 8_880_000,
            updated_ms: M_NEW + 1,
        },
        &local(M_NEW + 1),
    )
    .unwrap();
    db.set_watched(
        PROFILE,
        "tt0133093",
        "tt0133093",
        false,
        &local(M_OLD - 1000),
    )
    .unwrap();

    import_account(&client, &db, PROFILE, "Imported", &got_only())
        .await
        .unwrap();

    assert_eq!(
        db.progress(PROFILE, "tt1375666").unwrap(),
        Some((5_000_000, 8_880_000))
    );
    assert_eq!(
        db.watched_videos(PROFILE, "tt0133093").unwrap(),
        ["tt0133093"]
    );
    let hlc: String = db
        .sql()
        .query_row(
            "SELECT hlc FROM progress WHERE video_id = 'tt0083658'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let hlc = Hlc::decode(&hlc).unwrap();
    assert_eq!((hlc.wall_ms as i64, hlc.counter, hlc.device), (M_NEW, 0, 0));
}

#[tokio::test]
async fn reimport_keeps_local_only_addons_and_local_disable() {
    let server = serve(account).await;
    let db = Db::new();
    let client = client_for(&server).await;
    let row = |url: &str, enabled: bool| mq_library::AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled,
        allow_private: false,
    };
    db.set_addons(
        PROFILE,
        &[
            row("https://local.example/manifest.json", true),
            row("https://v3-cinemeta.strem.io/manifest.json", false),
        ],
        "0001",
    )
    .unwrap();

    let report = import_account(&client, &db, PROFILE, "Imported", &got_only())
        .await
        .unwrap();

    assert_eq!(report.addons_imported, 4);
    let rows = db.addons(PROFILE).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows[0].transport_url,
        "https://v3-cinemeta.strem.io/manifest.json"
    );
    assert!(!rows[0].enabled, "a local disable survives");
    assert_eq!(rows[4].transport_url, "https://local.example/manifest.json");
}

#[tokio::test]
async fn a_dead_session_is_reported_and_nothing_is_written() {
    let server = serve(account).await;
    let err = StremioClient::login_with_auth_key(http(), server.endpoints(), AuthKey::new("stale"))
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ImportError::InvalidAuthKey), "{err:?}");
}

#[tokio::test]
async fn a_failed_fetch_leaves_the_library_untouched() {
    for (status, body) in [
        (200, json!({"error": {"code": 2, "message": "boom"}})),
        (500, json!({})),
    ] {
        let server = serve(move |path, b| {
            if path == "/api/datastoreGet" {
                (status, body.clone())
            } else {
                account(path, b)
            }
        })
        .await;
        let db = Db::new();
        let client = client_for(&server).await;
        let err = import_account(&client, &db, PROFILE, "Imported", &got_only())
            .await
            .unwrap_err();
        match (status, &err) {
            (200, ImportError::Api { code: 2, .. }) => {}
            (500, ImportError::Http(HttpError::Status { status: 500, .. })) => {}
            _ => panic!("{err:?}"),
        }
        for table in [
            "import_raw",
            "profiles",
            "addons",
            "library_items",
            "progress",
            "watched",
        ] {
            assert_eq!(db.count(table), 0, "{table}");
        }
    }
}

#[tokio::test]
async fn link_login_polls_until_authorized_and_never_prints_the_key() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let server = serve(move |path, _| {
        if path.starts_with("/api/v2/create") {
            (
                200,
                json!({"result": {
                    "success": true,
                    "code": "JL1C",
                    "link": "https://link.stremio.com/JL1C",
                    "qrcode": "https://link.stremio.com/qr?data=x",
                }}),
            )
        } else if counter.fetch_add(1, Ordering::Relaxed) < 2 {
            (
                200,
                json!({"error": {"code": 101, "message": "Invalid or expired token"}}),
            )
        } else {
            (200, json!({"result": {"authKey": KEY}}))
        }
    })
    .await;

    let login = LinkLogin::start(http(), server.endpoints()).await.unwrap();
    assert_eq!(login.code, "JL1C");
    assert_eq!(login.link_url, "https://link.stremio.com/JL1C");
    assert_eq!(
        login.qr_url.as_deref(),
        Some("https://link.stremio.com/qr?data=x")
    );

    assert_eq!(login.poll().await.unwrap(), LinkPoll::Pending);
    assert_eq!(login.poll().await.unwrap(), LinkPoll::Pending);
    let polled = login.poll().await.unwrap();
    let LinkPoll::Authorized { auth_key } = &polled else {
        panic!("{polled:?}");
    };
    assert_eq!(auth_key.expose(), KEY);
    assert!(!format!("{polled:?}").contains(KEY));
    assert!(!format!("{auth_key:?}").contains(KEY));

    let paths = server.paths();
    assert_eq!(paths[0], "/api/v2/create?type=Create");
    assert_eq!(paths[1], "/api/v2/read?type=Read&code=JL1C");
}

#[tokio::test]
async fn link_login_expires_locally() {
    // The service answers "not ready" for a stale code just like a waiting one.
    let server = serve(|path, _| {
        if path.starts_with("/api/v2/create") {
            (200, json!({"result": {"code": "AAAA", "link": "https://link.stremio.com/AAAA", "qrcode": "q"}}))
        } else {
            (200, json!({"error": {"code": 101, "message": "Invalid or expired token"}}))
        }
    })
    .await;
    let mut login = LinkLogin::start(http(), server.endpoints()).await.unwrap();
    assert_eq!(login.poll().await.unwrap(), LinkPoll::Pending);
    login.ttl = Duration::ZERO;
    assert_eq!(login.poll().await.unwrap(), LinkPoll::Expired);
}

#[tokio::test]
#[ignore = "network"]
async fn real_link_service_hands_out_a_code_and_reports_pending() {
    let login = LinkLogin::start(http(), Endpoints::default())
        .await
        .unwrap();
    assert_eq!(login.code.len(), 4, "{}", login.code);
    assert!(login.link_url.starts_with("https://link.stremio.com/"));
    assert!(login.qr_url.is_some());
    assert_eq!(login.poll().await.unwrap(), LinkPoll::Pending);
}

#[tokio::test]
#[ignore = "network"]
async fn real_api_rejects_a_bogus_key() {
    let err =
        StremioClient::login_with_auth_key(http(), Endpoints::default(), AuthKey::new("bogus"))
            .await
            .err()
            .unwrap();
    assert!(matches!(err, ImportError::InvalidAuthKey), "{err:?}");
}

#[tokio::test]
#[ignore = "network, needs MQ_STREMIO_AUTH_KEY"]
async fn imports_a_real_account() {
    let key = std::env::var("MQ_STREMIO_AUTH_KEY").expect("MQ_STREMIO_AUTH_KEY");
    let db = Db::new();
    let client =
        StremioClient::login_with_auth_key(http(), Endpoints::default(), AuthKey::new(key))
            .await
            .unwrap();
    let report: ImportReport =
        import_account(&client, &db, PROFILE, "Imported", &SeriesVideos::new())
            .await
            .unwrap();
    // Counts only: no titles, no URLs.
    eprintln!(
        "addons {} (skipped {}), items {}, in progress {}, watched {}, deferred {}, unsupported {:?}",
        report.addons_imported,
        report.addons_skipped.len(),
        report.items_imported,
        report.in_progress,
        report.watched_marked,
        report.deferred_watched_items.len(),
        report.unsupported_fields,
    );
    assert!(report.addons_imported > 0);
}
