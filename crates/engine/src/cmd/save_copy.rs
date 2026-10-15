//! Save a Copy and Save As (`data-model.md` §6): `photo.saveCopy` and `photo.saveTo`, run as file
//! jobs ([`crate::filejobs`]) through `create::save::copy`, the one output pipeline at Match
//! quality. Neither touches the photo, its edits or its originals record.
//!
//! - `photo.saveCopy {ids}` (File ▸ Save a Copy) writes each photo that has edits as
//!   `<stem>-001.jpg` (the first free number) in its own folder, with its edits applied; photos
//!   without edits are left out. The copy joins the library as a new, unedited photo.
//! - `photo.saveTo {id, path, format?}` (the engine's half of File ▸ Save As…, which the UI's
//!   `photo.saveAs` runs with the path its save picker chose) writes the photo to `path` as JPEG or
//!   PNG: `format` (`"jpeg"` or `"png"`) or the path's extension decides; a name without a known
//!   extension takes the format's (`format`, else the last one used). A relative path is taken
//!   from the photo's folder. It never replaces a file: a path that holds one fails with code
//!   `exists` ("choose another name") and changes nothing; the folder must exist (code
//!   `no_folder`). A folder no watched folder covers is added as Scan Always when the job ends, so
//!   the new file appears; a folder under one is scanned. On success the folder and the format are
//!   remembered (`"save"` → `lastFolder`, `lastFormat`) for the next Save As.
//! - **The copy's owned fields.** The file carries all of the source's metadata and every owned
//!   field (caption, tags, star as a rating, the user's date), whatever the write-back options
//!   say. A copy written into a folder of the library is added from its header by the runner and
//!   then given the source's caption, tags, star and user date by ops (no undo step); a copy whose
//!   folder is scanned gets them from the file, as any new file does.
//!
//! Every check that needs no disk runs on the owner thread before the job starts (unknown photos,
//! offline folders, edits Dalica cannot render, a Save of the photo left unfinished, the path's
//! shape); the disk is touched only on the job's worker. One worker: two copies of photos with the
//! same stem would race for the same numbered name. Cancel stops between photos. The replies are
//! `{job, pending: true, total}` (Save As adds the `path` and `format` it will write); the job's
//! result is `{total, done, written: [path], photos: [PhotoId], failed: [{file, reason, code}],
//! cancelled}`, `code` being `exists`, `no_folder` or `failed`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use dalica_core::{DateTime, PhotoId, Timestamp};
use dalica_create::output::OutFormat;
use dalica_create::save::{self, copy::Destination};
use dalica_edit::Recipe;
use dalica_library::{Op, SaveState};
use dalica_meta::{Changes, DateTaken, WriteOptions};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;
use crate::settings::{MAX_PATH_BYTES, SaveFormat, SaveSettings};

/// The most photos one `photo.saveCopy` names.
pub const MAX_IDS: usize = 100_000;
/// `photo.saveCopy` with no photo that has edits.
pub const NO_EDITS: &str = "These pictures have no edits to save as a copy.";
/// A photo whose folder is offline.
pub const OFFLINE: &str = "The folder is offline: connect its drive to save a copy of its pictures.";
/// A photo whose last Save did not finish: its file is repaired first.
pub const PENDING: &str = "An earlier Save of this photo did not finish and is repaired first.";
/// Save As to a path that already holds a file (code `exists`).
pub const EXISTS: &str = "A file of that name already exists: choose another name.";
/// The label of Save a Copy's job: the progress reads "Saving copies 1 of 3".
pub const COPY_LABEL: &str = "Saving copies";
/// The label of Save As's job: "Saving 0 of 1".
pub const SAVE_AS_LABEL: &str = "Saving";
/// The name of the folder of kept originals: never a Save As destination.
const ORIGINALS_DIR: &str = ".dalica-originals";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.saveCopy",
            label: "Save a Copy",
            menu: "File/Save a Copy",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: writes each photo that has edits as "<stem>-001.jpg" (the first free number) in its folder with the edits applied at Match quality and all its metadata, as a save job (no undo step); the photo and its edits are untouched and the copy joins the library as a new, unedited photo with the photo's caption, tags, star and date. Photos without edits are left out. Replies {job, pending:true, total}; the job's result is {total, done, written:[path], photos:[PhotoId], failed:[{file, reason, code:"exists"|"no_folder"|"failed"}], cancelled}"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_copy,
            run: run_copy,
        },
        CommandSpec {
            id: "photo.saveTo",
            label: "Save As",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "path":string (absolute, or relative to the photo's folder; never inside .dalica-originals), "format"?:"jpeg"|"png"}: writes the photo with its edits to a new file at path (Save As; the UI's photo.saveAs asks for the path), JPEG at Match quality or PNG by format or the path's extension (a name without one takes the format's, else the last used), as a save job (no undo step). Never replaces a file: an existing path fails with code "exists"; the folder must exist (code "no_folder"); a folder under no watched folder is added as Scan Always. The folder and format are remembered for the next Save As. Replies {job, pending:true, total:1, path, format}; the job's result is as photo.saveCopy's"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_save_to,
            run: run_save_to,
        },
    ]
}

