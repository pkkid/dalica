//! The Import room's session (`organizing.md` §3 items 1–3): the source being listed, the
//! listing's generation (a new source drops the older listing and its results) and its candidates
//! with the room's choices. It lives in the engine's session, so the room reopens on the last
//! source while it is still there, and the CLI and MCP see the same listing.
//!
//! - **Sources** come from `dalica-scan`'s cached device list (cards and drives, refreshed off the
//!   calling thread at most every 2 s) and the recent sources in the settings: asking never waits
//!   on the disk. A camera's storage is a source too (`camera.open`, `crate::camera`): its
//!   candidates name objects on the camera, not files, and its listing reads them over PTP.
//! - **The listing** runs on a worker thread of its own ([`worker`]): `dalica-scan`'s bounded
//!   listing of an untrusted tree, each picture checked for duplicates as it comes. What it finds
//!   lands in a shared state the owner reads under a short lock when a command asks, so the owner
//!   thread never touches the disk. It is not a job of the activity list (the room shows its own
//!   status line, "Loading 112 of 340 files…"); dropping the listing (another source, Close, the
//!   session closing) stops the worker after its current file.
//! - **Library digests** the worker had to make are kept in the index (derived data, as Find
//!   Duplicates keeps them) the next time an `import.*` command runs.
//! - **Choices** (excluded, star, rotation, "Exclude duplicates") are the room's, held here and
//!   never an undo step; the import itself (M11.8) reads them.
//! - **Thumbnails** stay in memory only ([`thumbs`]): capped, the least recently used dropped and
//!   made again when asked for.

mod thumbs;
mod worker;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dalica_core::{DateTime, PhotoId};
use dalica_image::Rgba8;
use dalica_library::{ContentDigest, Op};
use dalica_scan::devices::{Device, DeviceKind, DeviceWatch};
use dalica_scan::import::{Group, ListOptions, TimeSource};
use serde_json::{Value, json};

use crate::camera::{CameraFile, CameraObject, Link};
use crate::error::CmdError;
use crate::session::Session;

pub use thumbs::{CAP_BYTES, MAX_REMAKING};
pub use worker::{DuplicateOf, ListState, MAX_WARNINGS};
use worker::{Found, LibraryFile, Plan, Shared, lock};

/// The most candidates one `import.list` answers.
pub const MAX_PAGE: usize = 1000;

/// The most ids one command takes (a listing's most files).
pub const MAX_IDS: usize = dalica_scan::import::MAX_FILES;

/// Why the room's choices are greyed out with nothing open.
pub const NOTHING_OPEN: &str = "No import source is open.";

/// Digests kept per `SetDigests` op.
const DIGEST_BATCH: usize = 512;

/// What kind of source is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A camera's card (its `DCIM` folder).
    Card,
    /// Another removable drive (its root).
    Drive,
    /// A folder chosen with Folder… or a recent source.
    Folder,
    /// A camera or phone over PTP/MTP (one storage of it, or all), read through its session.
    Camera,
}

impl SourceKind {
    pub fn id(self) -> &'static str {
        match self {
            SourceKind::Card => "card",
            SourceKind::Drive => "drive",
            SourceKind::Folder => "folder",
            SourceKind::Camera => "camera",
        }
    }
}

/// The source being listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub kind: SourceKind,
    /// What the room's "Import from:" shows.
    pub name: String,
    /// The folder listed: a card's `DCIM` folder, a drive's root or the chosen folder; for a
    /// camera, `camera:<id>:<storage>` (no folder on disk: [`crate::camera::source_path`]).
    pub path: PathBuf,
}

impl Source {
    fn json(&self) -> Value {
        json!({ "kind": self.kind.id(), "name": self.name, "path": self.path })
    }
}

/// The room's choices for the candidates.
#[derive(Clone, Debug, Default)]
struct Choices {
    /// Exclude All (on) or Include All (off): what a candidate without its own choice gets.
    all_excluded: bool,
    excluded: HashMap<u64, bool>,
    starred: HashMap<u64, bool>,
    /// Quarter turns clockwise, 0–3.
    turns: HashMap<u64, u8>,
}

impl Choices {
    fn excluded(&self, id: u64) -> bool {
        self.excluded.get(&id).copied().unwrap_or(self.all_excluded)
    }
}

