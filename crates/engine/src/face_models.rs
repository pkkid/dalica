//! Face models (`faces.md` §3 and §9; the commands are `cmd/face_models.rs`): the models folder
//! the host gives the session (`Session::set_face_models_dir`: `$DALICA_FACE_MODELS` or
//! `<config>/models`, shared by the app, the CLI and MCP), what it holds, what can be downloaded,
//! and the downloads, installs and removals that change it.
//!
//! - **What is listed** ([`Listing`], read on a worker whenever the folder is given, changes or is
//!   changed by a command here): the installed models (`faces_scan::installed_models`: a folder per
//!   id with `model.onnx` and `face-model.json`), the user's `catalog.json` ([`CATALOG_FILE`],
//!   `dalica_faces::catalog`) and the partial downloads in [`DOWNLOADS`]. With the built-in list
//!   (`dalica_faces::known::builtin`, reserved decision 5) they make `faces.models.list`'s models:
//!   the built-in ones first, then the catalog's, then any other installed model ("local").
//! - **Consent first.** A download or an install of a local file runs only with `acknowledged:
//!   true` (the ticked licence box); without it nothing is read, requested or written. A download is
//!   a `dalica-fetch` [`Consent`] for exactly its pinned file.
//! - **A download** (`JobKind::Network`, cancellable, with progress) goes through the session's
//!   transport: the HTTPS transport of `dalica-fetch` through the proxy of Options ▸ Network, or the
//!   one a test gives ([`Session::set_face_model_transport`]). It lands in `<models>/.downloads/<id>.onnx`
//!   (a cut or cancelled download resumes from its `.part`), is checked against its pinned size and
//!   SHA-256 (a wrong file is deleted by `dalica-fetch`), passes the self-test
//!   (`dalica_faces::selftest`; a model that fails it is deleted) and only then moves into
//!   `<models>/<id>/model.onnx` with its manifest written last: a model without a manifest is never
//!   used.
//! - **A local file** (`faces.models.install`, a `JobKind::Faces` job): read once (at most 512 MiB),
//!   recognised by its SHA-256 (the known models, then the catalog) or by its shapes with the
//!   assumptions stated (`dalica_faces::known::recognise`), self-tested on the bytes read, then
//!   copied (verified, never replacing) into the models folder.
//! - **Removing** a model deletes its two files and its cached embeddings in this library
//!   (`dalica_store::faces_cache`); while it runs the scan lets go of the model.
//! - **The models folder is never inside a watched folder**: downloads and installs are refused
//!   when it is.
//!
//! The owner thread never touches the disk: listing, inspecting, installing, downloading and
//! removing run on worker threads, whose answers [`Session::poll`] takes in.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use dalica_faces::catalog::{self, Catalog};
use dalica_faces::known::{self, Download, Recognition};
use dalica_faces::manifest::{self, Commercial, ModelManifest, Role};
use dalica_faces::{hash, limits, onnx, selftest};
use dalica_fetch::{Consent, FetchError, FileSpec, HttpsTransport, Proxy, Transport};
use dalica_fs::transfer::{self, Moved, Outcome as Placed};
use dalica_fs::{Expect, Identity, Times};
use dalica_store::faces_cache;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::faces_scan::{Installed, MANIFEST_FILE, MODEL_FILE, installed_models};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;
use crate::settings::{NetworkLogLevel, ProxyMode};

/// The user's own list of models to download, in the models folder (`faces.md` §3.2).
pub const CATALOG_FILE: &str = "catalog.json";
/// Where downloads land before their self-test, in the models folder (never a model id: hidden).
pub const DOWNLOADS: &str = ".downloads";
/// How long a command that needs the models folder read waits for it.
pub const LOOK_WAIT: Duration = Duration::from_secs(2);
/// The most local files inspected at once.
pub const MAX_INSPECTIONS: usize = 2;
/// What "commercial use" in the list describes (`faces.md` §3.2).
pub const COMMERCIAL_NOTE: &str = "Commercial use describes the weights' licence only; it says nothing about the data the model was trained on.";
/// Why a download or an install did nothing without `acknowledged: true`.
pub const NEEDS_CONSENT: &str = "Tick \"I have read these terms and accept them for my own use\" first: nothing was read, downloaded or installed.";
/// The match threshold assumed for a recogniser Dalica does not know: the cosine at Suggestion and
/// Grouping confidence 85 (`faces.md` §5), stated with the other assumptions.
pub const ASSUMED_MATCH_COSINE: f32 = 0.4;
/// Between two looks at the workers while a command waits for the folder to be read.
const WAIT_STEP: Duration = Duration::from_millis(2);

/// A transport the session's downloads go through (shared with the worker).
pub type SharedTransport = Arc<dyn Transport + Send + Sync>;

/// What the models folder held when it was last read.
#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub folder: PathBuf,
    /// The installed models, by id.
    pub installed: Vec<Installed>,
    /// The user's `catalog.json` (empty without one), with why entries were left out.
    pub catalog: Catalog,
    /// Partial downloads: the model id and the bytes kept.
    pub partial: Vec<(String, u64)>,
}

impl Listing {
    fn installed(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|m| m.manifest.id == id)
    }
}

/// The name a download of model `id` has in [`DOWNLOADS`].
fn staged_name(id: &str) -> String {
    format!("{id}.onnx")
}

/// The model id of a partial download's name (`<id>.onnx.part`).
fn partial_id(name: &str) -> Option<&str> {
    name.strip_suffix(".onnx.part").filter(|id| manifest::valid_id(id))
}

