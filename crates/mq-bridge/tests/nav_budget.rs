//! Navigation push budgets (plans/perf-nav.md): the core requests the shared
//! walk sends (open Home, rest on it, open a title, Discover, type in Search)
//! against local addons, each held to a budget of pushes and bytes. A push
//! that changes nothing the screen shows is over budget anywhere: every app
//! rebuilds its screen for each push.

use mq_bridge::{Outgoing, Output, Session};
use mq_library::{AddonRow, OWNER};
use mq_runtime::Core;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const CATALOGS: usize = 30;

fn meta(n: usize) -> Value {
    json!({
        "id": format!("tt{n:07}"), "type": "movie", "name": format!("Title {n}"),
        "poster": format!("https://images.metahub.space/poster/small/tt{n:07}/img"),
        "background": format!("https://images.metahub.space/background/medium/tt{n:07}/img"),
        "releaseInfo": "2020", "imdbRating": "7.1",
        "description": "A film for the push budget.",
        "links": [{"name": "Drama", "category": "Genres", "url": "stremio:///discover"}]
    })
}

/// The answer to one addon path: a catalog of 40 titles, a search (two of
/// three catalogs find titles, one finds none), or a meta.
fn answer(path: &str) -> Value {
    if let Some(rest) = path.strip_prefix("/meta/movie/tt") {
        let n: usize = rest.trim_end_matches(".json").parse().unwrap_or(1);
        return json!({ "meta": meta(n) });
    }
    if path.starts_with("/catalog/movie/find2/") {
        return json!({ "metas": [] });
    }
    if path.starts_with("/catalog/movie/find") {
        return json!({ "metas": (0..20).map(|i| meta(9000 + i)).collect::<Vec<_>>() });
    }
    let c: usize = path
        .strip_prefix("/catalog/movie/c")
        .and_then(|r| r.split(['.', '/']).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    json!({ "metas": (0..40).map(|i| meta(c * 100 + i)).collect::<Vec<_>>() })
}

/// One addon with `CATALOGS` Home catalogs, three search catalogs and metas.
async fn addon(core: &Core) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                // Search catalogs answer apart, as real addons do: each answer
                // is its own update, and the empty one changes nothing shown.
                for (find, ms) in [("find0", 20), ("find1", 120), ("find2", 240)] {
                    if path.contains(find) {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                    }
                }
                let body = answer(&path).to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let search = |id: &str| json!({"type": "movie", "id": id, "name": "Find", "extra": [{"name": "search", "isRequired": true}]});
    let mut catalogs: Vec<Value> = (0..CATALOGS)
        .map(|c| json!({"type": "movie", "id": format!("c{c}"), "name": format!("Row {c}")}))
        .collect();
    catalogs.extend(["find0", "find1", "find2"].map(search));
    let manifest = json!({
        "id": "budget.test", "version": "1.0.0", "name": "Budget",
        "types": ["movie"], "resources": ["catalog", "meta"], "idPrefixes": ["tt"],
        "catalogs": catalogs
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(OWNER, &[row], &core.hlc())
        .unwrap();
}

/// A list diff applied as every app applies it.
fn apply(list: &mut Vec<Value>, diff: &Value) {
    let index = || diff["index"].as_u64().unwrap() as usize;
    match diff["op"].as_str().unwrap() {
        "append" => list.extend(diff["values"].as_array().unwrap().iter().cloned()),
        "clear" => list.clear(),
        "pushFront" => list.insert(0, diff["value"].clone()),
        "pushBack" => list.push(diff["value"].clone()),
        "popFront" => drop(list.remove(0)),
        "popBack" => drop(list.pop()),
        "insert" => list.insert(index(), diff["value"].clone()),
        "set" => list[index()] = diff["value"].clone(),
        "remove" => drop(list.remove(index())),
        "truncate" => list.truncate(diff["length"].as_u64().unwrap() as usize),
        "reset" => *list = diff["values"].as_array().unwrap().clone(),
        op => panic!("unknown diff {op}"),
    }
}

/// What an action cost: pushes, their bytes, and those that changed nothing.
#[derive(Debug, Default)]
struct Cost {
    pushes: usize,
    bytes: usize,
    unchanged: usize,
    resets: usize,
}

struct Client {
    session: Session,
    rx: mpsc::UnboundedReceiver<Outgoing>,
    next_id: u64,
    /// Each open view's state as an app holds it: Home and the title page
    /// by their parts, every other view as its last snapshot.
    views: HashMap<u64, (String, Value)>,
    /// Pushes taken while waiting for a reply, counted with the next settle.
    pending: Cost,
}

impl Client {
    async fn new() -> Self {
        let core = Core::temporary(OWNER).unwrap();
        addon(&core).await;
        let (out, rx) = Output::channel();
        Self {
            session: Session::new(core, out),
            rx,
            next_id: 1,
            views: HashMap::new(),
            pending: Cost::default(),
        }
    }

    /// Sends a request; its reply, past any pushes (which are applied).
    async fn call(&mut self, mut request: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = id.into();
        self.session.handle_text(&request.to_string()).await;
        loop {
            let message = tokio::time::timeout(Duration::from_secs(10), self.rx.recv())
                .await
                .expect("reply")
                .expect("open")
                .json();
            if message["id"].as_u64() == Some(id) {
                assert_eq!(message["ok"], true, "{message}");
                return message["result"].clone();
            }
            let bytes = message.to_string().len();
            let mut pending = std::mem::take(&mut self.pending);
            self.apply(&message, bytes, &mut pending);
            self.pending = pending;
        }
    }

    async fn open(&mut self, view: &str, params: Value) -> u64 {
        let result = self
            .call(json!({"op": "subscribe", "view": view, "params": params}))
            .await;
        let sub = result["sub"].as_u64().unwrap();
        self.views
            .insert(sub, (view.to_owned(), result["snapshot"].clone()));
        sub
    }

    fn apply(&mut self, push: &Value, bytes: usize, cost: &mut Cost) {
        let Some((view, state)) = push["sub"].as_u64().and_then(|s| self.views.get_mut(&s)) else {
            return;
        };
        let before = state.clone();
        for update in push["updates"].as_array().unwrap() {
            match (view.as_str(), update["kind"].as_str()) {
                ("home" | "title", Some("header")) => state["header"] = update["header"].clone(),
                ("title", Some("similar")) => state["similar"] = update["tiles"].clone(),
                ("home" | "title", Some(kind)) => {
                    let key = if kind == "rows" { "rows" } else { "episodes" };
                    let mut list = state[key].as_array().cloned().unwrap_or_default();
                    for diff in update["diffs"].as_array().unwrap() {
                        cost.resets += usize::from(diff["op"] == "reset");
                        apply(&mut list, diff);
                    }
                    state[key] = list.into();
                }
                _ => *state = update.clone(),
            }
        }
        cost.pushes += 1;
        cost.bytes += bytes;
        cost.unchanged += usize::from(*state == before);
    }

    /// Takes pushes until none came for `quiet`, at most `max`.
    async fn settle(&mut self, quiet: Duration, max: Duration) -> Cost {
        let mut cost = std::mem::take(&mut self.pending);
        let end = tokio::time::Instant::now() + max;
        while let Ok(Some(out)) =
            tokio::time::timeout_at(end.min(tokio::time::Instant::now() + quiet), self.rx.recv())
                .await
        {
            let bytes = out.text().map_or(0, str::len);
            let push = out.json();
            self.apply(&push, bytes, &mut cost);
        }
        cost
    }
}

const QUIET: Duration = Duration::from_millis(500);
const MAX: Duration = Duration::from_secs(8);

#[tokio::test]
async fn the_walk_stays_inside_its_push_budget() {
    let mut c = Client::new().await;

    // Open Home: every row's answer once, coalesced, and never all rows again.
    let home = c.open("home", json!({})).await;
    let open = c.settle(QUIET, MAX).await;
    eprintln!("home open {open:?}");
    let rows = c.views[&home].1["rows"].as_array().unwrap().clone();
    assert_eq!(rows.len(), CATALOGS, "every row answered");
    let shown = serde_json::to_string(&rows).unwrap().len();
    assert_eq!(open.resets, 0, "no row is sent twice as a reset: {open:?}");
    assert_eq!(open.unchanged, 0, "{open:?}");
    assert!(open.pushes <= 12, "Home open: {open:?}");
    assert!(
        open.bytes <= shown * 3 / 2,
        "Home open sent {open:?} for {shown} bytes of rows"
    );

    // Rest on Home: nothing at all.
    let idle = c
        .settle(Duration::from_secs(1), Duration::from_secs(1))
        .await;
    assert_eq!(idle.pushes, 0, "Home idle: {idle:?}");

    // Open a title, as the title page does.
    let title = c
        .open("title", json!({"kind": "movie", "id": "tt0000101"}))
        .await;
    c.call(json!({"op": "command", "sub": title, "command": {"type": "titleVisible", "visible": true}})).await;
    let opened = c.settle(QUIET, MAX).await;
    eprintln!("title open {opened:?}");
    assert_eq!(opened.unchanged, 0, "{opened:?}");
    assert!(
        opened.pushes <= 4 && opened.bytes <= 60_000,
        "title open: {opened:?}"
    );
    c.call(json!({"op": "unsubscribe", "sub": title})).await;

    // Discover.
    c.open("discover", json!({})).await;
    let discover = c.settle(QUIET, MAX).await;
    eprintln!("discover open {discover:?}");
    assert_eq!(discover.unchanged, 0, "{discover:?}");
    assert!(discover.pushes <= 3, "Discover open: {discover:?}");

    // Type three letters in Search, each answered before the next.
    let search = c.open("search", json!({})).await;
    let mut typing = c.settle(QUIET, MAX).await;
    for query in ["s", "st", "sta"] {
        c.call(json!({"op": "command", "sub": search, "command": {"type": "setQuery", "query": query}})).await;
        let key = c.settle(QUIET, MAX).await;
        eprintln!("search {query} {key:?}");
        typing.pushes += key.pushes;
        typing.bytes += key.bytes;
        typing.unchanged += key.unchanged;
    }
    eprintln!("search typing {typing:?}");
    assert_eq!(
        typing.unchanged, 0,
        "a push that changes nothing: {typing:?}"
    );
    assert!(typing.pushes <= 12, "Search typing: {typing:?}");
}
