//! The MCP methods over JSON-RPC 2.0 (`mcp.md` §3): `initialize`, `ping`, `tools/list`,
//! `tools/call`, `resources/list`, `resources/templates/list` and `resources/read`, and the
//! notifications `initialized` and `cancelled`.
//!
//! - **Protocol errors** use the standard codes: `-32700` for a line that is not JSON, over 1 MiB
//!   or not UTF-8 (with `id: null`; the next line is still read), `-32600` for a message that is
//!   not a request, `-32601` for an unknown method, `-32602` for bad params (a helper's unknown
//!   argument names it and the accepted ones; an unknown tool), `-32002` for an unknown resource.
//! - **Failures of the operation** are tool results with `isError: true` and the control
//!   protocol's `code` and message, so the model can read and correct them; a panic in a tool is
//!   one with `code: "internal"`, and the session keeps serving (`AGENTS.md` §3).
//! - A result over 8 MiB encoded becomes a `limit` error.
//!
//! After LightCraft `crates/mcp/src/server.rs` (`Server`, `handle_line`, `handle`, `request`,
//! `guarded`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: the method table of
//! `mcp.md`, strict `jsonrpc: "2.0"`, a helper's arguments checked against its whole schema, the
//! panic guard naming `internal`, the result cap, and the transport and progress in modules of
//! their own.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

use crate::backend::{Backend, NoProgress, Progress, ToolError};
use crate::jsonrpc::{
    self, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, Input, METHOD_NOT_FOUND, PARSE_ERROR, RESOURCE_NOT_FOUND, Wire, error, response,
};
use crate::progress::Reporter;
use crate::tools::{self, ToolResult};
use crate::{MAX_LINE, MAX_RESULT};

/// The MCP revision Dalica implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// The revisions it speaks; a client asking for another gets [`PROTOCOL_VERSION`].
pub const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Dalica is a folder-based photo organizer with simple, non-destructive editing. \
Photos, folders and jobs have integer ids; albums have short hex ids (a91c0e); a light-table section is named like folder:3 or album:a91c0e. \
Everything is a command: command_list gives every id with its params doc, command_run {id, params} runs one (one undo step each), command_batch runs several in order. \
Photo commands run without `ids` act on the Photo Tray (the held photos plus the selection), as the bottom-bar buttons do. \
Commands that ask first in the app (delete, revert, remove a folder, rebuild) refuse until their params carry \"confirm\": true; deletes go to the OS trash. \
Long commands (export, save, scans) run as jobs: command_run waits for them and reports progress when the request has a progress token; cancelling the request stops the job before its next photo, and the photos done stay done. \
render_preview shows a photo with its edits and changes nothing. Failures come back as results with isError and a code (not_found, disabled, bad_params, needs_confirm, path_denied, limit, unsaved, internal).";

/// What one request answers.
enum Reply {
    Ok(Value),
    Err(i64, String),
    /// Cancelled while it ran: no response at all (`mcp.md` §6).
    Suppressed,
}

/// An MCP server bound to one backend.
pub struct Server {
    backend: Box<dyn Backend>,
    compact: bool,
    initialized: bool,
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Server {
        Server { backend, compact: false, initialized: false }
    }

    /// `--compact`: `tools/list` gives the helper tools only, not one tool per command
    /// (`mcp.md` §4.4); `command_run` still reaches every command.
    pub fn with_compact(mut self, compact: bool) -> Server {
        self.compact = compact;
        self
    }

    pub fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serves newline-delimited JSON-RPC from `input` until it closes, replies to `output`.
    /// Logs go to standard error only: `output` carries protocol messages and nothing else.
    pub fn serve(&mut self, input: impl std::io::Read + Send + 'static, output: impl std::io::Write + Send) -> std::io::Result<()> {
        jsonrpc::serve(self, input, output)
    }

