//! The job registry on a session (`roadmap.md` M5.5; `control-protocol.md` §6.13): scans are
//! listed as jobs with progress and a result; finished jobs expire after 10 minutes of virtual
//! time; the registry caps at 64 entries; `cancel` and `wait` answer unknown jobs with
//! `NotFound`; and a new library is told apart for the first-run panel.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dalica_core::{Clock, Timestamp, VirtualClock};
use dalica_engine::jobs::{CAP, KEEP};
use dalica_engine::{CacheDir, CmdError, JobKind, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_scan::HeaderReader;
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(120);

/// A folder `name` of `n` photos under `root`.
fn folder(root: &Path, name: &str, n: usize) -> PathBuf {
    let dir = root.join(name);
    dalica_fs::create_dir_all(&dir).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&dir.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    dir
}

fn open(library: &Path, clock: Arc<dyn Clock>) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    Session::open_with(library, clock, &options).unwrap()
}

fn virtual_clock() -> Arc<VirtualClock> {
    Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)))
}

/// A header reader that holds every file until it is opened.
fn gated() -> (HeaderReader, Arc<(Mutex<bool>, Condvar)>) {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (g, inner) = (Arc::clone(&gate), dalica_scan::default_reader());
    let reader: HeaderReader = Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
        let (lock, cv) = &*g;
        let _ = cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(8), |open| !*open).unwrap();
        inner(path, limits)
    });
    (reader, gate)
}

fn release(gate: &(Mutex<bool>, Condvar)) {
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}

#[test]
fn a_scan_is_listed_as_a_job_with_its_progress_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let photos = folder(dir.path(), "Lisbon", 5);
    let clock = virtual_clock();
    let mut s = open(&dir.path().join("library"), clock.clone());
    clock.advance(Duration::from_millis(1_840));
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    let jobs = s.jobs();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!((job.job, job.kind, job.state, job.cancellable, job.started_ms), (1, JobKind::Scan, JobState::Running, true, 1_840));

    let done = s.wait_job(job.job, PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Done);
    let v = serde_json::to_value(&done).unwrap();
    assert_eq!(v["kind"], "scan");
    assert_eq!(v["state"], "done");
    assert_eq!(v["progress"], json!({ "done": 5, "total": 5 }));
    assert_eq!(v["startedMs"], 1_840);
    assert_eq!(v["label"], "Scan finished: 5 pictures");
    assert_eq!(v["result"]["roots"], json!([photos]));
    assert_eq!((v["result"]["found"].clone(), v["result"]["indexed"].clone(), v["result"]["cancelled"].clone()), (json!(5), json!(5), json!(false)));
    assert!(v.get("error").is_none());
    assert_eq!(s.library().photos().count(), 5);
}

/// `roadmap.md` M5.5: the registry expires entries after 10 minutes of virtual time.
#[test]
fn finished_jobs_expire_after_ten_minutes_of_session_time() {
    let dir = tempfile::tempdir().unwrap();
    let photos = folder(dir.path(), "Lisbon", 2);
    let clock = virtual_clock();
    let mut s = open(&dir.path().join("library"), clock.clone());
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    let job = s.jobs()[0].job;
    s.wait_idle();
    assert_eq!(s.job(job).unwrap().state, JobState::Done);

    clock.advance(KEEP - Duration::from_millis(1));
    s.poll();
    assert!(s.job(job).is_some(), "kept for 10 minutes");
    clock.advance(Duration::from_millis(1));
    assert!(s.job(job).is_none() && s.jobs().is_empty(), "gone after 10 minutes, even before a poll");
    s.poll();
    assert!(s.jobs().is_empty());
    assert!(matches!(s.wait_job(job, Duration::ZERO), Err(CmdError::NotFound(_))));
    assert!(matches!(s.cancel_jobs(Some(job)), Err(CmdError::NotFound(_))));
}

/// `roadmap.md` M5.5: the registry caps at 64 entries, dropping the oldest finished first.
#[test]
fn the_registry_caps_at_64_entries() {
    let dir = tempfile::tempdir().unwrap();
    let photos = folder(dir.path(), "Lisbon", 1);
    let mut s = open(&dir.path().join("library"), virtual_clock());
    for _ in 0..CAP + 6 {
        s.execute("folders.add", &json!({ "path": photos })).unwrap();
        s.wait_idle();
    }
    let ids: Vec<u64> = s.jobs().iter().map(|j| j.job).collect();
    assert_eq!(ids.len(), CAP);
    assert_eq!(ids.first(), Some(&7), "the 6 oldest went");
    assert_eq!(ids.last(), Some(&(CAP as u64 + 6)));
    assert!(s.job(1).is_none());
}

/// `cancel(all)` stops every running scan; `wait` with a short timeout gives the running entry.
#[test]
fn cancel_all_stops_every_running_scan_and_wait_can_time_out() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (folder(dir.path(), "A", 3), folder(dir.path(), "B", 3));
    let mut s = open(&dir.path().join("library"), virtual_clock());
    let (reader, gate) = gated();
    s.set_scan_reader(Some(reader));
    s.execute("folders.add", &json!({ "path": a })).unwrap();
    s.execute("folders.add", &json!({ "path": b })).unwrap();
    let ids: Vec<u64> = s.jobs().iter().map(|j| j.job).collect();
    assert_eq!(ids.len(), 2);

    let waited = s.wait_job(ids[0], Duration::from_millis(20)).unwrap();
    assert_eq!(waited.state, JobState::Running, "the timeout gives the running entry");
    assert!(s.busy());

    assert_eq!(s.cancel_jobs(None).unwrap(), ids);
    release(&gate);
    for id in &ids {
        assert_eq!(s.wait_job(*id, PATIENCE).unwrap().state, JobState::Cancelled);
    }
    assert!(!s.busy());
    assert_eq!(s.library().photos().count(), 0, "nothing was applied before the cancel");
    assert!(s.cancel_jobs(None).unwrap().is_empty(), "nothing left to cancel");
}

/// The first-run panel's question: a library that never had a folder.
#[test]
fn a_new_library_is_new_until_a_folder_is_added() {
    let dir = tempfile::tempdir().unwrap();
    let photos = folder(dir.path(), "Lisbon", 1);
    let lib = dir.path().join("library");
    let s = open(&lib, virtual_clock());
    assert!(s.is_new_library());
    drop(s);
    let mut s = open(&lib, virtual_clock());
    assert!(s.is_new_library(), "opening it again does not make it old");
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    assert!(!s.is_new_library());
    s.flush().unwrap();
    drop(s);
    assert!(!open(&lib, virtual_clock()).is_new_library());
}
