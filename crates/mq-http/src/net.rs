use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// True for loopback, link-local, private, unique-local and unspecified hosts,
/// plus `localhost` names. DNS names are not resolved here; callers that need
/// rebinding protection check the connected address too.
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_private_v4(ip),
        Ok(IpAddr::V6(ip)) => is_private_v6(ip),
        Err(_) => false,
    }
}

/// Loopback, private, link-local and other non-public addresses.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_v4(ip),
        IpAddr::V6(ip) => is_private_v6(ip),
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.octets()[0] == 100 && (ip.octets()[1] & 0xC0) == 64 // CGNAT 100.64/10
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_private_v4(v4);
    }
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || (first & 0xfe00) == 0xfc00 // unique local
        || (first & 0xffc0) == 0xfe80 // link local
}

/// Removes credentials, query values and every path segment that is not a
/// Stremio request's own, keeping enough to identify the endpoint in logs.
/// What is kept comes from position, not from what a segment looks like (a
/// key can be short and look like an id or a word): the trailing
/// `manifest.json`, or the trailing `{resource}/{type}/{id}.json` of a
/// resource request. Everything before them is the addon's config, which
/// often carries a key (`realdebrid=KEY`), and an extra segment after the id
/// carries search terms; both are redacted, as is the whole path of any other
/// URL (a debrid download token, a file name).
pub fn redact_url(url: &Url) -> String {
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    let segments: Vec<&str> = url.path_segments().into_iter().flatten().collect();
    let kept = stremio_request(&segments);
    for (at, segment) in segments.iter().enumerate() {
        out.push('/');
        if kept.contains(&at) {
            out.push_str(segment);
        } else {
            out.push_str("<redacted>");
        }
    }
    if url.query().is_some() {
        out.push_str("?<redacted>");
    }
    out
}

/// The positions of a Stremio request's own segments in `segments`: the last
/// one when it is `manifest.json`; `{resource}/{type}/{id}.json` at the end,
/// or before a last `{extra}.json`, when `resource` is a Stremio resource.
fn stremio_request(segments: &[&str]) -> std::ops::Range<usize> {
    const RESOURCES: [&str; 5] = ["catalog", "meta", "stream", "subtitles", "addon_catalog"];
    let n = segments.len();
    let resource_at =
        |at: usize, id: &str| RESOURCES.contains(&segments[at]) && id.ends_with(".json");
    if segments.last() == Some(&"manifest.json") {
        n - 1..n
    } else if n >= 3 && resource_at(n - 3, segments[n - 1]) {
        n - 3..n
    } else if n >= 4 && resource_at(n - 4, segments[n - 1]) {
        // `{resource}/{type}/{id}/{extra}.json`: the extra (search terms, paging) is not kept.
        n - 4..n - 1
    } else {
        0..0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts() {
        for host in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.10",
            "172.16.5.4",
            "169.254.1.1",
            "100.64.0.1",
            "localhost",
            "[::1]",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(is_private_host(host), "{host}");
        }
        for host in [
            "8.8.8.8",
            "torrentio.strem.fun",
            "2606:4700::1111",
            "100.128.0.1",
        ] {
            assert!(!is_private_host(host), "{host}");
        }
    }

    #[test]
    fn redacts_config_segments_and_queries() {
        let url = Url::parse(
            "https://addon.example/eyJkZWJyaWQiOiJyZWFsZGVicmlkIiwia2V5IjoiYWJjIn0/stream/movie/tt1.json?apikey=secret",
        )
        .unwrap();
        let redacted = redact_url(&url);
        assert_eq!(
            redacted,
            "https://addon.example/<redacted>/stream/movie/tt1.json?<redacted>"
        );
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn keeps_only_the_stremio_request_by_position() {
        for (raw, want) in [
            // Tokens that look like an id or a word stay hidden in the config position.
            (
                "https://a.example/tt1234567/manifest.json",
                "https://a.example/<redacted>/manifest.json",
            ),
            (
                "https://a.example/stream/manifest.json",
                "https://a.example/<redacted>/manifest.json",
            ),
            // Search terms in the extra segment are not kept.
            (
                "https://c.example/cfg/catalog/movie/top/search=secret.json",
                "https://c.example/<redacted>/catalog/movie/top/<redacted>",
            ),
            // A short key, an addon config with a key in it, a debrid token and file name.
            (
                "https://a.example/k3y9Zq/manifest.json",
                "https://a.example/<redacted>/manifest.json",
            ),
            (
                "https://t.example/providers=yts|realdebrid=ABC123/stream/series/tt0944947:1:5.json",
                "https://t.example/<redacted>/stream/series/tt0944947:1:5.json",
            ),
            (
                "https://x.example/d/WCRZ4JNYGSMSI/Movie.2010.mkv",
                "https://x.example/<redacted>/<redacted>/<redacted>",
            ),
            (
                "https://k.example/meta/anime/kitsu%3A46474%3A1.json",
                "https://k.example/meta/anime/kitsu%3A46474%3A1.json",
            ),
            (
                "https://v.example/v3/media",
                "https://v.example/<redacted>/<redacted>",
            ),
        ] {
            assert_eq!(redact_url(&Url::parse(raw).unwrap()), want, "{raw}");
        }
    }
}
