//! The file-job runner (`roadmap.md` M8.3; `architecture.md` §5) with a fake item worker that
//! writes `scenes` JPEGs: progress counts, Cancel between items finishes the current one and starts
//! no other, results applied once and in order, a failing (or panicking) item reported with its
//! reason while the rest go on, a new file appears as a photo and a replaced one keeps its id (and a
//! rescan imports nothing from Dalica's own write), a new folder outside the library is added and
//! scanned, and the owner thread makes no `fs` call.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, Done, FileChange, FileItem, FileJob, JobKind, JobState, Notice, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(120);

fn jpeg(seed: u64, width: u32, height: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, width, height), 80, None).unwrap()
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
}

impl Lib {
    /// A library watching `Pictures/Lisbon` with `n` photos, scanned.
    fn new(n: usize) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64, 96, 64), Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
        s.wait_idle();
        Lib { dir, s }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn photo(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }
}

/// What the fake worker does with an item.
#[derive(Clone)]
enum Act {
    /// Writes a new file at the path.
    Create,
    /// Replaces photo's file at the path with a 120 × 80 JPEG.
    Replace(PhotoId),
    Fail(&'static str),
    Panic,
}

/// Lets items through one at a time, and records which started.
#[derive(Default)]
struct Gate {
    state: Mutex<(usize, Vec<usize>)>,
    cv: Condvar,
}

impl Gate {
    fn open(&self, n: usize) {
        self.state.lock().unwrap().0 = n;
        self.cv.notify_all();
    }

    /// Waits until item `i` may go on.
    fn pass(&self, i: usize) {
        let mut st = self.state.lock().unwrap();
        st.1.push(i);
        self.cv.notify_all();
        let _ = self.cv.wait_timeout_while(st, Duration::from_secs(30), |st| st.0 <= i).unwrap();
    }

    fn started(&self) -> Vec<usize> {
        self.state.lock().unwrap().1.clone()
    }

    fn wait_started(&self, i: usize) {
        let st = self.state.lock().unwrap();
        let _ = self.cv.wait_timeout_while(st, Duration::from_secs(30), |st| !st.1.contains(&i)).unwrap();
    }
}

struct Item {
    index: usize,
    path: PathBuf,
    act: Act,
    /// Held this long before it finishes.
    delay: Duration,
    gate: Option<Arc<Gate>>,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.path
    }
}

fn item(index: usize, path: PathBuf, act: Act) -> Item {
    Item { index, path, act, delay: Duration::ZERO, gate: None }
}

