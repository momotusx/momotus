use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// True for loopback, link-local, private, unique-local and unspecified hosts,
/// plus `localhost` names. DNS names are not resolved here; callers that need
/// rebinding protection check the connected address too.
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    // A fully qualified name (`localhost.`) is the same name: the system
    // resolver answers it as it answers the name without the dot.
    let host = host.strip_suffix('.').unwrap_or(host);
    if is_localhost(host) {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_private_v4(ip),
        Ok(IpAddr::V6(ip)) => is_private_v6(ip),
        Err(_) => false,
    }
}

fn is_localhost(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost")
}

/// Loopback, private, link-local and other non-public addresses.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_v4(ip),
        IpAddr::V6(ip) => is_private_v6(ip),
    }
}

// 198.18.0.0/15 stays public: fake-IP DNS proxies (Clash, Mihomo, Surge,
// sing-box) answer every public name from it.
fn is_private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || a == 0 // 0.0.0.0/8: 0.0.0.0 reaches this host on Linux and macOS
        || a == 100 && (b & 0xC0) == 64 // CGNAT 100.64/10
        || a == 192 && b == 0 && c == 0 // 192.0.0.0/24: DS-Lite and service continuity, on the router
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_v4(ip) {
        return is_private_v4(v4);
    }
    let first = ip.segments()[0];
    (first & 0xfe00) == 0xfc00 // unique local
        || (first & 0xffc0) == 0xfe80 // link local
        || (first & 0xffc0) == 0xfec0 // site local: deprecated, still on old internal networks
}

/// The IPv4 address an IPv6 form carries, which a local translator turns back
/// into that address: mapped `::ffff:0:0/96`, compatible `::/96` (`::` and
/// `::1` are 0.0.0.0 and 0.0.0.1 there), translated `::ffff:0:0:0/96`, NAT64
/// `64:ff9b::/96` and 6to4 `2002::/16`. Local-use NAT64 `64:ff9b:1::/48` has
/// no fixed embedding and stays public.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let v4 = |hi: u16, lo: u16| Some(Ipv4Addr::from(u32::from(hi) << 16 | u32::from(lo)));
    match ip.segments() {
        [0, 0, 0, 0, 0, 0 | 0xffff, hi, lo]
        | [0, 0, 0, 0, 0xffff, 0, hi, lo]
        | [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] => v4(hi, lo),
        [0x2002, hi, lo, ..] => v4(hi, lo),
        _ => None,
    }
}

/// What an installed addon's host adds to its playbacks' scope: an address
/// literal itself, exactly `127.0.0.1` and `::1` for `localhost` and
/// `*.localhost`, nothing for any other name. A public literal counts too: it
/// may sit in the device's own network, which playback treats as private.
pub fn local_addrs(host: &str) -> Vec<IpAddr> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        vec![ip.to_canonical()]
    } else if is_localhost(bare) {
        vec![Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]
    } else {
        Vec::new()
    }
}

/// Whether a playback hop may connect to `ip`: it is in `local` (canonical),
/// or it is public and not on the device's own network. A loopback scope is
/// an install running on this device, so the device's own network addresses
/// are its host too (an addon advertising its LAN address); the rest of their
/// on-link prefixes are not, and loopback addresses stay as `local` names
/// them (a literal itself, `localhost` both).
pub(crate) fn permitted(ip: IpAddr, local: &[IpAddr], device: &Device) -> bool {
    let ip = ip.to_canonical();
    let on_device =
        local.iter().any(IpAddr::is_loopback) && !ip.is_loopback() && device.own.contains(&ip);
    local.contains(&ip) || on_device || !is_private_ip(ip) && !device.holds(ip)
}

/// What playback treats as private beyond the classifier: the device's own
/// addresses and the on-link prefixes of its interfaces, which can be global
/// (a home IPv6 /64, a LAN on public IPv4).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Device {
    own: Vec<IpAddr>,
    nets: Vec<(IpAddr, u32)>,
    /// Point-to-point interfaces seen, for the snapshot's log line.
    tunnels: usize,
}

