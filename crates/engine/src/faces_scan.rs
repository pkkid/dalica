//! The face scan (`faces.md` §4, `roadmap.md` M13 → *The scan*): while Find faces in my photos is
//! on and a recogniser is installed, a `faces` job at the lowest priority follows the library's new
//! and changed photos, its pace following the user's input ([`Session::user_activity`]).
//!
//! - **The models** come from the models folder the host gives the session
//!   ([`Session::set_face_models_dir`]; the app, the CLI and MCP pass [`default_models_dir`]:
//!   `$DALICA_FACE_MODELS`, else `<config>/models`). Without one (tests, benches, a session nobody
//!   gave a folder) nothing is scanned. An installed model is a folder named by its id holding
//!   `model.onnx` and its manifest `face-model.json` (LightCraft's layout; [`installed_models`]).
//!   The recogniser in use is `people.model` when that one is installed, else the newest; the
//!   newest installed detector finds faces. Both are loaded, and the recogniser's embeddings cache
//!   opened, on a worker; a file whose hash differs from its manifest's is refused. Without a
//!   recogniser the folder is looked at again every [`RECHECK`] (and at once after
//!   [`Session::face_models_changed`]).
//! - **Per photo** (on the scan's own workers, [`process`]): the upright, unedited photo decoded at
//!   most [`EDGE`] pixels on its long side (a RAW through its embedded preview); a photo with no
//!   faces that the detector has not searched is searched once ([`dalica_faces::detect`]); every
//!   face without an embedding is aligned by the landmarks of the detection it overlaps (else cut
//!   by its box) and embedded. The worker appends the embeddings, landmarks and "searched" to the
//!   model's cache (`dalica-store`'s `faces_cache`) itself; the owner takes the results into the
//!   index (`faces_index.rs`) and writes the detected rectangles as Unnamed faces, one op per poll
//!   without an undo step (as a scan's imports are). A detection overlapping a face the photo has
//!   (named, unnamed or ignored) is that face, never a second one; a face the user removed is not
//!   found again because the photo stays searched.
//! - **Which photos, in which order** ([`Session::poll_face_scan`]): photos with named faces first
//!   (they seed suggestions), then the newest folders first, newest photos first. Videos, folders
//!   with "Find faces in this folder" off, hidden folders and folders on offline volumes are
//!   skipped: their files are never read. Find Faces Again (`faces.reset`) and `faces.detect` put
//!   their photos at the front and search them again.
//! - **Pace** ([`Pace`]): while the user drags, types or scrolls no new photo is started; while the
//!   pointer moves, one photo at a time; idle for three seconds, about half the cores; the window
//!   away (minimised, or idle on the People view), most of them. Photos in progress are never
//!   interrupted. The workers are also bounded by memory ([`MAX_WORKERS`]).
//! - **The job** (`JobKind::Faces`, cancellable) runs while photos wait: "Finding faces… 312 of
//!   4,980", the People group's header. Cancelling it stops the scan until it is asked for again
//!   (`faces.enable`, `faces.detect`, `faces.reset`).
//! - **Groups** of the unnamed faces are made on a worker whenever the unnamed faces, their
//!   embeddings or the Grouping confidence change (`faces_index.rs`).
//!
//! The owner thread never reads a file: the models folder, the models, the cache and the photos
//! are read on the scan's workers. Logs say counts and photo ids, never a name, a rectangle or an
//! embedding (`faces.md` §7).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use dalica_core::{FolderId, PhotoId, Rect01, Timestamp};
use dalica_faces::align::Rgb;
use dalica_faces::detect::Detector;
use dalica_faces::embed::Embedder;
use dalica_faces::manifest::{self, ModelManifest, Role};
use dalica_faces::matching::{Embedding, Packed};
use dalica_faces::runtime::Model;
use dalica_library::{Face, FacePerson, MAX_FACES, Op, Photo};
use dalica_scan::Rules;
use dalica_store::faces_cache::{self, CachedFace, FacesCache, PhotoStamp, Record};
use dalica_thumbs::{Pool, Priority};
use serde_json::{Value, json};

use crate::faces_index::{FaceAt, ModelInfo};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;

/// The long side of the picture faces are found in and cut from (`faces.md` §2).
pub const EDGE: u32 = 2048;
/// A detection lends its landmarks to a face it overlaps at least this much (intersection over
/// union).
pub const MATCH_IOU: f64 = 0.3;
/// A detection overlapping a face of the photo this much is that face: it is not added again.
pub const SAME_FACE_IOU: f64 = 0.3;
/// While the window reported dragging, typing or scrolling this recently, no photo is started.
pub const INPUT_HOLD: Duration = Duration::from_secs(1);
/// After this long without input or pointer movement the user is idle (`faces.md` §4).
pub const IDLE_AFTER: Duration = Duration::from_secs(3);
/// What one photo in progress may hold: its decoded picture (2048² RGBA), the copies the detector
/// and the recogniser read, and their scratch.
pub const WORKER_BYTES: u64 = 192 << 20;
/// The scan's share of the memory budget (half of its top, `architecture.md` §5).
pub const MEMORY_SHARE: u64 = 768 << 20;
/// The most photos in progress at once, by memory.
pub const MAX_WORKERS: usize = (MEMORY_SHARE / WORKER_BYTES) as usize;
/// Without a recogniser, the models folder is looked at again after this long.
pub const RECHECK: Duration = Duration::from_secs(10);
/// The waiting list is made again at most this often while it still has photos (a full pass over
/// the library), and at once when it ran dry.
pub const REQUEUE: Duration = Duration::from_secs(1);
/// While photos are being scanned, the groups are made again at most this often (and at once when
/// the scan is done).
pub const REGROUP: Duration = Duration::from_secs(2);
/// The environment variable naming the models folder (`faces.md` §3.1).
pub const MODELS_ENV: &str = "DALICA_FACE_MODELS";
/// An installed model's file, in its folder.
pub const MODEL_FILE: &str = "model.onnx";
/// An installed model's manifest, in its folder.
pub const MANIFEST_FILE: &str = "face-model.json";
/// The most faces one detection adds to a photo.
const MAX_NEW: usize = MAX_FACES;
/// Between two looks at the results while waiting.
const WAIT_STEP: Duration = Duration::from_millis(2);
/// The scan's photos in its own pool: the lowest priority there is.
const PHOTOS: Priority = Priority(0);
/// Loading the models, grouping and deleting the cached data go before the photos waiting.
const TASKS: Priority = Priority(1);

