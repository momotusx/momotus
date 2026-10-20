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
    /// The BEP 15 connect answered.
    pub connected_ms: Option<u64>,
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

pub(crate) struct Scraper {
    scope: AddressScope,
    http: reqwest::Client,
    kept: Mutex<HashMap<String, (Instant, SwarmCount)>>,
    last: Mutex<Vec<TrackerTrace>>,
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
            scope,
            http,
            kept: Mutex::default(),
            last: Mutex::default(),
        }
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
            let (scope, http) = (self.scope.clone(), self.http.clone());
            let trace = Trace {
                t0: now,
                to: Arc::new(Mutex::new(TrackerTrace {
                    tracker: tracker.clone(),
                    ..Default::default()
                })),
            };
            traces.push(trace.to.clone());
            asks.spawn(async move {
                let counts = ask(&scope, &http, &tracker, &hashes, &trace).await;
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
    scope: &AddressScope,
    http: &reqwest::Client,
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
        "udp" => udp(scope, &url, &ids, trace).await,
        "http" | "https" => over_http(scope, http, &url, &ids, trace).await,
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

/// The tracker's address: a literal must pass the scope, a name resolves
/// through it (only permitted answers come back).
async fn address(scope: &AddressScope, url: &mq_http::Url) -> Option<SocketAddr> {
    let port = url.port()?;
    if let Some(ip) = literal(url) {
        return scope.permits(ip).then_some(SocketAddr::new(ip, port));
    }
    let name = reqwest::dns::Name::from_str(url.host_str()?).ok()?;
    let mut found = scope.resolver().resolve(name).await.ok()?;
    found.next().map(|a| SocketAddr::new(a.ip(), port))
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

/// BEP 15: a connect, then one scrape per 70 hashes, each answer taken only
/// from the tracker's own address with the transaction id asked.
async fn udp(
    scope: &AddressScope,
    url: &mq_http::Url,
    ids: &[[u8; 20]],
    trace: &Trace,
) -> Option<Vec<SwarmCount>> {
    let Some(to) = address(scope, url).await else {
        return trace.fail("refused or not resolved");
    };
    lock(&trace.to).resolved_ms = trace.ms();
    let local: SocketAddr = match to {
        SocketAddr::V4(_) => (std::net::Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => (std::net::Ipv6Addr::UNSPECIFIED, 0).into(),
    };
    let Ok(socket) = UdpSocket::bind(local).await else {
        return trace.fail("no socket");
    };
    let tid = || rand_u32();
    let t = tid();
    let mut connect = PROTOCOL_ID.to_be_bytes().to_vec();
    connect.extend(0u32.to_be_bytes());
    connect.extend(t.to_be_bytes());
    let Some(answer) = exchange(&socket, to, &connect, 0, t).await else {
        return trace.fail("connect not sent or not received");
    };
    lock(&trace.to).connected_ms = trace.ms();
    let conn = u64::from_be_bytes(answer.get(8..16)?.try_into().ok()?);
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(PER_PACKET) {
        let t = tid();
        let mut scrape = conn.to_be_bytes().to_vec();
        scrape.extend(2u32.to_be_bytes());
        scrape.extend(t.to_be_bytes());
        chunk.iter().for_each(|h| scrape.extend(h));
        let Some(answer) = exchange(&socket, to, &scrape, 2, t).await else {
            return trace.fail("scrape not sent or not received");
        };
        for i in 0..chunk.len() {
            let at = 8 + 12 * i;
            let seeders = u32::from_be_bytes(answer.get(at..at + 4)?.try_into().ok()?);
            let leechers = u32::from_be_bytes(answer.get(at + 8..at + 12)?.try_into().ok()?);
            out.push(SwarmCount { seeders, leechers });
        }
    }
    Some(out)
}

/// Sends `packet` to `to` and waits for its answer: `action` and `tid` as
/// asked, from `to` itself (another source is ignored).
async fn exchange(
    socket: &UdpSocket,
    to: SocketAddr,
    packet: &[u8],
    action: u32,
    tid: u32,
) -> Option<Vec<u8>> {
    socket.send_to(packet, to).await.ok()?;
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, from) = socket.recv_from(&mut buf).await.ok()?;
        let answer = &buf[..n];
        if from == to
            && answer.get(0..4) == Some(&action.to_be_bytes()[..])
            && answer.get(4..8) == Some(&tid.to_be_bytes()[..])
        {
            return Some(answer.to_vec());
        }
    }
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
}
