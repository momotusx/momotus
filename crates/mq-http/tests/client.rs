use mq_http::test_dns::{Canary, Interface};
use mq_http::{
    Body, ClientConfig, HttpClient, HttpError, Method, Priority, RequestOptions, Url, local_addrs,
    test_dns,
};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves one canned response per connection and returns the base URL and the
/// first request head it received.
async fn serve(
    response: &'static [u8],
    delay: Duration,
) -> (String, tokio::sync::oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = socket.read(&mut buf).await.unwrap();
        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(response).await;
    });
    (format!("http://{addr}"), rx)
}

fn local_opts() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        timeout: Duration::from_secs(2),
        ..Default::default()
    }
}

#[tokio::test]
async fn reads_json_and_sends_range() {
    let (base, head) = serve(
        b"HTTP/1.1 206 Partial Content\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let resp = client
        .get_range(&format!("{base}/f.mkv"), 100, 50, &local_opts())
        .await
        .unwrap();
    assert_eq!(resp.status, 206);
    let value: serde_json::Value = resp.json().unwrap();
    assert_eq!(value["ok"], true);
    let head = head.await.unwrap().to_ascii_lowercase();
    assert!(head.contains("range: bytes=100-149"), "{head}");
    assert!(head.contains("user-agent: marquee/"), "{head}");
}

#[tokio::test]
async fn caps_body_size() {
    let (base, _) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789", Duration::ZERO).await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        max_bytes: 10,
        ..local_opts()
    };
    let err = client.get(&base, &opts).await.unwrap_err();
    assert!(matches!(err, HttpError::TooLarge { limit: 10 }), "{err:?}");
}

#[tokio::test]
async fn times_out() {
    let (base, _) = serve(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        Duration::from_secs(5),
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        timeout: Duration::from_millis(200),
        ..local_opts()
    };
    let err = client.get(&base, &opts).await.unwrap_err();
    assert!(matches!(err, HttpError::Timeout(_)), "{err:?}");
}

#[tokio::test]
async fn reports_status_and_blocks_private_by_default() {
    let (base, _) = serve(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\nnope",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let err = client
        .get(&base, &RequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::BlockedPrivate(_)), "{err:?}");
    let err = client.get(&base, &local_opts()).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn posts_json_body() {
    let (base, head) = serve(
        b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let resp = client
        .post_json(
            &format!("{base}/api/getUser"),
            &serde_json::json!({"authKey": "k"}),
            &local_opts(),
        )
        .await
        .unwrap();
    assert_eq!(resp.json::<serde_json::Value>().unwrap()["ok"], true);
    let head = head.await.unwrap().to_ascii_lowercase();
    assert!(head.starts_with("post /api/getuser"), "{head}");
    assert!(head.contains("content-type: application/json"), "{head}");
    assert!(head.ends_with("{\"authkey\":\"k\"}"), "{head}");
}

#[tokio::test]
#[ignore = "network"]
async fn fetches_real_cinemeta_manifest() {
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        priority: Priority::Addon,
        ..Default::default()
    };
    let manifest: serde_json::Value = client
        .get_json("https://v3-cinemeta.strem.io/manifest.json", &opts)
        .await
        .unwrap();
    assert_eq!(manifest["id"], "com.linvo.cinemeta");
}

#[tokio::test]
async fn posts_form_json_and_multipart_bodies() {
    let cases = [
        (
            Body::Form(vec![("a".into(), "x y".into()), ("b".into(), "1".into())]),
            "content-type: application/x-www-form-urlencoded",
            "a=x+y&b=1",
        ),
        (
            Body::Json(serde_json::json!({"k": 1})),
            "content-type: application/json",
            "{\"k\":1}",
        ),
        (
            Body::Multipart(vec![("magnet".into(), "magnet:?xt=urn:btih:abc".into())]),
            "content-type: multipart/form-data; boundary=",
            "name=\"magnet\"\r\n\r\nmagnet:?xt=urn:btih:abc\r\n",
        ),
    ];
    for (body, content_type, expected) in cases {
        let (base, head) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            Duration::ZERO,
        )
        .await;
        let client = HttpClient::new(ClientConfig::default()).unwrap();
        let resp = client
            .request(Method::POST, &base, Some(body), &local_opts())
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        let head = head.await.unwrap();
        assert!(head.starts_with("POST "), "{head}");
        assert!(head.to_ascii_lowercase().contains(content_type), "{head}");
        assert!(head.contains(expected), "{head}");
    }
}

#[tokio::test]
async fn surfaces_retry_after_on_status_errors() {
    let (base, _) = serve(
        b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let err = client.get(&base, &local_opts()).await.unwrap_err();
    match err {
        HttpError::Status {
            status: 429,
            retry_after,
            ..
        } => assert_eq!(retry_after, Some(Duration::from_secs(7))),
        other => panic!("{other:?}"),
    }
}

/// Live: a public name that resolves to loopback (nip.io) is refused unless
/// the request may reach private addresses. Run with `--ignored`.
#[tokio::test]
#[ignore = "network"]
async fn live_public_name_pointing_at_loopback_is_refused() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await;
        }
    });
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let url = format!("http://127.0.0.1.nip.io:{port}/");
    let public = RequestOptions::default();
    let err = http.get(&url, &public).await.unwrap_err();
    println!("public request: {err}");
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let ok = http.get(&url, &local).await.unwrap();
    assert_eq!(&ok.body[..], b"ok");
}

