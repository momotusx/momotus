use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::hlc::{Hlc, now_ms};
use mq_sync::SyncConfig;
use mq_sync::roster::TokenRequest;
use mq_sync::wire::{
    AccountResponse, PullResponse, PushRequest, PushResponse, RelayInfo, TokenGrant, WireRecord,
};
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
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
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
    async fn account(&self) -> SyncConfig {
        mq_sync::create_account(&self.http, &self.base, None, "test")
            .await
            .unwrap()
    }

    async fn push(
        &self,
        a: &SyncConfig,
        records: Vec<WireRecord>,
    ) -> Result<PushResponse, HttpError> {
        self.http
            .post_json(
                &format!("{}/v1/push", self.base),
                &PushRequest {
                    records,
                    head: a.checkpoint.hash,
                },
                &opts(Some(&a.device_token)),
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
        // Real bodies are at least a nonce and a tag; pad to 48 bytes.
        body: base64::engine::general_purpose::STANDARD.encode(format!("{body:<48}")),
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
            &a,
            vec![record(1, hlc(now, 1), "a1"), record(2, hlc(now, 1), "b1")],
        )
        .await
        .unwrap();
    assert_eq!((r.accepted, r.stale), (2, 0));

    // Older and equal versions are stale; a newer one replaces and moves to the end.
    let r = relay
        .push(
            &a,
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
    assert!(rest.records[0].seq > page.records[0].seq);
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
        // One 48-byte body plus its 96-byte row overhead fits, two do not.
        quota_bytes: 200,
        ..Config::default()
    })
    .await;
    let a = relay.account().await;
    let now = now_ms();
    let r = relay
        .push(&a, vec![record(1, hlc(now + 11 * 60 * 1000, 1), "x")])
        .await
        .unwrap();
    assert_eq!(r.accepted, 0);
    assert!(r.rejected[0].reason.contains("ahead"), "{:?}", r.rejected);

    let wrong = SyncConfig {
        device_token: "wrong".into(),
        ..a.clone()
    };
    let err = relay.push(&wrong, vec![]).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 401, .. }),
        "{err:?}"
    );

    relay
        .push(&a, vec![record(1, hlc(now, 1), "12345678")])
        .await
        .unwrap();
    let err = relay
        .push(&a, vec![record(2, hlc(now, 1), "12345678")])
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 413, .. }),
        "{err:?}"
    );
    // Replacing a record with one of the same size still fits.
    relay
        .push(&a, vec![record(1, hlc(now + 1, 1), "87654321")])
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
    let without = mq_sync::create_account(&relay.http, &relay.base, None, "t").await;
    assert!(
        matches!(
            without,
            Err(mq_sync::SyncError::Http(HttpError::Status {
                status: 403,
                ..
            }))
        ),
        "{without:?}"
    );
    let with = mq_sync::create_account(&relay.http, &relay.base, Some("letmein"), "t").await;
    assert!(with.is_ok(), "{with:?}");
}