/// The listing running or finished: its worker's state and thumbnails.
struct Listing {
    shared: Arc<Mutex<Shared>>,
    thumbs: Arc<Mutex<thumbs::ThumbCache>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Listing {
    fn drop(&mut self) {
        // The worker stops before its next file; what it finds from now on goes nowhere.
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The capture order and groups of the candidates, made again when more arrive.
#[derive(Debug, Default)]
struct View {
    /// How many candidates it was made from.
    count: usize,
    /// Candidate ids in capture order (then listing order).
    order: Vec<u64>,
    /// Each candidate's group, by id.
    group_of: Vec<usize>,
    groups: Vec<Group>,
}

impl View {
    fn of(times: &[DateTime]) -> View {
        let groups = dalica_scan::import::groups(times);
        let mut group_of = vec![0; times.len()];
        let mut order = Vec::with_capacity(times.len());
        for (n, g) in groups.iter().enumerate() {
            for m in &g.members {
                if let Some(slot) = group_of.get_mut(*m) {
                    *slot = n;
                }
                order.push(*m as u64);
            }
        }
        View { count: times.len(), order, group_of, groups }
    }
}

/// The import session: nothing open until a source is.
#[derive(Default)]
pub struct ImportSession {
    source: Option<Source>,
    generation: u64,
    listing: Option<Listing>,
    choices: Choices,
    exclude_duplicates: bool,
    view: View,
    /// The device list asked (`None`: `dalica-scan`'s own, of this system).
    devices: Option<Arc<DeviceWatch>>,
    /// The camera being listed, when the source is one.
    camera: Option<Arc<Link>>,
}

impl std::fmt::Debug for ImportSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportSession").field("source", &self.source).field("generation", &self.generation).finish_non_exhaustive()
    }
}

/// One candidate as the room shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Its id in this listing.
    pub id: u64,
    /// The file on the source, exactly as the source names it.
    pub path: PathBuf,
    /// Its folder below the source.
    pub relative: PathBuf,
    /// The name to show.
    pub name: String,
    /// The bytes read.
    pub size: u64,
    pub captured: DateTime,
    pub time_source: TimeSource,
    /// `md5:` and 32 lowercase hex digits.
    pub digest: String,
    /// Its group, counted in capture order from 0.
    pub group: usize,
    pub duplicate: Option<DuplicateOf>,
    /// The room's Exclude.
    pub excluded: bool,
    pub starred: bool,
    /// Quarter turns clockwise chosen in the room (0–3).
    pub turns: u8,
    /// The object on the camera, when the source is one.
    pub camera: Option<CameraObject>,
}

impl ImportSession {
    /// The source being listed.
    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    /// The listing's generation: it rises each time a source is opened.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// "Exclude duplicates" as the room has it.
    pub fn exclude_duplicates(&self) -> bool {
        self.exclude_duplicates
    }

    /// The candidates listed so far, in capture order.
    pub fn candidates(&self) -> Vec<Candidate> {
        let Some(listing) = &self.listing else { return Vec::new() };
        let shared = lock(&listing.shared);
        let times: Vec<DateTime> = shared.found.iter().map(|f| f.candidate.captured).collect();
        let view = View::of(&times);
        view.order
            .iter()
            .filter_map(|id| {
                let i = usize::try_from(*id).ok()?;
                Some(self.candidate(shared.found.get(i)?, view.group_of.get(i).copied().unwrap_or(0)))
            })
            .collect()
    }

    /// Whether candidate `c` will be imported: neither excluded nor a duplicate left out by
    /// "Exclude duplicates".
    pub fn will_import(&self, c: &Candidate) -> bool {
        !c.excluded && !(self.exclude_duplicates && c.duplicate.is_some())
    }

    /// Candidate `id`'s thumbnail when it is in memory; one that was dropped is made again on a
    /// worker (ask again later).
    pub fn thumbnail(&self, id: u64) -> Option<Arc<Rgba8>> {
        let listing = self.listing.as_ref()?;
        if let Some(t) = lock(&listing.thumbs).get(id) {
            return Some(t);
        }
        // A camera's thumbnail is made by its listing only (its candidates name no file).
        if self.camera.is_some() {
            return None;
        }
        let path = lock(&listing.shared).found.get(usize::try_from(id).ok()?)?.candidate.path.clone();
        thumbs::remake(&listing.thumbs, id, path, dalica_scan::import::THUMBNAIL);
        None
    }