// Scoped handles (plans/c21-c23-loopback.md 3.1).

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A loopback origin. Every connection gets `route(path)` as its whole answer
/// with `Connection: close`, so each request is a fresh connection and a fresh
/// lookup; every request head is kept.
struct Origin {
    addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Origin {
    async fn start<F, Fut>(port: u16, route: F) -> Origin
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        let listener = TcpListener::bind((LOOPBACK, port)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let (route, kept) = (Arc::new(route), heads.clone());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (route, kept) = (route.clone(), kept.clone());
                tokio::spawn(async move {
                    let head = read_head(&mut socket).await;
                    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                    let is_head = head.starts_with("HEAD ");
                    kept.lock().unwrap().push(head);
                    let mut answer = route(path).await;
                    if is_head && let Some(end) = answer.find("\r\n\r\n") {
                        answer.truncate(end + 4);
                    }
                    let _ = socket.write_all(answer.as_bytes()).await;
                });
            }
        });
        Origin { addr, heads }
    }

    async fn ok(port: u16) -> Origin {
        Origin::start(port, |_| async { reply("200 OK", "", "ok") }).await
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }

    fn head_of(&self, path: &str) -> Option<String> {
        let start = format!("GET {path} ");
        self.heads()
            .into_iter()
            .find(|h| h.starts_with(&start))
            .map(|h| h.to_ascii_lowercase())
    }
}

/// A request's head and, by its `Content-Length`, its body.
async fn read_head(socket: &mut tokio::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    let wanted = loop {
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
            let body = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break end + 4 + body;
        }
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return String::from_utf8_lossy(&data).into_owned(),
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
    };
    while data.len() < wanted {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn reply(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The canary's URLs: its literal, `name` pointed at it, its mapped literal
/// when it is IPv4.
fn urls(canary: &Canary, name: &str) -> Vec<String> {
    canary
        .hosts(name)
        .into_iter()
        .map(|h| format!("http://{h}/"))
        .collect()
}

fn scoped(local: &[IpAddr]) -> HttpClient {
    HttpClient::new(ClientConfig::default())
        .unwrap()
        .scoped(local)
}

fn playback() -> RequestOptions {
    RequestOptions {
        priority: Priority::Playback,
        timeout: Duration::from_secs(5),
        ..Default::default()
    }
}

/// Bounds a wait, so a hang fails with a message.
async fn within<T>(what: &str, f: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), f)
        .await
        .unwrap_or_else(|_| panic!("{what} did not finish within 5 s"))
}

async fn refused(http: &HttpClient, url: &str) -> HttpError {
    let err = within(url, http.get(url, &playback()))
        .await
        .expect_err(url);
    assert_eq!(err.kind(), "blocked address", "{url}: {err}");
    err
}

#[tokio::test]
async fn changing_dns_answers_are_checked_per_connection() {
    let origin = Origin::ok(0).await;
    let canary = Canary::default_tcp(origin.addr.port());
    test_dns::set("flip.c23.test", &[LOOPBACK]);
    let http = scoped(&[LOOPBACK]);
    let url = format!("http://flip.c23.test:{}/", origin.addr.port());
    let ok = within("first", http.get(&url, &playback()))
        .await
        .expect("the first answer is the origin");
    assert_eq!(&ok.body[..], b"ok");
    test_dns::set("flip.c23.test", &[canary.addr().ip()]);
    refused(&http, &url).await;
    assert_eq!(canary.hits(), 0);
    assert_eq!(origin.heads().len(), 1);
}

#[tokio::test]
async fn mixed_answers_never_reach_the_denied_address() {
    let origin = Origin::ok(0).await;
    let canary = Canary::default_tcp(origin.addr.port());
    // The canary first: a resolver that passes the list on connects to it.
    test_dns::set("mixed.c23.test", &[canary.addr().ip(), LOOPBACK]);
    let url = format!("http://mixed.c23.test:{}/", origin.addr.port());
    let ok = within("mixed", scoped(&[LOOPBACK]).get(&url, &playback()))
        .await
        .expect("reaches the origin");
    assert_eq!(&ok.body[..], b"ok");
    assert_eq!(canary.hits(), 0);
}

#[tokio::test]
async fn a_redirect_outside_the_scope_never_connects() {
    let canary = Canary::default_tcp(0);
    let targets = urls(&canary, "redirect.c23.test");
    let routes = targets.clone();
    let origin = Origin::start(0, move |path| {
        let at: usize = path.trim_start_matches("/to/").parse().unwrap();
        let location = format!("Location: {}\r\n", routes[at]);
        async move { reply("302 Found", &location, "") }
    })
    .await;
    let http = scoped(&[LOOPBACK]);
    for at in 0..targets.len() {
        let err = refused(&http, &origin.url(&format!("/to/{at}"))).await;
        let canary_ip = canary.addr().ip().to_string();
        assert!(!err.to_string().contains(&canary_ip), "{err}");
    }
    assert_eq!(canary.hits(), 0);
}

#[tokio::test]
async fn mapped_literals_match_the_scope() {
    let origin = Origin::ok(0).await;
    let mapped = format!("http://[::ffff:127.0.0.1]:{}/", origin.addr.port());
    // Out of scope, the mapped form of the origin's address is refused before
    // connecting.
    refused(&scoped(&[]), &mapped).await;
    assert!(origin.heads().is_empty());
    // In scope, it is the same address.
    let ok = within("mapped", scoped(&[LOOPBACK]).get(&mapped, &playback()))
        .await
        .expect("::ffff:127.0.0.1 is 127.0.0.1");
    assert_eq!(&ok.body[..], b"ok");
    let canary = Canary::default_tcp(0);
    if let IpAddr::V4(v4) = canary.addr().ip() {
        let url = format!("http://[::ffff:{v4}]:{}/", canary.addr().port());
        refused(&scoped(&[LOOPBACK]), &url).await;
    }
    assert_eq!(canary.hits(), 0);
}

