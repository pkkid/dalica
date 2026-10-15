//! JSON-RPC 2.0 over stdio, newline-delimited (`mcp.md` §3, §10): the message shapes, the error
//! codes, and the transport, a reader thread and a writer thread around the thread that owns the
//! backend, so `ping` and `notifications/cancelled` are read while a tool runs
//! ([`crate::progress`]).
//!
//! After LightCraft `crates/mcp/src/progress.rs` (`Wire`, `serve_wire`) and `server.rs`
//! (`response`, `error`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: lines are
//! read with a 1 MiB cap (an over-long line is skipped and answered `-32700`, and the next line is
//! read), the backend has a tick while no message comes, and the reader thread is not joined, so
//! a client that stops reading never blocks the server's exit.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::server::Server;

/// The longest input line, the newline included (as the control channel's, `control-protocol.md`
/// §4).
pub const MAX_LINE: usize = 1 << 20;
/// The largest `tools/call` result, encoded; a larger one becomes a `limit` error (`mcp.md` §10).
pub const MAX_RESULT: usize = 8 << 20;

pub(crate) const PARSE_ERROR: i64 = -32700;
pub(crate) const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const INTERNAL_ERROR: i64 = -32603;
pub(crate) const RESOURCE_NOT_FOUND: i64 = -32002;

/// How long the server waits for a message before it gives the backend a tick.
const TICK: Duration = Duration::from_millis(50);

pub(crate) fn response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub(crate) fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// What the reader thread read.
#[derive(Debug)]
pub(crate) enum Input {
    Line(String),
    /// A line over [`MAX_LINE`] (skipped to its newline).
    TooLong,
    NotUtf8,
    /// The input closed.
    Eof,
    Failed(std::io::Error),
}

/// Reads one line of at most `max` bytes with its newline; `None` at the end of the input. A
/// longer line is consumed to its newline and answered [`Input::TooLong`], so the next line is
/// read as usual.
pub(crate) fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Input>> {
    let mut buf = Vec::new();
    let mut over = false;
    loop {
        let avail = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if avail.is_empty() {
            if buf.is_empty() && !over {
                return Ok(None);
            }
            break;
        }
        let (take, found) = match avail.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (avail.len(), false),
        };
        if !over {
            // The newline counts towards the limit.
            if buf.len().saturating_add(take).saturating_add(usize::from(found)) > max {
                over = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(avail.get(..take).unwrap_or_default());
            }
        }
        r.consume(take.saturating_add(usize::from(found)));
        if found {
            break;
        }
    }
    if over {
        return Ok(Some(Input::TooLong));
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(Some(String::from_utf8(buf).map_or(Input::NotUtf8, Input::Line)))
}

/// The serving thread's ends of the transport.
pub(crate) struct Wire {
    inbox: Receiver<Input>,
    out: Sender<String>,
    /// Messages read while a tool ran that are not for it: served next, in order.
    deferred: RefCell<VecDeque<Input>>,
}

/// What [`Wire::next`] gives.
pub(crate) enum Next {
    Input(Input),
    /// Nothing came for a while.
    Idle,
}

impl Wire {
    pub(crate) fn new(inbox: Receiver<Input>, out: Sender<String>) -> Wire {
        Wire { inbox, out, deferred: RefCell::new(VecDeque::new()) }
    }

    /// The next message: a deferred one first, else one from the reader, waiting up to `wait`.
    pub(crate) fn next(&self, wait: Duration) -> Next {
        if let Some(i) = self.deferred.borrow_mut().pop_front() {
            return Next::Input(i);
        }
        match self.inbox.recv_timeout(wait) {
            Ok(i) => Next::Input(i),
            Err(RecvTimeoutError::Timeout) => Next::Idle,
            Err(RecvTimeoutError::Disconnected) => Next::Input(Input::Eof),
        }
    }

    /// A message the reader has already read, without waiting (not the deferred ones).
    pub(crate) fn try_next(&self) -> Option<Input> {
        match self.inbox.try_recv() {
            Ok(i) => Some(i),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Input::Eof),
        }
    }

    /// Keeps `input` for after the running tool.
    pub(crate) fn defer(&self, input: Input) {
        self.deferred.borrow_mut().push_back(input);
    }

    /// Writes one line. `Err` when the output has closed.
    pub(crate) fn send(&self, line: String) -> Result<(), ()> {
        self.out.send(line).map_err(|_| ())
    }
}

/// Serves `input` until it closes: a reader thread splits it into lines, this thread answers them
/// in order through `server`, and a writer thread writes each reply line to `output`.
pub(crate) fn serve(server: &mut Server, input: impl Read + Send + 'static, output: impl Write + Send) -> std::io::Result<()> {
    std::thread::scope(|scope| {
        let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
        let writer = std::thread::Builder::new().name("dalica-mcp-output".into()).spawn_scoped(scope, move || -> std::io::Result<()> {
            let mut output = output;
            for line in out_rx {
                output.write_all(line.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
            Ok(())
        })?;
        let (in_tx, in_rx) = std::sync::mpsc::channel::<Input>();
        // Not joined: a read blocked on a client that keeps its end open must not keep us here.
        let reader = std::thread::Builder::new().name("dalica-mcp-input".into()).spawn(move || {
            let mut input = BufReader::new(input);
            loop {
                let (next, last) = match read_line(&mut input, MAX_LINE) {
                    Ok(Some(i)) => (i, false),
                    Ok(None) => (Input::Eof, true),
                    Err(e) => (Input::Failed(e), true),
                };
                if in_tx.send(next).is_err() || last {
                    return;
                }
            }
        });
        if let Err(e) = reader {
            drop(out_tx);
            let _ = writer.join();
            return Err(e);
        }
        let wire = Wire::new(in_rx, out_tx);
        let served = loop {
            match wire.next(TICK) {
                Next::Idle => server.tick(),
                Next::Input(Input::Eof) => break Ok(()),
                Next::Input(Input::Failed(e)) => break Err(e),
                Next::Input(input) => {
                    if let Some(reply) = server.handle_input(input, Some(&wire))
                        && wire.send(reply).is_err()
                    {
                        break Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the MCP output closed"));
                    }
                }
            }
        };
        drop(wire);
        let written = writer.join().map_err(|_| std::io::Error::other("the MCP output thread panicked")).and_then(|r| r);
        served.and(written)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[u8], max: usize) -> Vec<String> {
        let mut r = std::io::BufReader::with_capacity(4, text);
        let mut out = Vec::new();
        while let Some(i) = read_line(&mut r, max).unwrap() {
            out.push(match i {
                Input::Line(l) => l,
                Input::TooLong => "<long>".into(),
                Input::NotUtf8 => "<utf8>".into(),
                other => format!("{other:?}"),
            });
        }
        out
    }

    /// The cap counts the newline; a long line is skipped whole and the next is read; CRLF and a
    /// last line without a newline are lines too.
    #[test]
    fn lines_are_capped_and_the_next_one_is_read() {
        assert_eq!(lines(b"abc\nabcd\nab\r\n\xff\nlast", 4), ["abc", "<long>", "ab", "<utf8>", "last"]);
        assert_eq!(lines(b"abcdefghij\nok\n", 4), ["<long>", "ok"]);
        assert_eq!(lines(b"abcdefghij", 4), ["<long>"]);
        assert!(lines(b"", 4).is_empty());
    }
}