/// What the window saw of the user since its last report (`faces.md` §4, *Pace follows the
/// user*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserActivity {
    /// Dragging, typing or scrolling: no new photo is started.
    Input,
    /// The pointer moving: one photo at a time.
    Pointer,
    /// Nothing: after three seconds of it, about half the cores.
    Idle,
    /// The window minimised, or idle on the People view: more.
    Away,
}

/// When the window last reported each kind of activity, on the session clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActivitySeen {
    /// The last report of [`UserActivity::Input`].
    pub input: Option<Timestamp>,
    /// The last report of [`UserActivity::Pointer`].
    pub pointer: Option<Timestamp>,
    /// The latest report, whatever it was, and when.
    pub last: Option<(UserActivity, Timestamp)>,
}

/// How hard the scan may work right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pace {
    /// Start nothing new: the user is dragging, typing or scrolling.
    Pause,
    /// One photo at a time: the pointer moves.
    Light,
    /// About half the cores: the user is idle.
    Normal,
    /// Most of the cores: the window is away.
    Full,
}

impl Pace {
    /// The pace for what the window reported, at `now`.
    pub fn of(seen: &ActivitySeen, now: Timestamp) -> Pace {
        let within = |t: Option<Timestamp>, d: Duration| t.is_some_and(|t| now.since(t) < d);
        if within(seen.input, INPUT_HOLD) {
            return Pace::Pause;
        }
        if matches!(seen.last, Some((UserActivity::Away, _))) {
            return Pace::Full;
        }
        if within(seen.input, IDLE_AFTER) || within(seen.pointer, IDLE_AFTER) {
            return Pace::Light;
        }
        Pace::Normal
    }

    /// Photos in progress at once at this pace on `cores` threads, never more than
    /// [`MAX_WORKERS`].
    pub fn workers(self, cores: usize) -> usize {
        let n = match self {
            Pace::Pause => return 0,
            Pace::Light => 1,
            Pace::Normal => cores / 2,
            Pace::Full => cores * 4 / 5,
        };
        n.clamp(1, MAX_WORKERS)
    }

    fn name(self) -> &'static str {
        match self {
            Pace::Pause => "pause",
            Pace::Light => "light",
            Pace::Normal => "normal",
            Pace::Full => "full",
        }
    }
}

fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The models folder for this machine (`faces.md` §3.1): `$DALICA_FACE_MODELS`, else
/// `<config>/models` (Linux `$XDG_CONFIG_HOME/dalica` or `~/.config/dalica`, macOS
/// `~/Library/Application Support/Dalica`, Windows `%APPDATA%\Dalica`). `None` without a home.
pub fn default_models_dir() -> Option<PathBuf> {
    models_dir_from(|k| std::env::var_os(k))
}

fn models_dir_from(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).map(PathBuf::from).filter(|p| p.is_absolute());
    if let Some(dir) = abs(MODELS_ENV) {
        return Some(dir);
    }
    let config = if cfg!(windows) {
        abs("APPDATA").map(|p| p.join("Dalica"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Application Support").join("Dalica"))
    } else {
        abs("XDG_CONFIG_HOME").map(|p| p.join("dalica")).or_else(|| abs("HOME").map(|h| h.join(".config").join("dalica")))
    };
    config.map(|c| c.join("models"))
}

/// One installed model: its manifest and its folder.
#[derive(Clone, Debug)]
pub struct Installed {
    pub manifest: ModelManifest,
    pub dir: PathBuf,
    /// When its model file was last written (the newest recogniser is the one in use).
    pub modified: Option<SystemTime>,
}

/// The models installed in `dir`: each a folder named by a valid model id holding [`MODEL_FILE`]
/// and a manifest ([`MANIFEST_FILE`]) that validates and names the same id. Anything else is
/// passed over, never trusted. Sorted by id. Reads the disk: call it on a worker.
pub fn installed_models(dir: &Path) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten().take(1024) {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        if !manifest::valid_id(&name) || !p.is_dir() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(p.join(MODEL_FILE)) else { continue };
        if !meta.is_file() {
            continue;
        }
        let limit = u64::try_from(manifest::MAX_MANIFEST_BYTES).unwrap_or(u64::MAX);
        let Some(m) = dalica_fs::read(&p.join(MANIFEST_FILE), limit).ok().flatten().and_then(|r| manifest::parse(&r.bytes).ok()) else {
            continue;
        };
        if m.id != name {
            continue;
        }
        out.push(Installed { manifest: m, dir: p, modified: meta.modified().ok() });
    }
    out.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    out
}

/// The newest of `models` with `role` (ties by id), or the one named `selected` when it is there.
fn pick<'a>(models: &'a [Installed], role: Role, selected: Option<&str>) -> Option<&'a Installed> {
    let of_role = || models.iter().filter(move |m| m.manifest.role == role);
    if let Some(id) = selected
        && let Some(m) = of_role().find(|m| m.manifest.id == id)
    {
        return Some(m);
    }
    of_role().max_by(|a, b| a.modified.cmp(&b.modified).then_with(|| b.manifest.id.cmp(&a.manifest.id)))
}

/// What the scan works with: the recogniser, its cache and the detector, loaded on a worker.
struct Engine {
    info: ModelInfo,
    embedder: Embedder,
    detector: Option<Detector>,
    cache: Mutex<FacesCache>,
}

impl Engine {
    fn tag(&self) -> String {
        format!("{}@{}", self.info.id, self.info.sha256)
    }
}

/// One photo as the cache held it: its stamp, whether it was searched, its faces packed.
type CachedEntry = (PhotoStamp, bool, Vec<(Rect01, Packed)>);

/// What a load brought: the engine and what its cache held, packed for the index.
struct Loaded {
    engine: Arc<Engine>,
    cached: Vec<CachedEntry>,
}

/// The file's SHA-256, checked against the manifest's when it gives one.
fn checked_hash(file: &Path, m: &ModelManifest) -> Result<String, String> {
    let sha = dalica_faces::hash::sha256_file(file).map_err(|e| format!("model {} could not be read: {e}", m.id))?;
    match &m.sha256 {
        Some(want) if *want != sha => Err(format!("model {}'s file does not match its manifest", m.id)),
        _ => Ok(sha),
    }
}