// --- which photos -------------------------------------------------------------------------------

/// The ids `p` names, when it names them as a list of ids; anything else is left to `run`.
fn named(p: &Value) -> Option<Vec<PhotoId>> {
    let list = p.get("ids")?.as_array()?;
    list.iter().map(|v| v.as_u64().map(PhotoId::new)).collect()
}

/// Why photo `id` cannot be written as a copy now, if it cannot (without reading the disk): its
/// folder is offline, its edits hold an operation Dalica cannot render, or its last Save did not
/// finish. The copy renders the photo's file as it is with its edits, so a saved photo's recorded
/// recipe is already in its pixels and is not checked.
fn refusal(s: &Session, id: PhotoId) -> Option<String> {
    let lib = s.library();
    let photo = lib.photo(id)?;
    if s.is_folder_offline(photo.folder) {
        return Some(OFFLINE.into());
    }
    if let Some(op) = lib.info(id).edits.iter().find(|op| !save::renders(op)) {
        let shown: String = op.chars().take(64).collect();
        return Some(format!("The edit {shown:?} cannot be rendered by this version of Dalica, so no copy is written."));
    }
    if lib.saved(id).is_some_and(|r| r.state == SaveState::Pending) {
        return Some(PENDING.into());
    }
    None
}

/// Enabled for photos with edits, at least one of which can be copied now; otherwise the reason
/// (the first photo's when every one would fail).
fn can_copy(s: &Session, p: &Value) -> Result<(), Disabled> {
    let Some(ids) = named(p) else { return Ok(()) };
    let lib = s.library();
    if ids.iter().any(|id| lib.photo(*id).is_none()) {
        return Ok(());
    }
    let edited: Vec<PhotoId> = ids.into_iter().filter(|id| !lib.info(*id).edits.is_empty()).collect();
    if edited.is_empty() {
        return Err(Disabled(NO_EDITS.into()));
    }
    match first_refusal(s, &edited) {
        Some(reason) => Err(Disabled(reason)),
        None => Ok(()),
    }
}

/// The first photo's refusal when every one of `ids` is refused; `None` when one can be copied.
fn first_refusal(s: &Session, ids: &[PhotoId]) -> Option<String> {
    let reasons = ids.iter().map(|id| refusal(s, *id)).collect::<Option<Vec<String>>>()?;
    reasons.into_iter().next()
}

/// Enabled unless the photo named cannot be written now.
fn can_save_to(s: &Session, p: &Value) -> Result<(), Disabled> {
    let Some(id) = p.get("id").and_then(Value::as_u64).map(PhotoId::new) else { return Ok(()) };
    match refusal(s, id) {
        Some(reason) => Err(Disabled(reason)),
        None => Ok(()),
    }
}

/// `ids` checked: at most [`MAX_IDS`], each known; repeats dropped, order kept.
fn known(s: &Session, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if s.library().photo(id).is_none() {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        }
        if seen.insert(id) {
            out.push(id);
        }
    }
    Ok(out)
}

// --- Save As: the path and the format -----------------------------------------------------------

/// The format a file name's extension names: `Some(Some(f))` for a JPEG or PNG name, `Some(None)`
/// for no extension (or one that names no image format), `None` for an image format Save As does
/// not write.
fn format_of_extension(name: &str) -> Option<Option<SaveFormat>> {
    let ext = dalica_core::names::split_ext(name).1.map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("jpg" | "jpeg" | "jpe" | "jfif") => Some(Some(SaveFormat::Jpeg)),
        Some("png") => Some(Some(SaveFormat::Png)),
        Some(
            "tif" | "tiff" | "webp" | "gif" | "bmp" | "jxl" | "psd" | "heic" | "heif" | "avif" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf"
            | "rw2" | "raf" | "mp4" | "mov",
        ) => None,
        _ => Some(None),
    }
}

