//! The webOS transport against a fake host (plans/webos.md 3.2): hops,
//! credentials on a cross-host redirect, the cap after gzip, private
//! addresses, the time left, and an abort.

use super::imports::FAKE;
use super::*;
use crate::{ClientConfig, HopRule, test_dns};
use serde_json::{Value, json};

const PUBLIC_A: &str = "93.184.216.34";
const PUBLIC_B: &str = "93.184.216.35";

/// A fresh fake host: `names` it resolves, `answer` for every fetch.
fn host(names: &[(&str, &[&str])], answer: impl Fn(&Value) -> Value + 'static) {
    FAKE.with(|f| {
        let mut f = f.borrow_mut();
        *f = Default::default();
        for (name, list) in names {
            let list = list.iter().map(|a| a.to_string()).collect();
            f.names.insert(name.to_string(), list);
        }
        f.answer = Some(Box::new(answer));
    });
}

fn requests() -> Vec<Value> {
    FAKE.with(|f| f.borrow().requests.clone())
}

fn url_of(request: &Value) -> &str {
    request["url"].as_str().unwrap()
}

fn header(request: &Value, name: &str) -> Option<String> {
    request["headers"].as_array().unwrap().iter().find_map(|h| {
        (h[0].as_str()?.eq_ignore_ascii_case(name)).then(|| h[1].as_str().unwrap().to_owned())
    })
}

fn ok(body: &str) -> Value {
    json!({"status": 200, "headers": [["Content-Type", "text/plain"]], "body": STANDARD.encode(body)})
}

fn moved(status: u16, to: &str) -> Value {
    json!({"status": status, "headers": [["Location", to]], "body": ""})
}

fn client() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn request(method: Method, url: &str, opts: &RequestOptions) -> Request {
    Request {
        method,
        url: Url::parse(url).unwrap(),
        body: None,
        range: None,
        timeout: Some(opts.timeout),
    }
}

async fn get(http: &HttpClient, url: &str, opts: &RequestOptions) -> Result<Response, HttpError> {
    http.host_send(request(Method::GET, url, opts), opts).await
}

#[tokio::test]
async fn a_redirect_chain_is_followed_hop_by_hop() {
    host(
        &[
            ("a.example", &[PUBLIC_A]),
            ("b.example", &["2606:2800::1", PUBLIC_B]),
        ],
        |r| match url_of(r) {
            "https://a.example/m" => moved(302, "https://b.example/x"),
            _ => ok("done"),
        },
    );
    let opts = RequestOptions::default();
    let response = get(&client(), "https://a.example/m", &opts).await.unwrap();
    assert_eq!(&response.body[..], b"done");
    assert_eq!(response.final_url.as_str(), "https://b.example/x");
    assert_eq!(response.header("content-type"), Some("text/plain"));
    let sent = requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0]["addresses"], json!([PUBLIC_A]));
    // The host connects to approved addresses only, IPv4 first.
    assert_eq!(sent[1]["addresses"], json!([PUBLIC_B, "2606:2800::1"]));
    assert_eq!(sent[1]["method"], "GET");
    assert_eq!(sent[1]["maxBytes"], json!(opts.max_bytes));
    assert!(sent[1]["timeoutMs"].as_u64().unwrap() <= 10_000);
    assert!(sent[1].get("body").is_none() && sent[1].get("prefixBytes").is_none());
    assert_eq!(header(&sent[0], "host"), None, "the host sets Host");
    assert!(
        header(&sent[0], "user-agent")
            .unwrap()
            .starts_with("Momotus/")
    );

    // RFC 9110 15.4: a 303 turns a POST into a GET without its body.
    host(&[("a.example", &[PUBLIC_A])], |r| {
        match r["method"].as_str() {
            Some("POST") => moved(303, "/done"),
            _ => ok("after"),
        }
    });
    let mut post = request(Method::POST, "https://a.example/form", &opts);
    post.body = Some(Body::Json(json!({"k": 1})));
    client().host_send(post, &opts).await.unwrap();
    let sent = requests();
    assert_eq!(
        header(&sent[0], "content-type").unwrap(),
        "application/json"
    );
    assert_eq!(sent[0]["body"], STANDARD.encode(br#"{"k":1}"#));
    assert_eq!(
        (sent[1]["method"].as_str(), sent[1].get("body")),
        (Some("GET"), None)
    );
}

#[tokio::test]
async fn credentials_reach_only_their_own_origin() {
    host(
        &[("a.example", &[PUBLIC_A]), ("b.example", &[PUBLIC_B])],
        |r| match url_of(r) {
            "https://a.example/m" => moved(302, "https://a.example/again"),
            "https://a.example/again" => moved(302, "https://b.example/cdn"),
            _ => ok("x"),
        },
    );
    let opts = RequestOptions {
        headers: vec![
            ("X-Api-Key".into(), "secret".into()),
            ("Referer".into(), "https://site.example/".into()),
        ],
        ..Default::default()
    };
    get(&client(), "https://user:pa%20ss@a.example/m", &opts)
        .await
        .unwrap();
    let sent = requests();
    let basic = format!("Basic {}", STANDARD.encode("user:pa ss"));
    for same in &sent[..2] {
        assert!(!url_of(same).contains("user"), "userinfo goes as a header");
        assert_eq!(header(same, "authorization"), Some(basic.clone()));
        assert_eq!(header(same, "x-api-key").as_deref(), Some("secret"));
    }
    let other = &sent[2];
    assert_eq!(url_of(other), "https://b.example/cdn");
    assert_eq!(header(other, "authorization"), None);
    assert_eq!(header(other, "x-api-key"), None);
    assert_eq!(
        header(other, "referer").as_deref(),
        Some("https://site.example/")
    );
}

#[tokio::test]
async fn the_cap_holds_on_the_decoded_body() {
    let opts = RequestOptions {
        max_bytes: 4,
        ..Default::default()
    };
    // The host decodes gzip and caps what it decoded.
    host(&[("a.example", &[PUBLIC_A])], |_| json!({"error": "cap"}));
    let err = get(&client(), "https://a.example/big", &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::TooLarge { limit: 4 }), "{err:?}");
    assert_eq!(requests()[0]["maxBytes"], json!(4));
    // A host that let more through is still held to the cap.
    host(&[("a.example", &[PUBLIC_A])], |_| ok("12345"));
    let err = get(&client(), "https://a.example/big", &opts)
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::TooLarge { .. }), "{err:?}");
}

