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

/// Removes credentials, query values and addon config path segments that
/// often carry API keys, keeping enough to identify the endpoint in logs.
pub fn redact_url(url: &Url) -> String {
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    for segment in url.path_segments().into_iter().flatten() {
        out.push('/');
        if looks_secret(segment) {
            out.push_str("<redacted>");
        } else {
            out.push_str(segment);
        }
    }
    if url.query().is_some() {
        out.push_str("?<redacted>");
    }
    out
}

fn looks_secret(segment: &str) -> bool {
    segment.len() >= 24
        && segment
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .count()
            * 10
            >= segment.len() * 8
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
}
