//! Find addons (plans/m12-addon-discovery.md 3.1 to 3.3 and 4): the lists
//! from a local fixture in place of Cinemeta, the real answers of 2026-10-18.

use mq_addons::AddonError;
use mq_library::AddonRow;
use mq_protocol::Manifest;
use mq_runtime::views::addon_catalog::{
    check_listed_manifest, entries, install_listed_with, listable,
};
use mq_runtime::{
    AddonCatalogSnapshot, AddonCatalogView, AddonList, AddonsView, Configuration, Core, CoreConfig,
    Installed, ListState, ListedAddon, ListedEntry, ListedError, ListedFailure,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const OFFICIAL: &str = include_str!("fixtures/addon_catalog/official.json");
const COMMUNITY: &str = include_str!("fixtures/addon_catalog/community.json");
const LISTS: &str = "/addon_catalog/all/";

/// A local Cinemeta: each path answers its status and body, cacheable for an
/// hour as Cinemeta's are.
struct Source {
    addr: std::net::SocketAddr,
    routes: Arc<Mutex<HashMap<String, (u16, String)>>>,
    hits: Arc<AtomicUsize>,
}

impl Source {
    async fn start(official: &str, community: &str) -> Source {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Arc<Mutex<HashMap<String, (u16, String)>>> = Arc::default();
        let hits = Arc::new(AtomicUsize::new(0));
        let source = Source {
            addr,
            routes: routes.clone(),
            hits: hits.clone(),
        };
        source.set("official", 200, official);
        source.set("community", 200, community);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (routes, hits) = (routes.clone(), hits.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                    hits.fetch_add(1, Ordering::SeqCst);
                    let (status, body) = routes
                        .lock()
                        .unwrap()
                        .get(&path)
                        .cloned()
                        .unwrap_or((404, String::new()));
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nCache-Control: public, max-age=3600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        source
    }

    fn set(&self, list: &str, status: u16, body: &str) {
        let path = format!("{LISTS}{list}.json");
        self.routes
            .lock()
            .unwrap()
            .insert(path, (status, body.to_owned()));
    }

    fn url(&self) -> String {
        format!("http://{}/manifest.json", self.addr)
    }
}

/// The snapshot once no list is loading.
async fn settled(view: &AddonCatalogView) -> AddonCatalogSnapshot {
    let started = Instant::now();
    loop {
        let snapshot = view.snapshot();
        if snapshot.lists.iter().all(|l| l.state != ListState::Loading) {
            return snapshot;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "{snapshot:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The snapshot once `ready` holds.
async fn until(
    view: &AddonCatalogView,
    ready: impl Fn(&AddonCatalogSnapshot) -> bool,
) -> AddonCatalogSnapshot {
    let started = Instant::now();
    loop {
        let snapshot = view.snapshot();
        if ready(&snapshot) {
            return snapshot;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "{snapshot:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn by_id<'a>(snapshot: &'a AddonCatalogSnapshot, id: &str) -> Vec<&'a ListedAddon> {
    snapshot
        .lists
        .iter()
        .flat_map(|l| &l.entries)
        .filter(|e| e.id == id)
        .collect()
}

fn one<'a>(snapshot: &'a AddonCatalogSnapshot, id: &str) -> &'a ListedAddon {
    let found = by_id(snapshot, id);
    assert_eq!(found.len(), 1, "{id}: {found:?}");
    found[0]
}

fn manifest(id: &str, hints: Value) -> Manifest {
    serde_json::from_value(json!({
        "id": id, "version": "1.0.0", "name": id, "resources": ["subtitles"],
        "types": ["movie"], "catalogs": [], "behaviorHints": hints,
    }))
    .unwrap()
}

fn row(url: &str, id: &str, enabled: bool) -> AddonRow {
    AddonRow {
        transport_url: url.into(),
        manifest_json: json!({
            "id": id, "version": "1.0.0", "name": id, "resources": [], "types": [], "catalogs": []
        })
        .to_string(),
        position: 0,
        enabled,
        allow_private: false,
    }
}

#[tokio::test]
async fn the_real_lists_show_only_what_is_safe() {
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let view = AddonCatalogView::open_from(&core, &source.url(), false);
    let snapshot = settled(&view).await;
    let [official, community] = &snapshot.lists[..] else {
        panic!("{snapshot:?}")
    };
    assert_eq!(
        (official.id.as_str(), official.name.as_str()),
        ("official", "Official")
    );
    assert_eq!(
        (community.id.as_str(), community.name.as_str()),
        ("community", "Community")
    );
    assert_eq!(official.state, ListState::Ready);
    assert_eq!(community.state, ListState::Ready);
    // The legacy OpenSubtitles transport and Local Files on http loopback
    // are refused; the five others show in the served order.
    let ids: Vec<&str> = official.entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "com.linvo.cinemeta",
            "com.linvo.stremiochannels",
            "org.stremio.watchhub",
            "org.stremio.pubdomainmovies",
            "org.stremio.opensubtitlesv3"
        ]
    );
    // The Trakt entry whose path carries tokens is refused; its plain twin
    // shows.
    assert_eq!(community.entries.len(), 95);
    assert!(
        community
            .entries
            .iter()
            .all(|e| !e.key.contains("redacted0"))
    );
    assert_eq!(by_id(&snapshot, "community.trakt-tv").len(), 1);
    // Both OpenSubtitles of the duplicate id show, on their own hosts.
    assert_eq!(by_id(&snapshot, "community.opensubtitles").len(), 2);
    // Names and descriptions as text: emoji, Hebrew, markup stripped.
    let names: Vec<&str> = community.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"🕒 Clockrr (Top Right)"), "{names:?}");
    assert!(names.contains(&"כתוביות בעברית"), "{names:?}");
    let dog = one(&snapshot, "org.stremio.doesTheDogDie");
    let words = dog.description.as_deref().unwrap();
    assert!(!words.contains('<') && !words.contains("href"), "{words}");
    assert!(words.contains("Open source at"), "{words}");
    // Configuration from the manifest; every entry has its configure page.
    let required = one(&snapshot, "community.aiostatus");
    assert_eq!(required.configuration, Configuration::Required);
    let optional = one(&snapshot, "org.community.cast-search");
    assert_eq!(optional.configuration, Configuration::Optional);
    let plain = one(&snapshot, "community.anime.kitsu");
    assert_eq!(plain.configuration, Configuration::None);
    for entry in [required, optional, plain] {
        let page = entry.key.strip_suffix("manifest.json").unwrap().to_owned() + "configure";
        assert_eq!(entry.configure_url, page);
    }
    assert_eq!(plain.provides, ["catalog", "meta", "subtitles"]);
    assert!(snapshot.lists.iter().flat_map(|l| &l.entries).all(|e| {
        e.key.starts_with("https://") && e.installed == Installed::Absent && e.copy_key.is_none()
    }));
}

#[test]
fn links_the_lists_may_show() {
    let ok = |raw: &str| listable(raw).map(|u| u.to_string());
    assert_eq!(
        ok("stremio://a.example/x/manifest.json").as_deref(),
        Some("https://a.example/x/manifest.json"),
        "stremio:// is normalized first"
    );
    assert_eq!(
        ok("https://a.example/manifest.json#frag").as_deref(),
        Some("https://a.example/manifest.json")
    );
    let refused = [
        "http://a.example/manifest.json",
        "https://user:pass@a.example/manifest.json",
        "https://user@a.example/manifest.json",
        &format!("https://a.example/{}/manifest.json", "t".repeat(65)),
        &format!("https://a.example/manifest.json?token={}", "t".repeat(65)),
        // A bare token as a query key, and tokens split over short parts.
        &format!("https://a.example/manifest.json?{}", "t".repeat(65)),
        &format!(
            "https://a.example/manifest.json?a={0}&b={0}&c={0}",
            "t".repeat(41)
        ),
        &format!(
            "https://a.example/{0}/{0}/{0}/manifest.json",
            "t".repeat(40)
        ),
        // A link over 512 bytes.
        &format!("https://{}.example/manifest.json", "h.".repeat(260)),
        "https://a.example/other.json",
        "https://a.example/manifest.json/other",
        "https://opensubtitles.strem.io/stremio/v1",
        "https://app.localhost/manifest.json",
        "https://localhost./manifest.json",
        "https://[::ffff:127.0.0.1]/manifest.json",
        "https://[::1]/manifest.json",
        "https://10.0.0.1/manifest.json",
        "https://192.168.1.2/manifest.json",
        "https://[fd00::1]/manifest.json",
        "https://127.0.0.1/manifest.json",
        "not a link",
    ];
    for raw in refused {
        assert_eq!(ok(raw), None, "{raw}");
    }
    // A segment or a value of exactly 64 characters is still a link.
    let segment = format!("https://a.example/{}/manifest.json", "t".repeat(64));
    assert!(ok(&segment).is_some());
    let value = format!("https://a.example/manifest.json?lang={}", "t".repeat(64));
    assert!(ok(&value).is_some());
    let key = format!("https://a.example/manifest.json?{}=1", "t".repeat(64));
    assert!(ok(&key).is_some());
    // A path or a query of exactly 128 characters is still a link.
    let path = format!("https://a.example/{}/manifest.json", "t".repeat(113));
    assert_eq!(Url::parse(&path).unwrap().path().len(), 128);
    assert!(ok(&path).is_none(), "its segment is over 64");
    let path = format!(
        "https://a.example/{}/{}/manifest.json",
        "t".repeat(56),
        "t".repeat(56)
    );
    assert_eq!(Url::parse(&path).unwrap().path().len(), 128);
    assert!(ok(&path).is_some());
    let query = format!(
        "https://a.example/manifest.json?a={}&b={}",
        "t".repeat(62),
        "t".repeat(61)
    );
    assert_eq!(Url::parse(&query).unwrap().query().unwrap().len(), 128);
    assert!(ok(&query).is_some());
}

fn raw(url: &str, id: &str, hints: Value) -> Value {
    json!({
        "transportUrl": url,
        "transportName": "http",
        "manifest": {
            "id": id, "version": "1.0.0", "name": format!("  {id}  "), "description": "<b>Bold</b>\n\n  text <i>here</i>",
            "resources": ["subtitles"], "types": ["movie"], "catalogs": [], "behaviorHints": hints,
        }
    })
}

#[test]
fn malformed_adult_and_duplicate_entries_are_skipped() {
    let list = [
        raw("https://a.example/manifest.json", "a", json!({})),
        json!({"transportUrl": "https://b.example/manifest.json"}),
        json!({"transportUrl": "https://c.example/manifest.json", "manifest": {"id": 3}}),
        json!("not an entry"),
        raw(
            "https://d.example/manifest.json",
            "d",
            json!({"adult": true}),
        ),
        raw("https://a.example/manifest.json#again", "a", json!({})),
        raw("https://e.example/manifest.json", "e", json!({})),
    ];
    let shown = entries(&list);
    let ids: Vec<&str> = shown.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["a", "e"]);
    assert_eq!(shown[0].name, "a", "trimmed");
    assert_eq!(shown[0].description.as_deref(), Some("Bold text here"));
}

