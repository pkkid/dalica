//! Walking the roots a folder at a time and reading what is found (`data-model.md` §7,
//! `security.md` §4.3).
//!
//! A scan lists each folder through `dalica-fs` (bounded, links followed safely, FIFOs and devices
//! never opened), leaves out what the [`SkipRules`] and the excluded subtrees say, recognises every
//! file by its content and reads its header inside a per-file guard: on a worker thread, inside
//! `catch_unwind`, within a time budget. Each folder's photos are handed out as soon as the folder
//! is read ([`scan_streaming`]), so a caller can show them while the walk goes on.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::{Entry, Limits};
use dalica_image::{Format, Header};
use dalica_library::{FolderInfo, MAX_CAPTION, MAX_DESCRIPTION, MAX_OPERATION, MAX_OPERATIONS, MAX_PLACE, PhotoInfo};
use dalica_sidecar::{Field, Sidecar};

use crate::meta::{FileValues, ReadMetadata};
use crate::skip::{FileTypes, SkipRules};

/// Files between two progress reports inside one folder.
const PROGRESS_EVERY: usize = 256;

/// The work budget of one file's header read (`security.md` §4.3).
pub const BUDGET: Duration = Duration::from_secs(10);

/// A photo file on disk and what its headers say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundPhoto {
    pub name: String,
    pub size: u64,
    pub mtime: Timestamp,
    /// The format its content says, whatever its name says.
    pub format: Format,
    /// As displayed (after EXIF orientation); 0 × 0 when the header could not be read (the
    /// thumbnail job says why).
    pub width: u32,
    pub height: u32,
    pub date: Option<DateTime>,
    /// From the folder's sidecar.
    pub star: bool,
    /// What the folder's sidecar says about the photo besides its star (caption, tags, hidden,
    /// recipe, redo list, date), as the model takes it ([`photo_info`]).
    pub info: PhotoInfo,
    /// What the file itself says about the fields Dalica owns (caption, tags, rating, place,
    /// date), read when [`Options::metadata`] asks for it and the file is new or changed; `None`
    /// when not read or unreadable ([`crate::meta`]).
    pub file: Option<FileValues>,
}

/// A folder holding at least one photo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFolder {
    pub path: PathBuf,
    /// Sorted by name.
    pub photos: Vec<FoundPhoto>,
    /// What the folder's sidecar says about the folder (description, date, place, hidden), as
    /// the model takes it ([`folder_info`]).
    pub info: FolderInfo,
}

/// Something a scan could not read; the scan went on without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub path: PathBuf,
    pub reason: String,
}

/// A file whose guarded read failed: an error, a panic, or more time than the budget. Given back
/// to the next scan in [`Options::failures`], it is not read again while its size and
/// modification time stay the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: Timestamp,
    /// What its content was recognised as before the read failed; `None` if even that failed
    /// (such a file is not listed).
    pub format: Option<Format>,
    pub reason: String,
}

/// How far a scan has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Folders listed and read.
    pub folders_done: usize,
    /// Folders found and still to read.
    pub folders_queued: usize,
    /// Files looked at.
    pub files: usize,
    /// Photos listed.
    pub found: usize,
    /// New failures.
    pub failed: usize,
}

/// What a streaming scan hands out as it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A folder holding photos, complete: every photo in it (a folder is never handed out twice).
    Folder(FoundFolder),
    /// After every folder read (whether or not it held photos), and every 256 files inside a big one.
    Progress(Progress),
    /// A file whose read failed for the first time (one given back unchanged is not reported again).
    Failed(Failure),
    Warning(Warning),
}

/// How a scan ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Roots walked to the end; folders under other roots are left as they are.
    pub walked: Vec<PathBuf>,
    /// Every failure in effect, sorted by path: the new ones and those given back that still
    /// match their file. Give them to the next scan.
    pub failures: Vec<Failure>,
    /// The cancel flag stopped it, or the receiver went away.
    pub cancelled: bool,
    pub progress: Progress,
}

