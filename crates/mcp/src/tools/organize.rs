//! The organizing helpers of `mcp.md` §4.2: `scan_folder`, `query_photos`, `photo_info`,
//! `select_photos`, `set_metadata`, `create_album`, `jobs_list` and `jobs_cancel`. Photo tools act
//! on the Photo Tray when `ids` is omitted.
//!
//! Every helper is a thin composition of engine commands run through the [`Backend`]
//! (`engine.execute`), never a second implementation of one (`architecture.md` §3, one
//! implementation per output):
//!
//! | Tool | Engine commands and backend methods |
//! |---|---|
//! | `scan_folder` | `folders.set` (Scan Once) or `folders.add` (Scan Always), waiting for the scan job; then `jobs.list` (the job's result) and `library.inspect` (the folders under the path, their counts and problems) |
//! | `query_photos` | `library.query` (the search grammar and its filters), then `photo.inspect` and the `library.inspect` rows of the folders involved for each photo's fields |
//! | `photo_info` | `photo.inspect`, `photo.metadata` (asked again until the file has been read), `photo.history.state`, `places.list` (its geotag), `faces.list` (its faces), the photo's `library.inspect` row and `tray.state` |
//! | `select_photos` | `tray.select` |
//! | `set_metadata` | `engine.group` of `photo.star.set`, `photo.caption.set`, `photo.tags.add` / `.remove`, `photo.hide` / `.unhide`, `photo.geo.set`, `photo.dates.set`: one undo step |
//! | `create_album` | `album.create` (the tray's photos by default, from `tray.state`) |
//! | `jobs_list`, `jobs_cancel` | `jobs.list`, `jobs.cancel` |
//!
//! `query_photos` filters the engine has no condition for (`has_caption`, `edited`, `hidden`,
//! `video: false`, `geotagged: false`, `taken_after`, `taken_before`) and a `sort` other than the
//! light table's order are applied here to the fields the engine reported for every match (at most
//! [`MAX_MATCHES`]); without them only the page is looked up.
//!
//! **People data** (`mcp.md` §5, `faces.md` §7): `photo_info`'s faces come from `faces.list`
//! through the backend, which withholds names and rectangles without `--automation-allow-people`
//! (`Policy::people_reply`); this module adds none of its own. `query_photos {query:
//! "person:<name>"}` is the search box's grammar, answered by the engine.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::{Tool, ToolResult, object};
use crate::backend::{Backend, Progress, ToolError};

/// `query_photos`' default page.
const DEFAULT_LIMIT: u64 = 100;
/// `query_photos`' largest page (`mcp.md` §10).
const MAX_LIMIT: u64 = 1000;
/// The most matches `query_photos` filters or sorts itself (`library.query`'s largest page).
const MAX_MATCHES: u64 = 10_000;
/// A `library.inspect` page.
const INSPECT_PAGE: u64 = 1000;
/// The `library.inspect` pages one lookup reads at most (a folder of 10⁶ photos).
const MAX_PAGES: u64 = 1000;
/// How long `photo_info` waits for the file's metadata to be read.
const METADATA_WAIT: Duration = Duration::from_secs(10);
/// The pause between two asks for the file's metadata.
const METADATA_STEP: Duration = Duration::from_millis(10);

