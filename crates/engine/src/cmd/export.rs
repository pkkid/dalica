//! Export (`creating.md` §2, `data-model.md` §6): `export.defaults` and `export.run`, run as a file
//! job ([`crate::filejobs`]).
//!
//! - `export.defaults {ids? | section? | album?}` (query) answers what the Export dialog starts
//!   from: the location (the stored one, else `<Pictures>/Dalica/Exports`), the suggested folder
//!   name (the section's name, the folder's when every photo named lies in one, else
//!   "Export 2026-06-04"), the size, the quality, numbers, the watermark, Remove location, how many
//!   pictures there are and whether videos are among them.
//! - `export.run {ids | section | album, dir, folderName, numbers?, size?, quality?, watermark?,
//!   removeLocation?}` checks everything on the owner thread without touching the disk (the
//!   options, the photos, each photo's current recipe, so unsaved edits are exported, and its
//!   owned fields), remembers the options used (missing ones come from the settings), and starts
//!   an `export` file job ("Exporting 12 of 48"; Cancel finishes the photos being written and
//!   starts no other). Its reply is `{job, pending: true, total, videos}`.
//! - **The job.** The first item to run makes the export folder through `create::export::plan`
//!   (the location made when missing, a free name: `Lisbon`, else `Lisbon-001`) and marks it
//!   Exported in its new folder sidecar (`"project": "exported"`, written by the store's sidecar
//!   writer), so the scan that brings the folder in gives it the mark and a rebuild keeps it. Each
//!   item renders its photo through `create::export::export_one` (the one output pipeline) and
//!   writes it under a name that is never taken. Videos are skipped and counted (until M15
//!   decodes them); a file that cannot be read as a picture is skipped with its reason. When the
//!   job ends the runner scans the export folder, adding it as Scan Always when no watched folder
//!   covers it (M5.12's rules), so its photos appear; a folder the library already holds gets
//!   `Op::SetProjects` here instead. No undo step: an export is not undone.
//! - **The result** (`jobs.wait`, `jobs.list`) names the folder and the written, skipped and failed
//!   files: `{total, done, written: [path], skipped: [{file, reason}], failed: [{file, reason}],
//!   cancelled, folder, name}`; the session's file-job event carries the message ("Exported 48
//!   pictures") and the folder to show.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_create::export::{self, Destination, ExportOptions, Kind, Planned, QualityPreset, Size};
use dalica_edit::Recipe;
use dalica_library::{FolderInfo, Op, Project};
use dalica_meta::{Changes, DateTaken};
use dalica_store::mapping::FolderState;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;
use crate::settings::{CUSTOM_QUALITY_DEFAULT, EXPORT_SIZE_RANGE, ExportQuality, ExportSettings, MAX_PATH_BYTES, MAX_WATERMARK_BYTES, QUALITY_RANGE};

/// The most photos one export names.
pub const MAX_IDS: usize = 100_000;
/// The longest folder name taken, in bytes (it is sanitized and shortened to a file name after).
pub const MAX_FOLDER_NAME: usize = 4096;
/// The threads an export renders on. Each holds one full-size picture, and the JPEG encoder is
/// parallel itself, so two keep the cores busy without holding many pictures in memory.
pub const WORKERS: usize = 2;
/// The job's label: the progress reads "Exporting 12 of 48".
pub const LABEL: &str = "Exporting";
/// What the status line says when there is nothing to export.
const NOTHING: &str = "Select some pictures to export first.";
/// File name extensions of videos: skipped and counted until M15 decodes them.
const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "mkv"];
/// The name of the folder of kept originals: never an export location.
const ORIGINALS_DIR: &str = ".dalica-originals";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "export.defaults",
            label: "Export Defaults",
            menu: "",
            keys: &[],
            params: r#"{"ids"?:[PhotoId], "section"?:"folder:<FolderId>", "album"?:AlbumId (at most one)}: what the Export dialog starts from: {location: path|null, folderName, fallbackName, resize:bool, size, quality:"automatic"|"normal"|"maximum"|"minimum"|"custom", customQuality, numbers, watermark:bool, watermarkText, removeLocation, count, videos:bool}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: defaults,
        },
        CommandSpec {
            id: "export.run",
            label: "Export",
            menu: "",
            keys: &[],
            params: r#"{"ids"?:[PhotoId] (in tray order) | "section"?:"folder:<FolderId>" (its visible photos) | "album"?:AlbumId (exactly one of the three), "dir":string (absolute; the folder the export folder is made in, made when missing), "folderName":string (sanitized; a taken name becomes "<name>-001"), "numbers"?:bool, "size"?:100…4000|"original", "quality"?:"automatic"|"normal"|"maximum"|"minimum"|"custom"|1…100, "watermark"?:string (its text) | true (the stored text) | false | null, "removeLocation"?:bool}: renders the photos with their current edits into a new folder as an export job (no undo step); options left out come from the settings and the ones used are remembered. Replies {job, pending:true, total, videos}; the job's result is {total, done, written:[path], skipped:[{file, reason}], failed:[{file, reason}], cancelled, folder, name}"#,
            effect: Effect::Files,
            history: History::None,
            enabled: has_target,
            run,
        },
    ]
}