    /// Handles one line without the transport (no progress, no cancellation); the reply line, or
    /// `None` for a notification, a response or a blank line.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() >= MAX_LINE {
            return Some(error(Value::Null, PARSE_ERROR, format!("the line is longer than {MAX_LINE} bytes")).to_string());
        }
        self.handle_input(Input::Line(line.to_string()), None)
    }

    /// Lets the backend's background work move on while no message comes.
    pub(crate) fn tick(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.backend.tick()));
    }

    pub(crate) fn handle_input(&mut self, input: Input, wire: Option<&Wire>) -> Option<String> {
        let line = match input {
            Input::Line(l) => l,
            Input::TooLong => return Some(error(Value::Null, PARSE_ERROR, format!("the line is longer than {MAX_LINE} bytes")).to_string()),
            Input::NotUtf8 => return Some(error(Value::Null, PARSE_ERROR, "the line is not UTF-8").to_string()),
            Input::Eof | Input::Failed(_) => return None,
        };
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                // Batches left the protocol in 2025-06-18; older clients still get an answer.
                if batch.is_empty() {
                    Some(error(Value::Null, INVALID_REQUEST, "an empty batch"))
                } else {
                    let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m, wire)).collect();
                    (!replies.is_empty()).then_some(Value::Array(replies))
                }
            }
            Ok(msg) => self.handle(msg, wire),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("not JSON: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    /// One message; `None` for notifications and responses.
    fn handle(&mut self, msg: Value, wire: Option<&Wire>) -> Option<Value> {
        let Value::Object(o) = &msg else { return Some(error(Value::Null, INVALID_REQUEST, "a message must be a JSON object")) };
        let id = o.get("id").cloned();
        let valid_id = id.clone().filter(|i| i.is_string() || i.is_number());
        if o.get("jsonrpc") != Some(&json!("2.0")) {
            return Some(error(valid_id.unwrap_or(Value::Null), INVALID_REQUEST, "`jsonrpc` must be \"2.0\""));
        }
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response to a request of ours (we send none): nothing to answer.
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(valid_id.unwrap_or(Value::Null), INVALID_REQUEST, "a request needs a `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        if id.is_none() {
            self.notification(method);
            return None;
        }
        let Some(id) = valid_id else { return Some(error(Value::Null, INVALID_REQUEST, "`id` must be a string or a number")) };
        if !(params.is_object() || params.is_null()) {
            return Some(error(id, INVALID_PARAMS, "`params` must be an object"));
        }
        match self.request(method, &params, &id, wire) {
            Reply::Ok(r) => Some(response(id, r)),
            Reply::Err(code, m) => Some(error(id, code, m)),
            Reply::Suppressed => None,
        }
    }

    fn notification(&mut self, method: &str) {
        match method {
            "notifications/initialized" => self.initialized = true,
            // A cancel that arrives between requests is for one already answered (or unknown).
            "notifications/cancelled" | "notifications/progress" | "notifications/roots/list_changed" => {}
            other => log::debug!("ignoring the notification {other}"),
        }
    }

    fn request(&mut self, method: &str, params: &Value, id: &Value, wire: Option<&Wire>) -> Reply {
        match method {
            "initialize" => Reply::Ok(self.initialize(params)),
            "ping" => Reply::Ok(json!({})),
            "tools/list" => match self.guarded(|b, compact| tools::definitions(b, compact)) {
                Ok(list) => Reply::Ok(json!({ "tools": list })),
                Err(e) => Reply::Err(INTERNAL_ERROR, e.message),
            },
            "tools/call" => self.call_tool(params, id, wire),
            "resources/list" => self.guarded(|b, _| crate::resources::list(b)).map_or_else(|e| Reply::Err(INTERNAL_ERROR, e.message), Reply::Ok),
            "resources/templates/list" => {
                self.guarded(|b, _| crate::resources::templates(b)).map_or_else(|e| Reply::Err(INTERNAL_ERROR, e.message), Reply::Ok)
            }
            "resources/read" => {
                let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                    return Reply::Err(INVALID_PARAMS, "resources/read needs `uri`".into());
                };
                match self.guarded(|b, _| crate::resources::read(b, uri)) {
                    Ok(Some(Ok(v))) => Reply::Ok(v),
                    Ok(Some(Err(e))) => Reply::Err(INTERNAL_ERROR, e),
                    Ok(None) => Reply::Err(RESOURCE_NOT_FOUND, format!("no resource {uri} (resources/list lists them)")),
                    Err(e) => Reply::Err(INTERNAL_ERROR, e.message),
                }
            }
            other => Reply::Err(METHOD_NOT_FOUND, format!("no method {other}")),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
        let version = SUPPORTED_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSION);
        let ui = self.backend.has_ui();
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": ui }, "resources": { "subscribe": false, "listChanged": false } },
            "serverInfo": { "name": "dalica", "title": "Dalica", "version": env!("CARGO_PKG_VERSION") },
            "instructions": format!("{INSTRUCTIONS} This server: {}.", self.backend.describe()),
        })
    }

    fn call_tool(&mut self, params: &Value, id: &Value, wire: Option<&Wire>) -> Reply {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Reply::Err(INVALID_PARAMS, "tools/call needs the tool's `name`".into());
        };
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        let has_ui = self.backend.has_ui();
        if let Err(e) = tools::check_args(has_ui, name, &args) {
            return Reply::Err(INVALID_PARAMS, e);
        }
        let token = params.pointer("/_meta/progressToken").filter(|t| t.is_string() || t.is_number()).cloned();
        let (result, cancelled) = match wire {
            Some(w) => {
                let mut reporter = Reporter::new(w, id.clone(), token);
                let r = self.run_tool(name, &args, &mut reporter);
                (r, reporter.cancelled())
            }
            None => (self.run_tool(name, &args, &mut NoProgress), false),
        };
        if cancelled {
            return Reply::Suppressed;
        }
        let Some(result) = result else { return Reply::Err(INVALID_PARAMS, format!("no tool {name} (tools/list lists them)")) };
        let value = result.to_value();
        let size = serde_json::to_string(&value).map_or(usize::MAX, |s| s.len());
        if size > MAX_RESULT {
            let e = ToolError::new(
                "limit",
                format!("the result is {size} bytes, over the {MAX_RESULT}-byte limit; ask for less (a smaller page or image)"),
            );
            return Reply::Ok(ToolResult::error(&e).to_value());
        }
        Reply::Ok(value)
    }

    /// Runs a tool behind the panic guard; `None` when there is no such tool.
    fn run_tool(&mut self, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
        match catch_unwind(AssertUnwindSafe(|| tools::call(self.backend.as_mut(), name, args, progress))) {
            Ok(r) => r,
            Err(p) => {
                let e = panicked(p.as_ref());
                log::error!("the tool {name} panicked: {}", e.message);
                Some(ToolResult::error(&e))
            }
        }
    }

    /// Runs `f` on the backend behind the panic guard: an escaped panic becomes an `internal`
    /// error and the session goes on serving.
    fn guarded<T>(&mut self, f: impl FnOnce(&mut dyn Backend, bool) -> T) -> Result<T, ToolError> {
        let compact = self.compact;
        catch_unwind(AssertUnwindSafe(|| f(self.backend.as_mut(), compact))).map_err(|p| panicked(p.as_ref()))
    }
}

/// The `internal` error of a caught panic, with its message.
fn panicked(p: &(dyn std::any::Any + Send)) -> ToolError {
    let text = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".into());
    ToolError::new("internal", format!("internal error: {text}"))
}
