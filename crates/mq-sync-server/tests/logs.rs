//! The binary writes its warnings to stderr, where journald keeps them, and
//! nothing but the ready event to stdout.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

/// The relay process, stopped however the test ends, so a failed
/// assertion never leaves it running.
struct Stopped(std::process::Child);

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_refused_sign_in_start_is_logged_on_stderr() {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-logs-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    // A free port for the relay, and a provider whose issuer nothing answers.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let identity = dir.join("identity.json");
    std::fs::write(
        &identity,
        serde_json::json!({
            "key": "11".repeat(32),
            "publicUrl": format!("http://127.0.0.1:{port}"),
            "oidc": [{ "id": "mock", "issuer": format!("http://127.0.0.1:{closed}"), "clientId": "c", "clientSecret": "s" }],
        })
        .to_string(),
    )
    .unwrap();
    let mut relay = Stopped(
        Command::new(env!("CARGO_BIN_EXE_mq-sync-server"))
            .args(["--db", dir.join("relay.sqlite").to_str().unwrap()])
            .args(["--listen", &format!("127.0.0.1:{port}")])
            .args(["--public-host", "127.0.0.1"])
            .args(["--identity", identity.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stdout = BufReader::new(relay.0.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert!(ready.contains("sync_server_ready"), "{ready}");

    // One plain HTTP request (the tests' reqwest has no TLS provider to build a client with).
    let body = serde_json::json!({ "verifierHash": "00".repeat(32), "purpose": "signin", "return": "momotus://auth" }).to_string();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "POST /v1/auth/mock/start HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(!answer.starts_with("HTTP/1.1 200"), "{answer}");

    relay.0.kill().unwrap();
    relay.0.wait().unwrap();
    let mut stderr = String::new();
    relay
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        stderr.contains("WARN") && stderr.contains("sign-in start"),
        "stderr: {stderr}"
    );
    assert!(rest.is_empty(), "stdout after the ready line: {rest}");
}
