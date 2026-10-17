mod common;

use common::{ErrorCheck, HASH, Server, client, route};
use mq_debrid::{CacheStatus, DebridError, Store, TorBox, TorrentStatus};
use std::time::Duration;

const KEY: &str = "tb-secret-key-0123456789abcdef";
const OTHER: &str = "1111111111111111111111111111111111111111";

fn store(server: &Server) -> TorBox {
    TorBox::new(client(), KEY).with_base_url(&server.base)
}

fn ok(data: &str) -> String {
    format!(r#"{{"success":true,"error":null,"detail":"ok","data":{data}}}"#)
}

const TORRENT_READY: &str = r#"{"id":42,"hash":"DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C","name":"Show.S01","progress":1,"download_state":"cached","download_finished":true,"download_present":true,"files":[{"id":0,"name":"Show.S01/Show.S01E01.mkv","size":1000,"short_name":"Show.S01E01.mkv"},{"id":1,"name":"Show.S01/Show.S01E02.mkv","size":2000,"short_name":"Show.S01E02.mkv"}]}"#;

#[tokio::test]
async fn user_maps_plan_and_expiry() {
    let server = Server::start(vec![route(
        "GET",
        "/v1/api/user/me",
        200,
        &ok(r#"{"id":1,"plan":2,"email":"a@example.com","premium_expires_at":"2051-02-18T04:08:43Z"}"#),
    )])
    .await;
    let user = store(&server).user().await.unwrap();
    assert!(user.is_premium);
    assert_eq!(user.premium_until, Some(2_560_306_123));
    let req = &server.requests()[0];
    assert!(req.head.contains(&format!("authorization: bearer {KEY}")));

    let free = Server::start(vec![route(
        "GET",
        "/v1/api/user/me",
        200,
        &ok(r#"{"id":1,"plan":0,"email":"a@example.com","premium_expires_at":null}"#),
    )])
    .await;
    let user = store(&free).user().await.unwrap();
    assert!(!user.is_premium);
    assert_eq!(user.premium_until, None);
}

#[tokio::test]
async fn check_marks_listed_hashes_cached_and_others_not() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/checkcached",
        200,
        &ok(&format!(
            r#"[{{"name":"x","size":1,"hash":"{}"}}]"#,
            HASH.to_uppercase()
        )),
    )])
    .await;
    let out = store(&server)
        .check(&[HASH.to_owned(), OTHER.to_owned()])
        .await
        .unwrap();
    assert_eq!(out, vec![CacheStatus::Cached, CacheStatus::NotCached]);
    let req = &server.requests()[0];
    assert!(req.target.contains("format=list"), "{}", req.target);
    assert_eq!(req.body, format!(r#"{{"hashes":["{HASH}","{OTHER}"]}}"#));
}

#[tokio::test]
async fn check_with_nothing_cached_accepts_null_data() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/checkcached",
        200,
        &ok("null"),
    )])
    .await;
    let out = store(&server).check(&[HASH.to_owned()]).await.unwrap();
    assert_eq!(out, vec![CacheStatus::NotCached]);
}

#[tokio::test]
async fn check_chunks_large_batches() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/checkcached",
        200,
        &ok("null"),
    )])
    .await;
    let hashes = vec![HASH.to_owned(); 250];
    let out = store(&server).check(&hashes).await.unwrap();
    assert_eq!(out.len(), 250);
    assert_eq!(server.count("POST", "/v1/api/torrents/checkcached"), 3);
}

#[tokio::test]
async fn add_magnet_sends_multipart_and_returns_the_torrent_id() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/createtorrent",
        200,
        &ok(&format!(
            r#"{{"hash":"{HASH}","torrent_id":42,"auth_id":"a"}}"#
        )),
    )])
    .await;
    let handle = store(&server).add_magnet(HASH).await.unwrap();
    assert_eq!(handle.id, "42");
    let req = &server.requests()[0];
    assert!(
        req.head.contains("multipart/form-data; boundary="),
        "{}",
        req.head
    );
    assert!(
        req.body.contains(&format!(
            "name=\"magnet\"\r\n\r\nmagnet:?xt=urn:btih:{HASH}\r\n"
        )),
        "{}",
        req.body
    );
}

#[tokio::test]
async fn duplicate_item_falls_back_to_the_existing_torrent() {
    let server = Server::start(vec![
        route(
            "POST",
            "/v1/api/torrents/createtorrent",
            400,
            r#"{"success":false,"error":"DUPLICATE_ITEM","detail":"This item already exists.","data":null}"#,
        ),
        route(
            "GET",
            "/v1/api/torrents/mylist",
            200,
            &ok(&format!("[{TORRENT_READY}]")),
        ),
    ])
    .await;
    let handle = store(&server).add_magnet(HASH).await.unwrap();
    assert_eq!(handle.id, "42");
}