/// The fake item worker: writes through `fs` on its own thread.
fn work(item: Item) -> Result<Done<usize>, String> {
    if let Some(g) = &item.gate {
        g.pass(item.index);
    }
    std::thread::sleep(item.delay);
    let changes = match item.act {
        Act::Create => {
            if let Some(dir) = item.path.parent() {
                dalica_fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            dalica_fs::replace(&item.path, &jpeg(100 + item.index as u64, 64, 48), Expect::Absent).map_err(|e| e.to_string())?;
            vec![FileChange::Created { path: item.path.clone() }]
        }
        Act::Replace(photo) => {
            let id = dalica_fs::identity(&item.path).map_err(|e| e.to_string())?.ok_or("gone")?;
            dalica_fs::replace(&item.path, &jpeg(7, 120, 80), Expect::Unchanged(id)).map_err(|e| e.to_string())?;
            vec![FileChange::Replaced { photo, path: item.path.clone() }]
        }
        Act::Fail(why) => return Err(why.to_string()),
        Act::Panic => panic!("a deliberate panic"),
    };
    Ok(Done { changes, value: item.index })
}

/// Polls until `f` holds or the patience runs out.
fn until(s: &mut Session, f: impl Fn(&Session) -> bool) {
    let start = Instant::now();
    while !f(s) {
        assert!(start.elapsed() < PATIENCE, "timed out");
        s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn progress_counts_rise_to_the_total_and_the_job_ends_done_with_its_event() {
    let mut lib = Lib::new(1);
    let gate = Arc::new(Gate::default());
    let items: Vec<Item> =
        (0..3).map(|i| Item { gate: Some(Arc::clone(&gate)), ..item(i, lib.lisbon().join(format!("copy-{i}.jpg")), Act::Create) }).collect();
    let folder = lib.lisbon();
    let job = FileJob::new(JobKind::Export, "Exporting", items, work)
        .on_finish(move |_, t| Notice { message: format!("Exported {} pictures", t.done), folder: Some(folder) });
    let id = lib.s.start_file_job(job);
    let entry = lib.s.job(id).unwrap();
    assert_eq!((entry.kind, entry.state, entry.cancellable), (JobKind::Export, JobState::Running, true));
    assert_eq!((entry.progress.done, entry.progress.total, entry.label.as_str()), (0, 3, "Exporting 0 of 3"));
    gate.open(1);
    until(&mut lib.s, |s| s.job(id).unwrap().progress.done == 1);
    assert_eq!(lib.s.job(id).unwrap().label, "Exporting 1 of 3");
    assert_eq!(lib.s.job(id).unwrap().state, JobState::Running);
    gate.open(3);
    let done = lib.s.wait_job(id, PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Done);
    assert_eq!((done.progress.done, done.progress.total), (3, 3));
    assert_eq!(done.label, "Exported 3 pictures");
    let result = done.result.unwrap();
    assert_eq!((result["total"].clone(), result["done"].clone(), result["cancelled"].clone()), (json!(3), json!(3), json!(false)));
    assert_eq!(result["folder"], json!(lib.lisbon()));
    let events: Vec<_> = lib.s.file_job_events().cloned().collect();
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!((e.job, e.kind, e.message.as_str(), e.done, e.total, e.cancelled), (id, JobKind::Export, "Exported 3 pictures", 3, 3, false));
    assert_eq!(e.folder.as_deref(), Some(lib.lisbon().as_path()));
    assert!(e.failures.is_empty());
    assert!(!lib.s.file_jobs_running());
}

#[test]
fn cancel_between_items_finishes_the_current_one_and_starts_no_other() {
    let mut lib = Lib::new(1);
    let gate = Arc::new(Gate::default());
    let paths: Vec<PathBuf> = (0..3).map(|i| lib.lisbon().join(format!("copy-{i}.jpg"))).collect();
    let items = paths.iter().enumerate().map(|(i, p)| Item { gate: Some(Arc::clone(&gate)), ..item(i, p.clone(), Act::Create) }).collect();
    let id = lib.s.start_file_job(FileJob::new(JobKind::Save, "Saving", items, work));
    gate.wait_started(0);
    assert_eq!(lib.s.cancel_jobs(Some(id)).unwrap(), [id]);
    gate.open(3);
    let done = lib.s.wait_job(id, PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Cancelled);
    assert_eq!(gate.started(), [0], "no item started after Cancel");
    assert!(paths[0].is_file() && !paths[1].exists() && !paths[2].exists());
    assert_eq!(done.result.unwrap()["done"], 1);
    assert_eq!((done.progress.done, done.progress.total), (1, 3));
    let e = lib.s.file_job_events().last().unwrap().clone();
    assert!(e.cancelled && e.done == 1 && e.total == 3);
    assert_eq!(e.message, "Saving cancelled: 1 of 3 done");
    // The finished item is in the library.
    assert!(lib.s.library().photos().any(|p| p.name == "copy-0.jpg"));
    assert!(!lib.s.library().photos().any(|p| p.name == "copy-1.jpg"));
}

#[test]
fn results_are_applied_once_and_in_item_order_whatever_order_they_finish_in() {
    let mut lib = Lib::new(1);
    let n = 8;
    // Later items finish first on four threads.
    let items = (0..n)
        .map(|i| Item { delay: Duration::from_millis(10 * (n - i) as u64), ..item(i, lib.lisbon().join(format!("copy-{i}.jpg")), Act::Create) })
        .collect();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let job = FileJob::new(JobKind::Export, "Exporting", items, work).workers(4).on_applied(move |s, a| {
        assert_eq!(a.index, a.value);
        assert_eq!(a.photos.len(), 1, "the new file is a photo by the time the caller sees it");
        assert!(s.library().photo(a.photos[0].1).is_some());
        log.lock().unwrap().push(a.index);
        Ok(Vec::new())
    });
    let id = lib.s.start_file_job(job);
    assert_eq!(lib.s.wait_job(id, PATIENCE).unwrap().state, JobState::Done);
    assert_eq!(*seen.lock().unwrap(), (0..n).collect::<Vec<_>>());
    // Polling again applies nothing twice.
    for _ in 0..10 {
        lib.s.poll();
    }
    assert_eq!(seen.lock().unwrap().len(), n);
    assert_eq!(lib.s.library().photos().count(), 1 + n);
}

#[test]
fn a_failing_item_is_reported_with_its_reason_while_the_rest_go_on() {
    let mut lib = Lib::new(1);
    let lisbon = lib.lisbon();
    let p = |i: usize| lisbon.join(format!("copy-{i}.jpg"));
    let items = vec![item(0, p(0), Act::Create), item(1, p(1), Act::Fail("disk full")), item(2, p(2), Act::Panic), item(3, p(3), Act::Create)];
    let id = lib.s.start_file_job(FileJob::new(JobKind::Save, "Saving", items, work).workers(2));
    let done = lib.s.wait_job(id, PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Done, "failed items do not fail the job");
    let e = lib.s.file_job_events().last().unwrap().clone();
    assert_eq!((e.done, e.total, e.cancelled), (2, 4, false));
    assert_eq!(e.failures.len(), 2);
    assert_eq!((e.failures[0].file.clone(), e.failures[0].reason.as_str()), (p(1), "disk full"));
    assert_eq!(e.failures[1].file, p(2));
    assert!(e.failures[1].reason.contains("deliberate panic"), "{}", e.failures[1].reason);
    assert_eq!(e.message, "Saving: 2 of 4 done, 2 failed");
    let result = done.result.unwrap();
    assert_eq!(result["failed"][0], json!({ "file": p(1), "reason": "disk full" }));
    assert!(lib.s.library().photos().any(|p| p.name == "copy-3.jpg"), "the item after the failures was done");
    // An Err from the caller's function is that item's failure too.
    let items = vec![item(0, p(4), Act::Create), item(1, p(5), Act::Create)];
    let job = FileJob::new(JobKind::Save, "Saving", items, work)
        .on_applied(|_, a| if a.index == 0 { Err("the record could not be set".into()) } else { Ok(Vec::new()) });
    let id = lib.s.start_file_job(job);
    lib.s.wait_job(id, PATIENCE).unwrap();
    let e = lib.s.file_job_events().last().unwrap().clone();
    assert_eq!((e.done, e.failures.len()), (1, 1));
    assert_eq!(e.failures[0].reason, "the record could not be set");
}

#[test]
fn a_new_file_appears_as_a_photo_and_a_replaced_one_keeps_its_id_with_no_import() {
    let mut lib = Lib::new(2);
    let first = lib.photo("IMG_0001.jpg");
    let before = lib.s.library().photo(first).unwrap().clone();
    assert_eq!((before.width, before.height), (96, 64));
    let replaced = lib.lisbon().join("IMG_0001.jpg");
    let created = lib.lisbon().join("IMG_0001-001.jpg");
    let photos = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&photos);
    let items = vec![item(0, replaced.clone(), Act::Replace(first)), item(1, created.clone(), Act::Create)];
    let job = FileJob::new(JobKind::Save, "Saving", items, work).on_applied(move |_, a| {
        log.lock().unwrap().extend(a.photos);
        Ok(Vec::new())
    });
    let ui = dalica_fs::mark_ui_thread();
    let id = lib.s.start_file_job(job);
    assert_eq!(lib.s.wait_job(id, PATIENCE).unwrap().state, JobState::Done);
    drop(ui);
    let after = lib.s.library().photo(first).unwrap().clone();
    let on_disk = std::fs::metadata(&replaced).unwrap();
    assert_eq!((after.id, after.name.as_str(), after.folder), (first, "IMG_0001.jpg", before.folder), "the same photo");
    assert_eq!((after.width, after.height, after.size), (120, 80, on_disk.len()), "with the new file's facts");
    let copy = lib.s.library().photos().find(|p| p.name == "IMG_0001-001.jpg").unwrap().clone();
    assert_eq!((copy.width, copy.height, copy.folder, copy.star), (64, 48, before.folder, false));
    assert_eq!(*photos.lock().unwrap(), [(replaced, first), (created, copy.id)]);
    assert!(lib.s.undo_stack().is_empty(), "no undo step");
    // A rescan of the folder finds Dalica's own write unchanged: nothing imported, ids kept.
    let count = lib.s.library().photos().count();
    lib.s.start_verification();
    lib.s.wait_idle();
    assert_eq!(lib.s.library().photos().count(), count);
    assert_eq!(lib.s.library().photo(first).unwrap(), &after);
    assert_eq!(lib.s.library().photos().find(|p| p.name == "IMG_0001-001.jpg").unwrap().id, copy.id);
    assert!(lib.s.undo_stack().is_empty(), "nothing imported from the files");
}

#[test]
fn a_new_folder_outside_the_library_is_added_and_scanned_when_the_job_ends() {
    let mut lib = Lib::new(1);
    let out = lib.dir.path().join("Elsewhere").join("Exports").join("Lisbon");
    let items: Vec<Item> = (0..3).map(|i| item(i, out.join(format!("{:03}-IMG.jpg", i + 1)), Act::Create)).collect();
    let applied = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&applied);
    let dir = out.clone();
    let job = FileJob::new(JobKind::Export, "Exporting", items, move |item: Item| {
        let mut done = work(item)?;
        done.changes.push(FileChange::Folder { path: dir.clone() });
        Ok(done)
    })
    .on_applied(move |_, a| {
        log.lock().unwrap().push(a.photos.len());
        Ok(Vec::new())
    });
    let ui = dalica_fs::mark_ui_thread();
    lib.s.start_file_job(job);
    lib.s.wait_idle();
    drop(ui);
    assert_eq!(*applied.lock().unwrap(), [0, 0, 0], "files whose folder waits for its scan are not listed");
    assert_eq!(lib.s.roots(), [out.clone(), lib.dir.path().join("Pictures")], "the unwatched folder is added as Scan Always");
    let folder = lib.s.library().folder_by_path(&out).unwrap();
    let mut names: Vec<String> = lib.s.library().photos_in(folder).iter().map(|p| p.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["001-IMG.jpg", "002-IMG.jpg", "003-IMG.jpg"]);
    // A folder under a watched one is scanned, not added.
    let inside = lib.dir.path().join("Pictures").join("Exports").join("Porto");
    let job = FileJob::new(JobKind::Export, "Exporting", vec![item(0, inside.join("a.jpg"), Act::Create)], work);
    lib.s.start_file_job(job);
    lib.s.wait_idle();
    assert_eq!(lib.s.roots().len(), 2);
    let folder = lib.s.library().folder_by_path(&inside).unwrap();
    assert_eq!(lib.s.library().photos_in(folder).len(), 1);
}

#[test]
fn a_job_of_no_items_ends_at_the_next_poll_and_a_replace_finds_its_photo_by_path() {
    let mut lib = Lib::new(1);
    let id = lib.s.start_file_job(FileJob::new(JobKind::Save, "Saving", Vec::<Item>::new(), work));
    lib.s.poll();
    let entry = lib.s.job(id).unwrap();
    assert_eq!((entry.state, entry.progress.total), (JobState::Done, 0));
    // A replace naming a photo the library does not have refreshes the photo at that path.
    let first = lib.photo("IMG_0001.jpg");
    let path = lib.lisbon().join("IMG_0001.jpg");
    let job = FileJob::new(JobKind::Save, "Saving", vec![item(0, path, Act::Replace(PhotoId::new(9_999)))], work);
    let id = lib.s.start_file_job(job);
    assert_eq!(lib.s.wait_job(id, PATIENCE).unwrap().state, JobState::Done);
    assert_eq!(lib.s.library().photos().count(), 1);
    assert_eq!(lib.photo("IMG_0001.jpg"), first);
    assert_eq!(lib.s.library().photo(first).unwrap().width, 120);
}
