//! The output helpers of `mcp.md` §4.2: `render_photo` and `export`.
//!
//! - `render_photo {id?, size?, format?, before?, recipe?, path?}`: the photo (the Photo Tray's
//!   first without `id`) rendered through the one output pipeline (`create::output::render`, which
//!   renders by `edit::render`, as Save and Export do): with its stored recipe, with none
//!   (`before`), or with a `recipe` given to preview without applying it. The long edge is
//!   [`MIN_PREVIEW_SIDE`]–[`MAX_PREVIEW_SIDE`] (1024 by default), never enlarged; PNG (or JPEG
//!   when the PNG would pass [`MAX_PNG_BYTES`], unless `format: "png"` was asked, which is then a
//!   `limit`) or JPEG at quality 90. With `path` the encoded file is also written, as a new file
//!   (never over one) inside an automation write root and outside every watched folder (`mcp.md`
//!   §5: the default write roots until M10.16 adds the flags), and the answer's `path` names it
//!   in the form the caller wrote, not the resolved one. Nothing in the library changes:
//!   the photo and its recipe are read through `photo.inspect`, the file read under the export's
//!   size limit.
//! - `export {ids? | section? | album?, dir, folder_name?, size?, quality?, numbered?, watermark?,
//!   videos?, remove_location?}`: `export.run` with the Export dialog's options mapped (`size` 1600
//!   by default, `0` the original size; `quality` `auto` is `automatic`; `folder_name` by default
//!   the one `export.defaults` suggests; without a target, the Photo Tray's photos), waiting for
//!   its job with progress and cancellation per photo (the backend's). `videos` is refused until
//!   M15 exports them. The result names each file written with the photo it came from (matched
//!   by the new name `create::export::names` gives it), its size in pixels and in bytes.

use std::path::{Path, PathBuf};

use dalica_create::export;
use dalica_create::output::{self, Choice, OutFormat, Purpose, Quality, Spec};
use dalica_edit::Recipe;
use serde_json::{Map, Value, json};

use super::core::{MAX_PNG_BYTES, MAX_PREVIEW_SIDE, MIN_PREVIEW_SIDE};
use super::edit::{bad, inspect, photo_id, recipe_entries, run};
use super::{Tool, ToolResult, object};
use crate::backend::{Backend, NoProgress, Progress, ToolError};
use crate::roots::{Output, Roots};

