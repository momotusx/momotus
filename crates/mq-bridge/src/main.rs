//! Development server: the bridge protocol over a WebSocket at `/ws`, plus an
//! optional static directory for the web UI build. Binds loopback only.

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use mq_bridge::Session;
use mq_runtime::{Core, CoreConfig, StoreId};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::mpsc;

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
    let mut app = Router::new().route("/ws", get(ws)).with_state(core);
    if let Ok(dir) = std::env::var("MQ_STATIC") {
        app = app.fallback_service(tower_http::services::ServeDir::new(dir));
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("{{\"event\":\"bridge_ready\",\"url\":\"ws://{addr}/ws\"}}");
    axum::serve(listener, app).await.expect("serve");
}

async fn ws(upgrade: WebSocketUpgrade, State(core): State<Core>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| serve_socket(socket, core))
}

async fn serve_socket(socket: WebSocket, core: Core) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sink
                .send(Message::Text(message.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
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
