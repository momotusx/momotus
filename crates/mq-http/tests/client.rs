use mq_http::{
    Body, ClientConfig, HttpClient, HttpError, Method, Priority, RequestOptions, Url, test_dns,
};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
                    kept.lock().unwrap().push(head);
                    let _ = socket.write_all(route(path).await.as_bytes()).await;
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

async fn read_head(socket: &mut tokio::net::TcpStream) -> String {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

fn reply(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A listener on a not-public address no scope here allows, counting the
/// connections it accepts: `127.0.0.3` on Linux, `[::1]` on macOS (lo0 has
/// only 127.0.0.1 and ::1). Hostile URLs are built from its address, so a
/// check that does not run lets them connect. `port` 0 picks one; a name test
/// passes the origin's, since a name's URL has one port for every answer.
struct Canary {
    addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
}

impl Canary {
    async fn start(port: u16) -> Canary {
        let ip: IpAddr = if cfg!(target_os = "linux") {
            Ipv4Addr::new(127, 0, 0, 3).into()
        } else {
            Ipv6Addr::LOCALHOST.into()
        };
        let listener = TcpListener::bind((ip, port))
            .await
            .expect("the canary must bind");
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = accepted.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                drop(socket);
            }
        });
        Canary { addr, accepted }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Its literal, its mapped literal when it is IPv4, and `name` pointed at
    /// it through `test-dns`.
    fn urls(&self, name: &str) -> Vec<String> {
        let (ip, port) = (self.addr.ip(), self.addr.port());
        test_dns::set(name, &[ip]);
        let mut urls = vec![
            format!("http://{}/", self.addr),
            format!("http://{name}:{port}/"),
        ];
        if let IpAddr::V4(v4) = ip {
            urls.push(format!("http://[::ffff:{v4}]:{port}/"));
        }
        urls
    }
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
    let canary = Canary::start(origin.addr.port()).await;
    test_dns::set("flip.c23.test", &[LOOPBACK]);
    let http = scoped(&[LOOPBACK]);
    let url = format!("http://flip.c23.test:{}/", origin.addr.port());
    let ok = within("first", http.get(&url, &playback()))
        .await
        .expect("the first answer is the origin");
    assert_eq!(&ok.body[..], b"ok");
    test_dns::set("flip.c23.test", &[canary.addr.ip()]);
    refused(&http, &url).await;
    assert_eq!(canary.accepted(), 0);
    assert_eq!(origin.heads().len(), 1);
}

#[tokio::test]
async fn mixed_answers_never_reach_the_denied_address() {
    let origin = Origin::ok(0).await;
    let canary = Canary::start(origin.addr.port()).await;
    // The canary first: a resolver that passes the list on connects to it.
    test_dns::set("mixed.c23.test", &[canary.addr.ip(), LOOPBACK]);
    let url = format!("http://mixed.c23.test:{}/", origin.addr.port());
    let ok = within("mixed", scoped(&[LOOPBACK]).get(&url, &playback()))
        .await
        .expect("reaches the origin");
    assert_eq!(&ok.body[..], b"ok");
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn a_redirect_outside_the_scope_never_connects() {
    let canary = Canary::start(0).await;
    let targets = canary.urls("redirect.c23.test");
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
        let canary_ip = canary.addr.ip().to_string();
        assert!(!err.to_string().contains(&canary_ip), "{err}");
    }
    assert_eq!(canary.accepted(), 0);
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
    let canary = Canary::start(0).await;
    if let IpAddr::V4(v4) = canary.addr.ip() {
        let url = format!("http://[::ffff:{v4}]:{}/", canary.addr.port());
        refused(&scoped(&[LOOPBACK]), &url).await;
    }
    assert_eq!(canary.accepted(), 0);
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
    let canary = Canary::start(0).await;
    let http = scoped(&[LOOPBACK]);
    // A literal (the check before connecting) and a name (the resolver).
    for url in &canary.urls("typed.c23.test")[..2] {
        let err = refused(&http, url).await;
        assert!(
            !err.to_string().contains(&canary.addr.ip().to_string()),
            "{err}"
        );
        let Err(err) = within(url, http.stream(url, None, &playback())).await else {
            panic!("{url} streamed");
        };
        assert_eq!(err.kind(), "blocked address", "{url}: {err}");
    }
    assert_eq!(canary.accepted(), 0);
}
