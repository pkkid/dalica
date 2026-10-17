//! Importing (`organizing.md` §3 items 4–6, `roadmap.md` M11): the import job that copies,
//! verifies, names and enters the photos the Import room chose, its one undo step, and removing
//! the copied files from the card they came from.
//!
//! - **Copying.** `import.run` plans on the owner thread from the listing alone (no file access):
//!   the candidates to copy (those given, or every one, less the excluded and, with "Exclude
//!   duplicates", the duplicates) and each one's folder (`<destination>/<name>`: the text given
//!   through `core::names::sanitize`, the capture date for "Date taken", the session clock's date
//!   for "Today's date"); the destination is watched (Scan Always) unless a watched folder covers
//!   it. An [`crate::jobs::JobKind::Import`] file job (`filejobs.rs`, one thread) then copies each
//!   file through `dalica-fs` `transfer::import_copy`: the folder made exclusively when it is new
//!   and imported into when it exists, the source checked against the size the listing read, a new
//!   temporary file, `fsync`, the source's modification time, a byte-for-byte check, and a name
//!   that never replaces one (a taken name gets `-001`, `-002` …). Cancel stops after the current
//!   file.
//! - **Entering.** Each copy enters the library on the owner thread as its result is applied: its
//!   folder (added when the library does not have it), the photo with the file's facts read back on
//!   the job's thread, the star and the rotation chosen in the room, and its digest (the
//!   listing's). The model holds the file's size and time before any rescan looks (rescans wait
//!   while a file job runs), so the watcher imports nothing of Dalica's own copies.
//! - **One undo step.** When the job ends, the files copied (all of them, or those before Cancel)
//!   are one [`FileStep`] "Import" on the library undo stack. Undo sends each copy still unchanged
//!   to the trash and takes its photo out of the library (a changed copy stays and says why; one
//!   already gone counts as undone); redo copies the file from the source again under the name it
//!   got, while the source is unchanged, and puts back what undo took out. The copied files are a
//!   ledger the step and the removal share, so both always know the copies as they are now.
//! - **Removing from the card.** `import.removeFromSource` removes, for the last import from a
//!   card or a drive, the files it copied and verified and nothing else, each through `dalica-fs`
//!   `remove::remove_copied` (only while the source still has the identity it had when it was
//!   copied and the copy still has its verified identity; to the OS trash where the card has one,
//!   deleted otherwise: the one permanent deletion in Dalica, `security.md` §5.7). It asks first
//!   (refused without `"confirm": true`), and automation never runs it
//!   ([`crate::automation::NEVER_AUTOMATED`]). From its start Undo Import waits; once a file was
//!   removed Undo Import stays disabled with [`REMOVED`].
//! - **What the room shows.** [`Session::import_status`] (`import.status`): the progress line
//!   ("Copying 45 of 340 at 38 MB/s") and, once the job ended, its result (folders, imported,
//!   skipped as duplicates or excluded, failed with reasons, bytes, speed) and the removal's. Each
//!   job's end is also a [`crate::FileJobEvent`] of kind `import`, for the UI's toast.
//!
//! The owner thread never touches the disk: every file call runs on the jobs' threads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use dalica_core::names;
use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_edit::{Op as EditOp, Recipe as EditRecipe, Turn};
use dalica_fs::Identity;
use dalica_fs::remove::{self, Removed};
use dalica_fs::transfer::{self, Imported, Outcome, Progress};
use dalica_fs::trash::{self, Sent};
use dalica_library::{ContentDigest, Folder, Op, Photo, Recipe};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Applied, Done, FileItem, FileJob, ItemFailure, Notice, Tally, default_message};
use crate::fileops::{Batch, Direction};
use crate::import::{NOTHING_OPEN, Source, SourceKind};
use crate::jobs::{JobId, JobKind};
use crate::session::Session;
use crate::settings::AfterImport;
use crate::undo::FileStep;

/// The undo step's label: "Undo Import", "Redo Import".
pub const LABEL: &str = "Import";
/// Why Undo Import is disabled once files were removed from the card.
pub const REMOVED: &str = "The originals were removed from the card.";
/// Why Undo Import waits while files are being removed from the card.
pub const REMOVING: &str = "The originals are being removed from the card.";
/// The question before removing files from the card (`organizing.md` §3).
pub const REMOVE_QUESTION: &str = "Remove the imported files from the card? This cannot be undone.";
/// Why `import.run` waits for the listing.
pub const STILL_LISTING: &str = "The source is still being listed.";
/// Why a second import waits.
pub const RUNNING: &str = "An import is already running.";
/// Why nothing else of the import runs while files leave the card.
pub const REMOVAL_RUNNING: &str = "The files are being removed from the card.";
/// Undo step ids of imports start here, far from the file operations' ids (`fileops.rs`).
const STEP_IDS: u64 = 1 << 62;