#[test]
fn lists_and_texts_are_capped() {
    let list: Vec<Value> = (0..350)
        .map(|i| {
            raw(
                &format!("https://h{i}.example/manifest.json"),
                &format!("a{i}"),
                json!({}),
            )
        })
        .collect();
    assert_eq!(entries(&list).len(), 300);
    let mut long = raw("https://l.example/manifest.json", "l", json!({}));
    long["manifest"]["name"] = json!("n".repeat(200));
    long["manifest"]["description"] = json!("d ".repeat(400));
    let shown = entries(&[long]);
    assert_eq!(shown[0].name.chars().count(), 80);
    assert_eq!(shown[0].description.as_ref().unwrap().chars().count(), 300);
}

#[test]
fn descriptions_and_names_read_as_plain_text() {
    let with = |field: &str, text: &str| {
        let mut entry = raw("https://a.example/manifest.json", "a", json!({}));
        entry["manifest"][field] = json!(text);
        entries(&[entry]).pop()
    };
    let text = |description: &str| with("description", description)?.description;
    // A `<` that opens no tag is text, and the rest stays.
    assert_eq!(
        text("a < b and <3 more").as_deref(),
        Some("a < b and <3 more")
    );
    assert_eq!(
        text("open <unclosed rest").as_deref(),
        Some("open <unclosed rest")
    );
    assert_eq!(text("1 < 2 and 3 > 2").as_deref(), Some("1 < 2 and 3 > 2"));
    // Entities decoded once; markup they spell is text.
    assert_eq!(
        text("Tom &amp; Jerry &amp;lt; &#39;x&#x27; &lt;b&gt; &nbsp;end &bogus; a&b").as_deref(),
        Some("Tom & Jerry &lt; 'x' <b> end &bogus; a&b")
    );
    // Tags and control characters are spaces.
    assert_eq!(
        text("<p>one</p><p>two\u{0}three</p>").as_deref(),
        Some("one two three")
    );
    assert_eq!(text("<br/>  "), None);
    // Names: whitespace collapsed; an empty one drops the entry.
    let name = |name: &str| with("name", name).map(|e| e.name);
    assert_eq!(
        name("  Open\n\tSubtitles \u{7} v3 ").as_deref(),
        Some("Open Subtitles v3")
    );
    assert_eq!(name(" \n\t "), None);
}