const IDS: &str = "Photo ids (default: the Photo Tray's photos)";

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "scan_folder",
            title: "Scan folder",
            description: "Add a folder to the watched folders (as the Folder Manager does) and scan it. `mode` is `once` (Scan Once, the default headless) or `always` (Scan Always, the default with the app attached). The path must be absolute and inside a read root. Waits for the scan (progress per batch of files when the request has a progress token) and answers {folder, section, photos, videos, skipped, problems}: the folder of the path itself (null when it holds no photos directly), the photos and videos in it and in the folders under it, the files the scan could not read, and the problems under the path. `wait: false` answers {path, pending: true} at once.".into(),
            input: object(
                json!({
                    "path": {"type": "string", "description": "Absolute folder path, inside a read root"},
                    "mode": {"type": "string", "enum": ["always", "once"], "description": "Scan Always or Scan Once"},
                    "wait": {"type": "boolean", "description": "Wait for the scan (default true)"},
                }),
                &["path"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "folder": {"type": ["integer", "null"]}, "section": {"type": ["string", "null"]}, "photos": {"type": "integer"},
                "videos": {"type": "integer"}, "skipped": {"type": "integer"}, "problems": {"type": "array"},
            }})),
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "query_photos",
            title: "Query photos",
            description: "Photos matching the light table's search and filters, without entering search mode. `query` uses the search box's grammar: words (all must match, as prefixes), `-word` (not), `tag:word`, `camera:word`. `tags` adds `tag:` terms (each word of each tag). Filters: `section` (folder:<id>, album:<id>, album:starred, album:recent), `starred`, `has_caption`, `edited` (unsaved edits), `hidden`, `video`, `geotagged`, `taken_after` / `taken_before` (YYYY, YYYY-MM, YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS, both inclusive, on the effective date). `sort` date | name | size | modified | manual (the light table's order, the default), `reverse`. Paged by `offset` and `limit` (100, at most 1000). `person:name` finds the photos of a person (names and nicknames, as prefixes). Answers {total, offset, photos: [{id, path, name, section, star, caption, tags, date, width, height, edited, hidden, video, geo, geotag}]} (geo: whether it is geotagged; geotag: [latitude, longitude] or null). Changes nothing.".into(),
            input: object(
                json!({
                    "query": {"type": "string", "description": "Search text (the search box's grammar)"},
                    "section": {"type": "string", "description": "Only this section's photos"},
                    "starred": {"type": "boolean"},
                    "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags every photo must have"},
                    "has_caption": {"type": "boolean"},
                    "edited": {"type": "boolean", "description": "Photos with (true) or without (false) unsaved edits"},
                    "hidden": {"type": "boolean"},
                    "video": {"type": "boolean"},
                    "geotagged": {"type": "boolean"},
                    "taken_after": {"type": "string", "description": "Effective date on or after (YYYY[-MM[-DD[THH:MM:SS]]])"},
                    "taken_before": {"type": "string", "description": "Effective date on or before (YYYY[-MM[-DD[THH:MM:SS]]])"},
                    "sort": {"type": "string", "enum": ["date", "name", "size", "modified", "manual"]},
                    "reverse": {"type": "boolean"},
                    "offset": {"type": "integer", "description": "Matches to skip (default 0)"},
                    "limit": {"type": "integer", "description": "Photos to list (default 100, at most 1000)"},
                }),
                &[],
            ),
            output: Some(json!({"type": "object", "properties": {
                "total": {"type": "integer"}, "offset": {"type": "integer"}, "photos": {"type": "array"},
            }, "required": ["total", "offset", "photos"]})),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "photo_info",
            title: "Photo info",
            description: "Everything Dalica knows about one photo: path, name, section, file size, dimensions (the file's and as displayed), dates (taken, set by the user, the file's, file modified, digitized), the EXIF summary, star, caption, tags, rotation, the recipe (string and list, and the redo list), albums, geotag ([latitude, longitude] or null: the location set in Dalica, else the file's GPS) and geo (whether it has one), faces ([{id, person, name, rect, displayed, ignored}]: names and rectangles only when the server was started with --automation-allow-people; without it names are null and rectangles left out), hidden, held in the Photo Tray, whether an originals backup is present, unsaved edits, video. Changes nothing.".into(),
            input: object(json!({ "id": {"type": "integer", "description": "Photo id"} }), &["id"]),
            output: Some(json!({"type": "object", "properties": {
                "id": {"type": "integer"}, "path": {"type": "string"}, "recipe": {"type": "object"},
            }, "required": ["id", "path", "recipe"]})),
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "select_photos",
            title: "Select photos",
            description: "Set the light table's selection (one section at a time: selecting in another section starts a new selection there) and optionally hold it in the Photo Tray, where held photos stay across sections. `mode` replace (default), add or remove. An unknown id is an error and leaves the selection unchanged. Answers {section, selection, held, tray}; photo tools run without `ids` act on `tray`.".into(),
            input: object(
                json!({
                    "ids": {"type": "array", "items": {"type": "integer"}, "description": "Photo ids"},
                    "section": {"type": "string", "description": "The section they are selected in (folder:<id>, album:<id>)"},
                    "mode": {"type": "string", "enum": ["replace", "add", "remove"]},
                    "hold": {"type": "boolean", "description": "Hold the selection in the Photo Tray"},
                }),
                &["ids"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "section": {"type": ["string", "null"]}, "selection": {"type": "array"}, "held": {"type": "array"}, "tray": {"type": "array"},
            }, "required": ["selection", "held", "tray"]})),
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "set_metadata",
            title: "Set metadata",
            description: "Change organizer data of photos as one undo step (one Edit ▸ Undo restores all of it): `star`, `caption` (\"\" clears it), `tags_add`, `tags_remove`, `hidden`, `geo` ([latitude, longitude] in degrees gives the photos that location; null removes their location, as Clear Location does), `date` (YYYY-MM-DDTHH:MM:SS, the date taken). Written to the sidecars and, per the Options (Write captions, tags and places), into the files' metadata: a location as EXIF GPS. Moving the location of more than 20 photos that already have one, or removing it from more than 5, asks first: `needs_confirm` until `confirm: true`. Answers {ids, changed, results}.".into(),
            input: object(
                json!({
                    "ids": {"type": "array", "items": {"type": "integer"}, "description": IDS},
                    "star": {"type": "boolean"},
                    "caption": {"type": "string"},
                    "tags_add": {"type": "array", "items": {"type": "string"}},
                    "tags_remove": {"type": "array", "items": {"type": "string"}},
                    "hidden": {"type": "boolean"},
                    "geo": {"type": ["array", "null"], "items": {"type": "number"}, "description": "[latitude, longitude] in degrees, or null to remove the location"},
                    "date": {"type": "string", "description": "Date taken, YYYY-MM-DDTHH:MM:SS"},
                    "confirm": {"type": "boolean", "description": "Go ahead where Dalica asks first (moving the location of more than 20 geotagged photos, removing it from more than 5)"},
                }),
                &[],
            ),
            output: None,
            read_only: false,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "create_album",
            title: "Create album",
            description: "A new album holding these photos in this order (default: the Photo Tray's photos), as Album Properties' OK makes it. Answers {album, section, name, count}.".into(),
            input: object(
                json!({
                    "name": {"type": "string", "description": "The album's name"},
                    "ids": {"type": "array", "items": {"type": "integer"}, "description": IDS},
                    "description": {"type": "string"},
                }),
                &["name"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "album": {"type": "string"}, "section": {"type": "string"}, "name": {"type": "string"}, "count": {"type": "integer"},
            }, "required": ["album", "section"]})),
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        Tool {
            name: "jobs_list",
            title: "List jobs",
            description: "Background jobs (scans, exports, saves, file operations): [{job, kind, label, state, progress, result?, error?}], as `jobs.list` gives them. Changes nothing.".into(),
            input: object(json!({}), &[]),
            output: None,
            read_only: true,
            destructive: false,
            idempotent: true,
        },
        Tool {
            name: "jobs_cancel",
            title: "Cancel jobs",
            description: "Cancel a background job (`job`), or every cancellable one: each stops before its next photo; photos done stay done. Answers {cancelled: [job…]}.".into(),
            input: object(json!({ "job": {"type": "integer", "description": "Job id (default: every cancellable job)"} }), &[]),
            output: None,
            read_only: false,
            destructive: false,
            idempotent: true,
        },
    ]
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    let r = match name {
        "scan_folder" => scan_folder(b, args, progress),
        "query_photos" => query_photos(b, args, progress),
        "photo_info" => photo_info(b, args, progress),
        "select_photos" => b.call("tray.select", given(args, &["ids", "section", "mode", "hold"]), progress),
        "set_metadata" => set_metadata(b, args, progress),
        "create_album" => create_album(b, args, progress),
        "jobs_list" => b.call("jobs.list", json!({}), progress),
        "jobs_cancel" => b.call("jobs.cancel", given(args, &["job"]), progress),
        _ => return None,
    };
    Some(ToolResult::from(r))
}