/// `/hop/{at}/{end}` redirects to `/hop/{at + 1}/{end}` until `end`.
async fn chain() -> Origin {
    Origin::start(0, |path| async move {
        let parts: Vec<u32> = path
            .split('/')
            .skip(2)
            .map(|p| p.parse().unwrap())
            .collect();
        let (at, end) = (parts[0], parts[1]);
        if at < end {
            let location = format!("Location: /hop/{}/{end}\r\n", at + 1);
            reply("302 Found", &location, "")
        } else {
            reply("200 OK", "", "end")
        }
    })
    .await
}

#[tokio::test]
async fn five_redirects_are_followed_and_a_sixth_is_refused() {
    let origin = chain().await;
    let http = scoped(&[LOOPBACK]);
    let five = within(
        "five",
        http.stream(
            &origin.url("/hop/0/5"),
            Some("bytes=10-".into()),
            &playback(),
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("five redirects: {e}"));
    assert_eq!(five.status, 200);
    assert_eq!(five.final_url.path(), "/hop/5/5");
    let last = origin
        .head_of("/hop/5/5")
        .expect("the fifth target is asked");
    assert!(last.contains("range: bytes=10-"), "{last}");
    let err = within("six", http.get(&origin.url("/hop/0/6"), &playback()))
        .await
        .expect_err("a sixth redirect is refused");
    assert_eq!(err.kind(), "network", "{err}");
    assert!(origin.head_of("/hop/5/6").is_some());
    assert!(
        origin.head_of("/hop/6/6").is_none(),
        "the sixth target is never asked"
    );
}

#[tokio::test]
async fn only_redirect_statuses_are_followed() {
    let origin = Origin::start(0, |path| async move {
        match path.as_str() {
            "/305" => reply("305 Use Proxy", "Location: /target\r\n", ""),
            "/306" => reply("306 Switch Proxy", "Location: /target\r\n", ""),
            "/302" => reply("302 Found", "", ""),
            _ => reply("200 OK", "", "target"),
        }
    })
    .await;
    let http = scoped(&[LOOPBACK]);
    for (path, status) in [("/305", 305), ("/306", 306), ("/302", 302)] {
        let response = within(path, http.stream(&origin.url(path), None, &playback()))
            .await
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(response.status, status, "{path}");
    }
    assert!(origin.head_of("/target").is_none());
}

#[tokio::test]
async fn same_host_redirects_never_hold_two_permits() {
    // Both first answers wait until both requests hold a slot, so a hop that
    // takes the next slot before dropping its own waits forever.
    let gate = Arc::new(tokio::sync::Barrier::new(2));
    let origin = Origin::start(0, move |path| {
        let gate = gate.clone();
        async move {
            if path == "/a" {
                gate.wait().await;
                reply("302 Found", "Location: /b\r\n", "")
            } else {
                reply("200 OK", "", "b")
            }
        }
    })
    .await;
    let http = HttpClient::new(ClientConfig {
        per_host_limit: 2,
        ..ClientConfig::default()
    })
    .unwrap()
    .scoped(&[LOOPBACK]);
    let (url, opts) = (origin.url("/a"), playback());
    let both = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(http.get(&url, &opts), http.get(&url, &opts))
    })
    .await
    .expect("both requests finish within 2 s: no hop holds two slots");
    assert_eq!(&both.0.unwrap().body[..], b"b");
    assert_eq!(&both.1.unwrap().body[..], b"b");
}

#[tokio::test]
async fn secrets_stay_with_their_origin() {
    let b = Origin::ok(0).await;
    let to_b = format!("Location: {}\r\n", b.url("/b"));
    let a = Origin::start(0, move |_| {
        let to_b = to_b.clone();
        async move { reply("302 Found", &to_b, "") }
    })
    .await;
    let opts = RequestOptions {
        headers: [
            ("X-Api-Key", "k1"),
            ("Authorization", "Bearer t1"),
            ("Referer", "https://site.example/"),
        ]
        .map(|(n, v)| (n.to_owned(), v.to_owned()))
        .into(),
        ..playback()
    };
    let http = scoped(&[LOOPBACK]);
    within("a to b", http.get(&a.url("/a"), &opts))
        .await
        .expect("the redirect is followed");
    let on_a = a.head_of("/a").unwrap();
    for header in [
        "x-api-key: k1",
        "authorization: bearer t1",
        "referer: https://site.example/",
    ] {
        assert!(on_a.contains(header), "{on_a}");
    }
    let assert_safe_only = |head: String| {
        assert!(head.contains("referer: https://site.example/"), "{head}");
        assert!(
            !head.contains("x-api-key") && !head.contains("authorization"),
            "{head}"
        );
    };
    assert_safe_only(b.head_of("/b").unwrap());
    // A request on B that A's headers belong to (C24's memo) is another origin too.
    let owned_by_a = RequestOptions {
        owner: Some(Url::parse(&a.url("/a")).unwrap()),
        ..opts
    };
    within("memo", http.get(&b.url("/memo"), &owned_by_a))
        .await
        .expect("b answers");
    assert_safe_only(b.head_of("/memo").unwrap());
}