#[test]
fn a_crafted_entry_never_outgrows_a_push() {
    let good = raw("https://g.example/manifest.json", "g", json!({}));
    let with = |field: &str, value: Value| {
        let mut entry = raw("https://c.example/manifest.json", "c", json!({}));
        entry["manifest"][field] = value;
        entry
    };
    let words = |n: usize, len: usize| json!(vec!["w".repeat(len); n]);
    // Over a field cap: dropped, the rest stay.
    let crafted = [
        with("id", json!("i".repeat(129))),
        with("types", words(17, 5)),
        with("types", words(1, 33)),
        with("resources", words(17, 5)),
        with("resources", words(1, 33)),
    ];
    for entry in crafted {
        let shown = entries(&[entry.clone(), good.clone()]);
        let ids: Vec<&str> = shown.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["g"], "{entry}");
    }
    assert_eq!(entries(&[with("id", json!("i".repeat(128)))]).len(), 1);
    // Within every field cap, with text that grows as JSON (four-byte
    // characters, escaped control characters).
    let big = |i: usize, word: &str| {
        let url = format!("https://h{i}.example/manifest.json");
        let mut entry = raw(&url, &format!("{i:0>128}"), json!({}));
        entry["manifest"]["name"] = json!("\u{10348}".repeat(80));
        entry["manifest"]["description"] = json!("\u{10348}".repeat(300));
        entry["manifest"]["types"] = json!(vec![word; 16]);
        entry["manifest"]["resources"] = json!(vec![word; 16]);
        entry
    };
    // Over 4 KiB as JSON: dropped.
    assert!(entries(&[big(0, &"\u{1}".repeat(32))]).is_empty());
    // Two full lists of the largest shown fit one push of the bridge (4 MiB).
    let list: Vec<Value> = (0..300).map(|i| big(i, &"w".repeat(32))).collect();
    let shown = entries(&list);
    assert_eq!(shown.len(), 300);
    let list = |id: &str| AddonList {
        id: id.into(),
        name: id.into(),
        state: ListState::Ready,
        entries: shown.clone(),
    };
    let snapshot = AddonCatalogSnapshot {
        lists: vec![list("official"), list("community")],
    };
    let push = json!({"sub": 1, "updates": [snapshot]}).to_string();
    assert!(push.len() < 4 * 1024 * 1024, "{}", push.len());
}