/// The keys of `args` among `keys` that are given and not `null`.
fn given(args: &Value, keys: &[&str]) -> Value {
    let mut o = Map::new();
    for k in keys {
        if let Some(v) = arg(args, k) {
            o.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(o)
}

/// Argument `key`, `null` counting as absent.
fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|v| !v.is_null())
}

fn bad(message: impl Into<String>) -> ToolError {
    ToolError::new("bad_params", message)
}

/// Runs engine command `command` with `params`.
fn exec(b: &mut dyn Backend, progress: &mut dyn Progress, command: &str, params: Value) -> Result<Value, ToolError> {
    b.call("engine.execute", json!({ "command": command, "params": params }), progress)
}

/// `ids` when given, else the Photo Tray's photos.
fn ids_or_tray(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Vec<Value>, ToolError> {
    if let Some(ids) = arg(args, "ids").and_then(Value::as_array) {
        return Ok(ids.clone());
    }
    let state = b.call("tray.state", json!({}), progress)?;
    Ok(state.get("tray").and_then(Value::as_array).cloned().unwrap_or_default())
}

// ---------------------------------------------------------------------------------------------
// scan_folder

fn scan_folder(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let path = arg(args, "path").and_then(Value::as_str).unwrap_or_default().to_string();
    let mode = match arg(args, "mode").and_then(Value::as_str) {
        Some(m) => m.to_string(),
        None if b.has_ui() => "always".to_string(),
        None => "once".to_string(),
    };
    let wait = arg(args, "wait").and_then(Value::as_bool).unwrap_or(true);
    let (command, params) = if mode == "always" {
        ("folders.add", json!({ "path": path }))
    } else {
        ("folders.set", json!({ "changes": [{ "path": path, "mode": mode }] }))
    };
    let reply = b.call("engine.execute", json!({ "command": command, "params": params, "wait": wait }), progress)?;
    if !wait {
        return Ok(json!({ "path": path, "pending": true, "reply": reply }));
    }
    let root: PathBuf = Path::new(&path).components().collect();
    let under = |p: &str| Path::new(p).starts_with(&root);

    // The files the scan of this path could not read: its job's `failed`.
    let jobs = b.call("jobs.list", json!({}), progress)?;
    let skipped = jobs
        .as_array()
        .into_iter()
        .flatten()
        .filter(|j| j.get("kind").and_then(Value::as_str) == Some("scan"))
        .filter(|j| j.pointer("/result/roots").and_then(Value::as_array).is_some_and(|r| r.iter().filter_map(Value::as_str).any(under)))
        .max_by_key(|j| j.get("job").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|j| j.pointer("/result/failed"))
        .map_or(0, |f| f.as_u64().or_else(|| f.as_array().map(|a| a.len() as u64)).unwrap_or(0));

    let inspected = exec(b, progress, "library.inspect", json!({}))?;
    let mut folder = Value::Null;
    let mut section = Value::Null;
    let mut sections = Vec::new();
    for row in folder_rows(&inspected) {
        let (Some(p), Some(s)) = (row.get("path").and_then(Value::as_str), row.get("section").and_then(Value::as_str)) else { continue };
        if !under(p) {
            continue;
        }
        if Path::new(p) == root {
            section = json!(s);
            folder = s.strip_prefix("folder:").and_then(|n| n.parse::<u64>().ok()).map_or(Value::Null, |n| json!(n));
        }
        sections.push(s.to_string());
    }
    let count_of = |s: &str| {
        inspected
            .get("sections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|x| x.get("section").and_then(Value::as_str) == Some(s))
            .and_then(|x| x.get("count").and_then(Value::as_u64))
            .unwrap_or(0)
    };
    let mut items = 0u64;
    let mut videos = 0u64;
    for s in &sections {
        items = items.saturating_add(count_of(s));
        for_each_listed(b, progress, s, |row| {
            if row.get("video") == Some(&Value::Bool(true)) {
                videos = videos.saturating_add(1);
            }
            true
        })?;
    }
    let problems: Vec<Value> = inspected
        .get("problems")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| p.get("path").and_then(Value::as_str).is_some_and(under))
        .cloned()
        .collect();
    Ok(json!({
        "folder": folder, "section": section, "photos": items.saturating_sub(videos), "videos": videos,
        "skipped": skipped, "problems": problems,
    }))
}

/// Every folder row of `library.inspect`'s tree.
fn folder_rows(inspected: &Value) -> impl Iterator<Item = &Value> {
    inspected
        .pointer("/tree/groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("rows").and_then(Value::as_array))
        .flatten()
        .filter(|r| r.get("section").and_then(Value::as_str).is_some_and(|s| s.starts_with("folder:")))
}

/// Calls `f` on each photo row `library.inspect` lists in `section`, page by page, until `f`
/// answers false or the rows end.
fn for_each_listed(b: &mut dyn Backend, progress: &mut dyn Progress, section: &str, mut f: impl FnMut(&Value) -> bool) -> Result<(), ToolError> {
    let mut offset = 0u64;
    for _ in 0..MAX_PAGES {
        let page = exec(b, progress, "library.inspect", json!({ "photos": section, "offset": offset, "limit": INSPECT_PAGE }))?;
        let rows = page.get("photos").and_then(Value::as_array).cloned().unwrap_or_default();
        for row in &rows {
            if !f(row) {
                return Ok(());
            }
        }
        let total = page.pointer("/page/total").and_then(Value::as_u64).unwrap_or(0);
        offset = offset.saturating_add(INSPECT_PAGE);
        if rows.is_empty() || offset >= total {
            return Ok(());
        }
    }
    Ok(())
}

/// The `library.inspect` rows of `ids`, read from their folders' sections (`folders` holds each
/// id's folder). A photo the view does not show (hidden while Hidden Pictures is off) has none.
fn listed_rows(b: &mut dyn Backend, progress: &mut dyn Progress, ids: &[u64], folders: &HashMap<u64, u64>) -> Result<HashMap<u64, Value>, ToolError> {
    let mut out = HashMap::new();
    let by_folder: BTreeSet<u64> = ids.iter().filter_map(|id| folders.get(id).copied()).collect();
    for f in by_folder {
        let mut wanted: BTreeSet<u64> = ids.iter().copied().filter(|id| folders.get(id) == Some(&f)).collect();
        for_each_listed(b, progress, &format!("folder:{f}"), |row| {
            if let Some(id) = row.get("id").and_then(Value::as_u64)
                && wanted.remove(&id)
            {
                out.insert(id, row.clone());
            }
            !wanted.is_empty()
        })?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// query_photos

/// A date bound: `YYYY`, `YYYY-MM`, `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS`.
fn date_bound(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
    let Some(text) = arg(args, key).and_then(Value::as_str) else { return Ok(None) };
    let shape = |pattern: &str| {
        text.len() == pattern.len() && text.bytes().zip(pattern.bytes()).all(|(c, p)| if p == b'9' { c.is_ascii_digit() } else { c == p })
    };
    if ["9999", "9999-99", "9999-99-99", "9999-99-99T99:99:99"].iter().any(|p| shape(p)) {
        Ok(Some(text.to_string()))
    } else {
        Err(bad(format!("`{key}` must be YYYY, YYYY-MM, YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS, not {text:?}")))
    }
}

/// One photo of a `query_photos` answer, from its `photo.inspect` entry and its `library.inspect`
/// row (`edited`, `video`, `geo`; without a row `edited` is whether it has edits, the others null).
fn photo_summary(info: &Value, row: Option<&Value>) -> Value {
    let from_row = |k: &str| row.and_then(|r| r.get(k)).cloned().unwrap_or(Value::Null);
    let edited = row
        .and_then(|r| r.get("edited"))
        .cloned()
        .unwrap_or_else(|| json!(info.pointer("/recipe/edits").and_then(Value::as_array).is_some_and(|e| !e.is_empty())));
    json!({
        "id": info["id"], "path": info["path"], "name": info["name"],
        "section": info.get("folder").and_then(Value::as_u64).map(|f| format!("folder:{f}")),
        "star": info["star"], "caption": info["caption"], "tags": info["tags"], "date": info["effectiveDate"],
        "width": info["width"], "height": info["height"], "edited": edited, "hidden": info["hidden"], "video": from_row("video"),
        "geo": from_row("geo"), "geotag": from_row("place"),
    })
}

/// Whether summary `p` passes the filters applied here.
fn passes(p: &Value, args: &Value, after: Option<&str>, before: Option<&str>) -> bool {
    let want = |k: &str| arg(args, k).and_then(Value::as_bool);
    let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    if want("has_caption").is_some_and(|w| w != p.get("caption").and_then(Value::as_str).is_some_and(|c| !c.is_empty())) {
        return false;
    }
    if want("edited").is_some_and(|w| w != flag("edited")) || want("hidden").is_some_and(|w| w != flag("hidden")) {
        return false;
    }
    if (want("video") == Some(false) && flag("video")) || (want("geotagged") == Some(false) && flag("geo")) {
        return false;
    }
    if after.is_some() || before.is_some() {
        let Some(date) = p.get("date").and_then(Value::as_str) else { return false };
        if after.is_some_and(|a| date < a) {
            return false;
        }
        if before.is_some_and(|b| date.get(..b.len()).unwrap_or(date) > b) {
            return false;
        }
    }
    true
}

fn query_photos(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let offset = arg(args, "offset").map_or(Some(0), Value::as_u64).ok_or_else(|| bad("`offset` must be a whole number"))?;
    let limit = arg(args, "limit").map_or(Some(DEFAULT_LIMIT), Value::as_u64).ok_or_else(|| bad("`limit` must be a whole number"))?;
    if limit > MAX_LIMIT {
        return Err(ToolError::new("limit", format!("`limit` is at most {MAX_LIMIT}, not {limit}")));
    }
    let after = date_bound(args, "taken_after")?;
    let before = date_bound(args, "taken_before")?;

    // What `library.query` answers itself.
    let mut text: Vec<String> = arg(args, "query").and_then(Value::as_str).map(|q| vec![q.to_string()]).unwrap_or_default();
    for tag in arg(args, "tags").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        text.extend(tag.split_whitespace().map(|w| format!("tag:{w}")));
    }
    let mut q = Map::new();
    if !text.is_empty() {
        q.insert("text".into(), json!(text.join(" ")));
    }
    for key in ["section", "starred"] {
        if let Some(v) = arg(args, key) {
            q.insert(key.into(), v.clone());
        }
    }
    let mut filters = Map::new();
    for (ours, theirs) in [("video", "videos"), ("geotagged", "geotagged")] {
        if arg(args, ours) == Some(&Value::Bool(true)) {
            filters.insert(theirs.into(), json!(true));
        }
    }
    if !filters.is_empty() {
        q.insert("filters".into(), Value::Object(filters));
    }

    let want = |k: &str| arg(args, k).and_then(Value::as_bool);
    let sort = arg(args, "sort").and_then(Value::as_str).filter(|s| *s != "manual");
    let reverse = want("reverse").unwrap_or(false);
    let here = sort.is_some()
        || reverse
        || after.is_some()
        || before.is_some()
        || ["has_caption", "edited", "hidden"].iter().any(|k| want(k).is_some())
        || want("video") == Some(false)
        || want("geotagged") == Some(false);

    if !here {
        q.insert("offset".into(), json!(offset));
        q.insert("limit".into(), json!(limit));
        let found = exec(b, progress, "library.query", Value::Object(q))?;
        let page = found.get("photos").and_then(Value::as_array).cloned().unwrap_or_default();
        let photos = summaries(b, progress, &page)?;
        let mut out = json!({ "total": found["total"], "offset": offset, "photos": photos });
        for k in ["truncated", "unavailable"] {
            if let Some(v) = found.get(k) {
                out[k] = v.clone();
            }
        }
        return Ok(out);
    }

    q.insert("limit".into(), json!(MAX_MATCHES));
    let found = exec(b, progress, "library.query", Value::Object(q))?;
    let total = found.get("total").and_then(Value::as_u64).unwrap_or(0);
    if total > MAX_MATCHES {
        return Err(ToolError::new(
            "limit",
            format!("{total} photos match; filtering or sorting here takes at most {MAX_MATCHES}: narrow the query or the section"),
        ));
    }
    let all = found.get("photos").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut photos: Vec<Value> = summaries(b, progress, &all)?.into_iter().filter(|p| passes(p, args, after.as_deref(), before.as_deref())).collect();
    if let Some(key) = sort {
        let field = match key {
            "date" => "date",
            "name" => "name",
            "size" => "size",
            _ => "mtime",
        };
        photos.sort_by(|x, y| order(&x[field], &y[field]));
    }
    if reverse {
        photos.reverse();
    }
    let total = photos.len();
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = usize::try_from(limit).unwrap_or(0);
    let page: Vec<Value> = photos.into_iter().skip(skip).take(take).collect();
    Ok(json!({ "total": total, "offset": offset, "photos": page }))
}

/// Orders two field values: numbers by value, text ignoring case, missing ones last.
fn order(x: &Value, y: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (x, y) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Greater,
        (_, Value::Null) => Ordering::Less,
        (Value::Number(a), Value::Number(b)) => a.as_f64().unwrap_or(0.0).total_cmp(&b.as_f64().unwrap_or(0.0)),
        (Value::String(a), Value::String(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (a, b) => a.to_string().cmp(&b.to_string()),
    }
}

/// The [`photo_summary`] of each `library.query` entry of `found`, in order, with the file's
/// `size` and `mtime` added (the sort keys).
fn summaries(b: &mut dyn Backend, progress: &mut dyn Progress, found: &[Value]) -> Result<Vec<Value>, ToolError> {
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<u64> = found.iter().filter_map(|p| p.get("id").and_then(Value::as_u64)).collect();
    let folders: HashMap<u64, u64> =
        found.iter().filter_map(|p| Some((p.get("id").and_then(Value::as_u64)?, p.get("folder").and_then(Value::as_u64)?))).collect();
    let infos = exec(b, progress, "photo.inspect", json!({ "ids": ids }))?;
    let rows = listed_rows(b, progress, &ids, &folders)?;
    Ok(infos
        .as_array()
        .into_iter()
        .flatten()
        .map(|info| {
            let id = info.get("id").and_then(Value::as_u64).unwrap_or(0);
            let mut p = photo_summary(info, rows.get(&id));
            p["size"] = info["size"].clone();
            p["mtime"] = info["mtime"].clone();
            p
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// photo_info

/// The EXIF summary's keys among `photo.metadata`'s Properties rows.
const EXIF_KEYS: &[&str] = &["camera", "lens", "exposure", "aperture", "iso", "focalLength", "flash", "orientation", "colorSpace"];

fn photo_info(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let Some(id) = arg(args, "id").and_then(Value::as_u64) else { return Err(bad("`id` must be a photo id")) };
    let info = exec(b, progress, "photo.inspect", json!({ "ids": [id] }))?;
    let Some(info) = info.as_array().and_then(|a| a.first()).cloned() else { return Err(ToolError::new("not_found", format!("no photo {id}"))) };
    let history = exec(b, progress, "photo.history.state", json!({ "id": id }))?;

    // The file's fields are read on a worker: ask until they are there.
    let start = Instant::now();
    let metadata = loop {
        let m = exec(b, progress, "photo.metadata", json!({ "id": id }))?;
        if m.get("pending") != Some(&Value::Bool(true)) || start.elapsed() >= METADATA_WAIT {
            break m;
        }
        std::thread::sleep(METADATA_STEP);
    };
    let row_value = |key: &str| {
        metadata
            .get("rows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|r| r.get("key").and_then(Value::as_str) == Some(key))
            .and_then(|r| r.get("value").cloned())
            .unwrap_or(Value::Null)
    };
    let exif: Map<String, Value> = EXIF_KEYS.iter().map(|k| ((*k).to_string(), row_value(k))).collect();

    let folder = info.get("folder").and_then(Value::as_u64).unwrap_or(0);
    let rows = listed_rows(b, progress, &[id], &HashMap::from([(id, folder)]))?;
    let row = rows.get(&id);
    let from_row = |k: &str| row.and_then(|r| r.get(k)).cloned().unwrap_or(Value::Null);
    let held = b.call("tray.state", json!({}), progress)?.get("held").and_then(Value::as_array).is_some_and(|h| h.contains(&json!(id)));

    let edits: Vec<String> =
        history.get("edits").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    // TODO(maintainer): no engine query says whether a photo with newer unsaved edits has an
    // originals backup (`photo.history.state` tells it only while every edit is saved), so
    // `originals` is null then; `photo.inspect` could carry the library's `can_revert`.
    let originals = if history.get("undoSave") == Some(&Value::Bool(true)) {
        json!(true)
    } else if edits.is_empty() {
        json!(false)
    } else {
        Value::Null
    };
    let unsaved = row.and_then(|r| r.get("edited")).cloned().unwrap_or_else(|| json!(!edits.is_empty()));
    // The geotag Dalica uses (the user's, else the file's GPS), whether or not the view shows the
    // photo; and its faces, whose names and rectangles the backend withholds without the people
    // permission. An app that has no such command yet answers null.
    let places = exec(b, progress, "places.list", json!({ "ids": [id] }))?;
    let geotag = places.pointer("/photos/0/geo").cloned().unwrap_or(Value::Null);
    let faces =
        exec(b, progress, "faces.list", json!({ "ids": [id], "ignored": true })).ok().and_then(|f| f.get("faces").cloned()).unwrap_or(Value::Null);
    Ok(json!({
        "id": id,
        "path": info["path"],
        "name": info["name"],
        "folder": folder,
        "section": format!("folder:{folder}"),
        "fileSize": info["size"],
        "dimensions": { "width": info["width"], "height": info["height"], "displayed": info["displayed"] },
        "dates": {
            "taken": info["effectiveDate"], "set": info["dateSet"], "file": info["date"],
            "modified": row_value("fileModified"), "digitized": row_value("dateDigitized"),
        },
        "exif": exif,
        "star": info["star"],
        "caption": info["caption"],
        "tags": info["tags"],
        "rotation": from_row("rotate"),
        "recipe": { "string": edits.join("|"), "list": edits, "redo": history["redo"] },
        "albums": info["albums"],
        "geo": !geotag.is_null(),
        "geotag": geotag,
        "faces": faces,
        "hidden": info["hidden"],
        "held": held,
        "originals": originals,
        "unsavedEdits": unsaved,
        "video": from_row("video"),
        "metadataPending": metadata.get("pending") == Some(&Value::Bool(true)),
    }))
}

// ---------------------------------------------------------------------------------------------
// set_metadata, create_album

fn set_metadata(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let ids = ids_or_tray(b, args, progress)?;
    if ids.is_empty() {
        return Err(ToolError::new("disabled", "Select some photos first, or pass `ids`."));
    }
    let mut steps = Vec::new();
    let mut changed = Vec::new();
    if let Some(on) = arg(args, "star") {
        steps.push(json!({ "command": "photo.star.set", "params": { "ids": ids, "on": on } }));
        changed.push("star");
    }
    if let Some(caption) = arg(args, "caption") {
        steps.push(json!({ "command": "photo.caption.set", "params": { "ids": ids, "caption": caption } }));
        changed.push("caption");
    }
    for (key, command) in [("tags_add", "photo.tags.add"), ("tags_remove", "photo.tags.remove")] {
        if let Some(tags) = arg(args, key).and_then(Value::as_array).filter(|t| !t.is_empty()) {
            steps.push(json!({ "command": command, "params": { "ids": ids, "tags": tags } }));
            changed.push(key);
        }
    }
    if let Some(hidden) = arg(args, "hidden").and_then(Value::as_bool) {
        // Show Again is disabled unless one of the photos is hidden: left out when none is.
        let infos = exec(b, progress, "photo.inspect", json!({ "ids": ids }))?;
        let any_hidden = infos.as_array().into_iter().flatten().any(|i| i.get("hidden") == Some(&Value::Bool(true)));
        if hidden || any_hidden {
            steps.push(json!({ "command": if hidden { "photo.hide" } else { "photo.unhide" }, "params": { "ids": ids } }));
        }
        changed.push("hidden");
    }
    // `geo: null` removes the location: present and null is not absent here.
    if let Some(geo) = args.get("geo") {
        let mut params = json!({ "ids": ids, "geo": geo });
        if let Some(confirm) = arg(args, "confirm") {
            params["confirm"] = confirm.clone();
        }
        steps.push(json!({ "command": "photo.geo.set", "params": params }));
        changed.push("geo");
    }
    if let Some(date) = arg(args, "date") {
        steps.push(json!({ "command": "photo.dates.set", "params": { "ids": ids, "set": date } }));
        changed.push("date");
    }
    if changed.is_empty() {
        return Err(bad("nothing to change: give star, caption, tags_add, tags_remove, hidden, geo or date"));
    }
    let results = if steps.is_empty() {
        Vec::new()
    } else {
        let r = b.call("engine.group", json!({ "steps": steps, "label": "Set Metadata" }), progress).map_err(|e| {
            if e.code != "needs_confirm" {
                return e;
            }
            // The question is the location's: set_metadata's own `confirm` answers it.
            ToolError { message: "this change asks first (`prompt` holds the question): give set_metadata \"confirm\": true to go ahead".into(), ..e }
        })?;
        r.get("results").and_then(Value::as_array).cloned().unwrap_or_default()
    };
    Ok(json!({ "ids": ids, "changed": changed, "results": results }))
}

fn create_album(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    let ids = ids_or_tray(b, args, progress)?;
    let mut params = given(args, &["name", "description"]);
    params["ids"] = json!(ids);
    let mut r = exec(b, progress, "album.create", params)?;
    if let Some(album) = r.get("album").map(|a| a.as_str().map_or_else(|| a.to_string(), String::from)) {
        r["section"] = json!(format!("album:{album}"));
    }
    Ok(r)
}
