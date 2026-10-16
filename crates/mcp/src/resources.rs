//! `resources/list`, `resources/templates/list` and `resources/read` (`mcp.md` §7): live JSON, read
//! again on every request (nothing is cached), each the result of the tool of the same content, so
//! a resource and its tool never disagree:
//!
//! | URI | Content |
//! |---|---|
//! | `dalica://library` | `doc_inspect` without photos |
//! | `dalica://commands` | `command_list` |
//! | `dalica://selection` | the selection and the Photo Tray with a summary of each tray photo |
//! | `dalica://photo/{id}` | `photo_info` of one photo (a template); `photo.inspect` until that tool exists |
//! | `dalica://edit-ops` | `list_edit_ops`; the names of the edit operations until that tool exists |
//!
//! An unknown URI, or a photo URI whose id is not a photo, is `-32002`. People data is never part
//! of a resource: a `faces` or `people` field is dropped.

use serde_json::{Map, Value, json};

use crate::backend::{Backend, NoProgress};
use crate::tools;

const MIME: &str = "application/json";
const PHOTO: &str = "dalica://photo/";
/// The most photos `dalica://selection` summarises (`total` counts them all).
const MAX_SUMMARIES: usize = 1000;

/// The fixed resources: URI, name, title, description.
const FIXED: [(&str, &str, &str, &str); 4] = [
    (
        "dalica://library",
        "library",
        "Library",
        "The library as doc_inspect gives it without photos: revision, watched folders, counts, tree, sections, search, undo, problems.",
    ),
    (
        "dalica://commands",
        "commands",
        "Commands",
        "The command registry as command_list gives it: id, label, menu path, shortcut, params doc, effect, enabled.",
    ),
    (
        "dalica://selection",
        "selection",
        "Selection and Photo Tray",
        "The Photo Tray (the photos photo tools act on without ids), with the selection and held photos where the app is attached, and a summary of each photo.",
    ),
    ("dalica://edit-ops", "edit-ops", "Edit operations", "Every edit operation as list_edit_ops gives it."),
];

/// The resources served.
pub(crate) fn list(_backend: &mut dyn Backend) -> Value {
    let resources: Vec<Value> = FIXED
        .iter()
        .map(|(uri, name, title, description)| json!({ "uri": uri, "name": name, "title": title, "description": description, "mimeType": MIME }))
        .collect();
    json!({ "resources": resources })
}

/// The resource templates served.
pub(crate) fn templates(_backend: &mut dyn Backend) -> Value {
    json!({ "resourceTemplates": [{
        "uriTemplate": format!("{PHOTO}{{id}}"),
        "name": "photo",
        "title": "Photo",
        "description": "Everything Dalica knows about one photo, as photo_info gives it (no people data).",
        "mimeType": MIME,
    }] })
}

/// The contents of `uri`, or why not (`None`: no such resource, `-32002`).
pub(crate) fn read(backend: &mut dyn Backend, uri: &str) -> Option<Result<Value, String>> {
    let content = match uri {
        "dalica://library" => required(tool(backend, "doc_inspect", json!({}))),
        "dalica://commands" => required(tool(backend, "command_list", json!({}))),
        "dalica://selection" => selection(backend),
        "dalica://edit-ops" => edit_ops(backend),
        other => photo(backend, other.strip_prefix(PHOTO)?)?,
    };
    Some(content.map(|v| json!({ "contents": [{ "uri": uri, "mimeType": MIME, "text": v.to_string() }] })))
}

/// What a tool answered.
enum Got {
    /// No tool of that name (yet).
    Missing,
    Ok(Value),
    Failed {
        code: String,
        message: String,
    },
}

/// Runs tool `name` as `tools/call` would, without progress.
fn tool(backend: &mut dyn Backend, name: &str, args: Value) -> Got {
    let Some(r) = tools::call(backend, name, &args, &mut NoProgress) else { return Got::Missing };
    let structured = r.structured.unwrap_or(Value::Null);
    if r.is_error {
        let text = |key: &str| structured.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        return Got::Failed { code: text("code"), message: text("error") };
    }
    Got::Ok(structured)
}