#[tokio::test]
async fn a_list_over_a_megabyte_is_not_read() {
    let entry = raw("https://n.example/manifest.json", "n", json!({}));
    let padded = json!({"addons": [entry], "pad": "x".repeat(1024 * 1024)}).to_string();
    let source = Source::start(&padded, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let snapshot = settled(&AddonCatalogView::open_from(&core, &source.url(), false)).await;
    assert_eq!(
        snapshot.lists[0].state,
        ListState::Failed {
            words: "Cinemeta sent a list Marquee cannot read".into()
        }
    );
    assert_eq!(snapshot.lists[1].state, ListState::Ready);
}

#[tokio::test]
async fn installed_states_follow_the_profiles_addons() {
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let view = AddonCatalogView::open_from(&core, &source.url(), false);
    let snapshot = settled(&view).await;
    let subs = one(&snapshot, "org.stremio.opensubtitlesv3").key.clone();
    let pick = by_id(&snapshot, "community.opensubtitles");
    let (pick_a, pick_b) = (pick[0].key.clone(), pick[1].key.clone());
    let trakt = one(&snapshot, "community.trakt-tv");
    let trakt_host = trakt.host.clone();
    let cinemeta = one(&snapshot, "com.linvo.cinemeta").key.clone();
    let hits = source.hits.load(Ordering::SeqCst);
    // This link, on; the first duplicate id's link, off; a configured Trakt
    // (same id and host, another link), off, behind one that is on; Cinemeta
    // with a stored manifest that no longer reads.
    let configured = format!("https://{trakt_host}/eyJ1c2VyIjoiYSJ9/manifest.json");
    let other = format!("https://{trakt_host}/eyJ1c2VyIjoiYiJ9/manifest.json");
    core.library()
        .set_addons(
            "p",
            &[
                row(&subs, "org.stremio.opensubtitlesv3", true),
                row(&pick_a, "community.opensubtitles", false),
                row(&configured, "community.trakt-tv", false),
                row(&other, "community.trakt-tv", false),
                AddonRow {
                    manifest_json: "not a manifest".into(),
                    ..row(&cinemeta, "com.linvo.cinemeta", true)
                },
            ],
            &core.hlc(),
        )
        .unwrap();
    // Once the change reached the view (any state but absent), every state
    // is checked exactly.
    let snapshot = until(&view, |s| {
        one(s, "org.stremio.opensubtitlesv3").installed != Installed::Absent
    })
    .await;
    let state = |key: &str| {
        let entry = snapshot
            .lists
            .iter()
            .flat_map(|l| &l.entries)
            .find(|e| e.key == key)
            .unwrap();
        (entry.installed, entry.copy_key.clone())
    };
    assert_eq!(state(&subs), (Installed::On, None));
    // An unreadable manifest still matches its exact link.
    assert_eq!(state(&cinemeta), (Installed::On, None));
    assert_eq!(state(&pick_a), (Installed::Off, None));
    // The other OpenSubtitles of that id is on another host: no copy.
    assert_eq!(state(&pick_b), (Installed::Absent, None));
    // Every Trakt copy is off: Turn on goes to the first in addon order.
    let trakt_key = one(&snapshot, "community.trakt-tv").key.clone();
    assert_eq!(
        state(&trakt_key),
        (Installed::CopyOff, Some(configured.clone()))
    );
    // One copy turned on, then OpenSubtitles removed: once the later change
    // shows, the Trakt card says Set up.
    core.set_addon_enabled(&other, true).unwrap();
    core.remove_addon(&subs).unwrap();
    let snapshot = until(&view, |s| {
        one(s, "org.stremio.opensubtitlesv3").installed == Installed::Absent
    })
    .await;
    let trakt = one(&snapshot, "community.trakt-tv");
    assert_eq!(
        (trakt.installed, trakt.copy_key.clone()),
        (Installed::CopyOn, None)
    );
    // Addon changes recompute states only: the lists were not asked again.
    assert_eq!(source.hits.load(Ordering::SeqCst), hits);
}

#[tokio::test]
async fn unreadable_addons_fail_the_lists_instead_of_reading_absent() {
    let dir = std::env::temp_dir().join(format!("mq-m12-unreadable-{}", std::process::id()));
    let core = Core::open(CoreConfig {
        data_dir: dir.clone(),
        profile_id: String::new(),
        device_id: 0,
    })
    .unwrap();
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let row = row("https://a.example/manifest.json", "a", true);
    core.library()
        .set_addons(core.profile(), &[row], &core.hlc())
        .unwrap();
    // A row the library can no longer read.
    let db = rusqlite::Connection::open(dir.join("library.sqlite")).unwrap();
    db.execute("UPDATE addons SET enabled = 'unreadable'", [])
        .unwrap();
    let snapshot = settled(&AddonCatalogView::open_from(&core, &source.url(), false)).await;
    for list in &snapshot.lists {
        assert_eq!(
            list.state,
            ListState::Failed {
                words: "Marquee could not read your addons".into()
            }
        );
        assert!(list.entries.is_empty());
    }
    drop((db, core));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fresh_waits_for_the_network_and_replaces_a_cached_list() {
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let first = settled(&AddonCatalogView::open_from(&core, &source.url(), false)).await;
    assert_eq!(first.lists[0].entries.len(), 5);
    // Cinemeta's list changes: the cache still answers the plain open.
    let one_entry = json!({"addons": [raw("https://n.example/manifest.json", "new", json!({}))]});
    source.set("official", 200, &one_entry.to_string());
    let hits = source.hits.load(Ordering::SeqCst);
    let cached = settled(&AddonCatalogView::open_from(&core, &source.url(), false)).await;
    assert_eq!(cached.lists[0].entries.len(), 5);
    assert_eq!(
        source.hits.load(Ordering::SeqCst),
        hits,
        "served from the cache"
    );
    // Refresh waits for the network.
    let fresh = settled(&AddonCatalogView::open_from(&core, &source.url(), true)).await;
    let ids: Vec<&str> = fresh.lists[0]
        .entries
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(ids, ["new"]);
    // A failed refresh is a failed list, never the cached one.
    source.set("official", 503, "");
    let failed = settled(&AddonCatalogView::open_from(&core, &source.url(), true)).await;
    assert_eq!(
        failed.lists[0].state,
        ListState::Failed {
            words: "Cinemeta answered with an error (503)".into()
        }
    );
    assert!(failed.lists[0].entries.is_empty());
    assert_eq!(failed.lists[1].state, ListState::Ready);
}

#[tokio::test]
async fn an_unreadable_list_fails_alone() {
    let source = Source::start("not json", COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let snapshot = settled(&AddonCatalogView::open_from(&core, &source.url(), false)).await;
    assert_eq!(
        snapshot.lists[0].state,
        ListState::Failed {
            words: "Cinemeta sent a list Marquee cannot read".into()
        }
    );
    assert_eq!(snapshot.lists[1].state, ListState::Ready);
}

#[test]
fn the_fetched_manifest_must_be_the_listed_addon() {
    let entry = ListedEntry {
        key: Url::parse("https://a.example/manifest.json").unwrap(),
        id: "a".into(),
    };
    assert_eq!(
        check_listed_manifest(&entry, &manifest("a", json!({}))),
        Ok(())
    );
    let configurable = manifest("a", json!({"configurable": true}));
    assert_eq!(check_listed_manifest(&entry, &configurable), Ok(()));
    let cases = [
        (manifest("b", json!({})), ListedFailure::Mismatch),
        (
            manifest("a", json!({"configurationRequired": true})),
            ListedFailure::NeedsSetup,
        ),
        (manifest("a", json!({"adult": true})), ListedFailure::Adult),
    ];
    for (fetched, failure) in cases {
        assert_eq!(check_listed_manifest(&entry, &fetched), Err(failure));
    }
}

fn refused(result: Result<(String, String), ListedError>) -> ListedFailure {
    match result {
        Err(ListedError::Refused(failure)) => failure,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_listed_install_writes_only_a_checked_manifest() {
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let view = AddonCatalogView::open_from(&core, &source.url(), false);
    let snapshot = settled(&view).await;
    let key = one(&snapshot, "org.stremio.opensubtitlesv3").key.clone();
    let entry = view.listed_entry(&key).unwrap();
    assert_eq!(entry.id, "org.stremio.opensubtitlesv3");
    // A key the open list does not show.
    let elsewhere = "https://elsewhere.example/manifest.json";
    assert_eq!(
        view.listed_entry(elsewhere).unwrap_err(),
        ListedFailure::NotListed
    );
    // Refused manifests and failed fetches write nothing.
    let id = "org.stremio.opensubtitlesv3";
    let refusals = [
        (manifest("other.id", json!({})), ListedFailure::Mismatch),
        (
            manifest(id, json!({"configurationRequired": true})),
            ListedFailure::NeedsSetup,
        ),
        (manifest(id, json!({"adult": true})), ListedFailure::Adult),
    ];
    for (fetched, failure) in refusals {
        let result = install_listed_with(&core, &entry, |_| async { Ok(fetched) }).await;
        assert_eq!(refused(result), failure);
    }
    let timeout = install_listed_with(&core, &entry, |_| async {
        Err::<Manifest, _>(AddonError::Http(mq_http::HttpError::Timeout(
            Duration::from_secs(10),
        )))
    })
    .await;
    assert_eq!(refused(timeout), ListedFailure::Timeout);
    let blocked = install_listed_with(&core, &entry, |_| async {
        Err::<Manifest, _>(AddonError::Http(mq_http::HttpError::BlockedPrivate(
            "10.0.0.1".into(),
        )))
    })
    .await;
    assert_eq!(refused(blocked), ListedFailure::Unreachable);
    assert!(core.library().addons("p").unwrap().is_empty());
    // The checked manifest is written, never with private reach, at the
    // link the list showed; a second install of it (a race) is installed.
    let asked = Arc::new(Mutex::new(Vec::new()));
    for _ in 0..2 {
        let asked = asked.clone();
        let installed = install_listed_with(&core, &entry, |url| async move {
            asked.lock().unwrap().push(url.to_string());
            Ok(manifest("org.stremio.opensubtitlesv3", json!({})))
        })
        .await
        .unwrap();
        assert_eq!(installed.0, "org.stremio.opensubtitlesv3");
    }
    assert_eq!(*asked.lock().unwrap(), [key.clone(), key.clone()]);
    let rows = core.library().addons("p").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].transport_url, key);
    assert!(!rows[0].allow_private && rows[0].enabled);
    // The manifest written is the one fetched and checked.
    let written: Value = serde_json::from_str(&rows[0].manifest_json).unwrap();
    assert_eq!(
        written,
        serde_json::to_value(manifest(id, json!({}))).unwrap()
    );
    let snapshot = until(&view, |s| one(s, id).installed == Installed::On).await;
    assert_eq!(one(&snapshot, id).installed, Installed::On);
}

/// A local addon answering its manifest, and the name that stands for it as
/// a public host on the listed path (`set_public`).
async fn manifest_origin(name: &str, manifest: Value) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let body = manifest.to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let localhost = std::net::Ipv4Addr::LOCALHOST.into();
    mq_http::test_dns::set_public(name, &[localhost]);
    (format!("{}:{}", name, addr.port()), hits)
}

#[tokio::test]
async fn the_listed_path_fetches_public_addresses_only() {
    let core = Core::temporary("p").unwrap();
    let (origin, hits) = manifest_origin("listed.m12.test", listed_manifest("listed")).await;
    // The addon at its private address: never fetched, nothing written.
    let port = origin.rsplit(':').next().unwrap();
    let private = ListedEntry {
        key: Url::parse(&format!("http://127.0.0.1:{port}/manifest.json")).unwrap(),
        id: "listed".into(),
    };
    let result = core.install_listed(&private).await;
    assert_eq!(refused(result), ListedFailure::Unreachable);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(core.library().addons("p").unwrap().is_empty());
    // At a name standing for a public host: fetched, checked, written.
    let entry = ListedEntry {
        key: Url::parse(&format!("http://{origin}/manifest.json")).unwrap(),
        id: "listed".into(),
    };
    let installed = core.install_listed(&entry).await.unwrap();
    assert_eq!(installed.0, "listed");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let rows = core.library().addons("p").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].allow_private);
}

