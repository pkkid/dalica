//! Where MCP tool calls end up: control-protocol methods (`control-protocol.md` §6), answered by
//! the in-process [`crate::Headless`] session today and by the running app in connect mode
//! (M10.17).
//!
//! After LightCraft `crates/mcp/src/backend.rs` (`Backend`, `ProgressHook`) @ b3d392b4, MIT OR
//! Apache-2.0 (`NOTICE`). Changed for Dalica: a failure carries the control protocol's error
//! `code` (and a destructive command's `prompt`) instead of a bare message, progress is a
//! [`Progress`] passed with each call rather than a hook stored in the backend, and rendering is a
//! method of its own.

use dalica_engine::CmdError;
use serde_json::{Map, Value, json};

/// A failed operation: the control protocol's error `code` (`control-protocol.md` §3) and its
/// message; for `needs_confirm`, the question the app would ask (`prompt`). Tools return it as a
/// result with `isError: true`, so the model can read and correct it (`mcp.md` §3).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
    pub prompt: Option<Value>,
}

impl ToolError {
    pub fn new(code: &str, message: impl Into<String>) -> ToolError {
        ToolError { code: code.to_string(), message: message.into(), prompt: None }
    }

    /// `{code, error, prompt?}`, as the control channel's error replies word it.
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("code".into(), json!(self.code));
        o.insert("error".into(), json!(self.message));
        if let Some(p) = &self.prompt {
            o.insert("prompt".into(), p.clone());
        }
        Value::Object(o)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<CmdError> for ToolError {
    fn from(e: CmdError) -> ToolError {
        ToolError::new(e.code(), e.to_string())
    }
}

/// Progress of a long call, reported between photos (`mcp.md` §6).
pub trait Progress {
    /// `done` of `total` so far, with what is happening ("Exporting 12 of 48"). Returns false
    /// when the request was cancelled: the caller stops before its next photo.
    fn report(&mut self, done: u64, total: u64, message: &str) -> bool;
}

/// Progress nobody listens to, and that is never cancelled ([`crate::Server::handle_line`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&mut self, _done: u64, _total: u64, _message: &str) -> bool {
        true
    }
}

/// A photo rendered with its edits, encoded (`render_preview`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub id: u64,
    pub bytes: Vec<u8>,
    /// `image/png`, or `image/jpeg` when the PNG would pass [`crate::MAX_PNG_BYTES`].
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    /// The recipe rendered, one operation string each (`data-model.md` §3.1).
    pub recipe: Vec<String>,
}

/// Something that answers control-protocol methods. Every tool goes through it, so the headless
/// session and the running app (connect mode) behave the same.
pub trait Backend {
    /// Calls one method: `engine.execute {command, params?, wait?}`, `engine.commands {filter?,
    /// enabledOnly?}`, `jobs.list`, `jobs.wait {job, timeoutMs?}`, `jobs.cancel {job?}`, … A
    /// command that starts a job reports its progress through `progress` while the call waits for
    /// it, and stops when `progress` says the request was cancelled.
    fn call(&mut self, method: &str, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError>;

    /// Renders photo `id` (`None`: the first photo of the Photo Tray) with its edits, its long
    /// side at most `max_side` pixels (never enlarged). Changes nothing in the library.
    fn render(&mut self, id: Option<u64>, max_side: u32) -> Result<Rendered, ToolError>;

    /// True when a window is attached (the UI tools of `mcp.md` §4.3 work).
    fn has_ui(&self) -> bool;

    /// A short description for the `instructions` string and the log ("headless, demo library").
    fn describe(&self) -> String;

    /// Called while the server waits for the next message (about every 50 ms), so background
    /// work (scans) moves on between calls.
    fn tick(&mut self) {}
}
