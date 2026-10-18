//! The HTTPS transport against local servers on loopback (`security.md` §8.2): a download and a
//! ranged resume, redirects (an https → http one refused, a loop capped), a truncated body, a
//! stalled server, oversized headers, a proxy `CONNECT` used when set and skipped for a `NO_PROXY`
//! host, and certificates that do not validate. No internet: every server listens on 127.0.0.1
//! with a certificate signed by a test root made for these tests (below; ECDSA P-256, valid
//! 2026–2125), and the transport trusts that root only, or the bundled web roots.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_fetch::spec::hex;
use dalica_fetch::{Consent, Downloaded, FetchError, FileSpec, HttpsTransport, Options, Proxy, TransportError, USER_AGENT, download};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

/// The test root (CN "Dalica test root").
const ROOT: &str = "-----BEGIN CERTIFICATE-----
MIIBnDCCAUKgAwIBAgITeDFkTlJPkPQi+FhzWzD6h15STzAKBggqhkjOPQQDAjAb
MRkwFwYDVQQDDBBEYWxpY2EgdGVzdCByb290MCAXDTI2MDEwMTAwMDAwMFoYDzIx
MjUxMjMxMDAwMDAwWjAbMRkwFwYDVQQDDBBEYWxpY2EgdGVzdCByb290MFkwEwYH
KoZIzj0CAQYIKoZIzj0DAQcDQgAEcw97VXPOStu/ZNh74Gj11zGvmznTzgs44CsD
p+/Zq3CnKnhlxoy3j5H8MuwZOVaGoBsjZbn66qW4Q3ae12RPE6NjMGEwHQYDVR0O
BBYEFKdFHWy9a+yHOX4nUnlHBowbIpBDMB8GA1UdIwQYMBaAFKdFHWy9a+yHOX4n
UnlHBowbIpBDMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMAoGCCqG
SM49BAMCA0gAMEUCICw+qMn3k1FME4fAt6JufUVcHuykis67fzhy/oCaP/yGAiEA
yFbCe0MnTPgMPc2KX0wAYNxF+7t28lqrdYHXJcYdFQ4=
-----END CERTIFICATE-----
";
/// A server certificate for `localhost` and 127.0.0.1, signed by [`ROOT`].
const LOCAL: &str = "-----BEGIN CERTIFICATE-----
MIIBwjCCAWigAwIBAgIUC2W68T7mumfgOsy3b/DU2eYn9v0wCgYIKoZIzj0EAwIw
GzEZMBcGA1UEAwwQRGFsaWNhIHRlc3Qgcm9vdDAgFw0yNjAxMDEwMDAwMDBaGA8y
MTI1MTIzMTAwMDAwMFowEDEOMAwGA1UEAwwFbG9jYWwwWTATBgcqhkjOPQIBBggq
hkjOPQMBBwNCAATriJr8GXnd1mgUGf1afdhdxh6idqAA+CKwvEccdD4sSRYRhG3U
Ns0plVKQfgNF3I9JRCosaV0dHR4WF1Me/sP0o4GSMIGPMAwGA1UdEwEB/wQCMAAw
DgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMBoGA1UdEQQTMBGC
CWxvY2FsaG9zdIcEfwAAATAdBgNVHQ4EFgQUiiVHNrnCIKFYqn5kZvFKE5vxj4Qw
HwYDVR0jBBgwFoAUp0UdbL1r7Ic5fidSeUcGjBsikEMwCgYIKoZIzj0EAwIDSAAw
RQIhAPjv8qnTjQoZn8br2ZnabFO/QbiFAqCHOcS80iZLT2J3AiAZm9Yw9K/kQ18d
FH0bCQZQTNO1IRvTm4CsBN93+XLkyw==
-----END CERTIFICATE-----
";
const LOCAL_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg/atjZegGyCAW8rBM
xypoX/ClzoknKjBWp6PYlW8lY9qhRANCAATriJr8GXnd1mgUGf1afdhdxh6idqAA
+CKwvEccdD4sSRYRhG3UNs0plVKQfgNF3I9JRCosaV0dHR4WF1Me/sP0
-----END PRIVATE KEY-----
";
/// A server certificate for `other.example` only, signed by [`ROOT`].
const OTHER: &str = "-----BEGIN CERTIFICATE-----
MIIBwDCCAWagAwIBAgIUC2W68T7mumfgOsy3b/DU2eYn9v4wCgYIKoZIzj0EAwIw
GzEZMBcGA1UEAwwQRGFsaWNhIHRlc3Qgcm9vdDAgFw0yNjAxMDEwMDAwMDBaGA8y
MTI1MTIzMTAwMDAwMFowEDEOMAwGA1UEAwwFb3RoZXIwWTATBgcqhkjOPQIBBggq
hkjOPQMBBwNCAATvipbutbisjqpJ5s1IcbrRcxZ2pMHzGe6zC3MW99SKrlzsnRH4
9SPX3+652LvytL5LBucJOBWpwm4PWfcc9L/ho4GQMIGNMAwGA1UdEwEB/wQCMAAw
DgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoGCCsGAQUFBwMBMBgGA1UdEQQRMA+C
DW90aGVyLmV4YW1wbGUwHQYDVR0OBBYEFKMk8q/TkfHB2MEJKcok+SfOHVzfMB8G
A1UdIwQYMBaAFKdFHWy9a+yHOX4nUnlHBowbIpBDMAoGCCqGSM49BAMCA0gAMEUC
IQC6tZ8UUDXJ2hQL0DrysydnaRSS4tKT4jVGzGQUtIPBHgIgQCu23Vwsp4eDS5uA
02koSfuLyO6aggexdU6jELk+wcc=
-----END CERTIFICATE-----
";
const OTHER_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgLrVYvLfMX8OF7+Fy
AT50e778V3e29116loLbx9GLqSmhRANCAATvipbutbisjqpJ5s1IcbrRcxZ2pMHz
Ge6zC3MW99SKrlzsnRH49SPX3+652LvytL5LBucJOBWpwm4PWfcc9L/h
-----END PRIVATE KEY-----
";

