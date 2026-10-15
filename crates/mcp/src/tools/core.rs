//! The shared core tools of every Craft app (`mcp.md` §4.1) that M10.4 builds: `command_list`,
//! `command_run`, `command_batch` and `render_preview` (`doc_inspect` is the `inspect` group's).
//!
//! After LightCraft `crates/mcp/src/tools.rs` (`helper_tools`, `call_tool`: `command_list`,
//! `command_run`, `command_batch`, `render_preview`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`).
//! Changed for Dalica: `command_run` and batch steps take `wait`, a batch has at most
//! [`MAX_BATCH_STEPS`] steps and stops at the step whose result would pass [`crate::MAX_RESULT`],
//! and `render_preview` renders through [`Backend::render`] (the one output pipeline) with its
//! long side checked against [`MAX_PREVIEW_SIDE`].

use serde_json::{Map, Value, json};

use super::{Tool, ToolResult, object};
use crate::backend::{Backend, Progress, ToolError};

/// The most steps one `command_batch` runs (`mcp.md` §10).
pub const MAX_BATCH_STEPS: usize = 256;
/// `render_preview`'s default long side, its smallest and its largest (`mcp.md` §10).
pub const DEFAULT_PREVIEW_SIDE: u64 = 1024;
pub const MIN_PREVIEW_SIDE: u64 = 16;
pub const MAX_PREVIEW_SIDE: u64 = 2048;
/// The largest PNG a render returns; a larger one comes back as a JPEG (`mcp.md` §10).
pub const MAX_PNG_BYTES: usize = 5 << 20;

const RUN: &str = "Run any Dalica command by id with its JSON params (command_list gives the ids and each one's params doc), e.g. {id: \"photo.star.set\", params: {ids: [3], on: true}}. One call is one undo step. Photo commands run without `ids` act on the Photo Tray. A command that starts a job (export, save, scan) waits for it and answers the job's final entry with its result, reporting progress when the request has a progress token; `wait: false` answers {job, pending: true} at once. Commands that ask first in the app (delete, revert, remove folder, rebuild) need \"confirm\": true in params.";

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "command_list",
            title: "List commands",
            description: "Every command in the registry: id, label, menu path, shortcut, params doc, effect (query, library, files, external), undo history, whether it is enabled now. Run one with command_run.".into(),
            input: object(
                json!({
                    "filter": {"type": "string", "description": "Only commands whose id, label or menu path contains this text (ignoring case)"},
                    "enabled_only": {"type": "boolean", "description": "Only commands that are enabled now"},
                }),
                &[],
            ),
            output: Some(json!({"type": "object", "properties": {"commands": {"type": "array", "items": {"type": "object"}}}, "required": ["commands"]})),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "command_run",
            title: "Run command",
            description: RUN.into(),
            input: object(
                json!({
                    "id": {"type": "string", "description": "Command id, e.g. photo.star.toggle"},
                    "params": {"type": "object", "description": "The command's params (its params doc in command_list)", "additionalProperties": true},
                    "wait": {"type": "boolean", "description": "Wait for a job the command starts (default true)"},
                }),
                &["id"],
            ),
            output: None,
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        Tool {
            name: "command_batch",
            title: "Run commands",
            description: format!(
                "Run up to {MAX_BATCH_STEPS} commands in order, each as command_run would (one undo step per command; a batch is not a transaction). Answers {{completed, failed, results: [{{id, ok, result | code + error}}]}}; stop_on_error (default true) stops at the first failure."
            ),
            input: object(
                json!({
                    "steps": {"type": "array", "description": "The commands, in order", "items": object(json!({
                        "id": {"type": "string"},
                        "params": {"type": "object", "additionalProperties": true},
                        "wait": {"type": "boolean"},
                    }), &["id"])},
                    "stop_on_error": {"type": "boolean", "description": "Stop at the first failing step (default true)"},
                }),
                &["steps"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "completed": {"type": "integer"}, "failed": {"type": "integer"}, "results": {"type": "array", "items": {"type": "object"}},
            }, "required": ["completed", "failed", "results"]})),
            read_only: false,
            destructive: true,
            idempotent: false,
        },
        Tool {
            name: "render_preview",
            title: "Render preview",
            description: format!(
                "An image of a photo with its edits (the same rendering Export uses); changes nothing. `max_side` is the long side in pixels ({DEFAULT_PREVIEW_SIDE} by default, {MIN_PREVIEW_SIDE}–{MAX_PREVIEW_SIDE}); never enlarged. PNG, or JPEG when the PNG would pass 5 MiB."
            ),
            input: object(
                json!({
                    "id": {"type": "integer", "description": "Photo id (default: the first photo in the Photo Tray)"},
                    "max_side": {"type": "integer", "description": "Long side in pixels"},
                }),
                &[],
            ),
            output: Some(json!({"type": "object", "properties": {
                "id": {"type": "integer"}, "width": {"type": "integer"}, "height": {"type": "integer"},
                "mimeType": {"type": "string"}, "recipe": {"type": "array", "items": {"type": "string"}},
            }, "required": ["id", "width", "height", "mimeType", "recipe"]})),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
    ]
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    Some(match name {
        "command_list" => {
            let mut p = Map::new();
            if let Some(f) = args.get("filter").filter(|v| !v.is_null()) {
                p.insert("filter".into(), f.clone());
            }
            if let Some(e) = args.get("enabled_only").filter(|v| !v.is_null()) {
                p.insert("enabledOnly".into(), e.clone());
            }
            ToolResult::from(b.call("engine.commands", Value::Object(p), progress).map(|list| json!({ "commands": list })))
        }
        "command_run" => match args.get("id").and_then(Value::as_str) {
            Some(id) => ToolResult::from(execute(b, id, args.get("params"), args.get("wait"), progress)),
            None => ToolResult::error(&ToolError::new("bad_params", "missing `id`")),
        },
        "command_batch" => batch(b, args, progress),
        "render_preview" => preview(b, args),
        _ => return None,
    })
}

