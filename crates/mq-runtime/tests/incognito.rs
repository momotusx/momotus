//! Incognito in the core (plans/batch-10-incognito.md 2 and 7.1): what a
//! scope writes stays in its own file and goes at the end, with a positive
//! control first (the scope held each write), and the device's tables are
//! the same after as before.

use mq_library::{AddonRow, DownloadRecord, INCOGNITO, LibraryItemRow, OWNER, ProgressRow};
use mq_runtime::views::title::TitleState;
use mq_runtime::{
    Core, CoreConfig, DownloadsView, HomeView, PlayTarget, PlaybackPhase, PlaybackView,
    ProfilesView, SearchView, TitleFacts, TitleView, TraktConfig, TraktHosts, VersionsParams,
    VersionsView,
};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TITLE: &str = "tt0013442";
/// A second title, played halfway.
const OTHER: &str = "tt0000002";

/// An addon on loopback: a manifest with a search catalog and meta, the
/// meta of `TITLE`, each answer after `delay`; counts its requests.
async fn addon(id: &str, delay: Duration) -> (AddonRow, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": format!("Addon {id}"), "types": ["movie"],
        "resources": ["catalog", "meta", "stream"], "idPrefixes": ["tt"],
        "catalogs": [{"type": "movie", "id": "top", "extra": [{"name": "search", "isRequired": true}]}]
    })
    .to_string();
    let meta = serde_json::json!({"meta": {"id": TITLE, "type": "movie", "name": "Nosferatu"}})
        .to_string();
    let streams = serde_json::json!({"streams": [
        {"name": "Fixture\n1080p", "title": "Nosferatu.1922.1080p.WEB.x264-GROUP",
         "url": format!("{}/nosferatu.mkv", origin().await)}
    ]})
    .to_string();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let served = manifest.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            let (manifest, meta, streams) = (served.clone(), meta.clone(), streams.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                tokio::time::sleep(delay).await;
                let body = if head.contains("manifest.json") {
                    manifest
                } else if head.contains("/meta/") && head.contains(TITLE) {
                    meta
                } else if head.contains("/meta/") {
                    // No such title: an answer with no meta.
                    r#"{"metas":[]}"#.to_owned()
                } else if head.contains("/stream/") {
                    streams
                } else {
                    r#"{"metas":[]}"#.to_owned()
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest,
        position: 0,
        enabled: true,
        allow_private: true,
    };
    (row, hits)
}

/// A loopback origin answering every range with one byte of a film.
async fn origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let reply = "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM";
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

/// A device on disk with the owner's addon list, and its data folder.
async fn device() -> (Core, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let core = open(dir.path());
    let (row, _) = addon("host", Duration::ZERO).await;
    core.library()
        .set_addons(OWNER, &[row], core.clock())
        .unwrap();
    (core, dir)
}

