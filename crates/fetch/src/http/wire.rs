//! The secure connection: the name lookup and the connection under one connect deadline, an
//! optional proxy tunnel (`CONNECT`), then TLS through rustls with the RustCrypto provider and
//! certificate validation always on. Every wait wakes at least four times a second to check the
//! cancel flag, and no read waits longer than the stall timeout for data.
//!
//! Ported from LightCraft `crates/fetch/src/http.rs` (`Stream`, `lookup`, `connect_any`,
//! `tls_config`) at f34c7a8a, with the name-lookup timeout of its `origin/main` (e65b97e2, "Time
//! out and cancel the name lookup") (MIT OR Apache-2.0); https only, and the proxy tunnel is
//! Dalica's own (`crate::proxy`).

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::response::Wire;
use crate::proxy::{self, Proxy};
use crate::spec::Url;
use crate::transport::{Limits, TransportError};

/// How often a blocked wait wakes up to check the cancel flag.
pub(crate) const TICK: Duration = Duration::from_millis(250);

fn io(e: &std::io::Error) -> TransportError {
    TransportError::Io(e.to_string())
}

fn is_timeout(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted)
}

/// The client configuration: TLS 1.2 and 1.3 from the pure-Rust provider, trusting exactly
/// `roots` (DER certificates), or Mozilla's roots compiled in (`webpki-roots`) when `None`.
pub(crate) fn tls_config(roots: Option<&[Vec<u8>]>) -> Result<Arc<rustls::ClientConfig>, TransportError> {
    let store = match roots {
        None => rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() },
        Some(roots) => {
            let mut store = rustls::RootCertStore::empty();
            for der in roots {
                store
                    .add(rustls::pki_types::CertificateDer::from(der.clone()))
                    .map_err(|e| TransportError::Tls(format!("a root certificate: {e}")))?;
            }
            store
        }
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .with_root_certificates(store)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// `now + d`, or a day from now when that overflows.
pub(crate) fn deadline_after(d: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(d).or_else(|| now.checked_add(Duration::from_secs(86_400))).unwrap_or(now)
}

/// Resolves a host name within `deadline`, checking the cancel flag while it runs. The system
/// resolver blocks with no timeout of its own, so it runs on a helper thread (which ends when the
/// resolver returns); a lookup that takes too long is given up on, and Cancel works during it.
pub(crate) fn lookup(
    host: &str,
    resolve: impl FnOnce() -> std::io::Result<Vec<SocketAddr>> + Send + 'static,
    deadline: Instant,
    limits: &Limits<'_>,
) -> Result<Vec<SocketAddr>, TransportError> {
    if limits.cancel.load(Ordering::Relaxed) {
        return Err(TransportError::Cancelled);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("dalica-fetch-lookup".into())
        .spawn(move || {
            let _ = tx.send(resolve());
        })
        .map_err(|e| TransportError::Connect(format!("{host}: {e}")))?;
    loop {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(TransportError::Connect(format!("{host}: the name lookup timed out")));
        }
        match rx.recv_timeout(left.min(TICK)) {
            Ok(r) => return r.map_err(|e| TransportError::Connect(format!("{host}: {e}"))),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(TransportError::Connect(format!("{host}: the name lookup failed")));
            }
        }
    }
}

/// Connects to the first of `addrs` that answers, all of them together within `deadline`.
pub(crate) fn connect_any(host: &str, addrs: &[SocketAddr], deadline: Instant, limits: &Limits<'_>) -> Result<TcpStream, TransportError> {
    let mut last = format!("{host}: the name has no address");
    for a in addrs {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(TransportError::Connect(format!("{host}: connecting timed out")));
        }
        match TcpStream::connect_timeout(a, left) {
            Ok(s) => return Ok(s),
            Err(e) => last = format!("{host}: {e}"),
        }
    }
    Err(TransportError::Connect(last))
}

/// A TCP connection to `host:port` (a name looked up, then each address tried) within `deadline`,
/// with short socket timeouts so every wait can check the cancel flag.
fn tcp(host: &str, port: u16, deadline: Instant, limits: &Limits<'_>) -> Result<TcpStream, TransportError> {
    let name = host.to_string();
    let addrs = lookup(host, move || (name.as_str(), port).to_socket_addrs().map(Iterator::collect), deadline, limits)?;
    let sock = connect_any(host, &addrs, deadline, limits)?;
    sock.set_read_timeout(Some(TICK)).map_err(|e| io(&e))?;
    sock.set_write_timeout(Some(TICK)).map_err(|e| io(&e))?;
    let _ = sock.set_nodelay(true);
    Ok(sock)
}

/// A TLS connection to a server.
pub(crate) struct Tls {
    conn: rustls::ClientConnection,
    sock: TcpStream,
}

impl Tls {
    /// Connects to `url`'s host (through `proxy` when it applies to that host) and completes the
    /// TLS handshake, validating the server's certificate for that host against `config`'s roots.
    pub(crate) fn open(url: &Url, proxy: &Proxy, config: &Arc<rustls::ClientConfig>, limits: &Limits<'_>) -> Result<Tls, TransportError> {
        // the lookup, the connection attempts and the proxy's answer share one connect timeout
        let deadline = deadline_after(limits.connect);
        let sock = match proxy.route(url.host()) {
            Some((phost, pport)) => {
                let mut sock = tcp(phost, pport, deadline, limits)?;
                proxy::tunnel(&mut sock, url.host(), url.port(), deadline, limits)?;
                sock
            }
            None => tcp(url.host(), url.port(), deadline, limits)?,
        };
        let name =
            rustls::pki_types::ServerName::try_from(url.host().to_string()).map_err(|e| TransportError::Tls(format!("{}: {e}", url.host())))?;
        let conn = rustls::ClientConnection::new(Arc::clone(config), name).map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut tls = Tls { conn, sock };
        tls.handshake(limits)?;
        Ok(tls)
    }