#[tokio::test]
async fn blocked_hops_are_typed() {
    let canary = Canary::default_tcp(0);
    let http = scoped(&[LOOPBACK]);
    // A literal (the check before connecting) and a name (the resolver).
    for url in &urls(&canary, "typed.c23.test")[..2] {
        let err = refused(&http, url).await;
        assert!(
            !err.to_string().contains(&canary.addr().ip().to_string()),
            "{err}"
        );
        let Err(err) = within(url, http.stream(url, None, &playback())).await else {
            panic!("{url} streamed");
        };
        assert_eq!(err.kind(), "blocked address", "{url}: {err}");
    }
    assert_eq!(canary.hits(), 0);
}

// The interface snapshot, through `scoped()` (plans/c21-c23-loopback.md 3.1).

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
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
        loopback: false,
        point_to_point,
    }
}

// The listed path (`no_proxy`): installs from the addon lists
// (plans/m12-addon-discovery.md 3.2). Its proxy rule is in tests/proxy.rs.

fn listed() -> RequestOptions {
    RequestOptions {
        no_proxy: true,
        timeout: Duration::from_secs(3),
        ..Default::default()
    }
}

#[tokio::test]
async fn the_listed_path_never_connects_to_a_private_address() {
    let origin = Origin::ok(0).await;
    let port = origin.addr.port();
    let canary = Canary::default_tcp(port);
    let http = quick_client();
    // Mixed answers, the private one first: it is never tried, and the
    // answer standing for a public address serves the request.
    test_dns::set("mixed.m12.test", &[canary.addr().ip()]);
    test_dns::set_public("mixed.m12.test", &[LOOPBACK]);
    let url = format!("http://mixed.m12.test:{port}/");
    let answer = within("mixed", http.get(&url, &listed()))
        .await
        .unwrap_or_else(|e| panic!("the public answer was not reached: {e}"));
    assert_eq!(&answer.body[..], b"ok");
    // Only private answers: nothing is left to connect to.
    test_dns::set("private.m12.test", &[canary.addr().ip()]);
    let url = format!("http://private.m12.test:{port}/");
    within("private", http.get(&url, &listed()))
        .await
        .expect_err("only private answers");
    assert_eq!(canary.hits(), 0, "a private answer was tried");
    // A name the system resolver itself answers with loopback, its trailing
    // dot and all.
    let url = format!("http://localhost.:{port}/");
    let err = within("localhost.", http.get(&url, &listed()))
        .await
        .expect_err("the system's own loopback name");
    assert_eq!(err.kind(), "blocked address");
    // A private literal.
    let err = within("literal", http.get(&origin.url("/"), &listed()))
        .await
        .expect_err("a private literal");
    assert_eq!(err.kind(), "blocked address");
    assert_eq!(origin.heads().len(), 1, "{:?}", origin.heads());
}

#[tokio::test]
async fn the_listed_path_never_follows_https_to_http() {
    let target = Origin::ok(0).await;
    test_dns::set_public("plain.m12.test", &[LOOPBACK]);
    test_dns::set_public("secure.m12.test", &[LOOPBACK]);
    let location = format!(
        "Location: http://plain.m12.test:{}/downgraded\r\n",
        target.addr.port()
    );
    let port = test_dns::https(move |path| match path {
        "/down" => reply("302 Found", &location, ""),
        _ => reply("200 OK", "", "secure"),
    });
    let http = quick_client();
    let base = format!("https://secure.m12.test:{port}");
    // The fixture answers over https.
    let answer = within("https", http.get(&format!("{base}/ok"), &listed()))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(&answer.body[..], b"secure");
    within("downgrade", http.get(&format!("{base}/down"), &listed()))
        .await
        .expect_err("an https to http redirect");
    assert!(target.heads().is_empty(), "{:?}", target.heads());
}

#[tokio::test]
async fn the_listed_path_never_follows_a_redirect_to_a_private_address() {
    let target = Origin::ok(0).await;
    let to_private = format!("Location: {}\r\n", target.url("/private"));
    let to_public = format!(
        "Location: http://listed-b.m12.test:{}/public\r\n",
        target.addr.port()
    );
    let origin = Origin::start(0, move |path| {
        let location = match path.as_str() {
            "/a" => to_private.clone(),
            _ => to_public.clone(),
        };
        async move { reply("302 Found", &location, "") }
    })
    .await;
    // Loopback fixtures standing for public origins.
    test_dns::set_public("listed-a.m12.test", &[LOOPBACK]);
    test_dns::set_public("listed-b.m12.test", &[LOOPBACK]);
    let http = quick_client();
    let base = format!("http://listed-a.m12.test:{}", origin.addr.port());
    within("to private", http.get(&format!("{base}/a"), &listed()))
        .await
        .expect_err("a public-to-private redirect");
    assert!(target.head_of("/private").is_none(), "{:?}", target.heads());
    // A redirect to another public origin is followed.
    let ok = within("to public", http.get(&format!("{base}/b"), &listed()))
        .await
        .expect("followed");
    assert_eq!(&ok.body[..], b"ok");
}

/// A client whose connections to addresses nobody serves fail fast.
fn quick_client() -> HttpClient {
    HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_millis(200),
        ..ClientConfig::default()
    })
    .unwrap()
}

