//! The photo viewer's session (`viewing.md` §3, `roadmap.md` M11): the file `dalica --view`
//! opened, its folder's photos in name order, and whether the viewer is read-only because another
//! Dalica holds the library.
//!
//! - **Opening** ([`Session::open_viewer`]) names the file and nothing else: the folder is listed
//!   on a worker thread (the scan's own reading of one folder, [`dalica_scan::scan_streaming`]
//!   stopped after it, so the photos, their headers and what the folder's sidecar says are read as
//!   a scan reads them), and `viewer.list` answers `{pending: true}` until it is there (ask again
//!   after a frame). Viewing writes nothing and adds nothing to the library.
//! - **Whose photos:** a photo of a folder the library holds is the library's own (its id, its
//!   star as the library has it); any other is shown from its file and its folder's sidecar, read
//!   only.
//! - **The first change** to a photo of a folder the library does not hold (`viewer.star`,
//!   `viewer.rotate`, `viewer.delete`, and `viewer.adopt` for Open in Dalica) adds that folder as
//!   Scan Once, as Open File… would: the folder and its photos as listed enter the library and its
//!   rule is set inside the same command, so the change and the adding are **one** undo step and
//!   one Undo takes both back. A delete is a file job with a step of its own (`fileops.rs`): there
//!   the adding is a library step of its own, before the trash's.
//! - **Locked:** when another Dalica holds the library, the app opens the viewer on an empty
//!   read-only library of its own ([`Session::open_locked_viewer`]): every photo is shown from its
//!   file and sidecar, and the changes and Open in Dalica are disabled with [`ALREADY_OPEN`].
//! - **Pixels** ([`Session::viewer_image`], [`Session::viewer_thumbnail`]): a listed photo is
//!   decoded by its path on the viewer's own workers, never on the owner thread: at the screen size
//!   (the thumbnail ladder's screen entry size) for the photo shown, and at [`THUMB_SIDE`] for the
//!   filmstrip, both under the decoders' limits and with the edits its folder's sidecar gives
//!   (rendered with `edit`). The next and previous photos are decoded ahead
//!   ([`Session::prefetch_viewer_images`]). What was decoded is kept in memory only (at most
//!   [`MAX_SCREENS`] screen-size pictures and [`THUMB_CAP_BYTES`] of thumbnails, the least recently
//!   used dropped), and a file that cannot be decoded gives its reason. Only the photos the viewer
//!   lists are read. The library's own photos are drawn from the edit view's preview instead.
//! - **Closing** is the window's to carry out: the viewer's UI calls [`Session::close_viewer`]
//!   (Esc, ×) or [`Session::open_in_dalica`], and the app reads [`Viewer::closed`] to quit, or to
//!   turn the viewer's window into the main window on the edit view of that photo.
//!
//! What may be read: `viewer.list` lists the folder of the file the viewer was opened with (the
//! user's own choice on the command line) or a folder the library indexes; any other folder is
//! refused, so automation reads nothing through the viewer that it could not read otherwise
//! (`control-protocol.md` §5.7).

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dalica_core::{Clock, PhotoId};
use dalica_image::{DecodeOptions, Limits, Rgba8};
use dalica_scan::{Event, Failure, FoundFolder, Mode, Rule, Scan};
use dalica_thumbs::{Pool, Priority};
use serde_json::{Value, json};

use crate::error::{CmdError, OpenError};
use crate::session::{Options, Session};

/// Why the viewer's changes and Open in Dalica are disabled while another Dalica holds the library.
pub const ALREADY_OPEN: &str = "Dalica is already open";
/// Why `viewer.list` refuses a folder that is neither the viewer's nor indexed by the library.
pub const NOT_THE_VIEWERS: &str = "The photo viewer lists only the folder of the file it was opened with, or a folder Dalica indexes.";
/// Why a viewer change refuses a path that is not one of the listed photos.
pub const NOT_LISTED: &str = "is not one of the photos the viewer lists: list its folder first (viewer.list)";
/// Why a viewer command that needs the open viewer cannot run.
pub const NOT_OPEN: &str = "The photo viewer is not open: give a `path`.";

/// How the viewer was closed, for the window to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closed {
    /// Esc or ×: the app quits.
    Quit,
    /// Open in Dalica: the window becomes the main window, on the edit view of this photo.
    OpenInDalica(PhotoId),
}