/// Loads the recogniser (`selected`, else the newest) and the newest detector installed in
/// `models`, and opens the recogniser's cache in the library directory `library`.
fn load(models: &Path, library: &Path, selected: Option<&str>) -> Result<Loaded, String> {
    let installed = installed_models(models);
    let rec = pick(&installed, Role::Embedder, selected).ok_or("no face recogniser is installed")?;
    let file = rec.dir.join(MODEL_FILE);
    let sha256 = checked_hash(&file, &rec.manifest)?;
    let embedder = Embedder::open(&file, &rec.manifest).map_err(|e| format!("model {}: {e}", rec.manifest.id))?;
    let dim = embedder.dimension();
    let calibration = rec.manifest.calibration().ok_or_else(|| format!("model {} gives no match threshold", rec.manifest.id))?;
    let model = faces_cache::Model { id: &rec.manifest.id, sha256: &sha256 };
    let (cache, opened) = FacesCache::open(library, &model, u32::try_from(dim).unwrap_or(u32::MAX)).map_err(|e| format!("the face cache: {e}"))?;
    log::info!("face model {} loaded; its cache: {opened:?}", rec.manifest.id);
    let detector = pick(&installed, Role::Detector, None).and_then(|d| {
        let file = d.dir.join(MODEL_FILE);
        let made = checked_hash(&file, &d.manifest)
            .and_then(|_| Model::open(&file).map_err(|e| e.to_string()))
            .and_then(|m| Detector::new(m, &d.manifest).map_err(|e| e.to_string()));
        match made {
            Ok(det) => Some(det),
            Err(e) => {
                log::warn!("face detector {} not used: {e}", d.manifest.id);
                None
            }
        }
    });
    let cached = cache
        .photos()
        .map(|p| {
            let faces = p.faces.iter().filter_map(|f| Some((f.rect, Embedding::new(&f.embedding).ok()?.pack()))).collect();
            (p.stamp, p.searched, faces)
        })
        .collect();
    let info = ModelInfo { id: rec.manifest.id.clone(), sha256, dim, calibration };
    Ok(Loaded { engine: Arc::new(Engine { info, embedder, detector, cache: Mutex::new(cache) }), cached })
}

/// One photo for a worker: everything owned, so the worker reads nothing of the session's.
struct Item {
    stamp: PhotoStamp,
    path: PathBuf,
    /// Every face rectangle the photo has (a detection overlapping one is that face).
    existing: Vec<Rect01>,
    /// The faces to embed.
    todo: Vec<Rect01>,
    /// Search the photo with the detector.
    detect: bool,
    /// Record the search in the cache (not for a `faces.detect` that only counts).
    record: bool,
    /// Forget what the cache held for the photo first (Find Faces Again).
    forget: bool,
    limits: dalica_image::Limits,
}

/// What a worker found for one photo.
struct Found {
    /// The model the embeddings are of (`<id>@<sha256>`).
    tag: String,
    stamp: PhotoStamp,
    /// The faces it was to embed.
    todo: Vec<Rect01>,
    /// The new faces the detector found; `None` when the photo was not searched.
    detected: Option<Vec<Rect01>>,
    embedded: Vec<(Rect01, Packed)>,
    /// The photo could not be read or searched (the reason; never a face's data).
    error: Option<String>,
}

/// Intersection over union of two `[x0, y0, x1, y1]` boxes (0 when either is empty).
fn iou(a: [f64; 4], b: [f64; 4]) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = w * h;
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if union > 0.0 && inter.is_finite() { inter / union } else { 0.0 }
}

fn corners(r: &Rect01) -> [f64; 4] {
    [r.x0(), r.y0(), r.x1(), r.y1()]
}

fn corners32(r: &[f32; 4]) -> [f64; 4] {
    r.map(f64::from)
}

/// The work for one photo; never panics (whatever goes wrong comes back as its error). The
/// embeddings, landmarks and the search go to the model's cache before the result goes back.
fn process(item: &Item, engine: &Engine) -> Found {
    let tag = engine.tag();
    let worked = match catch_unwind(AssertUnwindSafe(|| work(item, engine))) {
        Ok(Ok(w)) => w,
        Ok(Err(why)) => return Found { tag, stamp: item.stamp, todo: Vec::new(), detected: None, embedded: Vec::new(), error: Some(why) },
        Err(_) => {
            return Found {
                tag,
                stamp: item.stamp,
                todo: Vec::new(),
                detected: None,
                embedded: Vec::new(),
                error: Some("finding faces panicked".into()),
            };
        }
    };
    let mut records = Vec::new();
    if item.forget {
        records.push(Record::Forget { photo: item.stamp.id });
    }
    for (rect, landmarks, embedding) in &worked.faces {
        records.push(Record::Face { photo: item.stamp, face: CachedFace { rect: *rect, landmarks: *landmarks, embedding: embedding.clone() } });
    }
    if worked.detected.is_some() && item.record {
        records.push(Record::Searched { photo: item.stamp });
    }
    if let Err(e) = engine.cache.lock().unwrap_or_else(PoisonError::into_inner).append(&records) {
        log::warn!("face cache of photo {} not written: {e}", item.stamp.id);
    }
    let embedded = worked.faces.into_iter().filter_map(|(rect, _, v)| Some((rect, Embedding::new(&v).ok()?.pack()))).collect();
    Found { tag, stamp: item.stamp, todo: item.todo.clone(), detected: worked.detected, embedded, error: None }
}

/// What [`work`] made of one photo.
struct Worked {
    /// The new faces (when the photo was searched).
    detected: Option<Vec<Rect01>>,
    /// Each face embedded, with the landmarks it was aligned by.
    faces: Vec<(Rect01, Option<faces_cache::Landmarks>, Vec<f32>)>,
}