fn open(dir: &Path) -> Core {
    Core::open(CoreConfig {
        data_dir: dir.to_path_buf(),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap()
}

/// Every row of every device table, sorted, but the throughput and network
/// keys (they name no title, plans/batch-10-incognito.md 2.3).
fn census(dir: &Path) -> BTreeMap<String, Vec<String>> {
    use rusqlite::types::Value;
    let c = rusqlite::Connection::open(dir.join("library.sqlite")).unwrap();
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut all = BTreeMap::new();
    for table in tables {
        let mut stmt = c.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let n = stmt.column_count();
        let mut rows: Vec<String> = stmt
            .query_map([], |r| {
                let cells: Vec<String> = (0..n)
                    .map(|i| format!("{:?}", r.get::<_, Value>(i).unwrap()))
                    .collect();
                Ok(cells.join("|"))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        if table == "device_state" {
            rows.retain(|r| !r.starts_with("Text(\"speed.") && !r.starts_with("Text(\"network\""));
        }
        rows.sort();
        all.insert(table, rows);
    }
    all
}

/// Every file under `dir` whose bytes hold `text`.
fn files_naming(dir: &Path, text: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if std::fs::read(&path)
                .is_ok_and(|b| b.windows(text.len()).any(|w| w == text.as_bytes()))
            {
                found.push(path);
            }
        }
    }
    found
}

fn scope_folders(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join("incognito"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

fn item() -> LibraryItemRow {
    LibraryItemRow {
        item_id: TITLE.into(),
        kind: "movie".into(),
        name: "Nosferatu".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    }
}

fn target(id: &str) -> PlayTarget {
    PlayTarget {
        kind: "movie".into(),
        item_id: id.into(),
        video_id: id.into(),
        name: Some("Nosferatu".into()),
        ..PlayTarget::default()
    }
}

fn film(id: &str) -> VersionsParams {
    VersionsParams {
        kind: "movie".into(),
        video_id: id.into(),
        target: Target {
            title: "Nosferatu".into(),
            year: Some(1922),
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts: TitleFacts::read(Some("94 min"), Some("1922"), None, None),
        device_language: None,
    }
}

/// Waits up to 10 s for `done`.
async fn until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("waited 10 s for {what}");
}

fn progress(position_ms: i64) -> ProgressRow<'static> {
    ProgressRow {
        video_id: TITLE,
        item_id: TITLE,
        position_ms,
        duration_ms: 1000,
        updated_ms: position_ms,
    }
}

/// The census (7.1): a session's worth of profile writes in Incognito,
/// each held by the scope until the end, and the device's tables the same
/// after it, with no row naming the title or `incognito`.
#[tokio::test]
async fn nothing_incognito_wrote_is_on_the_device_after_it() {
    let (host, dir) = device().await;
    let before = census(dir.path());
    let core = host.open_incognito().unwrap();
    assert_eq!(core.profile(), INCOGNITO);
    let (extra, _) = addon("extra", Duration::ZERO).await;

    let library = core.library();
    // The title page: Add to Library and Watched, once its meta is in.
    let title = TitleView::open(&core, "movie", TITLE);
    until("the title", || {
        title.snapshot().header.state == TitleState::Ready
    })
    .await;
    title.set_in_library(true).unwrap();
    title.set_watched(TITLE, true).unwrap();
    drop(title);
    // A version picked on Versions and played to the end; another title
    // played halfway: the player's own reports.
    let versions = VersionsView::open(&core, film(TITLE));
    until("the versions", || versions.snapshot().done).await;
    let shown = versions.snapshot();
    let key = (shown.recommended.as_ref().or(shown.rows.first()))
        .expect("a version")
        .key
        .clone();
    let pick = versions.pick(&key);
    let played = PlaybackView::open_picked(&core, pick, target(TITLE), &versions.plays());
    until("the film ready", || {
        played.snapshot().phase == PlaybackPhase::Ready
    })
    .await;
    played.report_progress(890_000, 900_000, false);
    drop((played, versions));
    let half = PlaybackView::open(&core, Vec::new(), target(OTHER));
    half.report_progress(450_000, 900_000, false);
    drop(half);
    core.remove_from_continue(OTHER).unwrap();
    let search = SearchView::open(&core);
    search.remember(Some("nosferatu")).unwrap();
    core.set_prefs(&serde_json::json!({"subtitleLanguages": ["de"], "autoplayNext": false}))
        .unwrap();
    core.install_addon(&extra.transport_url).await.unwrap();
    let copied = host.library().addons(OWNER).unwrap()[0]
        .transport_url
        .clone();
    core.remove_addon(&copied).unwrap();
    mq_runtime::speed::record_dead(library, &"a".repeat(40), mq_library::hlc::now_ms());
    core.fetch_meta("movie", TITLE).await.unwrap();

    // The positive control: the scope held them all.
    assert_eq!(
        library.progress(INCOGNITO, OTHER).unwrap(),
        Some((450_000, 900_000))
    );
    assert!(library.progress(INCOGNITO, TITLE).unwrap().is_some());
    assert!(library.stream_memory(INCOGNITO, TITLE).unwrap().is_some());
    assert_eq!(
        library.recent_searches(INCOGNITO, 5).unwrap(),
        ["nosferatu"]
    );
    assert!(library.is_in_library(INCOGNITO, TITLE).unwrap());
    let installed = core.installed_addons().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].manifest.id, "extra");

    assert!(core.end_incognito());
    assert!(scope_folders(dir.path()).is_empty());
    drop((search, core));
    assert_eq!(before, census(dir.path()));
    let device = census(dir.path());
    for rows in device.values() {
        for row in rows {
            assert!(!row.contains(TITLE) && !row.contains(INCOGNITO), "{row}");
        }
    }
    assert!(
        dir.path().join("incognito").is_dir(),
        "incognito/ itself stays"
    );
}

/// A write still running at the end lands nowhere (7.1): an install whose
/// manifest answers after Leave changes nothing anywhere, and the next
/// entry starts empty.
#[tokio::test]
async fn a_late_write_lands_nowhere_and_re_entry_starts_empty() {
    let (host, dir) = device().await;
    let before = census(dir.path());
    let hosts_addons = host.library().addons(OWNER).unwrap();
    let core = host.open_incognito().unwrap();
    core.library()
        .upsert_item(core.profile(), &item(), true, core.clock())
        .unwrap();
    core.library()
        .record_search(core.profile(), "nosferatu", 1)
        .unwrap();
    // Positive control.
    assert!(core.library().is_in_library(INCOGNITO, TITLE).unwrap());
    let (slow, hits) = addon("slowaddon", Duration::from_millis(400)).await;
    let installing = tokio::spawn({
        let core = core.clone();
        async move { core.install_addon(&slow.transport_url).await }
    });
    while hits.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(core.end_incognito());
    assert!(scope_folders(dir.path()).is_empty(), "gone at Leave");
    // Refused at the library's own choke point (`closed`).
    assert!(
        installing.await.unwrap().is_err(),
        "the late write is refused"
    );

    assert_eq!(host.library().addons(OWNER).unwrap(), hosts_addons);
    assert!(scope_folders(dir.path()).is_empty());
    assert_eq!(before, census(dir.path()));
    assert!(files_naming(dir.path(), "slowaddon").is_empty());

    let again = host.open_incognito().unwrap();
    assert!(
        again
            .library()
            .recent_searches(INCOGNITO, 5)
            .unwrap()
            .is_empty()
    );
    assert!(again.library().library(INCOGNITO).unwrap().is_empty());
    again.end_incognito();
}

/// The host's settings, addons and order come along; edits inside never
/// reach it; a host without an addon list gets `incognitoFailed`.
#[tokio::test]
async fn entry_copies_the_host_and_edits_stay_inside() {
    let (host, _dir) = device().await;
    host.set_prefs(&serde_json::json!({"subtitleLanguages": ["tr", "en"]}))
        .unwrap();
    let core = host.open_incognito().unwrap();
    assert_eq!(core.prefs(), host.prefs());
    assert_eq!(
        core.library().addons(INCOGNITO).unwrap(),
        host.library().addons(OWNER).unwrap()
    );
    core.set_prefs(&serde_json::json!({"subtitleLanguages": ["fr"]}))
        .unwrap();
    assert_ne!(core.prefs(), host.prefs());
    assert_eq!(
        host.prefs(),
        host.device().prefs(),
        "the host handle reads the device"
    );
    core.end_incognito();

    let bare = Core::temporary("p").unwrap();
    let refused = bare.open_incognito().err().expect("refused");
    assert_eq!(refused.code(), "incognitoFailed");
}

/// Launch recovery (2.3): a folder no live scope locks goes, a locked one
/// stays, `incognito/` stays, and the first session is told.
#[tokio::test]
async fn launch_recovery_removes_what_a_killed_run_left() {
    let dir = tempfile::tempdir().unwrap();
    let scopes = dir.path().join("incognito");
    for name in ["0000000000000001", "0000000000000002"] {
        std::fs::create_dir_all(scopes.join(name)).unwrap();
        std::fs::write(scopes.join(name).join("library.sqlite"), "x").unwrap();
    }
    let held = std::fs::File::create(scopes.join("0000000000000002").join("lock")).unwrap();
    held.lock().unwrap();
    let core = open(dir.path());
    assert!(!scopes.join("0000000000000001").exists());
    assert!(scopes.join("0000000000000002").exists());
    assert!(scopes.is_dir());
    assert!(*core.incognito_ended_unseen().borrow());
    drop(held);
}

/// A torrent line of a dead run goes at the next launch with its files,
/// before any engine: unless a download names the torrent (then only the
/// line), or its scope is live in another handle (then nothing).
#[tokio::test]
async fn launch_recovery_clears_ended_torrent_lines() {
    let dir = tempfile::tempdir().unwrap();
    let (gone, kept, live) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
    {
        let core = open(dir.path());
        let version = serde_json::json!({"key": "k", "chain": [],
            "torrent": {"hash": kept, "file": 0, "name": "f.mkv"}});
        core.library()
            .add_download(&download(&version.to_string()))
            .unwrap();
    }
    let torrents = dir.path().join("torrents");
    for hash in [&gone, &kept, &live] {
        std::fs::create_dir_all(torrents.join(hash)).unwrap();
        std::fs::write(torrents.join(format!("{hash}.have")), "x").unwrap();
        std::fs::write(torrents.join(format!("{hash}.torrent")), "x").unwrap();
    }
    let scopes = dir.path().join("incognito");
    std::fs::create_dir_all(scopes.join("00000000000000aa")).unwrap();
    let held = std::fs::File::create(scopes.join("00000000000000aa").join("lock")).unwrap();
    held.lock().unwrap();
    let lines =
        format!("00000000000000ff {gone}\n00000000000000ff {kept}\n00000000000000aa {live}\n");
    std::fs::write(scopes.join("torrents"), lines).unwrap();

    let _core = open(dir.path());
    for end in ["", ".have", ".torrent"] {
        assert!(!torrents.join(format!("{gone}{end}")).exists(), "{end}");
        assert!(torrents.join(format!("{kept}{end}")).exists(), "{end}");
        assert!(torrents.join(format!("{live}{end}")).exists(), "{end}");
    }
    let left = std::fs::read_to_string(scopes.join("torrents")).unwrap();
    assert_eq!(left, format!("00000000000000aa {live}\n"));
    drop(held);
}

fn download(version: &str) -> DownloadRecord {
    DownloadRecord {
        id: "d1".into(),
        added_by: OWNER.into(),
        kind: "movie".into(),
        item_id: "tt1".into(),
        video_id: "tt1".into(),
        season: None,
        episode: None,
        facts: "{}".into(),
        request: "{}".into(),
        version: Some(version.into()),
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

/// Home in Incognito has no Continue Watching row though the scope holds
/// a progress row; the host's Home shows the same row.
#[tokio::test]
async fn home_in_incognito_has_no_continue_watching() {
    let (host, _dir) = device().await;
    let core = host.open_incognito().unwrap();
    for c in [&host, &core] {
        c.library()
            .set_progress(c.profile(), progress(300), c.clock())
            .unwrap();
    }
    let rows = |view: &HomeView| -> Vec<String> {
        view.snapshot().rows.into_iter().map(|r| r.row_id).collect()
    };
    let shown = HomeView::open(&host);
    let hidden = HomeView::open(&core);
    for _ in 0..200 {
        if rows(&shown).iter().any(|r| r == "continue-watching") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(&shown).iter().any(|r| r == "continue-watching"));
    assert!(!rows(&hidden).iter().any(|r| r == "continue-watching"));
    core.end_incognito();
}

/// A download asked in Incognito is the host's, kept for every profile,
/// with no smart step; the Downloads view in Incognito renders as the host.
#[tokio::test]
async fn a_download_in_incognito_is_the_hosts() {
    let (host, _dir) = device().await;
    let core = host.open_incognito().unwrap();
    let ids = mq_runtime::downloads::add(
        &core,
        mq_runtime::downloads::AddRequest {
            kind: "movie".into(),
            item_id: TITLE.into(),
            scope: mq_runtime::downloads::Scope::Video,
            video_id: Some(TITLE.into()),
            season: None,
            params: serde_json::json!({}),
            pick: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(ids.len(), 1);
    let rows = host.library().downloads().unwrap();
    assert_eq!(rows[0].added_by, OWNER);
    assert!(rows[0].smart.is_none());
    let view = DownloadsView::open(&core);
    let snapshot = view.snapshot();
    assert_eq!(snapshot.profile, OWNER);
    assert_eq!(snapshot.downloading.len(), 1, "{snapshot:?}");
    core.end_incognito();
    assert_eq!(host.library().downloads().unwrap().len(), 1, "it stays");
}

/// The picker in Incognito (7.1, C P1 3): the device's profiles, `active`
/// is `incognito`, the host named, nothing asserted on the way.
#[tokio::test]
async fn the_picker_lists_the_real_profiles_in_incognito() {
    let (host, _dir) = device().await;
    let core = host.open_incognito().unwrap();
    let (_state, rx) =
        tokio::sync::watch::channel((INCOGNITO.to_owned(), false, Some(OWNER.to_owned()), false));
    let view = ProfilesView::open(&core, rx);
    let (snapshot, _) = view.subscribe();
    assert_eq!(snapshot.active, INCOGNITO);
    assert_eq!(snapshot.incognito_host.as_deref(), Some(OWNER));
    let ids: Vec<&str> = snapshot.profiles.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, [OWNER]);
    assert!(!snapshot.locked && !snapshot.kids);
    core.end_incognito();
}

/// Search in Incognito never asks Trakt (2.4); the host's search does.
#[tokio::test]
async fn search_in_incognito_never_asks_trakt() {
    let (host, _dir) = device().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            *count.lock().unwrap() += 1;
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            let reply = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    host.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.clone(),
            auth: base,
            local: true,
        },
    }));
    let core = host.open_incognito().unwrap();
    let inside = SearchView::open(&core);
    inside.set_query("nosferatu");
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(*asked.lock().unwrap(), 0);
    // The control: the host's search reaches the fake Trakt.
    let outside = SearchView::open(&host);
    outside.set_query("nosferatu");
    for _ in 0..200 {
        if *asked.lock().unwrap() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(*asked.lock().unwrap() > 0);
    core.end_incognito();
}

/// A device out of room answers `noSpace` with neutral words: the
/// protocol's message never says why (review P2-4).
#[tokio::test]
async fn no_space_says_neutral_words() {
    let (host, _dir) = device().await;
    host.set_free_bytes(1024);
    let refused = host.open_incognito().err().expect("refused");
    assert_eq!(refused.code(), "noSpace");
    let said = refused.to_string().to_lowercase();
    assert!(!said.contains("space") && !said.contains("full"), "{said}");
}

/// A scope dropped without its end still counts down and goes: `ping`'s
/// `liveScopes` never counts a scope that is gone (review P3-10).
#[tokio::test]
async fn a_scope_dropped_without_its_end_counts_down() {
    let (host, dir) = device().await;
    let core = host.open_incognito().unwrap();
    assert_eq!(host.live_scope_count(), 1);
    assert_eq!(scope_folders(dir.path()).len(), 1);
    drop(core);
    assert_eq!(host.live_scope_count(), 0);
    assert!(scope_folders(dir.path()).is_empty());
}

/// What a log line writes, for the test below.
#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Logs are files on disk (logcat, the Mac's): in Incognito no line names
/// the title asked for, at any level; the host's lines still do (review
/// P3-12).
#[tokio::test]
async fn incognito_logs_name_no_title() {
    let lines = Sink(Arc::new(Mutex::new(Vec::new())));
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    // The only subscriber this binary sets.
    tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
    // Call sites other tests reached before had no subscriber to ask.
    tracing::callsite::rebuild_interest_cache();
    let read = || String::from_utf8_lossy(&lines.0.lock().unwrap()).into_owned();
    let (host, _dir) = device().await;
    let core = host.open_incognito().unwrap();
    let (inside, outside) = ("tt7654321", "tt7654322");
    assert!(core.fetch_meta("movie", inside).await.is_err());
    assert!(host.fetch_meta("movie", outside).await.is_err());
    let logged = read();
    assert!(logged.contains(outside), "the control: {logged}");
    assert!(!logged.contains(inside), "{logged}");
    core.end_incognito();
}