/// The file Save As writes: `path` (a relative one taken from `folder`) and its format, checked
/// without touching the disk. A name without a known extension takes the format's: `format`, else
/// `last`.
fn destination(folder: &Path, path: &Path, format: Option<SaveFormat>, last: SaveFormat) -> Result<(PathBuf, SaveFormat), CmdError> {
    let text = path.to_str().ok_or_else(|| CmdError::BadParams("path: not valid Unicode".into()))?;
    if text.len() > MAX_PATH_BYTES {
        return Err(CmdError::Limit(format!("path: at most {MAX_PATH_BYTES} bytes")));
    }
    if text.contains('\0') {
        return Err(CmdError::BadParams("path: no NUL characters".into()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CmdError::BadParams("path: `..` is not allowed".into()));
    }
    let path = if path.is_absolute() { path.to_path_buf() } else { folder.join(path) };
    if path.components().any(|c| c.as_os_str() == ORIGINALS_DIR) {
        return Err(CmdError::BadParams(format!("path: never inside {ORIGINALS_DIR}")));
    }
    let name = match (path.components().next_back(), text.ends_with(['/', '\\'])) {
        (Some(Component::Normal(n)), false) => n.to_str().unwrap_or_default().to_string(),
        _ => return Err(CmdError::BadParams(format!("path: {text:?} names no file"))),
    };
    if path == folder {
        return Err(CmdError::BadParams(format!("path: {text:?} names no file")));
    }
    let Some(by_extension) = format_of_extension(&name) else {
        return Err(CmdError::BadParams(format!("path: Save As writes JPEG (.jpg) or PNG (.png), not {name:?}")));
    };
    match (format, by_extension) {
        (Some(f), Some(e)) if f != e => {
            Err(CmdError::BadParams(format!("path: {name:?} is not a {} name; change the name or the format", f.id().to_uppercase())))
        }
        (_, Some(e)) => Ok((path, e)),
        (f, None) => {
            let f = f.unwrap_or(last);
            let renamed = path.with_file_name(format!("{name}.{}", out_format(f).extension()));
            Ok((renamed, f))
        }
    }
}

fn out_format(f: SaveFormat) -> OutFormat {
    match f {
        SaveFormat::Jpeg => OutFormat::Jpeg,
        SaveFormat::Png => OutFormat::Png,
    }
}

// --- the commands -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyParams {
    ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveToParams {
    id: PhotoId,
    path: PathBuf,
    format: Option<String>,
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let CopyParams { ids } = params(p)?;
    let edited: Vec<PhotoId> = known(s, ids)?.into_iter().filter(|id| !s.library().info(*id).edits.is_empty()).collect();
    if edited.is_empty() {
        return Err(CmdError::Disabled(NO_EDITS.into()));
    }
    if let Some(reason) = first_refusal(s, &edited) {
        return Err(CmdError::Disabled(reason));
    }
    // A photo refused while others are not fails as its own item, with its reason.
    let targets: Vec<(PhotoId, Destination)> = edited.into_iter().map(|id| (id, Destination::Numbered)).collect();
    let (job, total) = start(s, Kind::Copy, targets)?;
    Ok(json!({ "job": job, "pending": true, "total": total }))
}

fn run_save_to(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SaveToParams { id, path, format } = params(p)?;
    let format = match format.as_deref() {
        None => None,
        Some(f) => Some(SaveFormat::from_id(f).ok_or_else(|| {
            let shown: String = f.chars().take(16).collect();
            CmdError::BadParams(format!("format: \"jpeg\" or \"png\", not {shown:?}"))
        })?),
    };
    let lib = s.library();
    let folder =
        lib.photo(id).and_then(|p| lib.folder(p.folder)).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let (path, format) = destination(&folder, &path, format, s.settings().save().last_format)?;
    if let Some(reason) = refusal(s, id) {
        return Err(CmdError::Disabled(reason));
    }
    let to = Destination::Path { path: path.clone(), format: out_format(format) };
    let (job, total) = start(s, Kind::SaveAs(format), vec![(id, to)])?;
    Ok(json!({ "job": job, "pending": true, "total": total, "path": path, "format": format.id() }))
}

// --- the job ------------------------------------------------------------------------------------

/// Which command a job runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Copy,
    SaveAs(SaveFormat),
}

