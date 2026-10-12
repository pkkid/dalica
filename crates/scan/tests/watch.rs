//! The watchers (`data-model.md` §7): the polling backend on virtual time, the `notify` backend
//! on real time with events as hints, the fallback from one to the other, and the bounded queue.
//!
//! Each backend goes through the same changes on a temp tree (an add, a delete, a rename, a
//! rewrite, a folder rename). After each, the folders it reports are verified incrementally, and
//! the library must then equal what a full verification gives: events only say where to look.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use dalica_core::Timestamp;
use dalica_library::{Library, Op};
use dalica_scan::watch::{
    Backend, DirtyFolder, DirtyQueue, PollOptions, Prefer, QUEUE_CAP, QueueOptions, WatchOptions, Watcher, Watching, known_folders, rescan,
};
use dalica_scan::{Options, diff, diff_rescan, scan};
use dalica_scenes::{Scene, jpeg};

fn put(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let w = 64 + u32::try_from(seed % 50).unwrap();
    std::fs::write(path, jpeg(&Scene::new(seed, w, 64), 80, None).unwrap()).unwrap();
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seed);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

/// root/a/1.jpg, root/a/b/2.jpg, root/c/3.jpg, root/c/d/4.jpg.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().join("root");
    put(&r.join("a/1.jpg"), 1);
    put(&r.join("a/b/2.jpg"), 2);
    put(&r.join("c/3.jpg"), 3);
    put(&r.join("c/d/4.jpg"), 4);
    (dir, r)
}

fn apply_all(lib: &mut Library, ops: Vec<Op>) {
    for op in ops {
        lib.apply(op).unwrap();
    }
}

fn library(root: &Path) -> Library {
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(&[root.to_path_buf()], None));
    apply_all(&mut lib, ops);
    lib
}

/// `(folder, name, size)` of every photo, sorted.
fn photos(lib: &Library) -> Vec<(PathBuf, String, u64)> {
    let mut v: Vec<_> = lib.photos().map(|p| (lib.folder(p.folder).unwrap().path.clone(), p.name.clone(), p.size)).collect();
    v.sort();
    v
}

fn covered(dirty: &[DirtyFolder], folder: &Path) -> bool {
    dirty.iter().any(|d| d.covers(folder))
}

/// How a test drives a backend until the folders it waits for are reported.
trait Drive {
    fn start(&mut self, roots: &[PathBuf]) -> Watching;
    /// Polls until every folder in `want` is covered (or the backend's limit passes); everything
    /// reported meanwhile.
    fn until(&mut self, w: &mut Watching, want: &[PathBuf]) -> Vec<DirtyFolder>;
}

/// The polling backend on virtual time: a change must be reported within two sweeps.
struct Virtual {
    now: Timestamp,
    queue: QueueOptions,
    poll: PollOptions,
}

impl Virtual {
    fn new() -> Virtual {
        Virtual {
            now: Timestamp::from_millis(1_000_000),
            queue: QueueOptions::default(),
            poll: PollOptions { sweep: Duration::from_secs(4), step: Duration::from_secs(1), min_per_step: 1, max_per_step: 2000 },
        }
    }

    fn tick(&mut self, w: &mut Watching) -> Vec<DirtyFolder> {
        self.now = self.now.saturating_add(self.poll.step);
        w.poll(self.now)
    }
}

impl Drive for Virtual {
    fn start(&mut self, roots: &[PathBuf]) -> Watching {
        let mut w = Watching::start(roots, WatchOptions { prefer: Prefer::Polling, queue: self.queue, poll: self.poll, ..WatchOptions::default() });
        // The first sweep learns the tree and reports nothing.
        while w.polling().sweeps() == 0 {
            assert!(self.tick(&mut w).is_empty());
        }
        w
    }

    fn until(&mut self, w: &mut Watching, want: &[PathBuf]) -> Vec<DirtyFolder> {
        let mut out = Vec::new();
        let limit = w.polling().sweeps() + 2;
        while w.polling().sweeps() < limit {
            out.extend(self.tick(w));
        }
        // Let the last changes settle (virtual time costs nothing).
        self.now = self.now.saturating_add(self.queue.max_delay);
        out.extend(w.poll(self.now));
        for f in want {
            assert!(covered(&out, f), "{} not reported within two sweeps: {out:?}", f.display());
        }
        out
    }
}

/// The `notify` backend on real time: events are hints; generous timeouts.
struct Real;

const NOTIFY_TIMEOUT: Duration = Duration::from_secs(30);