    /// Waits for `f` to make progress, retrying timeouts until `stall` passes without any.
    fn retry<T>(limits: &Limits<'_>, mut f: impl FnMut() -> std::io::Result<T>) -> Result<T, TransportError> {
        let start = Instant::now();
        loop {
            if limits.cancel.load(Ordering::Relaxed) {
                return Err(TransportError::Cancelled);
            }
            match f() {
                Ok(v) => return Ok(v),
                Err(e) if is_timeout(&e) => {
                    if start.elapsed() >= limits.stall {
                        return Err(TransportError::Stalled);
                    }
                }
                Err(e) if e.kind() == ErrorKind::InvalidData => return Err(TransportError::Tls(e.to_string())),
                Err(e) => return Err(io(&e)),
            }
        }
    }

    fn handshake(&mut self, limits: &Limits<'_>) -> Result<(), TransportError> {
        let Tls { conn, sock } = self;
        while conn.is_handshaking() {
            Self::retry(limits, || conn.complete_io(sock))?;
        }
        Ok(())
    }

    /// Sends `data` through the TLS connection.
    pub(crate) fn write_all(&mut self, data: &[u8], limits: &Limits<'_>) -> Result<(), TransportError> {
        let Tls { conn, sock } = self;
        // buffered by rustls, then sent as TLS records
        conn.writer().write_all(data).map_err(|e| TransportError::Tls(e.to_string()))?;
        while conn.wants_write() {
            Self::retry(limits, || conn.write_tls(sock))?;
        }
        Ok(())
    }
}

impl Wire for Tls {
    /// Reads some bytes (0 at the end of the stream), waiting at most `stall` for them.
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        let Tls { conn, sock } = self;
        let start = Instant::now();
        loop {
            if limits.cancel.load(Ordering::Relaxed) {
                return Err(TransportError::Cancelled);
            }
            match conn.reader().read(buf) {
                Ok(n) => return Ok(n),
                // closed without close_notify: the body's length, chunks or pinned size tell
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(0),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(TransportError::Tls(e.to_string())),
            }
            // no plaintext buffered: more TLS records from the socket
            match conn.read_tls(sock) {
                Ok(0) => return Ok(0),
                Ok(_) => {
                    conn.process_new_packets().map_err(|e| TransportError::Tls(e.to_string()))?;
                    while conn.wants_write() {
                        Self::retry(limits, || conn.write_tls(sock))?;
                    }
                }
                Err(e) if is_timeout(&e) => {
                    if start.elapsed() >= limits.stall {
                        return Err(TransportError::Stalled);
                    }
                }
                Err(e) => return Err(io(&e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    fn limits(cancel: &AtomicBool) -> Limits<'_> {
        Limits { connect: Duration::from_millis(300), stall: Duration::from_secs(1), cancel }
    }

    /// A name lookup that hangs is given up on at the connect timeout, and Cancel stops it at once.
    #[test]
    fn a_hung_name_lookup_times_out_and_can_be_cancelled() {
        let cancel = AtomicBool::new(false);
        let slow = || {
            std::thread::sleep(Duration::from_secs(10));
            Ok(Vec::new())
        };
        let t = Instant::now();
        let r = lookup("slow.example", slow, deadline_after(Duration::from_millis(300)), &limits(&cancel));
        assert!(matches!(&r, Err(TransportError::Connect(m)) if m.contains("timed out")), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        cancel.store(true, Ordering::Relaxed);
        let t = Instant::now();
        assert!(matches!(lookup("slow.example", slow, deadline_after(Duration::from_secs(30)), &limits(&cancel)), Err(TransportError::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(1));
        // a lookup that answers is used
        let cancel = AtomicBool::new(false);
        let local: SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert_eq!(lookup("local", move || Ok(vec![local]), deadline_after(Duration::from_secs(5)), &limits(&cancel)).unwrap(), [local]);
    }

    /// Addresses share one deadline: once it has passed, no address is tried.
    #[test]
    fn addresses_share_one_connect_deadline() {
        let cancel = AtomicBool::new(false);
        let addrs: Vec<SocketAddr> = vec!["127.0.0.1:9".parse().unwrap(); 3];
        let r = connect_any("local", &addrs, Instant::now(), &limits(&cancel));
        assert!(matches!(&r, Err(TransportError::Connect(m)) if m.contains("timed out")), "{r:?}");
        assert!(matches!(connect_any("none", &[], deadline_after(Duration::from_secs(1)), &limits(&cancel)), Err(TransportError::Connect(_))));
    }

    #[test]
    fn the_tls_config_builds_with_the_pure_rust_provider() {
        assert!(tls_config(None).is_ok());
        assert!(matches!(tls_config(Some(&[b"not a certificate".to_vec()])), Err(TransportError::Tls(_))));
    }
}