/// Why an item failed, for the result's `code`; any other failure is `failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Code {
    Exists,
    NoFolder,
}

impl Code {
    fn id(self) -> &'static str {
        match self {
            Code::Exists => "exists",
            Code::NoFolder => "no_folder",
        }
    }
}

/// The codes of the items that failed for a known reason, by the item's file (each photo is one
/// item, so its file names it).
type Codes = Arc<Mutex<BTreeMap<PathBuf, Code>>>;

/// One photo to copy: everything its work needs, read from the model on the owner thread.
struct Item {
    id: PhotoId,
    folder: PathBuf,
    name: String,
    path: PathBuf,
    edits: Vec<String>,
    changes: Changes,
    to: Destination,
    options: save::Options,
    /// Why it fails without touching the disk, if it does.
    refused: Option<String>,
    codes: Codes,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.path
    }
}

impl Item {
    /// Notes the failure's code and answers its reason.
    fn fail(&self, code: Code, reason: String) -> String {
        self.codes.lock().unwrap_or_else(PoisonError::into_inner).insert(self.path.clone(), code);
        reason
    }
}

/// What one item wrote.
#[derive(Debug)]
struct Made {
    source: PhotoId,
    path: PathBuf,
}

/// One item, on the job's worker.
fn work(item: Item) -> Result<Done<Made>, String> {
    if let Some(reason) = item.refused.clone() {
        return Err(reason);
    }
    let recipe = Recipe::parse(&item.edits).map_err(|e| format!("its edits cannot be read: {e}"))?;
    if let Destination::Path { path, .. } = &item.to {
        let dir = path.parent().unwrap_or(path);
        match dalica_fs::stat(dir) {
            Ok(Some(st)) if !st.is_file => {}
            Ok(_) => return Err(item.fail(Code::NoFolder, format!("The folder {} does not exist.", dir.display()))),
            Err(e) => return Err(e.to_string()),
        }
        match dalica_fs::stat(path) {
            Ok(None) => {}
            Ok(Some(_)) => return Err(item.fail(Code::Exists, EXISTS.into())),
            Err(e) => return Err(e.to_string()),
        }
    }
    match save::copy::copy(&item.folder, &item.name, &recipe, &item.changes, &item.to, &item.options) {
        Ok(copied) => {
            let changes = vec![FileChange::Created { path: copied.path.clone() }];
            Ok(Done { changes, value: Made { source: item.id, path: copied.path } })
        }
        Err(save::Error::Fs(dalica_fs::Error::Io { source, .. }))
            if source.kind() == std::io::ErrorKind::AlreadyExists && matches!(item.to, Destination::Path { .. }) =>
        {
            Err(item.fail(Code::Exists, EXISTS.into()))
        }
        Err(e) => Err(e.reason()),
    }
}

/// The ops that give photo `copy` the owned fields of photo `source` it lacks: its caption, tags,
/// star and the date the user set (as Save gives a JPEG it writes beside its source).
fn owned_ops(s: &Session, source: PhotoId, copy: PhotoId) -> Vec<Op> {
    let lib = s.library();
    let (from, to) = (lib.info(source), lib.info(copy));
    let mut ops = Vec::new();
    if from.caption != to.caption {
        ops.push(Op::SetCaptions { captions: vec![(copy, from.caption.clone())] });
    }
    if from.tags != to.tags {
        ops.push(Op::SetTags { tags: vec![(copy, from.tags.clone())] });
    }
    let star = lib.photo(source).is_some_and(|p| p.star);
    if lib.photo(copy).is_some_and(|p| p.star != star) {
        ops.push(Op::SetStars { stars: vec![(copy, star)] });
    }
    if from.date.is_some() && from.date != to.date {
        ops.push(Op::SetDates { dates: vec![(copy, from.date)] });
    }
    ops
}

/// The library's owned fields of photo `id`, written into the copy (the place stays as the file
/// has it).
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

/// What the items wrote, gathered on the owner thread as they are applied.
#[derive(Default)]
struct Report {
    written: Vec<PathBuf>,
    photos: Vec<PhotoId>,
}

