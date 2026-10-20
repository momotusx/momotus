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
/// An answer with this many seeders settles its hash early. A tracker that
/// does not track a hash answers zero (engine review r1, P1-2).
const SETTLES: u32 = 2;
/// A hash's own trackers asked at most, after dedupe.
const MAX_OWN: usize = 16;
/// Trackers asked in one call at most: each is a name lookup on the shared
/// blocking pool (engine review r1, P2).
const MAX_TRACKERS: usize = 24;
/// How long a name that did not resolve stays unresolved. Shorter than
/// `ADDRESS_KEEP`: a cold resolver can fail once.
const FAILED_KEEP: Duration = Duration::from_secs(60);

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
    /// No addresses: the name did not resolve, kept for `FAILED_KEEP`.
    addrs: Option<(Instant, Vec<SocketAddr>)>,
    conn: Option<(Instant, SocketAddr, u64)>,
}

impl Kept {
    /// Something still stands.
    fn fresh(&self) -> bool {
        let addrs = self.addrs.as_ref().is_some_and(|(at, a)| {
            at.elapsed()
                < if a.is_empty() {
                    FAILED_KEEP
                } else {
                    ADDRESS_KEEP
                }
        });
        let conn = self
            .conn
            .is_some_and(|(at, _, _)| at.elapsed() < CONNECTION_KEEP);
        addrs || conn
    }
}

