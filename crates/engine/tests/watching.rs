//! Watching and external changes end to end (`roadmap.md` M5.17; `data-model.md` §7): on temp
//! libraries while the session runs, files and folders are added, removed, renamed and rewritten
//! outside Dalica and each change reaches the library, through the watcher (the polling backend on
//! virtual time, so every step is deterministic; one smoke test on the system's events) or, with
//! the watcher off, through the verification pass alone. A moved folder keeps its stars, a stale
//! temporary file is removed and a fresh one kept, a missing photo's sidecar entry survives 29
//! days and is gone after 31, and an emptied root (an unmounted volume's mount point) removes
//! nothing; such a root (or one on another device without the library's files) counts as offline
//! and gets no sidecar. The owner thread is marked as the UI thread: it makes no `fs` call. File changes the
//! test makes itself run on a helper thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_core::{Clock, PhotoId, SystemClock, Timestamp, VirtualClock};
#[cfg(unix)]
use dalica_engine::offline::RootProbe;
use dalica_engine::offline::{CHECK_EVERY, SETTLE};
use dalica_engine::watching::{FIRST_VERIFICATION, FULL_VERIFICATION, WatchMode};
use dalica_engine::{CacheDir, CmdError, JobKind, Options, Session, ThumbOptions};
use dalica_fs::Expect;
#[cfg(unix)]
use dalica_scan::volumes::{RootState, root_state};
use dalica_sidecar::{Field, Limits, Sidecar};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Runs `f` on a helper thread: the test's own file changes never count against the owner.
fn off_owner<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 120, 80), 80, None).unwrap()
}

fn put(path: &Path, seed: u64) {
    let path = path.to_path_buf();
    off_owner(move || {
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        let expect = dalica_fs::identity(&path).unwrap().map_or(Expect::Absent, Expect::Unchanged);
        dalica_fs::replace(&path, &jpeg(seed), expect).unwrap();
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

fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    off_owner(move || path.exists())
}

struct Fx {
    _tmp: tempfile::TempDir,
    library: PathBuf,
    /// The watched root, holding `Lisbon` (3 photos) and `Porto` (2).
    root: PathBuf,
    outside: PathBuf,
    clock: Arc<VirtualClock>,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Pictures");
        for i in 1..=3 {
            put(&root.join("Lisbon").join(format!("IMG_{i:04}.jpg")), i);
        }
        for i in 1..=2 {
            put(&root.join("Porto").join(format!("IMG_{i:04}.jpg")), 10 + i);
        }
        Fx {
            library: tmp.path().join("library"),
            outside: tmp.path().join("Elsewhere"),
            root,
            _tmp: tmp,
            // Far enough in the past that 31 virtual days later is still before the files' own times.
            clock: Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000))),
        }
    }

    fn open(&self, mode: WatchMode) -> Session {
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&self.library, self.clock.clone(), &options).unwrap();
        s.set_watching(mode);
        s
    }

    /// A library watching the root, scanned, the given photos starred and written to the sidecars.
    fn scanned(&self, mode: WatchMode, starred: &[(&str, &str)]) -> Session {
        let mut s = self.open(mode);
        s.execute("folders.add", &json!({ "path": self.root })).unwrap();
        s.wait_idle();
        for (folder, name) in starred {
            let id = photo(&s, &self.root.join(folder), name).unwrap();
            s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
        }
        s.flush().unwrap();
        s
    }

    /// Drives the polling watcher on virtual time: a few one-second ticks, each waited for, and
    /// whatever they reported rescanned and applied.
    fn settle(&self, s: &mut Session) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(PATIENCE), "the watcher answered");
            s.wait_idle();
        }
    }

    fn sidecar(&self, folder: &Path) -> Value {
        let path = folder.join(".dalica.json");
        let bytes = off_owner(move || std::fs::read(path).unwrap());
        serde_json::from_slice(&bytes).unwrap()
    }
}