/// A request that is not refused by the scope, so it fails some other way
/// (nothing serves the address).
async fn not_refused(http: &HttpClient, url: &str) {
    let err = within(url, http.get(url, &playback()))
        .await
        .expect_err(url);
    assert_ne!(err.kind(), "blocked address", "{url}: {err}");
}

#[tokio::test]
async fn the_interface_snapshot_is_read() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    assert!(test_dns::own_addresses(&http).is_empty());
    let _handle = http.scoped(&[]);
    let own = test_dns::own_addresses(&http);
    assert!(own.contains(&LOOPBACK), "{own:?}");
}

#[tokio::test]
async fn own_and_on_link_addresses_are_private_for_playback() {
    let http = quick_client();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let handle = http.scoped(&[]);
    test_dns::set("lan.c23.test", &[ip("2001:db8:1:2::1")]);
    for url in [
        "http://[2001:db8:1:2::1]:9/",
        "http://[2001:db8:1:2::abcd]:9/",
        "http://lan.c23.test:9/",
    ] {
        refused(&handle, url).await;
    }
    not_refused(&handle, "http://[2001:db8:1:3::1]:9/").await;
    // An addon installed by a literal on that link keeps its own host only.
    let addon = http.scoped(&local_addrs("2001:db8:1:2::5"));
    not_refused(&addon, "http://[2001:db8:1:2::5]:9/").await;
    refused(&addon, "http://[2001:db8:1:2::1]:9/").await;
    // Unscoped clients do not use the snapshot.
    not_refused(&http, "http://[2001:db8:1:2::1]:9/").await;
}

#[tokio::test]
async fn a_loopback_install_reaches_the_devices_own_addresses() {
    let http = quick_client();
    let lo = Interface {
        loopback: true,
        ..interface("::1/128", false)
    };
    let lan = interface("2001:db8:1:2::abcd/64", false);
    test_dns::set_interfaces(&http, Ok(vec![lo, lan]));
    test_dns::set("own.c23.test", &[ip("2001:db8:1:2::abcd")]);
    // An addon on this device whose streams name its LAN address.
    for host in ["127.0.0.1", "[::1]", "localhost"] {
        let addon = http.scoped(&local_addrs(host));
        not_refused(&addon, "http://[2001:db8:1:2::abcd]:9/").await;
        not_refused(&addon, "http://own.c23.test:9/").await;
        // The rest of the link stays out.
        refused(&addon, "http://[2001:db8:1:2::1]:9/").await;
    }
    // A loopback literal still scopes exactly itself.
    let v4 = http.scoped(&local_addrs("127.0.0.1"));
    refused(&v4, "http://[::1]:9/").await;
    // An install on another private host does not run here.
    let lan = http.scoped(&local_addrs("192.168.1.20"));
    refused(&lan, "http://[2001:db8:1:2::abcd]:9/").await;
    refused(&lan, "http://own.c23.test:9/").await;
}

#[tokio::test]
async fn a_failed_interface_read_keeps_the_snapshot() {
    // The log lines it writes are checked in tests/logs.rs.
    let http = quick_client();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let before = http.scoped(&[]);
    test_dns::set_interfaces(&http, Err(()));
    let after = http.scoped(&[]);
    let _ = (http.scoped(&[]), http.scoped(&[]));
    test_dns::set("kept.c23.test", &[ip("2001:db8:1:2::1")]);
    for handle in [&before, &after] {
        refused(handle, "http://[2001:db8:1:2::1]:9/").await;
        refused(handle, "http://kept.c23.test:9/").await;
    }
}

