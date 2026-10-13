//! M5 part 2, *Folders and scanning*: temp-dir scenarios while the session runs, seen through the
//! window as a user sees them (the headless driver: tree rows, light-table cells, the tree's
//! offline marks) and through the library as an agent reads it.
//!
//! - **The watcher** (the polling backend on the session's virtual clock, so every step is
//!   deterministic on every OS): files added, removed, renamed (keeping id and star) and rewritten;
//!   folders added, removed, renamed and moved (keeping ids and stars).
//! - **The verification pass alone**, with the watcher off: nothing changes until it is due; then
//!   it finds every change.
//! - **A volume "unplugged"** by renaming its root: its rows stay, dimmed "offline", with their
//!   photos; a star made meanwhile is journalled and waits (no folder is made at the old path);
//!   when the root returns the sidecar holds the star.
//! - **An emptied mount point** (the root's files gone, an empty folder left at its path: M5.17's
//!   fix): offline too, and no sidecar is written into it; when the files are back the star is.
//! - **First run on a fresh library** (`m5.md`'s manual check, automated): the window offers the
//!   first-run panel, Continue adds the standard places (planted under a temp home), folders and
//!   their thumbnails fill in while the scan still runs, then everything.
//!
//! The owner thread is marked as the UI thread while the window runs: it makes no `fs` call. The
//! test's own file changes run on helper threads.
//!
//! **Changes outside Dalica wait for a quiet session** ([`Fx::outside`]). Windows refuses to
//! rename or remove a folder while any handle under it is open ("Access is denied", os error 5),
//! and the session has workers the frames do not wait for: the online check of the roots, which
//! `poll` sends after every scan or rescan ends and which lists each root and looks for sample
//! photos in its folders, and the I/O worker, which writes a sidecar once its debounce has passed
//! on the session clock and first looks at the root it writes under. Either one can still be at
//! work after `tick` has settled, holding a handle under the folder the scenario renames next
//! (M7.28). So each change first waits for them (the check answered, every write landed), and the
//! root checks, counted, prove that none ran during the change.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::offline::{CHECK_EVERY, OnlineCheck, RootProbe, SETTLE};
use dalica_engine::watching::{FULL_VERIFICATION, WatchMode};
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_scan::{HeaderReader, Places};
use dalica_ui::panels::folders;
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(60);
const FRAMES: usize = 60 * 60;

// ---------------------------------------------------------------------------------------------
// Files changed outside Dalica (on helper threads)
// ---------------------------------------------------------------------------------------------

fn off_owner<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

fn jpeg(seed: u64, w: u32, h: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, w, h), 85, None).unwrap()
}

fn put(path: &Path, seed: u64) {
    let path = path.to_path_buf();
    off_owner(move || {
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        let expect = dalica_fs::identity(&path).unwrap().map_or(Expect::Absent, Expect::Unchanged);
        dalica_fs::replace(&path, &jpeg(seed, 160, 120), expect).unwrap();
    });
}

fn rename(from: &Path, to: &Path) {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    off_owner(move || {
        dalica_fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(from, to).unwrap();
    });
}

fn remove(path: &Path) {
    let path = path.to_path_buf();
    off_owner(move || if path.is_dir() { std::fs::remove_dir_all(path).unwrap() } else { std::fs::remove_file(path).unwrap() });
}

fn mkdir(path: &Path) {
    let path = path.to_path_buf();
    off_owner(move || std::fs::create_dir(path).unwrap());
}

fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    off_owner(move || path.exists())
}

fn sidecar(folder: &Path) -> Value {
    let path = folder.join(".dalica.json");
    let bytes = off_owner(move || std::fs::read(path).unwrap());
    serde_json::from_slice(&bytes).unwrap()
}

/// The session's looks at the watched roots (the online checks, and the I/O worker's check right
/// before a sidecar write), counted: begun, and still running.
#[derive(Default)]
struct Looks {
    begun: AtomicU64,
    running: AtomicU64,
}

impl Looks {
    fn count<T>(&self, look: impl FnOnce() -> T) -> T {
        self.begun.fetch_add(1, Ordering::SeqCst);
        self.running.fetch_add(1, Ordering::SeqCst);
        let out = look();
        self.running.fetch_sub(1, Ordering::SeqCst);
        out
    }

    fn now(&self) -> (u64, u64) {
        (self.begun.load(Ordering::SeqCst), self.running.load(Ordering::SeqCst))
    }
}

