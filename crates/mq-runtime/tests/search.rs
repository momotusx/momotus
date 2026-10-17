use mq_library::AddonRow;
use mq_runtime::{Core, RowState, SearchView};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A search addon answering every query with `ids`; counts its requests.
async fn search_addon(id: &str, ids: &'static [&'static str]) -> (AddonRow, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let metas: Vec<_> = ids
                    .iter()
                    .map(|id| serde_json::json!({"id": id, "type": "movie", "name": id}))
                    .collect();
                let body = serde_json::json!({ "metas": metas }).to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": id, "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "extra": [{"name": "search", "isRequired": true}]}]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    (row, hits)
}

#[tokio::test]
async fn debounces_dedupes_and_drops_empty_rows() {
    let core = Core::temporary("p").unwrap();
    let (a, hits) = search_addon("a", &["tt1", "tt2"]).await;
    let (b, _) = search_addon("b", &["tt2", "tt3"]).await;
    let (c, _) = search_addon("c", &[]).await;
    let mut rows = vec![a, b, c];
    for (i, r) in rows.iter_mut().enumerate() {
        r.position = i as i64;
    }
    core.library()
        .set_addons(core.profile(), &rows, &core.hlc())
        .unwrap();

    let view = SearchView::open(&core);
    for typed in ["s", "si", "sin", "sintel"] {
        view.set_query(typed);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let mut settled = None;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "sintel"
            && s.rows.iter().all(|r| r.state == RowState::Ready)
            && !s.rows.is_empty()
        {
            settled = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = settled.expect("search settled");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "typing is debounced");
    let ids: Vec<Vec<&str>> = s
        .rows
        .iter()
        .map(|r| r.tiles.iter().map(|t| t.id.as_str()).collect())
        .collect();
    assert_eq!(ids, vec![vec!["tt1", "tt2"], vec!["tt3"]]);
    assert_eq!(s.rows[0].title, "Movies");

    view.remember().unwrap();
    view.set_query("  ");
    assert_eq!(view.snapshot().recent, vec!["sintel".to_string()]);

    // A title named exactly what was typed moves to the front of its row.
    view.set_query("TT2");
    let mut first = None;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "TT2"
            && !s.rows.is_empty()
            && s.rows.iter().all(|r| r.state == RowState::Ready)
        {
            first = Some(s.rows[0].tiles[0].id.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(first.as_deref(), Some("tt2"));
}

/// An open search runs again when a search addon is installed, so its row
/// appears without retyping.
#[tokio::test]
async fn an_open_search_picks_up_a_new_addon() {
    let core = Core::temporary("p").unwrap();
    let (a, _) = search_addon("a", &["tt1"]).await;
    let (b, _) = search_addon("b", &["tt2"]).await;
    core.library()
        .set_addons(core.profile(), std::slice::from_ref(&a), &core.hlc())
        .unwrap();
    let view = SearchView::open(&core);
    view.set_query("sintel");
    let rows = |n: usize| {
        let s = view.snapshot();
        s.rows.len() == n && s.rows.iter().all(|r| r.state == RowState::Ready)
    };
    for _ in 0..300 {
        if rows(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(1), "{:?}", view.snapshot());
    let mut b = b;
    b.position = 1;
    core.library()
        .set_addons(core.profile(), &[a, b], &core.hlc())
        .unwrap();
    for _ in 0..300 {
        if rows(2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(2), "{:?}", view.snapshot());
    assert_eq!(view.snapshot().query, "sintel");
}
