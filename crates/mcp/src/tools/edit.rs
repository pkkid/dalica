//! The editing helpers of `mcp.md` §4.2: `list_edit_ops`, `get_recipe`, `set_recipe` and
//! `apply_edit`. Every change goes through an engine command on the backend: `set_recipe` one step
//! of the photo's own history (`editing.md` §5.1), `apply_edit` one library step; nothing here
//! touches pixels or files.
//!
//! - `list_edit_ops {op?}`: the rows of `edit::catalog()` as JSON (name, label, syntax, parameters
//!   with their kinds, ranges and defaults, whether the canvas may change size, the milestone,
//!   whether this build renders it).
//! - `get_recipe {id}`: `photo.history.state` as `{id, recipe, ops, redo, saved}`: the recipe as
//!   its `|`-joined text and as `{op, params, entry, known}` items (`entry` verbatim, so an
//!   operation Dalica cannot read round-trips through `set_recipe` unchanged); `saved` is true
//!   when the photo was saved in place and holds no newer edits (Undo Save applies).
//! - `set_recipe {id, recipe, label?, drop_unknown?}`: `edit.recipe.set` on the one photo, which
//!   refuses by name to drop an operation it cannot read unless `drop_unknown`.
//! - `apply_edit {ids?, op, params?}`: one operation appended to each photo (the Photo Tray's
//!   without `ids`) by Batch Edit, `edit.apply` (`editing.md` §9): **one library undo step for all
//!   the photos** ("Undo Auto Fix"; Edit ▸ Undo takes it back). The edit view's rules hold: a
//!   rotation right after a rotation merges and the same flip twice cancels (the engine's), only
//!   the last crop and straighten count and the same one again takes no step (that photo is left
//!   out), and Tuning and Fill Light merge into a trailing `tuning` entry, the values given
//!   replacing its values and the others staying. Photos whose merged entries differ run as one
//!   `engine.group` of `edit.apply`s, still one step ("Undo Change Photos"; connect mode runs a
//!   group's commands as separate steps). An operation this build cannot draw yet, which Batch Edit
//!   greys ("Arrives with Effects"), is still kept in each recipe (and skipped when drawn), one
//!   step of each photo's history as in M10, until it renders. Every photo is checked before any
//!   changes; progress is reported and cancellation honoured before the change.

use std::collections::HashSet;

use dalica_edit::Op as EditOp;
use dalica_edit::catalog::{self, Kind, OpInfo, Param};
use serde_json::{Map, Value, json};

use super::{Tool, ToolResult, object};
use crate::backend::{Backend, NoProgress, Progress, ToolError};