fn listed_manifest(id: &str) -> Value {
    json!({
        "id": id, "version": "1.0.0", "name": "Listed", "resources": ["subtitles"],
        "types": ["movie"], "catalogs": []
    })
}

#[tokio::test]
async fn a_listed_manifest_over_256_kib_is_not_installed() {
    let core = Core::temporary("p").unwrap();
    let mut big = listed_manifest("big");
    big["description"] = json!("d".repeat(256 * 1024));
    let (origin, hits) = manifest_origin("big.m12.test", big).await;
    let entry = ListedEntry {
        key: Url::parse(&format!("http://{origin}/manifest.json")).unwrap(),
        id: "big".into(),
    };
    assert_eq!(
        refused(core.install_listed(&entry).await),
        ListedFailure::Unreachable
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(core.library().addons("p").unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_addon_edits_all_land() {
    let core = Core::temporary("p").unwrap();
    let tasks: Vec<_> = (0..8)
        .map(|t| {
            let core = core.clone();
            tokio::spawn(async move {
                for i in 0..20 {
                    let id = format!("a{t}-{i}");
                    let entry = ListedEntry {
                        key: Url::parse(&format!("https://{id}.example/manifest.json")).unwrap(),
                        id: id.clone(),
                    };
                    install_listed_with(
                        &core,
                        &entry,
                        |_| async move { Ok(manifest(&id, json!({}))) },
                    )
                    .await
                    .unwrap();
                }
            })
        })
        .collect();
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(core.library().addons("p").unwrap().len(), 160);
}

#[tokio::test]
async fn commands_find_an_imported_row_by_its_canonical_link() {
    let core = Core::temporary("p").unwrap();
    let key = "https://h.example/manifest.json";
    let imported = format!("{key}#imported");
    let other = "https://other.example/manifest.json";
    let states = |core: &Core| -> Vec<(String, bool)> {
        let rows = core.library().addons("p").unwrap();
        rows.into_iter()
            .map(|r| (r.transport_url, r.enabled))
            .collect()
    };
    // The Stremio importer keeps fragments; the lists show the link
    // without one, and the card's commands use that.
    core.library()
        .set_addons(
            "p",
            &[row(&imported, "a", false), row(other, "b", true)],
            &core.hlc(),
        )
        .unwrap();
    core.set_addon_enabled(key, true).unwrap();
    core.move_addon(key, 1).unwrap();
    assert_eq!(
        states(&core),
        [(other.to_owned(), true), (imported.clone(), true)]
    );
    // Installing the listed link finds it installed.
    let entry = ListedEntry {
        key: Url::parse(key).unwrap(),
        id: "a".into(),
    };
    install_listed_with(&core, &entry, |_| async { Ok(manifest("a", json!({}))) })
        .await
        .unwrap();
    assert_eq!(states(&core).len(), 2);
    core.remove_addon(key).unwrap();
    assert_eq!(states(&core), [(other.to_owned(), true)]);
    // The exact link comes first: with both stored, each answers to its own.
    core.library()
        .set_addons(
            "p",
            &[row(&imported, "a", false), row(key, "a", false)],
            &core.hlc(),
        )
        .unwrap();
    core.set_addon_enabled(key, true).unwrap();
    assert_eq!(
        states(&core),
        [(imported.clone(), false), (key.to_owned(), true)]
    );
}

/// Takes every slot the core's client gives `host` (6, `ClientConfig`'s
/// default) with answers whose bodies never end, then stops the clock: a
/// request waiting for a slot now waits on virtual time, which jumps to the
/// next timer whenever nothing else can run.
async fn hold_slots(core: &Core, host: &str) -> Vec<mq_http::StreamingResponse> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
                .await;
            held.push(socket);
        }
    });
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    mq_http::test_dns::set(host, &[localhost]);
    let http = core.http().scoped(&[localhost]);
    let mut held = Vec::new();
    for _ in 0..6 {
        let url = format!("http://{host}:{port}/");
        held.push(http.stream(&url, None, &Default::default()).await.unwrap());
    }
    tokio::time::pause();
    held
}

