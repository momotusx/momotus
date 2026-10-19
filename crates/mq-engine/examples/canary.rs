//! The live check's host canary (plans/c27-torrent-policy.md 5): one port
//! per tracker form on `127.0.0.1`, which the Android emulator reaches as
//! `10.0.2.2`. Prints one JSON line per TCP connection or UDP datagram; the
//! check passes when a C27 build sends none.
//!
//! `cargo run -p mq-engine --example canary`

use std::io::Write;
use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::time::{SystemTime, UNIX_EPOCH};

/// H1, H2, H3 (HTTP, TCP) and U1, U2, U3 (UDP), as in the plan's table.
const TCP: [(u16, &str); 3] = [(47001, "H1"), (47003, "H2"), (47005, "H3")];
const UDP: [(u16, &str); 3] = [(47002, "U1"), (47004, "U2"), (47006, "U3")];

fn hit(form: &str, port: u16, proto: &str, from: std::net::SocketAddr) {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let line = format!(
        "{{\"ms\":{ms},\"form\":\"{form}\",\"port\":{port},\"proto\":\"{proto}\",\"from\":\"{from}\"}}"
    );
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn main() {
    let mut threads = Vec::new();
    for (port, form) in TCP {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("TCP port free");
        threads.push(std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Ok(from) = stream.peer_addr() {
                    hit(form, port, "tcp", from);
                }
            }
        }));
    }
    for (port, form) in UDP {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).expect("UDP port free");
        threads.push(std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((_, from)) = socket.recv_from(&mut buf) {
                hit(form, port, "udp", from);
            }
        }));
    }
    eprintln!("canary on 127.0.0.1: TCP 47001, 47003, 47005; UDP 47002, 47004, 47006");
    for t in threads {
        let _ = t.join();
    }
}