#[tokio::test]
async fn private_addresses_are_refused() {
    host(
        &[
            ("lan.example", &["192.168.1.1"]),
            ("mixed.example", &["10.0.0.1", PUBLIC_A]),
            ("a.example", &[PUBLIC_A]),
        ],
        |r| match url_of(r) {
            "https://a.example/hop" => moved(302, "https://10.0.0.1/admin"),
            _ => ok("x"),
        },
    );
    let http = client();
    let opts = RequestOptions::default();
    // A public name that leads only to the LAN never reaches the host.
    let err = get(&http, "https://lan.example/", &opts).await.unwrap_err();
    assert!(err.unresolved(), "{err:?}");
    let err = get(&http, "http://192.168.1.1/", &opts).await.unwrap_err();
    assert!(matches!(err, HttpError::BlockedPrivate(_)), "{err:?}");
    let err = get(&http, "https://a.example/hop", &opts)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("public to a private"), "{err}");
    // Mixed answers keep only the public one.
    get(&http, "https://mixed.example/", &opts).await.unwrap();
    let sent = requests();
    assert_eq!(
        sent.len(),
        2,
        "only a.example/hop and mixed.example went out"
    );
    assert_eq!(sent[1]["addresses"], json!([PUBLIC_A]));
    // A local addon may reach the LAN.
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    get(&http, "http://192.168.1.1/manifest.json", &local)
        .await
        .unwrap();

    // A playback's scope: the LAN literal and LAN answers are refused.
    test_dns::set_interfaces(&http, Ok(Vec::new()));
    let scoped = http.scoped(&[]);
    let err = get(&scoped, "http://192.168.1.1/v.mkv", &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HttpError::BlockedPrivate(m) if m == OUT_OF_SCOPE),
        "{err:?}"
    );
    let err = get(&scoped, "https://lan.example/v.mkv", &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HttpError::BlockedPrivate(m) if m == OUT_OF_SCOPE),
        "{err:?}"
    );
    get(&scoped, "https://mixed.example/v.mkv", &opts)
        .await
        .unwrap();
    assert_eq!(requests().last().unwrap()["addresses"], json!([PUBLIC_A]));
}

#[tokio::test]
async fn downgrades_foreign_hops_and_long_chains_are_refused() {
    host(
        &[("a.example", &[PUBLIC_A]), ("b.example", &[PUBLIC_B])],
        |r| match url_of(r) {
            "https://a.example/down" => moved(301, "http://a.example/down"),
            "https://a.example/away" => moved(302, "https://b.example/x"),
            url => {
                let n: u32 = url.rsplit('/').next().unwrap().parse().unwrap_or(0);
                moved(302, &format!("/{}", n + 1))
            }
        },
    );
    let http = client();
    let opts = RequestOptions::default();
    let err = get(&http, "https://a.example/down", &opts)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("https to http"), "{err}");
    let rule = http.with_hops(HopRule::under("a.example"));
    let err = get(&rule, "https://a.example/away", &opts)
        .await
        .unwrap_err();
    assert!(err.hop_refused(), "{err:?}");
    assert_eq!(requests().len(), 2, "b.example never asked");
    let err = get(&http, "https://a.example/0", &opts).await.unwrap_err();
    assert!(err.to_string().contains("too many redirects"), "{err}");
    assert_eq!(requests().len(), 2 + MAX_REDIRECTS + 1);
}

