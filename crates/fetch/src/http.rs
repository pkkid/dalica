//! The HTTPS transport (`security.md` §8.2, reserved decision 9): an HTTP/1.1 ranged GET over
//! `std::net` and rustls with the RustCrypto provider (`rustls-rustcrypto`; no `ring` or
//! `aws-lc-rs`, so nothing is compiled from C or assembly), trusting Mozilla's root certificates
//! compiled in (`webpki-roots`). Certificate validation is always on; there is no setting to turn
//! it off.
//!
//! Only what a download needs: one request per connection (`Connection: close`), exactly the
//! headers `fetch` built ([`Request::headers`]) plus `Host` and `Connection`; `Content-Length`,
//! chunked and read-to-close bodies; redirects followed here, at most [`MAX_REDIRECTS`], only to
//! `https://` addresses (a redirect to plain http is refused, however it is written). The status
//! line and headers are capped ([`MAX_HEAD`], [`MAX_HEADERS`]); `fetch` caps the body at the
//! spec's limit. Every wait checks the cancel flag at least four times a second and no read waits
//! longer than the stall timeout. A proxy ([`Proxy`]) is reached with `CONNECT` only.
//!
//! Ported from LightCraft `crates/fetch/src/http.rs` (`get`, `Url::join`) at f34c7a8a (MIT OR
//! Apache-2.0); its connection and reply reading are in `http/wire.rs` and `http/response.rs`.

mod response;
mod wire;

use std::sync::Arc;

pub use response::{MAX_HEAD, MAX_HEADERS};

use crate::proxy::Proxy;
use crate::spec::Url;
use crate::transport::{Body, Limits, Reply, Request, Transport, TransportError};
use response::{Conn, Memory, Response};
use wire::Tls;

/// Redirects followed for one request.
pub const MAX_REDIRECTS: usize = 8;

/// The HTTPS [`Transport`].
#[derive(Clone)]
pub struct HttpsTransport {
    config: Arc<rustls::ClientConfig>,
    proxy: Proxy,
}

impl std::fmt::Debug for HttpsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpsTransport").field("proxy", &self.proxy).finish_non_exhaustive()
    }
}

impl HttpsTransport {
    /// The transport trusting the bundled web roots, connecting through `proxy` where it applies.
    pub fn new(proxy: Proxy) -> Result<HttpsTransport, TransportError> {
        Ok(HttpsTransport { config: wire::tls_config(None)?, proxy })
    }

    /// The transport trusting exactly `roots` (DER certificates) instead of the bundled ones: a
    /// local test server's own root. Validation stays on; a certificate that is not one of
    /// `roots`' or not for the host still fails.
    pub fn with_roots(proxy: Proxy, roots: &[Vec<u8>]) -> Result<HttpsTransport, TransportError> {
        Ok(HttpsTransport { config: wire::tls_config(Some(roots))?, proxy })
    }

    /// One exchange: connect, send `request`, read the head.
    fn exchange(&self, url: &Url, request: &Request, limits: &Limits<'_>) -> Result<Response<Tls>, TransportError> {
        let mut head = format!("GET {} HTTP/1.1\r\nHost: {}\r\n", url.path(), url.authority());
        for (k, v) in request.headers() {
            if v.contains(['\r', '\n']) {
                return Err(TransportError::Protocol(format!("the {k} header has a line break")));
            }
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        let mut tls = Tls::open(url, &self.proxy, &self.config, limits)?;
        tls.write_all(head.as_bytes(), limits)?;
        response::read_head(Conn::new(tls), limits)
    }
}

impl Transport for HttpsTransport {
    fn get(&self, request: &Request, limits: &Limits<'_>) -> Result<Reply, TransportError> {
        let mut url = request.url().clone();
        let mut redirects = 0;
        loop {
            let response = self.exchange(&url, request, limits)?;
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                redirects += 1;
                if redirects > MAX_REDIRECTS {
                    return Err(TransportError::Protocol("too many redirects".into()));
                }
                let location = response.header("location").ok_or_else(|| TransportError::Protocol("a redirect without a Location".into()))?;
                url = redirect_target(&url, location)?;
                // the redirect's connection closes here, its body unread
                continue;
            }
            let length = response.content_length();
            let content_range = response.header("content-range").map(str::to_string);
            return Ok(Reply { status: response.status, length, content_range, body: Box::new(HttpBody(response)) });
        }
    }
}

/// A reply's body, read from its connection.
struct HttpBody(Response<Tls>);

impl Body for HttpBody {
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        self.0.read(buf, limits)
    }
}

