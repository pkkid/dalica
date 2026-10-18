//! Reading a server's reply: the status line and headers (capped), then the body as its length,
//! chunks or the end of the connection say, from any [`Wire`] (a secure connection, or memory for
//! the fuzz target).
//!
//! Ported from LightCraft `crates/fetch/src/http.rs` (`Conn`, `Response`, the head of `get`) at
//! f34c7a8a (MIT OR Apache-2.0), with the caps made constants, interim `1xx` heads skipped, and a
//! compressed or otherwise transformed body refused (every byte is hashed as it arrives).

use crate::transport::{Limits, TransportError};

/// The most bytes of the status line and headers together.
pub const MAX_HEAD: usize = 64 * 1024;
/// The most header lines in one head.
pub const MAX_HEADERS: usize = 100;
/// The most trailer lines after a chunked body.
const MAX_TRAILERS: usize = 100;
/// The longest chunk-size line.
const MAX_CHUNK_LINE: usize = 1024;
/// The most interim (`1xx`) heads before the real one.
const MAX_INTERIM: usize = 4;
/// The most bytes one read from the wire asks for.
const READ_CHUNK: usize = 64 * 1024;

/// Where a reply's bytes come from.
pub(crate) trait Wire {
    /// The next bytes (0 at the end of the stream), waiting at most `limits.stall`.
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError>;
}

fn protocol(why: &str) -> TransportError {
    TransportError::Protocol(why.to_string())
}

/// A wire with a read buffer (heads and chunk sizes are read line by line).
pub(crate) struct Conn<W> {
    wire: W,
    buf: Vec<u8>,
    pos: usize,
}

impl<W: Wire> Conn<W> {
    pub(crate) fn new(wire: W) -> Conn<W> {
        Conn { wire, buf: Vec::new(), pos: 0 }
    }

    /// Appends the next bytes of the wire to the buffer; 0 at the end of the stream.
    fn fill(&mut self, limits: &Limits<'_>) -> Result<usize, TransportError> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        } else if self.pos > 0 {
            // keep what is unread, drop what was used
            self.buf.drain(..self.pos.min(self.buf.len()));
            self.pos = 0;
        }
        let old = self.buf.len();
        self.buf.resize(old.saturating_add(READ_CHUNK), 0);
        let n = match self.buf.get_mut(old..) {
            Some(spare) => self.wire.read(spare, limits),
            None => Ok(0),
        };
        let n = n.inspect_err(|_| self.buf.truncate(old))?;
        self.buf.truncate(old.saturating_add(n));
        Ok(n)
    }

    /// One line without its CRLF (or LF); at most `max` bytes.
    fn line(&mut self, max: usize, limits: &Limits<'_>) -> Result<String, TransportError> {
        loop {
            let avail = self.buf.get(self.pos..).unwrap_or_default();
            if let Some(i) = avail.iter().position(|b| *b == b'\n') {
                if i > max {
                    return Err(protocol("a line of the reply is too long"));
                }
                let line = avail.get(..i).unwrap_or_default();
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                let s = String::from_utf8_lossy(line).into_owned();
                self.pos = self.pos.saturating_add(i).saturating_add(1);
                return Ok(s);
            }
            if avail.len() > max {
                return Err(protocol("a line of the reply is too long"));
            }
            if self.fill(limits)? == 0 {
                return Err(protocol("the connection closed in the middle of the reply"));
            }
        }
    }

    /// Up to `out.len()` body bytes (0 at the end of the stream).
    fn read(&mut self, out: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        if self.pos >= self.buf.len() && self.fill(limits)? == 0 {
            return Ok(0);
        }
        let avail = self.buf.get(self.pos..).unwrap_or_default();
        let n = avail.len().min(out.len());
        if let (Some(dst), Some(src)) = (out.get_mut(..n), avail.get(..n)) {
            dst.copy_from_slice(src);
        }
        self.pos = self.pos.saturating_add(n);
        Ok(n)
    }
}

/// How the body's end is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyMode {
    /// `Content-Length`: this many bytes are left.
    Length(u64),
    /// `Transfer-Encoding: chunked`: `left` bytes of the current chunk.
    Chunked { left: u64, done: bool },
    /// Until the server closes the connection.
    Close,
    /// No body, or its end was read.
    Done,
}

/// A reply's status, headers and body.
pub(crate) struct Response<W> {
    pub(crate) status: u16,
    headers: Vec<(String, String)>,
    conn: Conn<W>,
    body: BodyMode,
}

/// Reads the status line and headers from `conn`; interim `1xx` heads are skipped.
pub(crate) fn read_head<W: Wire>(mut conn: Conn<W>, limits: &Limits<'_>) -> Result<Response<W>, TransportError> {
    let mut interim = 0;
    let (status, headers) = loop {
        let (status, headers) = head(&mut conn, limits)?;
        if !(100..200).contains(&status) {
            break (status, headers);
        }
        if status == 101 {
            return Err(protocol("the server switched protocols"));
        }
        interim += 1;
        if interim > MAX_INTERIM {
            return Err(protocol("too many interim replies"));
        }
    };
    let find = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    if let Some(enc) = find("content-encoding")
        && !enc.trim().eq_ignore_ascii_case("identity")
    {
        return Err(protocol("the server compressed the file although asked not to"));
    }
    let lengths: Vec<&str> = headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim()).collect();
    let body = if status == 204 || status == 304 {
        BodyMode::Done
    } else if let Some(te) = find("transfer-encoding") {
        // only `chunked` (the last coding) is understood; anything else would transform the bytes
        let codings: Vec<String> = te.split(',').map(|c| c.trim().to_ascii_lowercase()).filter(|c| !c.is_empty()).collect();
        if codings.iter().all(|c| c == "identity" || c == "chunked") && codings.last().is_some_and(|c| c == "chunked") {
            BodyMode::Chunked { left: 0, done: false }
        } else {
            return Err(protocol("the server sent the file in a transfer coding Dalica does not read"));
        }
    } else if let Some(first) = lengths.first() {
        if lengths.iter().any(|l| l != first) {
            return Err(protocol("the reply announces two different lengths"));
        }
        let n = first.parse::<u64>().ok().filter(|_| first.bytes().all(|b| b.is_ascii_digit()));
        BodyMode::Length(n.ok_or_else(|| protocol("bad Content-Length"))?)
    } else {
        BodyMode::Close
    };
    Ok(Response { status, headers, conn, body })
}