/// The photo named `name` in `folder`, if the library has it.
fn photo(s: &Session, folder: &Path, name: &str) -> Option<PhotoId> {
    let f = s.library().folder_by_path(folder)?;
    s.library().photos_in(f).into_iter().find(|p| p.name == name).map(|p| p.id)
}

/// Every (folder path relative to `root`, photo name) in the library, sorted.
fn listing(s: &Session, root: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = s
        .library()
        .photos()
        .map(|p| {
            let folder = &s.library().folder(p.folder).unwrap().path;
            let rel = folder.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
            (rel.join("/"), p.name.clone())
        })
        .collect();
    v.sort();
    v
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    v.sort();
    v
}

fn size_of(s: &Session, id: PhotoId) -> u64 {
    s.library().photo(id).unwrap().size
}

/// `roadmap.md` M5.17: through the (polling) watcher, each external change reaches the library:
/// files added, removed, renamed (the photo keeps its id and star) and rewritten; folders added,
/// removed, renamed and moved (keeping their photos' ids and stars). A folder set to Remove inside
/// the root is not watched. The owner thread makes no `fs` call.
#[test]
fn external_changes_reach_the_library_through_the_watcher() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Polling, &[("Lisbon", "IMG_0001.jpg"), ("Lisbon", "IMG_0002.jpg")]);
    let ui = dalica_fs::mark_ui_thread();
    let lisbon = fx.root.join("Lisbon");
    let starred = photo(&s, &lisbon, "IMG_0001.jpg").unwrap();
    let renamed = photo(&s, &lisbon, "IMG_0002.jpg").unwrap();
    let rewritten = photo(&s, &lisbon, "IMG_0003.jpg").unwrap();
    fx.settle(&mut s);
    assert!(s.watched_roots().iter().any(|(r, _)| *r == fx.root), "{:?}", s.watched_roots());
    let reports = s.watch_reports();

    // Files: added, removed, renamed, rewritten.
    put(&lisbon.join("IMG_0004.jpg"), 4);
    remove(&fx.root.join("Porto").join("IMG_0002.jpg"));
    rename(&lisbon.join("IMG_0002.jpg"), &lisbon.join("Tram.jpg"));
    let before = size_of(&s, rewritten);
    put(&lisbon.join("IMG_0003.jpg"), 99);
    fx.settle(&mut s);
    assert_eq!(
        listing(&s, &fx.root),
        pairs(&[
            ("Lisbon", "IMG_0001.jpg"),
            ("Lisbon", "IMG_0003.jpg"),
            ("Lisbon", "IMG_0004.jpg"),
            ("Lisbon", "Tram.jpg"),
            ("Porto", "IMG_0001.jpg")
        ])
    );
    assert_eq!(photo(&s, &lisbon, "Tram.jpg"), Some(renamed), "a renamed file keeps its id");
    assert!(s.library().photo(renamed).unwrap().star, "and its star");
    assert_eq!(photo(&s, &lisbon, "IMG_0003.jpg"), Some(rewritten));
    assert_ne!(size_of(&s, rewritten), before, "a rewritten file is read again");
    assert!(s.watch_reports() > reports);

    // Folders: added, removed, renamed, then moved into another folder.
    put(&fx.root.join("Sintra").join("IMG_0001.jpg"), 21);
    remove(&fx.root.join("Porto"));
    fx.settle(&mut s);
    assert!(s.library().folder_by_path(&fx.root.join("Porto")).is_none());
    assert!(photo(&s, &fx.root.join("Sintra"), "IMG_0001.jpg").is_some());
    let folder = s.library().folder_by_path(&lisbon).unwrap();
    rename(&lisbon, &fx.root.join("Lisboa"));
    fx.settle(&mut s);
    assert_eq!(s.library().folder_by_path(&fx.root.join("Lisboa")), Some(folder), "a renamed folder keeps its id");
    let moved = fx.root.join("Archive").join("2026").join("Lisboa");
    rename(&fx.root.join("Lisboa"), &moved);
    fx.settle(&mut s);
    assert_eq!(s.library().folder_by_path(&moved), Some(folder), "a moved folder keeps its id");
    assert_eq!(photo(&s, &moved, "IMG_0001.jpg"), Some(starred));
    assert!(s.library().photo(starred).unwrap().star, "a moved folder keeps its stars");
    assert_eq!(s.library().photos_in(folder).len(), 4);

    // A folder set to Remove is not watched: what happens in it stays out of the library.
    s.execute("folders.set", &json!({ "changes": [{ "path": fx.root.join("Sintra"), "mode": "remove" }] })).unwrap();
    s.wait_idle();
    put(&fx.root.join("Sintra").join("IMG_0002.jpg"), 22);
    fx.settle(&mut s);
    assert!(s.library().folder_by_path(&fx.root.join("Sintra")).is_none());

    // The rescans were jobs like any scan.
    assert!(s.jobs().iter().any(|j| j.kind == JobKind::Scan && j.label.starts_with("Checked changed folders")), "{:?}", s.jobs());
    s.flush().unwrap();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    // The moved folder's sidecar went with it and still holds the star.
    assert_eq!(fx.sidecar(&moved)["photos"]["IMG_0001.jpg"]["star"], json!(true));
}

