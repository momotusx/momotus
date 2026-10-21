//! Development builds only: Trakt, Find addons' lists and Import from Stremio from local fakes.

use mq_runtime::{Core, StremioEndpoints};
use std::net::IpAddr;

/// Development builds only (`debug_assertions`; a release build has no hook):
/// `MQ_DEV_TRAKT=<loopback url>` points Trakt at a local fake (the web e2e on
/// the dev bridge, emulator walks through the FFI) with a test app id. A
/// shipped build talks to Trakt only as its build was configured, and a URL
/// off this machine is refused: no Trakt call goes elsewhere.
#[cfg(debug_assertions)]
pub fn dev_trakt(core: &Core) {
    let Ok(url) = std::env::var("MQ_DEV_TRAKT") else {
        return;
    };
    if !loopback_url(&url) {
        tracing::warn!("MQ_DEV_TRAKT ignored: only an http URL on this machine is taken");
        eprintln!("MQ_DEV_TRAKT ignored: only an http URL on this machine is taken");
        return;
    }
    core.set_trakt_config(Some(mq_runtime::TraktConfig {
        client_id: "marquee-dev".into(),
        redirect_uri: "urn:ietf:wg:oauth:2.0:oob".into(),
        hosts: mq_runtime::TraktHosts {
            api: url.clone(),
            auth: url,
            local: true,
        },
    }));
}

#[cfg(not(debug_assertions))]
pub fn dev_trakt(_: &Core) {}

/// Development builds only, on the same rule: `MQ_DEV_SEGMENTS=<loopback
/// base url>` asks a local fake for skip segments (TheIntroDB at
/// `<base>/theintrodb`, IntroDB at `<base>/introdb`, AniSkip at
/// `<base>/aniskip`, Kitsu at `<base>/kitsu`), so the web e2e plays a
/// recorded answer and never waits on a live service.
#[cfg(debug_assertions)]
pub fn dev_segments(core: &Core) {
    let Ok(url) = std::env::var("MQ_DEV_SEGMENTS") else {
        return;
    };
    if !loopback_url(&url) {
        tracing::warn!("MQ_DEV_SEGMENTS ignored: only an http URL on this machine is taken");
        eprintln!("MQ_DEV_SEGMENTS ignored: only an http URL on this machine is taken");
        return;
    }
    let base = url.trim_end_matches('/');
    core.segments().set_hosts(mq_runtime::segments::Hosts {
        theintrodb: format!("{base}/theintrodb"),
        introdb: format!("{base}/introdb"),
        aniskip: format!("{base}/aniskip"),
        kitsu: format!("{base}/kitsu"),
        local: true,
    });
}

#[cfg(not(debug_assertions))]
pub fn dev_segments(_: &Core) {}

/// Development builds only, on the same rule: `MQ_DEV_MDBLIST=<loopback base
/// url>` asks a local fake for IMDb and Rotten Tomatoes scores with a test
/// key, so the web e2e shows the ratings row without the project's key.
#[cfg(debug_assertions)]
pub fn dev_ratings(core: &Core) {
    let Ok(url) = std::env::var("MQ_DEV_MDBLIST") else {
        return;
    };
    if !loopback_url(&url) {
        tracing::warn!("MQ_DEV_MDBLIST ignored: only an http URL on this machine is taken");
        eprintln!("MQ_DEV_MDBLIST ignored: only an http URL on this machine is taken");
        return;
    }
    core.ratings().set_host(mq_runtime::ratings::Host {
        url: url.trim_end_matches('/').to_owned(),
        key: Some("marquee-dev".into()),
        local: true,
    });
}

#[cfg(not(debug_assertions))]
pub fn dev_ratings(_: &Core) {}

/// Development builds only, on the same rule: `MQ_DEV_REPLAY=<loopback base
/// url>` sends every request the core makes to a host off this machine
/// (addons, subtitles, any source) to a local recorder, as
/// `<base>/<scheme>/<host><path>`: the web e2e records each answer once and
/// then runs with the network off.
#[cfg(debug_assertions)]
pub fn dev_replay() {
    let Ok(url) = std::env::var("MQ_DEV_REPLAY") else {
        return;
    };
    if !loopback_url(&url) {
        tracing::warn!("MQ_DEV_REPLAY ignored: only an http URL on this machine is taken");
        eprintln!("MQ_DEV_REPLAY ignored: only an http URL on this machine is taken");
        return;
    }
    mq_http::set_dev_replay(&url);
}