/// The viewer's decoding workers.
pub const IMAGE_WORKERS: usize = 2;
/// The long side of a filmstrip thumbnail decoded by path (pixels).
pub const THUMB_SIDE: u32 = 160;
/// Screen-size pictures kept in memory (the photo shown, its neighbours, one more).
pub const MAX_SCREENS: usize = 4;
/// Bytes of filmstrip thumbnails kept in memory.
pub const THUMB_CAP_BYTES: usize = 32 * 1024 * 1024;
/// Results kept at most, failures included.
pub const MAX_KEPT: usize = 4096;

/// What a listed photo's pixels are now.
#[derive(Clone, Debug, PartialEq)]
pub enum Pixels {
    /// Decoded, upright, with the edits of its folder's sidecar.
    Ready(Arc<Rgba8>),
    /// On its way (asked for now if it was not).
    Pending,
    /// Why it cannot be shown.
    Failed(String),
}

/// One photo of the viewer's folder as the scan read it.
#[derive(Clone, Debug, PartialEq)]
struct Listed {
    name: String,
    width: u32,
    height: u32,
    /// The star the folder's sidecar gives it (shown while the library does not hold the photo).
    star: bool,
    /// The edits its folder's sidecar gives it (drawn while the library does not hold it).
    edits: Vec<String>,
    /// A video's facts as the scan read them (`None`: a still): listed with the video tile, never
    /// decoded (M15.16).
    video: Option<dalica_library::VideoFacts>,
}

/// The two sizes decoded by path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Screen,
    Thumb,
}

type ImageKey = (PathBuf, Kind);

/// The viewer's decodes by path: the workers, what they delivered, what is on its way.
struct Images {
    pool: Pool<ImageKey, Result<Rgba8, String>>,
    kept: HashMap<ImageKey, (Result<Arc<Rgba8>, String>, u64)>,
    asked: HashSet<ImageKey>,
    /// The screen-size pictures wanted now (the photo shown and its neighbours).
    wanted: Vec<PathBuf>,
    tick: u64,
}

impl std::fmt::Debug for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Images").field("kept", &self.kept.len()).field("asked", &self.asked.len()).finish()
    }
}

impl Images {
    fn new() -> Images {
        Images { pool: Pool::new(IMAGE_WORKERS), kept: HashMap::new(), asked: HashSet::new(), wanted: Vec::new(), tick: 0 }
    }

    /// Takes in what the workers delivered, then drops the least recently used past the caps.
    fn take(&mut self) {
        while let Some((key, result)) = self.pool.try_recv() {
            self.asked.remove(&key);
            self.tick = self.tick.saturating_add(1);
            self.kept.insert(key, (result.map(Arc::new), self.tick));
        }
        self.prune();
    }

    fn prune(&mut self) {
        let bytes = |kept: &HashMap<ImageKey, (Result<Arc<Rgba8>, String>, u64)>, kind: Kind| -> (usize, usize) {
            kept.iter().filter(|((_, k), _)| *k == kind).fold((0, 0), |(n, b), (_, (r, _))| (n + 1, b + r.as_ref().map_or(0, |i| i.data.len())))
        };
        loop {
            let (screens, _) = bytes(&self.kept, Kind::Screen);
            let (_, thumb_bytes) = bytes(&self.kept, Kind::Thumb);
            let kind = if screens > MAX_SCREENS {
                Some(Kind::Screen)
            } else if thumb_bytes > THUMB_CAP_BYTES {
                Some(Kind::Thumb)
            } else if self.kept.len() > MAX_KEPT {
                None
            } else {
                return;
            };
            let oldest = self.kept.iter().filter(|((_, k), _)| kind.is_none_or(|w| *k == w)).min_by_key(|(_, (_, t))| *t).map(|(key, _)| key.clone());
            match oldest {
                Some(key) => {
                    self.kept.remove(&key);
                }
                None => return,
            }
        }
    }
}

/// Decodes the photo at `path` to fit `long_side` pixels, upright, under `limits`, and draws
/// `edits` over it (on a worker).
fn decode(path: &Path, long_side: u32, options: &DecodeOptions, limits: &Limits, edits: &[String], cancel: &AtomicBool) -> Result<Rgba8, String> {
    let bytes = match dalica_fs::read(path, dalica_thumbs::MAX_FILE) {
        Ok(Some(r)) => r.bytes,
        Ok(None) => return Err("the file is missing".into()),
        Err(e) => return Err(e.to_string()),
    };
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    dalica_image::read_header(&bytes, limits).map_err(|e| e.reason())?;
    let image = dalica_image::decode_thumbnail_with(&bytes, long_side, options, limits).map_err(|e| e.reason())?;
    let recipe = dalica_edit::Recipe::parse(edits).unwrap_or_default();
    if recipe.ops().is_empty() {
        return Ok(image);
    }
    dalica_edit::render(dalica_edit::Source::upright(&image), &recipe, dalica_edit::Target::Full).map_err(|e| e.to_string())
}