/// The room's "Folder name:".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderName {
    /// A name typed (or a recent one).
    Text(String),
    /// "Date taken (YYYY-MM-DD)": one folder per capture date.
    DateTaken,
    /// "Today's date": the session clock's date.
    Today,
}

/// Called on the job's thread after each file of an import is copied, with its place in the job
/// (a test hook: [`Session::set_import_gate`]).
pub type Gate = Arc<dyn Fn(usize) + Send + Sync>;

/// One file an import copied, as the undo step and the removal know it now.
#[derive(Clone, Debug)]
struct Copied {
    /// The file on the source and its identity when it was copied.
    source: PathBuf,
    source_identity: Identity,
    /// Where it went: `<destination>/<folder>/<name>`.
    destination: PathBuf,
    folder: String,
    name: String,
    /// The library folder and photo it became.
    folder_id: FolderId,
    photo: PhotoId,
    star: bool,
    turns: u8,
    digest: String,
    /// The copy's verified identity; `None` while its import is undone.
    copy: Option<Identity>,
    /// What undo took out of the library, put back by redo.
    kept: Option<Op>,
    /// Removed from the source.
    removed: bool,
}

impl Copied {
    fn dir(&self) -> PathBuf {
        self.destination.join(&self.folder)
    }
}

/// The copied files of one import, shared by its undo step (on either stack) and its removal.
#[derive(Debug, Default)]
struct Ledger {
    files: Vec<Copied>,
    /// Why Undo Import cannot run ([`REMOVING`], [`REMOVED`]).
    blocked: Option<String>,
    /// Its undo or redo is running.
    busy: bool,
}

type Shared = Arc<Mutex<Ledger>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where the import is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Copying,
    Done,
    Cancelled,
}

impl State {
    fn id(self) -> &'static str {
        match self {
            State::Copying => "copying",
            State::Done => "done",
            State::Cancelled => "cancelled",
        }
    }
}

/// The removal from the card of the last import.
#[derive(Clone, Debug, Default)]
struct Removal {
    job: JobId,
    running: bool,
    trashed: usize,
    deleted: usize,
    kept: Vec<ItemFailure>,
}

/// The last import of this session.
struct Last {
    job: JobId,
    source: Source,
    destination: PathBuf,
    after: AfterImport,
    ledger: Shared,
    progress: Arc<Progress>,
    started: Instant,
    /// Seconds from start to end, once ended.
    seconds: Option<f64>,
    state: State,
    /// Files to copy.
    total: usize,
    duplicates: usize,
    excluded: usize,
    failures: Vec<ItemFailure>,
    removal: Option<Removal>,
}

/// The import running or last finished in this session.
#[derive(Default)]
pub struct ImportRun {
    job: Option<JobId>,
    last: Option<Last>,
    next_step: u64,
    gate: Option<Gate>,
}

impl std::fmt::Debug for ImportRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportRun").field("job", &self.job).field("last", &self.last.as_ref().map(|l| l.job)).finish_non_exhaustive()
    }
}

impl ImportRun {
    /// The import job running or last run ([`crate::jobs::JobKind::Import`]), if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }

    fn step_id(&mut self) -> u64 {
        self.next_step = self.next_step.saturating_add(1);
        STEP_IDS.saturating_add(self.next_step)
    }
}

// --- the jobs' items and work ----------------------------------------------------------------------

/// The facts of a copy, read back on the job's thread.
#[derive(Clone, Debug)]
struct Facts {
    size: u64,
    mtime: Timestamp,
    width: u32,
    height: u32,
    date: Option<DateTime>,
}

/// One file to copy (an import's, or a redo's).
#[derive(Clone, Debug)]
struct CopyItem {
    /// Its place in the job.
    order: usize,
    source: PathBuf,
    /// The bytes the listing read.
    size: u64,
    destination: PathBuf,
    folder: String,
    /// Redo: the name it got, and the source's identity when it was copied.
    exact: Option<String>,
    expect: Option<Identity>,
    star: bool,
    turns: u8,
    digest: String,
    /// Redo: its place in the ledger.
    index: usize,
}

impl FileItem for CopyItem {
    fn file(&self) -> &Path {
        &self.source
    }
}

/// A file copied and verified.
#[derive(Clone, Debug)]
struct Got {
    name: String,
    copy: Identity,
    source_identity: Identity,
    facts: Facts,
}

fn fs_err(e: dalica_fs::Error) -> String {
    e.to_string()
}