// --- what is exported ---------------------------------------------------------------------------

fn has_target(_: &Session, p: &Value) -> Result<(), Disabled> {
    let named = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let ids = p.get("ids").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    if ids || named("section") || named("album") { Ok(()) } else { Err(Disabled(NOTHING.into())) }
}

/// What an export names: photos in order, and the folder they came from, if one.
struct Target {
    photos: Vec<PhotoId>,
    /// The folder named as the section, or the one folder every photo named lies in.
    folder: Option<FolderId>,
}

/// `"folder:12"` → folder 12.
fn parse_section(text: &str) -> Result<FolderId, CmdError> {
    let id = text.strip_prefix("folder:").and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0);
    id.map(FolderId::new).ok_or_else(|| CmdError::BadParams(format!("section must be \"folder:<id>\", not {text:?}")))
}

/// The photos of `ids` (duplicates dropped, order kept), of `section` (its visible photos in
/// display order), or of `album`; at most one may be given.
fn target(s: &Session, ids: Option<Vec<PhotoId>>, section: Option<String>, album: Option<String>) -> Result<Target, CmdError> {
    let given = usize::from(ids.is_some()) + usize::from(section.is_some()) + usize::from(album.is_some());
    if given > 1 {
        return Err(CmdError::BadParams("name the photos by one of ids, section or album".into()));
    }
    if let Some(album) = album {
        // Albums arrive later (M10); until then no id names one.
        return Err(CmdError::NotFound(format!("no album {album:?}")));
    }
    let lib = s.library();
    if let Some(text) = section {
        let folder = parse_section(&text)?;
        if lib.folder(folder).is_none() {
            return Err(CmdError::NotFound(format!("no folder {folder}")));
        }
        let photos = lib.photos_in(folder).into_iter().filter(|p| !lib.info(p.id).hidden).map(|p| p.id).collect();
        return Ok(Target { photos, folder: Some(folder) });
    }
    let ids = ids.unwrap_or_default();
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    let mut photos = Vec::with_capacity(ids.len());
    let mut folders = HashSet::new();
    for id in ids {
        let p = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        folders.insert(p.folder);
        if seen.insert(id) {
            photos.push(id);
        }
    }
    let folder = if folders.len() == 1 { folders.into_iter().next() } else { None };
    Ok(Target { photos, folder })
}