/// Reads the models folder (on a worker).
fn look(dir: &Path) -> Listing {
    let installed = installed_models(dir);
    let limit = u64::try_from(catalog::MAX_CATALOG_BYTES).unwrap_or(u64::MAX);
    let catalog = match dalica_fs::read(&dir.join(CATALOG_FILE), limit) {
        Ok(Some(r)) => catalog::parse(&r.bytes),
        Ok(None) => Catalog::default(),
        Err(e) => Catalog { entries: Vec::new(), errors: vec![format!("catalog.json was not read: {e}")] },
    };
    let partial = dalica_fs::list_shaped(&dir.join(DOWNLOADS), |n| partial_id(n).is_some())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| Some((partial_id(&f.name)?.to_string(), f.len)))
        .collect();
    Listing { folder: dir.to_path_buf(), installed, catalog, partial }
}

/// How a worker's job ended, and its final label.
type Ended = (Outcome, String);

enum Msg {
    Looked(u64, Box<Listing>),
    Done(JobId, Ended),
}

/// The download running.
struct Running {
    job: JobId,
    id: String,
    name: String,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    /// The job's cancel flag, raised when the session closes.
    cancel: Arc<AtomicBool>,
}

enum TaskKind {
    Inspect,
    Install,
    /// Removing model `id`; `restore` is the models folder the scan was given back after it.
    Remove {
        id: String,
        restore: PathBuf,
        /// Set until the worker starts: it waits for the scan to have let go of the model (no
        /// load or photo of it in progress, so nothing writes its cache once it is deleted).
        waiting: bool,
    },
}

struct Task {
    job: JobId,
    kind: TaskKind,
}

/// The face models' state.
#[derive(Default)]
pub struct FaceModels {
    download: Option<JobId>,
    running: Option<Running>,
    tasks: Vec<Task>,
    listing: Option<Arc<Listing>>,
    /// The look on its way: its number.
    looking: Option<u64>,
    next: u64,
    /// The folder changed since it was read.
    stale: bool,
    channel: Option<(Sender<Msg>, Receiver<Msg>)>,
    transport: Option<SharedTransport>,
}

impl std::fmt::Debug for FaceModels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceModels")
            .field("download", &self.download)
            .field("downloading", &self.running.as_ref().map(|r| &r.id))
            .field("tasks", &self.tasks.len())
            .field("listed", &self.listing.as_ref().map(|l| l.installed.len()))
            .finish_non_exhaustive()
    }
}

impl Drop for FaceModels {
    /// A download still running when the session closes stops at its next read (what it fetched
    /// is kept to resume).
    fn drop(&mut self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::SeqCst);
        }
    }
}

impl FaceModels {
    /// The download running or last run ([`crate::jobs::JobKind::Network`]), if any.
    pub fn download(&self) -> Option<JobId> {
        self.download
    }

    /// The model being downloaded now.
    pub fn downloading(&self) -> Option<&str> {
        self.running.as_ref().map(|r| r.id.as_str())
    }

    /// What the models folder held when it was last read.
    pub fn listing(&self) -> Option<&Listing> {
        self.listing.as_deref()
    }

    /// Nothing reading, downloading, installing or removing.
    pub fn idle(&self) -> bool {
        self.looking.is_none() && self.running.is_none() && self.tasks.is_empty()
    }

    fn sender(&mut self) -> Sender<Msg> {
        let (tx, _) = self.channel.get_or_insert_with(mpsc::channel);
        tx.clone()
    }

    fn recv(&self) -> Option<Msg> {
        self.channel.as_ref().and_then(|(_, rx)| rx.try_recv().ok())
    }
}

/// Runs `work` on a thread of its own; its answer (or a caught panic) goes to `tx`.
fn spawn(name: &str, tx: Sender<Msg>, job: JobId, work: impl FnOnce() -> Ended + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let ended = catch_unwind(AssertUnwindSafe(work))
                .unwrap_or_else(|_| (Outcome::Failed("the face model task stopped unexpectedly".into()), "Face model task failed".into()));
            // The session may have closed meanwhile; nobody is waiting then.
            let _ = tx.send(Msg::Done(job, ended));
        })
        .map(|_| ())
        .map_err(|e| format!("cannot start a worker: {e}"))
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Detector => "detector",
        Role::Embedder => "recogniser",
    }
}

fn commercial_name(c: Commercial) -> &'static str {
    match c {
        Commercial::Yes => "yes",
        Commercial::No => "no",
        Commercial::Unknown => "unknown",
    }
}

/// The host of an `https://` address ("github.com").
fn host_of(url: &str) -> Option<&str> {
    url.strip_prefix("https://").and_then(|r| r.split(['/', '?', '#']).next()).filter(|h| !h.is_empty())
}

/// What the user reads about a model before it is downloaded or installed (`faces.md` §3.1).
pub fn model_facts(m: &ModelManifest, origin: &str) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "version": m.version,
        "role": role_name(m.role),
        "origin": origin,
        "licence": { "name": m.licence.name, "commercial": commercial_name(m.licence.commercial), "url": m.licence.url, "notice": m.licence.notice },
        "commercialUse": commercial_name(m.licence.commercial),
        "trainedOn": m.provenance,
        "sizeBytes": m.size_bytes,
        "page": m.source,
        "site": m.source.as_deref().and_then(host_of),
        "caution": origin != "builtin" || m.licence.commercial != Commercial::Yes,
    })
}

/// The recogniser in use among `installed`: `selected` when it is installed, else the newest (ties
/// by id), as the face scan picks it.
fn in_use<'a>(installed: &'a [Installed], selected: Option<&str>) -> Option<&'a str> {
    let recognisers = || installed.iter().filter(|m| m.manifest.role == Role::Embedder);
    if let Some(id) = selected
        && let Some(m) = recognisers().find(|m| m.manifest.id == id)
    {
        return Some(&m.manifest.id);
    }
    recognisers().max_by(|a, b| a.modified.cmp(&b.modified).then_with(|| b.manifest.id.cmp(&a.manifest.id))).map(|m| m.manifest.id.as_str())
}