fn shown(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Copies `item` (on the job's thread); see the module docs.
fn copy_one(item: &CopyItem, progress: &Progress) -> Result<Got, String> {
    let (Some(from), Some(name)) = (item.source.parent(), item.source.file_name().and_then(|n| n.to_str())) else {
        return Err(format!("{} has no name Dalica can copy", item.source.display()));
    };
    if dalica_fs::stat(&item.destination).map_err(fs_err)?.is_none() {
        dalica_fs::create_dir_all(&item.destination).map_err(fs_err)?;
    }
    let dir = item.destination.join(&item.folder);
    match dalica_fs::stat(&dir).map_err(fs_err)? {
        Some(st) if st.is_file => return Err(format!("{} is a file, not a folder", dir.display())),
        Some(_) => {}
        // Made exclusively; one made meanwhile is imported into.
        None => match transfer::new_folder(&item.destination, &item.folder).map_err(fs_err)? {
            Outcome::Done(()) | Outcome::Exists => {}
        },
    }
    let identity = dalica_fs::identity(&item.source).map_err(fs_err)?.ok_or_else(|| format!("{name} is no longer on the source"))?;
    match item.expect {
        Some(expect) if expect != identity => return Err(format!("{name} changed on the source since it was imported, so it is not copied again")),
        None if identity.size() != item.size => return Err(format!("{name} changed on the source since it was listed")),
        _ => {}
    }
    let names: Box<dyn Iterator<Item = String>> = match &item.exact {
        Some(exact) => Box::new(std::iter::once(exact.clone())),
        None => Box::new(names::candidates(name)),
    };
    for new_name in names {
        match transfer::import_copy(from, name, identity, &dir, &new_name, progress).map_err(fs_err)? {
            Imported::Copied(copy) => {
                let facts = facts_of(&dir.join(&new_name), copy)?;
                return Ok(Got { name: new_name, copy, source_identity: identity, facts });
            }
            Imported::Exists if item.exact.is_none() => {}
            Imported::Exists => return Err(format!("{} already has a file named {new_name}", shown(&dir))),
            Imported::Stopped => return Err("the import stopped before this file".into()),
        }
    }
    Err(format!("no free name for {name} in {}", shown(&dir)))
}

/// The copy at `path` (still `copy`) as a scan would list it: its size and time, and its header.
fn facts_of(path: &Path, copy: Identity) -> Result<Facts, String> {
    let stat = dalica_fs::stat(path)
        .map_err(fs_err)?
        .filter(|s| s.identity == copy)
        .ok_or_else(|| format!("{} changed right after it was copied", shown(path)))?;
    let mtime = Timestamp::from_system_time(stat.times.modified);
    let header = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dalica_image::read_header_file(path, &dalica_image::Limits::default())));
    Ok(match header {
        Ok(Ok((h, _))) => {
            let (width, height) = h.display_size();
            Facts { size: copy.size(), mtime, width, height, date: h.exif.as_ref().and_then(|e| e.date_time_original) }
        }
        _ => Facts { size: copy.size(), mtime, width: 0, height: 0, date: None },
    })
}

/// Undoing one copy: the copy to the trash while it is unchanged.
#[derive(Clone, Debug)]
struct UncopyItem {
    index: usize,
    path: PathBuf,
    expect: Option<Identity>,
}

impl FileItem for UncopyItem {
    fn file(&self) -> &Path {
        &self.path
    }
}

fn uncopy(item: &UncopyItem) -> Result<(), String> {
    let Some(expect) = item.expect else { return Ok(()) };
    match dalica_fs::identity(&item.path).map_err(fs_err)? {
        None => Ok(()),
        Some(id) if id != expect => Err(format!("The copy {} changed since it was imported, so it stays.", shown(&item.path))),
        Some(_) => match trash::send(&item.path).map_err(fs_err)? {
            Sent::Trashed(_) => Ok(()),
            Sent::NoTrash => Err(format!("The copy {} stays: its drive has no trash.", shown(&item.path))),
        },
    }
}

/// One source file to remove from the card.
#[derive(Clone, Debug)]
struct RemoveItem {
    index: usize,
    root: PathBuf,
    source: PathBuf,
    expect: Identity,
    copy: PathBuf,
    copy_identity: Identity,
}

impl FileItem for RemoveItem {
    fn file(&self) -> &Path {
        &self.source
    }
}

// --- helpers ----------------------------------------------------------------------------------------

/// The recipe of a photo turned `turns` quarter turns clockwise in the room.
fn rotation(turns: u8) -> Option<Recipe> {
    let turn = match turns % 4 {
        1 => Turn::Quarter,
        2 => Turn::Half,
        3 => Turn::ThreeQuarters,
        _ => return None,
    };
    let mut recipe = EditRecipe::default();
    recipe.push(EditOp::Rotate(turn)).ok()?;
    Some(Recipe { edits: recipe.print(), redo: Vec::new() })
}

fn batch(mut ops: Vec<Op>) -> Option<Op> {
    match ops.len() {
        0 => None,
        1 => ops.pop(),
        _ => Some(Op::Batch { ops }),
    }
}

/// "1 picture", "340 pictures".
fn pictures(n: usize) -> String {
    if n == 1 { "1 picture".into() } else { format!("{n} pictures") }
}

