use std::{
    collections::VecDeque,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, bail};
use librqbit_dualstack_sockets::ConnectOpts;
use librqbit_utp::{BindDevice, UtpSocketUdp};
use serde::Serialize;
use tracing::debug;

use crate::{
    Error, PeerConnectionOptions, Result,
    peer_connection::with_timeout,
    type_aliases::{BoxAsyncReadVectored, BoxAsyncWrite},
    vectored_traits::AsyncReadVectoredIntoCompat,
};

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ConnectionKind {
    #[serde(rename = "tcp")]
    Tcp,
    #[serde(rename = "utp")]
    Utp,
    #[serde(rename = "socks")]
    Socks,
}

impl std::fmt::Display for ConnectionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionKind::Tcp => f.write_str("tcp"),
            ConnectionKind::Utp => f.write_str("uTP"),
            ConnectionKind::Socks => f.write_str("socks"),
        }
    }
}

pub struct ConnectionOptions {
    // socks5://[username:password@]host:port
    // If set, all outgoing connections will go through the proxy over TCP.
    pub proxy_url: Option<String>,
    // TCP outgoing connections are enabled by default
    pub enable_tcp: bool,
    pub peer_opts: Option<PeerConnectionOptions>,
    /// Marquee fork: dials in flight at once across the session, metadata
    /// and live, TCP and uTP; `None` sets no bound (upstream).
    pub max_half_open: Option<usize>,
    /// Marquee fork: dials started in any 60 s across the session; `None`
    /// sets no bound (upstream).
    pub dials_per_minute: Option<usize>,
    /// Marquee fork: peers asked at once for one magnet's metadata; `None`
    /// keeps upstream's 128.
    pub metadata_peers: Option<usize>,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self {
            enable_tcp: true,
            proxy_url: None,
            peer_opts: None,
            max_half_open: None,
            dials_per_minute: None,
            metadata_peers: None,
        }
    }
}

const DIAL_WINDOW: Duration = Duration::from_secs(60);

/// Marquee fork: the bounds every dial passes before it opens a socket. A
/// NAT keeps each half-open entry past our socket's end, so both the
/// number in flight and the number started per minute count.
#[derive(Debug, Default)]
pub(crate) struct DialGate {
    half_open: Option<tokio::sync::Semaphore>,
    /// The budget and the start times inside the last minute.
    per_minute: Option<(usize, Mutex<VecDeque<tokio::time::Instant>>)>,
}

impl DialGate {
    pub fn new(
        max_half_open: Option<usize>,
        dials_per_minute: Option<usize>,
    ) -> anyhow::Result<Self> {
        if max_half_open == Some(0) || dials_per_minute == Some(0) {
            bail!("a dial bound of 0 dials nothing");
        }
        Ok(Self {
            half_open: max_half_open.map(tokio::sync::Semaphore::new),
            per_minute: dials_per_minute.map(|n| (n, Mutex::new(VecDeque::with_capacity(n)))),
        })
    }

    /// Waits for a half-open slot, then for the minute's budget; the slot
    /// is held until the returned permit drops. Slots go in arrival order.
    pub async fn enter(&self) -> Option<tokio::sync::SemaphorePermit<'_>> {
        let slot = match &self.half_open {
            Some(s) => Some(s.acquire().await.expect("the gate is never closed")),
            None => None,
        };
        let Some((budget, starts)) = &self.per_minute else {
            return slot;
        };
        loop {
            let next = {
                let mut starts = starts.lock().unwrap_or_else(|e| e.into_inner());
                let now = tokio::time::Instant::now();
                while starts
                    .front()
                    .is_some_and(|t| now.duration_since(*t) >= DIAL_WINDOW)
                {
                    starts.pop_front();
                }
                if starts.len() < *budget {
                    starts.push_back(now);
                    return slot;
                }
                starts[0] + DIAL_WINDOW
            };
            tokio::time::sleep_until(next).await;
        }
    }
}

/// Marquee fork (C27): a check on peer addresses; its `Debug` leaves the
/// closure out.
#[derive(Clone)]
pub(crate) struct Permits(pub Arc<dyn Fn(IpAddr) -> bool + Send + Sync>);