/// Whether the photo's file is a video (by its extension, as the scan lists them).
fn is_video(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

/// "Export 2026-06-04": the export folder's name when nothing names it (the date in UTC).
fn fallback_name(now: Timestamp) -> String {
    match DateTime::from_unix_secs(now.as_secs()) {
        Some(d) => format!("Export {}", d.date()),
        None => "Export".into(),
    }
}

/// The suggested folder name: the section's (or the one folder's) name, else the fallback.
fn suggested_name(s: &Session, target: &Target, fallback: &str) -> String {
    target
        .folder
        .and_then(|f| s.library().folder(f))
        .and_then(|f| f.path.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

// --- export.defaults ----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultsParams {
    ids: Option<Vec<PhotoId>>,
    section: Option<String>,
    album: Option<String>,
}

fn defaults(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let DefaultsParams { ids, section, album } = params(p)?;
    let target = target(s, ids, section, album)?;
    let fallback = fallback_name(s.clock.now());
    let videos = target.photos.iter().filter_map(|id| s.library().photo(*id)).filter(|p| is_video(&p.name)).count();
    let e = s.settings().export();
    Ok(json!({
        "location": s.export_location(),
        "folderName": suggested_name(s, &target, &fallback),
        "fallbackName": fallback,
        "resize": e.resize,
        "size": e.size,
        "quality": e.quality.id(),
        "customQuality": e.custom_quality,
        "numbers": e.numbers,
        "watermark": e.watermark,
        "watermarkText": e.watermark_text,
        "removeLocation": e.remove_location,
        "count": target.photos.len(),
        "videos": videos > 0,
    }))
}

// --- export.run: the parameters -----------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RunParams {
    ids: Option<Vec<PhotoId>>,
    section: Option<String>,
    album: Option<String>,
    dir: PathBuf,
    folder_name: String,
    numbers: Option<bool>,
    size: Option<SizeParam>,
    quality: Option<QualityParam>,
    /// Kept raw: left out (the settings'), `null` or `false` (none), `true` (the stored text) or
    /// the text. (`Option<Value>` would read `null` as left out.)
    #[serde(default, deserialize_with = "present")]
    watermark: Option<Value>,
    remove_location: Option<bool>,
}

/// A key that is there, `null` included.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SizeParam {
    Pixels(u32),
    Word(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum QualityParam {
    Value(u8),
    Preset(String),
}

/// `dir` as an export location: absolute, without `..`, valid Unicode of at most
/// [`MAX_PATH_BYTES`], never inside a folder of kept originals.
fn check_dir(dir: &Path) -> Result<(), CmdError> {
    let text = dir.to_str().ok_or_else(|| CmdError::BadParams("dir: not valid Unicode".into()))?;
    if text.len() > MAX_PATH_BYTES {
        return Err(CmdError::Limit(format!("dir: at most {MAX_PATH_BYTES} bytes")));
    }
    if !dir.is_absolute() || text.contains('\0') {
        return Err(CmdError::BadParams(format!("dir: an absolute path, not {text:?}")));
    }
    if dir.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CmdError::BadParams("dir: `..` is not allowed".into()));
    }
    if dir.components().any(|c| c.as_os_str() == ORIGINALS_DIR) {
        return Err(CmdError::BadParams(format!("dir: never inside {ORIGINALS_DIR}")));
    }
    Ok(())
}

/// The options asked for, the settings filling in what was left out; returned with the settings
/// to remember.
fn options(p: &RunParams, stored: &ExportSettings) -> Result<(ExportOptions, ExportSettings), CmdError> {
    let mut e = stored.clone();
    if let Some(n) = p.numbers {
        e.numbers = n;
    }
    if let Some(r) = p.remove_location {
        e.remove_location = r;
    }
    match &p.size {
        None => {}
        Some(SizeParam::Pixels(n)) if EXPORT_SIZE_RANGE.contains(n) => (e.resize, e.size) = (true, *n),
        Some(SizeParam::Word(w)) if w == "original" => e.resize = false,
        Some(_) => {
            let (lo, hi) = (EXPORT_SIZE_RANGE.start(), EXPORT_SIZE_RANGE.end());
            return Err(CmdError::BadParams(format!("size: {lo}…{hi} pixels or \"original\"")));
        }
    }
    let preset = match &p.quality {
        Some(QualityParam::Preset(name)) => Some(ExportQuality::from_id(name)),
        _ => None,
    };
    match (&p.quality, preset) {
        (None, _) => {}
        (Some(QualityParam::Value(q)), _) if QUALITY_RANGE.contains(q) => (e.quality, e.custom_quality) = (ExportQuality::Custom, *q),
        (_, Some(Some(q))) => e.quality = q,
        _ => return Err(CmdError::BadParams("quality: \"automatic\", \"normal\", \"maximum\", \"minimum\", \"custom\" or 1…100".into())),
    }
    if !QUALITY_RANGE.contains(&e.custom_quality) {
        e.custom_quality = CUSTOM_QUALITY_DEFAULT;
    }
    match &p.watermark {
        None => {}
        Some(Value::Null | Value::Bool(false)) => e.watermark = false,
        Some(Value::Bool(true)) => e.watermark = true,
        Some(Value::String(text)) if text.len() <= MAX_WATERMARK_BYTES => (e.watermark, e.watermark_text) = (true, text.trim().to_string()),
        Some(Value::String(_)) => return Err(CmdError::Limit(format!("watermark: at most {MAX_WATERMARK_BYTES} bytes"))),
        Some(_) => return Err(CmdError::BadParams("watermark: its text, true, false or null".into())),
    }
    if e.watermark && e.watermark_text.trim().is_empty() {
        return Err(CmdError::BadParams("watermark: there is no text to stamp".into()));
    }
    let o = ExportOptions {
        size: if e.resize { Size::LongSide(e.size) } else { Size::Original },
        quality: match e.quality {
            ExportQuality::Automatic => QualityPreset::Automatic,
            ExportQuality::Normal => QualityPreset::Normal,
            ExportQuality::Maximum => QualityPreset::Maximum,
            ExportQuality::Minimum => QualityPreset::Minimum,
            ExportQuality::Custom => QualityPreset::Custom(e.custom_quality),
        },
        numbers: e.numbers,
        watermark: e.watermark.then(|| e.watermark_text.clone()),
        remove_location: e.remove_location,
    };
    o.check().map_err(CmdError::BadParams)?;
    Ok((o, e))
}