/// A model that can be downloaded: built in, or from the catalog.
struct Offer {
    manifest: ModelManifest,
    download: Download,
    origin: &'static str,
}

fn offers(listing: Option<&Listing>) -> Vec<Offer> {
    let mut out: Vec<Offer> = known::builtin().into_iter().map(|(manifest, download)| Offer { manifest, download, origin: "builtin" }).collect();
    for e in listing.map(|l| l.catalog.entries.as_slice()).unwrap_or_default() {
        if let Some(download) = e.download() {
            out.push(Offer { manifest: e.manifest.clone(), download, origin: "catalog" });
        }
    }
    out
}

/// A file read and recognised.
struct Inspected {
    bytes: Vec<u8>,
    identity: Identity,
    sha256: String,
    file_name: String,
    recognition: Recognition,
}

impl Inspected {
    fn size(&self) -> u64 {
        u64::try_from(self.bytes.len()).unwrap_or(u64::MAX)
    }
}

/// Reads the model file at `path` (at most 512 MiB) and recognises it; `extra` are the catalog's
/// manifests, also recognised by hash.
fn inspect_file(path: &Path, extra: &[ModelManifest]) -> Result<Inspected, String> {
    let read =
        dalica_fs::read(path, limits::MAX_MODEL_BYTES).map_err(|e| e.to_string())?.ok_or_else(|| format!("{} does not exist", path.display()))?;
    let sha256 = hash::sha256_hex(&read.bytes);
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = u64::try_from(read.bytes.len()).unwrap_or(u64::MAX);
    let recognition = match onnx::probe_reader(&mut Cursor::new(&read.bytes)) {
        Ok(info) => with_threshold(known::recognise(&info, &sha256, size, &file_name, extra)),
        Err(e) => Recognition::Unsupported(e.to_string()),
    };
    Ok(Inspected { bytes: read.bytes, identity: read.identity, sha256, file_name, recognition })
}

/// An unknown recogniser gets [`ASSUMED_MATCH_COSINE`] as its match threshold, stated with its
/// other assumptions: without one the scan could not use it.
fn with_threshold(recognition: Recognition) -> Recognition {
    match recognition {
        Recognition::Assumed { mut manifest, mut assumptions } if manifest.role == Role::Embedder && manifest.thresholds.match_cosine.is_none() => {
            manifest.thresholds.match_cosine = Some(ASSUMED_MATCH_COSINE);
            assumptions.push(format!(
                "Two faces match at a cosine similarity of {ASSUMED_MATCH_COSINE:.2} or more at confidence 85, a usual starting point for face recognisers; move the Suggestion and Grouping confidences if names are suggested too often or too rarely."
            ));
            Recognition::Assumed { manifest, assumptions }
        }
        other => other,
    }
}

fn inspection_value(path: &Path, ins: &Inspected, installed: Option<&Listing>) -> Value {
    let (model, assumptions, refused) = match &ins.recognition {
        Recognition::Known(m) => (Some(m), Vec::new(), None),
        Recognition::Assumed { manifest, assumptions } => (Some(manifest), assumptions.clone(), None),
        Recognition::Unsupported(why) => (None, Vec::new(), Some(why.as_str())),
    };
    let known = matches!(ins.recognition, Recognition::Known(_));
    let origin = match &ins.recognition {
        Recognition::Known(m) if known::lookup(&ins.sha256).is_some_and(|k| k.id == m.id) => "builtin",
        Recognition::Known(_) => "catalog",
        _ => "local",
    };
    let is_installed =
        model.is_some_and(|m| installed.and_then(|l| l.installed(&m.id)).is_some_and(|i| i.manifest.sha256.as_deref() == Some(ins.sha256.as_str())));
    json!({
        "path": path,
        "fileName": ins.file_name,
        "sizeBytes": ins.size(),
        "sha256": ins.sha256,
        "known": known,
        "refused": refused,
        "assumptions": assumptions,
        "model": model.map(|m| model_facts(m, origin)),
        "installed": is_installed,
    })
}

/// Where the model file to install is now.
enum Staged<'a> {
    /// A local file of the user's: copied, never moved.
    Copy { dir: &'a Path, name: &'a str, identity: Identity },
    /// A download: moved out of [`DOWNLOADS`].
    Move { dir: &'a Path, name: &'a str, identity: Identity },
}

/// Puts the model file of `m` into `<models>/<id>/model.onnx` (never replacing another file) and
/// then writes its manifest; the file is given the time of the install, so the newest installed
/// recogniser is the one just added. Returns whether the same file was already there.
fn place(models: &Path, m: &ModelManifest, staged: Staged<'_>) -> Result<bool, String> {
    let s = |e: dalica_fs::Error| e.to_string();
    let sha = m.sha256.as_deref().ok_or("the model's manifest names no SHA-256")?;
    let dest = models.join(&m.id);
    dalica_fs::create_dir_all(&dest).map_err(s)?;
    let target = dest.join(MODEL_FILE);
    let (already, identity) = match dalica_fs::identity(&target).map_err(s)? {
        Some(identity) => {
            let have = hash::sha256_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            if have != sha {
                return Err(format!("another file is already installed as {}; remove that model first", m.id));
            }
            if let Staged::Move { dir, name, .. } = staged {
                dalica_fs::remove_shaped(&dir.join(name), |n| n == name).map_err(s)?;
            }
            (true, identity)
        }
        None => {
            let placed = match staged {
                Staged::Copy { dir, name, identity } => match transfer::copy_file(dir, name, identity, &dest, MODEL_FILE).map_err(s)? {
                    Placed::Done(id) => Some(id),
                    Placed::Exists => None,
                },
                Staged::Move { dir, name, identity } => match transfer::move_file(dir, name, identity, &dest, MODEL_FILE).map_err(s)? {
                    Placed::Done(Moved::Renamed(id) | Moved::Copied { identity: id, .. }) => Some(id),
                    Placed::Exists => None,
                },
            };
            let identity = placed.ok_or_else(|| format!("{} appeared during the install; nothing was replaced", target.display()))?;
            (false, identity)
        }
    };
    if !already {
        // A copy keeps its source's time; the install's time makes it the newest model.
        let times = Times { modified: SystemTime::now(), accessed: None };
        dalica_fs::set_times(&target, &times, identity).map_err(s)?;
    }
    write_manifest(&dest.join(MANIFEST_FILE), m)?;
    Ok(already)
}

