//! Create Collage, Save Draft and Edit Collage (`creating.md` §1 and §3, `roadmap.md` M14 →
//! *Output*); the commands are `cmd::collage_output`.
//!
//! - **Create Collage and Save Draft** run as one `collage` file job ([`crate::filejobs`],
//!   [`JobKind::Collage`]) of two steps on one worker, so Cancel has a boundary before anything is
//!   written: **render** (each picture rendered with its edits at the size its place needs, one at
//!   a time, by the edit view's renderer, `thumbs::ladder::render_edited`; then
//!   `create::collage::render` draws the page and encodes the JPEG at quality 92 with the date of
//!   making as its date), then **write** (the folder made when missing; the JPEG and its
//!   `.dalica-collage` recipe through `fs::exact_file`, which never replaces a file, under the first
//!   free pair of names of `core::names::candidates`: `Collage 2026-10-19.jpg`, else
//!   `Collage 2026-10-19-001.jpg`, each with the recipe of the same stem). Cancel during the
//!   render leaves the write unstarted: no file, no folder.
//! - **Where:** the `path` given (an absolute folder), else the folder of the collage the room was
//!   reopened from (Create Collage from a reopened collage writes a new one beside it), else the
//!   projects directory's `Collages` folder (`<Pictures>/Dalica/Collages`). That folder is marked
//!   `"project": "collages"`: in a new sidecar when the folder has none, else in the model once the
//!   library holds it. The new JPEG enters the library as an export's files do (added at once when
//!   its folder is indexed, else its folder scanned when the job ends, as Scan Always when no
//!   watched folder covers it); [`Session::collage_made`] names it, and for Create Collage the
//!   window opens it in the edit view. No undo step: a collage is a new file, as an export is.
//! - **Edit Collage** reads the photo's recipe (`<stem>.dalica-collage` beside it) on a worker (a
//!   `collage` file job of one step) and reopens the room from it: each clip matched by its id,
//!   then by its path among the photos already indexed (a recipe's path is compared in memory,
//!   never opened: `security.md` §5.5), the clips not found left out and reported.
//! - **Can Edit** (the edit view's top bar) answers from a cache per photo; a photo not yet looked
//!   at starts a short look on a worker (an answered `analysis` job) and answers "not yet".
//!
//! Nothing here touches the disk on the owner thread.

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use dalica_core::names;
use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_create::collage::page::Format;
use dalica_create::collage::recipe::{self, EXTENSION};
use dalica_create::collage::render::{self as collage_render, Picture};
use dalica_create::collage::{Collage as Doc, Placement, Recipe};
use dalica_library::{FolderInfo, Op, Project};
use dalica_meta::DateTaken;
use dalica_store::mapping::FolderState;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;

/// The projects directory's folder collages are written into.
pub const COLLAGES: &str = "Collages";
/// The word a collage's file name starts with: `Collage 2026-10-19.jpg`.
pub const WORD: &str = "Collage";
/// The job's label while Create Collage runs ("Creating collage 1 of 2").
pub const CREATE_LABEL: &str = "Creating collage";
/// The job's label while Save Draft runs.
pub const DRAFT_LABEL: &str = "Saving collage draft";
/// The job's label while Edit Collage reads the recipe.
pub const OPEN_LABEL: &str = "Opening collage";
/// The label of Can Edit's look at a photo.
pub const CHECK_LABEL: &str = "Looking for a collage recipe";
/// The most photos Can Edit remembers; past it the answers are forgotten and asked again.
pub const MAX_CHECKS: usize = 256;
/// The longest `path` taken, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// The folder of kept originals: never a collage's destination.
const ORIGINALS_DIR: &str = ".dalica-originals";
/// Why a command is refused with no collage open.
pub const NOT_OPEN: &str = "No collage is open.";
/// Why Create Collage is refused with nothing on the page.
pub const NOTHING_ON_PAGE: &str = "Put some pictures on the page first.";
/// Why Edit Collage is refused while the open collage has unsaved changes.
pub const UNSAVED_OPEN: &str = "The open collage has unsaved changes: save it as a draft or close it first.";