#[tokio::test]
async fn a_hop_gets_the_time_left_and_a_timeout_is_a_timeout() {
    host(
        &[("a.example", &[PUBLIC_A])],
        |_| json!({"error": "timeout"}),
    );
    let opts = RequestOptions {
        timeout: Duration::from_secs(3),
        ..Default::default()
    };
    let err = get(&client(), "https://a.example/slow", &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Timeout(t) if t == opts.timeout),
        "{err:?}"
    );
    let ms = requests()[0]["timeoutMs"].as_u64().unwrap();
    assert!((2_900..=3_000).contains(&ms), "{ms}");
    let short = RequestOptions {
        hop_headers: Some(Duration::from_millis(300)),
        ..opts
    };
    let _ = get(&client(), "https://a.example/slow", &short).await;
    let ms = requests()[1]["timeoutMs"].as_u64().unwrap();
    assert!(ms <= 300, "{ms}");
}

/// A request waiting for its host's slot fails at its own deadline, not
/// when the slot frees.
#[tokio::test]
async fn the_wait_for_a_slot_ends_at_the_deadline() {
    host(&[("a.example", &[PUBLIC_A])], |_| Value::Null);
    let http = HttpClient::new(ClientConfig {
        per_host_limit: 1,
        ..ClientConfig::default()
    })
    .unwrap();
    let hold = RequestOptions::default();
    let holder = get(&http, "https://a.example/hold", &hold);
    let opts = RequestOptions {
        timeout: Duration::from_secs(1),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let (_, waiter) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(4), holder),
        async {
            tokio::task::yield_now().await;
            let answer = get(&http, "https://a.example/next", &opts).await;
            (answer, started.elapsed())
        }
    );
    let (answer, waited) = waiter;
    assert!(matches!(answer, Err(HttpError::Timeout(_))), "{answer:?}");
    assert!(waited < Duration::from_millis(2_000), "{waited:?}");
    assert_eq!(requests().len(), 1, "the waiter never sent");
}

#[tokio::test]
async fn a_dropped_request_aborts_its_fetch() {
    host(&[("a.example", &[PUBLIC_A])], |_| Value::Null);
    let http = client();
    let opts = RequestOptions::default();
    let waited = tokio::time::timeout(
        Duration::from_millis(50),
        get(&http, "https://a.example/never", &opts),
    )
    .await;
    assert!(waited.is_err());
    let aborted = FAKE.with(|f| f.borrow().aborted.clone());
    assert_eq!(aborted.len(), 1);
    // A late answer for it is dropped, and its slot is gone.
    fetched(aborted[0], &ok("late").to_string());
    assert!(!lock(&FETCHES).contains_key(&aborted[0]));
}

#[tokio::test]
async fn statuses_failures_and_prefixes() {
    host(&[("a.example", &[PUBLIC_A])], |r| match url_of(r) {
        "https://a.example/busy" => {
            json!({"status": 429, "headers": [["Retry-After", "7"]], "body": STANDARD.encode("wait")})
        }
        "https://a.example/refused" => json!({"error": "refused"}),
        "https://a.example/odd" => json!({"nothing": true}),
        _ => {
            json!({"status": 403, "headers": [], "body": STANDARD.encode("abc"), "truncated": true})
        }
    });
    let http = client();
    let opts = RequestOptions::default();
    match get(&http, "https://a.example/busy", &opts).await {
        Err(HttpError::Status {
            status: 429,
            body,
            retry_after,
        }) => assert_eq!(
            (&body[..], retry_after),
            (&b"wait"[..], Some(Duration::from_secs(7)))
        ),
        other => panic!("{other:?}"),
    }
    let err = get(&http, "https://a.example/refused", &opts)
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Network(_)) && !err.unresolved(),
        "{err:?}"
    );
    let err = get(&http, "https://a.example/odd", &opts)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), "network");
    let err = get(&http, "https://nowhere.example/", &opts)
        .await
        .unwrap_err();
    assert!(err.unresolved(), "{err:?}");
    // A prefix comes back whatever its status, never as too large.
    let prefix = http
        .get_prefix("https://a.example/manifest", 0, 1024, &opts)
        .await
        .unwrap();
    assert_eq!((prefix.response.status, prefix.truncated), (403, true));
    assert_eq!(&prefix.response.body[..], b"abc");
    let sent = requests().pop().unwrap();
    assert_eq!(sent["prefixBytes"], json!(1024));
    assert_eq!(header(&sent, "range").as_deref(), Some("bytes=0-1023"));
    assert_eq!(
        header(&sent, "accept-encoding").as_deref(),
        Some("identity")
    );
}