const RECIPE: &str = "The recipe: its operation entries joined by | (\"crop(0.1,0.1,0.9,0.9)|autofix()\"; \"\" is none), or a list of entries or {op, params} items (params the text between the parentheses), as get_recipe gives them";

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    let recipe_out = json!({"type": "object", "properties": {
        "id": {"type": "integer"}, "recipe": {"type": "string"}, "ops": {"type": "array", "items": {"type": "object"}},
        "redo": {"type": "array", "items": {"type": "string"}}, "saved": {"type": "boolean"},
    }, "required": ["id", "recipe", "ops", "redo", "saved"]});
    vec![
        Tool {
            name: "list_edit_ops",
            title: "List edit operations",
            description: "Every edit operation a recipe can hold: name, label, syntax, parameters (name, label, kind with its range or choices, default), whether it may change the picture's size, the milestone that brings it and whether this build renders it (`available`; one that does not is kept in the recipe and skipped). `op` gives one. Changes nothing.".into(),
            input: object(json!({ "op": {"type": "string", "description": "Only this operation, e.g. crop"} }), &[]),
            output: Some(json!({"type": "object", "properties": {"ops": {"type": "array", "items": {"type": "object"}}}, "required": ["ops"]})),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "get_recipe",
            title: "Get recipe",
            description: "A photo's edits: {id, recipe: \"crop(…)|autofix()\", ops: [{op, params, entry, known}], redo: [entries], saved} (saved: it was saved in place and holds no newer edits). Changes nothing.".into(),
            input: object(json!({ "id": {"type": "integer", "description": "Photo id"} }), &["id"]),
            output: Some(recipe_out.clone()),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "set_recipe",
            title: "Set recipe",
            description: "Replace a photo's whole recipe in one undo step of its history (entries kept verbatim, in order). Refuses to drop an operation Dalica cannot read (legacy or from a newer version) unless drop_unknown is true, and to add one the photo does not hold. Answers the recipe as get_recipe does, with `changed`.".into(),
            input: object(
                json!({
                    "id": {"type": "integer", "description": "Photo id"},
                    "recipe": {"type": ["string", "array"], "description": RECIPE},
                    "label": {"type": "string", "description": "A note for your own use (the undo step is named by the last entry)"},
                    "drop_unknown": {"type": "boolean", "description": "Drop the operations Dalica cannot read (default false)"},
                }),
                &["id", "recipe"],
            ),
            output: Some(recipe_out),
            read_only: false,
            destructive: true,
            idempotent: true,
        },
        Tool {
            name: "apply_edit",
            title: "Apply edit",
            description: "Append one edit operation to each photo's recipe (default: the Photo Tray), like Batch Edit: one undo step for all the photos (cmd_edit_undo takes it back), with the edit view's rules: only the last crop and straighten count (the same one again takes no step), Tuning merges into a trailing tuning entry, a rotation after a rotation merges. `params` names the parameters as list_edit_ops gives them (left out: the default), or is the parameter text itself; a crop also takes rect: [x0, y0, x1, y1]. E.g. {op: \"autofix\"}, {op: \"crop\", params: {rect: [0.08, 0.1, 0.92, 0.86]}}, {op: \"tuning\", params: {fill: 0.3}}. Answers {entry, photos: [{id, recipe, changed}]}.".into(),
            input: object(
                json!({
                    "ids": {"type": "array", "items": {"type": "integer"}, "description": "Photo ids (default: the Photo Tray)"},
                    "op": {"type": "string", "description": "The operation's name (list_edit_ops), e.g. crop, autofix, tuning"},
                    "params": {"type": ["object", "string"], "description": "Its parameters by name, or the text between the parentheses"},
                }),
                &["op"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "entry": {"type": "string"}, "photos": {"type": "array", "items": {"type": "object"}},
            }, "required": ["entry", "photos"]})),
            read_only: false,
            destructive: false,
            idempotent: false,
        },
    ]
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    Some(match name {
        "list_edit_ops" => ToolResult::from(list_ops(args)),
        "get_recipe" => ToolResult::from(photo_id(args, "id").and_then(|id| get_recipe(b, id, progress))),
        "set_recipe" => ToolResult::from(set_recipe(b, args, progress)),
        "apply_edit" => ToolResult::from(apply_edit(b, args, progress)),
        _ => return None,
    })
}

// --- shared with `output` ---------------------------------------------------------------------

/// `engine.execute {command, params}`, waiting for any job it starts.
pub(super) fn run(b: &mut dyn Backend, command: &str, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    b.call("engine.execute", json!({ "command": command, "params": params }), progress)
}

pub(super) fn bad(message: impl Into<String>) -> ToolError {
    ToolError::new("bad_params", message)
}

/// Argument `key` as a photo id.
pub(super) fn photo_id(args: &Value, key: &str) -> Result<u64, ToolError> {
    match args.get(key) {
        Some(v) => v.as_u64().ok_or_else(|| ToolError::new("not_found", format!("no photo {v}"))),
        None => Err(bad(format!("missing `{key}`"))),
    }
}

