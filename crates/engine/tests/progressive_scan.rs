//! Scans as jobs, applied progressively (`roadmap.md` M5.5; `architecture.md` §5): on a temporary
//! library of 2 000 `scenes` photos in 40 folders, the first batch lands (with its thumbnail jobs
//! queued) while the walk is still going, each poll applies at most one batch, progress rises to
//! the totals, and a cancelled scan stops within one folder and keeps what it applied. A gated
//! header reader holds the walk at a chosen folder, so "before the walk ends" is certain rather
//! than a race.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::scanning::PHOTOS_PER_POLL;
use dalica_engine::{CacheDir, JobKind, JobState, Options, ScanProgress, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_scan::HeaderReader;
use serde_json::json;

/// How long a test waits for something that should take a moment.
const PATIENCE: Duration = Duration::from_secs(120);

/// `folders` folders `F00`… of `per` photos `IMG_0001.jpg`… under `root`; a few distinct JPEGs
/// are encoded once and written many times.
fn tree(root: &Path, folders: usize, per: usize) -> PathBuf {
    let jpegs: Vec<Vec<u8>> = (0..8).map(|i| dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 96, 64), 80, None).unwrap()).collect();
    for f in 0..folders {
        let dir = root.join(format!("F{f:02}"));
        dalica_fs::create_dir_all(&dir).unwrap();
        for i in 0..per {
            dalica_fs::replace(&dir.join(format!("IMG_{:04}.jpg", i + 1)), &jpegs[(f + i) % jpegs.len()], Expect::Absent).unwrap();
        }
    }
    root.to_path_buf()
}

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    Session::open_with(library, Arc::new(SystemClock), &options).unwrap()
}

/// A header reader that holds every file of one folder until it is opened, and records every
/// path it was asked for.
#[derive(Clone)]
struct Gate {
    folder: String,
    state: Arc<(Mutex<(bool, bool)>, Condvar)>,
    read: Arc<Mutex<Vec<PathBuf>>>,
}

impl Gate {
    fn new(folder: &str) -> Gate {
        Gate { folder: folder.into(), state: Arc::new((Mutex::new((false, false)), Condvar::new())), read: Arc::default() }
    }

    fn reader(&self) -> HeaderReader {
        let gate = self.clone();
        let inner = dalica_scan::default_reader();
        Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
            gate.read.lock().unwrap().push(path.to_path_buf());
            if path.parent().and_then(Path::file_name).is_some_and(|n| n == gate.folder.as_str()) {
                let (lock, cv) = &*gate.state;
                let mut st = lock.lock().unwrap();
                st.0 = true;
                cv.notify_all();
                // Held well inside the scan's 10 s budget per file.
                let _ = cv.wait_timeout_while(st, Duration::from_secs(8), |st| !st.1).unwrap();
            }
            inner(path, limits)
        })
    }

    fn reached(&self) -> bool {
        self.state.0.lock().unwrap().0
    }

    fn open(&self) {
        let (lock, cv) = &*self.state;
        lock.lock().unwrap().1 = true;
        cv.notify_all();
    }

    /// The folders whose files were read.
    fn folders_read(&self) -> HashSet<String> {
        self.read.lock().unwrap().iter().filter_map(|p| Some(p.parent()?.file_name()?.to_string_lossy().into_owned())).collect()
    }
}

/// The scan job `folders.add` just started.
fn last_job(s: &Session) -> u64 {
    let job = s.jobs().last().cloned().unwrap();
    assert_eq!(job.kind, JobKind::Scan);
    job.job
}