/// One address of an interface, as `getifaddrs` lists it.
#[derive(Debug, Clone)]
pub struct Interface {
    pub addr: IpAddr,
    pub mask: Option<IpAddr>,
    pub loopback: bool,
    pub point_to_point: bool,
}

/// Shorter prefixes add only the interface's own address, so one odd netmask
/// (a tunnel's `/0`, a driver's `/8`) cannot make unrelated public sources
/// unreachable.
const FLOOR_V4: u32 = 16;
const FLOOR_V6: u32 = 48;

impl Device {
    pub(crate) fn from_interfaces(list: Vec<Interface>) -> Self {
        let mut device = Device::default();
        for interface in list {
            let addr = interface.addr.to_canonical();
            device.own.push(addr);
            device.tunnels += usize::from(interface.point_to_point);
            // A fake-IP proxy's TUN takes 198.18.0.1/15 or /16, the range it
            // answers every public name from. Loopback is private anyway.
            let fake_ip = match addr {
                IpAddr::V4(v4) => matches!(v4.octets(), [198, 18 | 19, _, _]),
                IpAddr::V6(_) => false,
            };
            if interface.loopback || fake_ip {
                continue;
            }
            let floor = if addr.is_ipv4() { FLOOR_V4 } else { FLOOR_V6 };
            if let Some(len) = interface
                .mask
                .and_then(prefix_len)
                .filter(|&len| len >= floor)
            {
                device.nets.push((addr, len));
            }
        }
        device
    }

    /// Counts only, for the log: never an address.
    pub(crate) fn sizes(&self) -> (usize, usize, usize) {
        (self.own.len(), self.nets.len(), self.tunnels)
    }

    #[cfg(any(test, feature = "test-dns"))]
    pub(crate) fn own(&self) -> &[IpAddr] {
        &self.own
    }

    fn holds(&self, ip: IpAddr) -> bool {
        let embedded = match ip {
            IpAddr::V6(v6) => embedded_v4(v6).map(IpAddr::V4),
            IpAddr::V4(_) => None,
        };
        [Some(ip), embedded].into_iter().flatten().any(|ip| {
            self.own.contains(&ip)
                || self
                    .nets
                    .iter()
                    .any(|&(net, len)| same_prefix(ip, net, len))
        })
    }
}

/// A contiguous netmask's length; `None` for anything else.
fn prefix_len(mask: IpAddr) -> Option<u32> {
    let bits = wide(mask);
    (bits.leading_ones() == bits.count_ones()).then_some(bits.leading_ones())
}

fn same_prefix(a: IpAddr, b: IpAddr, len: u32) -> bool {
    let mask = u128::MAX.checked_shl(128 - len).unwrap_or(0);
    a.is_ipv4() == b.is_ipv4() && (wide(a) ^ wide(b)) & mask == 0
}

/// An address as 128 bits from the top, so one prefix length fits both
/// families.
fn wide(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(u32::from(v4)) << 96,
        IpAddr::V6(v6) => u128::from(v6),
    }
}

/// Every interface address with its netmask and flags. Claimed for Linux,
/// Android, macOS and iOS: other BSDs report compact netmasks this does not
/// decode.
#[cfg(unix)]
pub(crate) fn interfaces() -> std::io::Result<Vec<Interface>> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs only writes the list head we pass.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut list = Vec::new();
    let mut at = head;
    // SAFETY (every block in this loop): each node and the addresses it points
    // to live until freeifaddrs below.
    while let Some(ifa) = unsafe { at.as_ref() } {
        at = ifa.ifa_next;
        let flag = |f: libc::c_int| ifa.ifa_flags & f as libc::c_uint != 0;
        if let Some(addr) = unsafe { sockaddr_ip(ifa.ifa_addr, None) } {
            list.push(Interface {
                addr,
                mask: unsafe { sockaddr_ip(ifa.ifa_netmask, Some(addr)) },
                loopback: flag(libc::IFF_LOOPBACK),
                point_to_point: flag(libc::IFF_POINTOPOINT),
            });
        }
    }
    // SAFETY: `head` came from getifaddrs and nothing points into it now.
    unsafe { libc::freeifaddrs(head) };
    Ok(list)
}