fn root() -> Vec<Vec<u8>> {
    vec![CertificateDer::from_pem_slice(ROOT.as_bytes()).unwrap().to_vec()]
}

/// The transport trusting the test root, through `proxy`.
fn transport(proxy: Proxy) -> HttpsTransport {
    HttpsTransport::with_roots(proxy, &root()).unwrap()
}

/// What a test server does with one request.
enum Answer {
    /// Sends the bytes, then closes the TLS session properly.
    Send(Vec<u8>),
    /// Sends the bytes, then drops the connection without ending the TLS session.
    Cut(Vec<u8>),
    /// Sends the bytes, then holds the connection open, silent.
    Stall(Vec<u8>),
}

/// A TLS server on 127.0.0.1 answering every connection with `answer(index, request head)`; it
/// records each request head it read.
struct Server {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(cert: &str, key: &str, answer: impl Fn(usize, &str) -> Answer + Send + Sync + 'static) -> Server {
        let chain = vec![CertificateDer::from_pem_slice(cert.as_bytes()).unwrap()];
        let key = PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let answer = Arc::new(answer);
        let seen = Arc::clone(&heads);
        std::thread::spawn(move || {
            for (i, sock) in listener.incoming().enumerate() {
                let Ok(sock) = sock else { continue };
                let (config, answer, seen) = (Arc::clone(&config), Arc::clone(&answer), Arc::clone(&seen));
                std::thread::spawn(move || serve_one(sock, config, i, answer.as_ref(), &seen));
            }
        });
        Server { port, heads }
    }