/// The last collage written (Create Collage or Save Draft).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    /// Rises by one per collage written in this session: the window opens each new one once.
    pub seq: u64,
    pub job: JobId,
    /// The JPEG.
    pub path: PathBuf,
    /// Its recipe beside it.
    pub recipe: PathBuf,
    /// Save Draft wrote it (the edit view does not open it).
    pub draft: bool,
    /// The photo it is in the library, once its folder is indexed.
    pub photo: Option<PhotoId>,
}

/// The folder of the collage the open room was reopened from (Edit Collage), and the clips it
/// opened with: the room is that collage while it is open and holds one of them.
#[derive(Debug)]
struct Origin {
    folder: PathBuf,
    clips: HashSet<PhotoId>,
}

/// Can Edit's answer for one photo, valid while the photo's size and time are these.
#[derive(Clone, Copy, Debug)]
struct Check {
    stamp: (u64, Timestamp),
    state: CheckState,
}

#[derive(Clone, Copy, Debug)]
enum CheckState {
    Looking(JobId),
    Known(bool),
}

/// Create Collage's, Save Draft's and Edit Collage's state.
#[derive(Debug, Default)]
pub struct CollageOutput {
    job: Option<JobId>,
    made: Option<Made>,
    next_seq: u64,
    origin: Option<Origin>,
    checks: HashMap<PhotoId, Check>,
}

impl CollageOutput {
    /// The `collage` job running or last run, if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }
}

impl Session {
    /// Create Collage's and Save Draft's state.
    pub fn collage_output(&self) -> &CollageOutput {
        &self.collage_output
    }

    /// The last collage written in this session, with its photo once the library holds it.
    pub fn collage_made(&self) -> Option<Made> {
        let mut made = self.collage_output.made.clone()?;
        if made.photo.is_none_or(|id| self.library().photo(id).is_none()) {
            made.photo = self.collage_photo_at(&made.path);
        }
        Some(made)
    }

    /// The photo whose file is `path`, if indexed (compared in memory).
    pub(crate) fn collage_photo_at(&self, path: &Path) -> Option<PhotoId> {
        let lib = self.library();
        let folder = lib.folder_by_path(path.parent()?)?;
        lib.photo_by_name(folder, path.file_name()?.to_str()?).map(|p| p.id)
    }

    /// The folder Create Collage and Save Draft write into for `path` (the param, if given): that
    /// path, else the folder the open collage was reopened from, else `<Pictures>/Dalica/Collages`.
    /// Checked as written, never on the disk (automation checks the answer against its write roots,
    /// `control-protocol.md` §5.7).
    pub fn collage_destination(&self, path: Option<&Path>) -> Result<PathBuf, CmdError> {
        if let Some(p) = path {
            check_dir(p)?;
            return Ok(p.to_path_buf());
        }
        if let Some(folder) = self.collage_origin() {
            return Ok(folder.to_path_buf());
        }
        self.collages_dir()
            .ok_or_else(|| CmdError::Disabled("Dalica does not know your Pictures folder: give the folder to write the collage in (path).".into()))
    }

    /// `<Pictures>/Dalica/Collages`, when the Pictures folder is known.
    pub fn collages_dir(&self) -> Option<PathBuf> {
        self.projects_dir().map(|d| d.join(COLLAGES))
    }

    /// The folder the open room was reopened from, while it is still that collage.
    fn collage_origin(&self) -> Option<&Path> {
        let origin = self.collage_output.origin.as_ref()?;
        let doc = self.collage().document()?;
        doc.clips.iter().any(|c| origin.clips.contains(&c.id)).then_some(origin.folder.as_path())
    }

    /// The recipe of the photo `id`: `<stem>.dalica-collage` in its folder.
    pub fn collage_recipe_path(&self, id: PhotoId) -> Option<PathBuf> {
        let lib = self.library();
        let photo = lib.photo(id)?;
        let folder = lib.folder(photo.folder)?;
        let (stem, _) = names::split_ext(&photo.name);
        Some(folder.path.join(format!("{stem}.{EXTENSION}")))
    }

    /// The size and time of photo `id`, for Can Edit's cache.
    fn photo_stamp(&self, id: PhotoId) -> Option<(u64, Timestamp)> {
        self.library().photo(id).map(|p| (p.size, p.mtime))
    }
}