/// What `photo.inspect` knows of the photos `ids` names (every one known, or `not_found`), or of
/// the Photo Tray's when `ids` is left out (the backend fills them in, as for every photo command).
pub(super) fn inspect(b: &mut dyn Backend, ids: Option<&Value>, progress: &mut dyn Progress) -> Result<Vec<Value>, ToolError> {
    let params = match ids.filter(|v| !v.is_null()) {
        Some(ids) => json!({ "ids": ids }),
        None => json!({}),
    };
    match run(b, "photo.inspect", params, progress)? {
        Value::Array(photos) => Ok(photos),
        other => Err(ToolError::new("internal", format!("photo.inspect answered {other}"))),
    }
}

/// The recipe of argument `recipe` (a `|`-joined string or a list of entries and `{op, params}`
/// items) as entries, for `render_photo`; `set_recipe` passes it to the engine as it is.
pub(super) fn recipe_entries(recipe: &Value) -> Result<Vec<String>, ToolError> {
    let too_many = || ToolError::new("limit", format!("a recipe holds at most {} operations", dalica_edit::MAX_OPERATIONS));
    let list: Vec<String> = match recipe {
        Value::String(t) if t.is_empty() => Vec::new(),
        Value::String(t) => t.split('|').map(str::to_string).collect(),
        Value::Array(items) => {
            if items.len() > dalica_edit::MAX_OPERATIONS {
                return Err(too_many());
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(t) => out.push(t.clone()),
                    Value::Object(o) if o.keys().all(|k| k == "op" || k == "params") => {
                        let Some(op) = o.get("op").and_then(Value::as_str) else {
                            return Err(bad("recipe: each {op, params} item needs `op` as text"));
                        };
                        let params = match o.get("params") {
                            None | Some(Value::Null) => "",
                            Some(Value::String(p)) => p.as_str(),
                            Some(other) => return Err(bad(format!("recipe: `params` is the text between the parentheses, not {other}"))),
                        };
                        out.push(format!("{op}({params})"));
                    }
                    other => return Err(bad(format!("recipe: an entry is text or {{op, params}}, not {other}"))),
                }
            }
            out
        }
        other => return Err(bad(format!("recipe: text or a list, not {other}"))),
    };
    if list.len() > dalica_edit::MAX_OPERATIONS {
        return Err(too_many());
    }
    Ok(list)
}

// --- list_edit_ops --------------------------------------------------------------------------

fn list_ops(args: &Value) -> Result<Value, ToolError> {
    let rows: Vec<&OpInfo> = match args.get("op").and_then(Value::as_str) {
        None => catalog::catalog().iter().collect(),
        Some(name) => vec![
            catalog::lookup(name)
                .ok_or_else(|| ToolError::new("not_found", format!("no edit operation `{name}` (list_edit_ops without `op` lists them)")))?,
        ],
    };
    Ok(json!({ "ops": rows.into_iter().map(op_json).collect::<Vec<_>>() }))
}

/// One catalogue row as JSON.
fn op_json(op: &OpInfo) -> Value {
    json!({
        "name": op.name,
        "label": op.label,
        "syntax": op.syntax,
        "params": op.params.iter().map(param_json).collect::<Vec<_>>(),
        "changes_size": op.changes_size,
        "milestone": op.milestone,
        "available": op.renders(),
        "default_entry": op.default_entry(),
        "note": op.note,
    })
}

fn param_json(p: &Param) -> Value {
    let mut v = json!({ "name": p.name, "label": p.label, "default": p.default });
    let kind = match p.kind {
        Kind::Number { min, max } => json!({ "kind": "number", "min": min, "max": max }),
        Kind::Open => json!({ "kind": "number", "range": "open (settled with the operation's renderer)" }),
        Kind::Choice(choices) => json!({ "kind": "choice", "choices": choices }),
        Kind::Colour { none } => json!({ "kind": "colour", "format": if none { "#rrggbb or none" } else { "#rrggbb" } }),
        Kind::Flag => json!({ "kind": "flag" }),
        Kind::Seconds => json!({ "kind": "seconds" }),
        Kind::Strokes { max, fields } => {
            json!({ "kind": "strokes", "max": max, "separator": ";", "fields": fields.iter().map(param_json).collect::<Vec<_>>() })
        }
    };
    if let (Some(o), Value::Object(k)) = (v.as_object_mut(), kind) {
        o.extend(k);
    }
    v
}