/// Decodes, searches and embeds one photo.
fn work(item: &Item, engine: &Engine) -> Result<Worked, String> {
    let image = dalica_thumbs::thumbnail_with(&item.path, EDGE, &dalica_image::DecodeOptions::default(), &item.limits).map_err(|e| e.to_string())?;
    let (w, h) = (usize::try_from(image.width).unwrap_or(0), usize::try_from(image.height).unwrap_or(0));
    let rgb: Vec<u8> = image.data.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    drop(image);
    let pic = Rgb { data: &rgb, width: w, height: h };
    let found = match &engine.detector {
        Some(d) if item.detect || !item.todo.is_empty() => match d.detect(&pic, &d.options()) {
            Ok(f) => Some(f),
            Err(e) if item.detect => return Err(e.to_string()),
            Err(_) => None,
        },
        _ => None,
    };
    // The landmarks of the detection a face overlaps most, at least `MATCH_IOU`.
    let lend = |rect: &Rect01| -> Option<faces_cache::Landmarks> {
        let (_, best) = found
            .iter()
            .flatten()
            .map(|f| (iou(corners(rect), corners32(&f.rect)), f))
            .filter(|(o, _)| *o >= MATCH_IOU)
            .max_by(|a, b| a.0.total_cmp(&b.0))?;
        Some(best.landmarks.map(|(x, y)| [x, y]))
    };
    let mut faces: Vec<(Rect01, Option<faces_cache::Landmarks>)> = item.todo.iter().map(|r| (*r, lend(r))).collect();
    let detected = if item.detect {
        let mut new: Vec<Rect01> = Vec::new();
        for f in found.iter().flatten() {
            if new.len() >= MAX_NEW {
                break;
            }
            let [x0, y0, x1, y1] = corners32(&f.rect).map(|v| v.clamp(0.0, 1.0));
            let Ok(rect) = Rect01::new(x0, y0, x1, y1) else { continue };
            if item.existing.iter().chain(&new).any(|r| iou(corners(r), corners(&rect)) > SAME_FACE_IOU) {
                continue;
            }
            new.push(rect);
            faces.push((rect, Some(f.landmarks.map(|(x, y)| [x, y]))));
        }
        Some(new)
    } else {
        None
    };
    let embedded = faces
        .into_iter()
        .filter_map(|(rect, landmarks)| {
            let points = landmarks.map(|l| l.map(|[x, y]| (x, y)));
            let v = engine.embedder.embed_face(&pic, corners(&rect).map(|v| v as f32), points.as_ref()).ok()?;
            Some((rect, landmarks, v))
        })
        .collect();
    Ok(Worked { detected, faces: embedded })
}

/// What a pool job brings back.
enum Out {
    Loaded(u64, Result<Loaded, String>),
    Photo(Box<Found>),
    Grouped(u64, Vec<Vec<FaceAt>>),
    Dropped(Result<usize, String>),
}

/// Counters of the scan's work this session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceScanStats {
    /// Photos handed to a worker.
    pub started: u64,
    /// Photos whose file was read (decoded).
    pub read: u64,
    /// Photos the detector searched.
    pub searched: u64,
    /// Faces the detector added.
    pub found: u64,
    /// Faces embedded.
    pub embedded: u64,
    /// Photos that could not be read or searched.
    pub failed: u64,
}

/// What the waiting list was made for: it is made again when any of this changes.
#[derive(Clone, Debug, PartialEq)]
struct QueueKey {
    revision: u64,
    rules: Rules,
    offline: BTreeSet<PathBuf>,
    model: Option<String>,
    detector: bool,
}

/// The face scan's state.
#[derive(Default)]
pub struct FaceScan {
    job: Option<JobId>,
    seen: ActivitySeen,
    models_dir: Option<PathBuf>,
    pool: Option<Pool<u64, Out>>,
    next: u64,
    engine: Option<Arc<Engine>>,
    /// The load on its way (its number), and what the engine or the last load was for.
    loading: Option<u64>,
    load_for: Option<(PathBuf, Option<String>)>,
    last_look: Option<Timestamp>,
    load_error: Option<String>,
    queue: VecDeque<PhotoId>,
    queued_for: Option<QueueKey>,
    queued_at: Option<Timestamp>,
    /// Photos with a worker, by job key.
    in_flight: HashMap<u64, PhotoId>,
    /// Search again (Find Faces Again, `faces.detect`), and forget the cache's record first.
    force: HashSet<PhotoId>,
    forget: HashSet<PhotoId>,
    /// `faces.detect` without `apply`: found faces are counted, not written.
    dry: HashSet<PhotoId>,
    /// Photos that could not be read this session, as stamped.
    failed: HashMap<PhotoId, PhotoStamp>,
    /// Faces that could not be embedded this session (cut off the picture, say), as stamped.
    unembeddable: HashMap<FaceAt, PhotoStamp>,
    /// Detected rectangles to write as Unnamed faces at the next poll.
    pending: BTreeMap<PhotoId, Vec<Rect01>>,
    /// The grouping on its way (its key), and what was last looked at to decide whether to group.
    grouping: Option<u64>,
    group_seen: Option<(u64, u64, u32)>,
    grouped_at: Option<Timestamp>,
    dropping: bool,
    /// Cancelled by the user: nothing starts until the scan is asked for again.
    suspended: bool,
    /// Photos done and faces found since the job started.
    done: u64,
    found_in_job: u64,
    /// The progress the job shows.
    shown: (u64, u64),
    stats: FaceScanStats,
}

impl std::fmt::Debug for FaceScan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceScan")
            .field("job", &self.job)
            .field("queued", &self.queue.len())
            .field("in_flight", &self.in_flight.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl FaceScan {
    /// The `faces` job running or last run ([`crate::jobs::JobKind::Faces`]), if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }

    /// What the window has reported of the user's input.
    pub fn activity(&self) -> ActivitySeen {
        self.seen
    }

    /// The scan's counters this session.
    pub fn stats(&self) -> FaceScanStats {
        self.stats
    }

    /// The models folder the session was given, if any.
    pub fn models_dir(&self) -> Option<&Path> {
        self.models_dir.as_deref()
    }

    /// Photos waiting, and photos with a worker.
    pub fn waiting(&self) -> (usize, usize) {
        (self.queue.len(), self.in_flight.len())
    }

    /// Whether a detector is loaded.
    pub fn has_detector(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.detector.is_some())
    }

    /// Why no recogniser is loaded, when the last look found none or could not load it.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    fn note(&mut self, activity: UserActivity, now: Timestamp) {
        match activity {
            UserActivity::Input => self.seen.input = Some(now),
            UserActivity::Pointer => self.seen.pointer = Some(now),
            UserActivity::Idle | UserActivity::Away => {}
        }
        self.seen.last = Some((activity, now));
    }

    fn submit(&mut self, priority: Priority, work: impl FnOnce() -> Out + Send + 'static) -> u64 {
        self.next = self.next.wrapping_add(1);
        let key = self.next;
        let pool = self.pool.get_or_insert_with(|| Pool::new(Pace::Full.workers(cores()).max(2)));
        pool.submit(key, priority, move |_| work());
        key
    }

    /// Whether nothing is on its way and nothing is waiting that the pace would let start.
    fn idle(&self) -> bool {
        self.loading.is_none()
            && self.in_flight.is_empty()
            && self.queue.is_empty()
            && self.grouping.is_none()
            && self.pending.is_empty()
            && !self.dropping
    }
}

/// `n` with thousands separated by commas ("4,980").
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The scan's label while it runs (the People group's header).
pub fn progress_label(done: u64, total: u64) -> String {
    format!("Finding faces\u{2026} {} of {}", grouped(done), grouped(total))
}