/// "38 MB/s", "3.4 MB/s": `bytes` over `seconds`; `None` before any time passed.
pub fn speed_text(bytes: u64, seconds: f64) -> Option<String> {
    if seconds <= 0.0 || !seconds.is_finite() {
        return None;
    }
    let mb = bytes as f64 / 1_000_000.0 / seconds;
    Some(if mb < 10.0 { format!("{mb:.1} MB/s") } else { format!("{mb:.0} MB/s") })
}

/// The folders of `files`, in the order they were first copied into.
fn folders_of(files: &[Copied]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for f in files {
        let dir = f.dir();
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// What an import's notice says.
fn finished_message(copied: usize, total: usize, folders: &[PathBuf], tally: &Tally) -> String {
    let failed = tally.failures.len();
    let mut m = if tally.cancelled {
        format!("Import cancelled: {copied} of {} copied", pictures(total))
    } else if copied == 0 {
        "Nothing was imported".to_string()
    } else {
        match folders {
            [one] => format!("Imported {} into {}", pictures(copied), shown(one)),
            more => format!("Imported {} into {} folders", pictures(copied), more.len()),
        }
    };
    if failed > 0 {
        m.push_str(&format!(", {failed} failed"));
    }
    m
}

impl Session {
    /// Test hook: `gate` is called on the job's thread after each file of an import is copied,
    /// with the file's place in the job (it may block, to cancel at a known point); `None` removes
    /// it.
    #[doc(hidden)]
    pub fn set_import_gate(&mut self, gate: Option<Gate>) {
        self.import_run.gate = gate;
    }

    /// Why `import.run` cannot run now.
    pub(crate) fn import_run_disabled(&self) -> Option<&'static str> {
        if self.import.source().is_none() {
            return Some(NOTHING_OPEN);
        }
        let last = self.import_run.last.as_ref();
        if last.is_some_and(|l| l.state == State::Copying) {
            return Some(RUNNING);
        }
        if last.and_then(|l| l.removal.as_ref()).is_some_and(|r| r.running) {
            return Some(REMOVAL_RUNNING);
        }
        None
    }

    /// `import.run`: plans the import from the listing (no file access) and starts its job.
    pub(crate) fn import_start(
        &mut self,
        ids: Option<&[u64]>,
        destination: PathBuf,
        folder: FolderName,
        after: Option<AfterImport>,
    ) -> Result<Value, CmdError> {
        if let Some(why) = self.import_run_disabled() {
            return Err(CmdError::Disabled(why.into()));
        }
        // The listing's state (and the library digests it made, kept).
        if self.import_list(0, 0)["state"] == "loading" {
            return Err(CmdError::Disabled(STILL_LISTING.into()));
        }
        let Some(source) = self.import.source().cloned() else { return Err(CmdError::Disabled(NOTHING_OPEN.into())) };
        crate::automation::lexical(&destination, "destination").map_err(|r| CmdError::BadParams(r.message))?;
        if destination.starts_with(&source.path) {
            return Err(CmdError::BadParams(format!("{} is inside the source being imported from", destination.display())));
        }
        let today = DateTime::from_unix_secs(self.clock.now().as_secs()).map(|d| d.date().to_string()).unwrap_or_default();
        let typed = match &folder {
            FolderName::Text(t) => {
                let t = t.trim();
                if t.is_empty() {
                    return Err(CmdError::BadParams("`folderName` is empty: choose a folder name".into()));
                }
                Some(names::sanitize(t, &today))
            }
            FolderName::DateTaken | FolderName::Today => None,
        };
        let candidates = self.import.candidates();
        if let Some(ids) = ids
            && let Some(id) = ids.iter().find(|id| !candidates.iter().any(|c| c.id == **id))
        {
            return Err(CmdError::NotFound(format!("no candidate {id} in this listing")));
        }
        let chosen: Vec<_> = candidates.iter().filter(|c| ids.is_none_or(|ids| ids.contains(&c.id))).collect();
        let excluded = chosen.iter().filter(|c| c.excluded).count();
        let duplicates = chosen.iter().filter(|c| !c.excluded && c.duplicate.is_some() && self.import.exclude_duplicates()).count();
        let items: Vec<CopyItem> = chosen
            .iter()
            .filter(|c| self.import.will_import(c))
            .enumerate()
            .map(|(order, c)| CopyItem {
                order,
                source: c.path.clone(),
                size: c.size,
                destination: destination.clone(),
                folder: match (&folder, &typed) {
                    (_, Some(name)) => name.clone(),
                    (FolderName::DateTaken, _) => c.captured.date().to_string(),
                    _ => today.clone(),
                },
                exact: None,
                expect: None,
                star: c.starred,
                turns: c.turns,
                digest: c.digest.clone(),
                index: 0,
            })
            .collect();
        let mut planned: Vec<String> = Vec::new();
        for i in &items {
            if !planned.contains(&i.folder) {
                planned.push(i.folder.clone());
            }
        }
        // The destination is watched unless a watched folder covers it; its scan waits for the job.
        let watched = !self.folder_rules().indexes(&destination);
        if watched {
            self.add_root(destination.clone());
        }
        let after = after.unwrap_or_else(|| self.settings().import().after);
        let total = items.len();
        let ledger: Shared = Arc::new(Mutex::new(Ledger::default()));
        let progress = Arc::new(Progress::new());
        // Noted before the job starts: a job that cannot start ends inside `start_file_job`.
        self.import_run.last = Some(Last {
            job: 0,
            source: source.clone(),
            destination: destination.clone(),
            after,
            ledger: Arc::clone(&ledger),
            progress: Arc::clone(&progress),
            started: Instant::now(),
            seconds: None,
            state: State::Copying,
            total,
            duplicates,
            excluded,
            failures: Vec::new(),
            removal: None,
        });
        let (gate, applied) = (self.import_run.gate.clone(), Arc::clone(&ledger));
        let job = FileJob::new(JobKind::Import, "Copying", items, move |item: CopyItem| {
            let got = copy_one(&item, &progress)?;
            if let Some(gate) = &gate {
                gate(item.order);
            }
            Ok(Done { changes: Vec::new(), value: (item, got) })
        })
        .on_applied(move |s: &mut Session, a: Applied<(CopyItem, Got)>| s.import_enter(&applied, a.value).map(|_| Vec::new()))
        .on_finish(move |s: &mut Session, tally: &Tally| s.import_finished(tally));
        let id = self.start_file_job(job);
        self.import_run.job = Some(id);
        if let Some(last) = self.import_run.last.as_mut() {
            last.job = id;
        }
        // The recent sources, destinations and folder names, and the room's "After copying".
        self.change_settings(|st| {
            let mut i = st.import();
            i.after = after;
            i.recent_sources.retain(|p| *p != source.path);
            i.recent_sources.insert(0, source.path.clone());
            i.recent_destinations.retain(|p| *p != destination);
            i.recent_destinations.insert(0, destination.clone());
            if let FolderName::Text(t) = &folder {
                let t = t.trim().to_string();
                i.recent_folder_names.retain(|n| *n != t);
                i.recent_folder_names.insert(0, t);
            }
            st.set_import(&i);
        });
        Ok(json!({
            "job": id, "pending": true, "total": total, "destination": destination, "folders": planned,
            "skipped": { "duplicates": duplicates, "excluded": excluded }, "watched": watched,
        }))
    }

    /// One copy enters the library (on the owner thread, as the job's result is applied); returns
    /// its folder and photo.
    fn import_enter(&mut self, ledger: &Shared, (item, got): (CopyItem, Got)) -> Result<(FolderId, PhotoId), String> {
        let dir = item.destination.join(&item.folder);
        let mut ops = Vec::new();
        let folder_id = match self.library.folder_by_path(&dir) {
            Some(id) => id,
            None => {
                let id = self.library.next_folder_id();
                ops.push(Op::AddFolder { folder: Folder { id, path: dir.clone() } });
                id
            }
        };
        if self.library.photo_by_name(folder_id, &got.name).is_some() {
            return Err(format!(
                "{} was copied into {}, but the library already shows a photo of that name there; the folder's next scan brings it in",
                got.name,
                shown(&dir)
            ));
        }
        let photo = self.library.next_photo_id();
        let f = &got.facts;
        ops.push(Op::AddPhotos {
            photos: vec![Photo {
                id: photo,
                folder: folder_id,
                name: got.name.clone(),
                size: f.size,
                mtime: f.mtime,
                width: f.width,
                height: f.height,
                date: f.date,
                star: item.star,
            }],
        });
        if let Some(recipe) = rotation(item.turns) {
            ops.push(Op::SetRecipes { recipes: vec![(photo, recipe)] });
        }
        if let Some(op) = batch(ops) {
            self.apply_own(op).map_err(|e| format!("{} was copied, but the library could not take it: {e}", got.name))?;
        }
        if !item.digest.is_empty() {
            let digest = ContentDigest { size: f.size, mtime: f.mtime, digest: item.digest.clone() };
            if let Err(e) = self.apply_derived(Op::SetDigests { digests: vec![(photo, Some(digest))] }) {
                log::warn!("the digest of the imported {} was not kept: {e}", got.name);
            }
        }
        lock(ledger).files.push(Copied {
            source: item.source,
            source_identity: got.source_identity,
            destination: item.destination,
            folder: item.folder,
            name: got.name,
            folder_id,
            photo,
            star: item.star,
            turns: item.turns,
            digest: item.digest,
            copy: Some(got.copy),
            kept: None,
            removed: false,
        });
        Ok((folder_id, photo))
    }

    /// The import job ended: its undo step, its result and its notice.
    fn import_finished(&mut self, tally: &Tally) -> Notice {
        let Some(last) = self.import_run.last.as_mut() else { return Notice::default() };
        last.seconds = Some(last.started.elapsed().as_secs_f64());
        last.state = if tally.cancelled { State::Cancelled } else { State::Done };
        last.failures = tally.failures.clone();
        let ledger = Arc::clone(&last.ledger);
        let total = last.total;
        let (copied, folders) = {
            let l = lock(&ledger);
            (l.files.len(), folders_of(&l.files))
        };
        if copied > 0 {
            let id = self.import_run.step_id();
            self.undo.push_files(FileStep {
                id,
                label: LABEL,
                batch: Arc::new(ImportBatch { ledger, indices: (0..copied).collect() }),
                running: None,
            });
        }
        Notice { message: finished_message(copied, total, &folders, tally), folder: folders.first().cloned() }
    }

    /// Undoes (`dir` Undo) or redoes the files `indices` of `ledger` as a file job; ends file step
    /// `step` on the undo stack when it ends.
    fn import_reverse(&mut self, ledger: Shared, indices: Vec<usize>, step: u64, dir: Direction, since: u64) -> JobId {
        let files = {
            let mut l = lock(&ledger);
            l.busy = true;
            l.files.clone()
        };
        let done: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
        let (applied_ledger, applied_done) = (Arc::clone(&ledger), Arc::clone(&done));
        let finish = {
            let indices = indices.clone();
            move |s: &mut Session, tally: &Tally| -> Notice {
                let mut done = std::mem::take(&mut *lock(&done));
                done.sort_unstable();
                done.dedup();
                lock(&ledger).busy = false;
                let rest: Vec<usize> = indices.iter().copied().filter(|i| !done.contains(i)).collect();
                let count = done.len();
                let moved = (!done.is_empty()).then(|| FileStep {
                    id: s.import_run.step_id(),
                    label: LABEL,
                    batch: Arc::new(ImportBatch { ledger: Arc::clone(&ledger), indices: done }),
                    running: None,
                });
                let rest: Option<Arc<dyn Batch>> =
                    (!rest.is_empty()).then(|| Arc::new(ImportBatch { ledger: Arc::clone(&ledger), indices: rest }) as Arc<dyn Batch>);
                s.undo.file_step_ended(step, dir == Direction::Undo, rest, moved, since);
                let label = if dir == Direction::Undo { "Undo Import" } else { "Redo Import" };
                Notice { message: default_message(label, &Tally { done: count, ..tally.clone() }), folder: None }
            }
        };
        match dir {
            Direction::Undo => {
                let items: Vec<UncopyItem> = indices
                    .iter()
                    .rev()
                    .filter_map(|i| files.get(*i).map(|c| UncopyItem { index: *i, path: c.dir().join(&c.name), expect: c.copy }))
                    .collect();
                let job = FileJob::new(JobKind::Import, "Undo Import", items, |item: UncopyItem| {
                    uncopy(&item)?;
                    Ok(Done { changes: Vec::new(), value: item.index })
                })
                .on_applied(move |s: &mut Session, a: Applied<usize>| {
                    s.import_uncopied(&applied_ledger, a.value);
                    lock(&applied_done).push(a.value);
                    Ok(Vec::new())
                })
                .on_finish(finish);
                self.start_file_job(job)
            }
            Direction::Do | Direction::Redo => {
                let items: Vec<CopyItem> = indices
                    .iter()
                    .enumerate()
                    .filter_map(|(order, i)| {
                        let c = files.get(*i)?;
                        Some(CopyItem {
                            order,
                            source: c.source.clone(),
                            size: c.source_identity.size(),
                            destination: c.destination.clone(),
                            folder: c.folder.clone(),
                            exact: Some(c.name.clone()),
                            expect: Some(c.source_identity),
                            star: c.star,
                            turns: c.turns,
                            digest: c.digest.clone(),
                            index: *i,
                        })
                    })
                    .collect();
                let progress = Progress::new();
                let job = FileJob::new(JobKind::Import, "Redo Import", items, move |item: CopyItem| {
                    let got = copy_one(&item, &progress)?;
                    Ok(Done { changes: Vec::new(), value: (item, got) })
                })
                .on_applied(move |s: &mut Session, a: Applied<(CopyItem, Got)>| {
                    let index = a.value.0.index;
                    s.import_recopied(&applied_ledger, a.value)?;
                    lock(&applied_done).push(index);
                    Ok(Vec::new())
                })
                .on_finish(finish);
                self.start_file_job(job)
            }
        }
    }

    /// Undo trashed copy `index` (or found it gone): its photo leaves the library, kept for redo.
    fn import_uncopied(&mut self, ledger: &Shared, index: usize) {
        let Some(c) = lock(ledger).files.get(index).cloned() else { return };
        let lib = &self.library;
        let still = lib.photo(c.photo).is_some_and(|p| p.name == c.name && lib.folder(p.folder).is_some_and(|f| f.path == c.dir()));
        let kept = if still {
            match self.apply_own(Op::RemovePhotos { ids: vec![c.photo] }) {
                Ok(inverse) => Some(inverse),
                Err(e) => {
                    log::warn!("undoing the import of {}: the library could not follow: {e}", c.name);
                    None
                }
            }
        } else {
            None
        };
        if let Some(f) = lock(ledger).files.get_mut(index) {
            f.copy = None;
            f.kept = kept;
        }
    }

    /// Redo copied file `index` again: what undo took out comes back (its folder first, when a
    /// scan took it out meanwhile), or the copy enters afresh.
    fn import_recopied(&mut self, ledger: &Shared, (item, got): (CopyItem, Got)) -> Result<(), String> {
        let index = item.index;
        let Some(c) = lock(ledger).files.get(index).cloned() else { return Err("no such imported file".into()) };
        let dir = c.dir();
        if self.library.folder(c.folder_id).is_none() && self.library.folder_by_path(&dir).is_none() {
            self.apply_own(Op::AddFolder { folder: Folder { id: c.folder_id, path: dir.clone() } }).map_err(|e| e.to_string())?;
        }
        let fits = self.library.folder(c.folder_id).is_some_and(|f| f.path == dir) && self.library.photo(c.photo).is_none();
        let back = match &c.kept {
            Some(op) if fits => self.apply_own(op.clone()).ok().map(|_| (c.folder_id, c.photo)),
            _ => None,
        };
        let copy = got.copy;
        let (folder_id, photo) = match back {
            Some(ids) => ids,
            // Afresh, under new ids.
            None => self.import_enter(&Arc::new(Mutex::new(Ledger::default())), (item, got))?,
        };
        if let Some(f) = lock(ledger).files.get_mut(index) {
            f.copy = Some(copy);
            f.kept = None;
            f.folder_id = folder_id;
            f.photo = photo;
        }
        Ok(())
    }

    /// Why `import.removeFromSource` cannot run now.
    pub(crate) fn import_remove_disabled(&self) -> Option<String> {
        let Some(last) = &self.import_run.last else { return Some("Nothing was imported in this session.".into()) };
        if last.state == State::Copying {
            return Some("The import is still copying.".into());
        }
        if last.source.kind == SourceKind::Folder {
            return Some("Only files imported from a card or a drive are removed from it.".into());
        }
        if last.removal.as_ref().is_some_and(|r| r.running) {
            return Some(REMOVAL_RUNNING.into());
        }
        let l = lock(&last.ledger);
        if l.busy {
            return Some("Undo Import or Redo Import is still running.".into());
        }
        if !l.files.iter().any(|f| f.copy.is_some() && !f.removed) {
            return Some("No copied file is left to remove from the card.".into());
        }
        None
    }

    /// `import.removeFromSource {confirm}`: removes the verified copied files of the last import
    /// from its card or drive, as a job.
    pub(crate) fn import_remove(&mut self, confirm: bool) -> Result<Value, CmdError> {
        if let Some(why) = self.import_remove_disabled() {
            return Err(CmdError::Disabled(why));
        }
        if !confirm {
            return Err(CmdError::Disabled(format!("{REMOVE_QUESTION} Send \"confirm\": true to go ahead.")));
        }
        let Some(last) = self.import_run.last.as_mut() else { return Err(CmdError::Disabled("Nothing was imported in this session.".into())) };
        let root = last.source.path.clone();
        let ledger = Arc::clone(&last.ledger);
        let items: Vec<RemoveItem> = {
            let mut l = lock(&ledger);
            // From now on Undo Import waits: it never trashes a copy whose original goes.
            l.blocked = Some(REMOVING.into());
            l.files
                .iter()
                .enumerate()
                .filter(|(_, f)| !f.removed)
                .filter_map(|(index, f)| {
                    Some(RemoveItem {
                        index,
                        root: root.clone(),
                        source: f.source.clone(),
                        expect: f.source_identity,
                        copy: f.dir().join(&f.name),
                        copy_identity: f.copy?,
                    })
                })
                .collect()
        };
        last.removal = Some(Removal { running: true, ..Removal::default() });
        let total = items.len();
        let applied = Arc::clone(&ledger);
        let job = FileJob::new(JobKind::Import, "Removing from the card", items, |item: RemoveItem| {
            let removed = remove::remove_copied(&item.root, &item.source, item.expect, &item.copy, item.copy_identity).map_err(fs_err)?;
            Ok(Done { changes: Vec::new(), value: (item.index, removed) })
        })
        .on_applied(move |s: &mut Session, a: Applied<(usize, Removed)>| {
            let (index, removed) = a.value;
            if let Some(f) = lock(&applied).files.get_mut(index) {
                f.removed = true;
            }
            if let Some(r) = s.import_run.last.as_mut().and_then(|l| l.removal.as_mut()) {
                match removed {
                    Removed::Trashed(_) => r.trashed += 1,
                    Removed::Deleted => r.deleted += 1,
                }
            }
            Ok(Vec::new())
        })
        .on_finish(move |s: &mut Session, tally: &Tally| {
            {
                let mut l = lock(&ledger);
                let any = l.files.iter().any(|f| f.removed);
                l.blocked = any.then(|| REMOVED.to_string());
            }
            if let Some(r) = s.import_run.last.as_mut().and_then(|l| l.removal.as_mut()) {
                r.running = false;
                r.kept = tally.failures.clone();
            }
            let mut message = format!("Removed {} from the card", pictures(tally.done));
            if !tally.failures.is_empty() {
                message.push_str(&format!("; {} stayed", tally.failures.len()));
            }
            Notice { message, folder: None }
        });
        let id = self.start_file_job(job);
        if let Some(r) = self.import_run.last.as_mut().and_then(|l| l.removal.as_mut()) {
            r.job = id;
        }
        Ok(json!({ "job": id, "pending": true, "total": total, "source": root }))
    }

    /// `import.status`: the import running or last finished, and its removal from the card.
    pub fn import_status(&self) -> Value {
        let Some(last) = &self.import_run.last else { return json!({ "state": null }) };
        let done = self.job(last.job).map_or(0, |j| j.progress.done);
        let seconds = last.seconds.unwrap_or_else(|| last.started.elapsed().as_secs_f64());
        let speed = speed_text(last.progress.bytes(), seconds);
        let l = lock(&last.ledger);
        let copied = l.files.len();
        let bytes: u64 = l.files.iter().map(|f| f.source_identity.size()).sum();
        let status = match (last.state, &speed) {
            (State::Copying, Some(s)) => format!("Copying {done} of {} at {s}", last.total),
            (State::Copying, None) => format!("Copying {done} of {}", last.total),
            _ => format!("{} imported", pictures(copied)),
        };
        let failures = |list: &[ItemFailure]| -> Vec<Value> { list.iter().map(|f| json!({ "file": f.file, "reason": f.reason })).collect() };
        let removal = last.removal.as_ref().map(|r| {
            json!({
                "job": r.job, "running": r.running, "removed": r.trashed + r.deleted, "trashed": r.trashed, "deleted": r.deleted,
                "kept": failures(&r.kept),
            })
        });
        json!({
            "job": last.job,
            "state": last.state.id(),
            "status": status,
            "source": { "kind": last.source.kind.id(), "name": last.source.name, "path": last.source.path },
            "destination": last.destination,
            "after": last.after.id(),
            "total": last.total,
            "imported": copied,
            "folders": folders_of(&l.files),
            "skipped": { "duplicates": last.duplicates, "excluded": last.excluded },
            "failed": failures(&last.failures),
            "bytes": bytes,
            "seconds": seconds,
            "speed": speed,
            "removable": last.source.kind != SourceKind::Folder,
            "undoBlocked": l.blocked,
            "removal": removal,
        })
    }
}

/// An import's files as one undo step (`undo.rs`).
struct ImportBatch {
    ledger: Shared,
    indices: Vec<usize>,
}

impl Batch for ImportBatch {
    fn len(&self) -> usize {
        self.indices.len()
    }

    fn blocked(&self, dir: Direction) -> Option<String> {
        match dir {
            Direction::Undo => lock(&self.ledger).blocked.clone(),
            Direction::Do | Direction::Redo => None,
        }
    }

    fn start(&self, s: &mut Session, step: u64, dir: Direction, since: u64) -> JobId {
        s.import_reverse(Arc::clone(&self.ledger), self.indices.clone(), step, dir, since)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_rotations_and_messages_read_as_the_room_shows_them() {
        assert_eq!(speed_text(38_000_000, 1.0).as_deref(), Some("38 MB/s"));
        assert_eq!(speed_text(3_400_000, 1.0).as_deref(), Some("3.4 MB/s"));
        assert_eq!(speed_text(1, 0.0), None);
        assert_eq!(rotation(0), None);
        assert_eq!(rotation(1).map(|r| r.edits), Some(vec!["rotate(90)".to_string()]));
        assert_eq!(rotation(3).map(|r| r.edits), Some(vec!["rotate(270)".to_string()]));
        let t = |cancelled, failures: usize| Tally {
            total: 3,
            done: 3,
            failures: (0..failures).map(|_| ItemFailure { file: PathBuf::from("a"), reason: "x".into() }).collect(),
            cancelled,
        };
        let one = [PathBuf::from("/p/2026-06-04")];
        assert_eq!(finished_message(340, 340, &one, &t(false, 0)), "Imported 340 pictures into 2026-06-04");
        assert_eq!(finished_message(1, 1, &one, &t(false, 0)), "Imported 1 picture into 2026-06-04");
        assert_eq!(finished_message(3, 3, &[PathBuf::from("/a"), PathBuf::from("/b")], &t(false, 1)), "Imported 3 pictures into 2 folders, 1 failed");
        assert_eq!(finished_message(10, 120, &one, &t(true, 0)), "Import cancelled: 10 of 120 pictures copied");
    }
}