/// What a scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Roots that were walked to the end; folders under other roots are left as they are.
    pub walked: Vec<PathBuf>,
    /// Sorted by path.
    pub folders: Vec<FoundFolder>,
    pub warnings: Vec<Warning>,
    /// As [`Summary::failures`].
    pub failures: Vec<Failure>,
    pub cancelled: bool,
}

/// Reads one file's header. The default is `dalica-image`'s, which reads only as much of the file
/// as the header needs; tests plant readers that panic or stall.
pub type HeaderReader = Arc<dyn Fn(&Path, &dalica_image::Limits) -> dalica_image::Result<Header> + Send + Sync>;

/// `dalica-image`'s header reader.
pub fn default_reader() -> HeaderReader {
    Arc::new(|path: &Path, limits: &dalica_image::Limits| dalica_image::read_header_file(path, limits).map(|(h, _)| h))
}

/// How to scan. Owned and `Send`, so a caller can move it to a scanning thread.
#[derive(Clone)]
pub struct Options {
    /// Options → File Types.
    pub types: FileTypes,
    pub skip: SkipRules,
    /// Folders never walked: subtrees set to Remove under a watched root. A root listed here is
    /// walked as an empty one.
    pub excluded: Vec<PathBuf>,
    /// Checked before every folder and every file.
    pub cancel: Option<Arc<AtomicBool>>,
    /// The failures of an earlier scan ([`Summary::failures`]).
    pub failures: Vec<Failure>,
    /// Each file's header read gets this long ([`BUDGET`]).
    pub budget: Duration,
    pub limits: dalica_image::Limits,
    pub reader: HeaderReader,
    /// Whether to read the owned metadata of new and changed photos ([`crate::meta`]); off by
    /// default.
    pub metadata: ReadMetadata,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            types: FileTypes::all(),
            skip: SkipRules::system(),
            excluded: Vec::new(),
            cancel: None,
            failures: Vec::new(),
            budget: BUDGET,
            limits: dalica_image::Limits::default(),
            reader: default_reader(),
            metadata: ReadMetadata::Off,
        }
    }
}

impl fmt::Debug for Options {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("types", &self.types)
            .field("skip", &self.skip)
            .field("excluded", &self.excluded)
            .field("cancel", &self.cancel)
            .field("failures", &self.failures.len())
            .field("budget", &self.budget)
            .field("limits", &self.limits)
            .field(
                "metadata",
                &match &self.metadata {
                    ReadMetadata::Off => "off".to_string(),
                    ReadMetadata::NewOrChanged(known) => format!("new or changed ({} known)", known.len()),
                },
            )
            .finish_non_exhaustive()
    }
}

/// Scans `roots` with the default options. `cancel` stops it early.
pub fn scan(roots: &[PathBuf], cancel: Option<&AtomicBool>) -> Scan {
    collect(roots, &Options::default(), cancel)
}

/// Scans `roots` with `options` and gathers everything into one [`Scan`].
pub fn scan_with(roots: &[PathBuf], options: &Options) -> Scan {
    collect(roots, options, options.cancel.as_deref())
}

fn collect(roots: &[PathBuf], options: &Options, cancel: Option<&AtomicBool>) -> Scan {
    let mut out = Scan::default();
    let summary = run(roots, options, cancel, &mut |event| {
        match event {
            Event::Folder(f) => out.folders.push(f),
            Event::Warning(w) => out.warnings.push(w),
            Event::Progress(_) | Event::Failed(_) => {}
        }
        true
    });
    out.folders.sort_by(|a, b| a.path.cmp(&b.path));
    out.walked = summary.walked;
    out.failures = summary.failures;
    out.cancelled = summary.cancelled;
    out
}