/// `dir` as a collage's folder: absolute, without `.` or `..`, valid Unicode of at most
/// [`MAX_PATH_BYTES`], never inside a folder of kept originals.
fn check_dir(dir: &Path) -> Result<(), CmdError> {
    let text = dir.to_str().ok_or_else(|| CmdError::BadParams("path: not valid Unicode".into()))?;
    if text.len() > MAX_PATH_BYTES {
        return Err(CmdError::Limit(format!("path: at most {MAX_PATH_BYTES} bytes")));
    }
    if !dir.is_absolute() || text.contains('\0') {
        return Err(CmdError::BadParams(format!("path: an absolute folder, not {:?}", names::fit(text, 200))));
    }
    if dir.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(CmdError::BadParams("path: `.` and `..` are not allowed".into()));
    }
    if dir.components().any(|c| c.as_os_str() == ORIGINALS_DIR) {
        return Err(CmdError::BadParams(format!("path: never inside {ORIGINALS_DIR}")));
    }
    Ok(())
}

// --- Create Collage and Save Draft -----------------------------------------------------------------

/// What one picture of the collage is drawn from (gathered on the owner thread).
struct Source {
    /// Its file, or `None` for a photo no longer indexed (its place is left empty).
    path: Option<PathBuf>,
    edits: Vec<String>,
    caption: String,
    name: String,
}

/// Everything both steps of one collage job share.
struct Shared {
    doc: Doc,
    placements: Vec<Placement>,
    sources: Vec<Source>,
    size: (u32, u32),
    made: Option<DateTaken>,
    folder: PathBuf,
    /// The folder is the projects directory's `Collages`: it gets the project mark.
    mark: bool,
    /// The file name asked for (`Collage 2026-10-19.jpg`).
    name: String,
    recipe: Vec<u8>,
    now: Timestamp,
    /// The JPEG the render step made, for the write step.
    jpeg: Mutex<Option<Vec<u8>>>,
    /// Pictures that could not be read (their places are empty).
    unread: Mutex<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Render,
    Write,
}

/// One step of a collage job.
struct Step {
    stage: Stage,
    file: PathBuf,
    shared: Arc<Shared>,
}

impl FileItem for Step {
    fn file(&self) -> &Path {
        &self.file
    }
}

/// What a step did.
enum Wrote {
    Rendered,
    /// The JPEG and its recipe.
    Files {
        jpeg: PathBuf,
        recipe: PathBuf,
    },
    /// Nothing to write (the render failed).
    Nothing,
}

/// One step, on the worker.
fn step(item: Step) -> Result<Done<Wrote>, String> {
    let shared = &item.shared;
    match item.stage {
        Stage::Render => {
            let bytes = render(shared)?;
            *shared.jpeg.lock().unwrap_or_else(PoisonError::into_inner) = Some(bytes);
            Ok(Done { changes: Vec::new(), value: Wrote::Rendered })
        }
        Stage::Write => {
            let Some(jpeg) = shared.jpeg.lock().unwrap_or_else(PoisonError::into_inner).take() else {
                return Ok(Done { changes: Vec::new(), value: Wrote::Nothing });
            };
            let (jpeg, recipe) = write(shared, &jpeg)?;
            Ok(Done { changes: vec![FileChange::Created { path: jpeg.clone() }], value: Wrote::Files { jpeg, recipe } })
        }
    }
}

/// The page drawn and encoded: each picture rendered at the size its place asks, one at a time.
fn render(shared: &Shared) -> Result<Vec<u8>, String> {
    let mut unread = 0usize;
    let mut picture = |p: &Placement, size: (u32, u32)| {
        let got = shared.sources.get(p.clip).and_then(|s| picture_of(s, size));
        if got.is_none() {
            unread += 1;
        }
        got
    };
    let result = collage_render::render(&shared.doc, &shared.placements, &mut picture, shared.size, shared.made.as_ref(), &|| false, &mut |_, _| {});
    *shared.unread.lock().unwrap_or_else(PoisonError::into_inner) = unread;
    result.map_err(|e| e.to_string())
}