/// A folder renamed outside Dalica while its sidecar write is still to come: the write is tried at
/// the old path and fails (the folder is not there), the rescan finds the rename, and the write
/// lands at the new path. Nothing is reported unsaved afterwards: the failed attempt does not
/// outlive the write that landed (on slower machines this race is common).
#[test]
fn a_write_that_missed_a_renamed_folder_lands_at_its_new_path() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Polling, &[("Lisbon", "IMG_0001.jpg")]);
    let ui = dalica_fs::mark_ui_thread();
    let lisbon = fx.root.join("Lisbon");
    let second = photo(&s, &lisbon, "IMG_0002.jpg").unwrap();
    fx.settle(&mut s);
    // Renamed on disk before the next write: the star's write is tried at the old path.
    rename(&lisbon, &fx.root.join("Lisboa"));
    s.execute("photo.star.set", &json!({ "ids": [second], "on": true })).unwrap();
    fx.settle(&mut s);
    assert_eq!(photo(&s, &fx.root.join("Lisboa"), "IMG_0002.jpg"), Some(second), "the rename is found");
    s.flush().unwrap();
    let doc = fx.sidecar(&fx.root.join("Lisboa"));
    assert_eq!((doc["photos"]["IMG_0001.jpg"]["star"].clone(), doc["photos"]["IMG_0002.jpg"]["star"].clone()), (json!(true), json!(true)));
    assert!(!exists(&lisbon), "no folder made at the old path");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// `roadmap.md` M5.17: with the watcher switched off, the verification pass alone finds every