#[cfg(not(debug_assertions))]
pub fn dev_replay() {}

/// Development builds only, on the same rule: `MQ_DEV_CINEMETA=<loopback
/// manifest url>` reads Find addons' two lists from a local fake (walks of
/// the empty and filter-empty states, which the live lists never show).
#[cfg(debug_assertions)]
pub(crate) fn catalog_source() -> Option<String> {
    let url = std::env::var("MQ_DEV_CINEMETA").ok()?;
    if loopback_url(&url) {
        return Some(url);
    }
    tracing::warn!("MQ_DEV_CINEMETA ignored: only an http URL on this machine is taken");
    None
}

#[cfg(not(debug_assertions))]
pub(crate) fn catalog_source() -> Option<String> {
    None
}

/// Development builds only, on the same rule: `MQ_DEV_STREMIO=<loopback base
/// url>` points Import from Stremio at a local fake of its account API and
/// link service (apps/android/scripts/fake-stremio.ts), so the import path
/// into the recommended addons can be walked without an account.
#[cfg(debug_assertions)]
pub(crate) fn stremio_endpoints() -> StremioEndpoints {
    match std::env::var("MQ_DEV_STREMIO") {
        Ok(url) if loopback_url(&url) => {
            let base = url.trim_end_matches('/').to_owned();
            StremioEndpoints {
                api: base.clone(),
                link: base,
            }
        }
        Ok(_) => {
            tracing::warn!("MQ_DEV_STREMIO ignored: only an http URL on this machine is taken");
            StremioEndpoints::default()
        }
        Err(_) => StremioEndpoints::default(),
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn stremio_endpoints() -> StremioEndpoints {
    StremioEndpoints::default()
}

/// `http://` to `localhost` or a loopback address, with no user part.
#[cfg_attr(not(any(debug_assertions, test)), allow(dead_code))]
fn loopback_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::{StremioEndpoints, loopback_url, stremio_endpoints};

    /// The one test that sets `MQ_DEV_STREMIO`. A release build (`cargo test
    /// --release`, no debug assertions) never takes it.
    #[test]
    fn the_import_fake_is_taken_in_debug_builds_only_and_on_this_machine() {
        let real = StremioEndpoints::default();
        // SAFETY: no other test in this crate reads or writes MQ_DEV_STREMIO.
        unsafe { std::env::set_var("MQ_DEV_STREMIO", "http://127.0.0.1:7783/") };
        let fake = stremio_endpoints();
        if cfg!(debug_assertions) {
            assert_eq!(fake.api, "http://127.0.0.1:7783");
            assert_eq!(fake.link, "http://127.0.0.1:7783");
        } else {
            assert_eq!((fake.api, fake.link), (real.api.clone(), real.link.clone()));
        }
        unsafe { std::env::set_var("MQ_DEV_STREMIO", "http://stremio.example") };
        let refused = stremio_endpoints();
        assert_eq!(
            (refused.api, refused.link),
            (real.api.clone(), real.link.clone())
        );
        unsafe { std::env::remove_var("MQ_DEV_STREMIO") };
        let unset = stremio_endpoints();
        assert_eq!((unset.api, unset.link), (real.api, real.link));
    }

    #[test]
    fn the_dev_trakt_url_stays_on_this_machine() {
        for ok in [
            "http://127.0.0.1:17808",
            "http://localhost:1/x",
            "http://[::1]:9",
        ] {
            assert!(loopback_url(ok), "{ok}");
        }
        for no in [
            "https://api.trakt.tv",
            "http://trakt.example",
            "http://localhost.example:1",
            "http://localhost:1@trakt.example",
            "http://192.168.1.2:80",
            "https://127.0.0.1:1",
            "ftp://127.0.0.1",
        ] {
            assert!(!loopback_url(no), "{no}");
        }
    }
}