/// The value of a tool that always exists.
fn required(got: Got) -> Result<Value, String> {
    match got {
        Got::Ok(v) => Ok(v),
        Got::Failed { code, message } => Err(format!("{code}: {message}")),
        Got::Missing => Err("the tool behind this resource is not available".into()),
    }
}

/// `command_run photo.inspect` with `params`: the photos' entries.
fn inspect(backend: &mut dyn Backend, params: Value) -> Got {
    match tool(backend, "command_run", json!({ "id": "photo.inspect", "params": params })) {
        Got::Ok(v) => Got::Ok(v.get("result").cloned().unwrap_or(v)),
        other => other,
    }
}

/// `dalica://photo/{id}`; `None` when `id` is not a photo's.
fn photo(backend: &mut dyn Backend, id: &str) -> Option<Result<Value, String>> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: u64 = id.parse().ok()?;
    let got = match tool(backend, "photo_info", json!({ "id": id })) {
        Got::Missing => match inspect(backend, json!({ "ids": [id] })) {
            Got::Ok(Value::Array(list)) => Got::Ok(list.into_iter().next()?),
            other => other,
        },
        other => other,
    };
    match got {
        Got::Ok(Value::Object(mut o)) => {
            o.remove("faces");
            o.remove("people");
            Some(Ok(Value::Object(o)))
        }
        Got::Failed { code, .. } if code == "not_found" => None,
        other => Some(required(other)),
    }
}

/// `dalica://edit-ops`: `list_edit_ops`, or until that tool exists the operations' names.
fn edit_ops(backend: &mut dyn Backend) -> Result<Value, String> {
    match tool(backend, "list_edit_ops", json!({})) {
        Got::Missing => {
            let names = ["rotate", "flip_h", "flip_v"].into_iter().chain(dalica_edit::LISTED);
            Ok(json!({ "ops": names.map(|op| json!({ "op": op })).collect::<Vec<_>>() }))
        }
        other => required(other),
    }
}

/// `dalica://selection`: with the app attached, its selection and tray (`library.inspect`); headless,
/// the Photo Tray as the photo tools see it (a photo command without `ids` gets the tray's, so
/// `photo.inspect` run without them lists exactly the tray's photos). Either way `photos`
/// summarises the tray's first [`MAX_SUMMARIES`] photos and `total` counts them all.
fn selection(backend: &mut dyn Backend) -> Result<Value, String> {
    let mut out = Map::new();
    let photos = if backend.has_ui() {
        let v = backend.call("library.inspect", json!({}), &mut NoProgress).map_err(|e| e.to_string())?;
        let items: Vec<u64> = v.pointer("/tray/items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
        out.insert("selection".into(), v.get("selection").cloned().unwrap_or(Value::Null));
        out.insert("tray".into(), v.get("tray").cloned().unwrap_or(Value::Null));
        out.insert("total".into(), json!(items.len()));
        let first: Vec<u64> = items.into_iter().take(MAX_SUMMARIES).collect();
        if first.is_empty() { Vec::new() } else { entries(inspect(backend, json!({ "ids": first })))? }
    } else {
        let all = entries(inspect(backend, json!({})))?;
        let items: Vec<Value> = all.iter().filter_map(|p| p.get("id").cloned()).collect();
        out.insert("tray".into(), json!({ "items": items }));
        out.insert("total".into(), json!(all.len()));
        all
    };
    out.insert("photos".into(), Value::Array(photos.iter().take(MAX_SUMMARIES).map(summary).collect()));
    Ok(Value::Object(out))
}

/// The entries of a `photo.inspect` answer.
fn entries(got: Got) -> Result<Vec<Value>, String> {
    match required(got)? {
        Value::Array(list) => Ok(list),
        other => Err(format!("photo.inspect answered something that is not a list: {other}")),
    }
}

/// A photo's summary from its `photo.inspect` entry.
fn summary(p: &Value) -> Value {
    let mut o = Map::new();
    for key in ["id", "folder", "name", "path", "star", "caption", "tags", "hidden", "effectiveDate", "displayed"] {
        if let Some(v) = p.get(key) {
            o.insert(key.into(), v.clone());
        }
    }
    let edited = p.pointer("/recipe/edits").and_then(Value::as_array).is_some_and(|e| !e.is_empty());
    o.insert("edited".into(), json!(edited));
    Value::Object(o)
}
