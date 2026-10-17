mod common;

use common::{ErrorCheck, HASH, Server, client, route};
use mq_debrid::{
    CacheStatus, DebridError, DeviceLogin, LoginPoll, RealDebrid, Store, TorrentStatus,
};
use std::time::Duration;

const TOKEN: &str = "rd-secret-token-0123456789";

fn store(server: &Server) -> RealDebrid {
    RealDebrid::new(client(), TOKEN).with_base_url(&server.base)
}

const TORRENT_WAITING: &str = r#"{"id":"RD1","filename":"Show.S01","hash":"DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C","progress":0,"status":"waiting_files_selection","files":[{"id":1,"path":"/Show.S01E01.mkv","bytes":1000,"selected":0},{"id":2,"path":"/Show.S01E02.mkv","bytes":2000,"selected":0}],"links":[]}"#;
const TORRENT_READY: &str = r#"{"id":"RD1","filename":"Show.S01","hash":"dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c","progress":100,"status":"downloaded","files":[{"id":1,"path":"/Show.S01E01.mkv","bytes":1000,"selected":0},{"id":2,"path":"/Show.S01E02.mkv","bytes":2000,"selected":1}],"links":["https://real-debrid.com/d/AAA"]}"#;

#[tokio::test]
async fn user_reports_premium_and_sends_bearer_header_only() {
    let server = Server::start(vec![route(
        "GET",
        "/user",
        200,
        r#"{"id":1,"username":"alice","type":"premium","premium":1000,"expiration":"2032-06-06T04:42:42.000Z"}"#,
    )])
    .await;
    let user = store(&server).user().await.unwrap();
    assert_eq!(user.name, "alice");
    assert!(user.is_premium);
    assert_eq!(user.premium_until, Some(1_970_109_762));
    let req = &server.requests()[0];
    assert!(req.head.contains(&format!("authorization: bearer {TOKEN}")));
    assert!(!req.target.contains(TOKEN), "token must not be in the URL");
}

#[tokio::test]
async fn check_is_unknown_and_makes_no_request() {
    let server = Server::start(vec![]).await;
    let out = store(&server)
        .check(&[HASH.to_owned(), HASH.to_owned()])
        .await
        .unwrap();
    assert_eq!(out, vec![CacheStatus::Unknown; 2]);
    assert!(server.requests().is_empty());
    assert!(!store(&server).capabilities().instant_check);
}

#[tokio::test]
async fn add_magnet_posts_a_form_and_returns_the_id() {
    let server = Server::start(vec![route(
        "POST",
        "/torrents/addMagnet",
        201,
        r#"{"id":"RD1","uri":"https://api.real-debrid.com/rest/1.0/torrents/info/RD1"}"#,
    )])
    .await;
    let handle = store(&server)
        .add_magnet(&HASH.to_uppercase())
        .await
        .unwrap();
    assert_eq!(handle.id, "RD1");
    assert_eq!(handle.hash, HASH);
    let body = &server.requests()[0].body;
    assert_eq!(
        body,
        &format!("magnet=magnet%3A%3Fxt%3Durn%3Abtih%3A{HASH}")
    );
}

