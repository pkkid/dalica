//! MCP tools (`mcp.md` §4): each group a module with its definitions and its calls, all forwarded
//! to a [`Backend`] as control-protocol methods.
//!
//! After LightCraft `crates/mcp/src/tools.rs` (`ToolResult`, `tool`, `check_args`,
//! `tool_definitions`, `annotations`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for
//! Dalica: the tools are split into groups, every tool carries its four annotation hints from its
//! own definition (`openWorldHint` false everywhere: Dalica's tools never reach the network), a
//! failure is a result with the control protocol's `code`, and a helper's arguments are checked
//! against its whole input schema (unknown keys, required keys, types), not only its keys.
//!
//! | Group | Tools | Task |
//! |---|---|---|
//! | `core` | `command_list`, `command_run`, `command_batch`, `render_preview` | M10.4 |
//! | `inspect` | `doc_inspect` | M10.7 |
//! | `organize` | `scan_folder`, `query_photos`, `photo_info`, `select_photos`, `set_metadata`, `create_album`, `jobs_*` | M10.13 |
//! | `edit`, `output` | `list_edit_ops`, `get_recipe`, `set_recipe`, `apply_edit`; `render_photo`, `export` | M10.14 |
//! | `commands` | one `cmd_*` tool per command (left out by `--compact`) | M10.15 |
//! | `ui` | the connect-mode UI tools | M10.17 |

mod commands;
mod core;
mod edit;
mod inspect;
mod organize;
mod output;
mod ui;

pub use self::core::{MAX_BATCH_STEPS, MAX_PNG_BYTES, MAX_PREVIEW_SIDE, MIN_PREVIEW_SIDE};

use serde_json::{Map, Value, json};

use crate::backend::{Backend, Progress, ToolError};

/// One tool's definition.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: String,
    /// A JSON schema of the arguments (`type: object`, `additionalProperties: false` for helpers).
    pub input: Value,
    /// A JSON schema of `structuredContent`, where the result's shape is fixed.
    pub output: Option<Value>,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
}

impl Tool {
    /// The definition `tools/list` gives, with the four annotation hints.
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.input,
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.idempotent,
                "openWorldHint": false,
            },
        });
        if let (Some(schema), Some(o)) = (&self.output, v.as_object_mut()) {
            o.insert("outputSchema".into(), schema.clone());
        }
        v
    }
}

/// An object schema of `properties`, `required` named, nothing else allowed.
pub(crate) fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

/// The result of `tools/call`: content blocks, the error flag and `structuredContent`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
    pub structured: Option<Value>,
}

impl ToolResult {
    /// A JSON result: a compact JSON text block and the value as `structuredContent` (an object,
    /// so anything else is wrapped as `{result}`).
    pub fn json(v: Value) -> ToolResult {
        let text = serde_json::to_string(&v).unwrap_or_default();
        let structured = if v.is_object() { v } else { json!({ "result": v }) };
        ToolResult { content: vec![json!({ "type": "text", "text": text })], is_error: false, structured: Some(structured) }
    }

    /// A failed operation: `isError` with `{code, error, prompt?}` (`mcp.md` §3).
    pub fn error(e: &ToolError) -> ToolResult {
        ToolResult { is_error: true, ..ToolResult::json(e.to_json()) }
    }

    pub fn from(r: Result<Value, ToolError>) -> ToolResult {
        match r {
            Ok(v) => ToolResult::json(v),
            Err(e) => ToolResult::error(&e),
        }
    }

    /// An image block before the JSON of `meta`.
    pub fn image(bytes: &[u8], mime: &str, meta: Value) -> ToolResult {
        let mut r = ToolResult::json(meta);
        r.content.insert(0, json!({ "type": "image", "data": base64(bytes), "mimeType": mime }));
        r
    }

    /// The `tools/call` result object.
    pub fn to_value(&self) -> Value {
        let mut v = json!({ "content": self.content, "isError": self.is_error });
        if let (Some(s), Some(o)) = (&self.structured, v.as_object_mut()) {
            o.insert("structuredContent".into(), s.clone());
        }
        v
    }
}

/// Standard base64 with padding (RFC 4648 §4), for image content.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sym = |n: u32| char::from(ALPHABET.get((n & 63) as usize).copied().unwrap_or(b'='));
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}

/// The helper tools of every group (connect mode's UI tools when `has_ui`).
fn helpers(has_ui: bool) -> Vec<Tool> {
    let mut v = core::tools();
    v.extend(inspect::tools());
    v.extend(organize::tools());
    v.extend(edit::tools());
    v.extend(output::tools());
    v.extend(ui::tools(has_ui));
    v
}

/// Every tool `tools/list` gives: the helpers, then one tool per command unless `compact`.
pub(crate) fn definitions(b: &mut dyn Backend, compact: bool) -> Vec<Value> {
    let mut v = helpers(b.has_ui());
    if !compact {
        v.extend(commands::tools(b));
    }
    v.iter().map(Tool::to_json).collect()
}

