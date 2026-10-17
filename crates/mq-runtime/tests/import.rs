use mq_runtime::{Core, ImportSnapshot, ImportView, StremioEndpoints};
use serde_json::{Value, json};
use std::time::Duration;
use stremio_core::types::watched_bitfield::{WatchedBitField, WatchedField};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const SERIES: &str = "tt0944947";

/// Reads one request; returns its path.
async fn read_request(socket: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            let len: usize = head
                .to_ascii_lowercase()
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map_or(0, |v| v.trim().parse().unwrap());
            if buf.len() >= end + 4 + len {
                return head.split_whitespace().nth(1).unwrap().to_owned();
            }
        }
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn episodes() -> Vec<Value> {
    (1..=3)
        .map(|e| json!({"id": format!("{SERIES}:1:{e}"), "title": format!("E{e}"), "season": 1, "episode": e,
                        "released": format!("2011-04-0{e}T00:00:00.000Z")}))
        .collect()
}

/// Stremio's account API, link service and a meta addon on one port.
async fn stremio() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let origin = base.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let base = origin.clone();
            tokio::spawn(async move {
                let path = read_request(&mut socket).await;
                let ids: Vec<String> = episodes()
                    .iter()
                    .map(|v| v["id"].as_str().unwrap().to_owned())
                    .collect();
                let mut bits = WatchedBitField::construct_from_array(vec![], ids);
                bits.set(0, true);
                bits.set(1, true);
                let result = match path.split('?').next().unwrap() {
                    "/api/v2/create" => {
                        json!({"result": {"code": "QX7K2", "link": format!("{base}/QX7K2")}})
                    }
                    "/api/v2/read" => json!({"result": {"authKey": "secret-session"}}),
                    "/api/getUser" => json!({"result": {"_id": "u1", "email": "a@b.c"}}),
                    "/api/addonCollectionGet" => json!({"result": {"addons": [{
                        "transportUrl": format!("{base}/manifest.json"),
                        "manifest": {"id": "meta", "version": "1.0.0", "name": "Meta", "types": ["series"],
                                     "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]}
                    }]}}),
                    "/api/datastoreGet" => json!({"result": [{
                        "_id": SERIES, "name": "Game of Thrones", "type": "series", "removed": false, "temp": false,
                        "_mtime": "2026-03-01T00:00:00.000Z", "_ctime": "2026-01-01T00:00:00.000Z",
                        "state": {"lastWatched": "2026-03-01T00:00:00.000Z", "timeWatched": 0, "timeOffset": 0,
                                  "overallTimeWatched": 0, "timesWatched": 0, "flaggedWatched": 0, "duration": 0,
                                  "video_id": null, "watched": WatchedField::from(bits).to_string(), "noNotif": false}
                    }]}),
                    p if p.starts_with("/meta/series/") => json!({"meta": {
                        "id": SERIES, "type": "series", "name": "Game of Thrones", "videos": episodes()
                    }}),
                    other => panic!("unexpected request {other}"),
                };
                let body = result.to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

#[tokio::test]
async fn links_then_imports_addons_library_and_watched_episodes() {
    let base = stremio().await;
    let core = Core::temporary("p").unwrap();
    let view = ImportView::open(
        &core,
        StremioEndpoints {
            api: base.clone(),
            link: base.clone(),
        },
    );
    let mut saw_code = false;
    let mut end = None;
    for _ in 0..1000 {
        match view.snapshot() {
            ImportSnapshot::WaitingForLink { code, .. } => saw_code |= code == "QX7K2",
            s @ (ImportSnapshot::Done { .. } | ImportSnapshot::Failed { .. }) => {
                end = Some(s);
                break;
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(saw_code, "the code is shown while waiting");
    let Some(ImportSnapshot::Done { summary }) = end else {
        panic!("import did not finish: {end:?}");
    };
    assert_eq!(summary.addons_imported, 1);
    assert_eq!(summary.titles, 1);
    // Two episodes decoded through the episode list from the meta addon.
    assert_eq!(summary.watched, 2, "{summary:?}");
    assert_eq!(summary.series_unmatched, 0);
    assert_eq!(core.installed_addons().unwrap()[0].manifest.name, "Meta");
    let watched = core
        .library()
        .watched_videos(core.profile(), SERIES)
        .unwrap();
    assert_eq!(watched.len(), 2, "{watched:?}");
}
