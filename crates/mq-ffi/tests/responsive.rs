//! The core as the apps open it (the binding's own two-thread runtime and
//! the one request loop) stays responsive while the Rotten Tomatoes addon's
//! lists are read: a hung list never holds a simple request back. A Pixel
//! start waited 10 s for the core on 2026-10-22.

use marquee_core::{CoreListener, MarqueeCore};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Every reply by its request id, with when it came.
#[derive(Default)]
struct Replies {
    by_id: Mutex<HashMap<u64, Instant>>,
    came: Condvar,
}

impl CoreListener for Replies {
    fn on_message(&self, json: String) {
        let Ok(message) = serde_json::from_str::<Value>(&json) else {
            return;
        };
        // One message, or a batch of them.
        let messages = match message {
            Value::Array(all) => all,
            one => vec![one],
        };
        let mut by_id = self.by_id.lock().unwrap();
        for m in messages {
            if let Some(id) = m.get("id").and_then(Value::as_u64) {
                by_id.insert(id, Instant::now());
            }
        }
        self.came.notify_all();
    }
}

impl Replies {
    fn wait(&self, id: u64, within: Duration) -> Option<Instant> {
        let deadline = Instant::now() + within;
        let mut by_id = self.by_id.lock().unwrap();
        loop {
            if let Some(at) = by_id.get(&id) {
                return Some(*at);
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            by_id = self.came.wait_timeout(by_id, left).unwrap().0;
        }
    }
}

/// A local addon on its own thread: `answer` gives each path's body, and
/// paths under `hang` answer after an hour. It counts what it was asked.
fn serve(
    answer: fn(&str) -> Option<String>,
    hang: &'static str,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let asked: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = asked.clone();
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { return };
            let log = log.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = socket.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                log.lock().unwrap().push(path.clone());
                if !hang.is_empty() && path.starts_with(hang) {
                    std::thread::sleep(Duration::from_secs(3600));
                }
                let (status, body) = match answer(&path) {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", "{}".to_owned()),
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (url, asked)
}

fn catalog(path: &str) -> Option<String> {
    let metas = json!({"metas": [
        {"id": "tt0111161", "type": "movie", "name": "The Shawshank Redemption"},
        {"id": "tt0068646", "type": "movie", "name": "The Godfather"}
    ]});
    match path {
        "/manifest.json" => Some(
            json!({"id": "local", "version": "1.0.0", "name": "Local", "types": ["movie"],
                "resources": ["catalog"], "idPrefixes": ["tt"],
                "catalogs": [{"type": "movie", "id": "top", "name": "Popular"}]})
            .to_string(),
        ),
        p if p.starts_with("/catalog/") => Some(metas.to_string()),
        _ => None,
    }
}

fn rotten_tomatoes(path: &str) -> Option<String> {
    match path {
        "/manifest.json" => Some(
            json!({"id": "pw.ers.rottentomatoes", "version": "1.0.9", "name": "Rotten Tomatoes",
                "types": ["movie"], "resources": ["catalog"], "idPrefixes": ["tt"],
                "catalogs": [{"type": "movie", "id": "rtfresh_movie", "name": "RT: Certified Fresh",
                    "extra": [{"name": "genre", "isRequired": false, "options": ["Drama", "Comedy"]}]}]})
            .to_string(),
        ),
        _ => Some(r#"{"metas":[{"id":"tt0111161","type":"movie","name":"The Shawshank Redemption"}]}"#.to_owned()),
    }
}

#[test]
fn a_simple_request_answers_at_once_while_the_lists_are_read() {
    let data = std::env::temp_dir().join(format!("mq-ffi-responsive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let replies = Arc::new(Replies::default());
    let core = MarqueeCore::open(
        data.to_string_lossy().into_owned(),
        "default".into(),
        0,
        replies.clone(),
    )
    .unwrap();
    let (local, _) = serve(catalog, "");
    // Every list page hangs: the read stays on its way for the whole test.
    let (rt, asked) = serve(rotten_tomatoes, "/catalog/");
    let mut id = 0;
    let mut send = |op: Value| {
        id += 1;
        let mut op = op;
        op["id"] = json!(id);
        core.send(op.to_string());
        id
    };
    for url in [&local, &rt] {
        let install = send(json!({"op": "installAddon", "url": format!("{url}/manifest.json")}));
        replies
            .wait(install, Duration::from_secs(10))
            .expect("installed");
    }
    let home = send(json!({"op": "subscribe", "view": "home"}));
    replies
        .wait(home, Duration::from_secs(10))
        .expect("Home opened");
    // Home's first lookup starts the read of the lists.
    let started = Instant::now();
    while !asked
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.starts_with("/catalog/"))
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the read never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
    for op in [
        json!({"op": "subscribe", "view": "addons"}),
        json!({"op": "streamConditions"}),
        json!({"op": "subscribe", "view": "home"}),
    ] {
        let sent = Instant::now();
        let request = send(op.clone());
        let came = replies
            .wait(request, Duration::from_secs(15))
            .unwrap_or_else(|| panic!("{op} never answered"));
        let took = came - sent;
        println!("{op}: {} ms", took.as_millis());
        assert!(took < Duration::from_millis(200), "{op} took {took:?}");
    }
    let version = Instant::now();
    assert!(!core.version().is_empty());
    assert!(version.elapsed() < Duration::from_millis(200));
    drop(core);
    let _ = std::fs::remove_dir_all(&data);
}