#[tokio::test]
async fn torrent_and_link_use_ids_and_a_token_query() {
    let server = Server::start(vec![
        route("GET", "/v1/api/torrents/mylist", 200, &ok(TORRENT_READY)).query("id=42"),
        route(
            "GET",
            "/v1/api/torrents/requestdl",
            200,
            &ok(r#""https://store-1.tb-cdn.example/dl/abc""#),
        ),
    ])
    .await;
    let store = store(&server);
    let info = store.torrent("42").await.unwrap();
    assert_eq!(info.status, TorrentStatus::Ready);
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.files[1].index, 1);
    assert_eq!(info.files[1].path, "Show.S01/Show.S01E02.mkv");

    let link = store.link(&info, 1).await.unwrap();
    assert_eq!(link.url, "https://store-1.tb-cdn.example/dl/abc");
    assert_eq!(link.size, Some(2000));
    assert_eq!(link.filename, "Show.S01E02.mkv");
    assert!(link.expires_at.is_some());

    let dl = server
        .requests()
        .into_iter()
        .find(|r| r.target.starts_with("/v1/api/torrents/requestdl"))
        .unwrap();
    assert!(dl.target.contains(&format!("token={KEY}")), "{}", dl.target);
    assert!(dl.target.contains("torrent_id=42") && dl.target.contains("file_id=1"));
}

#[tokio::test]
async fn torrent_statuses_map() {
    for (body, expected) in [
        (
            r#"{"id":1,"download_state":"metaDL","download_finished":false,"files":[]}"#,
            TorrentStatus::Resolving,
        ),
        (
            r#"{"id":1,"download_state":"downloading","progress":0.4,"download_finished":false,"files":[]}"#,
            TorrentStatus::Downloading,
        ),
        (
            r#"{"id":1,"download_state":"stalled (no seeds)","download_finished":false,"files":[]}"#,
            TorrentStatus::Downloading,
        ),
        (
            r#"{"id":1,"download_state":"error","download_finished":false,"files":[]}"#,
            TorrentStatus::Failed("error".into()),
        ),
    ] {
        let server = Server::start(vec![route(
            "GET",
            "/v1/api/torrents/mylist",
            200,
            &ok(body),
        )])
        .await;
        let info = store(&server).torrent("1").await.unwrap();
        assert_eq!(info.status, expected, "{body}");
    }
}

#[tokio::test]
async fn select_files_is_a_noop_and_remove_sends_delete() {
    let server = Server::start(vec![route(
        "POST",
        "/v1/api/torrents/controltorrent",
        200,
        &ok("null"),
    )])
    .await;
    let store = store(&server);
    store.select_files("42", &[1]).await.unwrap();
    assert!(server.requests().is_empty());
    store.remove("42").await.unwrap();
    assert_eq!(
        server.requests()[0].body,
        r#"{"operation":"delete","torrent_id":42}"#
    );
}

#[tokio::test]
async fn maps_provider_errors() {
    let cases: [(u16, &str, ErrorCheck); 6] = [
        (
            403,
            r#"{"success":false,"error":"BAD_TOKEN","detail":"bad","data":null}"#,
            |e| matches!(e, DebridError::Auth),
        ),
        (
            403,
            r#"{"success":false,"error":"PLAN_RESTRICTED_FEATURE","detail":"upgrade","data":null}"#,
            |e| matches!(e, DebridError::NotPremium),
        ),
        (
            400,
            r#"{"success":false,"error":"ACTIVE_LIMIT","detail":"full","data":null}"#,
            |e| matches!(e, DebridError::Quota),
        ),
        (
            429,
            "{}",
            |e| matches!(e, DebridError::RateLimited { retry_after: Some(d) } if *d == Duration::from_secs(3)),
        ),
        (
            200,
            r#"{"success":false,"error":"COOLDOWN_LIMIT","detail":"wait","data":null}"#,
            |e| matches!(e, DebridError::RateLimited { retry_after: None }),
        ),
        (
            400,
            r#"{"success":false,"error":"BOZO_TORRENT","detail":"bad torrent","data":null}"#,
            |e| matches!(e, DebridError::Provider { code, message } if code == "BOZO_TORRENT" && message == "bad torrent"),
        ),
    ];
    for (status, body, check) in cases {
        let server = Server::start(vec![
            route("GET", "/v1/api/user/me", status, body).header("Retry-After", "3"),
        ])
        .await;
        let err = store(&server).user().await.unwrap_err();
        assert!(check(&err), "{status} {body} -> {err:?}");
    }
}