impl std::fmt::Debug for Permits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Permits")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SocksProxyConfig {
    pub host: String,
    pub port: u16,
    pub username_password: Option<(String, String)>,
}

#[derive(Default, Debug, Clone)]
pub(crate) struct StreamConnectorArgs {
    pub enable_tcp: bool,
    pub socks_proxy_config: Option<SocksProxyConfig>,
    pub utp_socket: Option<Arc<UtpSocketUdp>>,
    pub bind_device: Option<BindDevice>,
    pub ipv4_only: bool,
    pub permits: Option<Permits>,
    pub max_half_open: Option<usize>,
    pub dials_per_minute: Option<usize>,
    pub metadata_peers: Option<usize>,
}

impl SocksProxyConfig {
    pub fn parse(url: &str) -> anyhow::Result<Self> {
        let url = ::url::Url::parse(url).context("invalid proxy URL")?;
        if url.scheme() != "socks5" {
            anyhow::bail!("proxy URL should have socks5 scheme");
        }
        let host = url.host_str().context("missing host")?;
        let port = url.port().context("missing port")?;
        let up = url
            .password()
            .map(|p| (url.username().to_owned(), p.to_owned()));
        Ok(Self {
            host: host.to_owned(),
            port,
            username_password: up,
        })
    }

    async fn connect(
        &self,
        addr: SocketAddr,
    ) -> tokio_socks::Result<(
        impl tokio::io::AsyncRead + Unpin + 'static,
        impl tokio::io::AsyncWrite + Unpin + 'static,
    )> {
        let proxy_addr = (self.host.as_str(), self.port);

        let stream = if let Some((username, password)) = self.username_password.as_ref() {
            tokio_socks::tcp::Socks5Stream::connect_with_password(
                proxy_addr,
                addr,
                username.as_str(),
                password.as_str(),
            )
            .await?
        } else {
            tokio_socks::tcp::Socks5Stream::connect(proxy_addr, addr).await?
        };

        Ok(tokio::io::split(stream))
    }
}

gen_stats!(SingleStatAtomic SingleStatSnapshot, [
    attempts u64,
    successes u64,
    errors u64
], []);
gen_stats!(PerFamilyAtomic PerFamilySnapshot, [], [
    v4 SingleStatAtomic SingleStatSnapshot,
    v6 SingleStatAtomic SingleStatSnapshot
]);
gen_stats!(ConnectStatsAtomic ConnectStatsSnapshot, [], [
    socks PerFamilyAtomic PerFamilySnapshot,
    tcp PerFamilyAtomic PerFamilySnapshot,
    utp PerFamilyAtomic PerFamilySnapshot
]);

#[derive(Debug)]
pub(crate) struct StreamConnector {
    proxy_config: Option<SocksProxyConfig>,
    enable_tcp: bool,
    bind_device: Option<BindDevice>,
    utp_socket: Option<Arc<librqbit_utp::UtpSocketUdp>>,
    stats: ConnectStatsAtomic,
    ipv4_only: bool,
    permits: Option<Permits>,
    gate: DialGate,
    metadata_peers: usize,
}

impl StreamConnector {
    pub async fn new(config: StreamConnectorArgs) -> anyhow::Result<Self> {
        #[allow(clippy::single_match)]
        match (
            config.socks_proxy_config.is_some(),
            config.enable_tcp,
            config.utp_socket.is_some(),
        ) {
            (false, false, false) => {
                bail!("no way to connect to peers, enable TCP, uTP or socks proxy")
            }
            _ => {
                // TODO: maybe validate other combinations. For now there's no way to disable TCP
            }
        }

        if config.metadata_peers == Some(0) {
            bail!("0 metadata peers asks no peer");
        }
        Ok(Self {
            proxy_config: config.socks_proxy_config,
            enable_tcp: config.enable_tcp,
            utp_socket: config.utp_socket,
            bind_device: config.bind_device,
            stats: Default::default(),
            ipv4_only: config.ipv4_only,
            permits: config.permits,
            gate: DialGate::new(config.max_half_open, config.dials_per_minute)?,
            metadata_peers: config.metadata_peers.unwrap_or(128),
        })
    }

    /// Marquee fork: peers asked at once for one magnet's metadata.
    pub fn metadata_peers(&self) -> usize {
        self.metadata_peers
    }