// ponytail: no Windows build ships; read GetAdaptersAddresses if one does.
#[cfg(not(unix))]
pub(crate) fn interfaces() -> std::io::Result<Vec<Interface>> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// The IPv4 or IPv6 address in `sa`. A netmask is read as the family of its
/// interface address (`like`): BSDs may leave a mask's family unset and cut
/// it short at its `sa_len`, so nothing past that length is read.
///
/// # Safety
/// `sa` is null or points to a sockaddr from `getifaddrs`.
#[cfg(unix)]
unsafe fn sockaddr_ip(sa: *const libc::sockaddr, like: Option<IpAddr>) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    let family = match like {
        Some(IpAddr::V4(_)) => libc::AF_INET,
        Some(IpAddr::V6(_)) => libc::AF_INET6,
        // SAFETY: `sa` is a sockaddr (the caller's contract).
        None => i32::from(unsafe { (*sa).sa_family }),
    };
    let size = match family {
        libc::AF_INET => size_of::<libc::sockaddr_in>(),
        libc::AF_INET6 => size_of::<libc::sockaddr_in6>(),
        _ => return None,
    };
    // SAFETY: as above.
    #[cfg(target_vendor = "apple")]
    let size = size.min(usize::from(unsafe { (*sa).sa_len }));
    // SAFETY: zero is a valid sockaddr_storage, which is larger than `size`;
    // `sa` holds at least `size` bytes; the storage is aligned and sized for
    // both families.
    let mut storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    unsafe {
        std::ptr::copy_nonoverlapping(sa.cast::<u8>(), (&raw mut storage).cast::<u8>(), size)
    };
    let storage = &raw const storage;
    Some(if family == libc::AF_INET {
        let sin = unsafe { &*storage.cast::<libc::sockaddr_in>() };
        IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)))
    } else {
        let sin6 = unsafe { &*storage.cast::<libc::sockaddr_in6>() };
        IpAddr::V6(Ipv6Addr::from(sin6.sin6_addr.s6_addr))
    })
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
            "localhost.",
            "LOCALHOST",
            "app.localhost",
            "app.localhost.",
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
            "torrentio.strem.fun.",
            "localhost.example",
            "2606:4700::1111",
            "100.128.0.1",
        ] {
            assert!(!is_private_host(host), "{host}");
        }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn address_classes() {
        for public in [
            "8.8.8.8",
            "198.18.0.1",
            "192.0.2.1",
            "2606:4700::1111",
            "64:ff9b::808:808",
            "64:ff9b:1::1",
            "2001::1",
            "2001:2::1",
            "240.0.0.1",
            "224.0.0.1",
            "100::1",
        ] {
            assert!(!is_private_ip(ip(public)), "{public} is public");
        }
        for private in [
            // Today's set.
            "127.0.0.3",
            "10.1.2.3",
            "172.16.5.4",
            "192.168.0.10",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "::",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
            // New rows.
            "0.1.2.3",
            "192.0.0.2",
            "fec0::1",
            "::127.0.0.3",
            "::ffff:0:7f00:3",
            "64:ff9b::7f00:3",
            "2002:7f00:3::1",
        ] {
            assert!(is_private_ip(ip(private)), "{private} is not public");
        }
        // An embedded public address stays public.
        assert!(!is_private_ip(ip("2002:808:808::1")));
    }

    #[test]
    fn fake_ip_answers_stay_public() {
        for answer in ["198.18.0.1", "198.19.255.254"] {
            assert!(!is_private_ip(ip(answer)), "{answer}");
            assert!(permitted(ip(answer), &[], &Device::default()), "{answer}");
        }
    }

    #[test]
    fn local_addrs_of_install_hosts() {
        let loopback = vec![ip("127.0.0.1"), ip("::1")];
        for host in ["localhost", "a.localhost", "LOCALHOST"] {
            let found = local_addrs(host);
            assert_eq!(found, loopback, "{host}");
            assert!(!found.contains(&ip("127.0.0.3")), "{host}");
        }
        assert_eq!(local_addrs("192.168.1.5"), vec![ip("192.168.1.5")]);
        assert_eq!(local_addrs("[::1]"), vec![ip("::1")]);
        assert_eq!(local_addrs("[::ffff:192.168.1.5]"), vec![ip("192.168.1.5")]);
        // A literal on the device's own global network is its own host too.
        assert_eq!(local_addrs("[2001:db8::5]"), vec![ip("2001:db8::5")]);
        for name in ["addon.example", "nas.local", "localhost.example"] {
            assert!(local_addrs(name).is_empty(), "{name}");
        }
    }

    /// An interface with a prefix length.
    fn interface(cidr: &str, point_to_point: bool) -> Interface {
        let (addr, len) = cidr.split_once('/').unwrap();
        let (addr, len): (IpAddr, u32) = (ip(addr), len.parse().unwrap());
        let bits = u128::MAX.checked_shl(128 - len).unwrap_or(0);
        let mask = match addr {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::from((bits >> 96) as u32)),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from(bits)),
        };
        Interface {
            addr,
            mask: Some(mask),
            loopback: addr.is_loopback(),
            point_to_point,
        }
    }

    #[test]
    fn prefix_floors_and_links() {
        let device = Device::from_interfaces(vec![
            // At each floor the prefix counts; just below it, only the address.
            interface("203.0.113.5/16", false),
            interface("100.128.0.5/15", false),
            interface("2001:db8:a::5/48", false),
            interface("2001:db8:c::5/47", false),
            // Point-to-point links count their prefixes.
            interface("2001:db8:e::2/127", true),
            interface("2001:db8:f::5/64", true),
            // A /0 tunnel and fake-IP TUNs add only their addresses.
            interface("192.0.2.9/0", true),
            interface("198.18.0.1/15", true),
            interface("198.19.0.1/16", true),
            interface("127.0.0.1/8", false),
        ]);
        for denied in [
            "203.0.200.1",
            "2001:db8:a:ffff::1",
            "2001:db8:e::3",
            "2001:db8:f::9",
            "64:ff9b::cb00:c801",
            // Own addresses, whatever their prefix.
            "100.128.0.5",
            "2001:db8:c::5",
            "192.0.2.9",
            "198.18.0.1",
            "198.19.0.1",
        ] {
            assert!(!permitted(ip(denied), &[], &device), "{denied} is refused");
        }
        for allowed in [
            "100.129.0.1",
            "2001:db8:d::1",
            "2001:db8:b::1",
            "198.18.5.5",
            "198.19.7.7",
            "8.8.8.8",
        ] {
            assert!(permitted(ip(allowed), &[], &device), "{allowed} is allowed");
        }
        // A local addon's own host on that network stays reachable; its
        // neighbours do not.
        let local = [ip("203.0.200.7")];
        assert!(permitted(ip("203.0.200.7"), &local, &device));
        assert!(!permitted(ip("203.0.200.8"), &local, &device));
        assert_eq!(device.sizes(), (10, 4, 5));
    }

    #[test]
    fn the_device_is_read_from_its_interfaces() {
        let list = interfaces().expect("getifaddrs works here");
        assert!(
            list.iter().any(|i| i.addr == ip("127.0.0.1") && i.loopback),
            "{list:?}"
        );
        // Netmasks decode as contiguous prefixes of a sane length.
        for i in list.iter().filter(|i| i.addr.is_ipv4() && !i.loopback) {
            let len = i.mask.and_then(prefix_len);
            assert!(len.is_some_and(|l| (8..=32).contains(&l)), "{i:?}");
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