/// One picture with its edits, fitted to `(w, h)`'s long side: the edit view's renderer over the
/// file decoded to that size (larger by what a crop cuts away). `None` when it cannot be read.
fn picture_of(source: &Source, (w, h): (u32, u32)) -> Option<Picture> {
    let path = source.path.as_ref()?;
    let recipe = dalica_edit::Recipe::parse(&source.edits).unwrap_or_default();
    let key = dalica_edit::version::key_with(&recipe, dalica_edit::ops::version);
    let edits = dalica_thumbs::ladder::Edits { recipe, key };
    let file = dalica_thumbs::ladder::Source { path: path.clone(), file_size: 0, mtime_ms: 0 };
    let render = dalica_thumbs::ladder::Render::default();
    let limits = dalica_image::Limits::default();
    let long = w.max(h).max(1);
    let image = catch_unwind(AssertUnwindSafe(|| dalica_thumbs::ladder::render_edited(&file, &render, &edits, long, &limits))).ok()?.ok()?;
    Some(Picture { image, caption: source.caption.clone(), name: source.name.clone() })
}

/// Whether something (a file, a folder, a link) is at `path`.
fn taken(path: &Path) -> bool {
    !matches!(dalica_fs::stat(path), Ok(None))
}

/// Makes the folder when missing (with its project mark when it is the Collages folder and has no
/// sidecar yet), then writes the JPEG and its recipe under the first free pair of names.
fn write(shared: &Shared, jpeg: &[u8]) -> Result<(PathBuf, PathBuf), String> {
    let folder = &shared.folder;
    dalica_fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    if shared.mark && matches!(dalica_fs::stat(&folder.join(dalica_sidecar::FILE_NAME)), Ok(None)) {
        let state = FolderState {
            id: FolderId::new(0),
            path: folder.clone(),
            folder: FolderInfo::default(),
            photos: Vec::new(),
            project: Some(Project::Collages),
            ..FolderState::default()
        };
        match dalica_store::writer::write_folder(&state, shared.now) {
            dalica_store::writer::Outcome::Written | dalica_store::writer::Outcome::Unchanged => {}
            other => log::warn!("{}: the Collages mark was not written: {other:?}", folder.display()),
        }
    }
    for candidate in names::candidates(&shared.name) {
        let (stem, _) = names::split_ext(&candidate);
        let recipe_name = format!("{stem}.{EXTENSION}");
        let (jpeg_path, recipe_path) = (folder.join(&candidate), folder.join(&recipe_name));
        // A pair half taken is passed over (a stray recipe would be read as this collage's).
        if taken(&jpeg_path) || taken(&recipe_path) {
            continue;
        }
        let written = match dalica_fs::exact_file(folder, &candidate, jpeg) {
            Ok(identity) => identity,
            Err(dalica_fs::Error::Exists(_) | dalica_fs::Error::Link(_)) => continue,
            Err(e) => return Err(e.to_string()),
        };
        match dalica_fs::exact_file(folder, &recipe_name, &shared.recipe) {
            Ok(_) => return Ok((jpeg_path, recipe_path)),
            Err(dalica_fs::Error::Exists(_) | dalica_fs::Error::Link(_)) => {
                // A recipe of the name appeared between the look and the write: the JPEG just made
                // (still ours, nobody has seen it) goes, and the next pair is tried.
                if matches!(dalica_fs::identity(&jpeg_path), Ok(Some(i)) if i == written) {
                    let _ = dalica_fs::remove_own_file(&jpeg_path);
                }
                continue;
            }
            Err(e) => return Err(format!("the picture {} was written, its recipe was not: {e}", jpeg_path.display())),
        }
    }
    Err(format!("no free name for {} in {}", shared.name, folder.display()))
}

/// What Create Collage or Save Draft starts from.
pub(crate) struct Request {
    pub draft: bool,
    pub path: Option<PathBuf>,
    pub display: Option<(u32, u32)>,
    /// Save Draft: close the room once the job has its copy of the collage.
    pub close: bool,
}

/// The size the page is written at: the display's for Current Display (the size the window gives
/// now, at the page's ratio, else the one the page kept), else 3000 px on the long side.
pub(crate) fn output_size(doc: &Doc, display: Option<(u32, u32)>) -> (u32, u32) {
    match (&doc.page.format, display) {
        (Format::Display { .. }, Some((w, h))) => doc.page.pixel_size(w.max(h)),
        _ => doc.page.output_size(),
    }
}