/// A loopback origin that keeps connections alive and records, per request,
/// which accepted connection served it.
async fn keep_alive_origin() -> (SocketAddr, Arc<Mutex<Vec<usize>>>) {
    let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = Arc::new(Mutex::new(Vec::new()));
    let log = served.clone();
    tokio::spawn(async move {
        let mut connection = 0;
        while let Ok((mut socket, _)) = listener.accept().await {
            let (log, at) = (log.clone(), connection);
            connection += 1;
            tokio::spawn(async move {
                while !read_head(&mut socket).await.is_empty() {
                    log.lock().unwrap().push(at);
                    let reply = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
                    if socket.write_all(reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, served)
}

#[tokio::test]
async fn pooled_connections_never_cross_a_policy_change() {
    let (addr, served) = keep_alive_origin().await;
    let url = format!("http://{addr}/");
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let old = http.scoped(&[LOOPBACK]);
    for _ in 0..2 {
        within("before", old.get(&url, &playback())).await.unwrap();
    }
    assert_eq!(*served.lock().unwrap(), [0, 0], "one pooled connection");
    // Another open reads a changed snapshot.
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:9:9::abcd/64", false)]));
    let new = http.scoped(&[LOOPBACK]);
    within("old handle", old.get(&url, &playback()))
        .await
        .unwrap();
    within("new handle", new.get(&url, &playback()))
        .await
        .unwrap();
    let served = served.lock().unwrap().clone();
    assert!(
        served[2..].iter().all(|&connection| connection != 0),
        "a request after the change reused the old pool: {served:?}"
    );
}

#[tokio::test]
async fn url_credentials_stay_with_their_origin() {
    let b = Origin::ok(0).await;
    let to_b = format!("Location: {}\r\n", b.url("/b"));
    // A's own address, known once it listens: an absolute same-origin redirect.
    let a_base = Arc::new(Mutex::new(String::new()));
    let own = a_base.clone();
    let a = Origin::start(0, move |path| {
        let to_b = to_b.clone();
        let to_self = format!("Location: {}/there\r\n", own.lock().unwrap());
        async move {
            match path.as_str() {
                "/away" => reply("302 Found", &to_b, ""),
                "/near" => reply("302 Found", "Location: /here\r\n", ""),
                "/self" => reply("302 Found", &to_self, ""),
                _ => reply("200 OK", "", "ok"),
            }
        }
    })
    .await;
    *a_base.lock().unwrap() = format!("http://{}", a.addr);
    let http = scoped(&[LOOPBACK]);
    let with_user = |path: &str| format!("http://u:p@{}{path}", a.addr);
    for path in ["/away", "/near", "/self"] {
        within(path, http.get(&with_user(path), &playback()))
            .await
            .unwrap();
    }
    assert!(
        a.head_of("/there")
            .unwrap()
            .contains("authorization: basic dtpw"),
        "an absolute redirect to the same origin keeps them"
    );
    // `u:p` as Basic credentials.
    let basic = "authorization: basic dtpw";
    assert!(a.head_of("/away").unwrap().contains(basic));
    assert!(!b.head_of("/b").unwrap().contains("authorization"));
    assert!(
        a.head_of("/here").unwrap().contains(basic),
        "same origin keeps them"
    );
}

#[tokio::test]
async fn redirect_methods_follow_rfc_9110() {
    let origin = Origin::start(0, |path| async move {
        match path.strip_prefix("/s/") {
            Some(status) => reply(&format!("{status} Moved"), "Location: /target\r\n", ""),
            None => reply("200 OK", "", "ok"),
        }
    })
    .await;
    let http = scoped(&[LOOPBACK]);
    let form = || Some(Body::Form(vec![("k".into(), "v".into())]));
    for status in [301, 302, 303, 307, 308] {
        let url = origin.url(&format!("/s/{status}"));
        for method in [Method::POST, Method::PUT, Method::HEAD] {
            let body = if method == Method::HEAD { None } else { form() };
            within(&url, http.request(method.clone(), &url, body, &playback()))
                .await
                .unwrap_or_else(|e| panic!("{status} {method}: {e}"));
            let target = origin.heads().last().unwrap().clone();
            let to_get = match status {
                301 | 302 => method == Method::POST,
                303 => method != Method::HEAD,
                _ => false,
            };
            let (want, body) = if to_get {
                ("GET", false)
            } else {
                (method.as_str(), method != Method::HEAD)
            };
            assert!(
                target.starts_with(&format!("{want} /target ")),
                "{status} {method}: {target}"
            );
            assert_eq!(target.ends_with("k=v"), body, "{status} {method}: {target}");
        }
        let opts = playback();
        let ranged = http.stream(&url, Some("bytes=5-".into()), &opts);
        within(&url, ranged)
            .await
            .unwrap_or_else(|e| panic!("{status} range: {e}"));
        let target = origin.heads().last().unwrap().to_ascii_lowercase();
        assert!(target.contains("range: bytes=5-"), "{status}: {target}");
    }
}

/// `/stall` never answers; `/to-stall` redirects there; `/body` sends its
/// headers and 10 of 100 bytes; `/hop/{ms}/{at}/{end}` waits
/// `ms`, then redirects to the next hop until `end`; anything else is `ok`.
async fn slow_origin() -> Origin {
    Origin::start(0, |path| async move {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.as_slice() {
            ["", "stall"] => std::future::pending().await,
            ["", "to-stall"] => reply("302 Found", "Location: /stall\r\n", ""),
            // 10 of the 100 bytes it announces.
            ["", "body"] => "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n0123456789".into(),
            ["", "hop", ms, at, end] => {
                let (ms, at, end): (u64, u32, u32) = (
                    ms.parse().unwrap(),
                    at.parse().unwrap(),
                    end.parse().unwrap(),
                );
                tokio::time::sleep(Duration::from_millis(ms)).await;
                if at < end {
                    let next = format!("Location: /hop/{ms}/{}/{end}\r\n", at + 1);
                    reply("302 Found", &next, "")
                } else {
                    reply("200 OK", "", "end")
                }
            }
            _ => reply("200 OK", "", "ok"),
        }
    })
    .await
}

fn one_slot() -> HttpClient {
    HttpClient::new(ClientConfig {
        per_host_limit: 1,
        ..ClientConfig::default()
    })
    .unwrap()
    .scoped(&[LOOPBACK])
}

#[tokio::test]
async fn a_stalled_hop_times_out_and_frees_its_slot() {
    let origin = slow_origin().await;
    let http = one_slot();
    let opts = RequestOptions {
        hop_headers: Some(Duration::from_millis(300)),
        ..playback()
    };
    let started = Instant::now();
    let first = tokio::time::timeout(
        Duration::from_secs(1),
        http.stream(&origin.url("/stall"), None, &opts),
    )
    .await
    .expect("a stalled first hop ends within 1 s");
    assert_eq!(first.err().map(|e| e.kind()), Some("timeout"));
    let target = tokio::time::timeout(
        Duration::from_secs(1),
        http.get(&origin.url("/to-stall"), &opts),
    )
    .await
    .expect("a stalled redirect target ends within 1 s");
    assert_eq!(target.err().map(|e| e.kind()), Some("timeout"));
    assert!(started.elapsed() < Duration::from_secs(2));
    tokio::time::timeout(Duration::from_secs(1), http.get(&origin.url("/ok"), &opts))
        .await
        .expect("the host's one slot came back")
        .unwrap();
}

#[tokio::test]
async fn a_long_chain_of_quick_hops_succeeds() {
    let origin = slow_origin().await;
    let http = scoped(&[LOOPBACK]);
    let opts = RequestOptions {
        hop_headers: Some(Duration::from_millis(300)),
        ..playback()
    };
    // Five redirects of 200 ms each: longer than one header deadline in all.
    let url = origin.url("/hop/200/0/5");
    let end = within(&url, http.get(&url, &opts))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(&end.body[..], b"end");
}

#[tokio::test]
async fn a_slow_buffered_answer_within_its_timeout_succeeds() {
    // An addon or sync request as callers make it today: its headers come
    // after 10 s, within its own 15 s.
    let origin = slow_origin().await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        timeout: Duration::from_secs(15),
        ..local_opts()
    };
    let url = origin.url("/hop/11000/0/0");
    let answer = tokio::time::timeout(Duration::from_secs(20), http.get(&url, &opts))
        .await
        .expect("finishes within 20 s")
        .unwrap_or_else(|e| panic!("refused before its own timeout: {e}"));
    assert_eq!(&answer.body[..], b"end");
}

#[tokio::test]
async fn a_buffered_chain_of_slow_hops_fails_at_its_total_limit() {
    let origin = slow_origin().await;
    let http = scoped(&[LOOPBACK]);
    let opts = RequestOptions {
        timeout: Duration::from_secs(1),
        ..playback()
    };
    // Each hop is well within its header deadline; five of them are not
    // within the total.
    let url = origin.url("/hop/300/0/5");
    let err = within(&url, http.get(&url, &opts))
        .await
        .expect_err("the chain outlasts the total limit");
    assert_eq!(err.kind(), "timeout", "{err}");
    assert!(origin.head_of("/hop/300/5/5").is_none());
    // The deadline runs inside a hop too, not only between hops: a redirect
    // after 600 ms and a target answering after another 600 ms.
    let url = origin.url("/hop/600/0/1");
    let err = within(&url, http.get(&url, &opts))
        .await
        .expect_err("the second hop outlasts what is left of the total");
    assert_eq!(err.kind(), "timeout", "{err}");
}

#[tokio::test]
async fn a_buffered_chain_stops_at_its_total_while_queued_for_a_slot() {
    let target = slow_origin().await;
    // A name, so the target has its own host slot apart from the first hop's.
    test_dns::set("busy.c23.test", &[LOOPBACK]);
    let busy = format!("http://busy.c23.test:{}", target.addr.port());
    let to_busy = format!("Location: {busy}/ok\r\n");
    let start = Origin::start(0, move |_| {
        let to_busy = to_busy.clone();
        async move { reply("302 Found", &to_busy, "") }
    })
    .await;
    let http = one_slot();
    // A body holds the redirect target's one slot until the end of the test.
    let holder = within(
        "holder",
        http.stream(&format!("{busy}/body"), None, &playback()),
    )
    .await
    .unwrap();
    let opts = RequestOptions {
        timeout: Duration::from_millis(500),
        ..playback()
    };
    let started = Instant::now();
    let err = tokio::time::timeout(Duration::from_secs(2), http.get(&start.url("/go"), &opts))
        .await
        .expect("the request ends within 2 s, while the slot is still held")
        .expect_err("the slot never frees in time");
    assert_eq!(err.kind(), "timeout", "{err}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(target.head_of("/ok").is_none());
    drop(holder);
}

#[tokio::test]
async fn a_request_queued_across_a_refresh_takes_the_new_generation() {
    let (addr, served) = keep_alive_origin().await;
    // Also on 127.0.0.1: one host, so one slot for both origins.
    let slow = slow_origin().await;
    let http = HttpClient::new(ClientConfig {
        per_host_limit: 1,
        ..ClientConfig::default()
    })
    .unwrap();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let handle = http.scoped(&[LOOPBACK]);
    let url = format!("http://{addr}/");
    within("first", handle.get(&url, &playback()))
        .await
        .unwrap();
    let holder = within(
        "holder",
        handle.stream(&slow.url("/body"), None, &playback()),
    )
    .await
    .unwrap();
    let queued = tokio::spawn({
        let (handle, url) = (handle.clone(), url.clone());
        async move { handle.get(&url, &playback()).await }
    });
    // On this current-thread runtime one yield runs the request up to its
    // wait for the slot.
    tokio::task::yield_now().await;
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:9:9::abcd/64", false)]));
    let _refreshed = http.scoped(&[LOOPBACK]);
    drop(holder);
    within("queued", queued).await.unwrap().unwrap();
    let served = served.lock().unwrap().clone();
    assert_eq!(served.len(), 2, "{served:?}");
    assert_ne!(
        served[1], 0,
        "the queued request used the old pool: {served:?}"
    );
}

#[tokio::test]
async fn cancelled_requests_free_their_slots() {
    let origin = slow_origin().await;
    let http = one_slot();
    let opts = playback();
    let (ok_url, stall_url) = (origin.url("/ok"), origin.url("/stall"));
    let ok = || http.get(&ok_url, &opts);
    let in_time = Duration::from_secs(1);
    // Dropped while waiting for headers.
    let waiting = tokio::time::timeout(Duration::from_millis(100), http.get(&stall_url, &opts));
    assert!(waiting.await.is_err());
    tokio::time::timeout(in_time, ok())
        .await
        .expect("free after a drop while waiting for headers")
        .unwrap();
    // Dropped while queued behind a body that holds the slot.
    let holder = http
        .stream(&origin.url("/body"), None, &opts)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), ok())
            .await
            .is_err()
    );
    drop(holder);
    tokio::time::timeout(in_time, ok())
        .await
        .expect("free after a drop while queued")
        .unwrap();
    // A streaming response dropped mid-body.
    let mut body = http
        .stream(&origin.url("/body"), None, &opts)
        .await
        .unwrap();
    let first = body.next_chunk().await.expect("a chunk").unwrap();
    assert!(!first.is_empty());
    drop(body);
    tokio::time::timeout(in_time, ok())
        .await
        .expect("free after a drop mid-body")
        .unwrap();
}

#[tokio::test]
async fn a_network_change_drops_every_pooled_connection() {
    let (addr, served) = keep_alive_origin().await;
    let url = format!("http://{addr}/");
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let other = http.clone();
    http.get(&url, &local_opts()).await.unwrap();
    http.get(&url, &local_opts()).await.unwrap();
    // Said through any clone, it holds for all of them: after a network change a
    // pooled connection may lead nowhere and would hold a request until its timeout.
    other.network_changed();
    http.get(&url, &local_opts()).await.unwrap();
    assert_eq!(
        *served.lock().unwrap(),
        [0, 0, 1],
        "the pooled connection, then a new one"
    );
}

/// C27: the torrent engine's rule is playback's, read through one scope.
#[test]
fn torrent_scope_is_the_playback_rule() {
    let http = quick_client();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let scope = http.torrent_scope(&[]);
    let refused = [
        "127.0.0.1",
        "::1",
        "10.0.0.1",
        "::ffff:127.0.0.1",
        "64:ff9b::7f00:1",
        "2001:db8:1:2::1",
        "2001:db8:1:2::abcd",
    ];
    for addr in refused {
        assert!(!scope.permits(ip(addr)), "{addr} must be refused");
    }
    for addr in ["8.8.8.8", "198.18.0.1", "2001:db8:1:3::1"] {
        assert!(scope.permits(ip(addr)), "{addr} must be allowed");
    }
    assert_eq!(scope.refused(), refused.len() as u64);
    let seeder = http.torrent_scope(&[ip("::1")]);
    assert!(seeder.permits(ip("::1")));
    assert!(!seeder.permits(ip("127.0.0.1")));
    assert_eq!(seeder.refused(), 1);
}

#[tokio::test]
async fn torrent_scope_resolves_only_permitted_answers() {
    let scope = quick_client().torrent_scope(&[ip("::1")]);
    test_dns::set("mixed.c27.test", &[ip("127.0.0.1"), ip("::1")]);
    let kept: Vec<IpAddr> = scope
        .resolver()
        .resolve("mixed.c27.test".parse().unwrap())
        .await
        .expect("::1 is in scope")
        .map(|a| a.ip())
        .collect();
    assert_eq!(kept, vec![ip("::1")]);
    test_dns::set("canary.c27.test", &[ip("127.0.0.1")]);
    let err = match scope
        .resolver()
        .resolve("canary.c27.test".parse().unwrap())
        .await
    {
        Ok(_) => panic!("a name with no answer in scope must fail"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("scope"), "{err}");
    assert_eq!(scope.refused(), 2, "both refused answers counted");
}

#[test]
fn torrent_scope_refresh_rereads_the_interfaces() {
    let http = quick_client();
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    let scope = http.torrent_scope(&[]);
    assert!(!scope.permits(ip("2001:db8:1:2::1")));
    assert!(scope.permits(ip("2001:db8:1:3::1")));
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:3::abcd/64", false)]));
    scope.refresh();
    assert!(
        scope.permits(ip("2001:db8:1:2::1")),
        "A's link is public now"
    );
    assert!(
        !scope.permits(ip("2001:db8:1:3::1")),
        "B's link is on-link now"
    );
}

#[test]
fn counts_are_kept_apart() {
    let http = quick_client();
    let peers = http.torrent_scope(&[]);
    let trackers = peers.with_own_count();
    assert!(!trackers.permits(ip("127.0.0.1")));
    assert_eq!((peers.refused(), trackers.refused()), (0, 1));
    test_dns::set_interfaces(&http, Ok(vec![interface("2001:db8:1:2::abcd/64", false)]));
    peers.refresh();
    assert!(
        !trackers.permits(ip("2001:db8:1:2::1")),
        "one snapshot for both"
    );
    assert_eq!((peers.refused(), trackers.refused()), (0, 2));
}

#[test]
fn canaries_count_what_reaches_them() {
    let tcp = Canary::tcp(LOOPBACK, 0);
    drop(std::net::TcpStream::connect(tcp.addr()).unwrap());
    let udp = Canary::udp(LOOPBACK, 0);
    let socket = std::net::UdpSocket::bind((LOOPBACK, 0)).unwrap();
    socket.send_to(b"ping", udp.addr()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while (tcp.hits(), udp.hits()) != (1, 1) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!((tcp.hits(), udp.hits()), (1, 1));
    let hosts = tcp.hosts("canary.hosts.c27.test");
    let port = tcp.addr().port();
    assert_eq!(
        hosts,
        vec![
            format!("127.0.0.1:{port}"),
            format!("canary.hosts.c27.test:{port}"),
            format!("[::ffff:127.0.0.1]:{port}"),
        ]
    );
}