/// What the worker read for one file: its folder's photos, in name order.
#[derive(Debug)]
struct Listing {
    /// The file asked for.
    file: PathBuf,
    folder: PathBuf,
    /// The folder as the scan read it (`None` when it holds no photo Dalica shows): what the
    /// first change adds to the library.
    found: Option<FoundFolder>,
    photos: Vec<Listed>,
    /// Why the file asked for cannot be shown; `None` when it is listed.
    refused: Option<String>,
}

/// The read on its way.
#[derive(Debug)]
struct Reading {
    file: PathBuf,
    rx: Receiver<Listing>,
}

/// The viewer's state; nothing open unless the app was started with `--view`.
#[derive(Debug, Default)]
pub struct Viewer {
    file: Option<PathBuf>,
    /// Why changes are refused (another Dalica holds the library).
    locked: Option<&'static str>,
    listing: Option<Listing>,
    reading: Option<Reading>,
    /// A read that could not run, given once.
    failed: Option<(PathBuf, String)>,
    closed: Option<Closed>,
    /// Pixels decoded by path (made at the first ask).
    images: Option<Images>,
}

impl Viewer {
    /// The file the viewer shows.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// Whether the viewer is open (and not closed yet).
    pub fn is_open(&self) -> bool {
        self.file.is_some() && self.closed.is_none()
    }

    /// Changes are refused ("Dalica is already open").
    pub fn read_only(&self) -> bool {
        self.locked.is_some()
    }

    /// Why changes are refused, when they are.
    pub fn read_only_reason(&self) -> Option<&'static str> {
        self.locked
    }

    /// How the viewer was closed, once it was: the window quits, or becomes the main window.
    pub fn closed(&self) -> Option<Closed> {
        self.closed
    }
}

impl Session {
    /// Opens the photo viewer on `file` (`dalica --view FILE`): an absolute path, never read on
    /// this thread; the folder is listed on a worker (`viewer.list`). `BadParams` for a path that
    /// is relative, climbs with `..` or names no file.
    pub fn open_viewer(&mut self, file: &Path) -> Result<(), CmdError> {
        let file = viewer_path(file)?;
        let v = &mut self.viewer;
        v.file = Some(file.clone());
        v.closed = None;
        v.listing = None;
        v.failed = None;
        self.start_listing(file);
        Ok(())
    }

    /// The viewer when another Dalica holds the library: a read-only session on the empty library
    /// in `scratch` (a folder of the caller's, removed by it once the session is gone), with the
    /// viewer's changes disabled ([`ALREADY_OPEN`]) and `file` opened (a file the viewer cannot
    /// take is logged; `viewer.list` then says why).
    pub fn open_locked_viewer(scratch: &Path, file: &Path, clock: Arc<dyn Clock>, options: &Options) -> Result<Session, OpenError> {
        let mut s = Session::open_read_only(scratch, clock, options)?;
        s.viewer.locked = Some(ALREADY_OPEN);
        if let Err(e) = s.open_viewer(file) {
            log::warn!("viewer: {e}");
        }
        Ok(s)
    }

    /// Esc and ×: the viewer closes, and the app quits ([`Viewer::closed`]).
    pub fn close_viewer(&mut self) {
        if self.viewer.file.is_some() {
            self.viewer.closed = Some(Closed::Quit);
        }
    }

    /// Open in Dalica: the listed photo at `path` joins the library if its folder is not there yet
    /// (`viewer.adopt`, one undo step), and the viewer closes into the main window on the edit view
    /// of that photo ([`Viewer::closed`]). Returns its id.
    pub fn open_in_dalica(&mut self, path: &Path) -> Result<PhotoId, CmdError> {
        let reply = self.execute("viewer.adopt", &json!({ "path": path }))?;
        let id: PhotoId =
            serde_json::from_value(reply["id"].clone()).map_err(|e| CmdError::Internal(format!("viewer.adopt replied no photo id: {e}")))?;
        self.viewer.closed = Some(Closed::OpenInDalica(id));
        Ok(id)
    }

