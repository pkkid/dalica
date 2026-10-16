//! The connect-mode UI tools of `mcp.md` §4.3, each one control method of the running app
//! (`control-protocol.md` §6):
//!
//! | Tool | Method |
//! |---|---|
//! | `screenshot {max_size?, headless?, widget?, path?}` | `ui.screenshot` (which settles first), answered as image content, scaled to `max_size` (1600) on its long side; with `path` the app writes the file and it is read back |
//! | `inspect_ui {reset_perf?}` | `ui.inspect` |
//! | `list_widgets {filter?, region?}` | `ui.widgets` |
//! | `click {widget \| x, y, count?, button?, ctrl?, shift?, alt?, cmd?}` | `ui.clickWidget`, or `ui.pointer` at a point |
//! | `drag {widget, to_widget \| to_x, to_y, ctrl?}` | `ui.dragWidget` |
//! | `press_key {key \| combo, state?, ctrl?, shift?, alt?, cmd?}` | `ui.key` |
//! | `type_text {text}` | `ui.type` |
//! | `scroll {dx?, dy?, widget?, ctrl?}` | `ui.scroll` |
//! | `menu_invoke {path \| id, params?}` | `ui.menu.invoke` |
//! | `settle {timeout_ms?, jobs?}` | `ui.settle` |
//! | `control_call {method, params?}` | any method, passed through unchanged |
//!
//! They are listed only in connect mode; in headless mode a call answers `isError` saying how to
//! start the app with `--control` and connect with `--connect`.
//!
//! After LightCraft `crates/mcp/src/tools.rs` (`screenshot`, the UI arms of `call_tool`) @
//! b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: the tool names and arguments of
//! `mcp.md` §4.3, Dalica's control methods, and the screenshot scaled through the output pipeline.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::{Tool, ToolResult, object};
use crate::backend::{Backend, Progress, ToolError};

/// `screenshot`'s default long side.
pub const DEFAULT_SCREENSHOT_SIDE: u64 = 1600;
/// The largest screenshot file read back.
const MAX_SCREENSHOT_FILE: u64 = 64 << 20;

/// What a UI tool says in headless mode.
pub const NEEDS_APP: &str = "this tool drives the desktop app's window, and this MCP server is headless. Start the app with `dalica --control PORT` (add --demo for the demo library) and run the server with `dalica-cli mcp --connect 127.0.0.1:PORT`";

/// The tool names, in the order they are listed.
const NAMES: &[&str] =
    &["screenshot", "inspect_ui", "list_widgets", "click", "drag", "press_key", "type_text", "scroll", "menu_invoke", "settle", "control_call"];

/// The modifier flags, the same name in the tool and the method.
const MODIFIERS: [(&str, &str); 4] = [("ctrl", "ctrl"), ("shift", "shift"), ("alt", "alt"), ("cmd", "cmd")];