    fn url(&self, path: &str) -> String {
        format!("https://127.0.0.1:{}{path}", self.port)
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

fn serve_one(sock: TcpStream, config: Arc<rustls::ServerConfig>, i: usize, answer: &dyn Fn(usize, &str) -> Answer, seen: &Mutex<Vec<String>>) {
    sock.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let conn = rustls::ServerConnection::new(config).unwrap();
    let mut tls = rustls::StreamOwned::new(conn, sock);
    // a handshake the client refuses ends here, before any request
    let Some(head) = read_head(&mut tls) else { return };
    seen.lock().unwrap().push(head.clone());
    let (bytes, end) = match answer(i, &head) {
        Answer::Send(b) => (b, 0),
        Answer::Cut(b) => (b, 1),
        Answer::Stall(b) => (b, 2),
    };
    if tls.write_all(&bytes).and_then(|()| tls.flush()).is_err() {
        return;
    }
    match end {
        0 => {
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        1 => {
            let _ = tls.sock.shutdown(Shutdown::Both);
        }
        _ => std::thread::sleep(Duration::from_secs(20)),
    }
}

/// Reads a request head (up to the empty line) from `r`.
fn read_head(r: &mut impl Read) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match r.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// An HTTP proxy on 127.0.0.1: answers `CONNECT` with `status` and, on 200, tunnels to the
/// address asked for; records each request head.
struct ProxyServer {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl ProxyServer {
    fn start(status: u16) -> ProxyServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&heads);
        std::thread::spawn(move || {
            for sock in listener.incoming() {
                let Ok(mut sock) = sock else { continue };
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || {
                    let Some(head) = read_head(&mut sock) else { return };
                    seen.lock().unwrap().push(head.clone());
                    let target = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                    if status != 200 {
                        let _ = sock.write_all(format!("HTTP/1.1 {status} No\r\nContent-Length: 0\r\n\r\n").as_bytes());
                        return;
                    }
                    let Ok(server) = TcpStream::connect(&target) else { return };
                    let _ = sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                    let (mut a, mut b) = (sock.try_clone().unwrap(), server.try_clone().unwrap());
                    let (mut sock, mut server) = (sock, server);
                    std::thread::spawn(move || {
                        let _ = std::io::copy(&mut a, &mut server);
                    });
                    let _ = std::io::copy(&mut b, &mut sock);
                });
            }
        });
        ProxyServer { port, heads }
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

/// Seeded bytes standing in for a model file.
fn file(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(29) ^ ((i >> 8) as u8)).collect()
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
    r.extend_from_slice(body);
    r
}

fn get(t: &HttpsTransport, url: &str, dir: &Path, opts: &Options) -> Result<Downloaded, FetchError> {
    let spec = FileSpec::new(url, "file.bin", 1 << 20, None, None).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    download(&spec, &consent, dir, t, opts, &AtomicBool::new(false), &mut |_| {})
}

fn once() -> Options {
    Options { connect_timeout: Duration::from_secs(5), stall_timeout: Duration::from_secs(5), attempts: 1 }
}

#[test]
fn a_download_is_cut_and_resumes_with_a_range() {
    let bytes = file(200_000);
    let half = 70_000;
    let body = bytes.clone();
    let server = Server::start(LOCAL, LOCAL_KEY, move |i, head| {
        if i == 0 {
            // announces the whole file, sends part of it, drops the connection
            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
            r.extend_from_slice(&body[..half]);
            return Answer::Cut(r);
        }
        let from: usize = head.split("Range: bytes=").nth(1).and_then(|r| r.split('-').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
        let rest = &body[from..];
        let mut r = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\n\r\n",
            rest.len(),
            body.len() - 1,
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(rest);
        Answer::Send(r)
    });
    let sha = hex(&Sha256::digest(&bytes));
    let spec = FileSpec::new(&server.url("/models/face.onnx"), "face.onnx", 1 << 20, Some(bytes.len() as u64), Some(&sha)).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let opts = Options { attempts: 2, ..once() };
    let done = download(&spec, &consent, tmp.path(), &transport(Proxy::direct()), &opts, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), bytes);
    assert!(done.resumed_from > 0 && done.resumed_from <= half as u64, "{done:?}");
    let heads = server.heads();
    assert_eq!(heads.len(), 2);
    assert!(!heads[0].contains("Range:"), "{}", heads[0]);
    assert!(heads[1].contains(&format!("Range: bytes={}-\r\n", done.resumed_from)), "{}", heads[1]);
    // exactly the headers fetch built, plus Host and Connection
    let names: Vec<&str> = heads[1].lines().skip(1).filter(|l| !l.is_empty()).map(|l| l.split(':').next().unwrap()).collect();
    assert_eq!(names, ["Host", "User-Agent", "Accept", "Accept-Encoding", "Range", "Connection"]);
    assert!(heads[1].starts_with("GET /models/face.onnx HTTP/1.1\r\n"));
    assert!(heads[1].contains(&format!("Host: 127.0.0.1:{}\r\n", server.port)));
    assert!(heads[1].contains(&format!("User-Agent: {USER_AGENT}\r\n")));
}

#[test]
fn a_truncated_body_fails() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| {
        let mut r = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n".to_vec();
        r.extend_from_slice(&[7u8; 400]);
        Answer::Cut(r)
    });
    let tmp = tempfile::tempdir().unwrap();
    let r = get(&transport(Proxy::direct()), &server.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Io(m))) if m.contains("before the end")), "{r:?}");
    // a chunked body cut in a chunk fails too
    let chunked = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Cut(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nabc".to_vec()));
    let r = get(&transport(Proxy::direct()), &chunked.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Io(_)))), "{r:?}");
}