    /// Starts reading `file`'s folder on a worker (a read on its way for another file finishes
    /// unheard).
    fn start_listing(&mut self, file: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let options = self.scan_options();
        let key = file.clone();
        let spawned = std::thread::Builder::new().name("dalica-viewer".into()).spawn(move || {
            let _ = tx.send(read_listing(&key, &options));
        });
        match spawned {
            Ok(_) => self.viewer.reading = Some(Reading { file, rx }),
            Err(e) => {
                self.viewer.reading = None;
                self.viewer.failed = Some((file, format!("cannot start reading the folder: {e}")));
            }
        }
    }

    /// Takes in the worker's listing once it has arrived.
    fn take_listing(&mut self) {
        let Some(reading) = &self.viewer.reading else { return };
        match reading.rx.try_recv() {
            Ok(listing) => {
                self.viewer.reading = None;
                self.viewer.listing = Some(listing);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                let file = reading.file.clone();
                self.viewer.reading = None;
                self.viewer.failed = Some((file, "reading the folder stopped unexpectedly".into()));
            }
        }
    }

    /// `viewer.list`: the listing of `file` (the viewer's own without one), or `{pending: true}`
    /// while it is read; `refresh` reads the folder again.
    pub(crate) fn viewer_list(&mut self, file: Option<&Path>, refresh: bool) -> Result<Value, CmdError> {
        let file = match file {
            Some(f) => viewer_path(f)?,
            None => self.viewer.file.clone().ok_or_else(|| CmdError::Disabled(NOT_OPEN.into()))?,
        };
        let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let the_viewers = self.viewer.file.as_deref().and_then(Path::parent) == Some(folder.as_path());
        if !the_viewers && !self.folders.rules.indexes(&folder) {
            return Err(CmdError::Disabled(NOT_THE_VIEWERS.into()));
        }
        self.take_listing();
        if let Some((failed, why)) = self.viewer.failed.take()
            && failed == file
        {
            return Err(CmdError::NotFound(why));
        }
        let reading = self.viewer.reading.as_ref().is_some_and(|r| r.file == file);
        let fresh = self.viewer.listing.as_ref().is_some_and(|l| l.file == file);
        if reading {
            return Ok(json!({ "pending": true, "path": file }));
        }
        if !fresh || refresh {
            self.start_listing(file.clone());
            return Ok(json!({ "pending": true, "path": file }));
        }
        let Some(listing) = &self.viewer.listing else { return Ok(json!({ "pending": true, "path": file })) };
        if let Some(why) = &listing.refused {
            return Err(CmdError::BadParams(why.clone()));
        }
        Ok(self.listing_reply(listing))
    }

    /// The reply of `viewer.list` for `l`, with the library's own data for the photos it holds.
    fn listing_reply(&self, l: &Listing) -> Value {
        let lib = &self.library;
        let folder_id = lib.folder_by_path(&l.folder);
        let name = l.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let photos: Vec<Value> = l
            .photos
            .iter()
            .map(|p| {
                let own = folder_id.and_then(|f| lib.photo_by_name(f, &p.name));
                let mut out = json!({
                    "name": p.name,
                    "path": l.folder.join(&p.name),
                    "id": own.map(|o| o.id),
                    "star": own.map_or(p.star, |o| o.star),
                    "width": own.map_or(p.width, |o| o.width),
                    "height": own.map_or(p.height, |o| o.height),
                });
                match own {
                    Some(o) => crate::video::add_kind(&mut out, lib, o.id),
                    None => crate::video::add_listed_kind(&mut out, p.video.as_ref()),
                }
                out
            })
            .collect();
        json!({
            "file": l.file,
            "folder": l.folder,
            "index": l.photos.iter().position(|p| p.name == name),
            "photos": photos,
            "inLibrary": folder_id.is_some(),
            "readOnly": self.viewer.locked.is_some(),
            "reason": self.viewer.locked,
        })
    }

    /// Why the viewer's changes cannot run now, if they cannot.
    pub(crate) fn viewer_change_disabled(&self) -> Option<String> {
        if let Some(why) = self.viewer.locked {
            return Some(why.to_string());
        }
        None
    }