/// Scans `roots`, handing each [`Event`] to `emit` as it happens: each folder's photos as soon as
/// the folder is read, progress after every folder, failures and warnings as they are found.
/// `emit` returning `false` (a channel whose receiver is gone: `|e| tx.send(e).is_ok()`) stops the
/// scan like the cancel flag. Folders come depth first, each folder before its subfolders, in name
/// order; folders reached through links come after the rest of their root.
///
/// A cancelled scan stops before the next file; the folder it was reading is not handed out, and
/// the root it was walking is not in [`Summary::walked`].
pub fn scan_streaming(roots: &[PathBuf], options: &Options, mut emit: impl FnMut(Event) -> bool) -> Summary {
    run(roots, options, options.cancel.as_deref(), &mut emit)
}

/// What makes two paths the same folder: (device, inode) on Unix; the canonical path elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DirKey {
    #[cfg_attr(not(unix), allow(dead_code))]
    Inode(u64, u64),
    #[cfg_attr(unix, allow(dead_code))]
    Path(PathBuf),
}

#[cfg(unix)]
fn dir_key(path: &Path) -> Option<DirKey> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some(DirKey::Inode(m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn dir_key(path: &Path) -> Option<DirKey> {
    std::fs::canonicalize(path).ok().map(DirKey::Path)
}

/// One scan's state.
struct Scanner<'a> {
    options: &'a Options,
    cancel: Option<&'a AtomicBool>,
    emit: &'a mut dyn FnMut(Event) -> bool,
    reader: GuardedReader,
    given_back: HashMap<&'a Path, &'a Failure>,
    seen: HashSet<DirKey>,
    entries: usize,
    stopped: bool,
    summary: Summary,
}

/// How the walk of one root ended.
enum Walked {
    Complete,
    Incomplete,
}

fn run(roots: &[PathBuf], options: &Options, cancel: Option<&AtomicBool>, emit: &mut dyn FnMut(Event) -> bool) -> Summary {
    let mut s = Scanner {
        options,
        cancel,
        emit,
        reader: GuardedReader::new(options),
        given_back: options.failures.iter().map(|f| (f.path.as_path(), f)).collect(),
        seen: HashSet::new(),
        entries: 0,
        stopped: false,
        summary: Summary::default(),
    };
    for root in roots {
        if s.stopped || s.cancelled() {
            s.stop(root);
            break;
        }
        if s.excluded(root) {
            s.summary.walked.push(root.clone());
            continue;
        }
        if let Some(Walked::Complete) = s.walk_root(root) {
            s.summary.walked.push(root.clone());
        }
    }
    let mut summary = s.summary;
    summary.cancelled = s.stopped;
    summary.failures.sort_by(|a, b| a.path.cmp(&b.path));
    summary
}

impl Scanner<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }

    fn excluded(&self, path: &Path) -> bool {
        self.options.excluded.iter().any(|x| path.starts_with(x))
    }

    fn send(&mut self, event: Event) {
        if !self.stopped && !(self.emit)(event) {
            self.stopped = true;
        }
    }

    fn warn(&mut self, path: &Path, reason: String) {
        self.send(Event::Warning(Warning { path: path.to_path_buf(), reason }));
    }

    /// Marks the scan stopped (cancelled, or the receiver gone), with a warning on `at`.
    fn stop(&mut self, at: &Path) {
        self.warn(at, "Cancelled: the scan stopped before the end".into());
        self.stopped = true;
    }

    /// Walks one root, a folder at a time. `None`: the root could not be walked at all.
    fn walk_root(&mut self, root: &Path) -> Option<Walked> {
        match std::fs::metadata(root) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                self.warn(root, "not a folder".into());
                return None;
            }
            Err(e) => {
                self.warn(root, format!("cannot read the folder: {e}"));
                return None;
            }
        }
        if let Some(key) = dir_key(root) {
            self.seen.insert(key);
        }
        // Folders still to read, popped from the end: (path, depth below the root).
        let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
        // Folder links wait until the real tree is read, so a real folder always wins over a link to it.
        let mut links: Vec<(PathBuf, usize)> = Vec::new();
        loop {
            while let Some((dir, depth)) = stack.pop() {
                if self.stopped || self.cancelled() {
                    self.stop(&dir);
                    return Some(Walked::Incomplete);
                }
                let Some(listing) = self.list(&dir, root)? else { continue };
                if listing.truncated {
                    if self.cancelled() {
                        self.stop(&dir);
                    }
                    return Some(Walked::Incomplete);
                }
                let mut subdirs = Vec::new();
                let mut files = Vec::new();
                for e in listing.entries {
                    if e.is_dir {
                        if e.is_link {
                            links.push((e.path, depth + 1));
                        } else {
                            subdirs.push(e.path);
                        }
                    } else if e.is_file {
                        // Regular files only: a FIFO or device in a watched folder must never block a scan.
                        files.push(e);
                    }
                }
                for sub in subdirs.into_iter().rev() {
                    if let Some(sub) = self.enter(sub, depth + 1) {
                        stack.push(sub);
                    }
                }
                self.summary.progress.folders_queued = stack.len().saturating_add(links.len());
                let Some(folder) = self.read_folder(&dir, files) else {
                    self.stop(&dir);
                    return Some(Walked::Incomplete);
                };
                if !folder.photos.is_empty() {
                    self.send(Event::Folder(folder));
                }
                self.summary.progress.folders_done = self.summary.progress.folders_done.saturating_add(1);
                let progress = self.summary.progress;
                self.send(Event::Progress(progress));
            }
            if links.is_empty() {
                return Some(Walked::Complete);
            }
            links.sort_by(|a, b| a.0.components().cmp(b.0.components()));
            for (link, depth) in std::mem::take(&mut links) {
                if let Some(sub) = self.enter(link, depth) {
                    stack.push(sub);
                }
            }
            // Read the followed links' folders first link first.
            stack.reverse();
        }
    }

    /// Lists the entries of `dir` the scan keeps. `None`: the root itself could not be listed.
    /// `Some(None)`: a folder below it could not (warned, skipped).
    fn list(&mut self, dir: &Path, root: &Path) -> Option<Option<Listing>> {
        let limits = Limits { max_depth: 1, max_entries: dalica_fs::walk::MAX_ENTRIES.saturating_sub(self.entries), cancel: self.cancel };
        let skip = &self.options.skip;
        let excluded = &self.options.excluded;
        let walk = dalica_fs::walk(dir, limits, |e: &Entry| skip.skips(e) || (e.is_dir && excluded.iter().any(|x| e.path.starts_with(x))));
        let walk = match walk {
            Ok(w) => w,
            Err(e) => {
                self.warn(dir, e.to_string());
                return if dir == root { None } else { Some(None) };
            }
        };
        let mut truncated = false;
        for w in walk.warnings {
            match w {
                // Every subfolder is at the one-level limit of this listing; the scan walks it next.
                dalica_fs::Warning::TooDeep(_) => {}
                dalica_fs::Warning::Cancelled => truncated = true,
                dalica_fs::Warning::TooManyEntries => {
                    self.warn(dir, format!("more than {} entries: the scan stopped here", dalica_fs::walk::MAX_ENTRIES));
                    truncated = true;
                }
                dalica_fs::Warning::Loop(p) => self.warn(&p, "a link to a folder already scanned".into()),
                dalica_fs::Warning::Unreadable { path, reason } => self.warn(&path, reason),
            }
        }
        self.entries = self.entries.saturating_add(walk.entries.len());
        Some(Some(Listing { entries: walk.entries, truncated }))
    }

    /// Whether to walk the folder `path` at `depth` below its root: not past the depth limit, not
    /// a folder already walked.
    fn enter(&mut self, path: PathBuf, depth: usize) -> Option<(PathBuf, usize)> {
        if depth >= dalica_fs::walk::MAX_DEPTH {
            self.warn(&path, format!("deeper than {} folders: not scanned", dalica_fs::walk::MAX_DEPTH));
            return None;
        }
        let new = dir_key(&path).is_none_or(|key| self.seen.insert(key));
        if new {
            Some((path, depth))
        } else {
            self.warn(&path, "a link to a folder already scanned".into());
            None
        }
    }

    /// Reads one folder's files. `None`: the scan was cancelled before the folder was finished.
    fn read_folder(&mut self, dir: &Path, files: Vec<Entry>) -> Option<FoundFolder> {
        let types = self.options.types;
        let mut photos = Vec::new();
        let mut to_read: Vec<(Entry, String)> = Vec::new();
        // Every file name of the folder, for the RAWs' `.xmp` sidecars.
        let names: Vec<String> = match self.options.metadata {
            ReadMetadata::Off => Vec::new(),
            ReadMetadata::NewOrChanged(_) => files.iter().filter_map(|e| e.path.file_name().and_then(|n| n.to_str()).map(str::to_string)).collect(),
        };
        for entry in files {
            let Some(name) = entry.path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                self.warn(&entry.path, "the file name is not valid Unicode".into());
                continue;
            };
            // A failure given back unchanged: not read again, listed as before.
            if let Some(&old) = self.given_back.get(entry.path.as_path())
                && old.size == entry.size
                && old.mtime == entry.modified
            {
                self.summary.progress.files = self.summary.progress.files.saturating_add(1);
                self.summary.failures.push(old.clone());
                if let Some(f) = old.format.filter(|&f| types.allows_format(f)) {
                    photos.push(unread(&entry, &name, f));
                }
                continue;
            }
            to_read.push((entry, name));
        }
        let paths: Vec<PathBuf> = to_read.iter().map(|(e, _)| e.path.clone()).collect();
        let cancel = self.cancel;
        // The receiver went away while the folder was read.
        let gone = Cell::new(false);
        let mut progress = self.summary.progress;
        let mut failures = Vec::new();
        let emit = &mut *self.emit;
        let mut send = |event: Event| {
            if !gone.get() && !emit(event) {
                gone.set(true);
            }
        };
        let finished = self.reader.read_all(&paths, types, &|| gone.get() || cancel.is_some_and(|c| c.load(Ordering::Relaxed)), &mut |i, outcome| {
            let Some((entry, name)) = to_read.get(i) else { return };
            progress.files = progress.files.saturating_add(1);
            let failed = |format: Option<Format>, reason: String| Failure {
                path: entry.path.clone(),
                size: entry.size,
                mtime: entry.modified,
                format,
                reason,
            };
            let (photo, failure) = match outcome {
                Outcome::NotPhoto | Outcome::TypeOff | Outcome::Cancelled => (None, None),
                Outcome::Header(h) => {
                    let (width, height) = h.display_size();
                    let date = h.exif.as_ref().and_then(|e| e.date_time_original);
                    (Some(FoundPhoto { width, height, date, ..unread(entry, name, h.format) }), None)
                }
                // Recognised, not decoded yet: listed (a format tile until its decoder arrives).
                Outcome::HeaderError(f, dalica_image::Error::Unsupported(_)) => (Some(unread(entry, name, f)), None),
                Outcome::HeaderError(f, e) => (Some(unread(entry, name, f)), Some(failed(Some(f), e.to_string()))),
                Outcome::Failed(f, reason) => (f.map(|f| unread(entry, name, f)), Some(failed(f, reason))),
            };
            photos.extend(photo);
            if let Some(failure) = failure {
                progress.failed = progress.failed.saturating_add(1);
                failures.push(failure.clone());
                send(Event::Failed(failure));
            }
            // A big folder reports its progress as it goes, not only at its end.
            if progress.files.is_multiple_of(PROGRESS_EVERY) {
                send(Event::Progress(progress));
            }
        });
        self.summary.progress = progress;
        self.summary.failures.extend(failures);
        if gone.get() {
            self.stopped = true;
        }
        if !finished {
            return None;
        }
        if let ReadMetadata::NewOrChanged(known) = &self.options.metadata
            && !photos.is_empty()
        {
            // Read here, on the scan's thread, in the same pass: what the files say comes in with
            // the photos (`diff`).
            let cancel = self.cancel;
            let cancelled = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
            if !crate::meta::read_folder(dir, &names, &mut photos, known, &dalica_meta::Limits::default(), &cancelled) {
                return None;
            }
        }
        let mut info = FolderInfo::default();
        if !photos.is_empty() {
            // Read here, on the scan's thread: everything the user said in the sidecar comes in
            // with the photos, in the same op (`diff`), so no sidecar write can run in between.
            if let Some(sidecar) = self.read_sidecar(dir) {
                for p in &mut photos {
                    p.star = sidecar.star(&p.name) == Field::Value(true);
                    p.info = photo_info(&sidecar, &p.name);
                }
                info = folder_info(&sidecar);
            }
            photos.sort_by(|a, b| a.name.cmp(&b.name));
            self.summary.progress.found = self.summary.progress.found.saturating_add(photos.len());
        }
        Some(FoundFolder { path: dir.to_path_buf(), photos, info })
    }

    /// The folder's sidecar, if it has a readable one (a damaged one is the store's to set aside).
    fn read_sidecar(&mut self, folder: &Path) -> Option<Sidecar> {
        let path = folder.join(dalica_sidecar::FILE_NAME);
        let limits = dalica_sidecar::Limits::default();
        match dalica_fs::read(&path, limits.max_bytes as u64) {
            Ok(None) => None,
            Ok(Some(r)) => match Sidecar::parse(&r.bytes, &limits) {
                Ok(s) => Some(s),
                Err(e) => {
                    self.warn(&path, e.to_string());
                    None
                }
            },
            Err(e) => {
                self.warn(&path, e.to_string());
                None
            }
        }
    }
}

