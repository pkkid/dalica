//! What the engine's integration tests share: the quick/full switch, the lock around process-wide
//! fault points, sessions opened the usual way, small photos, and file reads made off the owner
//! thread (the owner thread's own file access is counted, so the tests read on a helper thread).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use dalica_core::{Clock, PhotoId, SystemClock, Timestamp, VirtualClock};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::Value;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// `quick` in a task's gate, `full` at the review gate.
pub fn pick<T>(quick: T, full: T) -> T {
    if self::full() { full } else { quick }
}

// --- Process-wide fault points ----------------------------------------------------------------------

static FAULTS: RwLock<()> = RwLock::new(());

thread_local! {
    /// This thread already holds [`FAULTS`] (one guard per thread; the others nest).
    static HELD: Cell<bool> = const { Cell::new(false) };
}

enum Held {
    Shared(#[allow(dead_code)] RwLockReadGuard<'static, ()>),
    Exclusive(#[allow(dead_code)] RwLockWriteGuard<'static, ()>),
    Nested,
}

/// A hold on the process-wide fault points ([`exclusive`] or [`shared`]); released when dropped.
pub struct FaultLock(Held);

impl Drop for FaultLock {
    fn drop(&mut self) {
        if !matches!(self.0, Held::Nested) {
            HELD.with(|h| h.set(false));
        }
    }
}

/// For a test that arms a fault point for every thread (`faults::arm_global`): no other test that
/// moves, copies or trashes a file (or disarms the points) runs meanwhile. Every point is disarmed
/// when it is taken. Take it before anything that takes [`shared`] (a [`Lib`]-style harness).
pub fn exclusive() -> FaultLock {
    if HELD.with(Cell::get) {
        return FaultLock(Held::Nested);
    }
    let guard = FAULTS.write().unwrap_or_else(PoisonError::into_inner);
    HELD.with(|h| h.set(true));
    dalica_fs::faults::disarm_all();
    FaultLock(Held::Exclusive(guard))
}

/// For a test whose jobs move, copy or trash files (a point armed for every thread would hit them),
/// or that calls `faults::disarm_all` (which clears the process-wide points too). Nests inside
/// [`exclusive`] on the same thread.
pub fn shared() -> FaultLock {
    if HELD.with(Cell::get) {
        return FaultLock(Held::Nested);
    }
    let guard = FAULTS.read().unwrap_or_else(PoisonError::into_inner);
    HELD.with(|h| h.set(true));
    FaultLock(Held::Shared(guard))
}

// --- Sessions -------------------------------------------------------------------------------------------

/// No thumbnail cache, one thumbnail worker.
pub fn options() -> Options {
    Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } }
}

/// A virtual clock fixed at 1 790 000 000 000 ms since the epoch.
pub fn virtual_clock() -> Arc<VirtualClock> {
    Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)))
}

/// `library` opened on `clock` with [`options`], its first scan done.
pub fn open_with(library: &Path, clock: Arc<dyn Clock>) -> Session {
    let mut s = Session::open_with(library, clock, &options()).unwrap();
    s.wait_idle();
    s
}

/// `library` opened on the system clock with [`options`], its first scan done.
pub fn open(library: &Path) -> Session {
    open_with(library, Arc::new(SystemClock))
}

// --- Photos and files --------------------------------------------------------------------------------

/// A 96 × 64 JPEG of scene `seed`.
pub fn jpeg(seed: u64) -> Vec<u8> {
    jpeg_of(seed, 96, 64)
}

/// A `width × height` JPEG of scene `seed` (quality 80).
pub fn jpeg_of(seed: u64, width: u32, height: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, width, height), 80, None).unwrap()
}

/// Runs `f` on a helper thread: the test's own file access never counts against the owner.
pub fn off_owner<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

/// Whether `path` exists (read off the owner thread).
pub fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    off_owner(move || path.exists())
}

/// The bytes of `path` (read off the owner thread).
pub fn read(path: &Path) -> Vec<u8> {
    read_opt(path).unwrap_or_else(|| panic!("{} is missing", path.display()))
}

/// The bytes of `path`, `None` when it cannot be read (read off the owner thread).
pub fn read_opt(path: &Path) -> Option<Vec<u8>> {
    let path = path.to_path_buf();
    off_owner(move || std::fs::read(path).ok())
}

/// The JSON in `path`, `None` when there is no file (read off the owner thread).
pub fn read_json(path: &Path) -> Option<Value> {
    read_opt(path).map(|b| serde_json::from_slice(&b).unwrap())
}

/// The sidecar entry for `name` in `folder`.
pub fn entry(folder: &Path, name: &str) -> Option<Value> {
    read_json(&folder.join(".dalica.json"))?.get("photos")?.get(name).cloned()
}

/// The names in `folder` (Dalica's own files left out), sorted (read off the owner thread).
pub fn files(folder: &Path) -> Vec<String> {
    let mut v: Vec<String> = names(folder).into_iter().filter(|n| !n.starts_with(".dalica")).collect();
    v.sort();
    v
}

/// Every name in `folder`, sorted; none when it is missing (read off the owner thread).
pub fn names(folder: &Path) -> Vec<String> {
    let folder = folder.to_path_buf();
    let mut v: Vec<String> = off_owner(move || match std::fs::read_dir(folder) {
        Ok(entries) => entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    });
    v.sort();
    v
}

/// `root/<rel>` for `"Trips/Porto"`.
pub fn under(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|c| !c.is_empty()).fold(root.to_path_buf(), |p, c| p.join(c))
}

/// A library watching `<disk>/Lisbon`, which holds `photos` as `IMG_0001.jpg`, `IMG_0002.jpg` … and,
/// when given, `sidecar` as its `.dalica.json`, scanned: the library's folder, the disk's, the
/// `Lisbon` folder, the session and the photos' ids in name order.
pub fn lisbon(photos: &[Vec<u8>], sidecar: Option<&Value>) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for (i, bytes) in photos.iter().enumerate() {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes, Expect::Absent).unwrap();
    }
    if let Some(sidecar) = sidecar {
        dalica_fs::replace(&folder.join(".dalica.json"), serde_json::to_vec_pretty(sidecar).unwrap().as_slice(), Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (lib, disk, folder, s, ids.into_iter().map(|(_, id)| id).collect())
}

/// Waits for the session's file jobs, then for a check of its watched folders made after them.
///
/// A check sent while a job renamed, moved or emptied a watched folder can find that folder gone
/// (or empty) and hold its sidecar writes as offline until the next check answers, so a flush
/// right after the job reports them unsaved. That race is the product's (the writes are delayed,
/// never lost: `rename.rs` `a_check_that_saw_a_renamed_watched_folder_gone_only_delays_its_writes`);
/// the file-operation tests wait for the next check before they flush.
pub fn wait_files(s: &mut Session) {
    let patience = std::time::Duration::from_secs(120);
    assert!(s.wait_file_jobs(patience), "the file jobs ended");
    assert!(s.refresh_volumes(patience), "the watched folders were checked");
}