// --- get_recipe and set_recipe ----------------------------------------------------------------

/// The text between an entry's parentheses.
fn inner(entry: &str) -> &str {
    entry.split_once('(').map_or("", |(_, rest)| rest.strip_suffix(')').unwrap_or(rest))
}

/// `{op, params, entry, known}` of one recipe entry.
fn entry_json(entry: &str) -> Value {
    let op = EditOp::parse(entry);
    let name = entry.split_once('(').map_or(entry, |(name, _)| name);
    json!({ "op": op.name().unwrap_or(name), "params": inner(entry), "entry": entry, "known": op.is_known() })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

fn get_recipe(b: &mut dyn Backend, id: u64, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let state = run(b, "photo.history.state", json!({ "id": id }), progress)?;
    let edits = strings(&state["edits"]);
    Ok(json!({
        "id": id,
        "recipe": edits.join("|"),
        "ops": edits.iter().map(|e| entry_json(e)).collect::<Vec<_>>(),
        "redo": strings(&state["redo"]),
        "saved": state.get("undoSave") == Some(&Value::Bool(true)),
    }))
}

fn set_recipe(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let id = photo_id(args, "id")?;
    let mut p = Map::new();
    p.insert("ids".into(), json!([id]));
    p.insert("recipe".into(), args.get("recipe").cloned().unwrap_or(Value::Null));
    if let Some(l) = args.get("label").filter(|v| !v.is_null()) {
        p.insert("label".into(), l.clone());
    }
    if let Some(d) = args.get("drop_unknown").filter(|v| !v.is_null()) {
        p.insert("dropUnknown".into(), d.clone());
    }
    let reply = run(b, "edit.recipe.set", Value::Object(p), progress)?;
    let changed = reply.pointer("/photos/0/changed").cloned().unwrap_or(Value::Bool(false));
    let mut v = get_recipe(b, id, progress)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("changed".into(), changed);
    }
    Ok(v)
}

// --- apply_edit ---------------------------------------------------------------------------------

/// A parameter's value as the entry writes it.
fn field(name: &str, v: &Value) -> Result<String, ToolError> {
    match v {
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) if !s.contains([',', '(', ')', '|', ';']) => Ok(s.clone()),
        Value::Bool(on) => Ok(if *on { "1" } else { "0" }.into()),
        other => Err(bad(format!("params.{name}: a number, a word or true/false, not {other}"))),
    }
}

/// The entry operation `row` with `params` stands for, and the parameters given by name (Tuning's
/// merge sends only those).
fn entry_of(row: &OpInfo, params: Option<&Value>) -> Result<(String, Map<String, Value>), ToolError> {
    let given = match params {
        None | Some(Value::Null) => Map::new(),
        Some(Value::String(text)) => return Ok((format!("{}({text})", row.name), Map::new())),
        Some(Value::Object(o)) => o.clone(),
        Some(other) => return Err(bad(format!("params: an object or the parameter text, not {other}"))),
    };
    let mut named = given.clone();
    if row.name == "crop"
        && let Some(rect) = named.remove("rect")
    {
        let Some(corners) = rect.as_array().filter(|a| a.len() == 4 && a.iter().all(Value::is_number)) else {
            return Err(bad(format!("params.rect: [x0, y0, x1, y1], four numbers, not {rect}")));
        };
        for (p, v) in row.params.iter().zip(corners) {
            if named.insert(p.name.into(), v.clone()).is_some() {
                return Err(bad(format!("params: give rect or {}, not both", p.name)));
            }
        }
    }
    if let Some(k) = named.keys().find(|k| !row.params.iter().any(|p| p.name == k.as_str())) {
        let mut accepted: Vec<&str> = row.params.iter().map(|p| p.name).collect();
        if row.name == "crop" {
            accepted.push("rect");
        }
        let accepted = if accepted.is_empty() { "none".to_string() } else { accepted.join(", ") };
        return Err(bad(format!("unknown parameter `{k}` for {}; expected: {accepted}", row.name)));
    }
    let mut fields = Vec::with_capacity(row.params.len());
    for p in row.params {
        match named.get(p.name).filter(|v| !v.is_null()) {
            Some(v) => fields.push(field(p.name, v)?),
            None if p.default.is_empty() && !matches!(p.kind, Kind::Strokes { .. }) => {
                return Err(bad(format!("{} needs `{}` (it has no default yet)", row.name, p.name)));
            }
            None => fields.push(p.default.to_string()),
        }
    }
    Ok((format!("{}({})", row.name, fields.join(",")), given))
}