/// `engine.execute {command, params, wait}`.
fn execute(b: &mut dyn Backend, id: &str, params: Option<&Value>, wait: Option<&Value>, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let mut p = Map::new();
    p.insert("command".into(), json!(id));
    p.insert("params".into(), params.filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!({})));
    if let Some(w) = wait.filter(|v| !v.is_null()) {
        p.insert("wait".into(), w.clone());
    }
    b.call("engine.execute", Value::Object(p), progress)
}

/// A batch step's progress: the steps done of all, while each step's own job reports through it
/// (so cancelling the request stops the step's job too).
struct Steps<'a> {
    inner: &'a mut dyn Progress,
    done: u64,
    total: u64,
}

impl Progress for Steps<'_> {
    fn report(&mut self, _done: u64, _total: u64, message: &str) -> bool {
        self.inner.report(self.done, self.total, message)
    }
}

fn batch(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> ToolResult {
    let steps = args.get("steps").and_then(Value::as_array).cloned().unwrap_or_default();
    if steps.len() > MAX_BATCH_STEPS {
        return ToolResult::error(&ToolError::new("limit", format!("at most {MAX_BATCH_STEPS} steps in one batch, not {}", steps.len())));
    }
    let stop = args.get("stop_on_error").and_then(Value::as_bool).unwrap_or(true);
    let total = u64::try_from(steps.len()).unwrap_or(u64::MAX);
    let (mut completed, mut failed, mut results, mut size) = (0u64, 0u64, Vec::new(), 0usize);
    for (n, step) in (0u64..).zip(&steps) {
        let id = step.get("id").and_then(Value::as_str).unwrap_or_default();
        let mut each = Steps { inner: &mut *progress, done: n, total };
        if !each.report(n, total, &format!("Step {} of {total}: {id}", n.saturating_add(1))) {
            return ToolResult::error(&ToolError::new("cancelled", "the batch was cancelled"));
        }
        let entry = match execute(b, id, step.get("params"), step.get("wait"), &mut each) {
            Ok(result) => {
                completed += 1;
                json!({ "id": id, "ok": true, "result": result })
            }
            Err(e) if e.code == "cancelled" => return ToolResult::error(&e),
            Err(e) => {
                failed += 1;
                let mut v = e.to_json();
                if let Some(o) = v.as_object_mut() {
                    o.insert("id".into(), json!(id));
                    o.insert("ok".into(), json!(false));
                }
                v
            }
        };
        size = size.saturating_add(serde_json::to_string(&entry).map_or(usize::MAX, |s| s.len()));
        if size > crate::MAX_RESULT {
            failed += 1;
            results.push(json!({ "id": id, "ok": false, "code": "limit", "error": format!("the batch's results would pass {} bytes: stopped after this step (it ran)", crate::MAX_RESULT) }));
            break;
        }
        let ok = entry.get("ok") == Some(&Value::Bool(true));
        results.push(entry);
        if !ok && stop {
            break;
        }
    }
    let _ = progress.report(total, total, "Done");
    let r = ToolResult::json(json!({ "completed": completed, "failed": failed, "results": results }));
    ToolResult { is_error: failed > 0, ..r }
}

fn preview(b: &mut dyn Backend, args: &Value) -> ToolResult {
    let side = args.get("max_side").and_then(Value::as_u64).unwrap_or(DEFAULT_PREVIEW_SIDE);
    if !(MIN_PREVIEW_SIDE..=MAX_PREVIEW_SIDE).contains(&side) || args.get("max_side").is_some_and(|v| v.is_i64() && !v.is_u64()) {
        let asked = args.get("max_side").cloned().unwrap_or(Value::Null);
        return ToolResult::error(&ToolError::new("limit", format!("max_side is {MIN_PREVIEW_SIDE}–{MAX_PREVIEW_SIDE} pixels, not {asked}")));
    }
    let id = match args.get("id").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match v.as_u64() {
            Some(id) => Some(id),
            None => return ToolResult::error(&ToolError::new("not_found", format!("no photo {v}"))),
        },
    };
    match b.render(id, u32::try_from(side).unwrap_or(u32::MAX)) {
        Ok(r) => {
            ToolResult::image(&r.bytes, r.mime, json!({ "id": r.id, "width": r.width, "height": r.height, "mimeType": r.mime, "recipe": r.recipe }))
        }
        Err(e) => ToolResult::error(&e),
    }
}
