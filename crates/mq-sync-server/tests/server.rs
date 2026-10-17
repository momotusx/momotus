use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::hlc::{Hlc, now_ms};
use mq_sync::wire::{AccountResponse, PullResponse, PushRequest, PushResponse, WireRecord};
use mq_sync_server::{Config, router};

struct Relay {
    base: String,
    http: HttpClient,
    _dir: TempDir,
}

struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn relay(config: Config) -> Relay {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mq-sync-server-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = router(&dir.join("relay.sqlite"), config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Relay {
        base,
        http: HttpClient::new(ClientConfig::default()).unwrap(),
        _dir: TempDir(dir),
    }
}

fn opts(token: Option<&str>) -> RequestOptions {
    RequestOptions {
        allow_private: true,
        headers: token
            .map(|t| vec![("authorization".into(), format!("Bearer {t}"))])
            .unwrap_or_default(),
        ..Default::default()
    }
}

impl Relay {
    async fn account(&self) -> AccountResponse {
        self.http
            .post_json(
                &format!("{}/v1/accounts", self.base),
                &serde_json::json!({}),
                &opts(None),
            )
            .await
            .unwrap()
            .json()
            .unwrap()
    }

    async fn push(&self, token: &str, records: Vec<WireRecord>) -> Result<PushResponse, HttpError> {
        self.http
            .post_json(
                &format!("{}/v1/push", self.base),
                &PushRequest { records },
                &opts(Some(token)),
            )
            .await?
            .json()
    }

    async fn pull(&self, token: &str, after: u64, limit: usize) -> PullResponse {
        self.http
            .get(
                &format!("{}/v1/pull?after={after}&limit={limit}", self.base),
                &opts(Some(token)),
            )
            .await
            .unwrap()
            .json()
            .unwrap()
    }
}

fn hlc(wall_ms: u64, device: u64) -> String {
    Hlc {
        wall_ms,
        counter: 0,
        device,
    }
    .encode()
}

fn record(key: u8, hlc: String, body: &str) -> WireRecord {
    use base64::Engine as _;
    WireRecord {
        coll: 3,
        key_id: hex::encode([key; 16]),
        hlc,
        epoch: 0,
        body: base64::engine::general_purpose::STANDARD.encode(body),
        seq: 0,
    }
}

#[tokio::test]
async fn newer_wins_each_replacement_takes_a_seq_and_pull_pages() {
    let relay = relay(Config::default()).await;
    let a = relay.account().await;
    let now = now_ms();
    let r = relay
        .push(
            &a.device_token,
            vec![record(1, hlc(now, 1), "a1"), record(2, hlc(now, 1), "b1")],
        )
        .await
        .unwrap();
    assert_eq!((r.accepted, r.stale), (2, 0));

    // Older and equal versions are stale; a newer one replaces and moves to the end.
    let r = relay
        .push(
            &a.device_token,
            vec![
                record(1, hlc(now - 1, 2), "a0"),
                record(2, hlc(now, 1), "b1"),
                record(1, hlc(now + 1, 2), "a2"),
            ],
        )
        .await
        .unwrap();
    assert_eq!((r.accepted, r.stale), (1, 2));

    let page = relay.pull(&a.device_token, 0, 1).await;
    assert_eq!((page.records.len(), page.more), (1, true));
    assert_eq!(page.records[0].key_id, hex::encode([2u8; 16]));
    let rest = relay.pull(&a.device_token, page.last, 500).await;
    assert_eq!((rest.records.len(), rest.more), (1, false));
    assert_eq!(rest.records[0].hlc, hlc(now + 1, 2));
    assert_eq!(rest.records[0].seq, 3);
    assert!(
        relay
            .pull(&a.device_token, rest.last, 500)
            .await
            .records
            .is_empty()
    );

    // Accounts never see each other's records.
    let b = relay.account().await;
    assert!(relay.pull(&b.device_token, 0, 500).await.records.is_empty());
}

#[tokio::test]
async fn refuses_future_clocks_bad_tokens_and_overflowing_accounts() {
    let relay = relay(Config {
        quota_bytes: 10,
        ..Config::default()
    })
    .await;
    let a = relay.account().await;
    let now = now_ms();
    let r = relay
        .push(
            &a.device_token,
            vec![record(1, hlc(now + 11 * 60 * 1000, 1), "x")],
        )
        .await
        .unwrap();
    assert_eq!(r.accepted, 0);
    assert!(r.rejected[0].reason.contains("ahead"), "{:?}", r.rejected);

    let err = relay.push("wrong", vec![]).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 401, .. }),
        "{err:?}"
    );

    relay
        .push(&a.device_token, vec![record(1, hlc(now, 1), "12345678")])
        .await
        .unwrap();
    let err = relay
        .push(&a.device_token, vec![record(2, hlc(now, 1), "12345678")])
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 413, .. }),
        "{err:?}"
    );
    // Replacing a record with one of the same size still fits.
    relay
        .push(
            &a.device_token,
            vec![record(1, hlc(now + 1, 1), "87654321")],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn invite_gates_sign_up() {
    let relay = relay(Config {
        invite: Some("letmein".into()),
        ..Config::default()
    })
    .await;
    let url = format!("{}/v1/accounts", relay.base);
    let err = relay
        .http
        .post_json(&url, &serde_json::json!({}), &opts(None))
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::Status { status: 403, .. }));
    let mut with = opts(None);
    with.headers.push(("x-invite".into(), "letmein".into()));
    assert!(
        relay
            .http
            .post_json(&url, &serde_json::json!({}), &with)
            .await
            .is_ok()
    );
}
