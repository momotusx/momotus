//! Development builds only: Trakt from a local fake.

use mq_runtime::Core;
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
    use super::loopback_url;

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