fn wall() -> Timestamp {
    let ms = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();
    Timestamp::from_millis(i64::try_from(ms).unwrap())
}

impl Drive for Real {
    fn start(&mut self, roots: &[PathBuf]) -> Watching {
        let queue = QueueOptions { debounce: Duration::from_millis(100), max_delay: Duration::from_secs(1), ..QueueOptions::default() };
        let w = Watching::start(roots, WatchOptions { queue, poll_network: false, ..WatchOptions::default() });
        assert_eq!(w.roots(), roots.iter().map(|r| (r.clone(), Backend::Native)).collect::<Vec<_>>(), "{:?}", w.fallbacks());
        w
    }

    fn until(&mut self, w: &mut Watching, want: &[PathBuf]) -> Vec<DirtyFolder> {
        let start = Instant::now();
        let mut out = Vec::new();
        loop {
            out.extend(w.poll(wall()));
            if want.iter().all(|f| covered(&out, f)) {
                // Take what is still settling too, so it does not leak into the next change.
                std::thread::sleep(Duration::from_millis(300));
                out.extend(w.poll(wall()));
                return out;
            }
            assert!(start.elapsed() < NOTIFY_TIMEOUT, "no events for {want:?} within {NOTIFY_TIMEOUT:?}: {out:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Verifies what was reported and checks the library against a full verification.
fn verify(lib: &mut Library, root: &Path, dirty: &[DirtyFolder]) {
    let inc = rescan(&known_folders(lib), &[root.to_path_buf()], dirty, &Options::default());
    let ops = diff_rescan(lib, &inc);
    apply_all(lib, ops);
    let mut full = lib.clone();
    let rest = diff(&mut full, &scan(&[root.to_path_buf()], None));
    assert_eq!(rest, [], "the incremental verification of {dirty:?} missed something");
    assert_eq!(photos(lib), photos(&library(root)));
}

/// Adds, deletes, renames and rewrites, each reported and verified.
fn changes_are_reported_and_verified(drive: &mut dyn Drive) {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    let mut w = drive.start(std::slice::from_ref(&r));

    // An add.
    put(&r.join("a/new.jpg"), 10);
    let d = drive.until(&mut w, &[r.join("a")]);
    verify(&mut lib, &r, &d);
    assert!(lib.photos().any(|p| p.name == "new.jpg"));

    // A delete.
    std::fs::remove_file(r.join("a/b/2.jpg")).unwrap();
    let d = drive.until(&mut w, &[r.join("a/b")]);
    verify(&mut lib, &r, &d);
    assert!(lib.folder_by_path(&r.join("a/b")).is_none());

    // A rename: the photo keeps its id.
    let three = lib.photos().find(|p| p.name == "3.jpg").unwrap().id;
    std::fs::rename(r.join("c/3.jpg"), r.join("c/three.jpg")).unwrap();
    let d = drive.until(&mut w, &[r.join("c")]);
    verify(&mut lib, &r, &d);
    assert_eq!(lib.photo(three).unwrap().name, "three.jpg");

    // A rewrite in place: new bytes, same name.
    let four = lib.photos().find(|p| p.name == "4.jpg").unwrap().clone();
    put(&r.join("c/d/4.jpg"), 41);
    let d = drive.until(&mut w, &[r.join("c/d")]);
    verify(&mut lib, &r, &d);
    assert_ne!(lib.photo(four.id).unwrap().size, four.size);

    // A folder rename: the folder and its photos keep their ids.
    let c = lib.folder_by_path(&r.join("c")).unwrap();
    std::fs::rename(r.join("c"), r.join("c2")).unwrap();
    let d = drive.until(&mut w, &[r.clone(), r.join("c2")]);
    verify(&mut lib, &r, &d);
    assert_eq!(lib.folder_by_path(&r.join("c2")), Some(c));
    assert_eq!(lib.photo(three).unwrap().folder, c);

    // A new folder with photos.
    put(&r.join("e/f/5.jpg"), 50);
    let d = drive.until(&mut w, &[r.join("e/f")]);
    verify(&mut lib, &r, &d);
    assert!(lib.folder_by_path(&r.join("e/f")).is_some());
}

#[test]
fn polling_reports_adds_deletes_renames_and_rewrites_within_two_sweeps() {
    changes_are_reported_and_verified(&mut Virtual::new());
}

#[test]
fn notify_reports_the_same_changes() {
    changes_are_reported_and_verified(&mut Real);
}

#[test]
fn polling_ignores_what_scans_skip_and_reports_nothing_when_nothing_changed() {
    let (_dir, r) = tree();
    let mut v = Virtual::new();
    let mut w = v.start(std::slice::from_ref(&r));
    std::fs::write(r.join("a/.dalica.json"), b"{}").unwrap();
    put(&r.join(".hidden/x.jpg"), 60);
    put(&r.join("a/@eaDir/x.jpg"), 61);
    let mut out = Vec::new();
    for _ in 0..12 {
        out.extend(v.tick(&mut w));
    }
    assert_eq!(out, []);
}

#[test]
fn polling_sweeps_are_sized_to_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().join("root");
    for i in 0..300 {
        std::fs::create_dir_all(r.join(format!("f{:02}", i / 10)).join(format!("g{i}"))).unwrap();
    }
    let mut v = Virtual::new();
    v.poll = PollOptions { sweep: Duration::from_secs(10), step: Duration::from_secs(1), min_per_step: 4, max_per_step: 2000 };
    let mut w = v.start(std::slice::from_ref(&r));
    // 1 + 30 + 300 folders over ten steps: 34 per step.
    assert_eq!(w.polling().folders(), 331);
    assert_eq!(w.polling().per_step(), 34);
    let (sweeps, listed) = (w.polling().sweeps(), w.polling().listed());
    let mut steps = 0;
    while w.polling().sweeps() == sweeps {
        v.tick(&mut w);
        steps += 1;
    }
    assert_eq!(w.polling().listed() - listed, 331);
    assert!(steps <= 11, "{steps} steps for one sweep");
    // A bound on the step makes a big library sweep slower instead.
    let mut v = Virtual::new();
    v.poll = PollOptions { max_per_step: 10, ..v.poll };
    let w = v.start(std::slice::from_ref(&r));
    assert_eq!(w.polling().per_step(), 10);
}

#[test]
fn an_unreachable_root_is_polled_and_reported_whole_when_it_returns() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().join("card");
    // Not there when watching starts: the native backend cannot watch it, the poller can wait.
    let mut w = Watching::start(std::slice::from_ref(&r), WatchOptions { poll_network: false, ..WatchOptions::default() });
    assert_eq!(w.roots(), [(r.clone(), Backend::Polling)]);
    assert_eq!(w.fallbacks().len(), 1, "{:?}", w.fallbacks());
    let mut v = Virtual::new();
    for _ in 0..4 {
        assert!(v.tick(&mut w).is_empty());
    }
    put(&r.join("x/1.jpg"), 70);
    let d = v.until(&mut w, &[r.clone(), r.join("x")]);
    assert!(d.contains(&DirtyFolder { path: r.clone(), subtree: true }), "{d:?}");
    // Unplugged again: nothing is reported (and nothing could be removed).
    std::fs::rename(&r, dir.path().join("card-out")).unwrap();
    let mut out = Vec::new();
    for _ in 0..12 {
        out.extend(v.tick(&mut w));
    }
    assert_eq!(out, []);
}

#[test]
fn ten_thousand_events_coalesce_within_the_cap() {
    let root = PathBuf::from("/photos");
    let t = Timestamp::from_millis(0);
    // Many events on few folders: one entry each.
    let mut q = DirtyQueue::new(std::slice::from_ref(&root), QueueOptions::default());
    for i in 0..10_000 {
        q.push(&root.join(format!("f{}", i % 3)), false, t);
    }
    assert_eq!((q.len(), q.events()), (3, 10_000));

    // Events on 10 000 different folders: the queue folds them into their parents and stays in
    // its cap, and what it hands out still covers every one of them.
    for cap in [QUEUE_CAP, 500] {
        let mut q = DirtyQueue::new(std::slice::from_ref(&root), QueueOptions { cap, ..QueueOptions::default() });
        let start = Instant::now();
        let folders: Vec<PathBuf> =
            (0..10_000).map(|i| root.join(format!("y{}", i % 7)).join(format!("m{}", i % 120)).join(format!("e{i}"))).collect();
        for f in &folders {
            q.push(f, false, t);
            assert!(q.len() <= cap);
        }
        let took = start.elapsed();
        assert!(q.folds() > 0);
        let out = q.take_ready(t.saturating_add(Duration::from_secs(1)));
        assert!(out.len() <= cap && q.is_empty());
        assert!(folders.iter().all(|f| covered(&out, f)), "a folder was dropped");
        println!("10 000 events, cap {cap}: {} entries, {took:?}", out.len());
        assert!(took < Duration::from_secs(10), "{took:?}");
    }
}