// ---------------------------------------------------------------------------------------------
// The window on a session with a virtual clock
// ---------------------------------------------------------------------------------------------

/// A temp home with `Pictures` (Lisbon: 3 photos, Porto: 2) and a "volume" `CARD` (one photo at
/// its root, Trip: 2), both watched; the window at 1400 × 900.
struct Fx {
    _tmp: tempfile::TempDir,
    pictures: PathBuf,
    card: PathBuf,
    outside: PathBuf,
    clock: Arc<VirtualClock>,
    looks: Arc<Looks>,
    d: Driver,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let pictures = tmp.path().join("Pictures");
        let card = tmp.path().join("Volumes").join("CARD");
        for i in 1..=3 {
            put(&pictures.join("Lisbon").join(format!("IMG_{i:04}.jpg")), i);
        }
        for i in 1..=2 {
            put(&pictures.join("Porto").join(format!("IMG_{i:04}.jpg")), 10 + i);
        }
        put(&card.join("DSC_0001.jpg"), 31);
        for i in 2..=3 {
            put(&card.join("Trip").join(format!("DSC_{i:04}.jpg")), 30 + i);
        }
        // Far in the past, so a day of virtual time is still before the files' own times.
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&tmp.path().join("library"), clock.clone(), &options).unwrap();
        // The default checks, counted.
        let looks = Arc::new(Looks::default());
        let check: OnlineCheck = Arc::new({
            let looks = Arc::clone(&looks);
            move |root: &Path| looks.count(|| dalica_scan::volumes::online(root))
        });
        let probe: RootProbe = Arc::new({
            let looks = Arc::clone(&looks);
            move |root: &Path, samples: &[PathBuf]| looks.count(|| dalica_scan::volumes::root_state(root, samples))
        });
        s.set_online_check(Some(check));
        s.set_root_probe(Some(probe));
        s.set_watching(WatchMode::Polling);
        for root in [&pictures, &card] {
            s.execute("folders.add", &json!({ "path": root })).unwrap();
        }
        s.wait_idle();
        assert!(s.refresh_volumes(PATIENCE));
        let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
        d.settle(FRAMES).unwrap();
        Fx { outside: tmp.path().join("Elsewhere"), pictures, card, clock, looks, d, _tmp: tmp }
    }

    /// Changes the folders outside Dalica, on a helper thread, once the session is quiet: no
    /// worker of its own holds a handle in them (see the module notes). Asserts that none of the
    /// session's root checks ran during the change.
    fn outside(&mut self, change: impl FnOnce() + Send) {
        self.quiet();
        let before = self.looks.now();
        assert_eq!(before.1, 0, "no root check runs once the session is quiet");
        off_owner(change);
        assert_eq!(self.looks.now(), before, "no root check ran while the folders changed");
    }

    /// Waits for the session's workers that the frames do not wait for: scans, rescans and
    /// thumbnails; the online check in flight (and one more, made on the disk as the session last
    /// saw it, so it changes nothing); the metadata writes; then every sidecar write that is
    /// waiting, so the I/O worker has nothing due until the clock moves. A write under a root
    /// found offline cannot land and is the scenario's to check: only then may the flush report
    /// it unsaved.
    fn quiet(&mut self) {
        let s = self.s_mut();
        s.wait_idle();
        assert!(s.refresh_volumes(PATIENCE), "the volume check answered");
        assert!(s.flush_metadata(PATIENCE), "the metadata writes landed");
        match s.flush() {
            Ok(()) => {}
            Err(CmdError::Unsaved(why)) => {
                assert!(!s.offline_roots().is_empty(), "only a write under an offline root waits: {why}")
            }
            Err(e) => panic!("flush: {e}"),
        }
    }

    fn s(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    fn s_mut(&mut self) -> &mut Session {
        self.d.app_mut().session_mut().unwrap()
    }

    /// The polling watcher on virtual time: a few one-second ticks, each waited for, what they
    /// reported rescanned and applied, then the window's frames until it is quiet.
    fn tick(&mut self) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(self.s_mut().wait_watching(PATIENCE), "the watcher answered");
            self.s_mut().wait_idle();
            self.d.step();
        }
        self.d.settle(FRAMES).unwrap();
    }

    /// The next online check, waited for, and frames until the window shows it.
    fn check_volumes(&mut self) {
        self.clock.advance(CHECK_EVERY);
        assert!(self.s_mut().refresh_volumes(PATIENCE), "the volume check answered");
        self.d.settle(FRAMES).unwrap();
    }

    fn photo(&self, folder: &Path, name: &str) -> Option<PhotoId> {
        let lib = self.s().library();
        let f = lib.folder_by_path(folder)?;
        lib.photos_in(f).into_iter().find(|p| p.name == name).map(|p| p.id)
    }

    fn star(&self, id: PhotoId) -> bool {
        self.s().library().photo(id).is_some_and(|p| p.star)
    }

    /// Every (folder relative to the temp dir, photo name) in the library, sorted.
    fn listing(&self) -> Vec<String> {
        let base = self.pictures.parent().unwrap();
        let lib = self.s().library();
        let mut v: Vec<String> = lib
            .photos()
            .map(|p| {
                let folder = &lib.folder(p.folder).unwrap().path;
                let rel: Vec<String> =
                    folder.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                format!("{}/{}", rel.join("/"), p.name)
            })
            .collect();
        v.sort();
        v
    }

    /// The tree's folder rows as the window lists them (`Lisbon (3)`), sorted.
    fn rows(&self) -> Vec<String> {
        let mut v: Vec<String> = self.d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).map(|w| w.label).collect();
        v.sort();
        v
    }

    /// The row labels the pane's model marks offline, sorted.
    fn offline_rows(&self) -> Vec<String> {
        let model = folders::current_model(self.d.ctx());
        let mut v: Vec<String> =
            (0..model.items.len()).filter_map(|i| model.row(i)).filter(|r| r.offline).map(|r| format!("{} ({})", r.name, r.count)).collect();
        v.sort();
        v
    }

    /// Clicks `folder`'s tree row: its section's header eases to the top of the light table.
    fn show(&mut self, folder: &Path) {
        let id = self.s().library().folder_by_path(folder).unwrap_or_else(|| panic!("no folder {}", folder.display()));
        self.d.click(&format!("tree:folder:{id}")).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    /// Whether the light table draws a cell for `id`.
    fn cell(&self, id: PhotoId) -> bool {
        let want = format!("thumb:{id}");
        self.d.widgets().iter().any(|w| w.id == want)
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// What each phase took, for the log.
pub struct Timings {
    pub phases: Vec<(&'static str, f64)>,
}

/// The temp-dir scenarios, one after another on one running session.
pub fn the_library_follows_its_folders() -> Timings {
    let mut t = Timings { phases: Vec::new() };
    let mut started = Instant::now();
    let mut phase = |t: &mut Timings, name: &'static str| {
        t.phases.push((name, started.elapsed().as_secs_f64()));
        started = Instant::now();
    };
    let mut fx = Fx::new();
    let ui = dalica_fs::mark_ui_thread();
    let lisbon = fx.pictures.join("Lisbon");
    let porto = fx.pictures.join("Porto");
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Lisbon (3)", "Porto (2)", "Trip (2)"]), "the tree after the first scan");
    let starred = fx.photo(&lisbon, "IMG_0001.jpg").unwrap();
    let renamed = fx.photo(&lisbon, "IMG_0002.jpg").unwrap();
    let rewritten = fx.photo(&lisbon, "IMG_0003.jpg").unwrap();
    fx.d.run("photo.star.set", &json!({ "ids": [starred, renamed], "on": true })).unwrap();
    fx.tick();
    assert!(fx.s().watched_roots().len() == 2, "both roots watched: {:?}", fx.s().watched_roots());
    phase(&mut t, "open and scan");

    // 1. Files, through the watcher: added, removed, renamed, rewritten.
    let size = fx.s().library().photo(rewritten).unwrap().size;
    fx.outside(|| {
        put(&lisbon.join("IMG_0004.jpg"), 4);
        remove(&porto.join("IMG_0002.jpg"));
        rename(&lisbon.join("IMG_0002.jpg"), &lisbon.join("Tram.jpg"));
        let p = lisbon.join("IMG_0003.jpg");
        let id = dalica_fs::identity(&p).unwrap().unwrap();
        dalica_fs::replace(&p, &jpeg(99, 320, 240), Expect::Unchanged(id)).unwrap();
    });
    fx.tick();
    assert_eq!(
        fx.listing(),
        strs(&[
            "Pictures/Lisbon/IMG_0001.jpg",
            "Pictures/Lisbon/IMG_0003.jpg",
            "Pictures/Lisbon/IMG_0004.jpg",
            "Pictures/Lisbon/Tram.jpg",
            "Pictures/Porto/IMG_0001.jpg",
            "Volumes/CARD/DSC_0001.jpg",
            "Volumes/CARD/Trip/DSC_0002.jpg",
            "Volumes/CARD/Trip/DSC_0003.jpg",
        ])
    );
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Lisbon (4)", "Porto (1)", "Trip (2)"]));
    let added = fx.photo(&lisbon, "IMG_0004.jpg").unwrap();
    fx.show(&lisbon);
    assert!(fx.cell(added), "the added photo has its cell");
    assert_eq!(fx.photo(&lisbon, "Tram.jpg"), Some(renamed), "a renamed file keeps its id");
    assert!(fx.star(renamed), "and its star");
    let now = fx.s().library().photo(rewritten).unwrap();
    assert!(now.size != size && now.width == 320, "a rewritten file is read again");
    phase(&mut t, "files through the watcher");

    // 2. Folders, through the watcher: added, removed, renamed, moved.
    let pictures = fx.pictures.clone();
    fx.outside(|| put(&pictures.join("Sintra").join("IMG_0001.jpg"), 21));
    fx.tick();
    assert!(fx.rows().contains(&"Sintra (1)".to_string()), "an added folder: {:?}", fx.rows());
    let porto_id = fx.s().library().folder_by_path(&porto).unwrap();
    fx.outside(|| {
        remove(&pictures.join("Sintra"));
        rename(&porto, &pictures.join("Porto 2026"));
    });
    fx.tick();
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Lisbon (4)", "Porto 2026 (1)", "Trip (2)"]), "removed and renamed folders");
    assert_eq!(fx.s().library().folder_by_path(&fx.pictures.join("Porto 2026")), Some(porto_id), "a renamed folder keeps its id");
    let lisbon_id = fx.s().library().folder_by_path(&lisbon).unwrap();
    let moved = fx.pictures.join("Archive").join("2026").join("Lisbon");
    fx.outside(|| rename(&lisbon, &moved));
    fx.tick();
    assert_eq!(fx.s().library().folder_by_path(&moved), Some(lisbon_id), "a moved folder keeps its id");
    assert_eq!(fx.photo(&moved, "IMG_0001.jpg"), Some(starred));
    assert!(fx.star(starred) && fx.star(renamed), "and its stars");
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Lisbon (4)", "Porto 2026 (1)", "Trip (2)"]));
    fx.s_mut().flush().unwrap();
    assert_eq!(sidecar(&moved)["photos"]["IMG_0001.jpg"]["star"], json!(true), "the sidecar moved with the folder");
    phase(&mut t, "folders through the watcher");

    // 3. The watcher off: nothing changes until the verification pass is due; then it finds all.
    fx.s_mut().set_watching(WatchMode::Off);
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.s().watched_roots().is_empty(), "nothing watched");
    let porto = fx.pictures.join("Porto 2026");
    let card = fx.card.clone();
    fx.outside(|| {
        put(&porto.join("IMG_0007.jpg"), 7);
        remove(&moved.join("IMG_0004.jpg"));
        rename(&moved.join("IMG_0003.jpg"), &moved.join("Castle.jpg"));
        put(&pictures.join("Cascais").join("IMG_0001.jpg"), 41);
        rename(&card.join("Trip"), &card.join("Trip 2026"));
    });
    let before = fx.listing();
    for _ in 0..3 {
        fx.clock.advance(Duration::from_secs(10));
        fx.d.settle(FRAMES).unwrap();
    }
    assert_eq!(fx.listing(), before, "without the watcher, nothing before the verification is due");
    fx.clock.advance(FULL_VERIFICATION);
    fx.d.step();
    fx.s_mut().wait_idle();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(
        fx.listing(),
        strs(&[
            "Pictures/Archive/2026/Lisbon/Castle.jpg",
            "Pictures/Archive/2026/Lisbon/IMG_0001.jpg",
            "Pictures/Archive/2026/Lisbon/Tram.jpg",
            "Pictures/Cascais/IMG_0001.jpg",
            "Pictures/Porto 2026/IMG_0001.jpg",
            "Pictures/Porto 2026/IMG_0007.jpg",
            "Volumes/CARD/DSC_0001.jpg",
            "Volumes/CARD/Trip 2026/DSC_0002.jpg",
            "Volumes/CARD/Trip 2026/DSC_0003.jpg",
        ]),
        "the verification pass alone found every change"
    );
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Cascais (1)", "Lisbon (3)", "Porto 2026 (2)", "Trip 2026 (2)"]));
    assert!(fx.star(starred) && fx.star(renamed), "the stars stay");
    phase(&mut t, "verification pass, watcher off");

    // 4. The volume unplugged (its root renamed away), the watcher back on: rows and photos stay,
    //    dimmed; a star meanwhile waits; no folder is made at the old path; it lands on return.
    fx.s_mut().set_watching(WatchMode::Polling);
    fx.tick();
    let trip = card.join("Trip 2026");
    let on_card = fx.photo(&trip, "DSC_0002.jpg").unwrap();
    let count = fx.s().library().photos().count();
    let away = fx.outside.join("CARD");
    fx.outside(|| rename(&card, &away));
    fx.check_volumes();
    fx.tick();
    assert_eq!(fx.s().root_online(&card), Some(false), "the card is offline");
    assert_eq!(fx.offline_rows(), strs(&["CARD (1)", "Trip 2026 (2)"]), "its rows, and only them, are dimmed offline");
    assert_eq!(fx.rows(), strs(&["CARD (1)", "Cascais (1)", "Lisbon (3)", "Porto 2026 (2)", "Trip 2026 (2)"]), "the rows stay");
    assert_eq!(fx.s().library().photos().count(), count, "its photos stay");
    fx.show(&trip);
    assert!(fx.cell(on_card), "with their cells");
    fx.d.click(&format!("thumb:{on_card}")).unwrap();
    fx.d.click("button:star").unwrap();
    assert!(fx.star(on_card), "the star is made (and journalled)");
    fx.tick();
    assert!(matches!(fx.s_mut().flush(), Err(CmdError::Unsaved(_))), "its sidecar write waits");
    assert_eq!(fx.s().pending_writes().map(|p| p.sidecars), Some(1), "pendingWrites counts it");
    assert!(!exists(&card), "no folder is made at the old path");
    fx.outside(|| rename(&away, &card));
    fx.check_volumes();
    fx.tick();
    assert_eq!(fx.s().root_online(&card), Some(true), "back online");
    assert_eq!(fx.offline_rows(), Vec::<String>::new(), "nothing dimmed");
    fx.s_mut().flush().unwrap();
    assert_eq!(sidecar(&trip)["photos"]["DSC_0002.jpg"]["star"], json!(true), "the star was written when the card returned");
    fx.clock.advance(SETTLE);
    assert!(fx.s_mut().refresh_volumes(PATIENCE));
    assert_eq!(fx.s().pending_writes(), None, "nothing waits");
    phase(&mut t, "unplugged and back");

    // 5. An emptied mount point: the card's files go, an empty folder stays at its path. Offline;
    //    a star meanwhile writes no sidecar into it; with the files back, the star is written.
    let at_root = fx.photo(&card, "DSC_0001.jpg").unwrap();
    fx.outside(|| {
        rename(&card, &away);
        mkdir(&card);
    });
    fx.check_volumes();
    fx.tick();
    assert_eq!(fx.s().root_online(&card), Some(false), "an empty folder where the library has photos is offline");
    assert_eq!(fx.s().library().photos().count(), count, "nothing removed");
    fx.d.run("photo.star.set", &json!({ "ids": [at_root], "on": true })).unwrap();
    fx.tick();
    assert!(matches!(fx.s_mut().flush(), Err(CmdError::Unsaved(_))), "reported unsaved");
    assert!(!exists(&card.join(".dalica.json")), "no sidecar in the empty mount point");
    assert!(!exists(&trip), "no folder made in it");
    fx.outside(|| {
        remove(&card);
        rename(&away, &card);
    });
    fx.check_volumes();
    fx.tick();
    fx.s_mut().flush().unwrap();
    assert_eq!(sidecar(&card)["photos"]["DSC_0001.jpg"]["star"], json!(true), "written once the files are back");
    assert_eq!(fx.s().library().photos().count(), count);
    phase(&mut t, "emptied mount point");

    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    t
}

// ---------------------------------------------------------------------------------------------
// First run on a fresh library
// ---------------------------------------------------------------------------------------------

/// Holds the scan's header reads in folder `held` until opened.
#[derive(Clone)]
struct Gate {
    held: &'static str,
    open: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    fn reader(&self) -> HeaderReader {
        let gate = self.clone();
        let inner = dalica_scan::default_reader();
        Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
            if path.parent().and_then(Path::file_name).is_some_and(|n| n == gate.held) {
                let (lock, cv) = &*gate.open;
                let open = lock.lock().unwrap();
                let _ = cv.wait_timeout_while(open, PATIENCE, |open| !*open).unwrap();
            }
            inner(path, limits)
        })
    }

    fn open(&self) {
        let (lock, cv) = &*self.open;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}