/// `props` with the four modifier flags added.
fn with_modifiers(props: Value) -> Value {
    let mut p = match props {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    for (k, d) in
        [("ctrl", "Hold Ctrl (the command modifier, Cmd on macOS)"), ("shift", "Hold Shift"), ("alt", "Hold Alt"), ("cmd", "Hold Cmd (macOS)")]
    {
        p.insert(k.into(), json!({"type": "boolean", "description": d}));
    }
    Value::Object(p)
}

/// (read only, destructive, idempotent).
type Hints = (bool, bool, bool);
const LOOK: Hints = (true, false, true);
const INPUT: Hints = (false, true, false);

fn tool(name: &'static str, title: &'static str, description: String, input: Value, hints: Hints) -> Tool {
    let (read_only, destructive, idempotent) = hints;
    Tool { name, title, description, input, output: None, read_only, destructive, idempotent }
}

/// This group's tools: only when a window is attached (`has_ui`).
pub(crate) fn tools(has_ui: bool) -> Vec<Tool> {
    if !has_ui {
        return Vec::new();
    }
    vec![
        tool(
            "screenshot",
            "Screenshot",
            format!(
                "The app's window as an image, after the window has settled (thumbnails and previews finished), scaled to max_size on its long side ({DEFAULT_SCREENSHOT_SIDE} by default). headless: true draws it on the CPU (works while the window is covered or minimised); widget crops it to one widget; path also writes the PNG there (inside the app's automation write roots). Answers the image and {{width, height, headless, mimeType, path?}}."
            ),
            object(
                json!({
                    "max_size": {"type": "integer", "description": "Long side in pixels (default 1600)"},
                    "headless": {"type": "boolean", "description": "Draw on the CPU instead of reading the window back"},
                    "widget": {"type": "string", "description": "Crop to this widget id (list_widgets)"},
                    "path": {"type": "string", "description": "Also write the PNG here (absolute, ending in .png)"},
                }),
                &[],
            ),
            (false, false, false),
        ),
        tool(
            "inspect_ui",
            "Inspect UI",
            "The window's state: view state, regions, scroll, hovered and focused widgets, popups, dialogs, toasts, jobs and perf counters (ui.inspect).".into(),
            object(json!({"reset_perf": {"type": "boolean", "description": "Clear the perf maxima"}}), &[]),
            LOOK,
        ),
        tool(
            "list_widgets",
            "List widgets",
            "The widgets drawn in the last frame with their automation ids and rects (ui.widgets). filter is a substring of the id; region restricts to a region of inspect_ui.".into(),
            object(
                json!({"filter": {"type": "string", "description": "Substring of the widget id"}, "region": {"type": "string", "description": "A region name (lightTable, tree, tray, …)"}}),
                &[],
            ),
            LOOK,
        ),
        tool(
            "click",
            "Click",
            "Click a widget by id (or the point x, y in screen points), as the user would: count 2 double-clicks, button right opens context menus, modifiers apply to the click.".into(),
            object(
                with_modifiers(json!({
                    "widget": {"type": "string", "description": "Widget id (list_widgets)"},
                    "x": {"type": "number", "description": "Screen x in points (with y, instead of widget)"},
                    "y": {"type": "number", "description": "Screen y in points"},
                    "count": {"type": "integer", "description": "Clicks (1–8, default 1)"},
                    "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "Mouse button (default left)"},
                })),
                &[],
            ),
            INPUT,
        ),
        tool(
            "drag",
            "Drag",
            "Drag a widget onto another widget (to_widget) or to a point (to_x, to_y): a photo onto a folder moves it (ctrl: copy), onto the tray holds it, onto an album adds it.".into(),
            object(
                json!({
                    "widget": {"type": "string", "description": "The widget dragged"},
                    "to_widget": {"type": "string", "description": "The widget dropped on"},
                    "to_x": {"type": "number", "description": "Drop x in screen points"},
                    "to_y": {"type": "number", "description": "Drop y in screen points"},
                    "ctrl": {"type": "boolean", "description": "Hold Ctrl (copy)"},
                }),
                &["widget"],
            ),
            INPUT,
        ),
        tool(
            "press_key",
            "Press key",
            "Press a key (egui names: A, Enter, Escape, ArrowLeft, F2, Delete) or a combo in the shortcut syntax (Ctrl+Shift+R, where Ctrl is the command modifier). state down or up holds or releases it.".into(),
            object(
                with_modifiers(json!({
                    "key": {"type": "string", "description": "Key name"},
                    "combo": {"type": "string", "description": "Shortcut, e.g. Ctrl+H"},
                    "state": {"type": "string", "enum": ["press", "down", "up"], "description": "press (default), down or up"},
                })),
                &[],
            ),
            INPUT,
        ),
        tool(
            "type_text",
            "Type text",
            "Type text into the focused widget (the search field, the caption bar, a dialog's field).".into(),
            object(json!({"text": {"type": "string", "description": "The text"}}), &["text"]),
            INPUT,
        ),
        tool(
            "scroll",
            "Scroll",
            "Scroll by dx, dy points, over a widget or where the pointer is; ctrl zooms where the view does.".into(),
            object(
                json!({
                    "dx": {"type": "number", "description": "Horizontal points"},
                    "dy": {"type": "number", "description": "Vertical points (positive scrolls down)"},
                    "widget": {"type": "string", "description": "Scroll over this widget"},
                    "ctrl": {"type": "boolean", "description": "Hold Ctrl"},
                }),
                &[],
            ),
            (false, false, false),
        ),
        tool(
            "menu_invoke",
            "Invoke menu item",
            "Click a menu item by path (labels as shown, Picture/Batch Edit/Rotate Clockwise) or by command id: an item ending in … opens its dialog ({dialog}), others run their command on the selection.".into(),
            object(
                json!({
                    "path": {"type": "string", "description": "Menu path, labels separated by /"},
                    "id": {"type": "string", "description": "The item's command id"},
                    "params": {"type": "object", "description": "Extra params for the command", "additionalProperties": true},
                }),
                &[],
            ),
            INPUT,
        ),
        tool(
            "settle",
            "Settle",
            "Run frames until nothing visible is pending (thumbnails, previews, animations, scanner batches); jobs: true also waits for metadata writes and file jobs. Answers {settled, frames, ms, busy}.".into(),
            object(
                json!({"timeout_ms": {"type": "integer", "description": "At most this long (default 5000, at most 60000)"}, "jobs": {"type": "boolean", "description": "Also wait for file jobs"}}),
                &[],
            ),
            LOOK,
        ),
        tool(
            "control_call",
            "Control call",
            "Call any method of the app's control channel with its params, passed through unchanged (the methods method lists them).".into(),
            object(
                json!({"method": {"type": "string", "description": "The method, e.g. ui.menu.tree"}, "params": {"type": "object", "description": "Its params", "additionalProperties": true}}),
                &["method"],
            ),
            INPUT,
        ),
    ]
}

/// The keys of `args` that are given (not `null`), renamed `from → to`.
fn renamed(args: &Value, keys: &[(&str, &str)]) -> Map<String, Value> {
    let mut o = Map::new();
    for (from, to) in keys {
        if let Some(v) = args.get(*from).filter(|v| !v.is_null()) {
            o.insert((*to).into(), v.clone());
        }
    }
    o
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    if !NAMES.contains(&name) {
        return None;
    }
    if !b.has_ui() {
        return Some(ToolResult::error(&ToolError::new("unsupported", format!("{name}: {NEEDS_APP}"))));
    }
    if name == "screenshot" {
        return Some(match take_screenshot(b, args, progress) {
            Ok((png, meta)) => ToolResult::image(&png, "image/png", meta),
            Err(e) => ToolResult::error(&e),
        });
    }
    if name == "control_call" {
        let method = args.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
        let params = args.get("params").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
        return Some(ToolResult::from(b.call(&method, params, progress)));
    }
    Some(match request(name, args) {
        Ok((method, params)) => ToolResult::from(b.call(method, Value::Object(params), progress)),
        Err(e) => ToolResult::error(&e),
    })
}

/// The method and params a UI tool sends (all but `screenshot` and `control_call`).
fn request(name: &str, args: &Value) -> Result<(&'static str, Map<String, Value>), ToolError> {
    let with_mods = |keys: &[(&'static str, &'static str)]| {
        let mut all = keys.to_vec();
        all.extend(MODIFIERS);
        all
    };
    let one_of = |p: &Map<String, Value>, a: &str, b: &str| {
        if p.contains_key(a) == p.contains_key(b) { Err(bad(format!("{name} needs {a} or {b} (one of them)"))) } else { Ok(()) }
    };
    Ok(match name {
        "inspect_ui" => ("ui.inspect", renamed(args, &[("reset_perf", "resetPerf")])),
        "list_widgets" => ("ui.widgets", renamed(args, &[("filter", "filter"), ("region", "region")])),
        "click" => click(args)?,
        "drag" => {
            let p = renamed(args, &[("widget", "id"), ("to_widget", "toWidget"), ("to_x", "toX"), ("to_y", "toY"), ("ctrl", "ctrl")]);
            let point = p.contains_key("toX") && p.contains_key("toY");
            if p.contains_key("toWidget") == point {
                return Err(bad("drag needs to_widget, or to_x and to_y (not both)"));
            }
            ("ui.dragWidget", p)
        }
        "press_key" => {
            let p = renamed(args, &with_mods(&[("key", "key"), ("combo", "combo"), ("state", "state")]));
            one_of(&p, "key", "combo")?;
            ("ui.key", p)
        }
        "type_text" => ("ui.type", renamed(args, &[("text", "text")])),
        "scroll" => ("ui.scroll", renamed(args, &[("dx", "dx"), ("dy", "dy"), ("widget", "widget"), ("ctrl", "ctrl")])),
        "menu_invoke" => {
            let p = renamed(args, &[("path", "path"), ("id", "id"), ("params", "params")]);
            one_of(&p, "path", "id")?;
            ("ui.menu.invoke", p)
        }
        "settle" => ("ui.settle", renamed(args, &[("timeout_ms", "timeoutMs"), ("jobs", "jobs")])),
        other => return Err(ToolError::new("internal", format!("{other} is not a UI tool"))),
    })
}

/// `click`: `ui.clickWidget` for a widget, `ui.pointer` (a move, then a press and a release per
/// click) for a point.
fn click(args: &Value) -> Result<(&'static str, Map<String, Value>), ToolError> {
    let point = (args.get("x").and_then(Value::as_f64), args.get("y").and_then(Value::as_f64));
    let widget = args.get("widget").filter(|v| !v.is_null());
    let mut keys = vec![("button", "button")];
    keys.extend(MODIFIERS);
    let mut p = renamed(args, &keys);
    match (widget, point) {
        (Some(w), (None, None)) => {
            p.insert("id".into(), w.clone());
            if let Some(c) = args.get("count").filter(|v| !v.is_null()) {
                p.insert("count".into(), c.clone());
            }
            Ok(("ui.clickWidget", p))
        }
        (None, (Some(x), Some(y))) => {
            let count = match args.get("count").filter(|v| !v.is_null()) {
                None => 1,
                Some(v) => v.as_u64().filter(|c| (1..=8).contains(c)).ok_or_else(|| ToolError::new("limit", format!("count is 1–8, not {v}")))?,
            };
            let mut events = vec![json!({"kind": "move", "x": x, "y": y})];
            for _ in 0..count {
                events.push(json!({"kind": "down", "x": x, "y": y}));
                events.push(json!({"kind": "up", "x": x, "y": y}));
            }
            p.insert("events".into(), Value::Array(events));
            Ok(("ui.pointer", p))
        }
        _ => Err(bad("click needs widget, or x and y (not both)")),
    }
}

/// `screenshot`: `ui.screenshot`, the PNG inline or read back from `path`, scaled to `max_size`.
fn take_screenshot(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<(Vec<u8>, Value), ToolError> {
    let side = match args.get("max_size").filter(|v| !v.is_null()) {
        None => DEFAULT_SCREENSHOT_SIDE,
        Some(v) => v
            .as_u64()
            .filter(|s| (16..=16_384).contains(s))
            .ok_or_else(|| ToolError::new("limit", format!("max_size is 16–16384 pixels, not {v}")))?,
    };
    let params = renamed(args, &[("headless", "headless"), ("widget", "widget"), ("path", "path")]);
    let path = params.get("path").and_then(Value::as_str).map(str::to_string);
    let r = b.call("ui.screenshot", Value::Object(params), progress)?;
    let png = match &path {
        Some(p) => match dalica_fs::read(Path::new(p), MAX_SCREENSHOT_FILE) {
            Ok(Some(read)) => read.bytes,
            Ok(None) => return Err(ToolError::new("not_found", format!("the app wrote no screenshot at {p}"))),
            Err(e) => return Err(ToolError::new("not_found", format!("the screenshot at {p} cannot be read: {e}"))),
        },
        None => match r.get("png").and_then(Value::as_str) {
            Some(text) => base64_decode(text).ok_or_else(|| ToolError::new("internal", "the app's screenshot is not base64"))?,
            None => return Err(ToolError::new("internal", "the app answered no screenshot")),
        },
    };
    let (w, h) = (r.get("width").and_then(Value::as_u64).unwrap_or(0), r.get("height").and_then(Value::as_u64).unwrap_or(0));
    let (png, width, height) = crate::connect::fit_png(png, w, h, u32::try_from(side).unwrap_or(u32::MAX))?;
    let mut meta =
        json!({ "width": width, "height": height, "headless": r.get("headless").cloned().unwrap_or(Value::Null), "mimeType": "image/png" });
    if let (Some(p), Some(o)) = (path, meta.as_object_mut()) {
        o.insert("path".into(), json!(p));
    }
    Ok((png, meta))
}

/// Standard base64 (RFC 4648 §4, padding optional); `None` for any other character.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c.checked_sub(b'a')?.checked_add(26)?,
            b'0'..=b'9' => c.checked_sub(b'0')?.checked_add(52)?,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body = bytes.strip_suffix(b"==").or_else(|| bytes.strip_suffix(b"=")).unwrap_or(&bytes);
    let mut out = Vec::with_capacity(body.len().saturating_mul(3) / 4);
    for chunk in body.chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        let [_, a, b, c] = n.to_be_bytes();
        out.extend([a, b, c].iter().take(chunk.len() - 1));
    }
    Some(out)
}

fn bad(message: impl Into<String>) -> ToolError {
    ToolError::new("bad_params", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_what_the_encoder_writes() {
        for raw in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0xfb, 0xff, 0x00, 0x10]] {
            assert_eq!(base64_decode(&super::super::base64(raw)).as_deref(), Some(raw));
        }
        assert_eq!(base64_decode("Zm9v\nYmFy"), Some(b"foobar".to_vec()));
        assert_eq!(base64_decode("Zm9*"), None);
        assert_eq!(base64_decode("Z"), None);
    }
}
