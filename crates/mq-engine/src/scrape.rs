//! Live swarm counts from trackers, for ranking: an addon's seeder number
//! goes stale both ways (28 listed with none left, 0 listed with six live:
//! bench/m13/start-reliability.jsonl). BEP 15 scrape over UDP, the BEP 48
//! convention over HTTP, every tracker at once within a short budget, each
//! destination under the trackers' address policy. No DHT.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mq_http::AddressScope;
use tokio::net::UdpSocket;
use tokio::task::JoinSet;

/// Seeders and leechers as the tracker that reported the most seeders
/// counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwarmCount {
    pub seeders: u32,
    pub leechers: u32,
}

/// One tracker's part in the last scrape call, for diagnosis: milliseconds
/// from the call's start to the end of each stage it reached. A stage left
/// `None` with no `failed` was cut by the budget.
#[derive(Clone, Debug, Default)]
pub struct TrackerTrace {
    pub tracker: String,
    /// The name resolved (UDP; an HTTP tracker resolves inside its request).
    pub resolved_ms: Option<u64>,
    /// The BEP 15 connect answered, or a kept connection id was taken.
    pub connected_ms: Option<u64>,
    /// A kept BEP 15 connection id was used.
    pub reused: bool,
    /// The counts came.
    pub answered_ms: Option<u64>,
    /// Why it gave up before the budget, when it did.
    pub failed: Option<&'static str>,
}

/// Where a tracker's stages are written as they end.
#[derive(Clone)]
struct Trace {
    t0: Instant,
    to: Arc<Mutex<TrackerTrace>>,
}