/// change, on the session clock: nothing before it is due, everything once it ran. A folder moved
/// in from outside the root brings its stars in its sidecar.
#[test]
fn with_the_watcher_off_the_verification_pass_alone_finds_everything() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Off, &[("Lisbon", "IMG_0001.jpg"), ("Porto", "IMG_0002.jpg")]);
    let ui = dalica_fs::mark_ui_thread();
    s.poll();
    assert!(s.watched_roots().is_empty(), "nothing watched");
    let lisbon = fx.root.join("Lisbon");
    put(&lisbon.join("IMG_0004.jpg"), 4);
    remove(&lisbon.join("IMG_0002.jpg"));
    rename(&lisbon.join("IMG_0003.jpg"), &lisbon.join("Tram.jpg"));
    put(&fx.root.join("Sintra").join("IMG_0001.jpg"), 21);
    // Porto leaves the root, and comes back later under another name.
    rename(&fx.root.join("Porto"), &fx.outside.join("Porto"));
    let before = listing(&s, &fx.root);

    // Not due yet: polls change nothing.
    fx.clock.advance(FULL_VERIFICATION - Duration::from_secs(1));
    for _ in 0..3 {
        s.poll();
    }
    assert!(!s.busy(), "no verification before it is due");
    assert_eq!(listing(&s, &fx.root), before);
    assert!(photo(&s, &lisbon, "IMG_0002.jpg").is_some());

    fx.clock.advance(Duration::from_secs(1));
    s.poll();
    assert!(s.busy(), "the verification started");
    s.wait_idle();
    assert_eq!(
        listing(&s, &fx.root),
        pairs(&[("Lisbon", "IMG_0001.jpg"), ("Lisbon", "IMG_0004.jpg"), ("Lisbon", "Tram.jpg"), ("Sintra", "IMG_0001.jpg")])
    );
    // Done: the next one is due a full interval later.
    fx.clock.advance(Duration::from_secs(60));
    s.poll();
    assert!(!s.busy());

    // The folder comes back under another name, its sidecar with it: the star comes back.
    rename(&fx.outside.join("Porto"), &fx.root.join("Porto 2026"));
    fx.clock.advance(FULL_VERIFICATION);
    s.poll();
    s.wait_idle();
    let back = photo(&s, &fx.root.join("Porto 2026"), "IMG_0002.jpg").unwrap();
    assert!(s.library().photo(back).unwrap().star, "a folder moved in brings its stars");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
}