/// Polls until `done` holds, recording the scan progress after every poll.
fn poll_until(s: &mut Session, progress: &mut Vec<ScanProgress>, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        s.poll();
        progress.push(s.scan_progress());
        assert!(start.elapsed() < PATIENCE, "gave up waiting: {:?}", s.scan_progress());
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// `roadmap.md` M5.5: the first batch is applied, and its photos have thumbnail jobs queued,
/// before the walk finishes; no poll applies more than one batch; the progress counts rise to the
/// totals. The owner thread makes no `fs` call throughout.
#[test]
fn the_first_batch_lands_with_its_thumbnails_queued_before_the_walk_ends() {
    let dir = tempfile::tempdir().unwrap();
    let root = tree(&dir.path().join("Pictures"), 40, 50);
    let mut s = open(&dir.path().join("library"));
    let gate = Gate::new("F39");
    s.set_scan_reader(Some(gate.reader()));
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();

    s.execute("folders.add", &json!({ "path": root })).unwrap();
    let job = last_job(&s);
    assert!(s.busy() && s.scan_progress().running);
    let mut progress = Vec::new();
    poll_until(&mut s, &mut progress, |s| s.library().photos().count() > 0);
    let first = s.library().photos().count();
    assert!(first <= PHOTOS_PER_POLL, "one poll applied {first} photos");
    let stats = s.thumbnail_stats();
    assert!(stats.jobs + stats.scanned + stats.failed >= first, "the first batch's thumbnails are not queued: {stats:?} for {first} photos");
    assert_eq!(s.job(job).unwrap().state, JobState::Running);

    // Everything before the held folder lands while the walk waits in it.
    let mut counts = vec![first];
    poll_until(&mut s, &mut progress, |s| {
        counts.push(s.library().photos().count());
        s.library().photos().count() == 39 * 50
    });
    assert!(gate.reached());
    assert_eq!(s.job(job).unwrap().state, JobState::Running, "the walk has not finished");
    assert_eq!(s.library().folders().count(), 39);
    let entry = s.job(job).unwrap();
    assert_eq!((entry.progress.done, entry.label.as_str()), (1950, "Scanning: 1,950 pictures found"));

    gate.open();
    poll_until(&mut s, &mut progress, |s| {
        counts.push(s.library().photos().count());
        s.job(job).is_some_and(|j| j.state.is_finished())
    });
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread touched the disk");

    let steps: HashSet<usize> = counts.iter().copied().collect();
    assert!(steps.len() > 2000 / PHOTOS_PER_POLL, "the library filled in {} steps", steps.len());
    for pair in counts.windows(2) {
        assert!(pair[1] - pair[0] <= PHOTOS_PER_POLL, "one poll applied {} photos", pair[1] - pair[0]);
    }
    for pair in progress.windows(2) {
        assert!(pair[1].found >= pair[0].found && pair[1].indexed >= pair[0].indexed, "progress went back: {pair:?}");
    }
    assert!(progress.iter().any(|p| p.running && p.indexed > 0 && p.indexed < 2000), "no progress was seen between start and end");
    assert_eq!(s.scan_progress(), ScanProgress { running: false, folders_queued: 0, found: 2000, indexed: 2000 });
    let entry = s.job(job).unwrap();
    assert_eq!(entry.state, JobState::Done);
    assert_eq!((entry.progress.done, entry.progress.total), (2000, 2000));
    let result = entry.result.unwrap();
    assert_eq!((result["folders"].clone(), result["found"].clone(), result["indexed"].clone()), (json!(40), json!(2000), json!(2000)));
    assert_eq!(result["walked"], json!([root]));
    assert_eq!(s.library().photos().count(), 2000);
    assert_eq!(s.library().folders().count(), 40);
    assert!(!s.busy());
    s.wait_idle();
    assert_eq!(s.library().sketch_count(), 2000);
    assert!(s.scan_warnings().is_empty(), "{:?}", s.scan_warnings());
}

/// `roadmap.md` M5.5: cancelling the scan job stops it within one folder (no file past the folder
/// it was reading is read) and keeps what was applied; a later scan finds the rest and the photos
/// already there keep their ids.
#[test]
fn a_cancelled_scan_stops_within_one_folder_and_keeps_what_it_applied() {
    let dir = tempfile::tempdir().unwrap();
    let root = tree(&dir.path().join("Pictures"), 40, 50);
    let mut s = open(&dir.path().join("library"));
    let gate = Gate::new("F20");
    s.set_scan_reader(Some(gate.reader()));
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    let job = last_job(&s);
    let mut progress = Vec::new();
    poll_until(&mut s, &mut progress, |s| gate.reached() && s.library().photos().count() == 20 * 50);
    let kept: Vec<(PathBuf, PhotoId)> = {
        let lib = s.library();
        let mut v: Vec<_> = lib.photos().map(|p| (lib.folder(p.folder).unwrap().path.join(&p.name), p.id)).collect();
        v.sort();
        v
    };

    assert_eq!(s.cancel_jobs(Some(job)).unwrap(), [job]);
    gate.open();
    let entry = s.wait_job(job, PATIENCE).unwrap();
    assert_eq!(entry.state, JobState::Cancelled, "{entry:?}");
    assert_eq!(entry.result.as_ref().unwrap()["cancelled"], true);
    assert_eq!(entry.result.as_ref().unwrap()["indexed"], 1000);
    assert!(entry.label.starts_with("Scan cancelled"), "{}", entry.label);
    assert!(!s.busy() && !s.scan_progress().running);
    // Nothing past the folder it was reading.
    let read = gate.folders_read();
    for f in 21..40 {
        assert!(!read.contains(&format!("F{f:02}")), "F{f:02} was read after the cancel: {read:?}");
    }
    // What was applied stays; the held folder never landed.
    assert_eq!(s.library().folders().count(), 20);
    assert_eq!(s.library().photos().count(), 1000);
    assert!(s.library().folder_by_path(&root.join("F20")).is_none());
    // Cancelling again: it has finished, so nothing is cancelled.
    assert_eq!(s.cancel_jobs(Some(job)).unwrap(), Vec::<u64>::new());

    // A new scan finds the rest; the photos already there keep their ids.
    s.set_scan_reader(None);
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 2000);
    let lib = s.library();
    for (path, id) in kept {
        let p = lib.photo(id).unwrap();
        assert_eq!(lib.folder(p.folder).unwrap().path.join(&p.name), path);
    }
}

