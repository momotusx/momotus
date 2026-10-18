//! Find addons (plans/m12-addon-discovery.md 3.1 to 3.3 and 4): the lists
//! from a local fixture in place of Cinemeta, the real answers of 2026-10-18.

use mq_addons::AddonError;
use mq_library::AddonRow;
use mq_protocol::Manifest;
use mq_runtime::views::addon_catalog::{
    check_listed_manifest, entries, install_listed_with, listable,
};
use mq_runtime::{
    AddonCatalogSnapshot, AddonCatalogView, AddonsView, Configuration, Core, Installed, ListState,
    ListedAddon, ListedEntry, ListedError, ListedFailure,
};
use serde_json::{Value, json};
use std::collections::HashMap;
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
    // Configuration from the manifest, the configure page beside it.
    let required = one(&snapshot, "community.aiostatus");
    assert_eq!(required.configuration, Configuration::Required);
    assert!(
        required
            .configure_url
            .as_deref()
            .unwrap()
            .ends_with("/configure")
    );
    let optional = one(&snapshot, "org.community.cast-search");
    assert_eq!(optional.configuration, Configuration::Optional);
    assert!(optional.configure_url.is_some());
    let plain = one(&snapshot, "community.anime.kitsu");
    assert_eq!(plain.configuration, Configuration::None);
    assert_eq!(plain.configure_url, None);
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
    // This link, on; the first duplicate id's link, off; a configured Trakt
    // (same id and host, another link), off, behind one that is on.
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
            ],
            &core.hlc(),
        )
        .unwrap();
    let snapshot = until(&view, |s| {
        one(s, "org.stremio.opensubtitlesv3").installed == Installed::On
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
    assert_eq!(state(&pick_a), (Installed::Off, None));
    // The other OpenSubtitles of that id is on another host: no copy.
    assert_eq!(state(&pick_b), (Installed::Absent, None));
    // Every Trakt copy is off: Turn on goes to the first in addon order.
    let trakt_key = one(&snapshot, "community.trakt-tv").key.clone();
    assert_eq!(
        state(&trakt_key),
        (Installed::CopyOff, Some(configured.clone()))
    );
    // One copy turned on: the card says Set up.
    core.set_addon_enabled(&other, true).unwrap();
    let snapshot = until(&view, |s| {
        one(s, "community.trakt-tv").installed == Installed::CopyOn
    })
    .await;
    assert_eq!(one(&snapshot, "community.trakt-tv").copy_key, None);
    // Removed: absent again.
    core.remove_addon(&subs).unwrap();
    until(&view, |s| {
        one(s, "org.stremio.opensubtitlesv3").installed == Installed::Absent
    })
    .await;
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
    let snapshot = until(&view, |s| one(s, id).installed == Installed::On).await;
    assert_eq!(one(&snapshot, id).installed, Installed::On);
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