/// The date of making, as the file's date and the name's day (UTC, as an export's names are).
fn made_at(now: Timestamp) -> Option<DateTime> {
    DateTime::from_unix_secs(now.as_secs())
}

impl Session {
    /// Starts Create Collage or Save Draft on the open collage (`cmd::collage_output`): checks
    /// everything on the owner thread without touching the disk, then starts the job. Replies
    /// `{job, pending, total, folder, name, draft, size}`.
    pub(crate) fn start_collage_output(&mut self, r: Request) -> Result<Value, CmdError> {
        let doc = self.collage().document().cloned().ok_or_else(|| CmdError::Disabled(NOT_OPEN.into()))?;
        if !doc.clips.iter().any(|c| c.on_page) {
            return Err(CmdError::Disabled(NOTHING_ON_PAGE.into()));
        }
        let folder = self.collage_destination(r.path.as_deref())?;
        let mark = self.collages_dir().is_some_and(|d| d == folder);
        let display = r.display.or_else(|| self.collage().display());
        let size = output_size(&doc, display);
        let placements = self.collage_placements();
        let lib = self.library();
        let mut sources = Vec::with_capacity(doc.clips.len());
        let mut paths = Vec::with_capacity(doc.clips.len());
        for clip in &doc.clips {
            let found = lib.photo(clip.id).and_then(|p| Some((p, lib.folder(p.folder)?)));
            let path = found.map(|(p, f)| f.path.join(&p.name));
            paths.push(path.as_deref().and_then(Path::to_str).unwrap_or_default().to_string());
            let info = lib.info(clip.id);
            sources.push(Source {
                path,
                edits: info.edits.clone(),
                caption: info.caption.clone(),
                name: found.map(|(p, _)| p.name.clone()).unwrap_or_default(),
            });
        }
        let recipe = Recipe::new(doc.clone(), paths, r.draft, Some(size)).map_err(|e| CmdError::BadParams(e.to_string()))?;
        let bytes = recipe.print();
        // Read back as Edit Collage will read it, before anything is written.
        Recipe::parse(&bytes).map_err(|e| CmdError::Internal(format!("the collage's recipe does not read back: {e}")))?;
        let now = self.clock.now();
        let made = made_at(now);
        let base = made.map_or_else(|| WORD.to_string(), |d| names::dated(WORD, d.date()));
        let name = format!("{}.jpg", names::sanitize(&base, WORD));
        let shared = Arc::new(Shared {
            doc: doc.clone(),
            placements,
            sources,
            size,
            made: made.map(DateTaken::local),
            folder: folder.clone(),
            mark,
            name: name.clone(),
            recipe: bytes,
            now,
            jpeg: Mutex::new(None),
            unread: Mutex::new(0),
        });
        let file = folder.join(&name);
        let steps: Vec<Step> =
            [Stage::Render, Stage::Write].into_iter().map(|stage| Step { stage, file: file.clone(), shared: Arc::clone(&shared) }).collect();
        let label = if r.draft { DRAFT_LABEL } else { CREATE_LABEL };
        let written: Arc<Mutex<Option<(PathBuf, PathBuf)>>> = Arc::default();
        let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
        let (applied, finished, finish_job, apply_job) = (Arc::clone(&written), Arc::clone(&written), Arc::clone(&job_id), Arc::clone(&job_id));
        let finish_shared = Arc::clone(&shared);
        let draft = r.draft;
        let job = FileJob::new(JobKind::Collage, label, steps, step)
            .on_applied(move |s, a| {
                if let Wrote::Files { jpeg, recipe } = a.value {
                    let photo = a.photos.iter().find(|(p, _)| *p == jpeg).map(|(_, id)| *id);
                    s.collage_written(apply_job.get().copied().unwrap_or(0), &jpeg, &recipe, draft, photo, &doc);
                    *applied.lock().unwrap_or_else(PoisonError::into_inner) = Some((jpeg, recipe));
                }
                Ok(Vec::new())
            })
            .on_finish(move |s, tally| s.collage_finished(tally, &finish_shared, draft, &finished, finish_job.get().copied()));
        let id = self.start_file_job(job);
        let _ = job_id.set(id);
        self.collage_output.job = Some(id);
        if r.close {
            self.collage.close();
            self.collage_output.origin = None;
        }
        Ok(json!({ "job": id, "pending": true, "total": 2, "folder": folder, "name": name, "draft": r.draft, "size": [size.0, size.1] }))
    }