/// Writes `m` as the manifest at `path`, replacing only a manifest just read.
fn write_manifest(path: &Path, m: &ModelManifest) -> Result<(), String> {
    let s = |e: dalica_fs::Error| e.to_string();
    let bytes = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
    let limit = u64::try_from(manifest::MAX_MANIFEST_BYTES).unwrap_or(u64::MAX);
    match dalica_fs::read(path, limit).map_err(s)? {
        Some(r) if r.bytes == bytes => Ok(()),
        Some(r) => dalica_fs::replace(path, &bytes, Expect::Unchanged(r.identity)).map(|_| ()).map_err(s),
        None => dalica_fs::replace(path, &bytes, Expect::Absent).map(|_| ()).map_err(s),
    }
}

/// The self-test's failure in words.
fn failed_test(t: &selftest::SelfTest) -> String {
    t.failure().unwrap_or("no check ran").to_string()
}

/// Installs the local file at `path` into `models` (the worker of `faces.models.install`).
fn install_file(path: &Path, models: &Path, extra: &[ModelManifest], cancel: &AtomicBool) -> Ended {
    let failed = |why: String| (Outcome::Failed(why), "Could not add the face model".to_string());
    let ins = match inspect_file(path, extra) {
        Ok(i) => i,
        Err(e) => return failed(e),
    };
    let (manifest, assumptions, known) = match &ins.recognition {
        Recognition::Known(m) => (m.clone(), Vec::new(), true),
        Recognition::Assumed { manifest, assumptions } => (manifest.clone(), assumptions.clone(), false),
        Recognition::Unsupported(why) => return failed(format!("this file cannot be used as a face model: {why}")),
    };
    let test = selftest::run(&ins.bytes, &manifest);
    if !test.ok {
        return failed(format!("the model failed its self-test ({}); nothing was installed", failed_test(&test)));
    }
    if cancel.load(Ordering::SeqCst) {
        return (Outcome::Cancelled(json!({ "installed": false })), "Adding the face model cancelled".into());
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return failed(format!("{} names no file Dalica can copy", path.display()));
    };
    match place(models, &manifest, Staged::Copy { dir, name, identity: ins.identity }) {
        Ok(already) => (
            Outcome::Done(json!({
                "id": manifest.id, "name": manifest.name, "role": role_name(manifest.role), "installed": true, "already": already,
                "known": known, "assumptions": assumptions, "sha256": ins.sha256, "selfTest": test,
            })),
            format!("Added {}", manifest.name),
        ),
        Err(e) => failed(e),
    }
}

/// What a download's worker needs.
struct Fetch {
    manifest: ModelManifest,
    spec: FileSpec,
    consent: Consent,
    models: PathBuf,
    transport: SharedTransport,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    log: NetworkLogLevel,
}

/// Downloads, checks, self-tests and installs one model (the worker of `faces.models.download`).
fn download_model(f: Fetch) -> Ended {
    let id = f.manifest.id.clone();
    let name = f.manifest.name.clone();
    let failed = |why: String| {
        log::warn!("face model download of {id} failed: {why}");
        (Outcome::Failed(why), format!("Could not download {name}"))
    };
    let staging = f.models.join(DOWNLOADS);
    if f.log != NetworkLogLevel::Errors {
        log::info!("face model download: {id} from {}", f.spec.url().display());
    }
    let mut progress = |p: &dalica_fetch::Progress| {
        f.done.store(p.done, Ordering::Relaxed);
        f.total.store(p.total.unwrap_or(0), Ordering::Relaxed);
    };
    let got = dalica_fetch::download(&f.spec, &f.consent, &staging, &*f.transport, &dalica_fetch::Options::default(), &f.cancel, &mut progress);
    let got = match got {
        Ok(d) => d,
        Err(FetchError::Cancelled) => {
            return (Outcome::Cancelled(json!({ "id": id, "installed": false })), format!("Download of {name} cancelled"));
        }
        Err(e) => return failed(e.to_string()),
    };
    let test = selftest::run_path(&got.path, &f.manifest);
    let staged = staged_name(&id);
    if !test.ok {
        let why = failed_test(&test);
        return match dalica_fs::remove_shaped(&got.path, |n| n == staged) {
            Ok(_) => failed(format!("the downloaded model failed its self-test ({why}); it was deleted")),
            Err(e) => failed(format!("the downloaded model failed its self-test ({why}) and could not be deleted: {e}")),
        };
    }
    let identity = match dalica_fs::identity(&got.path) {
        Ok(Some(i)) => i,
        Ok(None) => return failed(format!("{} vanished before it was installed", got.path.display())),
        Err(e) => return failed(e.to_string()),
    };
    match place(&f.models, &f.manifest, Staged::Move { dir: &staging, name: &staged, identity }) {
        Ok(already) => {
            if f.log != NetworkLogLevel::Errors {
                log::info!("face model {id} installed ({} bytes, resumed from {})", got.size, got.resumed_from);
            }
            (
                Outcome::Done(json!({
                    "id": id, "installed": true, "already": already, "sizeBytes": got.size, "sha256": got.sha256,
                    "resumedFrom": got.resumed_from, "fetched": got.fetched, "selfTest": test,
                })),
                format!("Downloaded {name}"),
            )
        }
        Err(e) => failed(e),
    }
}