/// `render_photo`'s default long edge.
pub const DEFAULT_RENDER_SIDE: u64 = 1024;
/// The JPEG quality `render_photo` writes.
pub const RENDER_QUALITY: u8 = 90;
/// `export`'s default long edge (`creating.md` §2).
pub const DEFAULT_EXPORT_SIDE: u64 = 1600;
/// The most photos an export's files are matched against (pages of `library.inspect`).
const MAX_MATCHED: usize = 100_000;
/// One page of `library.inspect` photos.
const PAGE: u64 = 1000;

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "render_photo",
            title: "Render photo",
            description: format!(
                "An image of a photo rendered by the pipeline Save and Export use: with its edits, without them (before: true), or with a recipe to preview without applying it. size is the long edge ({DEFAULT_RENDER_SIDE} by default, {MIN_PREVIEW_SIDE}–{MAX_PREVIEW_SIDE}), never enlarged. format png (JPEG instead when the PNG would pass 5 MiB, unless png was asked) or jpeg. With path (absolute, a new file inside an automation write root such as the temporary folder, never in a watched folder) the file is written too. Answers the image and {{id, width, height, mimeType, recipe, path?}}. Changes nothing in the library."
            ),
            input: object(
                json!({
                    "id": {"type": "integer", "description": "Photo id (default: the first photo in the Photo Tray)"},
                    "size": {"type": "integer", "description": "Long edge in pixels"},
                    "format": {"type": "string", "enum": ["png", "jpeg"], "description": "png (default) or jpeg"},
                    "before": {"type": "boolean", "description": "The unedited original (default false)"},
                    "recipe": {"type": ["string", "array"], "description": "Preview this recipe instead of the stored one (as set_recipe takes it), without applying it"},
                    "path": {"type": "string", "description": "Also write the file here (absolute)"},
                }),
                &[],
            ),
            output: Some(json!({"type": "object", "properties": {
                "id": {"type": "integer"}, "width": {"type": "integer"}, "height": {"type": "integer"},
                "mimeType": {"type": "string"}, "recipe": {"type": "array", "items": {"type": "string"}}, "path": {"type": "string"},
            }, "required": ["id", "width", "height", "mimeType", "recipe"]})),
            read_only: false,
            destructive: false,
            idempotent: false,
        },
        Tool {
            name: "export",
            title: "Export",
            description: format!(
                "Export as the Export dialog does: photos by ids, section (folder:<id>, album:<id>, album:starred, album:recent, album:search) or album (an album id, starred, recent, search), else the Photo Tray, rendered with their edits into a new folder under dir (folder_name, or the name the dialog suggests; a taken name gets -001), never overwriting, never enlarging, metadata copied. size is the long edge ({DEFAULT_EXPORT_SIDE} by default, 100–4000; 0 the original size); quality auto, normal, maximum, minimum or 1–100. Reports progress per photo; a cancel stops before the next photo and keeps the files done. Answers {{folder, name, files: [{{id, path, width, height, bytes}}], skipped, failed, cancelled, total, done}}."
            ),
            input: object(
                json!({
                    "ids": {"type": "array", "items": {"type": "integer"}, "description": "Photo ids, in this order"},
                    "section": {"type": "string", "description": "A light-table section, in its display order"},
                    "album": {"type": "string", "description": "An album id, starred, recent or search"},
                    "dir": {"type": "string", "description": "The folder the export folder is made in (absolute; a write root or a watched folder)"},
                    "folder_name": {"type": "string", "description": "The export folder's name"},
                    "size": {"type": "integer", "description": "Long edge in pixels (default 1600; 0: original size)"},
                    "quality": {"type": ["string", "integer"], "description": "auto, normal, maximum, minimum, or 1–100"},
                    "numbered": {"type": "boolean", "description": "Number the files in order (001-…)"},
                    "watermark": {"type": ["string", "boolean"], "description": "The watermark's text, true (the stored text) or false"},
                    "videos": {"type": "string", "enum": ["frame", "full"], "description": "How videos are exported (not built until M15: refused)"},
                    "remove_location": {"type": "boolean", "description": "Remove GPS coordinates from the files"},
                }),
                &["dir"],
            ),
            output: Some(json!({"type": "object", "properties": {
                "folder": {"type": ["string", "null"]}, "files": {"type": "array", "items": {"type": "object"}},
                "skipped": {"type": "array"}, "failed": {"type": "array"}, "cancelled": {"type": "boolean"},
            }, "required": ["folder", "files", "skipped", "failed", "cancelled"]})),
            read_only: false,
            destructive: false,
            idempotent: false,
        },
    ]
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    Some(match name {
        "render_photo" => render_photo(b, args),
        "export" => ToolResult::from(export(b, args, progress)),
        _ => return None,
    })
}

// --- render_photo -------------------------------------------------------------------------------

/// The watched folders, as `library.inspect` names them.
fn watched(b: &mut dyn Backend) -> Result<Vec<PathBuf>, ToolError> {
    let lib = run(b, "library.inspect", json!({}), &mut NoProgress)?;
    Ok(lib["watched"].as_array().into_iter().flatten().filter_map(|w| w["path"].as_str()).map(PathBuf::from).collect())
}