/// A photo whose header was not read: listed at 0 × 0.
fn unread(entry: &Entry, name: &str, format: Format) -> FoundPhoto {
    FoundPhoto {
        name: name.to_string(),
        size: entry.size,
        mtime: entry.modified,
        format,
        width: 0,
        height: 0,
        date: None,
        star: false,
        info: PhotoInfo::default(),
        file: None,
    }
}

/// What `sidecar` says about photo `name` (its star aside), as a photo new to the model takes it
/// (`data-model.md` §3, the rules of `dalica-store`'s `mapping::read_photo`): a value of the wrong
/// type is not used (the photo keeps the empty value, and the file keeps its value: writing only
/// changes keys whose value differs from what the file reads as); a caption past the cap is cut;
/// tags are cleaned; a recipe or redo list past the caps is not used.
pub fn photo_info(sidecar: &Sidecar, name: &str) -> PhotoInfo {
    let f = sidecar.photo(name);
    let tags: Vec<String> = usable(f.tags);
    PhotoInfo {
        caption: cut(usable(f.caption), MAX_CAPTION, dalica_library::clean_caption),
        tags: dalica_library::clean_tags(tags.iter().map(String::as_str)),
        hidden: usable(f.hidden),
        edits: operations(usable(f.edits)),
        redo: operations(usable(f.redo)),
        date: usable_value(f.date),
    }
}

