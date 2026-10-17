mod common;

use common::{ErrorCheck, HASH, Server, client, route};
use mq_debrid::{AllDebrid, CacheStatus, DebridError, Store, TorrentStatus};

const KEY: &str = "ad-secret-key-0123456789";

fn store(server: &Server) -> AllDebrid {
    AllDebrid::new(client(), KEY).with_base_url(&server.base)
}

const FILES: &str = r#"{"status":"success","data":{"magnets":[{"id":"7","files":[
  {"n":"Show.S01","e":[
    {"n":"Show.S01E01.mkv","s":1000,"l":"https://alldebrid.com/f/aaa"},
    {"n":"Extras","e":[{"n":"making.of.mkv","s":300,"l":"https://alldebrid.com/f/bbb"}]}
  ]},
  {"n":"readme.txt","s":10,"l":"https://alldebrid.com/f/ccc"}
]}]}}"#;

#[tokio::test]
async fn user_maps_premium() {
    let server = Server::start(vec![route(
        "GET",
        "/v4/user",
        200,
        r#"{"status":"success","data":{"user":{"username":"bob","isPremium":true,"premiumUntil":1900000000}}}"#,
    )])
    .await;
    let user = store(&server).user().await.unwrap();
    assert_eq!(user.name, "bob");
    assert!(user.is_premium);
    assert_eq!(user.premium_until, Some(1_900_000_000));
    assert!(
        server.requests()[0]
            .head
            .contains(&format!("authorization: bearer {KEY}"))
    );
    assert!(!server.requests()[0].target.contains(KEY));
}

#[tokio::test]
async fn check_is_unknown_and_makes_no_request() {
    let server = Server::start(vec![]).await;
    let out = store(&server).check(&[HASH.to_owned()]).await.unwrap();
    assert_eq!(out, vec![CacheStatus::Unknown]);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn add_magnet_uploads_with_the_array_field() {
    let server = Server::start(vec![route(
        "POST",
        "/v4/magnet/upload",
        200,
        &format!(
            r#"{{"status":"success","data":{{"magnets":[{{"magnet":"m","hash":"{HASH}","name":"n","size":1,"ready":true,"id":7}}]}}}}"#
        ),
    )])
    .await;
    let handle = store(&server).add_magnet(HASH).await.unwrap();
    assert_eq!(handle.id, "7");
    assert_eq!(
        server.requests()[0].body,
        format!("magnets%5B%5D=magnet%3A%3Fxt%3Durn%3Abtih%3A{HASH}")
    );
}

#[tokio::test]
async fn add_magnet_surfaces_per_magnet_errors() {
    let server = Server::start(vec![route(
        "POST",
        "/v4/magnet/upload",
        200,
        r#"{"status":"success","data":{"magnets":[{"magnet":"m","error":{"code":"MAGNET_MUST_BE_PREMIUM","message":"premium"}}]}}"#,
    )])
    .await;
    let err = store(&server).add_magnet(HASH).await.unwrap_err();
    assert!(matches!(err, DebridError::NotPremium), "{err:?}");
}

#[tokio::test]
async fn ready_magnet_flattens_the_file_tree() {
    let server = Server::start(vec![
        route(
            "POST",
            "/v4.1/magnet/status",
            200,
            r#"{"status":"success","data":{"magnets":[{"id":7,"filename":"Show.S01","size":1310,"status":"Ready","statusCode":4,"downloaded":1310}]}}"#,
        ),
        route("POST", "/v4/magnet/files", 200, FILES),
    ])
    .await;
    let info = store(&server).torrent("7").await.unwrap();
    assert_eq!(info.status, TorrentStatus::Ready);
    assert_eq!(info.progress, 1.0);
    let paths: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.index, f.path.as_str(), f.size))
        .collect();
    assert_eq!(
        paths,
        vec![
            (0, "Show.S01/Show.S01E01.mkv", 1000),
            (1, "Show.S01/Extras/making.of.mkv", 300),
            (2, "readme.txt", 10),
        ]
    );
    assert_eq!(
        info.files[0].link.as_deref(),
        Some("https://alldebrid.com/f/aaa")
    );
    assert_eq!(server.requests()[1].body, "id%5B%5D=7");
}