#[test]
fn redirects_are_followed_capped_and_never_to_http() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, head| {
        if head.starts_with("GET /start ") {
            Answer::Send(b"HTTP/1.1 302 Found\r\nLocation: /dir/real?sig=1\r\nContent-Length: 4\r\n\r\nskip".to_vec())
        } else {
            Answer::Send(ok(b"the file"))
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let done = get(&transport(Proxy::direct()), &server.url("/start"), &tmp.path().join("ok"), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"the file");
    let heads = server.heads();
    assert_eq!(heads.len(), 2);
    assert!(heads[1].starts_with("GET /dir/real?sig=1 HTTP/1.1\r\n"), "{}", heads[1]);

    // https → http is refused, however it is written, and nothing is requested from it
    for location in ["http://127.0.0.1:{port}/x", "HTTP://127.0.0.1:{port}/x", "ftp://127.0.0.1/x"] {
        let to_http = Server::start(LOCAL, LOCAL_KEY, move |_, _| {
            Answer::Send(format!("HTTP/1.1 301 Moved\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n").into_bytes())
        });
        let dir = tmp.path().join("http");
        // with every try allowed: a refused redirect is final, never asked again
        let r = get(&transport(Proxy::direct()), &to_http.url("/x"), &dir, &Options { attempts: 3, ..once() });
        assert!(matches!(&r, Err(FetchError::Transport(TransportError::Refused(m))) if m.contains("refused") || m.contains("unsupported")), "{r:?}");
        assert_eq!(to_http.heads().len(), 1);
        assert!(!dir.join("file.bin").exists());
    }

    // a redirect loop stops after the cap
    let looping =
        Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(b"HTTP/1.1 307 Again\r\nLocation: /again\r\nContent-Length: 0\r\n\r\n".to_vec()));
    let r = get(&transport(Proxy::direct()), &looping.url("/again"), &tmp.path().join("loop"), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Protocol(m))) if m.contains("too many")), "{r:?}");
    assert_eq!(looping.heads().len(), dalica_fetch::http::MAX_REDIRECTS + 1);
}