/// Cells drawing a picture this frame (covering or scaled up), and cells drawing nothing yet.
fn drawn(d: &Driver) -> (u64, u64) {
    let c = d.app().panels().map(|p| p.thumbnails.counts()).unwrap_or_default();
    (c.covered_cells + c.upscaled_cells, c.placeholder_cells + c.sketch_cells)
}

pub struct FirstRun {
    pub photos: usize,
    /// Frames from Continue until the first picture, until the first folder's were all drawn,
    /// and until every cell was.
    pub first: usize,
    pub first_folder: usize,
    pub all: usize,
    pub secs: f64,
}

/// A fresh library directory, its standard places planted under a temp home: the window offers
/// the first-run panel; Continue (the default choice) adds Pictures, Documents and Desktop as Scan
/// Always; the first folder's rows and thumbnails fill in while the scan is held in the second;
/// then every thumbnail.
pub fn first_run_fills_in() -> FirstRun {
    let started = Instant::now();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    for i in 0..10 {
        put(&home.join("Pictures").join("2026 Lisbon").join(format!("IMG_{i:04}.jpg")), 100 + i);
    }
    for i in 0..10 {
        put(&home.join("Pictures").join("2026 Porto").join(format!("IMG_{i:04}.jpg")), 200 + i);
    }
    for f in ["Documents", "Desktop"] {
        off_owner({
            let p = home.join(f);
            move || std::fs::create_dir_all(p).unwrap()
        });
    }
    let mut s = Session::open(&tmp.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_places(Places::under(&home), Vec::new());
    assert!(s.is_new_library(), "a fresh library directory");
    let gate = Gate { held: "2026 Porto", open: Arc::new((Mutex::new(false), Condvar::new())) };
    s.set_scan_reader(Some(gate.reader()));
    // As the window does: it persists the view, then gets the library.
    let mut app = App::new(None);
    app.persist_view();
    app.set_session(s);
    assert_eq!(app.state().dialogs.iter().map(|d| d.kind).collect::<Vec<_>>(), [DialogKind::FirstRun], "the first-run panel");
    let mut d = Driver::new(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    assert!(d.widgets().iter().any(|w| w.id == "dialog:firstRun"));
    let ui = dalica_fs::mark_ui_thread();
    d.click("button:dialog.continue").unwrap();
    assert!(d.state().dialogs.is_empty(), "Continue closes the panel");

    // The first folder fills in while the scan waits in the second.
    let (mut first, mut first_folder) = (None, None);
    let end = Instant::now() + PATIENCE;
    let mut frame = 0;
    while first_folder.is_none() {
        assert!(Instant::now() < end, "the first folder's thumbnails never filled in: {:?}", drawn(&d));
        d.step();
        frame += 1;
        let (pictures, _) = drawn(&d);
        if pictures > 0 && first.is_none() {
            first = Some(frame);
        }
        if pictures == 10 {
            first_folder = Some(frame);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(d.app().session().unwrap().busy(), "the scan is still running");
    let rows: Vec<String> = d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).map(|w| w.label).collect();
    assert_eq!(rows, ["2026 Lisbon (10)"], "the first folder is in the tree, the second not yet");
    gate.open();
    let end = Instant::now() + PATIENCE;
    loop {
        assert!(Instant::now() < end, "never every thumbnail: {:?}", drawn(&d));
        d.step();
        frame += 1;
        let (pictures, waiting) = drawn(&d);
        if pictures == 20 && waiting == 0 && !d.app().session().unwrap().busy() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    d.settle(FRAMES).unwrap();
    let mut rows: Vec<String> = d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).map(|w| w.label).collect();
    rows.sort();
    assert_eq!(rows, ["2026 Lisbon (10)", "2026 Porto (10)"]);
    let s = d.app().session().unwrap();
    let watched: Vec<PathBuf> =
        s.folder_rules().iter().filter(|(_, r)| r.mode.map(|m| m.id()) == Some("always")).map(|(p, _)| p.to_path_buf()).collect();
    assert_eq!(watched, [home.join("Desktop"), home.join("Documents"), home.join("Pictures")], "the standard places, Scan Always");
    assert!(!s.is_new_library());
    let photos = s.library().photos().count();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    FirstRun { photos, first: first.unwrap_or(0), first_folder: first_folder.unwrap_or(0), all: frame, secs: started.elapsed().as_secs_f64() }
}
