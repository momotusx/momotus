mod common;

use common::{ErrorCheck, HASH, Server, client, route};
use mq_debrid::{CacheStatus, DebridError, Premiumize, Store, TorrentStatus};

const KEY: &str = "pm-secret-key-0123456789";
const OTHER: &str = "1111111111111111111111111111111111111111";

fn store(server: &Server) -> Premiumize {
    Premiumize::new(client(), KEY).with_base_url(&server.base)
}

const DIRECTDL: &str = r#"{"status":"success","location":"https://x.example/a","filename":"Show","filesize":3000,"content":[
  {"path":"Show/Show.S01E01.mkv","size":1000,"link":"https://cdn.example/dl/1.mkv","stream_link":"https://cdn.example/s/1"},
  {"path":"Show/Show.S01E02.mkv","size":2000,"link":"https://cdn.example/dl/2.mkv","stream_link":"https://cdn.example/s/2"},
  {"path":"Show/notes.txt","size":5}
]}"#;

#[tokio::test]
async fn user_reports_premium_by_expiry() {
    let server = Server::start(vec![route(
        "GET",
        "/account/info",
        200,
        r#"{"status":"success","customer_id":123,"premium_until":4102444800,"limit_used":0.1,"space_used":5}"#,
    )])
    .await;
    let user = store(&server).user().await.unwrap();
    assert_eq!(user.name, "123");
    assert!(user.is_premium);
    assert_eq!(user.premium_until, Some(4_102_444_800));
    let target = &server.requests()[0].target;
    assert!(target.contains(&format!("apikey={KEY}")), "{target}");

    let expired = Server::start(vec![route(
        "GET",
        "/account/info",
        200,
        r#"{"status":"success","customer_id":"c","premium_until":1000}"#,
    )])
    .await;
    assert!(!store(&expired).user().await.unwrap().is_premium);
}

#[tokio::test]
async fn check_reads_the_boolean_list_in_order() {
    let server = Server::start(vec![route(
        "GET",
        "/cache/check",
        200,
        r#"{"status":"success","response":[true,false],"transcoded":[false,false],"filename":["a",""],"filesize":["1",""]}"#,
    )])
    .await;
    let out = store(&server)
        .check(&[HASH.to_owned(), OTHER.to_owned()])
        .await
        .unwrap();
    assert_eq!(out, vec![CacheStatus::Cached, CacheStatus::NotCached]);
    let target = &server.requests()[0].target;
    assert!(
        target.contains(&format!("items%5B%5D=magnet%3A%3Fxt%3Durn%3Abtih%3A{HASH}")),
        "{target}"
    );
}

#[tokio::test]
async fn check_chunks_large_batches() {
    let server = Server::start(vec![route(
        "GET",
        "/cache/check",
        200,
        &format!(
            r#"{{"status":"success","response":[{}]}}"#,
            vec!["false"; 50].join(",")
        ),
    )])
    .await;
    let hashes = vec![HASH.to_owned(); 120];
    let out = store(&server).check(&hashes).await.unwrap();
    assert_eq!(out.len(), 120);
    assert_eq!(server.count("GET", "/cache/check"), 3);
}

#[tokio::test]
async fn cached_magnet_resolves_through_directdl_without_a_transfer() {
    let server = Server::start(vec![route("POST", "/transfer/directdl", 200, DIRECTDL)]).await;
    let store = store(&server);
    let handle = store.add_magnet(HASH).await.unwrap();
    assert_eq!(handle.id, format!("dd:{HASH}"));

    let info = store.torrent(&handle.id).await.unwrap();
    assert_eq!(info.status, TorrentStatus::Ready);
    // The file without a link is dropped.
    assert_eq!(info.files.len(), 2);
    assert_eq!(info.files[1].path, "Show/Show.S01E02.mkv");

    let link = store.link(&info, 1).await.unwrap();
    assert_eq!(link.url, "https://cdn.example/dl/2.mkv");
    assert_eq!(link.size, Some(2000));
    assert_eq!(link.filename, "Show.S01E02.mkv");

    assert_eq!(server.count("POST", "/transfer/create"), 0);
    let body = &server.requests()[0].body;
    assert!(body.contains(&format!("apikey={KEY}")), "{body}");
    assert!(
        !server.requests()[0].target.contains(KEY),
        "key stays out of the URL"
    );
    store.remove(&handle.id).await.unwrap();
    assert_eq!(server.count("POST", "/transfer/delete"), 0);
}