    /// The library's id of the listed photo at `path`. When the library does not hold its folder,
    /// the folder and its photos as listed join it and the folder is set to Scan Once, inside the
    /// running command, so the adding is part of that command's undo step. `NotFound` for a path
    /// the viewer does not list.
    pub(crate) fn viewer_adopt(&mut self, path: &Path) -> Result<PhotoId, CmdError> {
        let path = viewer_path(path)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        self.take_listing();
        let listed = self.viewer.listing.as_ref().filter(|l| l.folder == folder && l.photos.iter().any(|p| p.name == name));
        let Some(found) = listed.and_then(|l| l.found.clone()) else {
            return Err(CmdError::NotFound(format!("{} {NOT_LISTED}", path.display())));
        };
        let own = |s: &Session| s.library.folder_by_path(&folder).and_then(|f| s.library.photo_by_name(f, &name)).map(|p| p.id);
        if let Some(id) = own(self) {
            return Ok(id);
        }
        if self.folders.rules.indexes(&folder) {
            // A folder the rules index that the library does not hold (yet): its scan brings it.
            return Err(CmdError::Disabled("Dalica is still reading this folder: try again in a moment.".into()));
        }
        // The folder and its photos as listed, then its rule: with the folder in the library, the
        // rule starts no scan (`Session::change_rules`).
        let ops = dalica_scan::diff(&mut self.library, &Scan { folders: vec![found], ..Scan::default() });
        for op in ops {
            self.apply(op)?;
        }
        let old = self.folders.rules.get(&folder).copied().unwrap_or_default();
        let changed = self.change_rules(vec![(folder.clone(), Rule { mode: Some(Mode::Once), faces: old.faces, indexed: false })]);
        if !changed.before.is_empty() {
            self.folders.change = Some(changed.before);
        }
        own(self).ok_or_else(|| CmdError::Internal(format!("{} did not join the library", path.display())))
    }

    /// The viewer's listing forgets `path` (a photo sent to the trash).
    pub(crate) fn viewer_forget(&mut self, path: &Path) {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else { return };
        let name = name.to_string_lossy();
        if let Some(l) = self.viewer.listing.as_mut().filter(|l| l.folder == folder) {
            l.photos.retain(|p| p.name != name);
            if let Some(found) = &mut l.found {
                found.photos.retain(|p| p.name != name);
            }
        }
    }
}

impl Session {
    /// The screen-size picture of the listed photo at `path` (the photo the viewer shows), asked
    /// for at the edit view's priority when it is not there; a screen-size decode of another photo
    /// still waiting is dropped unless it is a neighbour ([`Session::prefetch_viewer_images`]).
    pub fn viewer_image(&mut self, path: &Path) -> Pixels {
        let keep: Vec<PathBuf> = std::iter::once(path.to_path_buf()).chain(self.images().wanted.iter().skip(1).cloned()).collect();
        self.want_screens(keep);
        self.viewer_pixels(path, Kind::Screen, Priority::EDIT)
    }

    /// Decodes the listed photos at `paths` (the next and previous of the photo shown) ahead, at
    /// the prefetch priority.
    pub fn prefetch_viewer_images(&mut self, paths: &[PathBuf]) {
        let current = self.images().wanted.first().cloned();
        self.want_screens(current.into_iter().chain(paths.iter().cloned()).collect());
        for p in paths {
            let _ = self.viewer_pixels(p, Kind::Screen, Priority::PREFETCH);
        }
    }

    /// The filmstrip thumbnail of the listed photo at `path` ([`THUMB_SIDE`] pixels at most).
    pub fn viewer_thumbnail(&mut self, path: &Path) -> Pixels {
        self.viewer_pixels(path, Kind::Thumb, Priority::FILMSTRIP)
    }

    /// Decodes by path queued or running.
    pub fn viewer_images_pending(&self) -> usize {
        self.viewer.images.as_ref().map_or(0, |i| i.asked.len())
    }

    /// Waits up to `timeout` for every decode by path to arrive (tests); whether they did.
    pub fn wait_viewer_images(&mut self, timeout: std::time::Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        loop {
            let images = self.images();
            images.take();
            if images.asked.is_empty() {
                return true;
            }
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            if let Some((key, result)) = images.pool.recv_timeout(left.min(std::time::Duration::from_millis(50))) {
                images.asked.remove(&key);
                images.tick = images.tick.saturating_add(1);
                images.kept.insert(key, (result.map(Arc::new), images.tick));
            }
        }
    }

    fn images(&mut self) -> &mut Images {
        self.viewer.images.get_or_insert_with(Images::new)
    }

    /// The screen-size decodes wanted now are `paths`: the others still queued are dropped.
    fn want_screens(&mut self, paths: Vec<PathBuf>) {
        let images = self.images();
        if images.wanted == paths {
            return;
        }
        let stale: Vec<ImageKey> = images.asked.iter().filter(|(p, k)| *k == Kind::Screen && !paths.contains(p)).cloned().collect();
        for key in stale {
            images.pool.cancel(&key);
            images.asked.remove(&key);
        }
        images.wanted = paths;
    }

