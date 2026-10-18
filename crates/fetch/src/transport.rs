//! The [`Transport`]: a ranged GET answering a status, a length and a byte stream, chosen by the
//! caller (the HTTPS transport, M13.13; the platform's downloader; [`MemoryTransport`] in tests).
//! `fetch` builds every [`Request`] itself, so what is sent is the same whatever carries it: the
//! address, the first byte wanted and [`Request::headers`], nothing else.
//!
//! A transport enforces the connect and stall timeouts of [`Limits`] and follows redirects itself
//! (never from https to http); `fetch` checks the cancel flag between reads, and a transport that
//! may block wakes to check it too.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::spec::Url;

/// The User-Agent of every request: the program and its version, nothing about the user, the
/// machine or the system.
pub const USER_AGENT: &str = concat!("Dalica/", env!("CARGO_PKG_VERSION"));

/// The only header names a request carries (`security.md` §8.2: no cookie, token or personal
/// header). The transport adds what the protocol itself needs (`Host`, `Connection`).
pub const HEADER_NAMES: [&str; 4] = ["User-Agent", "Accept", "Accept-Encoding", "Range"];

/// One GET, made by `fetch` only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    url: Url,
    from: u64,
}

impl Request {
    pub(crate) fn new(url: Url, from: u64) -> Request {
        Request { url, from }
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The first byte wanted: 0 for the whole file, otherwise a resume (`Range: bytes=<from>-`).
    pub fn from(&self) -> u64 {
        self.from
    }

    /// Every header to send, in order: a generic User-Agent, `Accept: */*`, `Accept-Encoding:
    /// identity` (the bytes are hashed as they arrive) and, for a resume, `Range`.
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        let [ua, accept, encoding, range] = HEADER_NAMES;
        let mut headers = vec![(ua, USER_AGENT.to_string()), (accept, "*/*".to_string()), (encoding, "identity".to_string())];
        if self.from > 0 {
            headers.push((range, format!("bytes={}-", self.from)));
        }
        headers
    }
}

/// The limits a transport keeps: how long to wait for a connection and for the next bytes, and
/// the flag that cancels the download.
#[derive(Clone, Copy, Debug)]
pub struct Limits<'a> {
    pub connect: Duration,
    pub stall: Duration,
    pub cancel: &'a AtomicBool,
}

/// A server's answer: the status, the announced length of the body, the raw `Content-Range` of a
/// partial answer, and the body.
pub struct Reply {
    pub status: u16,
    /// `Content-Length`, when announced.
    pub length: Option<u64>,
    /// `Content-Range`, when present (`bytes <first>-<last>/<total or *>`).
    pub content_range: Option<String>,
    pub body: Box<dyn Body>,
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply").field("status", &self.status).field("length", &self.length).field("content_range", &self.content_range).finish()
    }
}

/// A reply's bytes.
pub trait Body {
    /// Reads the next bytes into `buf`; 0 at the end of the body. Waits at most `limits.stall`.
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError>;
}

/// Carries a [`Request`] to a server.
pub trait Transport {
    /// Sends `request` (exactly its address, range and headers) within `limits.connect` and
    /// answers the status line and headers; the body is read through the [`Reply`].
    fn get(&self, request: &Request, limits: &Limits<'_>) -> Result<Reply, TransportError>;
}

/// Why a transport failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// Name resolution or connecting failed, or took longer than the connect timeout.
    #[error("could not connect: {0}")]
    Connect(String),
    /// No bytes arrived within the stall timeout.
    #[error("the server stopped sending data")]
    Stalled,
    /// The cancel flag was raised while the transport waited.
    #[error("cancelled")]
    Cancelled,
    /// The secure connection failed (certificate, handshake).
    #[error("secure connection failed: {0}")]
    Tls(String),
    /// The server's answer could not be understood, or broke a rule (a redirect to http).
    #[error("unexpected answer: {0}")]
    Protocol(String),
    /// The connection broke.
    #[error("{0}")]
    Io(String),
}

impl TransportError {
    /// Whether trying again (resuming) may help.
    pub fn retryable(&self) -> bool {
        matches!(self, TransportError::Stalled | TransportError::Io(_) | TransportError::Protocol(_))
    }
}

/// How [`MemoryTransport`] answers one address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served {
    pub bytes: Vec<u8>,
    /// Answer this status with an empty body instead of the file.
    pub status: Option<u16>,
    /// Honour `Range` with a 206; otherwise always the whole file with a 200.
    pub ranges: bool,
    /// Announce `Content-Length`.
    pub announce: bool,
    /// Break the connection after this many body bytes of each answer.
    pub cut_after: Option<u64>,
    /// The most bytes one read hands out.
    pub chunk: usize,
}