/// What `edits` ends with when `entry` (operation `row`) is appended by the edit view's rules
/// before `edit.apply` takes it: `Some(entry)` to apply, `None` when the photo would not change
/// (the same crop or straighten as its last one: only the last counts). Tuning and Fill Light
/// merge into a trailing `tuning` entry: the values given by name (Fill Light's is Tuning's
/// `fill`, `editing.md` §6.5) replace its values, the others stay; without a trailing `tuning` they
/// start from Tuning's defaults. Tuning given as parameter text replaces every value.
fn photo_entry(row: &OpInfo, entry: &str, given: &Map<String, Value>, edits: &[String]) -> Result<Option<String>, ToolError> {
    let name_of = |e: &str| EditOp::parse(e).name().map(str::to_string);
    match row.name {
        "crop" | "straighten" => {
            let last = edits.iter().rev().find(|e| name_of(e).as_deref() == Some(row.name));
            Ok((last.map(String::as_str) != Some(entry)).then(|| entry.to_string()))
        }
        "tuning" if given.is_empty() => Ok(Some(entry.to_string())),
        "tuning" | "fill_light" => {
            let tuning = catalog::lookup("tuning").ok_or_else(|| ToolError::new("internal", "the catalogue has no tuning"))?;
            let defaults: Vec<String> = tuning.params.iter().map(|p| p.default.to_string()).collect();
            let mut values = match edits.last().filter(|e| name_of(e).as_deref() == Some("tuning")) {
                Some(last) => {
                    let held: Vec<String> = inner(last).split(',').map(|t| t.trim().to_string()).collect();
                    if held.len() == defaults.len() { held } else { defaults }
                }
                None => defaults,
            };
            let overrides: Vec<(&str, String)> = if row.name == "fill_light" {
                vec![("fill", inner(entry).to_string())]
            } else {
                given.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| field(k, v).map(|t| (k.as_str(), t))).collect::<Result<_, _>>()?
            };
            for (name, text) in overrides {
                if let Some(slot) = tuning.params.iter().position(|p| p.name == name).and_then(|i| values.get_mut(i)) {
                    *slot = text;
                }
            }
            Ok(Some(format!("tuning({})", values.join(","))))
        }
        _ => Ok(Some(entry.to_string())),
    }
}

/// The photos `edit.apply` (or a group of them) changed, from its answer.
fn changed_ids(reply: &Value) -> HashSet<u64> {
    let one = |r: &Value| r.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>()).unwrap_or_default();
    match reply.get("results").and_then(Value::as_array) {
        Some(results) => results.iter().flat_map(one).collect(),
        None => one(reply).into_iter().collect(),
    }
}