#[tokio::test]
async fn a_list_waiting_for_a_slot_fails_at_its_deadline() {
    let source = Source::start(OFFICIAL, COMMUNITY).await;
    let core = Core::temporary("p").unwrap();
    let _held = hold_slots(&core, "127.0.0.1").await;
    let started = tokio::time::Instant::now();
    let view = AddonCatalogView::open_from(&core, &source.url(), false);
    let snapshot = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let snapshot = view.snapshot();
            if snapshot.lists.iter().all(|l| l.state != ListState::Loading) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the lists end by their deadline");
    for list in &snapshot.lists {
        assert_eq!(
            list.state,
            ListState::Failed {
                words: "Cinemeta did not answer in time".into()
            }
        );
    }
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert_eq!(source.hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_listed_install_waiting_for_a_slot_fails_at_its_deadline() {
    let core = Core::temporary("p").unwrap();
    let _held = hold_slots(&core, "slots.m12.test").await;
    let entry = ListedEntry {
        key: Url::parse("https://slots.m12.test/manifest.json").unwrap(),
        id: "slots".into(),
    };
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(60), core.install_listed(&entry))
        .await
        .expect("the install ends by its deadline");
    assert_eq!(refused(result), ListedFailure::Timeout);
    assert!(started.elapsed() >= Duration::from_secs(10));
}

#[tokio::test]
async fn closing_the_view_ends_its_requests() {
    // A Cinemeta that never answers, telling when each connection opens
    // and when the client closes it.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (opened_tx, mut opened) = tokio::sync::mpsc::unbounded_channel();
    let (closed_tx, mut closed) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let _ = opened_tx.send(());
            let closed = closed_tx.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                }
                let _ = closed.send(());
            });
        }
    });
    let core = Core::temporary("p").unwrap();
    let view = AddonCatalogView::open_from(&core, &format!("http://{addr}/manifest.json"), false);
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), opened.recv())
            .await
            .expect("both lists are asked");
    }
    drop(view);
    // Both requests end with the view, long before their 10 s deadline.
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(2), closed.recv())
            .await
            .expect("the request ended with the view");
    }
}