    fn get_stat(&self, kind: ConnectionKind, is_v6: bool) -> &SingleStatAtomic {
        let stat = match kind {
            ConnectionKind::Tcp => &self.stats.tcp,
            ConnectionKind::Utp => &self.stats.utp,
            ConnectionKind::Socks => &self.stats.socks,
        };
        if is_v6 { &stat.v6 } else { &stat.v4 }
    }

    async fn with_stat<R, E>(
        &self,
        kind: ConnectionKind,
        is_v6: bool,
        fut: impl Future<Output = std::result::Result<R, E>>,
    ) -> std::result::Result<R, E> {
        let stat = self.get_stat(kind, is_v6);
        stat.attempts(1);
        fut.await
            .inspect(|_| stat.successes(1))
            .inspect_err(|_| stat.errors(1))
    }

    async fn tcp_connect(
        &self,
        addr: SocketAddr,
    ) -> librqbit_dualstack_sockets::Result<tokio::net::TcpStream> {
        self.with_stat(
            ConnectionKind::Tcp,
            addr.is_ipv6(),
            librqbit_dualstack_sockets::tcp_connect(
                addr,
                ConnectOpts {
                    // Setting source port doesn't work with cloudflare warp on linux
                    // source_port: self.tcp_source_port,
                    source_port: None,
                    bind_device: self.bind_device.as_ref(),
                },
            ),
        )
        .await
    }

    pub fn stats(&self) -> &ConnectStatsAtomic {
        &self.stats
    }

    /// Marquee fork: `timeout` bounds the dial, not the wait at the gate.
    pub async fn connect(
        &self,
        addr: SocketAddr,
        timeout: Duration,
    ) -> Result<(ConnectionKind, BoxAsyncReadVectored, BoxAsyncWrite)> {
        if addr.port() == 0 {
            return Err(Error::Anyhow(anyhow::anyhow!(
                "invalid peer address (port 0): {}",
                addr
            )));
        }
        // Marquee fork (plans/desktop-release.md W4): Windows' IPv6 sockets
        // are v6-only, so an IPv4-mapped peer (one heard on the dual-stack
        // listener) is dialed as the IPv4 address it is.
        #[cfg(windows)]
        let addr = SocketAddr::new(addr.ip().to_canonical(), addr.port());

        if self.ipv4_only && addr.is_ipv6() {
            return Err(Error::Anyhow(anyhow::anyhow!(
                "ipv6 disabled, skipping connection to {}",
                addr
            )));
        }

        // Marquee fork (C27): every outgoing peer, from every source and in
        // both the metadata and the live phase, is dialed only here. The
        // message names no address.
        // ponytail: a refused peer takes the normal reconnect backoff (10 s,
        // x6, a day at most); refuse in task_peer_adder too if they crowd it.
        if self.permits.as_ref().is_some_and(|p| !(p.0)(addr.ip())) {
            return Err(Error::Anyhow(anyhow::anyhow!(
                "peer outside the address policy"
            )));
        }

        // Marquee fork: every dial passes the gate; a refused address above
        // spends nothing.
        let _slot = self.gate.enter().await;
        with_timeout("connecting", timeout, self.dial(addr)).await
    }