    /// One collage written (on the owner thread, its JPEG in the library when its folder is): kept
    /// as the last one made, known to Can Edit, and the room's collage counts as written when it
    /// has not changed since the job took it.
    fn collage_written(&mut self, job: JobId, jpeg: &Path, recipe: &Path, draft: bool, photo: Option<PhotoId>, doc: &Doc) {
        let seq = self.collage_output.next_seq;
        self.collage_output.next_seq = seq.saturating_add(1);
        self.collage_output.made = Some(Made { seq, job, path: jpeg.to_path_buf(), recipe: recipe.to_path_buf(), draft, photo });
        if let Some(id) = photo
            && let Some(stamp) = self.photo_stamp(id)
        {
            self.remember_check(id, Check { stamp, state: CheckState::Known(true) });
        }
        if self.collage().document() == Some(doc) {
            self.collage.mark_saved();
        }
    }

    /// The job's end: the Collages folder marked if the library holds it by now, the message, and
    /// the job closed with its whole result.
    fn collage_finished(
        &mut self,
        tally: &Tally,
        shared: &Shared,
        draft: bool,
        written: &Mutex<Option<(PathBuf, PathBuf)>>,
        job: Option<JobId>,
    ) -> Notice {
        if shared.mark
            && let Some(id) = self.library().folder_by_path(&shared.folder)
            && self.library().project(id) != Some(Project::Collages)
            && let Err(e) = self.apply(Op::SetProjects { projects: vec![(id, Some(Project::Collages))] })
        {
            log::warn!("{}: not marked Collages: {e}", shared.folder.display());
        }
        let written = written.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let unread = *shared.unread.lock().unwrap_or_else(PoisonError::into_inner);
        let file = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = match (&written, tally.cancelled) {
            (Some((jpeg, _)), _) if draft => format!("Saved the collage draft {}", file(jpeg)),
            (Some((jpeg, _)), _) => format!("Created {}", file(jpeg)),
            (None, true) => "Collage cancelled: nothing was written".to_string(),
            (None, false) => "The collage was not written".to_string(),
        };
        let text = match unread {
            0 => text,
            1 if written.is_some() => format!("{text}; 1 picture could not be read"),
            n if written.is_some() => format!("{text}; {n} pictures could not be read"),
            _ => text,
        };
        if let Some(job) = job {
            let photo = written.as_ref().and_then(|(p, _)| self.collage_photo_at(p));
            let result = json!({
                "path": written.as_ref().map(|(p, _)| p),
                "recipe": written.as_ref().map(|(_, r)| r),
                "photo": photo,
                "draft": draft,
                "folder": shared.folder,
                "size": [shared.size.0, shared.size.1],
                "unread": unread,
                "cancelled": tally.cancelled,
                "failed": tally.failures.iter().map(|f| json!({ "file": f.file, "reason": f.reason })).collect::<Vec<_>>(),
            });
            let applied = tally.done.saturating_add(tally.failures.len());
            self.jobs.progress(job, JobProgress { done: applied as u64, total: tally.total as u64 }, text.clone());
            let outcome = match (&written, tally.cancelled, tally.failures.first()) {
                (Some(_), _, _) => Outcome::Done(result),
                (None, true, _) => Outcome::Cancelled(result),
                (None, false, Some(f)) => Outcome::Failed(f.reason.clone()),
                (None, false, None) => Outcome::Failed(text.clone()),
            };
            let now = self.clock.now();
            self.jobs.finish(job, outcome, text.clone(), now);
        }
        Notice { message: text, folder: written.is_some().then(|| shared.folder.clone()) }
    }
}

// --- Edit Collage ---------------------------------------------------------------------------------

/// Edit Collage's one step: the recipe to read.
struct Open {
    recipe: PathBuf,
}

impl FileItem for Open {
    fn file(&self) -> &Path {
        &self.recipe
    }
}