/// The library's owned fields of `id`, written into its exported file: the caption, tags, star
/// and a date the user set (the place stays as the file has it; `removeLocation` takes GPS out).
fn owned_changes(s: &Session, id: PhotoId, star: bool) -> Changes {
    let info = s.library().info(id);
    Changes {
        caption: Some((!info.caption.is_empty()).then(|| info.caption.clone())),
        tags: Some(info.tags.clone()),
        star: Some(star),
        place: None,
        date_taken: info.date.map(DateTaken::local),
    }
}

// --- export.run: the job ------------------------------------------------------------------------

/// What every item of one export shares: where the folder goes, and the folder once the first
/// item made it.
struct Shared {
    location: PathBuf,
    folder_name: String,
    fallback: String,
    options: ExportOptions,
    now: Timestamp,
    destination: OnceLock<Result<Destination, String>>,
}

impl Shared {
    /// The export folder, made (and marked) by the first item that asks; the others wait for it.
    fn destination(&self) -> Result<&Destination, String> {
        self.destination.get_or_init(|| self.make_folder()).as_ref().map_err(Clone::clone)
    }

    fn made(&self) -> Option<&Destination> {
        self.destination.get().and_then(|d| d.as_ref().ok())
    }

    /// Makes the export folder under a free name and writes its Exported mark. A mark that could
    /// not be written is logged: the folder is then marked when the job ends, if the library holds
    /// it by then.
    fn make_folder(&self) -> Result<Destination, String> {
        let plan = export::plan(&self.location, &self.folder_name, &self.fallback, Vec::new(), &self.options).map_err(|e| e.to_string())?;
        let state = FolderState {
            id: FolderId::new(0),
            path: plan.destination.folder.clone(),
            folder: FolderInfo::default(),
            photos: Vec::new(),
            project: Some(Project::Exported),
            ..FolderState::default()
        };
        match dalica_store::writer::write_folder(&state, self.now) {
            dalica_store::writer::Outcome::Written | dalica_store::writer::Outcome::Unchanged => {}
            other => log::warn!("{}: the Exported mark was not written: {other:?}", plan.destination.folder.display()),
        }
        Ok(plan.destination)
    }
}

/// One photo to export.
struct Item {
    planned: Planned,
    /// Why its recipe cannot be read, if it cannot: the item fails with it.
    bad_recipe: Option<String>,
    shared: Arc<Shared>,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.planned.source
    }
}

/// What became of one item.
enum Made {
    Written(PathBuf),
    Skipped(String),
}

/// One item, on a worker thread.
fn work(item: Item) -> Result<Done<Made>, String> {
    if let Some(reason) = item.bad_recipe {
        return Err(reason);
    }
    let destination = item.shared.destination()?;
    let mut changes = vec![FileChange::Folder { path: destination.folder.clone() }];
    // Cancel is the runner's, between items: a photo begun is finished.
    match export::export_one(&item.planned, destination, &|| false) {
        export::Outcome::Written(w) => {
            changes.push(FileChange::Created { path: w.path.clone() });
            Ok(Done { changes, value: Made::Written(w.path) })
        }
        export::Outcome::Skipped(reason) => Ok(Done { changes, value: Made::Skipped(reason) }),
        export::Outcome::Failed(reason) => Err(reason),
        export::Outcome::Cancelled => Err("cancelled".into()),
    }
}

/// What the items did, gathered on the owner thread as they are applied.
#[derive(Default)]
struct Report {
    written: Vec<PathBuf>,
    skipped: Vec<(PathBuf, String)>,
}

