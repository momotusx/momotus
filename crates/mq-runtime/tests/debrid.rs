//! Debrid accounts (plans/c10-handoff.md section A3) against local mock
//! services.

#[path = "../../mq-debrid/tests/common/mod.rs"]
mod common;

use common::{Server, route};
use mq_debrid::ResolveRequest;
use mq_runtime::{
    AccountState, Core, DebridCredential, DebridLoginSnapshot, DebridLoginView, DebridRecord,
    DebridSnapshot, DebridTombstone, DebridView, StoreId,
};
use std::time::{Duration, Instant};

const HOUR_MS: u64 = 3_600_000;

fn now_ms() -> u64 {
    mq_library::hlc::now_ms()
}

fn snapshot(core: &Core) -> DebridSnapshot {
    DebridView::open(core).subscribe().0
}

/// Waits up to 10 s for `done`.
async fn until(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out: {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn generation(core: &Core, service: &str) -> Option<String> {
    snapshot(core)
        .accounts
        .into_iter()
        .find(|a| a.service == service)
        .map(|a| a.generation)
}

fn services(core: &Core) -> Vec<String> {
    snapshot(core)
        .accounts
        .into_iter()
        .map(|a| a.service)
        .collect()
}

fn rd_record(generation: &str, access: &str, issued_at_ms: u64) -> DebridRecord {
    DebridRecord {
        service: "realdebrid".into(),
        generation: generation.into(),
        revision: 1,
        credential: DebridCredential::Oauth {
            client_id: "CID".into(),
            client_secret: "CSECRET".into(),
            refresh_token: "REF".into(),
            access_token: access.into(),
            issued_at_ms,
            expires_in_ms: HOUR_MS,
        },
    }
}

fn key(record: &DebridRecord) -> &str {
    match &record.credential {
        DebridCredential::Key { key } => key,
        DebridCredential::Oauth { access_token, .. } => access_token,
    }
}

const G1: &str = "0123456789abcdef0123456789abcdef";
const G2: &str = "fedcba9876543210fedcba9876543210";

#[tokio::test]
async fn set_debrid_keys_keeps_its_contract() {
    let core = Core::temporary("owner").unwrap();
    assert_eq!(
        core.set_debrid_keys(&[
            (StoreId::TorBox, "tb".into()),
            (StoreId::AllDebrid, "ad".into())
        ]),
        2
    );
    assert_eq!(services(&core), ["torbox", "alldebrid"]);
    assert_eq!(
        core.debrid().unwrap().stores(),
        [StoreId::TorBox, StoreId::AllDebrid]
    );
    assert!(
        snapshot(&core).pending.is_empty(),
        "the app already has them"
    );
    let torbox = generation(&core, "torbox").unwrap();

    // An OAuth connection the legacy call does not list stays.
    core.restore_debrid(vec![rd_record(G1, "ACC", now_ms())], vec![])
        .unwrap();
    assert_eq!(core.set_debrid_keys(&[(StoreId::TorBox, "tb".into())]), 2);
    assert_eq!(services(&core), ["torbox", "realdebrid"]);
    assert_eq!(
        generation(&core, "torbox").unwrap(),
        torbox,
        "unchanged key"
    );
    assert_eq!(generation(&core, "realdebrid").unwrap(), G1);

    // A changed key is a new connection; a listed OAuth service is replaced.
    assert_eq!(
        core.set_debrid_keys(&[
            (StoreId::RealDebrid, "rdkey".into()),
            (StoreId::TorBox, "tb2".into())
        ]),
        2
    );
    assert_eq!(services(&core), ["realdebrid", "torbox"]);
    assert_ne!(generation(&core, "torbox").unwrap(), torbox);
    assert_ne!(generation(&core, "realdebrid").unwrap(), G1);

    assert_eq!(core.set_debrid_keys(&[]), 0);
    assert!(core.debrid().is_none());
}

#[tokio::test]
async fn restore_refuses_a_tombstoned_generation() {
    let core = Core::temporary("owner").unwrap();
    let record = DebridRecord {
        service: "torbox".into(),
        generation: G1.into(),
        revision: 1,
        credential: DebridCredential::Key { key: "tb".into() },
    };
    let answer = core
        .restore_debrid(
            vec![record],
            vec![
                DebridTombstone {
                    service: "torbox".into(),
                    generation: G1.into(),
                },
                DebridTombstone {
                    service: "alldebrid".into(),
                    generation: G2.into(),
                },
            ],
        )
        .unwrap();
    assert_eq!(answer.refused, [G1]);
    // Only the tombstone without a record may go; the other waits for its
    // record to be deleted.
    assert_eq!(
        answer.tombstones,
        [DebridTombstone {
            service: "alldebrid".into(),
            generation: G2.into()
        }]
    );
    assert!(services(&core).is_empty());
    assert!(
        core.restore_debrid(
            vec![],
            vec![DebridTombstone {
                service: "nope".into(),
                generation: G1.into()
            }]
        )
        .is_err()
    );
}

#[tokio::test]
async fn restore_refuses_a_disconnected_generation() {
    let core = Core::temporary("owner").unwrap();
    let record = rd_record(G1, "ACC", now_ms());
    core.restore_debrid(vec![record.clone()], vec![]).unwrap();
    core.disconnect_debrid(StoreId::RealDebrid, G1);
    assert!(services(&core).is_empty());
    let answer = core.restore_debrid(vec![record], vec![]).unwrap();
    assert_eq!(answer.refused, [G1]);
    assert!(services(&core).is_empty());
}

#[tokio::test]
async fn stale_ack_is_ignored() {
    let core = Core::temporary("owner").unwrap();
    let record = core.set_debrid_key(StoreId::Premiumize, "pm");
    assert_eq!(record.revision, 1);
    assert_eq!(snapshot(&core).pending, std::slice::from_ref(&record));
    core.ack_debrid("premiumize", &record.generation, 2);
    core.ack_debrid("premiumize", G1, 1);
    assert_eq!(snapshot(&core).pending.len(), 1);
    core.ack_debrid("premiumize", &record.generation, 1);
    assert!(snapshot(&core).pending.is_empty());

    // A replacement supersedes a record that was never acknowledged.
    let first = core.set_debrid_key(StoreId::Premiumize, "pm2");
    let second = core.set_debrid_key(StoreId::Premiumize, "pm3");
    assert_eq!(snapshot(&core).pending, std::slice::from_ref(&second));
    core.ack_debrid("premiumize", &first.generation, 1);
    assert_eq!(snapshot(&core).pending, [second]);
}

#[tokio::test]
async fn ack_after_disconnect_is_ignored() {
    let core = Core::temporary("owner").unwrap();
    let record = core.set_debrid_key(StoreId::AllDebrid, "ad");
    core.disconnect_debrid(StoreId::AllDebrid, &record.generation);
    assert_eq!(snapshot(&core), DebridSnapshot::default());
    core.ack_debrid("alldebrid", &record.generation, 1);
    assert_eq!(snapshot(&core), DebridSnapshot::default());
}

const RD_CODE: &str = r#"{"device_code":"DEVSECRET","user_code":"ABCD1234","interval":1,"expires_in":900,"verification_url":"https://real-debrid.com/device","direct_verification_url":"https://real-debrid.com/authorize?device_id=DEVSECRET"}"#;

#[tokio::test]
async fn rd_device_flow_ends_in_an_oauth_record() {
    let server = Server::start(vec![
        route("GET", "/oauth/v2/device/code", 200, RD_CODE),
        route(
            "GET",
            "/oauth/v2/device/credentials",
            403,
            r#"{"error":null}"#,
        )
        .then(200, r#"{"client_id":"CID","client_secret":"CSECRET"}"#),
        route(
            "POST",
            "/oauth/v2/token",
            200,
            r#"{"access_token":"ACC","expires_in":3600,"token_type":"Bearer","refresh_token":"REF"}"#,
        ),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let before = now_ms();
    let login = DebridLoginView::open(&core, StoreId::RealDebrid);
    until("the code", || {
        matches!(login.snapshot(), DebridLoginSnapshot::Code { .. })
    })
    .await;
    let DebridLoginSnapshot::Code { user_code, qr, .. } = login.snapshot() else {
        unreachable!()
    };
    assert_eq!(user_code, "ABCD1234");
    assert!(!qr.contains("DEVSECRET"), "{qr}");
    until("done", || login.snapshot() == DebridLoginSnapshot::Done).await;
    let pending = snapshot(&core).pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].service, "realdebrid");
    assert_eq!(pending[0].revision, 1);
    assert_eq!(pending[0].generation.len(), 32);
    let DebridCredential::Oauth {
        client_id,
        access_token,
        issued_at_ms,
        expires_in_ms,
        ..
    } = &pending[0].credential
    else {
        panic!("expected oauth");
    };
    assert_eq!((client_id.as_str(), access_token.as_str()), ("CID", "ACC"));
    assert!(*issued_at_ms >= before && *issued_at_ms <= now_ms());
    assert_eq!(*expires_in_ms, HOUR_MS);
    assert_eq!(services(&core), ["realdebrid"]);
}

#[tokio::test]
async fn alldebrid_first_poll_waits_5_s() {
    let server = Server::start(vec![
        route(
            "GET",
            "/v4.1/pin/get",
            200,
            r#"{"status":"success","data":{"pin":"Q8W6","check":"c","expires_in":600,"user_url":"https://alldebrid.com/pin/?pin=Q8W6","base_url":"https://alldebrid.com/pin/"}}"#,
        ),
        route(
            "POST",
            "/v4/pin/check",
            200,
            r#"{"status":"success","data":{"activated":false,"expires_in":599}}"#,
        ),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let login = DebridLoginView::open(&core, StoreId::AllDebrid);
    until("the code", || {
        matches!(login.snapshot(), DebridLoginSnapshot::Code { .. })
    })
    .await;
    until("the first check", || {
        server.count("POST", "/v4/pin/check") == 1
    })
    .await;
    let requests = server.requests();
    let waited = requests[1].at - requests[0].at;
    assert!(
        waited >= Duration::from_secs(5) && waited < Duration::from_secs(6),
        "first check after {waited:?}"
    );
}

const TB_START: &str = r#"{"success":true,"error":null,"detail":"ok","data":{"device_code":"TBDEVICE","code":"123456","interval":1,"expires_in":600,"verification_url":"https://torbox.app/oauth/device?app=Marquee","verification_url_complete":"https://torbox.app/oauth/device?app=Marquee&code=123456"}}"#;
const TB_PENDING: &str =
    r#"{"success":false,"error":"DEVICE_CODE_NOT_USED","detail":"wait","data":null}"#;
const TB_PATH: &str = "/v1/api/user/auth/device/token";

fn tb_token(key: &str) -> String {
    format!(r#"{{"success":true,"error":null,"detail":"ok","data":{{"access_token":"{key}"}}}}"#)
}

/// TorBox answers "pending" once, then the key after `delay`.
async fn torbox(delay: Duration) -> Server {
    Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, TB_START),
        route("POST", TB_PATH, 400, TB_PENDING)
            .then(200, &tb_token("KEY1"))
            .delayed(delay)
            .then(200, &tb_token("KEY2")),
    ])
    .await
}

async fn collection_in_flight(server: &Server) {
    until("the second poll", || server.count("POST", TB_PATH) == 2).await;
}

#[tokio::test]
async fn torbox_collection_survives_view_close() {
    let server = torbox(Duration::from_millis(800)).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    collection_in_flight(&server).await;
    drop(login);
    until("the pending key", || !snapshot(&core).pending.is_empty()).await;
    let pending = snapshot(&core).pending;
    assert_eq!(key(&pending[0]), "KEY1");
    assert_eq!(services(&core), ["torbox"]);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(server.count("POST", TB_PATH), 2, "no poll after the close");
}

#[tokio::test]
async fn late_collection_after_replacement_is_discarded() {
    let server = torbox(Duration::from_millis(800)).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    collection_in_flight(&server).await;
    let typed = core.set_debrid_key(StoreId::TorBox, "TYPED");
    until("used", || login.snapshot() == DebridLoginSnapshot::Used).await;
    assert_eq!(snapshot(&core).pending, std::slice::from_ref(&typed));
    assert_eq!(generation(&core, "torbox").unwrap(), typed.generation);
}

#[tokio::test]
async fn late_collection_after_disconnect_is_discarded() {
    let server = torbox(Duration::from_millis(800)).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let typed = core.set_debrid_key(StoreId::TorBox, "TYPED");
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    collection_in_flight(&server).await;
    core.disconnect_debrid(StoreId::TorBox, &typed.generation);
    until("used", || login.snapshot() == DebridLoginSnapshot::Used).await;
    assert_eq!(snapshot(&core), DebridSnapshot::default());
}

#[tokio::test]
async fn superseding_login_discards_the_older() {
    let server = torbox(Duration::from_millis(1500)).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let older = DebridLoginView::open(&core, StoreId::TorBox);
    collection_in_flight(&server).await;
    let newer = DebridLoginView::open(&core, StoreId::TorBox);
    until("newer done", || {
        newer.snapshot() == DebridLoginSnapshot::Done
    })
    .await;
    // One request in flight per service: the newer sign-in's first poll
    // waits for the older one's answer (1.5 s), not just its own second.
    let polls: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.target == TB_PATH)
        .map(|r| r.at)
        .collect();
    assert!(
        polls[2] - polls[1] >= Duration::from_millis(1400),
        "{:?}",
        polls[2] - polls[1]
    );
    until("older used", || {
        older.snapshot() == DebridLoginSnapshot::Used
    })
    .await;
    let pending = snapshot(&core).pending;
    assert_eq!(pending.len(), 1);
    assert_eq!(key(&pending[0]), "KEY2");
}

#[tokio::test]
async fn rd_refresh_rotates_into_pending() {
    let server = Server::start(vec![route(
        "POST",
        "/oauth/v2/token",
        200,
        r#"{"access_token":"NEW","expires_in":3600,"token_type":"Bearer","refresh_token":"REF2"}"#,
    )])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    // Issued 50 minutes ago: past the 45 minute mark, so at once.
    let issued = now_ms() - 50 * 60_000;
    core.restore_debrid(vec![rd_record(G1, "OLD", issued)], vec![])
        .unwrap();
    until("the rotated record", || !snapshot(&core).pending.is_empty()).await;
    let record = &snapshot(&core).pending[0];
    assert_eq!((record.generation.as_str(), record.revision), (G1, 2));
    let DebridCredential::Oauth {
        access_token,
        refresh_token,
        issued_at_ms,
        ..
    } = &record.credential
    else {
        panic!("expected oauth");
    };
    assert_eq!(
        (access_token.as_str(), refresh_token.as_str()),
        ("NEW", "REF2")
    );
    assert!(*issued_at_ms > issued);
    let body = &server.requests()[0].body;
    assert!(body.contains("code=REF") && body.contains("client_secret=CSECRET"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.count("POST", "/oauth/v2/token"), 1, "one refresh");
}

#[tokio::test]
async fn auth_error_on_refresh_moves_to_reconnect() {
    let server = Server::start(vec![route(
        "POST",
        "/oauth/v2/token",
        400,
        r#"{"error":"invalid_grant"}"#,
    )])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    core.restore_debrid(vec![rd_record(G1, "OLD", now_ms() + HOUR_MS)], vec![])
        .unwrap();
    until("reconnect", || {
        snapshot(&core)
            .accounts
            .first()
            .is_some_and(|a| a.state == AccountState::Reconnect)
    })
    .await;
    assert!(snapshot(&core).pending.is_empty());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.count("POST", "/oauth/v2/token"), 1, "no retry loop");
}

const RD_READY: &str = r#"{"id":"RD1","filename":"Movie","hash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c","progress":100,"status":"downloaded","files":[{"id":1,"path":"/Movie.mkv","bytes":1000,"selected":1}],"links":["https://real-debrid.com/d/AAA"]}"#;

#[tokio::test]
async fn a_refused_token_refreshes_once_and_retries_that_request() {
    let server = Server::start(vec![
        route(
            "POST",
            "/rest/1.0/torrents/addMagnet",
            401,
            r#"{"error":"bad_token","error_code":8}"#,
        )
        .then(201, r#"{"id":"RD1"}"#),
        route("GET", "/rest/1.0/torrents/info/RD1", 200, RD_READY),
        route(
            "POST",
            "/rest/1.0/unrestrict/link",
            200,
            r#"{"filename":"Movie.mkv","filesize":1000,"download":"https://cdn.example/Movie.mkv"}"#,
        ),
        route(
            "POST",
            "/oauth/v2/token",
            200,
            r#"{"access_token":"NEW","expires_in":3600,"token_type":"Bearer","refresh_token":"REF"}"#,
        ),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    core.restore_debrid(vec![rd_record(G1, "OLD", now_ms())], vec![])
        .unwrap();
    let link = core
        .debrid()
        .unwrap()
        .resolve(ResolveRequest::new(
            "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
        ))
        .await
        .unwrap();
    assert_eq!(link.url, "https://cdn.example/Movie.mkv");
    assert_eq!(server.count("POST", "/oauth/v2/token"), 1);
    let adds: Vec<String> = server
        .requests()
        .into_iter()
        .filter(|r| r.target == "/rest/1.0/torrents/addMagnet")
        .map(|r| r.head)
        .collect();
    assert!(adds[0].contains("bearer old") && adds[1].contains("bearer new"));
    assert!(
        server
            .requests()
            .iter()
            .filter(|r| r.target != "/oauth/v2/token" && r.target != "/rest/1.0/torrents/addMagnet")
            .all(|r| r.head.contains("bearer new")),
        "later requests use the rotated token"
    );
    let record = &snapshot(&core).pending[0];
    assert_eq!(record.revision, 2);
}

#[tokio::test]
async fn a_rejected_key_moves_to_reconnect() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/checkcached",
        401,
        r#"{"success":false,"error":"BAD_TOKEN","detail":"bad","data":null}"#,
    )])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    core.set_debrid_keys(&[(StoreId::TorBox, "bad".into())]);
    let err = core
        .debrid()
        .unwrap()
        .resolve(ResolveRequest::new(
            "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, mq_debrid::DebridError::Auth), "{err:?}");
    assert_eq!(snapshot(&core).accounts[0].state, AccountState::Reconnect);
}

#[tokio::test]
async fn a_refresh_after_disconnect_is_dropped() {
    let server = Server::start(vec![
        route(
            "POST",
            "/oauth/v2/token",
            200,
            r#"{"access_token":"NEW","expires_in":3600,"token_type":"Bearer","refresh_token":"REF2"}"#,
        )
        .delayed(Duration::from_millis(500)),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    core.restore_debrid(vec![rd_record(G1, "OLD", now_ms() - 50 * 60_000)], vec![])
        .unwrap();
    until("the refresh in flight", || {
        server.count("POST", "/oauth/v2/token") == 1
    })
    .await;
    core.disconnect_debrid(StoreId::RealDebrid, G1);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(snapshot(&core), DebridSnapshot::default());
}

#[tokio::test]
async fn a_token_without_a_lifetime_is_not_refreshed_in_a_loop() {
    let server = Server::start(vec![route(
        "POST",
        "/oauth/v2/token",
        200,
        r#"{"access_token":"NEW","expires_in":0,"token_type":"Bearer","refresh_token":"REF"}"#,
    )])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    core.restore_debrid(vec![rd_record(G1, "OLD", now_ms() - 50 * 60_000)], vec![])
        .unwrap();
    until("the first refresh", || {
        server.count("POST", "/oauth/v2/token") == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(server.count("POST", "/oauth/v2/token"), 1);
}

#[tokio::test]
async fn slow_down_adds_5_s() {
    let server = Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, TB_START),
        route(
            "POST",
            TB_PATH,
            429,
            r#"{"success":false,"error":"COOLDOWN_LIMIT","detail":"slow","data":null}"#,
        )
        .then(400, TB_PENDING),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let _login = DebridLoginView::open(&core, StoreId::TorBox);
    until("two polls", || server.count("POST", TB_PATH) == 2).await;
    let polls: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|r| r.target == TB_PATH)
        .map(|r| r.at)
        .collect();
    // The interval was 1 s; after the slow-down it is 6 s.
    let gap = polls[1] - polls[0];
    assert!(
        gap >= Duration::from_secs(6) && gap < Duration::from_secs(7),
        "{gap:?}"
    );
}

#[tokio::test]
async fn polling_through_network_failures_ends_at_expiry() {
    let start = TB_START.replace(r#""expires_in":600"#, r#""expires_in":2"#);
    let server = Server::start(vec![
        route("GET", "/v1/api/user/auth/device/start", 200, &start),
        // Not JSON: the answer cannot be read, like a broken connection.
        route("POST", TB_PATH, 200, "<html>"),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&server.base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    until("expired", || {
        login.snapshot() == DebridLoginSnapshot::Expired
    })
    .await;
    assert!(server.count("POST", TB_PATH) >= 1, "kept polling");
}

/// A raw service: `drops` connections are closed unanswered, every later one
/// gets `body` with status 200. Returns its origin and how many it accepted.
async fn raw_service(
    drops: usize,
    body: &'static str,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let seen = accepted.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let n = seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                if n < drops {
                    return;
                }
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (base, accepted)
}

/// A service that drops the first connection without answering (seen on one
/// of Real-Debrid's hosts): the sign-in starts exactly twice and shows its code.
#[tokio::test]
async fn a_dropped_start_is_tried_once_more() {
    use std::sync::atomic::Ordering;
    let (base, accepted) = raw_service(1, TB_START).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    until("the code", || {
        matches!(login.snapshot(), DebridLoginSnapshot::Code { .. })
    })
    .await;
    // The first poll waits the code's interval (1 s): only the two starts so far.
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
}

/// An answer the app cannot read is not the network: one start, then why.
#[tokio::test]
async fn an_unreadable_start_is_not_tried_again() {
    use std::sync::atomic::Ordering;
    let (base, accepted) = raw_service(0, "not json").await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    until("failed", || {
        matches!(login.snapshot(), DebridLoginSnapshot::Failed { .. })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

/// Closing the sign-in while it waits to try again: no second start.
#[tokio::test]
async fn closing_during_the_wait_starts_nothing_more() {
    use std::sync::atomic::Ordering;
    let (base, accepted) = raw_service(1, TB_START).await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&base);
    let login = DebridLoginView::open(&core, StoreId::TorBox);
    until("the first start", || accepted.load(Ordering::SeqCst) == 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(login);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

/// The default store follows the services' order, not which connected first,
/// so a restore (which hands records in that order) keeps it.
#[tokio::test]
async fn the_default_store_follows_the_services_order() {
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_key(StoreId::TorBox, "TBKEY");
    core.set_debrid_key(StoreId::RealDebrid, "RDKEY");
    assert_eq!(services(&core), ["realdebrid", "torbox"]);
}

/// A local addon lists an infoHash; Real-Debrid resolves it to a link on a
/// loopback origin. The link is the provider's, so the addon's private reach
/// does not carry over to it.
#[tokio::test]
async fn a_resolved_debrid_link_gets_no_private_reach() {
    use mq_runtime::{PlayTarget, PlaybackPhase, PlaybackView};
    let origin = Server::start(vec![route("GET", "/Movie.mkv", 200, "M")]).await;
    let link = format!(
        r#"{{"filename":"Movie.mkv","filesize":1000,"download":"{}/Movie.mkv"}}"#,
        origin.base
    );
    let rd = Server::start(vec![
        route(
            "POST",
            "/rest/1.0/torrents/addMagnet",
            201,
            r#"{"id":"RD1"}"#,
        ),
        route("GET", "/rest/1.0/torrents/info/RD1", 200, RD_READY),
        route("POST", "/rest/1.0/unrestrict/link", 200, &link),
    ])
    .await;
    let core = Core::temporary("owner").unwrap();
    core.set_debrid_origin(&rd.base);
    core.restore_debrid(vec![rd_record(G1, "TOKEN", now_ms())], vec![])
        .unwrap();
    // The local addon and its candidates, as tests/playback.rs builds them.
    let local = "http://127.0.0.1:9/manifest.json";
    let manifest = r#"{"id":"local","version":"1.0.0","name":"Local","types":["movie"],"resources":["stream"],"catalogs":[]}"#;
    let row = mq_library::AddonRow {
        transport_url: local.into(),
        manifest_json: manifest.into(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(core.profile(), &[row], &core.hlc())
        .unwrap();
    let streams =
        br#"{"streams":[{"name":"RD","infoHash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c"}]}"#;
    let candidates: Vec<_> = mq_protocol::decode_streams(streams)
        .unwrap()
        .streams
        .iter()
        .map(|w| mq_streams::ParsedStream {
            addon_url: local.parse().ok(),
            ..mq_streams::normalize(w, "local", "Local", 0, 0)
        })
        .collect();
    let target = PlayTarget {
        kind: "movie".into(),
        item_id: "tt1727587".into(),
        video_id: "tt1727587".into(),
        ..PlayTarget::default()
    };
    let view = PlaybackView::open(&core, candidates, target);
    until("playback settles", || {
        view.snapshot().phase != PlaybackPhase::Resolving
    })
    .await;
    let s = view.snapshot();
    assert_eq!(rd.count("POST", "/rest/1.0/unrestrict/link"), 1, "{s:?}");
    assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
    assert_eq!(s.attempts[0].error, "The source could not be reached");
    assert_eq!(origin.count("GET", "/Movie.mkv"), 0, "the origin was asked");
}
