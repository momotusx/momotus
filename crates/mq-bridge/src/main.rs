//! Development server: the bridge protocol over a WebSocket at `/ws`, plus an
//! optional static directory for the web UI build. Binds loopback only.

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{Request, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::{Core, CoreConfig, StoreId};
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("MQ_BRIDGE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7777);
    let data = std::env::var("MQ_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".mq-data"));
    // Error reports (plans/error-reports.md 6.1): the bridge prints no log,
    // so the reports' layer is its only subscriber; the panic hook first.
    {
        use tracing_subscriber::prelude::*;
        let _ = tracing_subscriber::registry()
            .with(mq_runtime::reports::layer())
            .try_init();
    }
    mq_runtime::reports::install(&data);
    let core = Core::open(CoreConfig {
        data_dir: data,
        profile_id: std::env::var("MQ_PROFILE").unwrap_or_else(|_| "default".into()),
        device_id: 0,
    })
    .expect("open core");
    core.set_debrid_keys(&StoreId::keys_from_env());
    mq_bridge::dev_trakt(&core);
    mq_bridge::dev_tmdb(&core);
    mq_bridge::dev_segments(&core);
    mq_bridge::dev_replay();
    let named = std::env::var("MQ_BRIDGE_ORIGINS").unwrap_or_default();
    let (origins, ignored) = allowed_origins(port, &named);
    for entry in ignored {
        eprintln!("MQ_BRIDGE_ORIGINS entry ignored: {entry} is not http(s)://host[:port]");
    }
    let mut app = router(core, origins.clone());
    if let Ok(dir) = std::env::var("MQ_STATIC") {
        app = with_static(app, PathBuf::from(dir));
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    let origins = serde_json::to_string(&origins).unwrap_or_default();
    println!("{{\"event\":\"bridge_ready\",\"url\":\"ws://{addr}/ws\",\"origins\":{origins}}}");
    axum::serve(listener, app).await.expect("serve");
}

#[derive(Clone)]
struct Bridge {
    core: Core,
    origins: Arc<[String]>,
    refused: Arc<Refused>,
}

fn router(core: Core, origins: Vec<String>) -> Router {
    let origins = origins.into();
    Router::new().route("/ws", get(ws)).with_state(Bridge {
        core,
        origins,
        refused: Arc::default(),
    })
}

/// The web build at `dir`. A path that names no file and has no extension
/// is a page of the app (a reload, a bookmark of /title/...): it gets
/// index.html and the app routes it. A missing asset stays a 404.
fn with_static(app: Router, dir: PathBuf) -> Router {
    let index = ServeFile::new(dir.join("index.html"));
    let page = move |request: Request| {
        let mut index = index.clone();
        async move {
            let name = request.uri().path().rsplit('/').next().unwrap_or_default();
            if name.contains('.') {
                return StatusCode::NOT_FOUND.into_response();
            }
            match index.try_call(request).await {
                Ok(response) => response.into_response(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    };
    app.fallback_service(ServeDir::new(dir).fallback(page.into_service()))
}

/// Origins already said refused: each is said once, so a page that keeps
/// knocking fills no log.
#[derive(Default)]
struct Refused(Mutex<HashSet<String>>);

/// Refused origins remembered; past this many, no more are said.
const REFUSED_MAX: usize = 64;
/// An origin as it is said, at most this many characters.
const ORIGIN_SAID_MAX: usize = 200;

impl Refused {
    /// `origin`, cut to `ORIGIN_SAID_MAX` characters, the first time it is
    /// refused; `None` after that.
    fn first(&self, origin: &str) -> Option<String> {
        let said: String = origin.chars().take(ORIGIN_SAID_MAX).collect();
        let mut seen = self.0.lock().unwrap_or_else(|e| e.into_inner());
        (seen.len() < REFUSED_MAX && seen.insert(said.clone())).then_some(said)
    }
}

async fn ws(
    upgrade: WebSocketUpgrade,
    headers: HeaderMap,
    State(Bridge {
        core,
        origins,
        refused,
    }): State<Bridge>,
) -> axum::response::Response {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if !origin_allowed(origin, &origins) {
        // Another program's page, or another site's: no session for it.
        // Said here, once per origin, since the page itself only sees a
        // failed handshake.
        if let Some(said) = refused.first(origin.unwrap_or_default()) {
            eprintln!(
                "socket refused for origin {said}; allowed: {}",
                origins.join(", ")
            );
        }
        return StatusCode::FORBIDDEN.into_response();
    }
    // The page reached this socket by a loopback name: the link stays on
    // this machine. Any other name came through something else (a
    // forwarded port), plain ws: no passkey output travels it.
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let private = host.is_some_and(loopback_authority);
    upgrade
        .on_upgrade(move |socket| serve_socket(socket, core, private))
        .into_response()
}

/// `localhost` or a loopback address, with or without a port.
fn loopback_authority(authority: &str) -> bool {
    let name = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    name.eq_ignore_ascii_case("localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The pages that may open the socket, as exact origins (scheme, name and
/// port): this server's own on `port` (it serves the web build under
/// `MQ_STATIC`), and those `named` lists (`MQ_BRIDGE_ORIGINS`,
/// comma-separated, such as the web dev server's, which
/// `apps/web/scripts/bridge.sh` names from its port). There is no other
/// default: any other loopback port is another program's page (a dev
/// server, a notebook, a local tool's web UI) and gets no session. Returns
/// the list and the named entries that are not `http(s)://host[:port]`.
fn allowed_origins(port: u16, named: &str) -> (Vec<String>, Vec<String>) {
    let mut allowed = ["127.0.0.1", "localhost", "[::1]"]
        .map(|name| format!("http://{name}:{port}"))
        .to_vec();
    let mut ignored = Vec::new();
    for entry in named.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        match page_origin(entry) {
            Some(origin) => allowed.push(origin),
            None => ignored.push(entry.to_owned()),
        }
    }
    (allowed, ignored)
}

/// `entry` as a browser sends it in `Origin`: lowercase, one trailing slash
/// dropped; `None` unless it is `http(s)://` and a host with an optional
/// port, with no path, query, fragment, credentials or space.
fn page_origin(entry: &str) -> Option<String> {
    let origin = entry
        .strip_suffix('/')
        .unwrap_or(entry)
        .to_ascii_lowercase();
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))?;
    let plain =
        !authority.is_empty() && !authority.contains(['/', '?', '#', '@', '*', ' ', '\\', ',']);
    plain.then_some(origin)
}

/// A browser page may open the socket only from one of `allowed`, matched
/// whole: another port or another name (a rebound page sends its own name)
/// is refused, and so is `null` (a file or a sandboxed frame). A request
/// with no `Origin` does not come from a page: a local process, which can
/// read the data directory anyway.
fn origin_allowed(origin: Option<&str>, allowed: &[String]) -> bool {
    origin.is_none_or(|origin| allowed.iter().any(|ok| ok.eq_ignore_ascii_case(origin)))
}

async fn serve_socket(socket: WebSocket, core: Core, private: bool) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = Output::channel();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let Outgoing::Message { text, held } = message else {
                // The client stopped reading past the session's budget: the
                // session ends, and the client opens a new one.
                let close = CloseFrame {
                    code: close_code::AGAIN,
                    reason: "overflow".into(),
                };
                let _ = sink.send(Message::Close(Some(close))).await;
                return;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                return;
            }
            // Written: its share of the budget goes back.
            drop(held);
        }
    });
    let mut session = Session::over(core, tx, private);
    while let Some(Ok(message)) = stream.next().await {
        if let Message::Text(text) = message {
            session.handle_text(&text).await;
        }
    }
    drop(session);
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::{allowed_origins, origin_allowed, router, with_static};
    use futures_util::{SinkExt, StreamExt};
    use mq_runtime::Core;
    use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest};

    /// The list `apps/web/scripts/bridge.sh` gives a bridge on `port`.
    fn dev(port: u16) -> Vec<String> {
        allowed_origins(port, "http://127.0.0.1:5173,http://localhost:5173").0
    }

    #[test]
    fn only_this_server_and_the_dev_server_open_the_socket() {
        let allowed = dev(7777);
        for ok in [
            None,
            Some("http://127.0.0.1:7777"),
            Some("http://localhost:7777"),
            Some("http://[::1]:7777"),
            Some("http://127.0.0.1:5173"),
            Some("http://LOCALHOST:5173"),
        ] {
            assert!(origin_allowed(ok, &allowed), "{ok:?}");
        }
        for refused in [
            "https://evil.example",
            "http://192.168.1.5:7777",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "null",
            "file://",
            "https://127.0.0.1:7777",
            "http://127.0.0.2:7777",
            "http://127.0.0.1",
            "http://localhost",
        ] {
            assert!(!origin_allowed(Some(refused), &allowed), "{refused}");
        }
    }

    #[test]
    fn a_page_on_another_loopback_port_is_refused() {
        let allowed = dev(7777);
        for port in [80, 3000, 5174, 7778, 8080, 8888] {
            for name in ["127.0.0.1", "localhost", "[::1]"] {
                let origin = format!("http://{name}:{port}");
                assert!(!origin_allowed(Some(&origin), &allowed), "{origin}");
            }
        }
    }

    #[test]
    fn with_nothing_named_only_this_server_opens_the_socket() {
        let (allowed, ignored) = allowed_origins(7777, "");
        assert!(ignored.is_empty());
        assert!(origin_allowed(Some("http://localhost:7777"), &allowed));
        // The Vite default port is another program's page unless named.
        for refused in ["http://127.0.0.1:5173", "http://localhost:5173", ""] {
            assert!(!origin_allowed(Some(refused), &allowed), "{refused}");
        }
    }

    #[test]
    fn named_origins_are_read_as_a_browser_sends_them() {
        let (allowed, ignored) = allowed_origins(
            17801,
            " HTTP://127.0.0.1:15301/ ,,http://localhost:5180,*,127.0.0.1:9000,http://a/b,http://u@h:1,https://",
        );
        assert!(origin_allowed(Some("http://127.0.0.1:15301"), &allowed));
        assert!(origin_allowed(Some("http://localhost:5180"), &allowed));
        assert!(origin_allowed(Some("http://127.0.0.1:17801"), &allowed));
        assert!(!origin_allowed(Some("http://127.0.0.1:5173"), &allowed));
        assert_eq!(
            ignored,
            [
                "*",
                "127.0.0.1:9000",
                "http://a/b",
                "http://u@h:1",
                "https://"
            ]
        );
    }

    #[test]
    fn a_refused_origin_is_said_once_and_cut_short() {
        let refused = super::Refused::default();
        assert_eq!(
            refused.first("http://localhost:3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(refused.first("http://localhost:3000"), None);
        let long = format!("http://{}.example", "a".repeat(500));
        assert_eq!(refused.first(&long).map(|s| s.chars().count()), Some(200));
        for port in 0..100 {
            refused.first(&format!("http://localhost:{port}"));
        }
        assert_eq!(
            refused.first("http://new.example"),
            None,
            "a full memory says no more"
        );
    }

    #[test]
    fn a_name_rebound_to_loopback_is_not_this_server() {
        // evil.example now resolves to 127.0.0.1: its page sends its own
        // name as Origin and as Host.
        assert!(!origin_allowed(
            Some("http://evil.example:7777"),
            &dev(7777)
        ));
    }

    /// The real server: a page on another loopback port gets no socket, so
    /// no op of any kind reaches a session; the dev server's page and a
    /// client with no `Origin` get one and are answered.
    #[tokio::test]
    async fn the_server_refuses_another_ports_page_before_any_op() {
        let core = Core::temporary("default").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = router(core, dev(port));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let url = format!("ws://127.0.0.1:{port}/ws");
        let open = |origin: Option<&str>| {
            let mut request = url.as_str().into_client_request().unwrap();
            if let Some(origin) = origin {
                request
                    .headers_mut()
                    .insert("origin", origin.parse().unwrap());
            }
            tokio_tungstenite::connect_async(request)
        };
        for origin in [
            "http://localhost:3000".to_owned(),
            "http://127.0.0.1:8080".to_owned(),
            format!("http://127.0.0.1:{}", port + 1),
        ] {
            match open(Some(&origin)).await {
                Err(tungstenite::Error::Http(response)) => {
                    assert_eq!(response.status(), 403, "{origin}")
                }
                other => panic!("{origin} opened a socket: {:?}", other.map(|_| ())),
            }
        }
        let own = format!("http://localhost:{port}");
        for origin in [Some("http://127.0.0.1:5173"), Some(own.as_str()), None] {
            let (mut socket, _) = open(origin).await.unwrap();
            socket
                .send(Message::text(r#"{"id":1,"op":"ping"}"#))
                .await
                .unwrap();
            let reply = socket.next().await.unwrap().unwrap();
            assert!(
                reply.to_text().unwrap().contains(r#""ok":true"#),
                "{origin:?}: {reply}"
            );
        }
        server.abort();
    }

    /// A reload or a link to an inner page gets the app; a missing asset is
    /// still a 404, so a stale bundle name never loads the page as script.
    #[tokio::test]
    async fn an_app_page_gets_index_html_and_a_missing_asset_404() {
        let dir = std::env::temp_dir().join(format!("mq-static-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>app").unwrap();
        std::fs::write(dir.join("assets/a.js"), "js").unwrap();
        let core = Core::temporary("default").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = with_static(router(core, dev(port)), dir.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        for (path, status, body) in [
            ("/", 200, "<!doctype html>app"),
            ("/title/movie/tt0056923", 200, "<!doctype html>app"),
            ("/welcome?step=2", 200, "<!doctype html>app"),
            ("/settings/", 200, "<!doctype html>app"),
            ("/assets/a.js", 200, "js"),
            ("/assets/missing.js", 404, ""),
            ("/favicon.ico", 404, ""),
        ] {
            let response = reqwest::get(format!("http://127.0.0.1:{port}{path}"))
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{path}");
            assert_eq!(response.text().await.unwrap(), body, "{path}");
        }
        server.abort();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_socket_reached_by_another_name_is_not_a_private_link() {
        use super::loopback_authority;
        for private in [
            "127.0.0.1:7777",
            "localhost:7777",
            "[::1]:7777",
            "LOCALHOST",
        ] {
            assert!(loopback_authority(private), "{private}");
        }
        for plain in ["192.168.1.5:7777", "marquee.lan:7777", "relay.example", ""] {
            assert!(!loopback_authority(plain), "{plain}");
        }
    }
}