/// Checks a helper's arguments against its input schema before anything runs: an unknown key
/// (named with the accepted keys), a missing required key or a value of the wrong type is
/// JSON-RPC `-32602`. Tools that are not helpers (`cmd_*`) take any object; their command checks
/// its own params.
pub(crate) fn check_args(has_ui: bool, name: &str, args: &Value) -> Result<(), String> {
    let args = if args.is_null() { &Value::Object(Map::new()) } else { args };
    let Some(tool) = helpers(has_ui).into_iter().find(|t| t.name == name) else {
        return if args.is_object() { Ok(()) } else { Err(format!("the arguments of {name} must be an object")) };
    };
    validate(&tool.input, args, name)
}

/// Whether `value` fits `schema` (the subset the helpers use: `type`, `properties`, `required`,
/// `additionalProperties: false`, `items`, `enum`); `at` names the value in the message.
fn validate(schema: &Value, value: &Value, at: &str) -> Result<(), String> {
    let fits = match schema.get("type").and_then(Value::as_str) {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("number") => value.is_number(),
        Some("boolean") => value.is_boolean(),
        _ => true,
    };
    if !fits {
        let want = schema.get("type").and_then(Value::as_str).unwrap_or("value");
        return Err(format!("{at} must be {} {want}, not {value}", if want.starts_with(['a', 'e', 'i', 'o']) { "an" } else { "a" }));
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array)
        && !choices.contains(value)
    {
        let names: Vec<String> = choices.iter().map(Value::to_string).collect();
        return Err(format!("{at} must be one of {}, not {value}", names.join(", ")));
    }
    if let (Some(given), Some(props)) = (value.as_object(), schema.get("properties").and_then(Value::as_object)) {
        if schema.get("additionalProperties") == Some(&Value::Bool(false))
            && let Some(bad) = given.keys().find(|k| !props.contains_key(*k))
        {
            let accepted: Vec<&str> = props.keys().map(String::as_str).collect();
            return Err(format!("unknown argument \"{bad}\" for {at}; expected: {}", accepted.join(", ")));
        }
        for req in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if !given.contains_key(req) {
                return Err(format!("{at} needs \"{req}\""));
            }
        }
        for (k, v) in given {
            if let Some(s) = props.get(k)
                && !v.is_null()
            {
                validate(s, v, &format!("{at}.{k}"))?;
            }
        }
    }
    if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
        for (i, v) in list.iter().enumerate() {
            validate(items, v, &format!("{at}[{i}]"))?;
        }
    }
    Ok(())
}

/// Runs tool `name`; `None` when no group has it.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    let empty = Value::Object(Map::new());
    let args = if args.is_null() { &empty } else { args };
    core::call(b, name, args, progress)
        .or_else(|| inspect::call(b, name, args, progress))
        .or_else(|| organize::call(b, name, args, progress))
        .or_else(|| edit::call(b, name, args, progress))
        .or_else(|| output::call(b, name, args, progress))
        .or_else(|| commands::call(b, name, args, progress))
        .or_else(|| ui::call(b, name, args, progress))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_like_rfc_4648() {
        for (raw, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    /// Keys, required keys, types, enums, nested objects and array items; `null` counts as absent.
    #[test]
    fn helper_arguments_are_checked_against_the_schema() {
        let schema = object(
            json!({
                "id": {"type": "string"},
                "n": {"type": "integer"},
                "mode": {"type": "string", "enum": ["a", "b"]},
                "steps": {"type": "array", "items": object(json!({"id": {"type": "string"}}), &["id"])},
            }),
            &["id"],
        );
        assert_eq!(validate(&schema, &json!({"id": "x", "n": 3, "mode": "a", "steps": [{"id": "y"}], }), "t"), Ok(()));
        assert_eq!(validate(&schema, &json!({"id": "x", "n": null}), "t"), Ok(()));
        let err = |v: Value| validate(&schema, &v, "t").unwrap_err();
        assert_eq!(err(json!({"id": "x", "zz": 1})), "unknown argument \"zz\" for t; expected: id, mode, n, steps");
        assert_eq!(err(json!({})), "t needs \"id\"");
        assert_eq!(err(json!({"id": 1})), "t.id must be a string, not 1");
        assert_eq!(err(json!({"id": "x", "n": 1.5})), "t.n must be an integer, not 1.5");
        assert_eq!(err(json!({"id": "x", "mode": "c"})), "t.mode must be one of \"a\", \"b\", not \"c\"");
        assert_eq!(err(json!({"id": "x", "steps": [{"id": "y", "q": 1}]})), "unknown argument \"q\" for t.steps[0]; expected: id");
        assert_eq!(err(json!([])), "t must be an object, not []");
    }
}