/// What `sidecar` says about its folder, as a folder new to the model takes it (the rules of
/// `dalica-store`'s `mapping::read_folder`, as [`photo_info`]).
pub fn folder_info(sidecar: &Sidecar) -> FolderInfo {
    let f = sidecar.folder();
    FolderInfo {
        description: cut(usable(f.description), MAX_DESCRIPTION, dalica_library::clean_description),
        date: usable_value(f.date),
        place: cut(usable(f.place), MAX_PLACE, dalica_library::clean_place),
        hidden: usable(f.hidden),
    }
}

/// The value read, or the empty one when the key is absent or holds the wrong type.
fn usable<T: Default>(f: Field<T>) -> T {
    match f {
        Field::Value(v) => v,
        Field::Absent | Field::WrongType(_) => T::default(),
    }
}

fn usable_value<T>(f: Field<T>) -> Option<T> {
    match f {
        Field::Value(v) => Some(v),
        Field::Absent | Field::WrongType(_) => None,
    }
}

/// Text past `max` bytes cleaned (cut); shorter text as it is.
fn cut(text: String, max: usize, clean: fn(&str) -> String) -> String {
    if text.len() > max { clean(&text) } else { text }
}

/// A recipe or redo list, or none past the caps of `dalica-library`.
fn operations(list: Vec<String>) -> Vec<String> {
    if list.len() > MAX_OPERATIONS || list.iter().any(|op| op.len() > MAX_OPERATION) { Vec::new() } else { list }
}