/// Units no scan walked since the session opened (changes made while the app was closed) are
/// verified shortly after the first poll.
#[test]
fn changes_made_while_closed_are_found_after_the_first_poll() {
    let fx = Fx::new();
    let s = fx.scanned(WatchMode::Off, &[]);
    drop(s);
    remove(&fx.root.join("Porto").join("IMG_0001.jpg"));
    let mut s = fx.open(WatchMode::Off);
    let ui = dalica_fs::mark_ui_thread();
    assert!(photo(&s, &fx.root.join("Porto"), "IMG_0001.jpg").is_some(), "the index remembers it");
    s.poll();
    fx.clock.advance(FIRST_VERIFICATION - Duration::from_secs(1));
    s.poll();
    assert!(!s.busy());
    fx.clock.advance(Duration::from_secs(1));
    s.poll();
    s.wait_idle();
    assert!(photo(&s, &fx.root.join("Porto"), "IMG_0001.jpg").is_none());
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// `security.md` §5.8: when a folder is scanned (a scan, or a rescan of dirty folders), its stale
/// temporary files (older than a day by the session clock) are removed, a fresh one is kept, and a
/// name of nearly the same shape is never touched.
#[test]
fn a_stale_temp_is_removed_and_a_fresh_one_kept() {
    let fx = Fx::new();
    let lisbon = fx.root.join("Lisbon");
    let now = fx.clock.now();
    let at = |ago: Duration| UNIX_EPOCH + Duration::from_millis(u64::try_from(now.as_millis()).unwrap()) - ago;
    let temp = |folder: &Path, name: &str, ago: Duration| {
        let (path, t) = (folder.join(name), at(ago));
        off_owner(move || {
            std::fs::write(&path, b"half a sidecar").unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(t).unwrap();
        });
    };
    let stale = ".IMG_0001.jpg.dalica-tmp-0123456789abcdef";
    let fresh = ".dalica.json.dalica-tmp-fedcba9876543210";
    let near = ".IMG_0002.jpg.dalica-tmp-0123456789ABCDEF";
    temp(&lisbon, stale, 2 * DAY);
    temp(&lisbon, fresh, Duration::from_secs(60 * 60));
    temp(&lisbon, near, 2 * DAY);

    let mut s = fx.scanned(WatchMode::Polling, &[]);
    let ui = dalica_fs::mark_ui_thread();
    assert!(!exists(&lisbon.join(stale)), "the stale temporary file is removed");
    assert!(exists(&lisbon.join(fresh)), "a fresh one is kept");
    assert!(exists(&lisbon.join(near)), "a near-miss name is never touched");

    // A rescan of a dirty folder cleans it too.
    fx.settle(&mut s);
    let porto = fx.root.join("Porto");
    temp(&porto, stale, 3 * DAY);
    put(&porto.join("IMG_0003.jpg"), 13);
    fx.settle(&mut s);
    assert!(photo(&s, &porto, "IMG_0003.jpg").is_some());
    assert!(!exists(&porto.join(stale)));
    assert!(exists(&lisbon.join(fresh)));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// `roadmap.md` M5.17: a photo deleted outside Dalica leaves the library at once; its sidecar
/// entry (with its star) survives 29 days of the folder's writes and is pruned by the first write
/// after 30, on virtual time.
#[test]
fn a_missing_photos_entry_survives_29_days_and_is_gone_after_31() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Polling, &[("Lisbon", "IMG_0001.jpg"), ("Lisbon", "IMG_0002.jpg")]);
    let ui = dalica_fs::mark_ui_thread();
    let lisbon = fx.root.join("Lisbon");
    let other = photo(&s, &lisbon, "IMG_0003.jpg").unwrap();
    fx.settle(&mut s);
    remove(&lisbon.join("IMG_0002.jpg"));
    fx.settle(&mut s);
    assert!(photo(&s, &lisbon, "IMG_0002.jpg").is_none(), "the photo left the library");
    s.flush().unwrap();
    let doc = fx.sidecar(&lisbon);
    assert_eq!(doc["photos"]["IMG_0002.jpg"]["star"], json!(true), "its entry is kept");
    assert!(doc["dalica"]["missing"]["IMG_0002.jpg"].is_string(), "{doc}");

    let toggle = |s: &mut Session| {
        s.execute("photo.star.toggle", &json!({ "ids": [other] })).unwrap();
        s.flush().unwrap();
    };
    fx.clock.advance(29 * DAY);
    toggle(&mut s);
    assert_eq!(fx.sidecar(&lisbon)["photos"]["IMG_0002.jpg"]["star"], json!(true), "29 days: still there");
    fx.clock.advance(2 * DAY);
    toggle(&mut s);
    let doc = fx.sidecar(&lisbon);
    assert!(doc["photos"].get("IMG_0002.jpg").is_none(), "31 days: pruned: {doc}");
    assert!(doc["dalica"].get("missing").is_none(), "{doc}");
    assert_eq!(doc["photos"]["IMG_0001.jpg"]["star"], json!(true), "the others stay");
    let bytes = serde_json::to_vec(&doc).unwrap();
    assert_eq!(Sidecar::parse(&bytes, &Limits::default()).unwrap().star("IMG_0001.jpg"), Field::Value(true));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// Data safety: a root that suddenly holds no entries at all (the mount point an unmounted volume
/// leaves behind) is treated like an offline one: neither the watcher's rescan nor the
/// verification pass removes anything under it, and everything is still there, with the same ids
/// and stars, when the volume comes back.
#[test]
fn an_emptied_root_removes_nothing() {
    let fx = Fx::new();
    // Photos directly at the root too.
    put(&fx.root.join("IMG_0100.jpg"), 100);
    let mut s = fx.scanned(WatchMode::Polling, &[("", "IMG_0100.jpg"), ("Lisbon", "IMG_0001.jpg")]);
    let ui = dalica_fs::mark_ui_thread();
    let before = listing(&s, &fx.root);
    let ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    fx.settle(&mut s);

    // "Unmounted": the root's content goes away and an empty folder stays at its path.
    let away = fx.outside.join("volume");
    rename(&fx.root, &away);
    off_owner({
        let root = fx.root.clone();
        move || std::fs::create_dir(root).unwrap()
    });
    fx.settle(&mut s);
    assert_eq!(listing(&s, &fx.root), before, "the watcher's rescan removed nothing");
    assert!(s.scan_warnings().iter().any(|w| w.contains("the folder is empty")), "{:?}", s.scan_warnings());
    fx.clock.advance(FULL_VERIFICATION);
    s.poll();
    s.wait_idle();
    assert_eq!(listing(&s, &fx.root), before, "the verification removed nothing");

    // "Mounted" again.
    remove(&fx.root);
    rename(&away, &fx.root);
    fx.settle(&mut s);
    s.poll();
    s.wait_idle();
    assert_eq!(listing(&s, &fx.root), before);
    let mut now: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    now.sort();
    let mut was = ids;
    was.sort();
    assert_eq!(now, was, "same ids");
    let root_photo = photo(&s, &fx.root, "IMG_0100.jpg").unwrap();
    assert!(s.library().photo(root_photo).unwrap().star);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// A folder deleted outside Dalica while its sidecar write is still to come (an edit within the
/// debounce): once the folder has left the library its write is dropped, not kept waiting for a
/// folder that will never come back. Flush is Ok, nothing waits, and nothing is created at the old
/// path. (Found by `accept M5` on Windows, where a deletion can be seen in two steps.)
#[test]
fn a_pending_write_of_a_folder_deleted_outside_dalica_is_dropped() {
    let fx = Fx::new();
    let mut s = fx.scanned(WatchMode::Polling, &[]);
    let ui = dalica_fs::mark_ui_thread();
    let porto = fx.root.join("Porto");
    let id = photo(&s, &porto, "IMG_0001.jpg").unwrap();
    fx.settle(&mut s);
    // A star written now, then a second edit within the debounce (the clock does not move): dirty,
    // not yet due, so no write runs while the folder is being deleted.
    s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    s.flush().unwrap();
    assert_eq!(fx.sidecar(&porto)["photos"]["IMG_0001.jpg"]["star"], json!(true));
    s.execute("photo.star.set", &json!({ "ids": [id], "on": false })).unwrap();
    remove(&porto);
    fx.settle(&mut s);
    assert!(s.library().folder_by_path(&porto).is_none(), "the folder left the library");
    for _ in 0..3 {
        fx.clock.advance(Duration::from_secs(30));
        fx.settle(&mut s);
    }
    assert_eq!(s.flush(), Ok(()), "nothing is left to save for a folder that is gone");
    assert_eq!(s.pending_writes(), None, "nothing waits");
    assert!(!exists(&porto), "nothing is created at the old path");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// The system's change events (native backend, real time, generous timeouts: events are hints
/// whose timing differs per OS): a photo added and one removed reach the library while the
/// session is polled as the UI polls it.
#[test]
fn the_systems_events_reach_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Pictures");
    put(&root.join("Lisbon").join("IMG_0001.jpg"), 1);
    put(&root.join("Lisbon").join("IMG_0002.jpg"), 2);
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&tmp.path().join("library"), Arc::new(SystemClock), &options).unwrap();
    assert_eq!(s.watching(), WatchMode::Native, "the default");
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let ui = dalica_fs::mark_ui_thread();
    let until = |s: &mut Session, what: &str, done: &dyn Fn(&Session) -> bool| {
        let start = Instant::now();
        while !done(s) {
            assert!(start.elapsed() < PATIENCE, "gave up waiting for {what}");
            s.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    until(&mut s, "the watcher", &|s| !s.watched_roots().is_empty());
    let lisbon = root.join("Lisbon");
    put(&lisbon.join("IMG_0003.jpg"), 3);
    until(&mut s, "the new photo", &|s| photo(s, &lisbon, "IMG_0003.jpg").is_some());
    remove(&lisbon.join("IMG_0001.jpg"));
    until(&mut s, "the removal", &|s| photo(s, &lisbon, "IMG_0001.jpg").is_none());
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// A card with a photo at its root and one in a folder, scanned, watcher off, checked online.
fn card(fx: &Fx) -> (Session, PathBuf, PhotoId) {
    let card = fx.outside.join("CARD");
    put(&card.join("IMG_0001.jpg"), 31);
    put(&card.join("Trip").join("IMG_0002.jpg"), 32);
    let mut s = fx.open(WatchMode::Off);
    s.execute("folders.add", &json!({ "path": card })).unwrap();
    s.wait_idle();
    assert!(s.refresh_volumes(PATIENCE));
    assert_eq!(s.root_online(&card), Some(true));
    let id = photo(&s, &card, "IMG_0001.jpg").unwrap();
    (s, card, id)
}

/// Advances to the next online check and waits for it.
fn check(fx: &Fx, s: &mut Session) {
    fx.clock.advance(CHECK_EVERY);
    assert!(s.refresh_volumes(PATIENCE));
}

/// Data safety (M5.13's open item): a root left as an empty folder (the mount point of an
/// unmounted volume) counts as offline. A star made meanwhile is journalled and reported unsaved,
/// and no sidecar appears in the empty folder; once the files are back the star is written.
#[test]
fn an_emptied_mount_point_is_offline_and_gets_no_sidecar() {
    let fx = Fx::new();
    let (mut s, card, id) = card(&fx);
    let ui = dalica_fs::mark_ui_thread();
    let away = fx.outside.join("CARD (volume)");
    rename(&card, &away);
    off_owner({
        let card = card.clone();
        move || std::fs::create_dir(card).unwrap()
    });
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(false), "an empty folder where the library has photos is offline");
    s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    assert!(s.library().photo(id).unwrap().star, "the star is applied and journalled");
    s.poll();
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("not there")), "reported unsaved");
    assert!(!exists(&card.join(".dalica.json")), "no sidecar in the empty mount point");
    assert_eq!(s.pending_writes().map(|p| p.sidecars), Some(1));

    // The volume is back: the star is written.
    remove(&card);
    rename(&away, &card);
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(true));
    s.flush().unwrap();
    assert_eq!(fx.sidecar(&card)["photos"]["IMG_0001.jpg"]["star"], json!(true));
    fx.clock.advance(SETTLE);
    assert!(s.refresh_volumes(PATIENCE));
    assert_eq!(s.pending_writes(), None);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}