#[tokio::test]
async fn add_magnet_rejects_a_bad_hash_without_a_request() {
    let server = Server::start(vec![]).await;
    let err = store(&server).add_magnet("nope").await.unwrap_err();
    assert!(matches!(err, DebridError::Provider { .. }), "{err:?}");
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn torrent_maps_status_files_and_aligns_links_to_selected_files() {
    let server = Server::start(vec![
        route("GET", "/torrents/info/RD1", 200, TORRENT_WAITING).then(200, TORRENT_READY),
    ])
    .await;
    let store = store(&server);
    let waiting = store.torrent("RD1").await.unwrap();
    assert_eq!(waiting.status, TorrentStatus::WaitingFileSelection);
    assert_eq!(waiting.files.len(), 2);
    assert_eq!(waiting.files[0].path, "Show.S01E01.mkv");
    assert!(waiting.files.iter().all(|f| f.link.is_none()));

    let ready = store.torrent("RD1").await.unwrap();
    assert_eq!(ready.status, TorrentStatus::Ready);
    assert_eq!(ready.hash, HASH);
    assert_eq!(ready.progress, 1.0);
    assert_eq!(ready.files[0].link, None);
    assert_eq!(
        ready.files[1].link.as_deref(),
        Some("https://real-debrid.com/d/AAA")
    );
}

#[tokio::test]
async fn select_files_and_link_and_remove() {
    let server = Server::start(vec![
        route("POST", "/torrents/selectFiles/RD1", 204, ""),
        route("GET", "/torrents/info/RD1", 200, TORRENT_READY),
        route(
            "POST",
            "/unrestrict/link",
            200,
            r#"{"id":"X","filename":"Show.S01E02.mkv","filesize":2000,"download":"https://cdn.example/d/Show.S01E02.mkv"}"#,
        ),
        route("DELETE", "/torrents/delete/RD1", 204, ""),
    ])
    .await;
    let store = store(&server);
    store.select_files("RD1", &[2, 5]).await.unwrap();
    let info = store.torrent("RD1").await.unwrap();
    let link = store.link(&info, 2).await.unwrap();
    assert_eq!(link.url, "https://cdn.example/d/Show.S01E02.mkv");
    assert_eq!(link.size, Some(2000));
    assert_eq!(link.filename, "Show.S01E02.mkv");
    store.remove("RD1").await.unwrap();

    let requests = server.requests();
    assert_eq!(requests[0].body, "files=2%2C5");
    let unrestrict = requests
        .iter()
        .find(|r| r.target == "/unrestrict/link")
        .unwrap();
    assert_eq!(
        unrestrict.body,
        "link=https%3A%2F%2Freal-debrid.com%2Fd%2FAAA"
    );
    assert_eq!(server.count("DELETE", "/torrents/delete/RD1"), 1);
}

#[tokio::test]
async fn link_for_a_file_without_a_link_is_an_error() {
    let server = Server::start(vec![route("GET", "/torrents/info/RD1", 200, TORRENT_READY)]).await;
    let store = store(&server);
    let info = store.torrent("RD1").await.unwrap();
    let err = store.link(&info, 1).await.unwrap_err();
    assert!(
        matches!(err, DebridError::Provider { ref code, .. } if code == "no_link"),
        "{err:?}"
    );
}

#[tokio::test]
async fn maps_provider_errors() {
    let cases: [(u16, &str, ErrorCheck); 5] = [
        (401, r#"{"error":"bad_token","error_code":8}"#, |e| {
            matches!(e, DebridError::Auth)
        }),
        (
            403,
            r#"{"error":"permission_denied","error_code":9}"#,
            |e| matches!(e, DebridError::NotPremium),
        ),
        (
            503,
            r#"{"error":"too_many_active_downloads","error_code":21}"#,
            |e| matches!(e, DebridError::Quota),
        ),
        (
            429,
            r#"{"error":"too_many_requests","error_code":34}"#,
            |e| matches!(e, DebridError::RateLimited { retry_after: Some(d) } if *d == Duration::from_secs(7)),
        ),
        (
            503,
            r#"{"error":"torrent_too_big","error_code":29}"#,
            |e| matches!(e, DebridError::Provider { code, message } if code == "29" && message == "torrent_too_big"),
        ),
    ];
    for (status, body, check) in cases {
        let server = Server::start(vec![
            route("GET", "/user", status, body).header("Retry-After", "7"),
        ])
        .await;
        let err = store(&server).user().await.unwrap_err();
        assert!(check(&err), "{status} {body} -> {err:?}");
    }
}

const DEVICE_CODE: &str = r#"{"device_code":"DEV123","user_code":"ABCD1234","interval":5,"expires_in":1800,"verification_url":"https://real-debrid.com/device"}"#;