/// "Exported 48 pictures", "Exported 46 pictures; 2 skipped, 1 failed", "Export cancelled: 12 of
/// 48 pictures exported".
fn message(written: usize, total: usize, skipped: usize, failed: usize, cancelled: bool) -> String {
    let pictures = |n: usize| if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") };
    let head =
        if cancelled { format!("Export cancelled: {written} of {} exported", pictures(total)) } else { format!("Exported {}", pictures(written)) };
    let mut tail = Vec::new();
    if skipped > 0 {
        tail.push(format!("{skipped} skipped"));
    }
    if failed > 0 {
        tail.push(format!("{failed} failed"));
    }
    if tail.is_empty() { head } else { format!("{head}; {}", tail.join(", ")) }
}

fn run(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: RunParams = params(p)?;
    check_dir(&p.dir)?;
    if p.folder_name.len() > MAX_FOLDER_NAME {
        return Err(CmdError::Limit(format!("folderName: at most {MAX_FOLDER_NAME} bytes")));
    }
    let (options, remembered) = options(&p, &s.settings().export())?;
    let RunParams { ids, section, album, dir, folder_name, .. } = p;
    let target = target(s, ids, section, album)?;
    if target.photos.is_empty() {
        return Err(CmdError::Disabled(NOTHING.into()));
    }
    // Every photo's file, kind, current recipe and owned fields, in tray order.
    let mut items = Vec::with_capacity(target.photos.len());
    let mut bad = Vec::with_capacity(target.photos.len());
    for &id in &target.photos {
        let lib = s.library();
        let Some((photo, folder)) = lib.photo(id).and_then(|p| Some((p, lib.folder(p.folder)?))) else {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        };
        let (path, kind) = (folder.path.join(&photo.name), if is_video(&photo.name) { Kind::Video } else { Kind::Photo });
        let changes = owned_changes(s, id, photo.star);
        let (recipe, why) = match Recipe::parse(&lib.info(id).edits) {
            Ok(r) => (r, None),
            Err(e) => (Recipe::default(), Some(format!("cannot read its edits: {e}"))),
        };
        items.push(export::Item { path, kind, recipe, changes });
        bad.push(why);
    }
    let (planned, videos) = export::names(items, options.numbers);
    if planned.is_empty() {
        return Err(CmdError::Disabled("There are no pictures to export: videos are not exported yet.".into()));
    }
    let now = s.clock.now();
    // The options used are the next export's defaults; the default location stays unset.
    let default_location = s.pictures_dir.as_deref().map(crate::settings::default_export_location);
    let location = (default_location.as_deref() != Some(dir.as_path())).then(|| dir.clone());
    s.change_settings(|st| st.set_export(&ExportSettings { location, ..remembered }));

    let shared = Arc::new(Shared { location: dir, folder_name, fallback: fallback_name(now), options, now, destination: OnceLock::new() });
    let total = planned.len();
    let job_items: Vec<Item> = planned
        .into_iter()
        .map(|planned| {
            let bad_recipe = bad.get(planned.index).cloned().flatten();
            Item { planned, bad_recipe, shared: Arc::clone(&shared) }
        })
        .collect();
    let report = Arc::new(Mutex::new(Report { skipped: videos.iter().map(|v| (v.path.clone(), v.reason.clone())).collect(), ..Report::default() }));
    let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
    let (applied_report, finish_job) = (Arc::clone(&report), Arc::clone(&job_id));
    let job = FileJob::new(JobKind::Export, LABEL, job_items, work)
        .workers(WORKERS)
        .on_applied(move |_, a| {
            let mut r = applied_report.lock().unwrap_or_else(PoisonError::into_inner);
            match a.value {
                Made::Written(path) => r.written.push(path),
                Made::Skipped(reason) => r.skipped.push((a.file, reason)),
            }
            Ok(Vec::new())
        })
        .on_finish(move |s, tally| finish(s, tally, &shared, &report, finish_job.get().copied()));
    let id = s.start_file_job(job);
    let _ = job_id.set(id);
    Ok(json!({ "job": id, "pending": true, "total": total, "videos": videos.len() }))
}