#[tokio::test]
async fn processing_magnet_does_not_fetch_files() {
    for (code, status) in [
        (0, TorrentStatus::Queued),
        (1, TorrentStatus::Downloading),
        (3, TorrentStatus::Downloading),
        (7, TorrentStatus::Failed("Not downloaded in 20 min.".into())),
    ] {
        let body = format!(
            r#"{{"status":"success","data":{{"magnets":[{{"id":7,"filename":"x","size":100,"status":"Not downloaded in 20 min.","statusCode":{code},"downloaded":50}}]}}}}"#
        );
        let server = Server::start(vec![route("POST", "/v4.1/magnet/status", 200, &body)]).await;
        let info = store(&server).torrent("7").await.unwrap();
        assert_eq!(info.status, status);
        assert!(info.files.is_empty());
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn link_unlocks_the_file_link() {
    let server = Server::start(vec![
        route(
            "POST",
            "/v4.1/magnet/status",
            200,
            r#"{"status":"success","data":{"magnets":{"id":7,"filename":"x","size":1,"statusCode":4}}}"#,
        ),
        route("POST", "/v4/magnet/files", 200, FILES),
        route(
            "POST",
            "/v4/link/unlock",
            200,
            r#"{"status":"success","data":{"link":"https://x.debrid.example/dl/abc/Show.S01E01.mkv","filename":"Show.S01E01.mkv","filesize":1000,"id":"abc"}}"#,
        ),
    ])
    .await;
    let store = store(&server);
    let info = store.torrent("7").await.unwrap();
    let link = store.link(&info, 0).await.unwrap();
    assert_eq!(link.url, "https://x.debrid.example/dl/abc/Show.S01E01.mkv");
    assert_eq!(link.size, Some(1000));
    assert_eq!(link.filename, "Show.S01E01.mkv");
    let unlock = server
        .requests()
        .into_iter()
        .find(|r| r.target == "/v4/link/unlock")
        .unwrap();
    assert_eq!(unlock.body, "link=https%3A%2F%2Falldebrid.com%2Ff%2Faaa");
}

#[tokio::test]
async fn delayed_links_are_reported_not_faked() {
    let server = Server::start(vec![
        route(
            "POST",
            "/v4.1/magnet/status",
            200,
            r#"{"status":"success","data":{"magnets":[{"id":7,"statusCode":4}]}}"#,
        ),
        route("POST", "/v4/magnet/files", 200, FILES),
        route(
            "POST",
            "/v4/link/unlock",
            200,
            r#"{"status":"success","data":{"link":"","filename":"x","filesize":1,"delayed":99}}"#,
        ),
    ])
    .await;
    let store = store(&server);
    let info = store.torrent("7").await.unwrap();
    let err = store.link(&info, 0).await.unwrap_err();
    assert!(
        matches!(err, DebridError::Provider { ref code, .. } if code == "link_delayed"),
        "{err:?}"
    );
}

#[tokio::test]
async fn remove_posts_the_id() {
    let server = Server::start(vec![route(
        "POST",
        "/v4/magnet/delete",
        200,
        r#"{"status":"success","data":{"message":"Magnet was successfully deleted"}}"#,
    )])
    .await;
    store(&server).remove("7").await.unwrap();
    assert_eq!(server.requests()[0].body, "id=7");
}

#[tokio::test]
async fn maps_provider_errors() {
    let cases: [(u16, &str, ErrorCheck); 5] = [
        (
            200,
            r#"{"status":"error","error":{"code":"AUTH_BAD_APIKEY","message":"bad"}}"#,
            |e| matches!(e, DebridError::Auth),
        ),
        (
            200,
            r#"{"status":"error","error":{"code":"MUST_BE_PREMIUM","message":"p"}}"#,
            |e| matches!(e, DebridError::NotPremium),
        ),
        (
            200,
            r#"{"status":"error","error":{"code":"MAGNET_TOO_MANY_ACTIVE","message":"30"}}"#,
            |e| matches!(e, DebridError::Quota),
        ),
        (429, "{}", |e| matches!(e, DebridError::RateLimited { .. })),
        (
            200,
            r#"{"status":"error","error":{"code":"NO_SERVER","message":"vpn"}}"#,
            |e| matches!(e, DebridError::Provider { code, message } if code == "NO_SERVER" && message == "vpn"),
        ),
    ];
    for (status, body, check) in cases {
        let server = Server::start(vec![route("GET", "/v4/user", status, body)]).await;
        let err = store(&server).user().await.unwrap_err();
        assert!(check(&err), "{status} {body} -> {err:?}");
    }
}
