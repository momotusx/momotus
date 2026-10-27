//! Nuvio plugin repositories in the core (plans/plugins-and-marketplace.md 7,
//! the plugin pass of 4.4, R7's defaults), against our own fixture
//! repository (`mq-plugins/tests/fixtures/plugin-repo`) served over https on
//! loopback. No third-party repository is asked.

use mq_library::OWNER;
use mq_protocol::Source;
use mq_runtime::defaults::{DefaultChoice, DefaultInstalled};
use mq_runtime::{Core, RowOrigin, TitleFacts, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MANIFEST: &str = include_str!("../../mq-plugins/tests/fixtures/plugin-repo/manifest.json");
const SCRIPT: &str = include_str!("../../mq-plugins/tests/fixtures/plugin-repo/test-film.js");
const HOST: &str = "plugins.c-plugins.test";

/// The repository on loopback, with every path it was asked.
struct Fixture {
    base: String,
    asked: Arc<Mutex<Vec<String>>>,
}

fn reply(status: u16, kind: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

impl Fixture {
    fn start() -> Fixture {
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        // The directory and the manifest host it may list, on this fixture too.
        for name in [HOST, "nuvioplugins.com", "raw.githubusercontent.com"] {
            mq_http::test_dns::set_public(name, &[loopback]);
        }
        let asked: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = asked.clone();
        let base: Arc<Mutex<String>> = Arc::default();
        let at = base.clone();
        let port = mq_http::test_dns::https_requests(move |r| {
            log.lock().unwrap().push(r.path.clone());
            let base = at.lock().unwrap().clone();
            match r.path.as_str() {
                "/repo/manifest.json" | "/momotus/fixture/main/manifest.json" => {
                    reply(200, "application/json", MANIFEST)
                }
                // The directory page: our repository on the one host it may list, twice, and a
                // link elsewhere it refuses.
                "/dir" => {
                    let port = base.rsplit(':').next().unwrap_or_default();
                    let ours = format!(
                        "https://raw.githubusercontent.com:{port}/momotus/fixture/main/manifest.json"
                    );
                    let page = format!(
                        "<a href=\"{ours}\">ours</a> <code>{ours}</code> <a href=\"https://{HOST}:{port}/repo/manifest.json\">elsewhere</a>"
                    );
                    reply(200, "text/html", &page)
                }
                "/repo/test-film.js" => {
                    reply(200, "text/javascript", &SCRIPT.replace("{{BASE}}", &base))
                }
                // The provider finds nothing by the TMDB id `tmdb_77` gives.
                "/lookup/movie/77" => reply(404, "text/plain", ""),
                p if p.starts_with("/lookup/") => {
                    reply(200, "application/json", r#"{"title": "Fixture Film"}"#)
                }
                _ => reply(404, "text/plain", ""),
            }
        });
        let url = format!("https://{HOST}:{port}");
        *base.lock().unwrap() = url.clone();
        Fixture { base: url, asked }
    }

    fn repo(&self) -> String {
        format!("{}/repo/manifest.json", self.base)
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

fn film(id: &str) -> VersionsParams {
    VersionsParams {
        kind: "movie".into(),
        video_id: id.into(),
        target: Target {
            title: "Fixture Film".into(),
            year: Some(2001),
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts: TitleFacts::read(Some("90 min"), Some("2001"), None, None),
        device_language: None,
    }
}

async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..600 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("waited 15 s for {what}");
}

async fn settled(core: &Core, params: VersionsParams) -> VersionsView {
    let view = VersionsView::open(core, params);
    until("the plugin pass", || {
        let s = view.snapshot();
        s.done && !s.plugins.pending
    })
    .await;
    view
}

/// The plugin defaults in `defaultAddons`, with their state.
async fn state(core: &Core) -> Vec<(String, DefaultInstalled)> {
    let all = core.default_addons().await.unwrap().addons;
    (all.into_iter().filter(|a| a.group == "plugins"))
        .map(|a| (a.key, a.installed))
        .collect()
}

fn core() -> Core {
    let core = Core::temporary(OWNER).unwrap();
    core.use_plugin_defaults(Vec::new());
    core
}

/// A repository added after its sheet lists its providers on; Versions runs
/// them and their links are rows of "From plugins", never the pick, each
/// playing its own link.
#[tokio::test]
async fn a_repository_s_provider_lists_rows_from_plugins() {
    let fixture = Fixture::start();
    let core = core();
    let preview = core.preview_plugin_repo(&fixture.repo()).await.unwrap();
    assert_eq!(
        (preview.name.as_str(), preview.host.as_str()),
        ("Momotus test plugins", HOST)
    );
    assert_eq!(preview.providers, ["Test film"]);
    // The sheet wrote nothing.
    assert!(core.plugin_repos().await.unwrap().repos.is_empty());
    let answer = core.add_plugin_repo(&fixture.repo()).await.unwrap();
    assert!(answer.available);
    let repo = &answer.repos[0];
    assert_eq!(repo.name.as_deref(), Some("Momotus test plugins"));
    assert_eq!(repo.providers.len(), 1);
    assert!(repo.enabled && repo.providers[0].enabled);
    assert_eq!(repo.providers[0].types, ["movie", "series"]);

    let view = settled(&core, film("tt0000001")).await;
    let s = view.snapshot();
    assert_eq!(s.plugins.rows.len(), 1, "{:?}", fixture.asked());
    let row = &s.plugins.rows[0];
    assert_eq!(row.origin, RowOrigin::Plugin);
    assert_eq!(row.source_name.as_deref(), Some("Test film"));
    assert_eq!(row.host.as_deref(), Some(HOST));
    assert!(row.key.starts_with(mq_runtime::plugins::KEY_PREFIX));
    // No addon: nothing is picked, and the page is not empty.
    assert!(s.recommended.is_none() && s.rows.is_empty());
    assert!(s.empty.is_none());
    let played = view.candidates(&row.key);
    assert_eq!(played.len(), 1);
    let Source::Http { url } = &played[0].source else {
        panic!("{:?}", played[0].source)
    };
    assert_eq!(
        url.as_str(),
        format!("{}/media/test-film.webm", fixture.base)
    );
    assert_eq!(played[0].addon_url.as_ref().unwrap().scheme(), "mq-plugin");
    // The provider asked its own host for the film, by the IMDb id (no
    // TMDB in a test core).
    assert!(
        fixture
            .asked()
            .contains(&"/lookup/movie/tt0000001".to_owned())
    );
}

/// A provider turned off runs nothing; turned on again it runs.
#[tokio::test]
async fn a_provider_turned_off_runs_nothing() {
    let fixture = Fixture::start();
    let core = core();
    core.add_plugin_repo(&fixture.repo()).await.unwrap();
    core.set_plugin_enabled(&fixture.repo(), "test-film", false)
        .unwrap();
    let repos = core.plugin_repos().await.unwrap().repos;
    assert!(!repos[0].providers[0].enabled);
    let view = settled(&core, film("tt0000002")).await;
    assert!(view.snapshot().plugins.rows.is_empty());
    assert!(!fixture.asked().iter().any(|p| p.starts_with("/lookup/")));
    core.set_plugin_enabled(&fixture.repo(), "test-film", true)
        .unwrap();
    let view = settled(&core, film("tt0000002")).await;
    assert_eq!(view.snapshot().plugins.rows.len(), 1);
    // The whole repository off.
    core.set_plugin_repo_enabled(&fixture.repo(), false)
        .unwrap();
    let view = VersionsView::open(&core, film("tt0000003"));
    assert!(!view.snapshot().plugins.pending);
    // Removed, it is gone from the list.
    core.remove_plugin_repo(&fixture.repo()).unwrap();
    assert!(core.plugin_repos().await.unwrap().repos.is_empty());
}

/// A TMDB on loopback that answers every lookup with the film 77.
async fn tmdb_77() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut head = [0u8; 4096];
            let _ = socket.read(&mut head).await;
            let body = r#"{"movie_results": [{"id": 77, "vote_average": 7.0, "vote_count": 9}], "tv_results": []}"#;
            let _ = socket
                .write_all(&reply(200, "application/json", body))
                .await;
        }
    });
    format!("http://{addr}")
}

/// A provider that finds nothing by the TMDB id is asked again by the IMDb
/// id: Manitux's providers read only that one and answer a TMDB id with an
/// empty list.
#[tokio::test]
async fn a_provider_empty_by_tmdb_is_asked_by_imdb() {
    let fixture = Fixture::start();
    let core = core();
    core.set_tmdb_host(mq_runtime::TmdbHost {
        url: tmdb_77().await,
        token: Some("test".into()),
        local: true,
    });
    core.add_plugin_repo(&fixture.repo()).await.unwrap();
    let view = settled(&core, film("tt0000004")).await;
    let asked = fixture.asked();
    assert_eq!(view.snapshot().plugins.rows.len(), 1, "{asked:?}");
    let lookups: Vec<&String> = asked.iter().filter(|p| p.starts_with("/lookup/")).collect();
    assert_eq!(
        lookups,
        ["/lookup/movie/77", "/lookup/movie/tt0000004"],
        "{asked:?}"
    );
}

/// A link that is no repository writes nothing.
#[tokio::test]
async fn a_bad_repository_writes_nothing() {
    let fixture = Fixture::start();
    let core = core();
    let missing = format!("{}/none/manifest.json", fixture.base);
    assert!(core.add_plugin_repo(&missing).await.is_err());
    assert!(
        core.add_plugin_repo(&fixture.repo().replace("https", "http"))
            .await
            .is_err()
    );
    assert!(core.plugin_repos().await.unwrap().repos.is_empty());
}

/// R7: both repositories come on for a profile with addons, once; one the
/// user removes stays removed; the onboarding list carries them with a
/// switch; a kids profile gets none and runs none. Nothing is fetched to
/// give them.
#[tokio::test]
async fn the_default_repositories_come_on_once() {
    let dir = std::env::temp_dir().join(format!("mq-plugins-defaults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let start = || {
        Core::open(mq_runtime::CoreConfig {
            data_dir: dir.clone(),
            profile_id: OWNER.into(),
            device_id: 9,
        })
        .unwrap()
    };
    let core = start();
    // First run: listed, absent, until onboarding answers.
    assert_eq!(
        state(&core).await,
        [
            ("nuvio-tr".to_owned(), DefaultInstalled::Absent),
            ("nuvio-world".to_owned(), DefaultInstalled::Absent)
        ]
    );
    let choice = |key: &str, on| DefaultChoice {
        key: key.into(),
        on,
    };
    core.install_defaults(&[choice("cinemeta", true), choice("nuvio-tr", false)])
        .unwrap();
    assert_eq!(
        state(&core).await,
        [
            ("nuvio-tr".to_owned(), DefaultInstalled::Off),
            ("nuvio-world".to_owned(), DefaultInstalled::Absent)
        ]
    );
    // The next start gives the one never answered, on; the one turned off
    // stays off.
    drop(core);
    let core = start();
    assert_eq!(
        state(&core).await,
        [
            ("nuvio-tr".to_owned(), DefaultInstalled::Off),
            ("nuvio-world".to_owned(), DefaultInstalled::On)
        ]
    );
    let world = core.plugin_defaults()[1].url.clone();
    core.remove_plugin_repo(&world).unwrap();
    drop(core);
    let core = start();
    assert_eq!(state(&core).await[1].1, DefaultInstalled::Absent);
    // A kids profile: none listed, none given, every change refused.
    core.set_pin(OWNER, Some("1357"), None).await.unwrap();
    let kid = core.for_profile(&core.create_kids_profile("Kid", "owl").unwrap());
    assert!(kid.plugin_repos().await.unwrap().repos.is_empty());
    assert!(!kid.plugin_repos().await.unwrap().available);
    assert!(kid.add_plugin_repo("https://example.com/").await.is_err());
    assert!(kid.install_defaults(&[choice("nuvio-tr", true)]).is_err());
    drop((core, kid));
    let _ = std::fs::remove_dir_all(&dir);
}

/// R8: Find addons lists the repositories a directory page names, on raw.githubusercontent.com
/// only, each read with its providers, languages and kinds; one added shows as installed; a kids
/// profile sees none; a page that cannot be read gives the shipped snapshot.
#[tokio::test]
async fn the_directory_lists_its_repositories() {
    let fixture = Fixture::start();
    let port = fixture.base.rsplit(':').next().unwrap().to_owned();
    let page =
        |path: &str| url::Url::parse(&format!("https://nuvioplugins.com:{port}{path}")).unwrap();
    let core = core();
    core.use_plugin_directory_at(page("/dir"));
    let listed = core.plugin_directory().await;
    assert_eq!(listed.source, "nuvioplugins.com");
    assert!(!listed.from_snapshot);
    assert_eq!(listed.repos.len(), 1, "{:?}", fixture.asked());
    let repo = &listed.repos[0];
    assert_eq!(repo.name, "Momotus test plugins");
    assert_eq!(repo.host, "raw.githubusercontent.com");
    assert_eq!(repo.providers, ["Test film"]);
    assert_eq!(repo.languages, ["en"]);
    assert_eq!(repo.types, ["movie", "series"]);
    assert!(!repo.installed);
    core.add_plugin_repo(&repo.url).await.unwrap();
    assert!(core.plugin_directory().await.repos[0].installed);
    // Kept a day: the page was read once.
    assert_eq!(fixture.asked().iter().filter(|p| *p == "/dir").count(), 1);

    let owner = self::core();
    owner.set_pin(OWNER, Some("1357"), None).await.unwrap();
    let kid = owner.for_profile(&owner.create_kids_profile("Kid", "owl").unwrap());
    assert!(kid.plugin_directory().await.repos.is_empty());

    let lost = self::core();
    lost.use_plugin_directory_at(page("/gone"));
    assert!(lost.plugin_directory().await.from_snapshot);
}