    fn candidate(&self, f: &Found, group: usize) -> Candidate {
        let c = &f.candidate;
        let id = c.index as u64;
        Candidate {
            id,
            path: c.path.clone(),
            relative: c.relative.clone(),
            name: c.name.clone(),
            size: c.size,
            captured: c.captured,
            time_source: c.time_source,
            digest: c.digest.clone(),
            group,
            duplicate: f.duplicate,
            excluded: self.choices.excluded(id),
            starred: self.choices.starred.get(&id).copied().unwrap_or(false),
            turns: self.choices.turns.get(&id).copied().unwrap_or(0),
            camera: f.camera.clone(),
        }
    }

    /// The camera object of candidate `c` with the session that reaches it, when the source is a
    /// camera.
    pub(crate) fn camera_file(&self, c: &Candidate) -> Option<CameraFile> {
        Some(CameraFile { link: Arc::clone(self.camera.as_ref()?), object: c.camera.clone()? })
    }

    /// The cards and drives from the cached list (never waits on the disk).
    fn devices(&self) -> Vec<Device> {
        match &self.devices {
            Some(w) => w.get(),
            None => dalica_scan::devices::devices(),
        }
    }

    /// Brings the capture order and groups up to date with the candidates found.
    fn refresh_view(&mut self) {
        let Some(listing) = &self.listing else {
            self.view = View::default();
            return;
        };
        let shared = lock(&listing.shared);
        if shared.found.len() == self.view.count {
            return;
        }
        let times: Vec<DateTime> = shared.found.iter().map(|f| f.candidate.captured).collect();
        drop(shared);
        self.view = View::of(&times);
    }
}