/// A root whose device changed and that holds none of the library's files (another volume mounted
/// in its place) is offline and gets no sidecar; one on a new device that holds the library's
/// files (a drive plugged in again under a new device number) is accepted. Device ids are Unix
/// only; the probe is planted to stand for the other device.
#[cfg(unix)]
#[test]
fn a_root_on_another_device_is_offline_unless_its_files_are_there() {
    use std::sync::atomic::{AtomicU8, Ordering};
    let fx = Fx::new();
    let (mut s, card, id) = card(&fx);
    // 0: as it is; 1: another device without our files; 2: another device with them.
    let swap = Arc::new(AtomicU8::new(0));
    let probe: RootProbe = {
        let swap = Arc::clone(&swap);
        Arc::new(move |root: &Path, samples: &[PathBuf]| {
            let real = root_state(root, samples);
            match swap.load(Ordering::SeqCst) {
                0 => real,
                n => RootState { device: real.device.map(|d| d ^ 0xff), known: n == 2, ..real },
            }
        })
    };
    s.set_root_probe(Some(probe));
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(true), "the device is remembered");
    let ui = dalica_fs::mark_ui_thread();

    swap.store(1, Ordering::SeqCst);
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(false), "another device without our files");
    s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    s.poll();
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(m)) if m.contains("not there")));
    assert!(!exists(&card.join(".dalica.json")), "nothing written while held, though the folder is there");

    swap.store(2, Ordering::SeqCst);
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(true), "a new device holding our files is accepted");
    s.flush().unwrap();
    assert_eq!(fx.sidecar(&card)["photos"]["IMG_0001.jpg"]["star"], json!(true));
    // The new device is remembered from now on.
    check(&fx, &mut s);
    assert_eq!(s.root_online(&card), Some(true));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0);
}