impl Served {
    /// `bytes`, ranges honoured, the length announced, never cut, 4 KiB per read.
    pub fn new(bytes: Vec<u8>) -> Served {
        Served { bytes, status: None, ranges: true, announce: true, cut_after: None, chunk: 4096 }
    }
}

/// A request [`MemoryTransport`] received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The whole address.
    pub url: String,
    pub from: u64,
    pub headers: Vec<(String, String)>,
}

/// What [`MemoryTransport`] has counted.
#[derive(Debug, Default)]
struct Counts {
    requests: Vec<Seen>,
    reads: u64,
    bytes: u64,
}

/// An in-memory transport for tests: serves files registered with [`MemoryTransport::serve`],
/// records every request and counts the reads and bytes it hands out. An address it does not know
/// answers 404.
#[derive(Debug, Default)]
pub struct MemoryTransport {
    files: Mutex<HashMap<String, Served>>,
    counts: std::sync::Arc<Mutex<Counts>>,
}

impl MemoryTransport {
    pub fn new() -> MemoryTransport {
        MemoryTransport::default()
    }

    /// Serves `served` at `url` (replacing what was served there).
    pub fn serve(&self, url: &str, served: Served) {
        let key = Url::parse(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string());
        lock(&self.files).insert(key, served);
    }

    /// Every request so far, in order.
    pub fn requests(&self) -> Vec<Seen> {
        lock(&self.counts).requests.clone()
    }

    /// Body reads answered so far (ends of bodies included).
    pub fn reads(&self) -> u64 {
        lock(&self.counts).reads
    }

    /// Body bytes handed out so far.
    pub fn bytes(&self) -> u64 {
        lock(&self.counts).bytes
    }
}

/// A lock that survives a panicked holder (the data is test bookkeeping, still usable).
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Transport for MemoryTransport {
    fn get(&self, request: &Request, limits: &Limits<'_>) -> Result<Reply, TransportError> {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        let url = request.url().to_string();
        let headers = request.headers().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        lock(&self.counts).requests.push(Seen { url: url.clone(), from: request.from(), headers });
        let served = lock(&self.files).get(&url).cloned();
        let empty = |status: u16| Reply { status, length: Some(0), content_range: None, body: Box::new(MemoryBody::empty(self.counts.clone())) };
        let Some(served) = served else { return Ok(empty(404)) };
        if let Some(status) = served.status {
            return Ok(empty(status));
        }
        let total = u64::try_from(served.bytes.len()).unwrap_or(u64::MAX);
        let from = request.from();
        let (status, start, content_range) = if from > 0 && served.ranges {
            if from >= total {
                return Ok(Reply {
                    status: 416,
                    length: Some(0),
                    content_range: Some(format!("bytes */{total}")),
                    body: Box::new(MemoryBody::empty(self.counts.clone())),
                });
            }
            (206, from, Some(format!("bytes {from}-{}/{total}", total.saturating_sub(1))))
        } else {
            (200, 0, None)
        };
        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let bytes = served.bytes.get(start..).unwrap_or_default().to_vec();
        let length = served.announce.then(|| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let body = MemoryBody { bytes, pos: 0, cut_after: served.cut_after, chunk: served.chunk.max(1), counts: self.counts.clone() };
        Ok(Reply { status, length, content_range, body: Box::new(body) })
    }
}

struct MemoryBody {
    bytes: Vec<u8>,
    pos: usize,
    cut_after: Option<u64>,
    chunk: usize,
    counts: std::sync::Arc<Mutex<Counts>>,
}

impl MemoryBody {
    fn empty(counts: std::sync::Arc<Mutex<Counts>>) -> MemoryBody {
        MemoryBody { bytes: Vec::new(), pos: 0, cut_after: None, chunk: 1, counts }
    }
}

impl Body for MemoryBody {
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        if self.cut_after.is_some_and(|cut| u64::try_from(self.pos).unwrap_or(u64::MAX) >= cut) {
            return Err(TransportError::Io("the connection was reset".into()));
        }
        let mut n = buf.len().min(self.chunk).min(self.bytes.len().saturating_sub(self.pos));
        if let Some(cut) = self.cut_after {
            let left = usize::try_from(cut).unwrap_or(usize::MAX).saturating_sub(self.pos);
            n = n.min(left);
        }
        let src = self.bytes.get(self.pos..self.pos.saturating_add(n)).unwrap_or_default();
        for (d, s) in buf.iter_mut().zip(src) {
            *d = *s;
        }
        let n = src.len();
        self.pos = self.pos.saturating_add(n);
        let mut counts = lock(&self.counts);
        counts.reads = counts.reads.saturating_add(1);
        counts.bytes = counts.bytes.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    }
}