/// One folder's kept entries.
struct Listing {
    entries: Vec<Entry>,
    /// The entry limit or the cancel flag stopped the listing.
    truncated: bool,
}

/// What the guarded read of one file gave.
enum Outcome {
    /// Its content is no format Dalica knows.
    NotPhoto,
    /// A format whose type is turned off.
    TypeOff,
    Header(Header),
    /// Recognised as this format, but its header could not be read.
    HeaderError(Format, dalica_image::Error),
    /// The read panicked, ran out of time or could not open the file; the format if it was
    /// recognised before that.
    Failed(Option<Format>, String),
    /// The cancel flag was set before the file was read.
    Cancelled,
}

/// What the reading thread says about one file, in order: `Sniffed`, then (for a recognised
/// format) one of the others; or only `Io` or `Cancelled`.
enum Reply {
    Sniffed(Option<Format>),
    TypeOff,
    Header(dalica_image::Result<Header>),
    Io(String),
    Panicked(String),
    Cancelled,
}

/// A file for the reading thread.
struct Job {
    path: PathBuf,
    types: FileTypes,
}

/// The reading thread's two ends.
struct Worker {
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
}

/// Files handed to the reading thread ahead of the one waited for, so it never idles between
/// files; the cancel flag (given as an `Arc`) is checked by the thread itself before each one.
const READ_AHEAD: usize = 16;