#[tokio::test]
async fn device_login_goes_pending_then_approved() {
    let server = Server::start(vec![
        route("GET", "/device/code", 200, DEVICE_CODE).query("new_credentials=yes"),
        route(
            "GET",
            "/device/credentials",
            403,
            r#"{"error":"authorization_pending","error_code":0}"#,
        )
        .then(200, r#"{"client_id":"CID","client_secret":"CSECRET"}"#),
        route(
            "POST",
            "/token",
            200,
            r#"{"access_token":"ACC","expires_in":3600,"token_type":"Bearer","refresh_token":"REF"}"#,
        ),
    ])
    .await;
    let http = client();
    let login = DeviceLogin::start_at(&http, &server.base).await.unwrap();
    assert_eq!(login.user_code, "ABCD1234");
    assert_eq!(login.verification_url, "https://real-debrid.com/device");
    assert_eq!(login.interval, Duration::from_secs(5));
    assert_eq!(login.expires_in, Duration::from_secs(1800));

    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Pending));
    let LoginPoll::Approved(creds) = login.poll().await.unwrap() else {
        panic!("expected approval");
    };
    assert_eq!(creds.access_token, "ACC");
    assert_eq!(creds.refresh_token, "REF");
    assert_eq!(creds.client_id, "CID");
    assert_eq!(creds.expires_in, 3600);
    // Secrets stay out of Debug output.
    let debug = format!("{creds:?}");
    assert!(!debug.contains("ACC") && !debug.contains("REF") && !debug.contains("CSECRET"));

    let token_req = server
        .requests()
        .into_iter()
        .find(|r| r.target == "/token")
        .unwrap();
    assert!(
        token_req.body.contains("client_id=CID"),
        "{}",
        token_req.body
    );
    assert!(token_req.body.contains("code=DEV123"), "{}", token_req.body);
    assert!(
        token_req
            .body
            .contains("grant_type=http%3A%2F%2Foauth.net%2Fgrant_type%2Fdevice%2F1.0"),
        "{}",
        token_req.body
    );
}

#[tokio::test]
async fn device_login_reports_expiry() {
    let server = Server::start(vec![route(
        "GET",
        "/device/code",
        200,
        r#"{"device_code":"D","user_code":"U","interval":5,"expires_in":0,"verification_url":"https://real-debrid.com/device"}"#,
    )])
    .await;
    let login = DeviceLogin::start_at(&client(), &server.base)
        .await
        .unwrap();
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Expired));
    assert_eq!(server.count("GET", "/device/credentials"), 0);
}

#[tokio::test]
async fn device_login_surfaces_server_expiry() {
    let server = Server::start(vec![
        route("GET", "/device/code", 200, DEVICE_CODE),
        route(
            "GET",
            "/device/credentials",
            400,
            r#"{"error":"expired_token"}"#,
        ),
    ])
    .await;
    let login = DeviceLogin::start_at(&client(), &server.base)
        .await
        .unwrap();
    assert!(matches!(login.poll().await.unwrap(), LoginPoll::Expired));
}

#[tokio::test]
async fn refresh_swaps_the_access_token() {
    let server = Server::start(vec![route(
        "POST",
        "/token",
        200,
        r#"{"access_token":"NEW","expires_in":3600,"token_type":"Bearer","refresh_token":"REF"}"#,
    )])
    .await;
    let creds = mq_debrid::Credentials {
        client_id: "CID".into(),
        client_secret: "CSECRET".into(),
        access_token: "OLD".into(),
        refresh_token: "REF".into(),
        expires_in: 0,
    };
    let fresh = creds.refresh_at(&client(), &server.base).await.unwrap();
    assert_eq!(fresh.access_token, "NEW");
    assert_eq!(fresh.client_id, "CID");
    let body = &server.requests()[0].body;
    assert!(
        body.contains("code=REF") && body.contains("client_secret=CSECRET"),
        "{body}"
    );
}