/// "Saved 2 copies", "Saved 1 of 2 copies; 1 failed", "Save a Copy cancelled: 1 of 3 copies
/// saved"; Save As: "Saved as IMG_0001.png", "Save As failed: <reason>".
fn message(kind: Kind, written: &[PathBuf], tally: &Tally) -> String {
    match kind {
        Kind::Copy => {
            let copies = |n: usize| if n == 1 { "1 copy".to_string() } else { format!("{n} copies") };
            let (done, failed) = (tally.done, tally.failures.len());
            let head = if tally.cancelled {
                format!("Save a Copy cancelled: {done} of {} saved", copies(tally.total))
            } else if failed > 0 {
                format!("Saved {done} of {}", copies(tally.total))
            } else {
                format!("Saved {}", copies(done))
            };
            if failed > 0 { format!("{head}; {failed} failed") } else { head }
        }
        Kind::SaveAs(_) => match (written.first(), tally.failures.first()) {
            (Some(path), _) => format!("Saved as {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            (None, Some(f)) => format!("Save As failed: {}", f.reason),
            (None, None) => "Save As cancelled".into(),
        },
    }
}

/// `xmp:MetadataDate` of the copy: the session's time.
fn metadata_date(now: Timestamp) -> Option<DateTaken> {
    DateTime::from_unix_secs(now.as_secs()).and_then(|d| DateTaken::new(d, Some(0), None))
}

/// Starts the job writing each `(photo, destination)` of `targets` (each known). Never touches
/// the disk. Returns the job and its item count.
fn start(s: &mut Session, kind: Kind, targets: Vec<(PhotoId, Destination)>) -> Result<(JobId, usize), CmdError> {
    let now = s.clock.now();
    let codes: Codes = Arc::new(Mutex::new(BTreeMap::new()));
    let mut items = Vec::with_capacity(targets.len());
    for (id, to) in targets {
        let lib = s.library();
        let Some((photo, folder)) = lib.photo(id).and_then(|p| Some((p.clone(), lib.folder(p.folder)?.path.clone()))) else {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        };
        let base = save::Options::new(now);
        let options = save::Options { write: WriteOptions { metadata_date: metadata_date(now), ..base.write.clone() }, ..base };
        items.push(Item {
            id,
            path: folder.join(&photo.name),
            folder,
            name: photo.name.clone(),
            edits: lib.info(id).edits.clone(),
            changes: owned_changes(s, id, photo.star),
            to,
            options,
            refused: refusal(s, id),
            codes: Arc::clone(&codes),
        });
    }
    let total = items.len();
    let report = Arc::new(Mutex::new(Report::default()));
    let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
    let (applied, finish_job) = (Arc::clone(&report), Arc::clone(&job_id));
    let label = if kind == Kind::Copy { COPY_LABEL } else { SAVE_AS_LABEL };
    let job = FileJob::new(JobKind::Save, label, items, work)
        .on_applied(move |s, a| {
            let Made { source, path } = a.value;
            let copy = a.photos.iter().find(|(p, _)| *p == path).map(|(_, id)| *id);
            let ops = copy.map(|c| owned_ops(s, source, c)).unwrap_or_default();
            if let Kind::SaveAs(format) = kind {
                remember(s, &path, format);
            }
            let mut r = applied.lock().unwrap_or_else(PoisonError::into_inner);
            r.written.push(path);
            r.photos.extend(copy);
            Ok(if ops.is_empty() { Vec::new() } else { vec![Op::Batch { ops }] })
        })
        .on_finish(move |s, tally| finish(s, tally, kind, &report, &codes, finish_job.get().copied()));
    let id = s.start_file_job(job);
    let _ = job_id.set(id);
    Ok((id, total))
}

/// Save As's folder and format, for the next Save As.
fn remember(s: &mut Session, path: &Path, format: SaveFormat) {
    let Some(folder) = path.parent().map(Path::to_path_buf) else { return };
    let current = s.settings().save();
    if current.last_folder.as_deref() == Some(folder.as_path()) && current.last_format == format {
        return;
    }
    s.change_settings(|st| st.set_save(&SaveSettings { last_folder: Some(folder), last_format: format, ..current }));
}

/// The job's end (on the owner thread): its message, and the job closed with the whole result.
fn finish(s: &mut Session, tally: &Tally, kind: Kind, report: &Mutex<Report>, codes: &Mutex<BTreeMap<PathBuf, Code>>, job: Option<JobId>) -> Notice {
    let r = report.lock().unwrap_or_else(PoisonError::into_inner);
    let text = message(kind, &r.written, tally);
    let folder = match kind {
        Kind::SaveAs(_) => r.written.first().and_then(|p| p.parent()).map(Path::to_path_buf),
        Kind::Copy => None,
    };
    // The job ends here with the files it wrote; the runner's own end after this is left out (a
    // job keeps its first end). Without an id (no worker thread could start, and the runner ended
    // the job before `start` had it) the runner's failure stands.
    if let Some(job) = job {
        let codes = codes.lock().unwrap_or_else(PoisonError::into_inner);
        let failed: Vec<Value> = tally
            .failures
            .iter()
            .map(|f| json!({ "file": f.file, "reason": f.reason, "code": codes.get(&f.file).map_or("failed", |c| c.id()) }))
            .collect();
        let result = json!({
            "total": tally.total,
            "done": tally.done,
            "written": r.written,
            "photos": r.photos,
            "failed": failed,
            "cancelled": tally.cancelled,
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
    use super::*;
    use crate::filejobs::ItemFailure;

    fn failure() -> ItemFailure {
        ItemFailure { file: PathBuf::from("a.jpg"), reason: EXISTS.into() }
    }

    #[test]
    fn messages_count_what_happened() {
        let t = |done, failures: usize, cancelled| Tally { total: 3, done, failures: (0..failures).map(|_| failure()).collect(), cancelled };
        assert_eq!(message(Kind::Copy, &[], &t(3, 0, false)), "Saved 3 copies");
        assert_eq!(message(Kind::Copy, &[], &Tally { total: 1, done: 1, ..Tally::default() }), "Saved 1 copy");
        assert_eq!(message(Kind::Copy, &[], &t(2, 1, false)), "Saved 2 of 3 copies; 1 failed");
        assert_eq!(message(Kind::Copy, &[], &t(1, 0, true)), "Save a Copy cancelled: 1 of 3 copies saved");
        let one = Tally { total: 1, done: 1, ..Tally::default() };
        assert_eq!(message(Kind::SaveAs(SaveFormat::Png), &[PathBuf::from("p").join("IMG_0001.png")], &one), "Saved as IMG_0001.png");
        let failed = Tally { total: 1, failures: vec![failure()], ..Tally::default() };
        assert_eq!(message(Kind::SaveAs(SaveFormat::Jpeg), &[], &failed), format!("Save As failed: {EXISTS}"));
    }

    #[test]
    fn the_destination_takes_its_format_from_the_name_or_the_parameter() {
        let folder = std::env::temp_dir().join("Lisbon");
        let jpeg = SaveFormat::Jpeg;
        let d = |p: &str, f: Option<SaveFormat>| destination(&folder, Path::new(p), f, jpeg);
        assert_eq!(d("a.PNG", None).unwrap(), (folder.join("a.PNG"), SaveFormat::Png));
        assert_eq!(d("a.jpeg", Some(jpeg)).unwrap(), (folder.join("a.jpeg"), jpeg));
        assert_eq!(d("a", Some(SaveFormat::Png)).unwrap(), (folder.join("a.png"), SaveFormat::Png));
        assert_eq!(d("Lisbon.2024", None).unwrap(), (folder.join("Lisbon.2024.jpg"), jpeg), "the last format used");
        assert_eq!(destination(&folder, Path::new("b"), None, SaveFormat::Png).unwrap(), (folder.join("b.png"), SaveFormat::Png));
        assert_eq!(d("sub/c.jpg", None).unwrap(), (folder.join("sub").join("c.jpg"), jpeg), "relative to the photo's folder");
        let abs = folder.join("Out").join("x.png");
        assert_eq!(destination(&folder, &abs, None, jpeg).unwrap(), (abs.clone(), SaveFormat::Png));
        for (p, f) in [
            ("a.png", Some(jpeg)),
            ("a.tif", None),
            ("", None),
            ("sub/", None),
            ("../a.jpg", None),
            (".dalica-originals/a.jpg", None),
            ("a\0.jpg", None),
        ] {
            assert!(matches!(d(p, f), Err(CmdError::BadParams(_))), "{p:?} {f:?}: {:?}", d(p, f));
        }
        assert!(matches!(d(&"x".repeat(MAX_PATH_BYTES + 1), None), Err(CmdError::Limit(_))));
    }
}