/// Removes model `id` from `models` and its cached embeddings from the library directory `library`
/// (the worker of `faces.models.remove`).
fn remove_model(models: &Path, library: &Path, id: &str) -> Ended {
    let run = || -> Result<Value, String> {
        let s = |e: dalica_fs::Error| e.to_string();
        let dir = models.join(id);
        let mut files = 0u32;
        // The manifest first: a model without one is never used, even if its file stays.
        for name in [MANIFEST_FILE, MODEL_FILE] {
            if dalica_fs::remove_shaped(&dir.join(name), |n| n == name).map_err(s)? {
                files += 1;
            }
        }
        let staged = staged_name(id);
        let part = format!("{staged}.part");
        for name in [staged.as_str(), part.as_str()] {
            dalica_fs::remove_shaped(&models.join(DOWNLOADS).join(name), |n| n == name).map_err(s)?;
        }
        let cache = faces_cache::remove(library, id).map_err(|e| e.to_string())?;
        Ok(json!({ "id": id, "removed": files > 0, "embeddingsRemoved": cache }))
    };
    match run() {
        Ok(v) => (Outcome::Done(v), "Removed the face model".into()),
        Err(e) => (Outcome::Failed(e), "Could not remove the face model".into()),
    }
}

/// "12.3 of 38.7 MB".
fn megabytes(done: u64, total: u64) -> String {
    let mb = |b: u64| b as f64 / 1_000_000.0;
    if total > 0 { format!("{:.1} of {:.1} MB", mb(done), mb(total)) } else { format!("{:.1} MB", mb(done)) }
}

impl Session {
    /// The face models' state (`faces.md` §3).
    pub fn face_models(&self) -> &FaceModels {
        &self.face_models
    }

    /// The transport the face model downloads use from now on: `None` for the HTTPS transport
    /// through the proxy of Options ▸ Network (the default); tests give an in-memory one or one
    /// trusting a local server's root.
    pub fn set_face_model_transport(&mut self, transport: Option<SharedTransport>) {
        self.face_models.transport = transport;
    }

    /// The models folder: the scan's, or the one a removal took from it for a moment.
    fn models_folder(&self) -> Option<PathBuf> {
        let removing = self.face_models.tasks.iter().find_map(|t| match &t.kind {
            TaskKind::Remove { restore, .. } => Some(restore.clone()),
            _ => None,
        });
        removing.or_else(|| self.face_scan.models_dir().map(Path::to_path_buf))
    }

    /// The face models' turn: takes in what their workers finished, reports the download's
    /// progress, and reads the models folder again when it was given, changed or changed by a
    /// command. Never blocks or touches the disk; returns how many answers it took.
    pub fn poll_face_models(&mut self) -> usize {
        let mut n = 0;
        while let Some(msg) = self.face_models.recv() {
            n += 1;
            match msg {
                Msg::Looked(number, listing) => {
                    if self.face_models.looking == Some(number) {
                        self.face_models.looking = None;
                        self.face_models.listing = Some(Arc::new(*listing));
                    }
                }
                Msg::Done(job, ended) => self.face_model_task_done(job, ended),
            }
        }
        self.start_removals();
        if let Some(r) = &self.face_models.running {
            let (done, total) = (r.done.load(Ordering::Relaxed), r.total.load(Ordering::Relaxed));
            let label = format!("Downloading {}: {}", r.name, megabytes(done, total));
            self.jobs.progress(r.job, JobProgress { done, total }, label);
        }
        self.look_at_models();
        n
    }

    /// Starts reading the models folder when it was never read, is another one or changed.
    fn look_at_models(&mut self) {
        let Some(dir) = self.models_folder() else {
            self.face_models.listing = None;
            return;
        };
        let m = &mut self.face_models;
        let current = m.listing.as_ref().is_some_and(|l| l.folder == dir) && !m.stale;
        if current || m.looking.is_some() {
            return;
        }
        m.stale = false;
        m.next = m.next.wrapping_add(1);
        let number = m.next;
        m.looking = Some(number);
        let tx = m.sender();
        let spawned = std::thread::Builder::new().name("dalica-face-models".into()).spawn(move || {
            let listing = catch_unwind(AssertUnwindSafe(|| look(&dir))).unwrap_or_else(|_| Listing { folder: dir, ..Listing::default() });
            let _ = tx.send(Msg::Looked(number, Box::new(listing)));
        });
        if let Err(e) = spawned {
            log::warn!("face models: cannot read the models folder: {e}");
            self.face_models.looking = None;
        }
    }

    fn face_model_task_done(&mut self, job: JobId, (outcome, label): Ended) {
        let now = self.clock.now();
        let ok = matches!(outcome, Outcome::Done(_));
        self.jobs.finish(job, outcome, label, now);
        if self.face_models.running.as_ref().is_some_and(|r| r.job == job) {
            self.face_models.running = None;
            self.face_models.stale = true;
            if ok {
                self.face_models_changed();
            }
            return;
        }
        let Some(at) = self.face_models.tasks.iter().position(|t| t.job == job) else { return };
        let task = self.face_models.tasks.remove(at);
        match task.kind {
            TaskKind::Inspect => {}
            TaskKind::Install => {
                self.face_models.stale = true;
                if ok {
                    self.face_models_changed();
                }
            }
            TaskKind::Remove { id, restore, .. } => {
                self.face_models.stale = true;
                if self.settings().people().model.as_deref() == Some(id.as_str()) {
                    self.change_settings(|st| {
                        let mut people = st.people();
                        people.model = None;
                        st.set_people(&people);
                    });
                }
                self.set_face_models_dir(Some(restore));
            }
        }
    }