fn apply_edit(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let name = args.get("op").and_then(Value::as_str).ok_or_else(|| bad("missing `op`"))?;
    let row = catalog::lookup(name).ok_or_else(|| bad(format!("no edit operation `{name}` (list_edit_ops lists them)")))?;
    let (entry, given) = entry_of(row, args.get("params"))?;
    if let Some(Err(e)) = EditOp::parse(&entry).typed() {
        return Err(bad(format!("{entry}: {e}")));
    }
    // Every photo known (or the tray's) before any changes.
    let photos = inspect(b, args.get("ids"), progress)?;
    let ids: Vec<u64> = photos.iter().filter_map(|p| p.get("id").and_then(Value::as_u64)).collect();
    if ids.is_empty() {
        return Err(ToolError::new("disabled", "Select some photos first, or pass `ids`."));
    }
    let total = u64::try_from(ids.len()).unwrap_or(u64::MAX);
    if !progress.report(0, total, &format!("Applying {} to {total} photo(s)", row.label)) {
        return Err(ToolError::new("cancelled", "the request was cancelled"));
    }
    if !row.renders() {
        return apply_unrendered(b, row, &entry, &ids, progress);
    }
    // The entry each photo takes; photos taking the same one share an `edit.apply`.
    let mut plan: Vec<(String, Vec<u64>)> = Vec::new();
    for (id, photo) in ids.iter().zip(&photos) {
        let Some(e) = photo_entry(row, &entry, &given, &strings(&photo["recipe"]["edits"]))? else { continue };
        match plan.iter_mut().find(|(have, _)| *have == e) {
            Some((_, list)) => list.push(*id),
            None => plan.push((e, vec![*id])),
        }
    }
    let mut steps: Vec<Value> = plan.into_iter().map(|(e, list)| json!({ "command": "edit.apply", "params": { "ids": list, "op": e } })).collect();
    // One library undo step for every photo (`mcp.md` §4.2): one `edit.apply`, or, when Tuning
    // merged into different trailing entries, one group of them.
    let reply = match steps.len() {
        0 => json!({ "ids": [] }),
        1 => {
            let step = steps.remove(0);
            run(b, "edit.apply", step["params"].clone(), &mut NoProgress)?
        }
        _ => b.call("engine.group", json!({ "steps": steps, "label": "Change Photos" }), &mut NoProgress)?,
    };
    let changed = changed_ids(&reply);
    let after = inspect(b, Some(&json!(ids)), &mut NoProgress)?;
    let done: Vec<Value> = after
        .iter()
        .filter_map(|p| {
            let id = p.get("id").and_then(Value::as_u64)?;
            Some(json!({ "id": id, "recipe": strings(&p["recipe"]["edits"]).join("|"), "changed": changed.contains(&id) }))
        })
        .collect();
    let _ = progress.report(total, total, "Done");
    Ok(json!({ "entry": entry, "photos": done }))
}

/// An operation this build cannot draw yet (Batch Edit greys it, "Arrives with Effects"): kept in
/// each recipe and skipped when drawn, as `list_edit_ops` says, through `photo.history.apply`, one
/// step of each photo's history (M10's way) until it renders and goes through `edit.apply`.
fn apply_unrendered(b: &mut dyn Backend, row: &OpInfo, entry: &str, ids: &[u64], progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let total = u64::try_from(ids.len()).unwrap_or(u64::MAX);
    let mut done = Vec::with_capacity(ids.len());
    for (n, id) in (0u64..).zip(ids) {
        if !progress.report(n, total, &format!("Applying {} to photo {} of {total}", row.label, n.saturating_add(1))) {
            return Err(ToolError::new("cancelled", "the request was cancelled"));
        }
        let state = run(b, "photo.history.apply", json!({ "id": id, "op": entry }), &mut NoProgress)?;
        let changed = state.get("changed").and_then(Value::as_bool).unwrap_or(true);
        done.push(json!({ "id": id, "recipe": strings(&state["edits"]).join("|"), "changed": changed }));
    }
    let _ = progress.report(total, total, "Done");
    Ok(json!({ "entry": entry, "photos": done }))
}