/// The job's end (on the owner thread): the folder marked if the library already holds it, the
/// message, and the job closed with the whole result.
fn finish(s: &mut Session, tally: &Tally, shared: &Shared, report: &Mutex<Report>, job: Option<JobId>) -> Notice {
    let folder = shared.made().map(|d| d.folder.clone());
    if let Some(f) = &folder
        && let Some(id) = s.library().folder_by_path(f)
        && s.library().project(id) != Some(Project::Exported)
        && let Err(e) = s.apply(Op::SetProjects { projects: vec![(id, Some(Project::Exported))] })
    {
        log::warn!("{}: not marked Exported: {e}", f.display());
    }
    let r = report.lock().unwrap_or_else(PoisonError::into_inner);
    let text = message(r.written.len(), tally.total, r.skipped.len(), tally.failures.len(), tally.cancelled);
    // The job ends here with the whole result; the runner's own end after this is then left out
    // (a job keeps its first end). Without an id (no worker thread could start, and the runner
    // ended the job before `run` had it) the runner's failure stands.
    if let Some(job) = job {
        let result = json!({
            "total": tally.total,
            "done": r.written.len(),
            "written": r.written,
            "skipped": r.skipped.iter().map(|(file, reason)| json!({ "file": file, "reason": reason })).collect::<Vec<_>>(),
            "failed": tally.failures.iter().map(|f| json!({ "file": f.file, "reason": f.reason })).collect::<Vec<_>>(),
            "cancelled": tally.cancelled,
            "folder": folder,
            "name": shared.made().map(|d| d.name.clone()),
        });
        let applied = tally.done.saturating_add(tally.failures.len());
        s.jobs.progress(job, JobProgress { done: applied as u64, total: tally.total as u64 }, text.clone());
        let outcome = if tally.cancelled { Outcome::Cancelled(result) } else { Outcome::Done(result) };
        let now = s.clock.now();
        s.jobs.finish(job, outcome, text.clone(), now);
    }
    Notice { message: text, folder }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dalica_library::Photo;

    use super::*;
    use crate::cmd::testing::library;

    const PATIENCE: Duration = Duration::from_secs(120);

    #[test]
    fn messages_count_what_happened() {
        assert_eq!(message(48, 48, 0, 0, false), "Exported 48 pictures");
        assert_eq!(message(1, 1, 0, 0, false), "Exported 1 picture");
        assert_eq!(message(46, 47, 2, 1, false), "Exported 46 pictures; 2 skipped, 1 failed");
        assert_eq!(message(12, 48, 0, 0, true), "Export cancelled: 12 of 48 pictures exported");
    }

    #[test]
    fn videos_are_named_by_their_extension() {
        assert!(is_video("clip.MP4") && is_video("a.mov") && is_video("b.m4v") && is_video("c.mkv"));
        assert!(!is_video("a.jpg") && !is_video("mp4") && !is_video(""));
    }

    /// Videos are skipped and counted; an export of videos alone is refused.
    #[test]
    fn videos_are_skipped_and_counted() {
        let (dirs, mut s, ids) = library(2);
        let lisbon = dirs.photos.path().join("Lisbon");
        let folder = s.library().folder_by_path(&lisbon).unwrap();
        dalica_fs::replace(&lisbon.join("clip.mp4"), b"not decoded yet", dalica_fs::Expect::Absent).unwrap();
        let clip = s.library.next_photo_id();
        let video = Photo {
            id: clip,
            folder,
            name: "clip.mp4".into(),
            size: 15,
            mtime: Timestamp::from_millis(0),
            width: 0,
            height: 0,
            date: None,
            star: false,
        };
        s.apply(Op::AddPhotos { photos: vec![video] }).unwrap();
        let out = dirs.photos.path().join("Out");
        let only = s.execute("export.run", &json!({ "ids": [clip], "dir": out, "folderName": "Clips" }));
        assert!(matches!(only, Err(CmdError::Disabled(_))), "{only:?}");
        assert!(!out.exists(), "nothing made");
        let d = s.execute("export.defaults", &json!({ "ids": [ids[0], clip] })).unwrap();
        assert_eq!((d["count"].clone(), d["videos"].clone(), d["folderName"].clone()), (json!(2), json!(true), json!("Lisbon")));
        let reply = s.execute("export.run", &json!({ "ids": [ids[0], clip, ids[1]], "dir": out, "folderName": "Mixed", "size": 100 })).unwrap();
        assert_eq!((reply["total"].clone(), reply["videos"].clone()), (json!(2), json!(1)));
        let job = reply["job"].as_u64().unwrap();
        let done = s.wait_job(job, PATIENCE).unwrap();
        let result = done.result.unwrap();
        assert_eq!(result["done"], 2);
        assert_eq!(result["skipped"], json!([{ "file": lisbon.join("clip.mp4"), "reason": "videos are not exported yet" }]));
        assert_eq!(done.label, "Exported 2 pictures; 1 skipped");
        let mut names: Vec<String> =
            std::fs::read_dir(out.join("Mixed")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, [".dalica.json", "IMG_0001.jpg", "IMG_0002.jpg"]);
    }
}