/// Where `render_photo` may write `path`: a new file inside a write root, outside every watched
/// folder. Returns the resolved folder (the one written in), the file name, and the folder as the
/// caller wrote it (the one the answer names).
fn render_target(b: &mut dyn Backend, path: &str) -> Result<(PathBuf, String, PathBuf), ToolError> {
    let target = Roots::default().check_output(Path::new(path), "path", &[], Output::File)?;
    let watched: Vec<PathBuf> =
        watched(b)?.iter().filter_map(|w| std::fs::canonicalize(w).ok()).map(|w| dalica_engine::automation::plain(&w)).collect();
    if watched.iter().any(|w| target.starts_with(w)) {
        return Err(ToolError::new("path_denied", format!("{path} is inside a watched folder; a render is not a photo of the library")));
    }
    match (target.parent(), target.file_name().and_then(|n| n.to_str()), Path::new(path).parent()) {
        (Some(folder), Some(name), Some(asked)) => Ok((folder.to_path_buf(), name.to_string(), asked.to_path_buf())),
        _ => Err(bad(format!("{path} names no file"))),
    }
}

fn render_photo(b: &mut dyn Backend, args: &Value) -> ToolResult {
    match render(b, args) {
        Ok((bytes, mime, meta)) => ToolResult::image(&bytes, mime, meta),
        Err(e) => ToolResult::error(&e),
    }
}

fn render(b: &mut dyn Backend, args: &Value) -> Result<(Vec<u8>, &'static str, Value), ToolError> {
    let side = match args.get("size") {
        None => DEFAULT_RENDER_SIDE,
        Some(v) => v
            .as_u64()
            .filter(|s| (MIN_PREVIEW_SIDE..=MAX_PREVIEW_SIDE).contains(s))
            .ok_or_else(|| ToolError::new("limit", format!("size is {MIN_PREVIEW_SIDE}–{MAX_PREVIEW_SIDE} pixels, not {v}")))?,
    };
    let before = args.get("before").and_then(Value::as_bool).unwrap_or(false);
    let asked = args.get("recipe").filter(|v| !v.is_null());
    if before && asked.is_some() {
        return Err(bad("give before or recipe, not both"));
    }
    let format = args.get("format").and_then(Value::as_str);
    let target = args.get("path").and_then(Value::as_str).map(|p| render_target(b, p)).transpose()?;

    let ids = match args.get("id") {
        Some(_) => Some(json!([photo_id(args, "id")?])),
        None => None,
    };
    let photos = inspect(b, ids.as_ref(), &mut NoProgress)?;
    let Some(photo) = photos.first() else { return Err(ToolError::new("disabled", "Select a photo first, or pass `id`.")) };
    let id = photo["id"].as_u64().unwrap_or_default();
    let entries = match (before, asked) {
        (true, _) => Vec::new(),
        (false, Some(r)) => recipe_entries(r)?,
        (false, None) => {
            photo.pointer("/recipe/edits").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
        }
    };
    let recipe = Recipe::parse(&entries).map_err(|e| ToolError::new("limit", format!("the recipe cannot be read: {e}")))?;
    let path = PathBuf::from(photo["path"].as_str().unwrap_or_default());
    let source = match dalica_fs::read(&path, export::MAX_FILE) {
        Ok(Some(read)) => read.bytes,
        Ok(None) => return Err(ToolError::new("not_found", format!("photo {id}'s file {} is missing", path.display()))),
        Err(e) => return Err(ToolError::new("not_found", format!("photo {id}'s file cannot be read: {e}"))),
    };

    let mut spec = Spec::new(Purpose::Export);
    spec.long_side = Some(u32::try_from(side).unwrap_or(u32::MAX));
    let fail = |e: output::Error| ToolError::new("unsupported", format!("photo {id} cannot be rendered: {}", e.reason()));
    let jpeg = |spec: &mut Spec| {
        spec.format = Choice::Exactly(OutFormat::Jpeg);
        spec.quality = Quality::Fixed(RENDER_QUALITY);
    };
    let mut mime = "image/png";
    if format == Some("jpeg") {
        jpeg(&mut spec);
        mime = "image/jpeg";
    } else {
        spec.format = Choice::Exactly(OutFormat::Png);
    }
    let mut out = output::render(&source, &recipe, &spec).map_err(fail)?;
    if mime == "image/png" && out.bytes.len() > MAX_PNG_BYTES {
        if format == Some("png") {
            return Err(ToolError::new(
                "limit",
                format!("the PNG is {} bytes, over the {MAX_PNG_BYTES}-byte limit; ask for a smaller size or format jpeg", out.bytes.len()),
            ));
        }
        jpeg(&mut spec);
        out = output::render(&source, &recipe, &spec).map_err(fail)?;
        mime = "image/jpeg";
    }
    let mut meta =
        json!({ "id": id, "width": out.report.width, "height": out.report.height, "mimeType": mime, "recipe": recipe.print(), "before": before });
    if let Some((folder, name, asked)) = target {
        let created =
            dalica_fs::create::new_file(&folder, &name, &out.bytes).map_err(|e| ToolError::new("io", format!("the render was not written: {e}")))?;
        // The answer names the file in the caller's own form (absolute, without `..`, checked
        // above), not the resolved one: that is `/private/var/…` for `/var/…` on macOS and a
        // verbatim `\\?\C:\…` on Windows, which an agent would then show or reuse.
        if let Some(o) = meta.as_object_mut() {
            o.insert("path".into(), json!(asked.join(created.name)));
        }
    }
    Ok((out.bytes, mime, meta))
}