/// The question `faces.enable {on: false}` offers (`faces.md` §7).
pub const DROP_OFFER: &str = "Also delete the face data Dalica worked out from your photos? Face rectangles and names stay.";

impl Session {
    /// The window's report of the user's input (each frame, or whenever it changes), for the face
    /// scan's pace. It notes the time and gives the scan its turn ([`Session::poll_face_scan`]).
    pub fn user_activity(&mut self, activity: UserActivity) {
        let now = self.clock.now();
        self.face_scan.note(activity, now);
        self.poll_face_scan();
    }

    /// The face scan's state (`faces.md` §4).
    pub fn face_scan(&self) -> &FaceScan {
        &self.face_scan
    }

    /// Gives the session the models folder (`None`: no models, no scan). The host passes
    /// [`default_models_dir`]; tests their own folder. Looks at it again at the next poll.
    pub fn set_face_models_dir(&mut self, dir: Option<PathBuf>) {
        self.face_scan.models_dir = dir;
        self.face_models_changed();
    }

    /// A model was installed, removed or chosen: the models folder is looked at again at the next
    /// poll (the recogniser in use is reloaded if it is another one).
    pub fn face_models_changed(&mut self) {
        self.face_scan.load_for = None;
        self.face_scan.last_look = None;
    }

    /// The face scan's turn: takes in what its workers finished, writes the faces they found,
    /// loads the models when needed, starts photos as the pace allows, keeps its job and the
    /// groups current. Never blocks and never touches the disk; returns how many results it took.
    pub fn poll_face_scan(&mut self) -> usize {
        let now = self.clock.now();
        let n = self.take_face_results();
        self.write_found_faces();
        let people = self.settings().people();
        let wanted = people.find_faces && !self.is_read_only() && self.face_scan.models_dir.is_some() && !self.face_scan.suspended;
        if wanted {
            self.ensure_face_models(now, people.model.as_deref());
            if self.face_scan.engine.is_some() {
                self.refresh_face_queue(now);
                self.start_face_photos(now);
            }
        } else {
            self.face_scan.queue.clear();
            self.face_scan.queued_for = None;
            if !people.find_faces || self.face_scan.models_dir.is_none() {
                self.face_scan.engine = None;
                self.face_scan.load_for = None;
            }
        }
        self.update_face_job(now);
        self.regroup_faces(people.grouping_confidence, now);
        n
    }

    /// Polls the face scan until it is idle (nothing loading, waiting or in progress, the groups
    /// current) or `timeout` has passed; returns whether it got there. Blocks the calling thread
    /// (CLI, tests).
    pub fn wait_faces(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll_face_scan();
            if self.face_scan.idle() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// Starts the scan again after a cancel (`faces.enable`, `faces.detect`, `faces.reset`).
    pub(crate) fn resume_face_scan(&mut self) {
        self.face_scan.suspended = false;
        self.face_scan.queued_for = None;
    }

    /// Find Faces Again for `ids` (after their found faces were taken out): forgets what the index
    /// and the cache know of them and searches them again first.
    pub(crate) fn search_faces_again(&mut self, ids: &[PhotoId], record: bool) {
        for id in ids.iter().rev() {
            self.faces_index.forget(*id);
            self.face_scan.force.insert(*id);
            self.face_scan.failed.remove(id);
            self.face_scan.unembeddable.retain(|f, _| f.photo != *id);
            if record {
                self.face_scan.forget.insert(*id);
                self.face_scan.dry.remove(id);
            } else {
                self.face_scan.dry.insert(*id);
            }
            self.face_scan.queue.retain(|q| q != id);
            self.face_scan.queue.push_front(*id);
        }
        self.resume_face_scan();
    }

    /// Stops the scan and deletes every model's cached embeddings in the library directory
    /// (`faces.enable {on: false, dropEmbeddings: true}`); the index forgets them too.
    pub(crate) fn drop_face_embeddings(&mut self) {
        self.face_scan.engine = None;
        self.face_scan.load_for = None;
        self.faces_index.reset(None);
        let dir = self.dir.clone();
        self.face_scan.dropping = true;
        self.face_scan.submit(TASKS, move || {
            let run = || -> Result<usize, String> {
                let ids = faces_cache::list(&dir).map_err(|e| e.to_string())?;
                let mut n = 0;
                for id in ids {
                    if faces_cache::remove(&dir, &id).map_err(|e| e.to_string())? {
                        n += 1;
                    }
                }
                Ok(n)
            };
            Out::Dropped(catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|_| Err("deleting the face data panicked".into())))
        });
    }

    /// The scan's state for `faces.index` and the People group's header.
    pub(crate) fn face_scan_status(&self) -> Value {
        let s = &self.face_scan;
        let people = self.settings().people();
        let (waiting, working) = s.waiting();
        let total = s.done.saturating_add(waiting as u64).saturating_add(working as u64);
        let state = if !people.find_faces {
            "off"
        } else if s.models_dir.is_none() || (s.engine.is_none() && s.loading.is_none()) {
            "noModel"
        } else if s.loading.is_some() && s.engine.is_none() {
            "loading"
        } else if s.suspended {
            "cancelled"
        } else if waiting + working == 0 {
            "idle"
        } else if Pace::of(&s.seen, self.clock.now()) == Pace::Pause && working == 0 {
            "paused"
        } else {
            "running"
        };
        let label = if waiting + working > 0 { Value::String(progress_label(s.done, total)) } else { Value::Null };
        json!({
            "state": state,
            "model": self.faces_index.model(),
            "detector": s.has_detector(),
            "problem": s.load_error,
            "done": s.done,
            "total": total,
            "label": label,
            "job": s.job,
            "pace": Pace::of(&s.seen, self.clock.now()).name(),
            "faces": self.faces_index.faces(),
            "groups": self.faces_index.groups().len(),
            "stats": {
                "started": s.stats.started, "read": s.stats.read, "searched": s.stats.searched,
                "found": s.stats.found, "embedded": s.stats.embedded, "failed": s.stats.failed,
            },
        })
    }