    /// The pixels of the listed photo at `path` in size `kind`, asked for at `priority` when
    /// missing.
    fn viewer_pixels(&mut self, path: &Path, kind: Kind, priority: Priority) -> Pixels {
        self.take_listing();
        let listed = path.parent().zip(path.file_name()).and_then(|(folder, name)| {
            let name = name.to_string_lossy();
            self.viewer.listing.as_ref().filter(|l| l.folder == folder).and_then(|l| l.photos.iter().find(|p| p.name == name)).cloned()
        });
        let Some(listed) = listed else {
            return Pixels::Failed(format!("{} {NOT_LISTED}", path.display()));
        };
        // A video is the video tile with its reason, never decoded (its frames follow with M15.15).
        if listed.video.is_some() {
            return Pixels::Failed(crate::video::CANNOT_PLAY.into());
        }
        let (_, render, screen, limits) = self.thumbs.screen_source();
        let images = self.images();
        images.take();
        let key = (path.to_path_buf(), kind);
        images.tick = images.tick.saturating_add(1);
        let tick = images.tick;
        if let Some((result, used)) = images.kept.get_mut(&key) {
            *used = tick;
            return match result {
                Ok(image) => Pixels::Ready(Arc::clone(image)),
                Err(why) => Pixels::Failed(why.clone()),
            };
        }
        if images.asked.contains(&key) {
            return Pixels::Pending;
        }
        let long_side = match kind {
            Kind::Screen => dalica_thumbs::ladder::pixels(dalica_thumbs::pack::Size::Screen, screen),
            Kind::Thumb => THUMB_SIDE,
        };
        let options = render.decode_options();
        let file = key.0.clone();
        images.pool.submit(key.clone(), priority, move |cancel| {
            catch_unwind(AssertUnwindSafe(|| decode(&file, long_side, &options, &limits, &listed.edits, cancel)))
                .unwrap_or_else(|_| Err("the decoder stopped unexpectedly".into()))
        });
        images.asked.insert(key);
        Pixels::Pending
    }
}

/// `path` as the viewer takes it: absolute, without `..`, naming a file in a folder.
fn viewer_path(path: &Path) -> Result<PathBuf, CmdError> {
    if !path.is_absolute() {
        return Err(CmdError::BadParams(format!("`path` must be absolute, not {}", path.display())));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(CmdError::BadParams(format!("`path` must not climb with `..`: {}", path.display())));
    }
    if path.file_name().is_none() || path.parent().is_none() {
        return Err(CmdError::BadParams(format!("`path` names no file: {}", path.display())));
    }
    Ok(path.to_path_buf())
}

/// Reads `file`'s folder as a scan reads it, that folder alone (on the worker thread).
fn read_listing(file: &Path, options: &dalica_scan::Options) -> Listing {
    let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut found: Option<FoundFolder> = None;
    let mut failures: Vec<Failure> = Vec::new();
    // The scan hands out a folder before the folders inside it and reports progress after each
    // folder read: it stops once the first one is done.
    dalica_scan::scan_streaming(std::slice::from_ref(&folder), options, |event| match event {
        Event::Folder(f) => {
            let ours = f.path == folder;
            if ours {
                found = Some(f);
            }
            !ours
        }
        Event::Failed(f) => {
            failures.push(f);
            true
        }
        Event::Progress(p) => p.folders_done == 0,
        Event::Warning(_) => true,
    });
    let mut photos: Vec<Listed> = found
        .iter()
        .flat_map(|f| f.photos.iter())
        .map(|p| Listed { name: p.name.clone(), width: p.width, height: p.height, star: p.star, edits: p.info.edits.clone(), video: p.video.clone() })
        .collect();
    photos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    let refused = if photos.iter().any(|p| p.name == name) {
        None
    } else if let Some(f) = failures.iter().find(|f| f.path == file) {
        Some(format!("Dalica cannot show {name}: {}", f.reason))
    } else {
        Some(match std::fs::symlink_metadata(file) {
            Err(_) => format!("Dalica cannot show {name}: there is no such file"),
            Ok(m) if m.is_dir() => format!("Dalica cannot show {name}: it is a folder"),
            Ok(_) => format!("Dalica cannot show {name}: it is not a picture Dalica reads, or its type is off in Options ▸ File Types"),
        })
    };
    Listing { file: file.to_path_buf(), folder, found, photos, refused }
}