    async fn dial(
        &self,
        addr: SocketAddr,
    ) -> Result<(ConnectionKind, BoxAsyncReadVectored, BoxAsyncWrite)> {
        if let Some(proxy) = self.proxy_config.as_ref() {
            let (r, w) = self
                .with_stat(ConnectionKind::Socks, addr.is_ipv6(), proxy.connect(addr))
                .await?;
            debug!(?addr, "connected through SOCKS5");
            return Ok((
                ConnectionKind::Socks,
                Box::new(r.into_vectored_compat()),
                Box::new(w),
            ));
        }

        // Try to connect over TCP first. If in 1 second we haven't connected, try uTP also (if configured).
        // Whoever connects first wins.
        let tcp_connect = async {
            if !self.enable_tcp {
                return Ok(None);
            }
            let conn = self.tcp_connect(addr).await?;
            debug!(?addr, "connected over TCP");
            Ok::<_, librqbit_dualstack_sockets::Error>(Some(conn))
        };

        let tcp_failed_notify = tokio::sync::Notify::new();

        let utp_connect = async {
            let sock = match self.utp_socket.as_ref() {
                Some(sock) => sock,
                None => return Ok(None),
            };

            // Give TCP priority as it's more mature and simpler.
            if self.enable_tcp {
                // wait until either 1 second has passed or TCP failed.
                tokio::select! {
                    _ = tcp_failed_notify.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }

            let conn = self
                .with_stat(ConnectionKind::Utp, addr.is_ipv6(), sock.connect(addr))
                .await?;

            debug!(?addr, "connected over uTP");
            Ok(Some(conn))
        };

        tokio::pin!(tcp_connect);
        tokio::pin!(utp_connect);

        let mut tcp_err: Option<Option<librqbit_dualstack_sockets::Error>> = None;
        let mut utp_err: Option<Option<librqbit_utp::Error>> = None;

        // wait until all fail, or one succeeds.
        loop {
            if let (Some(tcp), Some(utp)) = (tcp_err.as_mut(), utp_err.as_mut()) {
                match (tcp.take(), utp.take()) {
                    (Some(tcp), Some(utp)) => return Err(Error::Connect { tcp, utp }),
                    (Some(tcp), None) => return Err(Error::TcpConnect(tcp)),
                    (None, Some(utp)) => return Err(Error::UtpConnect(utp)),
                    (None, None) => return Err(Error::ConnectDisabled),
                }
            }
            tokio::select! {
                tcp_res = &mut tcp_connect, if tcp_err.is_none() => {
                    match tcp_res {
                        Ok(Some(stream)) => {
                            let (r, w) = stream.into_split();
                            return Ok((ConnectionKind::Tcp, Box::new(r), Box::new(w)));
                        },
                        Ok(None) => {
                            tcp_err = Some(None);
                            tcp_failed_notify.notify_waiters();
                        }
                        Err(e) => {
                            tcp_err = Some(Some(e));
                            tcp_failed_notify.notify_waiters();
                        }
                    }
                },
                utp_res = &mut utp_connect, if utp_err.is_none() => {
                    match utp_res {
                        Ok(Some(stream)) => {
                            let (r, w) = stream.split();
                            return Ok((ConnectionKind::Utp, Box::new(r), Box::new(w)));
                        },
                        Ok(None) => {
                            utp_err = Some(None);
                        }
                        Err(e) => {
                            utp_err = Some(Some(e));
                        }
                    }
                },
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOON: Duration = Duration::from_millis(50);

    #[tokio::test(start_paused = true)]
    async fn a_dial_waits_for_a_half_open_slot() {
        let gate = DialGate::new(Some(2), None).unwrap();
        let first = gate.enter().await;
        let _second = gate.enter().await;
        assert!(tokio::time::timeout(SOON, gate.enter()).await.is_err());
        drop(first);
        assert!(tokio::time::timeout(SOON, gate.enter()).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_minute_starts_at_most_its_budget() {
        let gate = DialGate::new(None, Some(3)).unwrap();
        for _ in 0..3 {
            assert!(tokio::time::timeout(SOON, gate.enter()).await.is_ok());
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(59), gate.enter())
                .await
                .is_err(),
            "the budget is spent for this minute"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(2), gate.enter())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn connect_passes_the_gate_and_its_timeout_skips_the_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut kept = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                kept.push(s);
            }
        });
        let connector = Arc::new(
            StreamConnector::new(StreamConnectorArgs {
                enable_tcp: true,
                max_half_open: Some(1),
                dials_per_minute: Some(2),
                ..Default::default()
            })
            .await
            .unwrap(),
        );
        let wait = Duration::from_millis(300);

        // Held slot: the dial waits longer than its timeout, then connects.
        let held = connector.gate.enter().await;
        let dial = tokio::spawn({
            let c = connector.clone();
            async move { c.connect(addr, Duration::from_millis(100)).await.is_ok() }
        });
        tokio::time::sleep(wait).await;
        assert!(!dial.is_finished(), "a dial waits for the held slot");
        drop(held);
        assert!(dial.await.unwrap(), "the wait is not part of the timeout");

        // Two starts spent in this minute: the next waits.
        assert!(
            tokio::time::timeout(wait, connector.connect(addr, wait))
                .await
                .is_err()
        );
    }
}
