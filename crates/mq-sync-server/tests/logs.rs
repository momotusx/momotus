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

/// One plain HTTP request to the relay: the whole answer as text.
fn ask(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `--mail` (plans/accounts-email.md): a file that is not there starts the
/// relay with email sign-in off; with one, a mail the service never takes
/// is logged without the address or the code.
#[test]
fn the_mail_file_is_optional_and_logs_never_name_the_address() {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-logs-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let identity = dir.join("identity.json");
    let mail = dir.join("ses.json");
    let run = |port: u16| {
        std::fs::write(
            &identity,
            serde_json::json!({ "key": "11".repeat(32), "publicUrl": format!("http://127.0.0.1:{port}") })
                .to_string(),
        )
        .unwrap();
        let mut relay = Stopped(
            Command::new(env!("CARGO_BIN_EXE_mq-sync-server"))
                .args(["--db", dir.join(format!("{port}.sqlite")).to_str().unwrap()])
                .args(["--listen", &format!("127.0.0.1:{port}")])
                .args(["--public-host", "127.0.0.1"])
                .args(["--identity", identity.to_str().unwrap()])
                .args(["--mail", mail.to_str().unwrap()])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut ready = String::new();
        BufReader::new(relay.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert!(ready.contains("sync_server_ready"), "{ready}");
        relay
    };
    let stop = |mut relay: Stopped| {
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
        stderr
    };

    let port = free_port();
    let off = run(port);
    let info = ask(port, "GET", "/v1/relay", "");
    assert!(info.contains("\"providers\":[]"), "{info}");
    assert!(stop(off).contains("email sign-in is off"));

    // A mail endpoint nothing answers on this machine.
    std::fs::write(
        &mail,
        serde_json::json!({
            "accessKeyId": "AKID", "secretAccessKey": "secret", "region": "us-east-1",
            "from": "noreply@example.org", "endpoint": format!("http://127.0.0.1:{}", free_port()),
        })
        .to_string(),
    )
    .unwrap();
    let port = free_port();
    let on = run(port);
    assert!(ask(port, "GET", "/v1/relay", "").contains("\"providers\":[\"email\"]"));
    let body = serde_json::json!({ "verifierHash": "00".repeat(32), "purpose": "signin", "email": "Secret.Person@example.org" });
    let answer = ask(port, "POST", "/v1/auth/email/start", &body.to_string());
    // No answer from the mail service is unclear: the flow comes back (E9).
    assert!(
        answer.starts_with("HTTP/1.1 200") && answer.contains("\"flow\""),
        "{answer}"
    );
    let stderr = stop(on);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        stderr.contains("WARN") && stderr.contains("mail service"),
        "stderr: {stderr}"
    );
    for secret in [
        "secret.person",
        "Secret.Person",
        "example.org",
        "AKID",
        "secret",
    ] {
        assert!(!stderr.contains(secret), "stderr names {secret}: {stderr}");
    }
}
