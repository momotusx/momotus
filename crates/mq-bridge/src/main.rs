//! Development server: the bridge protocol over a WebSocket at `/ws`, plus an
//! optional static directory for the web UI build. Binds loopback only.

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::{Core, CoreConfig, StoreId};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("MQ_BRIDGE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7777);
    let data = std::env::var("MQ_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".mq-data"));
    let core = Core::open(CoreConfig {
        data_dir: data,
        profile_id: std::env::var("MQ_PROFILE").unwrap_or_else(|_| "default".into()),
        device_id: 0,
    })
    .expect("open core");
    core.set_debrid_keys(&StoreId::keys_from_env());
    mq_bridge::dev_trakt(&core);
    let mut app = Router::new().route("/ws", get(ws)).with_state(core);
    if let Ok(dir) = std::env::var("MQ_STATIC") {
        app = app.fallback_service(tower_http::services::ServeDir::new(dir));
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("{{\"event\":\"bridge_ready\",\"url\":\"ws://{addr}/ws\"}}");
    axum::serve(listener, app).await.expect("serve");
}

async fn ws(
    upgrade: WebSocketUpgrade,
    headers: HeaderMap,
    State(core): State<Core>,
) -> axum::response::Response {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if !origin_allowed(origin) {
        // Another site's page in the user's browser: no session for it.
        return StatusCode::FORBIDDEN.into_response();
    }
    upgrade
        .on_upgrade(move |socket| serve_socket(socket, core))
        .into_response()
}

/// A browser page may open the socket only from a loopback origin (this
/// server, or the web UI's dev server). A name is taken only as
/// `localhost`: another name that resolves to loopback is a rebound page
/// from elsewhere. A request with no `Origin` does not come from a page.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        // `null` (a file or a sandboxed frame) and anything else.
        return false;
    };
    let name = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    name.eq_ignore_ascii_case("localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

async fn serve_socket(socket: WebSocket, core: Core) {
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
    let mut session = Session::new(core, tx);
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
    use super::origin_allowed;

    #[test]
    fn only_this_server_and_loopback_pages_open_the_socket() {
        for ok in [
            None,
            Some("http://127.0.0.1:7777"),
            Some("http://localhost:5173"),
            Some("http://[::1]:5173"),
            Some("http://127.0.0.2"),
        ] {
            assert!(origin_allowed(ok), "{ok:?}");
        }
        for refused in [
            "https://evil.example",
            "http://192.168.1.5:7777",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "null",
            "file://",
        ] {
            assert!(!origin_allowed(Some(refused)), "{refused}");
        }
    }

    #[test]
    fn a_name_rebound_to_loopback_is_not_this_server() {
        // evil.example now resolves to 127.0.0.1: its page sends its own
        // name as Origin and as Host.
        assert!(!origin_allowed(Some("http://evil.example:7777")));
    }
}