#[tokio::test]
async fn installed_addons_carry_their_configure_page() {
    let core = Core::temporary("p").unwrap();
    let configurable = |url: &str, hints: Value| AddonRow {
        transport_url: url.into(),
        manifest_json: json!({
            "id": url, "version": "1.0.0", "name": "x", "resources": [], "types": [],
            "catalogs": [], "behaviorHints": hints,
        })
        .to_string(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    core.library()
        .set_addons(
            "p",
            &[
                configurable(
                    "https://a.example/cfg/manifest.json?next=/x/manifest.json",
                    json!({"configurable": true}),
                ),
                configurable(
                    "https://b.example/eyJ1c2VyIjoiYSJ9/manifest.json",
                    json!({"configurationRequired": true}),
                ),
                configurable("https://c.example/manifest.json", json!({})),
            ],
            &core.hlc(),
        )
        .unwrap();
    let pages: Vec<Option<String>> = AddonsView::open(&core)
        .snapshot()
        .addons
        .into_iter()
        .map(|a| a.configure_url)
        .collect();
    assert_eq!(
        pages,
        [
            Some("https://a.example/cfg/configure?next=/x/manifest.json".into()),
            Some("https://b.example/eyJ1c2VyIjoiYSJ9/configure".into()),
            None,
        ]
    );
}

/// Live: the two lists from Cinemeta itself, and OpenSubtitles v3 installed
/// from Official into a temporary core through the real listed path. Prints
/// one JSON line per check for bench/m12/. Run with `--ignored`.
#[tokio::test]
#[ignore = "network"]
async fn live_lists_and_install() {
    let core = Core::temporary("p").unwrap();
    let started = Instant::now();
    let view = AddonCatalogView::open(&core, true);
    let snapshot = settled(&view).await;
    let took = started.elapsed().as_millis();
    let served: Vec<usize> = {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let mut counts = Vec::new();
        for list in ["official", "community"] {
            let url = format!("https://v3-cinemeta.strem.io{LISTS}{list}.json");
            let body: Value = http.get_json(&url, &Default::default()).await.unwrap();
            counts.push(body["addons"].as_array().map_or(0, Vec::len));
        }
        counts
    };
    for (list, served) in snapshot.lists.iter().zip(served) {
        let count = |c: Configuration| list.entries.iter().filter(|e| e.configuration == c).count();
        println!(
            "{}",
            json!({
                "list": list.id, "state": format!("{:?}", list.state), "served": served,
                "shown": list.entries.len(), "refused": served - list.entries.len(),
                "required": count(Configuration::Required), "optional": count(Configuration::Optional),
                "subtitles": list.entries.iter().filter(|e| e.provides.iter().any(|p| p == "subtitles")).count(),
                "stream": list.entries.iter().filter(|e| e.provides.iter().any(|p| p == "stream")).count(),
                "openMs": took,
            })
        );
    }
    let key = one(&snapshot, "org.stremio.opensubtitlesv3").key.clone();
    let entry = view.listed_entry(&key).unwrap();
    let started = Instant::now();
    let installed = core.install_listed(&entry).await.unwrap();
    let rows = core.library().addons("p").unwrap();
    println!(
        "{}",
        json!({
            "install": installed.0, "name": installed.1, "ms": started.elapsed().as_millis(),
            "rows": rows.len(), "allowPrivate": rows[0].allow_private,
            "linkIsListed": rows[0].transport_url == key,
        })
    );
    let snapshot = until(&view, |s| {
        one(s, "org.stremio.opensubtitlesv3").installed == Installed::On
    })
    .await;
    assert_eq!(
        one(&snapshot, "org.stremio.opensubtitlesv3").installed,
        Installed::On
    );
}
