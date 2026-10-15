//! Progress and cancellation while a tool runs (`mcp.md` §6). The backend keeps its session on
//! the serving thread and calls [`Reporter::report`] between photos; each call reads what the
//! reader thread has received meanwhile:
//!
//! - `notifications/cancelled` for this request stops it (the caller cancels its job, which ends
//!   before its next photo) and suppresses its response;
//! - `ping` is answered at once;
//! - anything else waits, in order, until the tool has answered.
//!
//! With a `progressToken` in the request's `_meta`, `notifications/progress` reports `progress`
//! (photos done) of `total`, strictly increasing, at most ten per second plus the final one.
//!
//! After LightCraft `crates/mcp/src/progress.rs` (`progress_hook`) @ b3d392b4, MIT OR Apache-2.0
//! (`NOTICE`). Changed for Dalica: a value passed with the call instead of a hook stored in the
//! backend, any job's progress (not only exports), and the final notification sent whatever the
//! interval.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::backend::Progress;
use crate::jsonrpc::{Input, Wire, notification, response};

/// The shortest time between two notifications that are not the final one.
const INTERVAL: Duration = Duration::from_millis(100);

/// One request's progress.
pub(crate) struct Reporter<'w> {
    wire: &'w Wire,
    id: Value,
    token: Option<Value>,
    cancelled: bool,
    /// The input has ended or failed: nothing more to read.
    closed: bool,
    last: u64,
    at: Option<Instant>,
}

impl<'w> Reporter<'w> {
    /// Progress of request `id`, reported to `token` when there is one.
    pub(crate) fn new(wire: &'w Wire, id: Value, token: Option<Value>) -> Reporter<'w> {
        Reporter { wire, id, token, cancelled: false, closed: false, last: 0, at: None }
    }

    /// Whether the client cancelled the request.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Reads what has arrived: a cancel of this request, pings, and the rest kept for later.
    fn drain(&mut self) {
        while !self.closed {
            match self.wire.try_next() {
                None => break,
                Some(Input::Line(text)) => self.look(text),
                Some(end @ (Input::Eof | Input::Failed(_))) => {
                    // The input ended: the tool finishes, then the server stops.
                    self.wire.defer(end);
                    self.closed = true;
                }
                Some(other) => self.wire.defer(other),
            }
        }
    }

    fn look(&mut self, text: String) {
        let msg: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let id = msg.get("id");
        match msg.get("method").and_then(Value::as_str) {
            Some("notifications/cancelled") if id.is_none() && msg.pointer("/params/requestId") == Some(&self.id) => self.cancelled = true,
            Some("ping")
                if msg.get("jsonrpc") == Some(&json!("2.0"))
                    && id.is_some_and(|i| i.is_string() || i.is_number())
                    && msg.get("params").is_none_or(|p| p.is_null() || p.is_object()) =>
            {
                let _ = self.wire.send(response(id.cloned().unwrap_or(Value::Null), json!({})).to_string());
            }
            _ => self.wire.defer(Input::Line(text)),
        }
    }
}

impl Progress for Reporter<'_> {
    fn report(&mut self, done: u64, total: u64, message: &str) -> bool {
        self.drain();
        if self.cancelled {
            return false;
        }
        let due = done >= total || self.at.is_none_or(|at| at.elapsed() >= INTERVAL);
        if let Some(token) = &self.token
            && total > 0
            && done > self.last
            && due
        {
            let params = json!({ "progressToken": token, "progress": done, "total": total, "message": message });
            let _ = self.wire.send(notification("notifications/progress", params).to_string());
            self.last = done;
            self.at = Some(Instant::now());
        }
        true
    }
}