/// Where a redirect from `from` to `location` (a `Location` header) leads: an absolute https
/// address, a scheme-relative (`//host/path`), absolute-path (`/path`) or relative (`file`) one.
/// A redirect to plain http, or to any other scheme, is refused.
pub fn redirect_target(from: &Url, location: &str) -> Result<Url, TransportError> {
    let refused = |why: String| TransportError::Protocol(why);
    let l = location.trim();
    let scheme =
        l.split_once(':').map(|(s, _)| s).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')));
    let joined = match scheme {
        Some(s) if s.eq_ignore_ascii_case("https") => l.to_string(),
        Some(s) if s.eq_ignore_ascii_case("http") => return Err(refused("a redirect from https to http was refused".into())),
        Some(s) => return Err(refused(format!("a redirect to an unsupported address `{}:`", s.chars().take(16).collect::<String>()))),
        None if l.starts_with("//") => format!("https:{l}"),
        None if l.starts_with('/') => format!("https://{}{l}", from.authority()),
        None => {
            // relative to the current directory
            let path = from.path().split('?').next().unwrap_or("/");
            let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
            format!("https://{}{dir}/{l}", from.authority())
        }
    };
    Url::parse(&joined).map_err(|e| refused(format!("a redirect to an unusable address: {e}")))
}

/// A whole reply read from `bytes` as the transport reads one from a server, `chunk` bytes per
/// read (for the fuzz target `response`, `security.md` §7.1). The body is read to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadReply {
    pub status: u16,
    pub length: Option<u64>,
    pub content_range: Option<String>,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// Reads [`ReadReply`] from `bytes`: an error for anything the transport would refuse.
pub fn read_reply(bytes: &[u8], chunk: usize) -> Result<ReadReply, TransportError> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let limits = Limits { connect: std::time::Duration::ZERO, stall: std::time::Duration::ZERO, cancel: &cancel };
    let mut r = response::read_head(Conn::new(Memory { data: bytes, chunk }), &limits)?;
    let length = r.content_length();
    let mut body = Vec::new();
    let mut buf = vec![0u8; chunk.clamp(1, 64 * 1024)];
    loop {
        let n = r.read(&mut buf, &limits)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(buf.get(..n).unwrap_or_default());
        if body.len() > bytes.len() {
            return Err(TransportError::Protocol("a body longer than the reply".into()));
        }
    }
    let content_range = r.header("content-range").map(str::to_string);
    let location = r.header("location").map(str::to_string);
    Ok(ReadReply { status: r.status, length, content_range, location, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_redirects() {
        let u = Url::parse("https://a.example/x/y/file?q").unwrap();
        assert_eq!(redirect_target(&u, "https://b.example/z").unwrap().host(), "b.example");
        assert_eq!(redirect_target(&u, "HTTPS://b.example:8443/z").unwrap().port(), 8443);
        assert_eq!(redirect_target(&u, "/root").unwrap().path(), "/root");
        assert_eq!(redirect_target(&u, "other").unwrap().path(), "/x/y/other");
        assert_eq!(redirect_target(&u, "//c.example/p").unwrap().host(), "c.example");
        let on_port = Url::parse("https://a.example:8443/x").unwrap();
        assert_eq!(redirect_target(&on_port, "/y").unwrap().to_string(), "https://a.example:8443/y");
        // no downgrade from https to http, however the redirect is written; no other scheme
        for bad in
            ["http://a.example/z", "HTTP://a.example/z", "http:/a.example", "ftp://nope", "file:///etc/passwd", "data:,x", "https://u:p@h/", "/a b"]
        {
            assert!(redirect_target(&u, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reads_replies() {
        let r = read_reply(b"HTTP/1.1 206 Partial\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\n\r\nabcIGNORED", 2).unwrap();
        assert_eq!((r.status, r.length, r.content_range.as_deref(), r.body.as_slice()), (206, Some(3), Some("bytes 2-4/5"), b"abc".as_slice()));
        let r = read_reply(
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;x\r\nabc\r\n2\r\nde\r\n0\r\nT: 1\r\n\r\n",
            1,
        )
        .unwrap();
        assert_eq!((r.status, r.length, r.body.as_slice()), (200, None, b"abcde".as_slice()));
        let r = read_reply(b"HTTP/1.0 200 OK\nX: y\n\nto the end", 64).unwrap();
        assert_eq!((r.length, r.body.as_slice()), (None, b"to the end".as_slice()));
        let r = read_reply(b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n", 7).unwrap();
        assert_eq!(r.location.as_deref(), Some("/next"));
        for bad in [
            &b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort"[..],
            b"HTTP/2 200\r\n\r\n",
            b"HTTP/1.1 2000 OK\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: -1\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab",
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcX",
            b"HTTP/1.1 200 OK\r\nno colon\r\n\r\n",
            b"HTTP/1.1 200 OK\r\n folded\r\n\r\n",
            b"HTTP/1.1 101 Switching\r\n\r\n",
            b"HTTP/1.1 200 OK\r\n",
        ] {
            assert!(read_reply(bad, 3).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        let mut big = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..=MAX_HEADERS {
            big.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
        }
        big.extend_from_slice(b"\r\n");
        assert!(read_reply(&big, 4096).is_err());
    }
}