/// Reads and parses a recipe under its limits (on a worker).
fn read_recipe(path: &Path) -> Result<Recipe, String> {
    match dalica_fs::read(path, recipe::MAX_BYTES as u64) {
        Ok(Some(r)) => Recipe::parse(&r.bytes).map_err(|e| e.to_string()),
        Ok(None) => Err("there is no collage recipe beside this photo".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// What Edit Collage opened: how many clips, and those not found.
type Opened = (usize, Vec<Missing>);

/// A clip of a recipe that is not in the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub id: PhotoId,
    pub path: String,
}

/// The collage of `recipe` with its clips matched to the library: by id, then by path among the
/// photos indexed (in memory only); the clips not found are left out (with the moves, swaps,
/// background and frame centre naming them) and returned.
pub(crate) fn matched(s: &Session, recipe: &Recipe) -> (Doc, Vec<Missing>) {
    let lib = s.library();
    let mut doc = recipe.collage.clone();
    let mut map: HashMap<PhotoId, PhotoId> = HashMap::new();
    let mut taken: HashSet<PhotoId> = HashSet::new();
    let mut missing = Vec::new();
    let mut clips = Vec::with_capacity(doc.clips.len());
    for (i, clip) in doc.clips.iter().enumerate() {
        let path = recipe.paths.get(i).cloned().unwrap_or_default();
        let by_path = || if path.is_empty() { None } else { s.collage_photo_at(Path::new(&path)) };
        let found = if lib.photo(clip.id).is_some() { Some(clip.id) } else { by_path() };
        match found.filter(|id| !taken.contains(id)) {
            Some(id) => {
                taken.insert(id);
                map.insert(clip.id, id);
                clips.push(dalica_create::collage::Clip { id, on_page: clip.on_page });
            }
            None => missing.push(Missing { id: clip.id, path }),
        }
    }
    doc.clips = clips;
    let to = |id: PhotoId| map.get(&id).copied();
    doc.moves = doc.moves.iter().filter_map(|m| to(m.id).map(|id| dalica_create::collage::Move { id, ..*m })).collect();
    doc.swaps = doc.swaps.iter().filter_map(|[a, b]| Some([to(*a)?, to(*b)?])).collect();
    doc.background.picture = doc.background.picture.and_then(to);
    doc.frame_centre = doc.frame_centre.and_then(to);
    (doc, missing)
}

impl Session {
    /// Starts Edit Collage for photo `id` (`cmd::collage_output`). Replies `{job, pending, recipe}`.
    pub(crate) fn start_collage_edit(&mut self, id: PhotoId) -> Result<Value, CmdError> {
        if self.library().photo(id).is_none() {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        }
        if self.collage().is_changed() {
            return Err(CmdError::Disabled(UNSAVED_OPEN.into()));
        }
        let recipe = self.collage_recipe_path(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let folder = recipe.parent().map(Path::to_path_buf).unwrap_or_default();
        let report: Arc<Mutex<Option<Opened>>> = Arc::default();
        let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
        let (applied, finished, finish_job) = (Arc::clone(&report), Arc::clone(&report), Arc::clone(&job_id));
        let job = FileJob::new(JobKind::Collage, OPEN_LABEL, vec![Open { recipe: recipe.clone() }], |o: Open| {
            read_recipe(&o.recipe).map(|r| Done { changes: Vec::new(), value: r })
        })
        .on_applied(move |s, a| {
            if s.collage().is_changed() {
                return Err(UNSAVED_OPEN.into());
            }
            let (doc, missing) = matched(s, &a.value);
            doc.check().map_err(|e| e.to_string())?;
            let display = match doc.page.format {
                Format::Display { width, height } => Some((width, height)),
                Format::Ratio { .. } => None,
            };
            let clips = doc.clips.iter().map(|c| c.id).collect();
            let n = doc.clips.len();
            s.collage.open(doc, display);
            s.collage_output.origin = Some(Origin { folder: folder.clone(), clips });
            *applied.lock().unwrap_or_else(PoisonError::into_inner) = Some((n, missing));
            Ok(Vec::new())
        })
        .on_finish(move |s, tally| {
            let report = finished.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let text = match &report {
                Some((_, m)) if m.is_empty() => "Opened the collage".to_string(),
                Some((_, m)) if m.len() == 1 => "Opened the collage; 1 of its pictures is no longer in the library".to_string(),
                Some((_, m)) => format!("Opened the collage; {} of its pictures are no longer in the library", m.len()),
                None => tally
                    .failures
                    .first()
                    .map_or_else(|| "The collage was not opened".to_string(), |f| format!("Could not open the collage: {}", f.reason)),
            };
            if let Some(job) = finish_job.get().copied() {
                let outcome = match &report {
                    Some((clips, missing)) => Outcome::Done(json!({
                        "opened": true,
                        "clips": clips,
                        "missing": missing.iter().map(|m| json!({ "id": m.id, "path": m.path })).collect::<Vec<_>>(),
                    })),
                    None => Outcome::Failed(text.clone()),
                };
                let now = s.clock.now();
                s.jobs.finish(job, outcome, text.clone(), now);
            }
            Notice { message: text, folder: None }
        });
        let job = self.start_file_job(job);
        let _ = job_id.set(job);
        self.collage_output.job = Some(job);
        Ok(json!({ "job": job, "pending": true, "recipe": recipe }))
    }

    /// Can Edit for photo `id`: `Some(answer)` when known for the photo as it is now, else `None`
    /// with a look at the disk started on a worker (or still running). Replies
    /// `{id, canEdit, pending, job?}`.
    pub(crate) fn collage_can_edit(&mut self, id: PhotoId) -> Result<Value, CmdError> {
        let stamp = self.photo_stamp(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        if self.collage_made().is_some_and(|m| m.photo == Some(id)) {
            self.remember_check(id, Check { stamp, state: CheckState::Known(true) });
        }
        let now = self.clock.now();
        self.jobs.collect(now);
        let known = self.collage_output.checks.get(&id).copied().filter(|c| c.stamp == stamp);
        let state = match known.map(|c| c.state) {
            Some(CheckState::Known(b)) => return Ok(json!({ "id": id, "canEdit": b, "pending": false })),
            Some(CheckState::Looking(job)) => match self.job(job) {
                Some(e) if e.state.is_finished() => {
                    let b = e.result.as_ref().and_then(|r| r.get("canEdit")).and_then(Value::as_bool).unwrap_or(false);
                    self.remember_check(id, Check { stamp, state: CheckState::Known(b) });
                    return Ok(json!({ "id": id, "canEdit": b, "pending": false }));
                }
                Some(_) => CheckState::Looking(job),
                None => self.look_for_recipe(id),
            },
            None => self.look_for_recipe(id),
        };
        self.remember_check(id, Check { stamp, state });
        match state {
            CheckState::Known(b) => Ok(json!({ "id": id, "canEdit": b, "pending": false })),
            CheckState::Looking(job) => Ok(json!({ "id": id, "canEdit": false, "pending": true, "job": job })),
        }
    }

    /// Starts the look for photo `id`'s recipe on a worker (an answered job).
    fn look_for_recipe(&mut self, id: PhotoId) -> CheckState {
        let Some(path) = self.collage_recipe_path(id) else { return CheckState::Known(false) };
        let now = self.clock.now();
        let (job, _cancel, tx) = self.jobs.start_answered(JobKind::Analysis, CHECK_LABEL, false, now);
        let spawned = std::thread::Builder::new().name("dalica-collage-check".into()).spawn(move || {
            let readable = catch_unwind(AssertUnwindSafe(|| read_recipe(&path).is_ok())).unwrap_or(false);
            let _ = tx.send((Outcome::Done(json!({ "id": id, "canEdit": readable })), CHECK_LABEL.to_string()));
        });
        match spawned {
            Ok(_) => CheckState::Looking(job),
            Err(e) => {
                let now = self.clock.now();
                self.jobs.finish(job, Outcome::Failed(e.to_string()), CHECK_LABEL, now);
                CheckState::Known(false)
            }
        }
    }

    fn remember_check(&mut self, id: PhotoId, check: Check) {
        let checks = &mut self.collage_output.checks;
        if checks.len() >= MAX_CHECKS && !checks.contains_key(&id) {
            checks.retain(|_, c| matches!(c.state, CheckState::Looking(_)));
        }
        checks.insert(id, check);
    }
}