// --- export -------------------------------------------------------------------------------------

/// `export.run`'s params from the tool's arguments; `ids` given when the export is the tray's.
fn run_params(args: &Value, tray: Option<Vec<u64>>, folder_name: Value) -> Map<String, Value> {
    let mut p = Map::new();
    for key in ["ids", "section", "album"] {
        if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
            p.insert(key.into(), v.clone());
        }
    }
    if let Some(ids) = tray {
        p.insert("ids".into(), json!(ids));
    }
    p.insert("dir".into(), args.get("dir").cloned().unwrap_or(Value::Null));
    p.insert("folderName".into(), folder_name);
    let size = match args.get("size").filter(|v| !v.is_null()) {
        None => json!(DEFAULT_EXPORT_SIDE),
        Some(v) if v.as_u64() == Some(0) => json!("original"),
        Some(v) => v.clone(),
    };
    p.insert("size".into(), size);
    if let Some(q) = args.get("quality").filter(|v| !v.is_null()) {
        p.insert("quality".into(), if q == "auto" { json!("automatic") } else { q.clone() });
    }
    if let Some(n) = args.get("numbered").filter(|v| !v.is_null()) {
        p.insert("numbers".into(), n.clone());
    }
    if let Some(w) = args.get("watermark") {
        p.insert("watermark".into(), w.clone());
    }
    if let Some(r) = args.get("remove_location").filter(|v| !v.is_null()) {
        p.insert("removeLocation".into(), r.clone());
    }
    p
}

/// The photos an export named, `(id, path)` in its order, to tell which file came from which.
fn candidates(b: &mut dyn Backend, args: &Value, tray: Option<&[u64]>) -> Result<Vec<(u64, PathBuf, bool)>, ToolError> {
    let of = |p: &Value| Some((p["id"].as_u64()?, PathBuf::from(p["path"].as_str()?), p["video"].as_bool().unwrap_or(false)));
    let section = match (args.get("section").and_then(Value::as_str), args.get("album").and_then(Value::as_str)) {
        (Some(s), _) => Some(s.to_string()),
        (None, Some(a)) => Some(format!("album:{a}")),
        (None, None) => None,
    };
    let Some(section) = section else {
        let ids = tray.map(|t| json!(t)).or_else(|| args.get("ids").cloned());
        return Ok(inspect(b, ids.as_ref(), &mut NoProgress)?.iter().filter_map(of).collect());
    };
    let mut out = Vec::new();
    let mut offset = 0u64;
    while out.len() < MAX_MATCHED {
        let page = run(b, "library.inspect", json!({ "photos": section, "offset": offset, "limit": PAGE }), &mut NoProgress)?;
        let photos = page["photos"].as_array().cloned().unwrap_or_default();
        out.extend(photos.iter().filter_map(of));
        offset = offset.saturating_add(PAGE);
        if photos.is_empty() || offset >= page.pointer("/page/total").and_then(Value::as_u64).unwrap_or(0) {
            break;
        }
    }
    Ok(out)
}