#[tokio::test]
async fn uncached_magnet_becomes_a_transfer_with_folder_listing() {
    let server = Server::start(vec![
        route(
            "POST",
            "/transfer/directdl",
            200,
            r#"{"status":"error","message":"Not cached"}"#,
        ),
        route(
            "POST",
            "/transfer/create",
            200,
            r#"{"status":"success","id":"T1","name":"Show","type":"torrent"}"#,
        ),
        route(
            "GET",
            "/transfer/list",
            200,
            r#"{"status":"success","transfers":[{"id":"T1","name":"Show","status":"running","progress":0.25}]}"#,
        )
        .then(
            200,
            r#"{"status":"success","transfers":[{"id":"T0","name":"o","status":"finished"},{"id":"T1","name":"Show","status":"finished","progress":1,"folder_id":"F1"}]}"#,
        ),
        route(
            "GET",
            "/folder/list",
            200,
            r#"{"status":"success","content":[
              {"id":"a","name":"Show.S01E02.mkv","type":"file","size":2000,"link":"https://cdn.example/dl/2.mkv"},
              {"id":"d","name":"Sub","type":"folder"}]}"#,
        )
        .query("id=F1"),
        route(
            "GET",
            "/folder/list",
            200,
            r#"{"status":"success","content":[
              {"id":"b","name":"Show.S01E01.mkv","type":"file","size":1000,"link":"https://cdn.example/dl/1.mkv"}]}"#,
        )
        .query("id=d"),
        route("POST", "/transfer/delete", 200, r#"{"status":"success"}"#),
    ])
    .await;
    let store = store(&server);
    let handle = store.add_magnet(HASH).await.unwrap();
    assert_eq!(handle.id, "T1");

    let running = store.torrent("T1").await.unwrap();
    assert_eq!(running.status, TorrentStatus::Downloading);
    assert_eq!(running.progress, 0.25);
    assert!(running.files.is_empty());

    let done = store.torrent("T1").await.unwrap();
    assert_eq!(done.status, TorrentStatus::Ready);
    let paths: Vec<_> = done
        .files
        .iter()
        .map(|f| (f.index, f.path.as_str()))
        .collect();
    assert_eq!(
        paths,
        vec![(0, "Show.S01E02.mkv"), (1, "Sub/Show.S01E01.mkv")]
    );
    let link = store.link(&done, 1).await.unwrap();
    assert_eq!(link.url, "https://cdn.example/dl/1.mkv");

    store.remove("T1").await.unwrap();
    assert_eq!(server.count("POST", "/transfer/delete"), 1);
}

#[tokio::test]
async fn maps_provider_errors() {
    let cases: [(u16, &str, ErrorCheck); 5] = [
        (
            200,
            r#"{"status":"error","message":"Not logged in."}"#,
            |e| matches!(e, DebridError::Auth),
        ),
        (401, "{}", |e| matches!(e, DebridError::Auth)),
        (
            200,
            r#"{"status":"error","message":"You need a premium account."}"#,
            |e| matches!(e, DebridError::NotPremium),
        ),
        (429, "{}", |e| matches!(e, DebridError::RateLimited { .. })),
        (
            200,
            r#"{"status":"error","message":"Something odd"}"#,
            |e| matches!(e, DebridError::Provider { message, .. } if message == "Something odd"),
        ),
    ];
    for (status, body, check) in cases {
        let server = Server::start(vec![route("GET", "/account/info", status, body)]).await;
        let err = store(&server).user().await.unwrap_err();
        assert!(check(&err), "{status} {body} -> {err:?}");
    }
}