impl Trace {
    fn ms(&self) -> Option<u64> {
        Some(u64::try_from(self.t0.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    fn fail<T>(&self, why: &'static str) -> Option<T> {
        lock(&self.to).failed = Some(why);
        None
    }
}

/// The whole scrape, every tracker in parallel: late answers are dropped.
pub(crate) const BUDGET: Duration = Duration::from_secs(1);
/// How long an answer stands for its hash.
const KEEP: Duration = Duration::from_secs(5 * 60);
/// Hashes in one UDP scrape (BEP 15 fits about 74 in a datagram).
const PER_PACKET: usize = 70;
/// An HTTP scrape answer larger than this is not a scrape answer.
const MAX_ANSWER: usize = 64 * 1024;
const PROTOCOL_ID: u64 = 0x41727101980;
/// A BEP 15 datagram goes again after this long without its answer.
const RESEND: Duration = Duration::from_millis(250);
/// Sends of one BEP 15 datagram at most: the first and two more.
const SENDS: u32 = 3;
/// How long a tracker's resolved addresses stand (checked again on use).
const ADDRESS_KEEP: Duration = Duration::from_secs(5 * 60);
/// How long a BEP 15 connection id is used: BEP 15 lets a client use one
/// for a minute.
const CONNECTION_KEEP: Duration = Duration::from_secs(60);
/// A warm-up gives up after this long.
const WARM_BUDGET: Duration = Duration::from_secs(3);

pub(crate) struct Scraper {
    reach: Reach,
    kept: Mutex<HashMap<String, (Instant, SwarmCount)>>,
    last: Mutex<Vec<TrackerTrace>>,
}

/// What a tracker call needs: the policy, the HTTP client, and what each
/// tracker left from earlier calls.
#[derive(Clone)]
struct Reach {
    scope: AddressScope,
    http: reqwest::Client,
    trackers: Arc<Mutex<HashMap<String, Kept>>>,
}

/// A tracker's resolved addresses and BEP 15 connection id, kept between
/// calls so a later scrape skips the lookup and the connect.
#[derive(Clone, Default)]
struct Kept {
    addrs: Option<(Instant, Vec<SocketAddr>)>,
    conn: Option<(Instant, SocketAddr, u64)>,
}

impl Scraper {
    /// Trackers reached as the session reaches them (no proxy, no
    /// redirects, names through the scope's resolver).
    pub fn new(scope: AddressScope) -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .dns_resolver(scope.resolver())
            .timeout(BUDGET)
            .build()
            .unwrap_or_default();
        Self {
            reach: Reach {
                scope,
                http,
                trackers: Arc::default(),
            },
            kept: Mutex::default(),
            last: Mutex::default(),
        }
    }

    /// Resolves `trackers` and makes each UDP one's BEP 15 connect, so a
    /// scrape soon after costs one round trip (a title's page opened before
    /// its versions are asked for).
    pub async fn warm(&self, trackers: &[&str]) {
        let mut set = JoinSet::new();
        for t in trackers {
            let (reach, t) = (self.reach.clone(), (*t).to_owned());
            set.spawn(async move {
                let Ok(url) = mq_http::Url::parse(&t) else {
                    return;
                };
                let addrs = addresses(&reach, &url).await;
                if url.scheme() == "udp"
                    && !addrs.is_empty()
                    && kept_conn(&reach, url.as_str(), &addrs).is_none()
                {
                    let _ = connect(&reach, url.as_str(), &addrs).await;
                }
            });
        }
        let all = async { while set.join_next().await.is_some() {} };
        let _ = tokio::time::timeout(WARM_BUDGET, all).await;
    }

    /// Each tracker's stages in the last call that asked any.
    pub fn last(&self) -> Vec<TrackerTrace> {
        lock(&self.last).clone()
    }

    /// One count per `(info hash, its own trackers)`, `None` when no tracker
    /// answered for it in time. `defaults` are asked for every hash too.
    /// It ends early once each hash has an answer from one of its own
    /// trackers, or seeders from a default one: a default tracker that does
    /// not know a hash answers zero, which settles nothing.
    pub async fn scrape(
        &self,
        torrents: &[(String, Vec<String>)],
        defaults: &[&str],
    ) -> Vec<Option<SwarmCount>> {
        let now = Instant::now();
        let mut found: HashMap<String, SwarmCount> = HashMap::new();
        let mut by_tracker: HashMap<String, Vec<String>> = HashMap::new();
        let mut waiting: HashMap<&str, &[String]> = HashMap::new();
        {
            let kept = lock(&self.kept);
            for (hash, own) in torrents {
                if let Some((at, count)) = kept.get(hash)
                    && now.duration_since(*at) < KEEP
                {
                    found.insert(hash.clone(), *count);
                    continue;
                }
                for t in own
                    .iter()
                    .map(String::as_str)
                    .chain(defaults.iter().copied())
                {
                    let hashes = by_tracker.entry(t.to_owned()).or_default();
                    if !hashes.contains(hash) {
                        hashes.push(hash.clone());
                    }
                    waiting.insert(hash.as_str(), own.as_slice());
                }
            }
        }
        let mut asks = JoinSet::new();
        let mut traces = Vec::new();
        for (tracker, hashes) in by_tracker {
            let reach = self.reach.clone();
            let trace = Trace {
                t0: now,
                to: Arc::new(Mutex::new(TrackerTrace {
                    tracker: tracker.clone(),
                    ..Default::default()
                })),
            };
            traces.push(trace.to.clone());
            asks.spawn(async move {
                let counts = ask(&reach, &tracker, &hashes, &trace).await;
                (tracker, counts)
            });
        }
        let deadline = tokio::time::Instant::now() + BUDGET;
        let mut fresh: HashMap<String, SwarmCount> = HashMap::new();
        while !waiting.is_empty()
            && let Ok(Some(answer)) = tokio::time::timeout_at(deadline, asks.join_next()).await
        {
            let Ok((tracker, counts)) = answer else {
                continue;
            };
            for (hash, count) in counts {
                let settles = count.seeders > 0
                    || waiting
                        .get(hash.as_str())
                        .is_some_and(|own| own.contains(&tracker));
                if settles {
                    waiting.remove(hash.as_str());
                }
                let best = fresh.entry(hash).or_insert(count);
                if count.seeders > best.seeders {
                    *best = count;
                }
            }
        }
        asks.abort_all();
        if !traces.is_empty() {
            *lock(&self.last) = traces.iter().map(|t| lock(t).clone()).collect();
        }
        {
            let mut kept = lock(&self.kept);
            kept.retain(|_, (at, _)| now.duration_since(*at) < KEEP);
            for (hash, count) in &fresh {
                kept.insert(hash.clone(), (now, *count));
            }
        }
        found.extend(fresh);
        torrents
            .iter()
            .map(|(h, _)| found.get(h).copied())
            .collect()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// One tracker's counts for `hashes`; nothing when it is refused, silent or
/// unreadable.
async fn ask(
    reach: &Reach,
    tracker: &str,
    hashes: &[String],
    trace: &Trace,
) -> Vec<(String, SwarmCount)> {
    let Ok(url) = mq_http::Url::parse(tracker) else {
        return trace.fail("not a URL").unwrap_or_default();
    };
    // A hash in another form (base32) is not asked for.
    let (hashes, ids): (Vec<String>, Vec<[u8; 20]>) = hashes
        .iter()
        .filter_map(|h| id(h).map(|i| (h.clone(), i)))
        .unzip();
    if ids.is_empty() {
        return trace.fail("no hex info hash").unwrap_or_default();
    }
    let counts = match url.scheme() {
        "udp" => udp(reach, &url, &ids, trace).await,
        "http" | "https" => over_http(&reach.scope, &reach.http, &url, &ids, trace).await,
        _ => trace.fail("no scrape for this scheme"),
    };
    if counts.is_some() {
        lock(&trace.to).answered_ms = trace.ms();
    }
    counts
        .map(|c| hashes.into_iter().zip(c).collect())
        .unwrap_or_default()
}

/// An info hash in 40 hex characters.
fn id(hash: &str) -> Option<[u8; 20]> {
    let mut out = [0u8; 20];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(hash.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    (hash.len() == 40).then_some(out)
}

/// The tracker's addresses, IPv4 first: a literal must pass the scope; a
/// name resolves through it (only permitted answers come back) and is kept
/// for `ADDRESS_KEEP`, each kept address checked again on use, since the
/// policy follows the network.
async fn addresses(reach: &Reach, url: &mq_http::Url) -> Vec<SocketAddr> {
    let Some(port) = url.port() else {
        return Vec::new();
    };
    if let Some(ip) = literal(url) {
        return if reach.scope.permits(ip) {
            vec![SocketAddr::new(ip, port)]
        } else {
            Vec::new()
        };
    }
    let key = url.as_str();
    let kept = lock(&reach.trackers).get(key).and_then(|k| k.addrs.clone());
    if let Some((at, addrs)) = kept
        && at.elapsed() < ADDRESS_KEEP
    {
        let still: Vec<SocketAddr> = addrs
            .into_iter()
            .filter(|a| reach.scope.permits(a.ip()))
            .collect();
        if !still.is_empty() {
            return still;
        }
    }
    let Some(name) = url
        .host_str()
        .and_then(|h| reqwest::dns::Name::from_str(h).ok())
    else {
        return Vec::new();
    };
    let Ok(found) = reach.scope.resolver().resolve(name).await else {
        return Vec::new();
    };
    let mut addrs: Vec<SocketAddr> = found.map(|a| SocketAddr::new(a.ip(), port)).collect();
    addrs.sort_by_key(SocketAddr::is_ipv6);
    lock(&reach.trackers)
        .entry(key.to_owned())
        .or_default()
        .addrs = Some((Instant::now(), addrs.clone()));
    addrs
}

/// The tracker's kept connection id while it stands, with the address it
/// came from when that is still one of `addrs`.
fn kept_conn(reach: &Reach, key: &str, addrs: &[SocketAddr]) -> Option<(SocketAddr, u64)> {
    let kept = lock(&reach.trackers).get(key)?.conn?;
    let (at, to, conn) = kept;
    (at.elapsed() < CONNECTION_KEEP && addrs.contains(&to)).then_some((to, conn))
}

/// A UDP socket for `to`'s family.
async fn bind(to: SocketAddr) -> Option<UdpSocket> {
    let local: SocketAddr = match to {
        SocketAddr::V4(_) => (std::net::Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => (std::net::Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    UdpSocket::bind(local).await.ok()
}

/// BEP 15 connect to each of `addrs` in turn (half a second each while
/// another is left): the socket, the address that answered and its
/// connection id, which is kept for `CONNECTION_KEEP`.
async fn connect(
    reach: &Reach,
    key: &str,
    addrs: &[SocketAddr],
) -> Option<(UdpSocket, SocketAddr, u64)> {
    for (i, &to) in addrs.iter().enumerate() {
        let Some(socket) = bind(to).await else {
            continue;
        };
        let t = rand_u32();
        let mut packet = PROTOCOL_ID.to_be_bytes().to_vec();
        packet.extend(0u32.to_be_bytes());
        packet.extend(t.to_be_bytes());
        let slice = (i + 1 < addrs.len()).then_some(2 * RESEND);
        let Some(answer) = exchange(&socket, to, &packet, 0, t, slice).await else {
            continue;
        };
        let Some(conn) = answer.get(8..16).and_then(|b| b.try_into().ok()) else {
            continue;
        };
        let conn = u64::from_be_bytes(conn);
        lock(&reach.trackers)
            .entry(key.to_owned())
            .or_default()
            .conn = Some((Instant::now(), to, conn));
        return Some((socket, to, conn));
    }
    None
}

/// The URL's host when it is an address, not a name.
fn literal(url: &mq_http::Url) -> Option<IpAddr> {
    let host = url.host_str()?;
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse().ok()
}

/// BEP 15: a connect (skipped while a kept connection id stands), then one
/// scrape per 70 hashes, each answer taken only from the tracker's own
/// address with the transaction id asked. A kept id the tracker no longer
/// takes is dropped and the connect made again.
async fn udp(
    reach: &Reach,
    url: &mq_http::Url,
    ids: &[[u8; 20]],
    trace: &Trace,
) -> Option<Vec<SwarmCount>> {
    let addrs = addresses(reach, url).await;
    if addrs.is_empty() {
        return trace.fail("refused or not resolved");
    }
    lock(&trace.to).resolved_ms = trace.ms();
    let key = url.as_str();
    let mut reuse = kept_conn(reach, key, &addrs);
    loop {
        let reused = reuse.is_some();
        let (socket, to, conn) = match reuse.take() {
            Some((to, conn)) => match bind(to).await {
                Some(socket) => (socket, to, conn),
                None => return trace.fail("no socket"),
            },
            None => match connect(reach, key, &addrs).await {
                Some(found) => found,
                None => return trace.fail("connect not sent or not received"),
            },
        };
        {
            let mut t = lock(&trace.to);
            t.connected_ms = trace.ms();
            t.reused = reused;
        }
        // A kept id gets half a second before a fresh connect.
        let slice = reused.then_some(2 * RESEND);
        match scrape_all(&socket, to, conn, ids, slice).await {
            Some(counts) => return Some(counts),
            None if reused => {
                if let Some(k) = lock(&reach.trackers).get_mut(key) {
                    k.conn = None;
                }
            }
            None => return trace.fail("scrape not sent or not received"),
        }
    }
}

/// The scrapes for `ids` over connection `conn`, 70 hashes a datagram.
async fn scrape_all(
    socket: &UdpSocket,
    to: SocketAddr,
    conn: u64,
    ids: &[[u8; 20]],
    slice: Option<Duration>,
) -> Option<Vec<SwarmCount>> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(PER_PACKET) {
        let t = rand_u32();
        let mut scrape = conn.to_be_bytes().to_vec();
        scrape.extend(2u32.to_be_bytes());
        scrape.extend(t.to_be_bytes());
        chunk.iter().for_each(|h| scrape.extend(h));
        let answer = exchange(socket, to, &scrape, 2, t, slice).await?;
        for i in 0..chunk.len() {
            let at = 8 + 12 * i;
            let seeders = u32::from_be_bytes(answer.get(at..at + 4)?.try_into().ok()?);
            let leechers = u32::from_be_bytes(answer.get(at + 8..at + 12)?.try_into().ok()?);
            out.push(SwarmCount { seeders, leechers });
        }
    }
    Some(out)
}

/// Sends `packet` to `to`, again every `RESEND` up to `SENDS` times, and
/// waits for its answer: `action` and `tid` as asked, from `to` itself
/// (another source is ignored). The tracker's error answer (action 3), or
/// `slice` passing, ends it with nothing; without a slice the caller's
/// budget ends it.
async fn exchange(
    socket: &UdpSocket,
    to: SocketAddr,
    packet: &[u8],
    action: u32,
    tid: u32,
    slice: Option<Duration>,
) -> Option<Vec<u8>> {
    let start = tokio::time::Instant::now();
    let end = slice.map(|s| start + s);
    let mut buf = vec![0u8; 2048];
    for sent in 1..=SENDS {
        socket.send_to(packet, to).await.ok()?;
        let resend = (sent < SENDS).then(|| start + RESEND * sent);
        let wake = match (resend, end) {
            (Some(r), Some(e)) => Some(r.min(e)),
            (r, e) => r.or(e),
        };
        loop {
            let got = match wake {
                Some(w) => match tokio::time::timeout_at(w, socket.recv_from(&mut buf)).await {
                    Ok(got) => got,
                    Err(_) => break,
                },
                None => socket.recv_from(&mut buf).await,
            };
            let (n, from) = got.ok()?;
            let answer = &buf[..n];
            if from != to || answer.get(4..8) != Some(&tid.to_be_bytes()[..]) {
                continue;
            }
            match answer.get(0..4) {
                Some(a) if a == action.to_be_bytes() => return Some(answer.to_vec()),
                Some(a) if a == 3u32.to_be_bytes() => return None,
                _ => {}
            }
        }
        if end.is_some_and(|e| tokio::time::Instant::now() >= e) {
            return None;
        }
    }
    None
}

fn rand_u32() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(Instant::now().elapsed().as_nanos());
    h.finish() as u32
}

/// BEP 48: the announce URL's last part `announce...` becomes `scrape...`;
/// a tracker whose URL has no such part has no scrape.
fn scrape_url(announce: &mq_http::Url, ids: &[[u8; 20]]) -> Option<String> {
    let path = announce.path();
    let (dir, last) = path.rsplit_once('/')?;
    let rest = last.strip_prefix("announce")?;
    let mut url = announce.clone();
    url.set_path(&format!("{dir}/scrape{rest}"));
    let mut query = url.query().map(str::to_owned).unwrap_or_default();
    for h in ids {
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str("info_hash=");
        h.iter().for_each(|b| query.push_str(&format!("%{b:02X}")));
    }
    url.set_query(Some(&query));
    Some(url.into())
}

async fn over_http(
    scope: &AddressScope,
    http: &reqwest::Client,
    announce: &mq_http::Url,
    ids: &[[u8; 20]],
    trace: &Trace,
) -> Option<Vec<SwarmCount>> {
    // A literal skips the resolver: it is checked here.
    if literal(announce).is_some_and(|ip| !scope.permits(ip)) {
        return trace.fail("refused");
    }
    let Some(url) = scrape_url(announce, ids) else {
        return trace.fail("no scrape URL");
    };
    let Ok(mut answer) = http.get(url).send().await else {
        return trace.fail("request failed");
    };
    if !answer.status().is_success() {
        return trace.fail("not a success status");
    }
    let mut body = Vec::new();
    while let Some(chunk) = answer.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_ANSWER {
            return trace.fail("answer too large");
        }
    }
    files(&body, ids).or_else(|| trace.fail("not a scrape answer"))
}

/// The `files` dictionary of a bencoded scrape answer, in the order asked;
/// a hash it leaves out counts nothing.
fn files(body: &[u8], ids: &[[u8; 20]]) -> Option<Vec<SwarmCount>> {
    use librqbit_bencode::BencodeValue as V;
    let V::Dict(top) = librqbit_bencode::dyn_from_bytes::<Vec<u8>>(body).ok()? else {
        return None;
    };
    let V::Dict(files) = top.get(&b"files"[..])? else {
        return None;
    };
    let int = |d: &HashMap<Vec<u8>, V<Vec<u8>>>, k: &[u8]| match d.get(k) {
        Some(V::Integer(n)) => u32::try_from(*n).ok(),
        _ => None,
    };
    Some(
        ids.iter()
            .map(|h| match files.get(&h[..]) {
                Some(V::Dict(d)) => SwarmCount {
                    seeders: int(d, b"complete").unwrap_or(0),
                    leechers: int(d, b"incomplete").unwrap_or(0),
                },
                _ => SwarmCount {
                    seeders: 0,
                    leechers: 0,
                },
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_urls_follow_bep_48() {
        let id = [0xabu8; 20];
        let url = |s: &str| scrape_url(&mq_http::Url::parse(s).unwrap(), &[id]);
        let hash: String = (0..20).map(|_| "%AB").collect();
        assert_eq!(
            url("http://t.example:80/announce").unwrap(),
            format!("http://t.example/scrape?info_hash={hash}")
        );
        assert_eq!(
            url("https://t.example/x/announce.php?k=1").unwrap(),
            format!("https://t.example/x/scrape.php?k=1&info_hash={hash}")
        );
        assert_eq!(url("http://t.example/a/b"), None);
    }

    #[test]
    fn a_scrape_answer_reads_complete_and_incomplete() {
        let h = [7u8; 20];
        let mut body = b"d5:filesd20:".to_vec();
        body.extend(h);
        body.extend(b"d8:completei12e10:downloadedi90e10:incompletei3eeee");
        let counts = files(&body, &[h, [8u8; 20]]).unwrap();
        assert_eq!(
            counts[0],
            SwarmCount {
                seeders: 12,
                leechers: 3
            }
        );
        assert_eq!(
            counts[1],
            SwarmCount {
                seeders: 0,
                leechers: 0
            }
        );
        assert_eq!(files(b"not bencode", &[h]), None);
    }

    /// A BEP 15 tracker on 127.0.0.1 counting `seeders` for every hash, or
    /// (with `None`) one that keeps its socket and never answers.
    fn tracker(seeders: Option<u32>) -> String {
        let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!("udp://{}/announce", socket.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                let Some(seeders) = seeders.filter(|_| n >= 16) else {
                    continue;
                };
                let mut answer = buf[8..16].to_vec();
                if buf[8..12] == 0u32.to_be_bytes() {
                    answer.extend(7u64.to_be_bytes());
                }
                for _ in 0..(n - 16) / 20 {
                    answer.extend(seeders.to_be_bytes());
                    answer.extend([0u8; 8]);
                }
                let _ = socket.send_to(&answer, from);
            }
        });
        url
    }

    /// The count for one hash with `own` trackers and `defaults`, and how
    /// long the scrape took.
    async fn timed(own: &str, defaults: &str) -> (Option<SwarmCount>, Duration) {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let scraper = Scraper::new(http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]));
        let hash = "0123456789abcdef0123456789abcdef01234567".to_owned();
        let started = Instant::now();
        let counts = scraper
            .scrape(&[(hash, vec![own.to_owned()])], &[defaults])
            .await;
        (counts[0], started.elapsed())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_scrape_ends_once_each_hash_has_its_own_answer() {
        let seeders = |n| {
            Some(SwarmCount {
                seeders: n,
                leechers: 0,
            })
        };
        let early = BUDGET / 2;
        // Its own tracker's zero (a dead swarm) is an answer.
        let (count, took) = timed(&tracker(Some(0)), &tracker(None)).await;
        assert_eq!(count, seeders(0));
        assert!(
            took < early,
            "own answer in, a silent default waited for: {took:?}"
        );
        let (count, took) = timed(&tracker(None), &tracker(Some(5))).await;
        assert_eq!(count, seeders(5));
        assert!(took < early, "seeders from a default settle it: {took:?}");
        let (count, took) = timed(&tracker(None), &tracker(Some(0))).await;
        assert_eq!(count, seeders(0));
        assert!(took >= BUDGET, "a default's zero settled it: {took:?}");
    }

    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const H1: &str = "1111111111111111111111111111111111111111";
    const H2: &str = "2222222222222222222222222222222222222222";
    const V4: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    struct Fake {
        url: String,
        seen: Arc<AtomicUsize>,
        id: Arc<AtomicU64>,
    }

    /// A BEP 15 tracker on `ip`, named `host` in its URL, counting `seeders`
    /// for every hash. It drops its first `lose` datagrams, answers a scrape
    /// on another connection id than its current one with an error, and
    /// counts every datagram.
    fn fake(ip: IpAddr, host: &str, seeders: u32, lose: usize) -> Fake {
        let socket = std::net::UdpSocket::bind((ip, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        let (seen, id) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicU64::new(7)));
        let (got, current) = (seen.clone(), id.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                if got.fetch_add(1, Ordering::SeqCst) < lose || n < 16 {
                    continue;
                }
                let id = current.load(Ordering::SeqCst);
                let mut answer = buf[8..16].to_vec();
                if buf[8..12] == 0u32.to_be_bytes() {
                    answer.extend(id.to_be_bytes());
                } else if buf[..8] != id.to_be_bytes() {
                    answer[..4].copy_from_slice(&3u32.to_be_bytes());
                    answer.extend(b"bad connection id");
                } else {
                    for _ in 0..(n - 16) / 20 {
                        answer.extend(seeders.to_be_bytes());
                        answer.extend([0u8; 8]);
                    }
                }
                let _ = socket.send_to(&answer, from);
            }
        });
        Fake {
            url: format!("udp://{host}:{port}/announce"),
            seen,
            id,
        }
    }

    /// A scraper that may reach the loopback addresses these tests use.
    fn loopback_scraper() -> Scraper {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let allowed: [IpAddr; 3] = [V4, "::1".parse().unwrap(), "127.0.0.3".parse().unwrap()];
        Scraper::new(http.torrent_scope(&allowed))
    }

    /// One hash's count from `url` alone, and how long it took.
    async fn one(s: &Scraper, url: &str, hash: &str) -> (Option<u32>, Duration) {
        let started = Instant::now();
        let counts = s
            .scrape(&[(hash.to_owned(), vec![url.to_owned()])], &[])
            .await;
        (counts[0].map(|c| c.seeders), started.elapsed())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_lost_datagram_is_sent_again() {
        let f = fake(V4, "127.0.0.1", 4, 1);
        let (count, took) = one(&loopback_scraper(), &f.url, H1).await;
        assert_eq!(count, Some(4), "no answer after a lost connect");
        assert!(took >= RESEND, "answered before any resend: {took:?}");
        assert_eq!(f.seen.load(Ordering::SeqCst), 3, "lost, sent again, scrape");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_kept_connection_id_saves_the_connect() {
        let (f, s) = (fake(V4, "127.0.0.1", 4, 0), loopback_scraper());
        assert_eq!(one(&s, &f.url, H1).await.0, Some(4));
        assert_eq!(f.seen.load(Ordering::SeqCst), 2, "connect and scrape");
        assert_eq!(one(&s, &f.url, H2).await.0, Some(4));
        assert_eq!(
            f.seen.load(Ordering::SeqCst),
            3,
            "the second scrape connected again"
        );
        assert!(s.last()[0].reused, "the trace says it was reused");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_connection_id_the_tracker_dropped_is_made_again() {
        let (f, s) = (fake(V4, "127.0.0.1", 4, 0), loopback_scraper());
        assert_eq!(one(&s, &f.url, H1).await.0, Some(4));
        f.id.store(8, Ordering::SeqCst); // the tracker restarted
        let (count, took) = one(&s, &f.url, H2).await;
        assert_eq!(count, Some(4), "no count after the tracker's error");
        assert!(took < RESEND, "waited out the error answer: {took:?}");
        assert_eq!(
            f.seen.load(Ordering::SeqCst),
            5,
            "refused scrape, connect, scrape"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_address_is_tried_ipv4_first() {
        let s = loopback_scraper();
        let f = fake(V4, "both.scrape.test", 4, 0);
        mq_http::test_dns::set("both.scrape.test", &["::1".parse().unwrap(), V4]);
        let (count, took) = one(&s, &f.url, H1).await;
        assert_eq!(count, Some(4));
        assert!(took < RESEND, "IPv6 tried before IPv4: {took:?}");
        let f = fake(V4, "dead-first.scrape.test", 5, 0);
        mq_http::test_dns::set(
            "dead-first.scrape.test",
            &["127.0.0.3".parse().unwrap(), V4],
        );
        assert_eq!(
            one(&s, &f.url, H2).await.0,
            Some(5),
            "the second address untried"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_resolver_is_paid_once_by_the_warm_up() {
        let s = loopback_scraper();
        let f = fake(V4, "slow.scrape.test", 6, 0);
        let slow = Duration::from_millis(700);
        mq_http::test_dns::set_slow("slow.scrape.test", &[V4], slow);
        let started = Instant::now();
        s.warm(&[f.url.as_str()]).await;
        assert!(started.elapsed() >= slow, "the warm-up did not resolve");
        let (count, took) = one(&s, &f.url, H1).await;
        assert_eq!(count, Some(6));
        assert!(
            took < RESEND,
            "the scrape resolved or connected again: {took:?}"
        );
        assert!(s.last()[0].reused, "the warm-up's connection id unused");
    }
}