/// Whether `file`'s stem is `stem` or `stem` with the `-001` a taken name gets.
fn named_from(file: &Path, stem: &str) -> bool {
    let Some(have) = file.file_stem().and_then(|s| s.to_str()) else { return false };
    have == stem
        || have.strip_prefix(stem).and_then(|rest| rest.strip_prefix('-')).is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// Each written file as `{id, path, width, height, bytes}`: the photo it came from by the name
/// the export planned for it (`null` when no photo matches), its size read from its header.
fn files(written: &[PathBuf], photos: Vec<(u64, PathBuf, bool)>, numbers: bool) -> Vec<Value> {
    let ids: Vec<u64> = photos.iter().map(|(id, _, _)| *id).collect();
    let items: Vec<export::Item> = photos
        .into_iter()
        .map(|(_, path, video)| export::Item {
            path,
            kind: if video { export::Kind::Video } else { export::Kind::Photo },
            recipe: Recipe::default(),
            changes: Default::default(),
        })
        .collect();
    let (mut planned, _) = export::names(items, numbers);
    written
        .iter()
        .map(|path| {
            let at = planned.iter().position(|p| named_from(path, &p.stem));
            let id = at.map(|i| planned.remove(i)).and_then(|p| ids.get(p.index).copied());
            let header = dalica_image::read_header_file(path, &dalica_image::Limits::default()).ok().map(|(h, _)| h);
            let bytes = std::fs::metadata(path).ok().map(|m| m.len());
            json!({ "id": id, "path": path, "width": header.as_ref().map(|h| h.width), "height": header.as_ref().map(|h| h.height), "bytes": bytes })
        })
        .collect()
}

fn export(b: &mut dyn Backend, args: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
    if args.get("videos").is_some_and(|v| !v.is_null()) {
        return Err(ToolError::new("unsupported", "`videos`: videos are not exported until M15 (they are skipped and counted)"));
    }
    let named = ["ids", "section", "album"].iter().any(|k| args.get(*k).is_some_and(|v| !v.is_null()));
    let tray = if named { None } else { Some(inspect(b, None, &mut NoProgress)?.iter().filter_map(|p| p["id"].as_u64()).collect::<Vec<u64>>()) };
    let folder_name = match args.get("folder_name").filter(|v| !v.is_null()) {
        Some(n) => n.clone(),
        None => {
            let mut target = run_params(args, tray.clone(), Value::Null);
            target.retain(|k, _| ["ids", "section", "album"].contains(&k.as_str()));
            run(b, "export.defaults", Value::Object(target), &mut NoProgress)?.get("folderName").cloned().unwrap_or(Value::Null)
        }
    };
    let params = run_params(args, tray.clone(), folder_name);
    let numbers = args.get("numbered").and_then(Value::as_bool).unwrap_or(false);
    let entry = run(b, "export.run", Value::Object(params), progress)?;
    let result = entry.get("result").cloned().unwrap_or(Value::Null);
    let written: Vec<PathBuf> = result["written"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
    let photos = candidates(b, args, tray.as_deref())?;
    Ok(json!({
        "folder": result.get("folder").cloned().unwrap_or(Value::Null),
        "name": result.get("name").cloned().unwrap_or(Value::Null),
        "files": files(&written, photos, numbers),
        "skipped": result.get("skipped").cloned().unwrap_or_else(|| json!([])),
        "failed": result.get("failed").cloned().unwrap_or_else(|| json!([])),
        "cancelled": result.get("cancelled").cloned().unwrap_or(Value::Bool(false)),
        "total": result.get("total").cloned().unwrap_or(Value::Null),
        "done": result.get("done").cloned().unwrap_or(Value::Null),
    }))
}
