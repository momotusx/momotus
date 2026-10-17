#![allow(dead_code)]
//! A tiny multi-request HTTP server with canned JSON routes, in the spirit of
//! `crates/mq-http/tests/client.rs`. Routes match on method, exact path and an
//! optional query substring; each route can answer a sequence of responses
//! (the last one repeats) to model state changes between polls.

use mq_http::{ClientConfig, HttpClient};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    /// Lowercased request head.
    pub head: String,
    pub body: String,
}

pub struct Route {
    method: &'static str,
    path: &'static str,
    query: Option<&'static str>,
    responses: Vec<(u16, String, Duration)>,
    headers: Vec<(&'static str, &'static str)>,
    hits: AtomicUsize,
}

pub fn route(method: &'static str, path: &'static str, status: u16, body: &str) -> Route {
    Route {
        method,
        path,
        query: None,
        responses: vec![(status, body.to_owned(), Duration::ZERO)],
        headers: Vec::new(),
        hits: AtomicUsize::new(0),
    }
}

impl Route {
    /// Only matches when the query string contains this text.
    pub fn query(mut self, text: &'static str) -> Self {
        self.query = Some(text);
        self
    }

    /// Next response after the previous ones were served.
    pub fn then(mut self, status: u16, body: &str) -> Self {
        self.responses
            .push((status, body.to_owned(), Duration::ZERO));
        self
    }

    /// The last response added is answered only after `delay` (a request
    /// in flight).
    pub fn delayed(mut self, delay: Duration) -> Self {
        if let Some(last) = self.responses.last_mut() {
            last.2 = delay;
        }
        self
    }

    pub fn header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }
}

pub struct Server {
    pub base: String,
    log: Arc<Mutex<Vec<Recorded>>>,
}

impl Server {
    pub async fn start(routes: Vec<Route>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(routes);
        let shared_log = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(handle(socket, routes.clone(), shared_log.clone()));
            }
        });
        Server { base, log }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.log.lock().unwrap().clone()
    }

    /// Requests whose target starts with `path`.
    pub fn count(&self, method: &str, path: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.method == method && r.target.starts_with(path))
            .count()
    }
}

async fn handle(mut socket: TcpStream, routes: Arc<Vec<Route>>, log: Arc<Mutex<Vec<Recorded>>>) {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let (head_end, content_length) = loop {
        let n = socket.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            return;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..pos]).to_ascii_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (pos + 4, len);
        }
    };
    while data.len() < head_end + content_length {
        let n = socket.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&data[..head_end]).to_string();
    let body = String::from_utf8_lossy(&data[head_end..]).to_string();
    let mut first = head.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_owned();
    let target = first.next().unwrap_or("").to_owned();
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    log.lock().unwrap().push(Recorded {
        method: method.clone(),
        target: target.clone(),
        head: head.to_ascii_lowercase(),
        body,
    });
    let found = routes.iter().find(|r| {
        r.method == method && r.path == path && r.query.is_none_or(|q| query.contains(q))
    });
    let (status, payload, extra) = match found {
        Some(r) => {
            let i = r
                .hits
                .fetch_add(1, Ordering::SeqCst)
                .min(r.responses.len() - 1);
            let (status, payload, delay) = r.responses[i].clone();
            tokio::time::sleep(delay).await;
            (status, payload, r.headers.clone())
        }
        None => (404, r#"{"error":"no route"}"#.to_owned(), Vec::new()),
    };
    let mut response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    for (name, value) in extra {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&payload);
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

pub fn client() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

pub const HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

/// A predicate over the error a provider call returned.
pub type ErrorCheck = fn(&mq_debrid::DebridError) -> bool;