    /// Loads the models when there are none, the choice or the folder changed, or (without a
    /// recogniser) the last look is [`RECHECK`] old.
    fn ensure_face_models(&mut self, now: Timestamp, selected: Option<&str>) {
        let s = &mut self.face_scan;
        let Some(dir) = s.models_dir.clone() else { return };
        if s.loading.is_some() {
            return;
        }
        let want = (dir.clone(), selected.map(str::to_string));
        if s.load_for.as_ref() == Some(&want) {
            let stale = s.engine.is_none() && s.last_look.is_none_or(|t| now.since(t) >= RECHECK);
            if !stale {
                return;
            }
        }
        s.load_for = Some(want);
        s.last_look = Some(now);
        let library = self.dir.clone();
        let selected = selected.map(str::to_string);
        s.next = s.next.wrapping_add(1);
        let number = s.next;
        s.loading = Some(number);
        s.submit(TASKS, move || {
            let run = || load(&dir, &library, selected.as_deref());
            Out::Loaded(number, catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|_| Err("loading the face models panicked".into())))
        });
    }

    /// Takes in every result the workers finished.
    fn take_face_results(&mut self) -> usize {
        let mut n = 0;
        while let Some((key, out)) = self.face_scan.pool.as_ref().and_then(Pool::try_recv) {
            n += 1;
            match out {
                Out::Loaded(number, result) => self.face_models_loaded(number, result),
                Out::Photo(found) => {
                    self.face_scan.in_flight.remove(&key);
                    self.face_photo_done(*found);
                }
                Out::Grouped(k, groups) => {
                    if self.face_scan.grouping == Some(k) {
                        self.face_scan.grouping = None;
                        self.faces_index.set_groups(k, groups);
                        log::debug!("faces grouped: {} groups", self.faces_index.groups().len());
                    }
                }
                Out::Dropped(result) => {
                    self.face_scan.dropping = false;
                    match result {
                        Ok(n) => log::info!("face data deleted: {n} caches"),
                        Err(e) => log::warn!("face data not deleted: {e}"),
                    }
                }
            }
        }
        n
    }

    fn face_models_loaded(&mut self, number: u64, result: Result<Loaded, String>) {
        let s = &mut self.face_scan;
        if s.loading != Some(number) {
            return;
        }
        s.loading = None;
        match result {
            Ok(loaded) => {
                s.load_error = None;
                let info = loaded.engine.info.clone();
                let same = self.faces_index.model_info().is_some_and(|m| m.id == info.id && m.sha256 == info.sha256);
                if !same {
                    self.faces_index.reset(Some(info));
                }
                let mut faces = 0usize;
                for (stamp, searched, list) in loaded.cached {
                    let current = self.library.photo(stamp.id).is_some_and(|p| PhotoStamp::of(p) == stamp);
                    if current && !same {
                        faces = faces.saturating_add(list.len());
                        self.faces_index.add(stamp, searched, list);
                    }
                }
                log::info!("face index: {faces} faces from the cache");
                self.face_scan.engine = Some(loaded.engine);
                self.face_scan.queued_for = None;
            }
            Err(e) => {
                log::info!("face scan: {e}");
                s.load_error = Some(e);
                s.engine = None;
            }
        }
    }

    fn face_photo_done(&mut self, found: Found) {
        let s = &mut self.face_scan;
        let id = found.stamp.id;
        s.done = s.done.saturating_add(1);
        let current = self.library.photo(id).is_some_and(|p| PhotoStamp::of(p) == found.stamp);
        let same_model = s.engine.as_ref().is_some_and(|e| e.tag() == found.tag);
        if let Some(why) = found.error {
            s.stats.failed = s.stats.failed.saturating_add(1);
            s.failed.insert(id, found.stamp);
            s.force.remove(&id);
            s.forget.remove(&id);
            log::debug!("face scan: photo {id} not read: {why}");
            return;
        }
        s.stats.read = s.stats.read.saturating_add(1);
        s.stats.embedded = s.stats.embedded.saturating_add(found.embedded.len() as u64);
        if !current || !same_model {
            return;
        }
        for rect in &found.todo {
            if !found.embedded.iter().any(|(r, _)| r == rect) {
                s.unembeddable.insert(FaceAt::new(id, rect), found.stamp);
            }
        }
        let searched = found.detected.is_some();
        if searched {
            s.stats.searched = s.stats.searched.saturating_add(1);
            s.force.remove(&id);
            s.forget.remove(&id);
        }
        let dry = s.dry.remove(&id);
        if let Some(new) = found.detected {
            s.stats.found = s.stats.found.saturating_add(new.len() as u64);
            s.found_in_job = s.found_in_job.saturating_add(new.len() as u64);
            if !dry && !new.is_empty() {
                s.pending.entry(id).or_default().extend(new);
            }
        }
        self.faces_index.add(found.stamp, searched && !dry, found.embedded);
    }

    /// Writes the detected rectangles as Unnamed faces: one op, no undo step, as a scan's imports.
    fn write_found_faces(&mut self) {
        if self.face_scan.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.face_scan.pending);
        let lib = &self.library;
        let mut changes = Vec::new();
        for (id, rects) in pending {
            if lib.photo(id).is_none() {
                continue;
            }
            let mut list = lib.faces(id).to_vec();
            for rect in rects {
                if list.len() >= MAX_FACES {
                    break;
                }
                if list.iter().any(|f| iou(corners(&f.rect), corners(&rect)) > SAME_FACE_IOU) {
                    continue;
                }
                list.push(Face::new(rect, FacePerson::Unnamed));
            }
            if list.as_slice() != lib.faces(id) {
                changes.push((id, list));
            }
        }
        if changes.is_empty() {
            return;
        }
        let photos = changes.len();
        if let Err(e) = self.apply_from_files(Op::SetFaces { faces: changes }) {
            log::warn!("found faces of {photos} photos not written: {e}");
        }
    }

    /// Whether the scan may read the files of folder `folder`: it finds faces, it is not hidden
    /// and its volume is not offline.
    fn face_folder_readable(&self, folder: FolderId) -> bool {
        let Some(f) = self.library.folder(folder) else { return false };
        self.folder_rules().faces(&f.path) && !self.library.is_folder_hidden(folder) && !self.is_folder_offline(folder)
    }

    /// Whether the scan may read photo `id`'s file: a photo (not a video) in a folder it may read.
    pub(crate) fn face_scan_may_read(&self, id: PhotoId) -> bool {
        let Some(photo) = self.library.photo(id) else { return false };
        !self.library.facts(id).video && self.face_folder_readable(photo.folder)
    }

    /// Whether `photo` (one the scan may read) needs it: it did not fail as it is, and it is to be
    /// searched or has a face without an embedding that could be made.
    fn face_needs(&self, photo: &Photo, engine: &Engine) -> bool {
        let stamp = PhotoStamp::of(photo);
        if self.face_scan.failed.get(&photo.id) == Some(&stamp) {
            return false;
        }
        let faces = self.library.faces(photo.id);
        self.face_detect_wanted(photo, engine)
            || faces.iter().any(|f| {
                self.faces_index.embedding(photo, &f.rect).is_none()
                    && self.face_scan.unembeddable.get(&FaceAt::new(photo.id, &f.rect)) != Some(&stamp)
            })
    }

    /// Whether `photo` is to be searched: asked for, or without faces and not searched yet.
    fn face_detect_wanted(&self, photo: &Photo, engine: &Engine) -> bool {
        engine.detector.is_some()
            && (self.face_scan.force.contains(&photo.id) || (self.library.faces(photo.id).is_empty() && !self.faces_index.searched(photo)))
    }

    /// The work photo `id` needs, or `None` when it needs none (or must not be read).
    fn face_item(&self, id: PhotoId, engine: &Engine) -> Option<Item> {
        let lib = &self.library;
        let photo = lib.photo(id)?;
        if !self.face_scan_may_read(id) || !self.face_needs(photo, engine) {
            return None;
        }
        let faces = lib.faces(id);
        let detect = self.face_detect_wanted(photo, engine);
        let (_, _, _, limits) = self.thumbs.screen_source();
        Some(Item {
            stamp: PhotoStamp::of(photo),
            path: lib.folder(photo.folder)?.path.join(&photo.name),
            existing: faces.iter().map(|f| f.rect).collect(),
            todo: faces.iter().filter(|f| self.faces_index.embedding(photo, &f.rect).is_none()).map(|f| f.rect).collect(),
            detect,
            record: !self.face_scan.dry.contains(&id),
            forget: self.face_scan.forget.contains(&id),
            limits,
        })
    }

    /// Makes the waiting list again when the library, the folder rules, the offline volumes or the
    /// model changed: photos with named faces first, then the newest folders, newest photos first.
    fn refresh_face_queue(&mut self, now: Timestamp) {
        let Some(engine) = self.face_scan.engine.clone() else { return };
        let key = QueueKey {
            revision: self.library.revision(),
            rules: self.folder_rules().clone(),
            offline: self.offline_roots(),
            model: self.faces_index.model().map(str::to_string),
            detector: engine.detector.is_some(),
        };
        if let Some(last) = &self.face_scan.queued_for {
            if *last == key {
                return;
            }
            // Only the library changed: new and changed photos join once the list ran dry, at most
            // once every `REQUEUE` (a pass over the library is not made on every frame).
            let same = last.rules == key.rules && last.offline == key.offline && last.model == key.model && last.detector == key.detector;
            let recent = self.face_scan.queued_at.is_some_and(|t| now.since(t) < REQUEUE);
            if same && (!self.face_scan.queue.is_empty() || recent) {
                return;
            }
        }
        let lib = &self.library;
        let readable: HashMap<FolderId, bool> = lib.folders().map(|f| (f.id, self.face_folder_readable(f.id))).collect();
        let busy: HashSet<PhotoId> = self.face_scan.in_flight.values().copied().collect();
        let date = |id: PhotoId| {
            let p = lib.photo(id);
            (lib.info(id).date.or(p.and_then(|p| p.date)), p.map(|p| p.mtime))
        };
        let mut newest: HashMap<FolderId, _> = HashMap::new();
        for p in lib.photos() {
            let d = date(p.id);
            newest.entry(p.folder).and_modify(|n| *n = std::cmp::max(*n, d)).or_insert(d);
        }
        let mut list: Vec<(bool, _, FolderId, _, PhotoId)> = Vec::new();
        for p in lib.photos() {
            let may = readable.get(&p.folder).copied().unwrap_or(false) && !lib.facts(p.id).video;
            if busy.contains(&p.id) || !may || !self.face_needs(p, &engine) {
                continue;
            }
            let named = lib.faces(p.id).iter().any(|f| f.person.person().is_some());
            let folder = newest.get(&p.folder).copied().unwrap_or_default();
            list.push((named, folder, p.folder, date(p.id), p.id));
        }
        // Named first; then the newest folders, then the newest photos; ties by id.
        list.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2)).then_with(|| b.3.cmp(&a.3)).then_with(|| a.4.cmp(&b.4))
        });
        let forced: Vec<PhotoId> = self.face_scan.queue.iter().copied().filter(|id| self.face_scan.force.contains(id)).collect();
        let mut queue: VecDeque<PhotoId> = forced.iter().copied().collect();
        let first: HashSet<PhotoId> = forced.into_iter().collect();
        queue.extend(list.into_iter().map(|e| e.4).filter(|id| !first.contains(id)));
        log::debug!("face scan: {} photos waiting", queue.len());
        self.face_scan.queue = queue;
        self.face_scan.queued_for = Some(key);
        self.face_scan.queued_at = Some(now);
    }

    /// Hands photos to the workers while the pace allows: none while the user is busy; a photo
    /// asked for by name (Find Faces Again, `faces.detect`) one at a time even then.
    fn start_face_photos(&mut self, now: Timestamp) {
        let Some(engine) = self.face_scan.engine.clone() else { return };
        let pace = Pace::of(&self.face_scan.seen, now);
        let allowed = pace.workers(cores());
        while let Some(&id) = self.face_scan.queue.front() {
            let asked = self.face_scan.force.contains(&id);
            let room = if asked { allowed.max(1) } else { allowed };
            if self.face_scan.in_flight.len() >= room {
                break;
            }
            self.face_scan.queue.pop_front();
            if self.face_scan.in_flight.values().any(|p| *p == id) {
                continue;
            }
            let Some(item) = self.face_item(id, &engine) else {
                self.face_scan.force.remove(&id);
                self.face_scan.forget.remove(&id);
                self.face_scan.dry.remove(&id);
                continue;
            };
            let engine = Arc::clone(&engine);
            let key = self.face_scan.submit(PHOTOS, move || Out::Photo(Box::new(process(&item, &engine))));
            self.face_scan.in_flight.insert(key, id);
            self.face_scan.stats.started = self.face_scan.stats.started.saturating_add(1);
        }
    }

    /// Starts, updates or ends the `faces` job; a cancelled job stops the scan until it is asked
    /// for again.
    fn update_face_job(&mut self, now: Timestamp) {
        let s = &mut self.face_scan;
        let busy = !s.queue.is_empty() || !s.in_flight.is_empty();
        let total = s.done.saturating_add(s.queue.len() as u64).saturating_add(s.in_flight.len() as u64);
        let running = s.job.filter(|j| self.jobs.get(*j, now).is_some_and(|e| !e.state.is_finished()));
        match running {
            None if busy => {
                s.done = 0;
                s.found_in_job = 0;
                let total = s.queue.len().saturating_add(s.in_flight.len()) as u64;
                let (job, _) = self.jobs.start(JobKind::Faces, progress_label(0, total), true, now);
                s.job = Some(job);
                s.shown = (0, total);
            }
            Some(job) if self.jobs.cancel_requested(job) => {
                s.suspended = true;
                s.queue.clear();
                s.queued_for = None;
                let result = json!({ "photos": s.done, "found": s.found_in_job });
                self.jobs.finish(job, Outcome::Cancelled(result), "Finding faces stopped", now);
                log::info!("face scan cancelled after {} photos", s.done);
            }
            Some(job) if busy => {
                if s.shown != (s.done, total) {
                    self.jobs.progress(job, JobProgress { done: s.done, total }, progress_label(s.done, total));
                    s.shown = (s.done, total);
                }
            }
            Some(job) => {
                let result = json!({ "photos": s.done, "found": s.found_in_job });
                log::info!("face scan done: {} photos, {} faces found", s.done, s.found_in_job);
                self.jobs.finish(job, Outcome::Done(result), "Faces found", now);
                s.done = 0;
            }
            None => {}
        }
    }

    /// Makes the groups again on a worker when the unnamed faces, their embeddings or the Grouping
    /// confidence changed; while photos are being scanned, at most every [`REGROUP`].
    fn regroup_faces(&mut self, confidence: u32, now: Timestamp) {
        if self.face_scan.grouping.is_some() {
            return;
        }
        let Some(info) = self.faces_index.model_info() else { return };
        let threshold = info.threshold(confidence);
        let seen = (self.library.revision(), self.faces_index.generation(), threshold.to_bits());
        if self.face_scan.group_seen == Some(seen) {
            return;
        }
        let scanning = !self.face_scan.queue.is_empty() || !self.face_scan.in_flight.is_empty();
        if scanning && self.face_scan.grouped_at.is_some_and(|t| now.since(t) < REGROUP) {
            return;
        }
        self.face_scan.group_seen = Some(seen);
        let key = self.faces_index.grouping_key(&self.library, threshold);
        if self.faces_index.grouped_for(key) {
            return;
        }
        let members = self.faces_index.members(&self.library);
        self.face_scan.grouping = Some(key);
        self.face_scan.grouped_at = Some(now);
        self.face_scan.submit(TASKS, move || {
            let run = || {
                let groups = dalica_faces::cluster::Groups::build(threshold, members);
                groups.groups().into_iter().map(|g| g.faces).collect::<Vec<_>>()
            };
            Out::Grouped(key, catch_unwind(AssertUnwindSafe(run)).unwrap_or_default())
        });
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::Arc;

    use dalica_core::VirtualClock;

    use super::*;

    #[test]
    fn each_report_is_noted_with_the_session_clocks_time() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_000)));
        let mut s = Session::open(dir.path(), clock.clone()).unwrap();
        let t = Timestamp::from_millis;
        assert_eq!(s.face_scan().activity(), ActivitySeen::default());
        s.user_activity(UserActivity::Input);
        clock.advance(Duration::from_millis(500));
        s.user_activity(UserActivity::Pointer);
        clock.advance(Duration::from_millis(500));
        s.user_activity(UserActivity::Idle);
        assert_eq!(
            s.face_scan().activity(),
            ActivitySeen { input: Some(t(1_000)), pointer: Some(t(1_500)), last: Some((UserActivity::Idle, t(2_000))) }
        );
        s.user_activity(UserActivity::Away);
        assert_eq!(s.face_scan().activity().last, Some((UserActivity::Away, t(2_000))));
        assert_eq!((s.face_scan().activity().input, s.face_scan().job()), (Some(t(1_000)), None), "the earlier input is kept; no job started");
    }

    /// Input holds every new photo for a second; the pointer allows one; three idle seconds about
    /// half the cores; away most of them; never more than memory allows.
    #[test]
    fn the_pace_follows_the_users_input() {
        let t = Timestamp::from_millis;
        let mut seen = ActivitySeen::default();
        assert_eq!(Pace::of(&seen, t(0)), Pace::Normal);
        seen.input = Some(t(10_000));
        seen.last = Some((UserActivity::Input, t(10_000)));
        assert_eq!(Pace::of(&seen, t(10_999)), Pace::Pause);
        assert_eq!(Pace::of(&seen, t(11_000)), Pace::Light);
        assert_eq!(Pace::of(&seen, t(13_000)), Pace::Normal);
        seen.pointer = Some(t(13_000));
        assert_eq!(Pace::of(&seen, t(15_999)), Pace::Light);
        seen.last = Some((UserActivity::Away, t(16_000)));
        assert_eq!(Pace::of(&seen, t(16_000)), Pace::Full);
        assert_eq!([Pace::Pause, Pace::Light, Pace::Normal, Pace::Full].map(|p| p.workers(16)), [0, 1, MAX_WORKERS.min(8), MAX_WORKERS.min(12)]);
        assert_eq!(Pace::Normal.workers(1), 1);
        assert_eq!(MAX_WORKERS, 4);
    }

    #[test]
    fn the_models_folder_follows_the_platform_convention() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v));
        let own = if cfg!(windows) { r"C:\m" } else { "/m" };
        assert_eq!(models_dir_from(|k| (k == MODELS_ENV).then(|| OsString::from(own))), Some(PathBuf::from(own)));
        assert_eq!(models_dir_from(env(&[(MODELS_ENV, "relative")])), None, "a relative folder is not taken");
        if cfg!(target_os = "linux") {
            assert_eq!(models_dir_from(env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.config/dalica/models")));
            assert_eq!(models_dir_from(env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/c")])), Some(PathBuf::from("/c/dalica/models")));
        }
    }

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(progress_label(312, 4_980), "Finding faces\u{2026} 312 of 4,980");
        assert_eq!([grouped(0), grouped(999), grouped(1_000), grouped(1_234_567)], ["0", "999", "1,000", "1,234,567"]);
    }

    #[test]
    fn overlap_is_intersection_over_union() {
        assert!((iou([0.0, 0.0, 0.5, 0.5], [0.0, 0.0, 0.5, 0.5]) - 1.0).abs() < 1e-12);
        assert!((iou([0.0, 0.0, 0.5, 0.5], [0.25, 0.0, 0.75, 0.5]) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(iou([0.0, 0.0, 0.1, 0.1], [0.5, 0.5, 0.6, 0.6]), 0.0);
        assert_eq!(iou([0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]), 0.0);
    }
}