/// "1,204": `n` with its thousands grouped.
fn count(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The room's status line: "Loading 112 of 340 files…", "340 files", "No pictures found".
fn status(s: &Shared) -> String {
    let listed = s.found.len() as u64;
    match s.state {
        ListState::Loading if !s.walked => "Loading…".into(),
        ListState::Loading => format!("Loading {} of {} files…", count(s.progress.done as u64), count(s.progress.files as u64)),
        ListState::Failed => "The source could not be read to the end".into(),
        _ if !s.walked => match s.warnings.first() {
            Some(w) => format!("The source cannot be read: {}", w.reason),
            None => "No pictures found".into(),
        },
        _ if listed == 0 => "No pictures found".into(),
        _ if listed == 1 => "1 file".into(),
        _ => format!("{} files", count(listed)),
    }
}

/// A candidate as `import.list` answers it.
fn candidate_json(c: &Candidate, will_import: bool, thumbnail: bool) -> Value {
    let duplicate = match c.duplicate {
        None => Value::Null,
        Some(DuplicateOf::Photo(id)) => json!({ "photo": id }),
        Some(DuplicateOf::Candidate(id)) => json!({ "candidate": id }),
    };
    json!({
        "id": c.id,
        "name": c.name,
        "path": c.path,
        "folder": c.relative,
        "size": c.size,
        "captured": c.captured.to_string(),
        "timeSource": match c.time_source { TimeSource::Metadata => "metadata", TimeSource::File => "file" },
        "group": c.group,
        "duplicate": duplicate,
        "excluded": c.excluded,
        "willImport": will_import,
        "starred": c.starred,
        "rotation": u32::from(c.turns) * 90,
        "thumbnail": thumbnail,
    })
}

/// A folder's name to show: its last part, else the whole path.
fn folder_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl Session {
    /// Test hook: the device list `import.sources` and `import.openSource` ask is the one inside the mount
    /// folders `value` names (paths joined like `PATH`, read as `DALICA_DEVICE_ROOTS` is),
    /// refreshed when older than `max_age`; `None` gives back this system's own list.
    #[doc(hidden)]
    pub fn set_import_device_roots(&mut self, value: Option<OsString>, max_age: Duration) {
        self.import.devices = value.map(|v| {
            let parents = dalica_scan::devices::device_roots(Some(v)).unwrap_or_default();
            DeviceWatch::new(move || dalica_scan::devices::devices_in(&parents, &[]), max_age)
        });
    }

    /// Keeps the library digests the listing worker made in the index (derived data). Never
    /// touches the disk.
    fn keep_import_digests(&mut self) {
        let Some(listing) = &self.import.listing else { return };
        let mut made = std::mem::take(&mut lock(&listing.shared).digests);
        made.retain(|(id, d)| self.library.photo(*id).is_some() && self.library.digest_record(*id) != Some(d));
        let digests: Vec<(PhotoId, Option<ContentDigest>)> = made.into_iter().map(|(id, d)| (id, Some(d))).collect();
        for batch in digests.chunks(DIGEST_BATCH) {
            if let Err(e) = self.apply_derived(Op::SetDigests { digests: batch.to_vec() }) {
                log::error!("import digests not kept: {e}");
            }
        }
    }

    /// `import.sources`: the cards, drives and recent sources (a recent source that is a card's or
    /// drive's folder now is listed once, as the device).
    pub(crate) fn import_sources(&mut self) -> Value {
        self.keep_import_digests();
        let devices = self.import.devices();
        let mut sources: Vec<Value> = devices
            .iter()
            .map(|d| {
                let kind = match d.kind {
                    DeviceKind::Card => "card",
                    DeviceKind::Drive => "drive",
                };
                json!({ "kind": kind, "name": d.name, "path": d.folder, "root": d.root })
            })
            .collect();
        for path in self.settings().import().recent_sources {
            if !devices.iter().any(|d| d.folder == path) {
                sources.push(json!({ "kind": "recent", "name": folder_name(&path), "path": path }));
            }
        }
        json!({ "sources": sources })
    }

    /// `import.openSource`: lists `device` (a card's or drive's folder, or its root, from the cached list)
    /// or the folder `path`, dropping the listing before it. Never touches the disk: the worker
    /// reports a folder that cannot be read in the status line.
    pub(crate) fn import_open(&mut self, device: Option<&Path>, path: Option<&Path>) -> Result<Value, CmdError> {
        self.keep_import_digests();
        let source = match (device, path) {
            (Some(d), None) => {
                let Some(x) = self.import.devices().into_iter().find(|x| x.folder == d || x.root == d) else {
                    return Err(CmdError::NotFound(format!("no card or drive at {} (import.sources lists them)", d.display())));
                };
                let kind = if x.kind == DeviceKind::Card { SourceKind::Card } else { SourceKind::Drive };
                Source { kind, name: x.name, path: x.folder }
            }
            (None, Some(p)) => {
                crate::automation::lexical(p, "path").map_err(|r| CmdError::BadParams(r.message))?;
                Source { kind: SourceKind::Folder, name: folder_name(p), path: p.to_path_buf() }
            }
            _ => return Err(CmdError::BadParams("give either `device` (a card or drive from import.sources) or `path` (a folder)".into())),
        };
        let source_root = source.path.clone();
        let types = self.settings().file_types();
        self.import_start_listing(source, None, move |plan, cancel, shared, thumbs| {
            let root = source_root;
            let options = ListOptions { types, cancel: Some(cancel), ..ListOptions::default() };
            worker::run(root, options, plan, shared, thumbs);
        })
    }

    /// `camera.open`: lists the camera `id` (its storage `storage`, else every storage) in the
    /// Import room, dropping the listing before it. Never talks to the camera on the owner thread:
    /// the listing's worker opens its session.
    pub(crate) fn import_open_camera(&mut self, id: &str, storage: Option<u32>) -> Result<Value, CmdError> {
        self.keep_import_digests();
        let (camera, link) = self.camera_link(id).map_err(CmdError::Disabled)?;
        let name = match storage {
            Some(st) => match camera.storages.iter().find(|s| s.id == st) {
                Some(s) if camera.storages.len() > 1 => format!("{} ({})", camera.name(), s.name),
                Some(_) => camera.name(),
                None => return Err(CmdError::NotFound(format!("the camera has no storage {st} (camera.list lists them)"))),
            },
            None => camera.name(),
        };
        let path = crate::camera::source_path(id, storage);
        let source = Source { kind: SourceKind::Camera, name, path: path.clone() };
        let types = self.settings().file_types();
        let worker_link = Arc::clone(&link);
        self.import_start_listing(source, Some(link), move |plan, cancel, shared, thumbs| {
            let src = worker::CameraSource { link: worker_link, storage, types, root: path, cancel };
            worker::run_camera(src, plan, shared, thumbs);
        })
    }

    /// Drops the listing before it and starts `run` on a worker for `source`.
    fn import_start_listing(
        &mut self,
        source: Source,
        camera: Option<Arc<Link>>,
        run: impl FnOnce(Plan, Arc<AtomicBool>, Arc<Mutex<Shared>>, Arc<Mutex<thumbs::ThumbCache>>) + Send + 'static,
    ) -> Result<Value, CmdError> {
        // The listing before goes first: its worker stops after its current file.
        self.import.listing = None;
        self.import.camera = None;
        let cancel = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let cache = Arc::new(Mutex::new(thumbs::ThumbCache::default()));
        let plan = self.import_plan();
        let (worker_cancel, worker_shared, worker_thumbs) = (Arc::clone(&cancel), Arc::clone(&shared), Arc::clone(&cache));
        let spawned =
            std::thread::Builder::new().name("dalica-import-list".into()).spawn(move || run(plan, worker_cancel, worker_shared, worker_thumbs));
        if let Err(e) = spawned {
            return Err(CmdError::Internal(format!("cannot start the import listing: {e}")));
        }
        let detect = self.settings().import().detect_duplicates;
        let imp = &mut self.import;
        imp.generation = imp.generation.saturating_add(1);
        imp.listing = Some(Listing { shared, thumbs: cache, cancel });
        imp.camera = camera;
        imp.choices = Choices::default();
        imp.view = View::default();
        imp.source = Some(source.clone());
        imp.exclude_duplicates = detect;
        Ok(json!({ "generation": imp.generation, "source": source.json(), "excludeDuplicates": detect }))
    }

    /// The library's photos by recorded size, each with its digest when the index holds one that
    /// still fits the file. From the library alone.
    fn import_plan(&self) -> Plan {
        let lib = &self.library;
        let mut plan: Plan = HashMap::new();
        for p in lib.photos() {
            let Some(folder) = lib.folder(p.folder) else { continue };
            let digest = lib.content_digest(p.id).map(str::to_string);
            plan.entry(p.size).or_default().push(LibraryFile { id: p.id, path: folder.path.join(&p.name), digest, tried: false });
        }
        plan
    }

    /// `import.list`: the status and a page of the candidates in capture order.
    pub(crate) fn import_list(&mut self, offset: usize, limit: usize) -> Value {
        self.keep_import_digests();
        self.import.refresh_view();
        let imp = &self.import;
        let Some(listing) = &imp.listing else {
            return json!({
                "generation": imp.generation, "source": null, "state": null, "status": "", "total": 0, "duplicates": 0,
                "excluded": 0, "toImport": 0, "excludeDuplicates": imp.exclude_duplicates, "offset": offset, "groups": [], "candidates": [],
            });
        };
        let shared = lock(&listing.shared);
        let cache = lock(&listing.thumbs);
        let (mut duplicates, mut excluded, mut to_import) = (0u64, 0u64, 0u64);
        for f in &shared.found {
            let c = imp.candidate(f, 0);
            duplicates += u64::from(c.duplicate.is_some());
            excluded += u64::from(c.excluded);
            to_import += u64::from(imp.will_import(&c));
        }
        let page: Vec<Value> = imp
            .view
            .order
            .iter()
            .skip(offset)
            .take(limit)
            .filter_map(|id| {
                let i = usize::try_from(*id).ok()?;
                let c = imp.candidate(shared.found.get(i)?, imp.view.group_of.get(i).copied().unwrap_or(0));
                Some(candidate_json(&c, imp.will_import(&c), cache.has(*id)))
            })
            .collect();
        let groups: Vec<Value> =
            imp.view.groups.iter().map(|g| json!({ "start": g.start.to_string(), "end": g.end.to_string(), "count": g.members.len() })).collect();
        let warnings: Vec<Value> = shared.warnings.iter().map(|w| json!({ "path": w.path, "reason": w.reason })).collect();
        json!({
            "generation": imp.generation,
            "source": imp.source.as_ref().map(Source::json),
            "state": shared.state.id(),
            "status": status(&shared),
            "progress": { "files": shared.progress.files, "done": shared.progress.done, "batches": shared.batches },
            "total": imp.view.count,
            "duplicates": duplicates,
            "excluded": excluded,
            "toImport": to_import,
            "excludeDuplicates": imp.exclude_duplicates,
            "truncated": shared.truncated,
            "warnings": warnings,
            "warningCount": shared.warning_count,
            "offset": offset,
            "groups": groups,
            "candidates": page,
        })
    }

    /// Fails with `NotFound` unless every id is a candidate listed so far.
    fn import_check_ids(&self, ids: &[u64]) -> Result<(), CmdError> {
        let listed = self.import.listing.as_ref().map_or(0, |l| lock(&l.shared).found.len());
        match ids.iter().find(|id| usize::try_from(**id).map_or(true, |i| i >= listed)) {
            Some(id) => Err(CmdError::NotFound(format!("no candidate {id} in this listing"))),
            None => Ok(()),
        }
    }

    /// `import.exclude`: Exclude (`on`) or Include the candidates `ids`.
    pub(crate) fn import_exclude(&mut self, ids: &[u64], on: bool) -> Result<Value, CmdError> {
        self.keep_import_digests();
        self.import_check_ids(ids)?;
        for id in ids {
            self.import.choices.excluded.insert(*id, on);
        }
        Ok(self.import_counts())
    }

    /// `import.excludeAll`: Exclude All (`on`) or Include All, those listed later too.
    pub(crate) fn import_exclude_all(&mut self, on: bool) -> Value {
        self.keep_import_digests();
        self.import.choices.all_excluded = on;
        self.import.choices.excluded.clear();
        self.import_counts()
    }

    /// `import.mark`: the candidates' star (set or cleared) and rotation (`turns` quarter turns
    /// clockwise added, −3 … 3).
    pub(crate) fn import_mark(&mut self, ids: &[u64], star: Option<bool>, turns: Option<i8>) -> Result<Value, CmdError> {
        self.keep_import_digests();
        self.import_check_ids(ids)?;
        let choices = &mut self.import.choices;
        for id in ids {
            if let Some(on) = star {
                choices.starred.insert(*id, on);
            }
            if let Some(t) = turns {
                let now = i16::from(choices.turns.get(id).copied().unwrap_or(0));
                let next = u8::try_from((now + i16::from(t)).rem_euclid(4)).unwrap_or(0);
                choices.turns.insert(*id, next);
            }
        }
        Ok(self.import_counts())
    }

    /// `import.options`: "Exclude duplicates".
    pub(crate) fn import_options(&mut self, exclude_duplicates: bool) -> Value {
        self.keep_import_digests();
        self.import.exclude_duplicates = exclude_duplicates;
        self.import_counts()
    }

    /// `import.close`: drops the listing (its worker stops after its current file).
    pub(crate) fn import_close(&mut self) -> Value {
        self.keep_import_digests();
        let was_open = self.import.source.is_some();
        let generation = self.import.generation;
        let devices = self.import.devices.take();
        self.import = ImportSession { generation, devices, ..ImportSession::default() };
        json!({ "closed": was_open, "generation": generation })
    }

    /// What the room's choices leave: the candidates to import, excluded and duplicates.
    fn import_counts(&self) -> Value {
        let candidates = self.import.candidates();
        let to_import = candidates.iter().filter(|c| self.import.will_import(c)).count();
        let excluded = candidates.iter().filter(|c| c.excluded).count();
        let duplicates = candidates.iter().filter(|c| c.duplicate.is_some()).count();
        json!({
            "total": candidates.len(), "toImport": to_import, "excluded": excluded, "duplicates": duplicates,
            "excludeDuplicates": self.import.exclude_duplicates,
        })
    }

    /// Whether a source is open (the room's choices are enabled).
    pub(crate) fn import_is_open(&self) -> bool {
        self.import.source.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_group_their_thousands() {
        assert_eq!(count(0), "0");
        assert_eq!(count(340), "340");
        assert_eq!(count(1204), "1,204");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn the_status_line_follows_the_listing() {
        let mut s = Shared::default();
        assert_eq!(status(&s), "Loading…");
        s.walked = true;
        s.progress.files = 340;
        s.progress.done = 112;
        assert_eq!(status(&s), "Loading 112 of 340 files…");
        s.state = ListState::Done;
        assert_eq!(status(&s), "No pictures found");
        s.walked = false;
        s.warnings.push(dalica_scan::Warning { path: PathBuf::from("/card"), reason: "gone".into() });
        assert_eq!(status(&s), "The source cannot be read: gone");
    }
}