/// Reads headers on a worker thread, inside `catch_unwind`, each within the budget. A worker that
/// overruns is abandoned (its thread ends when its read does) and a fresh one takes the next file.
struct GuardedReader {
    reader: HeaderReader,
    limits: dalica_image::Limits,
    budget: Duration,
    /// The scan's cancel flag, when it is shared: the reading thread stops at the next file.
    cancel: Option<Arc<AtomicBool>>,
    worker: Option<Worker>,
}

/// The start of the reason given to a file that ran out of time.
const OVERRUN: &str = "took longer than the work budget";
/// The start of the reason given when the reading thread went away.
const STOPPED: &str = "the reading thread stopped";

impl GuardedReader {
    fn new(options: &Options) -> GuardedReader {
        GuardedReader { reader: options.reader.clone(), limits: options.limits, budget: options.budget, cancel: options.cancel.clone(), worker: None }
    }

    fn spawn(&self) -> Option<Worker> {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (reply_tx, replies) = mpsc::channel::<Reply>();
        let (reader, limits, cancel) = (self.reader.clone(), self.limits, self.cancel.clone());
        std::thread::Builder::new()
            .name("dalica-scan-read".into())
            .spawn(move || {
                for job in job_rx {
                    let going_on = if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                        reply_tx.send(Reply::Cancelled).is_ok()
                    } else {
                        run_job(&job, &reader, &limits, &mut |r| reply_tx.send(r).is_ok())
                    };
                    if !going_on {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Worker { jobs, replies })
    }

    /// Reads `paths` in order, handing each one's outcome to `on` with its index. Returns `false`
    /// when `cancelled` stopped it before the end.
    fn read_all(&mut self, paths: &[PathBuf], types: FileTypes, cancelled: &dyn Fn() -> bool, on: &mut dyn FnMut(usize, Outcome)) -> bool {
        // Files handed to the current worker: `paths[i..sent]` are on their way.
        let mut sent = 0;
        for (i, path) in paths.iter().enumerate() {
            if cancelled() {
                return false;
            }
            if self.worker.is_none() {
                self.worker = self.spawn();
                sent = i;
            }
            let outcome = match &self.worker {
                // No thread to be had: read here, still inside `catch_unwind`, without the time budget.
                None => {
                    let mut replies = Vec::new();
                    run_job(&Job { path: path.clone(), types }, &self.reader, &self.limits, &mut |r| {
                        replies.push(r);
                        true
                    });
                    outcome(replies.into_iter().map(Ok))
                }
                Some(worker) => {
                    while let Some(next) = paths.get(sent).filter(|_| sent < i.saturating_add(READ_AHEAD)) {
                        if worker.jobs.send(Job { path: next.clone(), types }).is_err() {
                            break;
                        }
                        sent += 1;
                    }
                    if sent <= i {
                        Outcome::Failed(None, format!("{STOPPED} before reading the file"))
                    } else {
                        let deadline = Instant::now() + self.budget;
                        outcome(std::iter::from_fn(|| Some(worker.replies.recv_timeout(deadline.saturating_duration_since(Instant::now())))))
                    }
                }
            };
            match &outcome {
                Outcome::Cancelled => return false,
                // Abandon it: it is stuck in (or lost) this file; the next file gets a fresh thread.
                Outcome::Failed(_, reason) if reason.starts_with(OVERRUN) || reason.starts_with(STOPPED) => self.worker = None,
                _ => {}
            }
            on(i, outcome);
        }
        true
    }
}

/// Folds one file's replies into its outcome, stopping at the first that decides it.
fn outcome(replies: impl Iterator<Item = Result<Reply, RecvTimeoutError>>) -> Outcome {
    let mut format = None;
    for reply in replies {
        match reply {
            Ok(Reply::Sniffed(None)) => return Outcome::NotPhoto,
            Ok(Reply::Sniffed(Some(f))) => format = Some(f),
            Ok(Reply::TypeOff) => return Outcome::TypeOff,
            Ok(Reply::Cancelled) => return Outcome::Cancelled,
            Ok(Reply::Header(Ok(h))) => return Outcome::Header(h),
            Ok(Reply::Header(Err(e))) => {
                return match format {
                    Some(f) => Outcome::HeaderError(f, e),
                    None => Outcome::Failed(None, e.to_string()),
                };
            }
            Ok(Reply::Io(e)) => return Outcome::Failed(format, format!("cannot read the file: {e}")),
            Ok(Reply::Panicked(m)) => return Outcome::Failed(format, format!("reading the header panicked: {m}")),
            Err(RecvTimeoutError::Timeout) => return Outcome::Failed(format, OVERRUN.to_string()),
            Err(RecvTimeoutError::Disconnected) => return Outcome::Failed(format, format!("{STOPPED} while reading the file")),
        }
    }
    Outcome::Failed(format, format!("{STOPPED} while reading the file"))
}

/// Recognises one file and reads its header, sending each step to `reply` (which returns `false`
/// when nobody listens any more). Returns whether the listener is still there.
fn run_job(job: &Job, reader: &HeaderReader, limits: &dalica_image::Limits, reply: &mut dyn FnMut(Reply) -> bool) -> bool {
    let mut listening = true;
    let mut send = |r: Reply| {
        listening = listening && reply(r);
        listening
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let head = match read_head(&job.path) {
            Ok(h) => h,
            Err(e) => {
                send(Reply::Io(e.to_string()));
                return;
            }
        };
        let format = dalica_image::sniff(&head);
        if !send(Reply::Sniffed(format)) {
            return;
        }
        match format {
            Some(f) if !job.types.allows_format(f) => {
                send(Reply::TypeOff);
            }
            Some(_) => {
                send(Reply::Header(reader(&job.path, limits)));
            }
            None => {}
        }
    }));
    if let Err(panic) = result {
        let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
        send(Reply::Panicked(message));
    }
    listening
}

/// The first bytes of a file, as many as [`dalica_image::sniff`] needs.
fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::with_capacity(dalica_image::sniff::SNIFF_LEN);
    std::fs::File::open(path)?.take(dalica_image::sniff::SNIFF_LEN as u64).read_to_end(&mut head)?;
    Ok(head)
}