/// A cancelled rebuild keeps the old model (and its undo stack); a finished one replaces it as a
/// `rebuild` job.
#[test]
fn a_cancelled_rebuild_keeps_the_old_model() {
    let dir = tempfile::tempdir().unwrap();
    let root = tree(&dir.path().join("Pictures"), 6, 5);
    let mut s = open(&dir.path().join("library"));
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    s.flush().unwrap();
    let model = s.library().model().clone();

    let gate = Gate::new("F03");
    s.set_scan_reader(Some(gate.reader()));
    s.execute("library.rebuild", &json!({})).unwrap();
    let job = s.jobs().last().cloned().unwrap();
    assert_eq!(job.kind, JobKind::Rebuild);
    let mut progress = Vec::new();
    poll_until(&mut s, &mut progress, |_| gate.reached());
    assert_eq!(s.library().model(), &model, "a rebuild builds to the side");
    s.cancel_jobs(None).unwrap();
    gate.open();
    assert_eq!(s.wait_job(job.job, PATIENCE).unwrap().state, JobState::Cancelled);
    assert_eq!(s.library().model(), &model);
    assert_eq!(s.undo_stack().len(), 1);

    s.set_scan_reader(None);
    s.execute("library.rebuild", &json!({})).unwrap();
    let job = s.jobs().last().cloned().unwrap();
    assert_eq!(s.wait_job(job.job, PATIENCE).unwrap().state, JobState::Done);
    assert_eq!(s.library().photos().count(), 30);
    assert_eq!(s.library().starred().len(), 1, "the star came back from the sidecar");
    assert!(s.undo_stack().is_empty());
}

/// A file whose header read fails is reported once and given back to the next scan, which does
/// not read it again until it changes.
#[test]
fn a_failing_file_is_reported_once_and_not_read_again_until_it_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = tree(&dir.path().join("Pictures"), 1, 3);
    let bad = root.join("F00").join("IMG_0002.jpg");
    let calls = Arc::new(Mutex::new(0usize));
    let reader: HeaderReader = {
        let (calls, bad, inner) = (Arc::clone(&calls), bad.clone(), dalica_scan::default_reader());
        Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
            if path == bad {
                *calls.lock().unwrap() += 1;
                return Err(dalica_image::Error::Malformed("a planted failure".into()));
            }
            inner(path, limits)
        })
    };
    let mut s = open(&dir.path().join("library"));
    s.set_scan_reader(Some(reader));
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(s.scan_failures().len(), 1);
    let warned = |s: &Session| s.scan_warnings().iter().filter(|w| w.contains("IMG_0002.jpg")).count();
    assert_eq!(warned(&s), 1, "{:?}", s.scan_warnings());
    assert_eq!(s.library().photos().count(), 3, "still listed");

    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(*calls.lock().unwrap(), 1, "read again unchanged");
    assert_eq!(warned(&s), 1);

    let file = std::fs::File::options().write(true).open(&bad).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(10)).unwrap();
    drop(file);
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(*calls.lock().unwrap(), 2, "a changed file is read again");
    assert_eq!(s.scan_failures().len(), 1);
}