#[test]
fn a_stalled_server_times_out_and_cancel_stops_the_wait() {
    // silent after part of the body
    let server = Server::start(LOCAL, LOCAL_KEY, |i, _| {
        if i == 0 { Answer::Stall(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc".to_vec()) } else { Answer::Stall(Vec::new()) }
    });
    let tmp = tempfile::tempdir().unwrap();
    let opts = Options { stall_timeout: Duration::from_millis(600), ..once() };
    let t = Instant::now();
    let r = get(&transport(Proxy::direct()), &server.url("/f"), tmp.path(), &opts);
    assert!(matches!(r, Err(FetchError::Transport(TransportError::Stalled))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // silent before the head
    let t = Instant::now();
    let r = get(&transport(Proxy::direct()), &server.url("/g"), tmp.path(), &opts);
    assert!(matches!(r, Err(FetchError::Transport(TransportError::Stalled))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // a long stall timeout is cut short by Cancel
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::Relaxed);
    });
    let spec = FileSpec::new(&server.url("/h"), "h.bin", 1 << 20, None, None).unwrap();
    let consent = Consent::acknowledged(&spec, true).unwrap();
    let t = Instant::now();
    let opts = Options { stall_timeout: Duration::from_secs(60), ..once() };
    let r = download(&spec, &consent, tmp.path(), &transport(Proxy::direct()), &opts, &cancel, &mut |_| {});
    assert!(matches!(r, Err(FetchError::Cancelled)), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn oversized_headers_are_refused() {
    let server = Server::start(LOCAL, LOCAL_KEY, |i, _| {
        let mut r = b"HTTP/1.1 200 OK\r\n".to_vec();
        if i == 0 {
            // one header line past the cap
            r.extend_from_slice(format!("X-Big: {}\r\n", "a".repeat(dalica_fetch::http::MAX_HEAD)).as_bytes());
        } else {
            // many small ones
            for n in 0..=dalica_fetch::http::MAX_HEADERS {
                r.extend_from_slice(format!("X-{n}: v\r\n").as_bytes());
            }
        }
        r.extend_from_slice(b"Content-Length: 1\r\n\r\nx");
        Answer::Send(r)
    });
    let tmp = tempfile::tempdir().unwrap();
    for path in ["/long", "/many"] {
        let r = get(&transport(Proxy::direct()), &server.url(path), tmp.path(), &once());
        assert!(matches!(&r, Err(FetchError::Transport(TransportError::Protocol(m))) if m.contains("too")), "{path}: {r:?}");
    }
    assert!(!tmp.path().join("file.bin").exists());
}

#[test]
fn a_proxy_is_used_through_connect_and_skipped_for_no_proxy_hosts() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(ok(b"through")));
    let proxy = ProxyServer::start(200);
    let tmp = tempfile::tempdir().unwrap();
    let env = |vars: Vec<(&'static str, String)>| move |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());
    let via = Proxy::from_vars(env(vec![("HTTPS_PROXY", format!("http://127.0.0.1:{}", proxy.port))])).unwrap();
    let done = get(&transport(via), &server.url("/a"), &tmp.path().join("a"), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"through");
    let asked = proxy.heads();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with(&format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n", server.port)), "{}", asked[0]);
    // the proxy saw only the tunnel request: no path, no User-Agent, no Range
    assert!(!asked[0].contains("/a") && !asked[0].contains("User-Agent"), "{}", asked[0]);
    assert_eq!(server.heads().len(), 1);

    // NO_PROXY names the host: straight to the server
    let bypass =
        Proxy::from_vars(env(vec![("HTTPS_PROXY", format!("127.0.0.1:{}", proxy.port)), ("NO_PROXY", "example.org, 127.0.0.1".into())])).unwrap();
    get(&transport(bypass), &server.url("/b"), &tmp.path().join("b"), &once()).unwrap();
    assert_eq!(proxy.heads().len(), 1);
    assert_eq!(server.heads().len(), 2);

    // the manual setting
    let manual = Proxy::manual("127.0.0.1", proxy.port).unwrap();
    get(&transport(manual), &server.url("/c"), &tmp.path().join("c"), &once()).unwrap();
    assert_eq!(proxy.heads().len(), 2);

    // a proxy asking for a password fails, saying so
    let locked = ProxyServer::start(407);
    let r = get(&transport(Proxy::manual("127.0.0.1", locked.port).unwrap()), &server.url("/d"), &tmp.path().join("d"), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Connect(m))) if m.contains("password")), "{r:?}");
    assert_eq!(server.heads().len(), 3);
}

#[test]
fn a_certificate_that_does_not_validate_fails() {
    let server = Server::start(LOCAL, LOCAL_KEY, |_, _| Answer::Send(ok(b"secret")));
    let tmp = tempfile::tempdir().unwrap();
    // the bundled web roots do not know the test root
    let bundled = HttpsTransport::new(Proxy::direct()).unwrap();
    let r = get(&bundled, &server.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Tls(m))) if m.contains("UnknownIssuer")), "{r:?}");
    // a certificate for another name, from a trusted root
    let other = Server::start(OTHER, OTHER_KEY, |_, _| Answer::Send(ok(b"secret")));
    let r = get(&transport(Proxy::direct()), &other.url("/f"), tmp.path(), &once());
    assert!(matches!(&r, Err(FetchError::Transport(TransportError::Tls(m))) if m.contains("not valid for name")), "{r:?}");
    // no request reached either server, and nothing was written
    assert!(server.heads().is_empty() && other.heads().is_empty());
    assert!(!tmp.path().join("file.bin").exists());
    // the same server, trusted, by name too
    let done = get(&transport(Proxy::direct()), &format!("https://localhost:{}/f", server.port), tmp.path(), &once()).unwrap();
    assert_eq!(std::fs::read(&done.path).unwrap(), b"secret");
}