#[tokio::test]
async fn head_epochs_canonical_clocks_and_device_tokens() {
    let relay = relay(Config::default()).await;
    let a = relay.account().await;
    let now = now_ms();
    relay
        .push(&a, vec![record(1, hlc(now, 1), "v0")])
        .await
        .unwrap();
    // An epoch other than the account's is refused, the whole push (409),
    // so nothing is taken from the device's queue.
    let mut rotated = record(1, hlc(now, 1), "v1");
    rotated.epoch = 1;
    let err = relay.push(&a, vec![rotated.clone()]).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 409, .. }),
        "{err:?}"
    );
    // Once the account moved to epoch 1 (key rotation), the same hlc
    // re-sealed under it replaces the old body, and epoch 0 is refused.
    rusqlite::Connection::open(relay._dir.0.join("relay.sqlite"))
        .unwrap()
        .execute("UPDATE account SET epoch = 1", [])
        .unwrap();
    let r = relay.push(&a, vec![rotated]).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert!(
        relay
            .push(&a, vec![record(4, hlc(now, 1), "old")])
            .await
            .is_err()
    );
    // A push naming another roster head is refused whole too.
    let elsewhere = SyncConfig {
        checkpoint: mq_sync::wire::RosterHead::default(),
        ..a.clone()
    };
    assert!(relay.push(&elsewhere, vec![]).await.is_err());
    let mut fine = record(5, hlc(now, 1), "v1");
    fine.epoch = 1;
    assert_eq!(relay.push(&a, vec![fine]).await.unwrap().accepted, 1);
    // Too short to be a sealed body.
    let mut empty = record(3, hlc(now, 1), "");
    empty.body = String::new();
    empty.epoch = 1;
    let r = relay.push(&a, vec![empty]).await.unwrap();
    assert_eq!(r.rejected[0].code, mq_sync::wire::Refusal::Invalid);
    // Uppercase hex would sort wrongly as text: refused.
    let mut upper = record(2, hlc(now, 1).to_uppercase(), "x");
    upper.epoch = 1;
    let r = relay.push(&a, vec![upper]).await.unwrap();
    assert_eq!(r.rejected.len(), 1);

    let page = relay.pull(&a.device_token, 0, 500).await;
    assert_eq!(page.head, page.last);
    assert!(!page.generation.is_empty());
    assert_eq!(page.records[0].epoch, 1);
    assert_eq!(page.epoch, 1);
    assert_eq!(page.roster.unwrap().hash, a.checkpoint.hash);
}

/// A device member gets a fresh token by signing for this relay, now, once.
#[tokio::test]
async fn a_member_signs_for_a_fresh_token_once() {
    let relay = relay(Config::default()).await;
    let a = relay.account().await;
    let info: RelayInfo = relay
        .http
        .get(&format!("{}/v1/relay", relay.base), &opts(None))
        .await
        .unwrap()
        .json()
        .unwrap();
    let member = a.secret();
    let grant = |request: TokenRequest| TokenGrant {
        sig: member.sign_token(&request),
        request,
    };
    let ask = |grant: TokenGrant| {
        let http = relay.http.clone();
        let url = format!("{}/v1/tokens", relay.base);
        async move {
            http.post_json(&url, &grant, &opts(None))
                .await
                .and_then(|r| r.json::<AccountResponse>())
        }
    };
    let request = TokenRequest::new(info.relay_id, a.keys.account, member.id(), info.server_ms);
    let fresh = ask(grant(request.clone())).await.unwrap();
    assert_ne!(fresh.device_token, a.device_token);
    assert!(
        relay
            .pull(&fresh.device_token, 0, 500)
            .await
            .records
            .is_empty()
    );
    // The same request again (a replayed nonce), one signed for another
    // relay, one from the past, and one signed by someone else: refused.
    assert!(ask(grant(request.clone())).await.is_err());
    let other_relay = TokenRequest::new(
        mq_sync::roster::Hex([9; 16]),
        a.keys.account,
        member.id(),
        info.server_ms,
    );
    assert!(ask(grant(other_relay)).await.is_err());
    let stale = TokenRequest::new(
        info.relay_id,
        a.keys.account,
        member.id(),
        info.server_ms - 6 * 60 * 1000,
    );
    assert!(ask(grant(stale)).await.is_err());
    // A member holds at most three such tokens; more revoke the oldest.
    let mut tokens = vec![fresh.device_token.clone()];
    for _ in 0..3 {
        let request = TokenRequest::new(info.relay_id, a.keys.account, member.id(), info.server_ms);
        tokens.push(ask(grant(request)).await.unwrap().device_token);
    }
    let pulls = |t: String| {
        let http = relay.http.clone();
        let url = format!("{}/v1/pull", relay.base);
        async move { http.get(&url, &opts(Some(&t))).await.is_ok() }
    };
    assert!(!pulls(tokens[0].clone()).await, "the oldest went");
    for t in &tokens[1..] {
        assert!(pulls(t.clone()).await);
    }
    let stranger = mq_sync::roster::MemberSecret::generate();
    let forged = TokenRequest::new(info.relay_id, a.keys.account, member.id(), info.server_ms);
    let forged = TokenGrant {
        sig: stranger.sign_token(&forged),
        request: forged,
    };
    assert!(ask(forged).await.is_err());
}