    /// Polls until the face models' workers are idle or `timeout` has passed; returns whether they
    /// got there. Blocks the calling thread (CLI, tests).
    pub fn wait_face_models(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll_face_models();
            if self.face_models.idle() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// The models folder read as it is now, waiting at most [`LOOK_WAIT`] for a worker to read it
    /// (only the first time, or after a change: the window's poll keeps it read); `refresh` reads
    /// it again first. `None` without a models folder.
    fn current_listing(&mut self, refresh: bool) -> Result<Option<Arc<Listing>>, CmdError> {
        if refresh {
            self.face_models.stale = true;
        }
        let until = Instant::now() + LOOK_WAIT;
        loop {
            self.poll_face_models();
            let Some(dir) = self.models_folder() else { return Ok(None) };
            let m = &self.face_models;
            if let Some(l) = m.listing.as_ref().filter(|l| l.folder == dir && !m.stale && m.looking.is_none()) {
                return Ok(Some(l.clone()));
            }
            if Instant::now() >= until {
                return Err(CmdError::Disabled("The models folder is still being read; try again.".into()));
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// The models folder for a command that writes to it: given, outside every watched folder, in a
    /// session that writes.
    fn writable_models_folder(&self) -> Result<PathBuf, CmdError> {
        if self.is_read_only() {
            return Err(CmdError::ReadOnly("face models are not added or removed in a read-only session (--read-only)".into()));
        }
        let dir = self
            .models_folder()
            .ok_or_else(|| CmdError::Disabled("This session has no face models folder (DALICA_FACE_MODELS, or the configuration folder).".into()))?;
        if let Some(root) = self.roots().iter().find(|r| dir.starts_with(r)) {
            return Err(CmdError::Disabled(format!(
                "The face models folder {} is inside the watched folder {}; set DALICA_FACE_MODELS to a folder outside the library.",
                dir.display(),
                root.display()
            )));
        }
        Ok(dir)
    }

    /// `faces.models.list`.
    pub(crate) fn list_face_models(&mut self, refresh: bool) -> Result<Value, CmdError> {
        let listing = self.current_listing(refresh)?;
        let selected = self.settings().people().model;
        let installed = listing.as_ref().map(|l| l.installed.as_slice()).unwrap_or_default();
        let used = in_use(installed, selected.as_deref()).map(str::to_string);
        let downloading = self.face_models.running.as_ref().map(|r| r.id.clone());
        let mut models = Vec::new();
        let mut listed = Vec::new();
        let state = |id: &str| -> (&'static str, Option<u64>) {
            let partial = listing.as_ref().and_then(|l| l.partial.iter().find(|(p, _)| p == id).map(|(_, n)| *n));
            if listing.as_ref().is_some_and(|l| l.installed(id).is_some()) {
                ("installed", None)
            } else if downloading.as_deref() == Some(id) {
                ("downloading", partial)
            } else if partial.is_some() {
                ("partial", partial)
            } else {
                ("available", None)
            }
        };
        for o in offers(listing.as_deref()) {
            let (st, partial) = state(&o.manifest.id);
            let mut v = model_facts(&o.manifest, o.origin);
            if let Some(obj) = v.as_object_mut() {
                obj.insert("sizeBytes".into(), json!(o.download.size_bytes));
                obj.insert("site".into(), json!(o.download.host()));
                obj.insert("state".into(), json!(st));
                obj.insert("partialBytes".into(), json!(partial));
                obj.insert("inUse".into(), json!(used.as_deref() == Some(o.manifest.id.as_str())));
                obj.insert("default".into(), json!(o.manifest.id == known::DEFAULT_RECOGNISER));
            }
            listed.push(o.manifest.id.clone());
            models.push(v);
        }
        for i in installed.iter().filter(|i| !listed.contains(&i.manifest.id)) {
            let mut v = model_facts(&i.manifest, "local");
            if let Some(obj) = v.as_object_mut() {
                obj.insert("state".into(), json!("installed"));
                obj.insert("partialBytes".into(), Value::Null);
                obj.insert("inUse".into(), json!(used.as_deref() == Some(i.manifest.id.as_str())));
                obj.insert("default".into(), json!(false));
            }
            models.push(v);
        }
        let problem = self.writable_models_folder().err().map(|e| e.to_string());
        let download = self
            .face_models
            .running
            .as_ref()
            .map(|r| json!({ "id": r.id, "job": r.job, "done": r.done.load(Ordering::Relaxed), "total": r.total.load(Ordering::Relaxed) }));
        Ok(json!({
            "folder": listing.as_ref().map(|l| l.folder.clone()),
            "problem": problem,
            "selected": selected,
            "inUse": used,
            "loaded": self.faces_index.model(),
            "download": download,
            "catalogErrors": listing.as_ref().map(|l| l.catalog.errors.clone()).unwrap_or_default(),
            "commercialNote": COMMERCIAL_NOTE,
            "models": models,
        }))
    }

    /// An absolute path of a model file, from a command's params.
    fn model_path(path: &Path) -> Result<(), CmdError> {
        if !path.is_absolute() || path.as_os_str().len() > crate::automation::MAX_PATH || path.file_name().is_none() {
            return Err(CmdError::BadParams(format!("path: an absolute path of a model file, not {}", path.display())));
        }
        Ok(())
    }

    fn start_task(
        &mut self,
        kind: JobKind,
        label: &str,
        task: TaskKind,
        work: impl FnOnce(&AtomicBool) -> Ended + Send + 'static,
    ) -> Result<JobId, CmdError> {
        let now = self.clock.now();
        let (job, cancel) = self.jobs.start(kind, label, true, now);
        let tx = self.face_models.sender();
        if let Err(e) = spawn("dalica-face-models", tx, job, move || work(&cancel)) {
            self.jobs.finish(job, Outcome::Failed(e.clone()), "Face model task failed", now);
            return Err(CmdError::Limit(e));
        }
        self.face_models.tasks.push(Task { job, kind: task });
        Ok(job)
    }

    /// `faces.models.inspect {path}`: a job whose result says what the file is.
    pub(crate) fn inspect_face_model(&mut self, path: PathBuf) -> Result<Value, CmdError> {
        Self::model_path(&path)?;
        let running = self.face_models.tasks.iter().filter(|t| matches!(t.kind, TaskKind::Inspect)).count();
        if running >= MAX_INSPECTIONS {
            return Err(CmdError::Limit(format!("at most {MAX_INSPECTIONS} model files are looked at once")));
        }
        let listing = match self.models_folder() {
            Some(_) => self.current_listing(false)?,
            None => None,
        };
        let extra = listing.as_ref().map(|l| l.catalog.manifests()).unwrap_or_default();
        let job = self.start_task(JobKind::Faces, "Looking at a face model file", TaskKind::Inspect, move |_| match inspect_file(&path, &extra) {
            Ok(ins) => (Outcome::Done(inspection_value(&path, &ins, listing.as_deref())), "Looked at the face model file".into()),
            Err(e) => (Outcome::Failed(e), "Could not read the face model file".into()),
        })?;
        Ok(json!({ "job": job, "pending": true }))
    }

    fn changing_models(&self) -> Result<(), CmdError> {
        if self.face_models.tasks.iter().any(|t| matches!(t.kind, TaskKind::Install | TaskKind::Remove { .. })) {
            return Err(CmdError::Limit("another face model is being added or removed; wait for it to finish".into()));
        }
        Ok(())
    }

    /// `faces.models.install {path, acknowledged}`.
    pub(crate) fn install_face_model(&mut self, path: PathBuf, acknowledged: bool) -> Result<Value, CmdError> {
        if !acknowledged {
            return Err(CmdError::Disabled(NEEDS_CONSENT.into()));
        }
        Self::model_path(&path)?;
        let models = self.writable_models_folder()?;
        self.changing_models()?;
        let extra = self.current_listing(false)?.map(|l| l.catalog.manifests()).unwrap_or_default();
        let job =
            self.start_task(JobKind::Faces, "Adding a face model", TaskKind::Install, move |cancel| install_file(&path, &models, &extra, cancel))?;
        Ok(json!({ "job": job, "pending": true }))
    }

    /// `faces.models.download {id, acknowledged}`.
    pub(crate) fn download_face_model(&mut self, id: &str, acknowledged: bool) -> Result<Value, CmdError> {
        if !acknowledged {
            return Err(CmdError::Disabled(NEEDS_CONSENT.into()));
        }
        if !manifest::valid_id(id) {
            return Err(CmdError::BadParams(format!("id: a model id of faces.models.list, not {:?}", id.chars().take(80).collect::<String>())));
        }
        let models = self.writable_models_folder()?;
        let listing = self.current_listing(false)?;
        let offer = offers(listing.as_deref())
            .into_iter()
            .find(|o| o.manifest.id == id)
            .ok_or_else(|| CmdError::NotFound(format!("no face model {id} can be downloaded (faces.models.list)")))?;
        if let Some(r) = &self.face_models.running {
            if r.id == id {
                return Ok(json!({ "id": id, "job": r.job, "pending": true }));
            }
            return Err(CmdError::Limit(format!("{} is being downloaded; one download at a time", r.name)));
        }
        let installed = listing.as_ref().and_then(|l| l.installed(id));
        if installed.is_some_and(|i| i.manifest.sha256.as_deref() == Some(offer.download.sha256.as_str())) {
            return Ok(json!({ "id": id, "job": null, "pending": false, "installed": true, "already": true }));
        }
        if installed.is_some() {
            return Err(CmdError::Disabled(format!("another file is installed as {id}; remove that model first")));
        }
        let d = &offer.download;
        let spec = FileSpec::new(&d.url, &staged_name(id), d.size_bytes, Some(d.size_bytes), Some(&d.sha256))
            .map_err(|e| CmdError::BadParams(format!("the model's address cannot be downloaded: {e}")))?;
        let consent = Consent::acknowledged(&spec, acknowledged).map_err(|e| CmdError::Disabled(e.to_string()))?;
        let transport = self.face_model_transport()?;
        let now = self.clock.now();
        let name = offer.manifest.name.clone();
        let (job, cancel) = self.jobs.start(JobKind::Network, format!("Downloading {name}"), true, now);
        let (done, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(d.size_bytes)));
        let fetch = Fetch {
            manifest: offer.manifest,
            spec,
            consent,
            models,
            transport,
            cancel: cancel.clone(),
            done: done.clone(),
            total: total.clone(),
            log: self.settings().network().log_level,
        };
        let tx = self.face_models.sender();
        if let Err(e) = spawn("dalica-face-download", tx, job, move || download_model(fetch)) {
            self.jobs.finish(job, Outcome::Failed(e.clone()), "Face model download failed", now);
            return Err(CmdError::Limit(e));
        }
        self.face_models.download = Some(job);
        self.face_models.running = Some(Running { job, id: id.to_string(), name, done, total, cancel });
        Ok(json!({ "id": id, "job": job, "pending": true }))
    }

    /// The transport for a download: the one given, else HTTPS through the proxy of Options ▸
    /// Network.
    fn face_model_transport(&self) -> Result<SharedTransport, CmdError> {
        if let Some(t) = &self.face_models.transport {
            return Ok(t.clone());
        }
        let net = self.settings().network();
        let proxy = match net.proxy {
            ProxyMode::System => Proxy::system(),
            ProxyMode::Manual => Proxy::manual(&net.proxy_host, net.proxy_port),
        }
        .map_err(|e| CmdError::Disabled(e.to_string()))?;
        let https = HttpsTransport::new(proxy).map_err(|e| CmdError::Disabled(format!("downloads cannot start: {e}")))?;
        Ok(Arc::new(https))
    }

    /// `faces.models.cancel {id}`.
    pub(crate) fn cancel_face_model(&mut self, id: &str) -> Result<Value, CmdError> {
        let job = match &self.face_models.running {
            Some(r) if r.id == id => r.job,
            _ => return Err(CmdError::NotFound(format!("no download of {:?} is running", id.chars().take(80).collect::<String>()))),
        };
        self.cancel_jobs(Some(job))?;
        Ok(json!({ "id": id, "job": job, "cancelled": true }))
    }

    /// `faces.models.remove {id}`.
    pub(crate) fn remove_face_model(&mut self, id: &str) -> Result<Value, CmdError> {
        if !manifest::valid_id(id) {
            return Err(CmdError::BadParams(format!("id: a model id of faces.models.list, not {:?}", id.chars().take(80).collect::<String>())));
        }
        let models = self.writable_models_folder()?;
        self.changing_models()?;
        if self.face_models.running.as_ref().is_some_and(|r| r.id == id) {
            return Err(CmdError::Limit(format!("{id} is being downloaded; cancel the download first")));
        }
        let listing = self.current_listing(false)?;
        let installed =
            listing.as_ref().and_then(|l| l.installed(id)).ok_or_else(|| CmdError::NotFound(format!("no face model {id} is installed")))?;
        let recogniser = installed.manifest.role == Role::Embedder;
        // The scan lets go of the model (its file and its cache) while it is removed, and is given
        // the folder back when the removal ends.
        self.set_face_models_dir(None);
        self.poll_face_scan();
        if recogniser && self.faces_index.model() == Some(id) {
            self.faces_index.reset(None);
        }
        let now = self.clock.now();
        let (job, _) = self.jobs.start(JobKind::Faces, "Removing a face model", false, now);
        self.face_models.tasks.push(Task { job, kind: TaskKind::Remove { id: id.to_string(), restore: models, waiting: true } });
        self.start_removals();
        Ok(json!({ "id": id, "job": job, "pending": true }))
    }

    /// Starts the removals waiting, once the scan holds nothing of any model (it was given no
    /// folder, so it loads nothing new).
    fn start_removals(&mut self) {
        if !self.face_scan.is_idle() {
            return;
        }
        let library = self.dir.clone();
        let mut starts = Vec::new();
        for t in &mut self.face_models.tasks {
            if let TaskKind::Remove { id, restore, waiting } = &mut t.kind
                && *waiting
            {
                *waiting = false;
                starts.push((t.job, id.clone(), restore.clone()));
            }
        }
        for (job, id, models) in starts {
            let tx = self.face_models.sender();
            let library = library.clone();
            if let Err(e) = spawn("dalica-face-models", tx, job, move || remove_model(&models, &library, &id)) {
                // The answer never comes: end the job and give the scan its folder back now.
                let ended = (Outcome::Failed(e), "Could not remove the face model".to_string());
                self.face_model_task_done(job, ended);
            }
        }
    }

    /// `faces.models.select {id}`: the recogniser to use (`None`: the newest installed).
    pub(crate) fn select_face_model(&mut self, id: Option<String>) -> Result<Value, CmdError> {
        if let Some(id) = &id {
            if !manifest::valid_id(id) {
                return Err(CmdError::BadParams(format!("id: a model id of faces.models.list, not {:?}", id.chars().take(80).collect::<String>())));
            }
            let listing = self.current_listing(false)?;
            let model =
                listing.as_ref().and_then(|l| l.installed(id)).ok_or_else(|| CmdError::NotFound(format!("no face model {id} is installed")))?;
            if model.manifest.role != Role::Embedder {
                return Err(CmdError::BadParams(format!("{id} is a face detector; only a recogniser is chosen with Use")));
            }
        }
        self.change_settings(|st| {
            let mut people = st.people();
            people.model = id.clone();
            st.set_people(&people);
        });
        self.face_models_changed();
        self.poll_face_scan();
        Ok(json!({ "selected": id }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_downloads_are_named_by_their_model() {
        assert_eq!(partial_id("sface-2021dec.onnx.part"), Some("sface-2021dec"));
        for bad in ["sface.onnx", ".x.onnx.part", "A.onnx.part", "x.part", ".onnx.part"] {
            assert_eq!(partial_id(bad), None, "{bad}");
        }
        assert_eq!(staged_name("yunet-2023mar"), "yunet-2023mar.onnx");
    }

    #[test]
    fn sizes_read_in_megabytes() {
        assert_eq!(megabytes(12_300_000, 38_696_353), "12.3 of 38.7 MB");
        assert_eq!(megabytes(5, 0), "0.0 MB");
    }

    #[test]
    fn the_site_is_the_host_of_the_page() {
        assert_eq!(host_of("https://huggingface.co/fal/AuraFace-v1"), Some("huggingface.co"));
        assert_eq!(host_of("http://x"), None);
        assert_eq!(host_of("https://"), None);
    }
}