/// At most `MAX_TRACKERS` trackers for one call: every default, then the
/// trackers the most hashes share, the first named first on a tie.
fn capped(
    mut by_tracker: HashMap<String, Vec<String>>,
    order: &[String],
    defaults: &[&str],
) -> HashMap<String, Vec<String>> {
    if by_tracker.len() <= MAX_TRACKERS {
        return by_tracker;
    }
    let mut rest: Vec<(usize, usize, &String)> = order
        .iter()
        .enumerate()
        .filter(|(_, t)| !defaults.contains(&t.as_str()))
        .map(|(i, t)| (by_tracker.get(t).map_or(0, Vec::len), i, t))
        .collect();
    rest.sort_by_key(|&(shared, first, _)| (std::cmp::Reverse(shared), first));
    let room = MAX_TRACKERS.saturating_sub(by_tracker.len() - rest.len());
    let dropped: Vec<String> = rest
        .into_iter()
        .skip(room)
        .map(|(_, _, t)| t.clone())
        .collect();
    for t in dropped {
        by_tracker.remove(&t);
    }
    by_tracker
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
                let (addrs, _) = addresses(&reach, &url).await;
                if url.scheme() == "udp"
                    && !addrs.is_empty()
                    && kept_conn(&reach, url.as_str(), &addrs).is_none()
                {
                    let _ = connect(&reach, url.as_str(), &addrs, false).await;
                }
            });
        }
        let all = async { while set.join_next().await.is_some() {} };
        let _ = tokio::time::timeout(WARM_BUDGET, all).await;
    }

    /// Drops every tracker's kept addresses and connection id: after a
    /// network change they may lead nowhere (Wi-Fi to an IPv6-only network,
    /// say), and a connection id belongs to the address it was made from.
    pub fn forget_trackers(&self) {
        lock(&self.reach.trackers).clear();
    }

    /// Each tracker's stages in the last call that asked any.
    pub fn last(&self) -> Vec<TrackerTrace> {
        lock(&self.last).clone()
    }

    /// One count per `(info hash, its own trackers)`, `None` when no answer
    /// told anything in time; `defaults` are asked for every hash too. An
    /// answer tells something when it comes from one of the hash's own
    /// trackers or counts any seeder or leecher: a tracker that does not
    /// track a hash answers 0/0, while one that counts leechers knows the
    /// swarm. A count is the answer with the most seeders, then leechers. A
    /// hash settles early on `SETTLES` seeders, or once every tracker asked
    /// for it has answered or failed. At most `MAX_OWN` own trackers per
    /// hash and `MAX_TRACKERS` per call.
    pub async fn scrape(
        &self,
        torrents: &[(String, Vec<String>)],
        defaults: &[&str],
    ) -> Vec<Option<SwarmCount>> {
        let now = Instant::now();
        let mut found: HashMap<String, SwarmCount> = HashMap::new();
        let mut by_tracker: HashMap<String, Vec<String>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        // Per hash asked, its own trackers.
        let mut owners: HashMap<String, Vec<String>> = HashMap::new();
        {
            let kept = lock(&self.kept);
            for (hash, own) in torrents {
                if let Some((at, count)) = kept.get(hash)
                    && now.duration_since(*at) < KEEP
                {
                    found.insert(hash.clone(), *count);
                    continue;
                }
                let mut mine: Vec<&str> = Vec::new();
                for t in own.iter().map(String::as_str) {
                    if mine.len() < MAX_OWN && !mine.contains(&t) {
                        mine.push(t);
                    }
                }
                owners.insert(hash.clone(), mine.iter().map(|t| (*t).to_owned()).collect());
                for t in mine.into_iter().chain(defaults.iter().copied()) {
                    let hashes = by_tracker.entry(t.to_owned()).or_insert_with(|| {
                        order.push(t.to_owned());
                        Vec::new()
                    });
                    if !hashes.contains(hash) {
                        hashes.push(hash.clone());
                    }
                }
            }
        }
        let by_tracker = capped(by_tracker, &order, defaults);
        // Per hash, the trackers still out.
        let mut out: HashMap<String, usize> = HashMap::new();
        for hash in by_tracker.values().flatten() {
            *out.entry(hash.clone()).or_default() += 1;
        }
        lock(&self.reach.trackers).retain(|_, k| k.fresh());
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
                (tracker, hashes, counts)
            });
        }
        let deadline = tokio::time::Instant::now() + BUDGET;
        let mut fresh: HashMap<String, SwarmCount> = HashMap::new();
        while !out.is_empty()
            && let Ok(Some(answer)) = tokio::time::timeout_at(deadline, asks.join_next()).await
        {
            let Ok((tracker, asked, counts)) = answer else {
                continue;
            };
            for (hash, count) in counts {
                let own = owners.get(&hash).is_some_and(|o| o.contains(&tracker));
                if !own && count.seeders == 0 && count.leechers == 0 {
                    continue; // a tracker that does not track it
                }
                if count.seeders >= SETTLES {
                    out.remove(&hash);
                }
                let best = fresh.entry(hash).or_insert(count);
                if (count.seeders, count.leechers) > (best.seeders, best.leechers) {
                    *best = count;
                }
            }
            for hash in asked {
                if let Some(n) = out.get_mut(&hash) {
                    *n -= 1;
                    if *n == 0 {
                        out.remove(&hash);
                    }
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

/// The tracker's addresses, IPv4 first, and whether they came from the
/// keep: a literal must pass the scope; a name resolves through it (only
/// permitted answers come back) and is kept for `ADDRESS_KEEP`, each kept
/// address checked again on use, since the policy follows the network.
async fn addresses(reach: &Reach, url: &mq_http::Url) -> (Vec<SocketAddr>, bool) {
    let Some(port) = url.port() else {
        return (Vec::new(), false);
    };
    if let Some(ip) = literal(url) {
        let permitted = reach.scope.permits(ip);
        return (
            permitted
                .then(|| SocketAddr::new(ip, port))
                .into_iter()
                .collect(),
            false,
        );
    }
    let key = url.as_str();
    let kept = lock(&reach.trackers).get(key).and_then(|k| k.addrs.clone());
    if let Some((at, addrs)) = kept {
        // A name that did not resolve is not looked up again for a while.
        if addrs.is_empty() && at.elapsed() < FAILED_KEEP {
            return (Vec::new(), false);
        }
        if at.elapsed() < ADDRESS_KEEP {
            let still: Vec<SocketAddr> = addrs
                .into_iter()
                .filter(|a| reach.scope.permits(a.ip()))
                .collect();
            if !still.is_empty() {
                return (still, true);
            }
        }
    }
    let Some(name) = url
        .host_str()
        .and_then(|h| reqwest::dns::Name::from_str(h).ok())
    else {
        return (Vec::new(), false);
    };
    let mut addrs: Vec<SocketAddr> = match reach.scope.resolver().resolve(name).await {
        Ok(found) => found.map(|a| SocketAddr::new(a.ip(), port)).collect(),
        Err(_) => Vec::new(),
    };
    addrs.sort_by_key(SocketAddr::is_ipv6);
    lock(&reach.trackers)
        .entry(key.to_owned())
        .or_default()
        .addrs = Some((Instant::now(), addrs.clone()));
    (addrs, false)
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
/// another is left; a quarter each, the last too, for `kept` addresses,
/// which leaves time to resolve again): the socket, the address that
/// answered and its connection id, which is kept for `CONNECTION_KEEP`.
async fn connect(
    reach: &Reach,
    key: &str,
    addrs: &[SocketAddr],
    kept: bool,
) -> Option<(UdpSocket, SocketAddr, u64)> {
    for (i, &to) in addrs.iter().enumerate() {
        let Some(socket) = bind(to).await else {
            continue;
        };
        let t = rand_u32();
        let mut packet = PROTOCOL_ID.to_be_bytes().to_vec();
        packet.extend(0u32.to_be_bytes());
        packet.extend(t.to_be_bytes());
        let slice = if kept {
            Some(RESEND)
        } else {
            (i + 1 < addrs.len()).then_some(2 * RESEND)
        };
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
/// takes is dropped and the connect made again; when every kept address is
/// quiet, the name is resolved once more (the network may have moved).
async fn udp(
    reach: &Reach,
    url: &mq_http::Url,
    ids: &[[u8; 20]],
    trace: &Trace,
) -> Option<Vec<SwarmCount>> {
    let (mut addrs, mut kept) = addresses(reach, url).await;
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
            None => match connect(reach, key, &addrs, kept).await {
                Some(found) => found,
                None if kept => {
                    if let Some(k) = lock(&reach.trackers).get_mut(key) {
                        *k = Kept::default();
                    }
                    let (fresh, _) = addresses(reach, url).await;
                    if fresh.is_empty() || fresh == addrs {
                        return trace.fail("connect not sent or not received");
                    }
                    (addrs, kept) = (fresh, false);
                    continue;
                }
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

    /// `n` lists, each in the one before.
    fn nested(n: usize) -> Vec<u8> {
        [vec![b'l'; n], vec![b'e'; n]].concat()
    }

    /// Every bencode parse goes through the vendored parser, which refuses
    /// nesting past `MAX_DEPTH` before it recurses: 10,000 levels overflowed
    /// a 2 MiB stack and aborted the process (engine review r1, P1-1). A
    /// crash here kills the test binary.
    #[test]
    fn deep_nesting_is_refused_without_a_crash() {
        use librqbit_bencode::{MAX_DEPTH, dyn_from_bytes};
        let depth = MAX_DEPTH as usize;
        assert!(
            dyn_from_bytes::<Vec<u8>>(&nested(depth)).is_ok(),
            "the bound itself parses"
        );
        assert!(
            dyn_from_bytes::<Vec<u8>>(&nested(depth + 1)).is_err(),
            "one past it"
        );
        for n in [10_000, 100_000] {
            assert!(
                dyn_from_bytes::<Vec<u8>>(&nested(n)).is_err(),
                "{n} levels parsed"
            );
            let mut body = b"d5:filesd20:".to_vec();
            body.extend([7u8; 20]);
            body.extend(nested(n));
            body.extend(b"ee");
            assert_eq!(
                files(&body, &[[7u8; 20]]),
                None,
                "{n} levels in a scrape answer"
            );
        }
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

    /// A BEP 15 tracker on 127.0.0.1 counting `seeders` and no leechers for
    /// every hash, or (with `None`) one that keeps its socket and never
    /// answers.
    fn tracker(seeders: Option<u32>) -> String {
        counting(seeders.map(|s| (s, 0)))
    }

    /// As `tracker`, counting `(seeders, leechers)`.
    fn counting(answer: Option<(u32, u32)>) -> String {
        counting_after(answer, Duration::ZERO)
    }

    /// As `counting`, each answer `delay` late.
    fn counting_after(answer: Option<(u32, u32)>, delay: Duration) -> String {
        let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!("udp://{}/announce", socket.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                let Some((seeders, leechers)) = answer.filter(|_| n >= 16) else {
                    continue;
                };
                std::thread::sleep(delay);
                let mut reply = buf[8..16].to_vec();
                if buf[8..12] == 0u32.to_be_bytes() {
                    reply.extend(7u64.to_be_bytes());
                }
                for _ in 0..(n - 16) / 20 {
                    reply.extend(seeders.to_be_bytes());
                    reply.extend(0u32.to_be_bytes()); // completed
                    reply.extend(leechers.to_be_bytes());
                }
                let _ = socket.send_to(&reply, from);
            }
        });
        url
    }

    /// The count for one hash with `own` trackers and `defaults`, and how
    /// long the scrape took.
    async fn timed(own: &str, defaults: &str) -> (Option<SwarmCount>, Duration) {
        timed_with(&[own], &[defaults]).await
    }

    /// The count for one hash with `own` trackers and `defaults`, and how
    /// long the scrape took.
    async fn timed_with(own: &[&str], defaults: &[&str]) -> (Option<SwarmCount>, Duration) {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let scraper = Scraper::new(http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]));
        let hash = "0123456789abcdef0123456789abcdef01234567".to_owned();
        let own = own.iter().map(|t| (*t).to_owned()).collect();
        let started = Instant::now();
        let counts = scraper.scrape(&[(hash, own)], defaults).await;
        (counts[0], started.elapsed())
    }

    /// Which answers tell something: an own tracker's 0/0 (a dead swarm), a
    /// default's leechers (it knows the swarm), not a default's 0/0 (it does
    /// not track it); on equal seeders, more leechers (Apple walk NS1: Big
    /// Buck Bunny's default trackers all answered 0 seeders, 32 to 37
    /// leechers).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_counts_from_an_own_tracker_or_with_peers() {
        let count = |seeders, leechers| Some(SwarmCount { seeders, leechers });
        let (c, took) = timed_with(&[], &[counting(Some((0, 32))).as_str()]).await;
        assert_eq!(c, count(0, 32), "a default's leechers");
        assert!(
            took < BUDGET / 2,
            "every tracker in, still waiting: {took:?}"
        );
        let (c, _) = timed_with(&[], &[counting(Some((0, 0))).as_str()]).await;
        assert_eq!(c, None, "a default's 0/0 counted");
        let (c, _) = timed_with(&[counting(Some((0, 0))).as_str()], &[]).await;
        assert_eq!(c, count(0, 0), "an own tracker's 0/0 dropped");
        // The answer with more leechers comes second.
        let later = counting_after(Some((1, 9)), Duration::from_millis(100));
        let (c, _) = timed_with(&[counting(Some((1, 5))).as_str()], &[later.as_str()]).await;
        assert_eq!(c, count(1, 9), "equal seeders, fewer leechers kept");
    }

    /// A hash settles early on 2 or more seeders, or once every tracker
    /// asked has answered or failed; a zero alone waits for the rest, since
    /// a tracker that does not track a hash answers zero (engine review r1,
    /// P1-2: the old rule settled on any own answer, even a zero).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_hash_settles_on_seeders_or_once_every_tracker_is_in() {
        let seeders = |n| {
            Some(SwarmCount {
                seeders: n,
                leechers: 0,
            })
        };
        let early = BUDGET / 2;
        // A private address: the policy refuses it at once, a failure.
        let refused = "udp://10.0.0.1:6969/announce";
        let (count, took) = timed(&tracker(Some(5)), &tracker(None)).await;
        assert_eq!(count, seeders(5));
        assert!(
            took < early,
            "5 seeders waited for a silent tracker: {took:?}"
        );
        let (count, took) = timed(&tracker(Some(0)), &tracker(None)).await;
        assert_eq!(count, seeders(0));
        assert!(
            took >= BUDGET,
            "a zero settled it with a tracker out: {took:?}"
        );
        let (count, took) = timed(&tracker(Some(0)), &tracker(Some(7))).await;
        assert_eq!(count, seeders(7), "the larger count lost");
        assert!(took < early, "7 seeders did not settle it: {took:?}");
        let (count, took) = timed(&tracker(Some(0)), &tracker(Some(0))).await;
        assert_eq!(count, seeders(0));
        assert!(took < early, "both answered, still waiting: {took:?}");
        let (count, took) = timed(&tracker(Some(1)), refused).await;
        assert_eq!(count, seeders(1));
        assert!(took < early, "a failed tracker was waited for: {took:?}");
        // Two hashes: the first has heard from every tracker (one failed),
        // the second has seeders while a tracker of its own is still out.
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let s = Scraper::new(http.torrent_scope(&[V4]));
        let started = Instant::now();
        let both = [
            (H1.to_owned(), vec![tracker(Some(1)), refused.to_owned()]),
            (H2.to_owned(), vec![tracker(Some(5)), tracker(None)]),
        ];
        assert_eq!(s.scrape(&both, &[]).await, [seeders(1), seeders(5)]);
        let took = started.elapsed();
        assert!(
            took < early,
            "a hash with every tracker in still out: {took:?}"
        );
    }

    /// At most 16 own trackers per hash and 24 per call, the shared ones
    /// kept; a name that did not resolve is not looked up again within a
    /// minute (engine review r1, P2: each lookup holds a thread of the
    /// shared blocking pool).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_trackers_asked_are_capped_and_failed_names_kept() {
        let s = loopback_scraper();
        let slow = Duration::from_millis(300);
        let refused: IpAddr = "10.0.0.1".parse().unwrap();
        let name = |i: usize| {
            let host = format!("cap{i}.scrape.test");
            mq_http::test_dns::set_slow(&host, &[refused], slow);
            format!("udp://{host}:6969/announce")
        };
        let one = vec![(H1.to_owned(), (0..20).map(name).collect::<Vec<_>>())];
        s.scrape(&one, &[]).await;
        assert_eq!(s.last().len(), MAX_OWN, "own trackers of one hash");
        // Shared by the last two hashes, first named 31st: kept for being
        // shared, not for coming early.
        let shared = name(99);
        let many: Vec<(String, Vec<String>)> = (0..4)
            .map(|h| {
                let mut own: Vec<String> = (0..10).map(|i| name(100 + 10 * h + i)).collect();
                if h >= 2 {
                    own.push(shared.clone());
                }
                (format!("{h}{}", &H2[1..]), own)
            })
            .collect();
        let started = Instant::now();
        s.scrape(&many, &[]).await;
        assert!(started.elapsed() >= slow, "the names were not looked up");
        let asked: Vec<String> = s.last().into_iter().map(|t| t.tracker).collect();
        assert_eq!(asked.len(), MAX_TRACKERS, "trackers in one call");
        assert!(
            asked.contains(&shared),
            "the tracker every hash shares was dropped"
        );
        let started = Instant::now();
        s.scrape(&many, &[]).await;
        assert!(started.elapsed() < slow, "failed names looked up again");
    }

    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const H1: &str = "1111111111111111111111111111111111111111";
    const H2: &str = "2222222222222222222222222222222222222222";
    const V4: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    struct Fake {
        url: String,
        port: u16,
        seen: Arc<AtomicUsize>,
        id: Arc<AtomicU64>,
        /// Set, it answers nothing more.
        quiet: Arc<std::sync::atomic::AtomicBool>,
    }

    /// A BEP 15 tracker on `ip`, named `host` in its URL, counting `seeders`
    /// for every hash. It drops its first `lose` datagrams, answers a scrape
    /// on another connection id than its current one with an error, and
    /// counts every datagram.
    fn fake(ip: IpAddr, host: &str, seeders: u32, lose: usize) -> Fake {
        fake_at(ip, 0, host, seeders, lose)
    }

    /// As `fake`, on `port` (0: any free one).
    fn fake_at(ip: IpAddr, port: u16, host: &str, seeders: u32, lose: usize) -> Fake {
        let socket = std::net::UdpSocket::bind((ip, port)).unwrap();
        let port = socket.local_addr().unwrap().port();
        let (seen, id) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicU64::new(7)));
        let quiet = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (got, current, hushed) = (seen.clone(), id.clone(), quiet.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                if got.fetch_add(1, Ordering::SeqCst) < lose
                    || n < 16
                    || hushed.load(Ordering::SeqCst)
                {
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
            port,
            seen,
            id,
            quiet,
        }
    }

    /// After a network change the kept addresses and connection ids go:
    /// the next scrape resolves and connects afresh (engine review r1, P3).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_network_change_forgets_the_kept_trackers() {
        let s = loopback_scraper();
        let f = fake(V4, "change.scrape.test", 3, 0);
        let slow = Duration::from_millis(300);
        mq_http::test_dns::set_slow("change.scrape.test", &[V4], slow);
        s.warm(&[f.url.as_str()]).await;
        s.forget_trackers();
        let (count, took) = one(&s, &f.url, H1).await;
        assert_eq!(count, Some(3));
        assert!(
            took >= slow,
            "the kept address outlived the change: {took:?}"
        );
        assert!(
            !s.last()[0].reused,
            "the kept connection id outlived the change"
        );
    }

    /// Every kept address quiet: the name is resolved once more within the
    /// same scrape, and the tracker found where it moved (engine review r1,
    /// P3: an IPv6-only network after Wi-Fi, with no change reported).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_tracker_whose_kept_address_went_quiet_is_resolved_again() {
        let s = loopback_scraper();
        let old = fake(V4, "moved.scrape.test", 4, 0);
        mq_http::test_dns::set("moved.scrape.test", &[V4]);
        assert_eq!(one(&s, &old.url, H1).await.0, Some(4));
        let v6: IpAddr = "::1".parse().unwrap();
        let new = fake_at(v6, old.port, "moved.scrape.test", 9, 0);
        old.quiet.store(true, Ordering::SeqCst);
        mq_http::test_dns::set("moved.scrape.test", &[v6]);
        let (count, took) = one(&s, &new.url, H2).await;
        assert_eq!(count, Some(9), "the quiet kept address was not given up");
        assert!(took < BUDGET, "{took:?}");
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