/// One head: the status and the headers, at most [`MAX_HEAD`] bytes and [`MAX_HEADERS`] lines.
fn head<W: Wire>(conn: &mut Conn<W>, limits: &Limits<'_>) -> Result<(u16, Vec<(String, String)>), TransportError> {
    let status_line = conn.line(MAX_HEAD, limits)?;
    let mut parts = status_line.split(' ');
    let version = parts.next().unwrap_or_default();
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(protocol("not an HTTP/1.x reply"));
    }
    let code = parts.next().unwrap_or_default();
    let status = (code.len() == 3 && code.bytes().all(|b| b.is_ascii_digit()))
        .then(|| code.parse::<u16>().ok())
        .flatten()
        .filter(|s| (100..=599).contains(s))
        .ok_or_else(|| protocol("bad status line"))?;
    let mut headers = Vec::new();
    let mut total = status_line.len();
    loop {
        let line = conn.line(MAX_HEAD.saturating_sub(total), limits)?;
        if line.is_empty() {
            break;
        }
        total = total.saturating_add(line.len()).saturating_add(2);
        if total > MAX_HEAD || headers.len() >= MAX_HEADERS {
            return Err(protocol("the reply's headers are too large"));
        }
        if line.starts_with([' ', '\t']) {
            return Err(protocol("folded header lines are not accepted"));
        }
        let (k, v) = line.split_once(':').ok_or_else(|| protocol("a header line without a colon"))?;
        if k.is_empty() || k.contains([' ', '\t']) {
            return Err(protocol("a bad header name"));
        }
        headers.push((k.to_string(), v.trim().to_string()));
    }
    Ok((status, headers))
}

impl<W: Wire> Response<W> {
    /// The first header named `name` (case-insensitive).
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The body's length when the server announced it.
    pub(crate) fn content_length(&self) -> Option<u64> {
        match self.body {
            BodyMode::Length(n) => Some(n),
            BodyMode::Done => Some(0),
            _ => None,
        }
    }

    /// Reads body bytes into `out` (0 at the end of the body).
    pub(crate) fn read(&mut self, out: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        if out.is_empty() {
            return Ok(0);
        }
        match &mut self.body {
            BodyMode::Done => Ok(0),
            BodyMode::Length(left) => {
                if *left == 0 {
                    self.body = BodyMode::Done;
                    return Ok(0);
                }
                let want = usize::try_from(*left).unwrap_or(usize::MAX).min(out.len());
                let n = self.conn.read(out.get_mut(..want).unwrap_or_default(), limits)?;
                if n == 0 {
                    return Err(TransportError::Io("the connection closed before the end of the file".into()));
                }
                *left = left.saturating_sub(u64::try_from(n).unwrap_or(u64::MAX));
                Ok(n)
            }
            BodyMode::Close => {
                let n = self.conn.read(out, limits)?;
                if n == 0 {
                    self.body = BodyMode::Done;
                }
                Ok(n)
            }
            BodyMode::Chunked { left, done } => {
                if *done {
                    return Ok(0);
                }
                if *left == 0 {
                    let line = self.conn.line(MAX_CHUNK_LINE, limits)?;
                    let hex = line.split(';').next().unwrap_or_default().trim();
                    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(protocol("bad chunk size"));
                    }
                    let size = u64::from_str_radix(hex, 16).map_err(|_| protocol("bad chunk size"))?;
                    if size == 0 {
                        // trailers, up to the empty line
                        let mut n = 0;
                        while !self.conn.line(MAX_HEAD, limits)?.is_empty() {
                            n += 1;
                            if n > MAX_TRAILERS {
                                return Err(protocol("too many trailers"));
                            }
                        }
                        *done = true;
                        return Ok(0);
                    }
                    *left = size;
                }
                let want = usize::try_from(*left).unwrap_or(usize::MAX).min(out.len());
                let n = self.conn.read(out.get_mut(..want).unwrap_or_default(), limits)?;
                if n == 0 {
                    return Err(TransportError::Io("the connection closed in the middle of a chunk".into()));
                }
                *left = left.saturating_sub(u64::try_from(n).unwrap_or(u64::MAX));
                if *left == 0 && !self.conn.line(16, limits)?.is_empty() {
                    return Err(protocol("missing CRLF after a chunk"));
                }
                Ok(n)
            }
        }
    }
}

/// Bytes in memory as a wire, handed out `chunk` bytes at a time.
pub(crate) struct Memory<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) chunk: usize,
}

impl Wire for Memory<'_> {
    fn read(&mut self, buf: &mut [u8], _limits: &Limits<'_>) -> Result<usize, TransportError> {
        let n = buf.len().min(self.chunk.max(1)).min(self.data.len());
        let (head, rest) = self.data.split_at(n);
        if let Some(dst) = buf.get_mut(..n) {
            dst.copy_from_slice(head);
        }
        self.data = rest;
        Ok(n)
    }
}
