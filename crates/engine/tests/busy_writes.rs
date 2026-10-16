//! `Session::metadata_writes_busy` (M10.54): an app polls the session only while it has background
//! work, and the metadata writer's job is registered and its results applied by `poll`, so a
//! caption write queued in an idle app must count as background work until it lands; a write
//! waiting for its folder to come back must not (the writer tries it again on its own). It is
//! apart from `Session::busy`, which stays the scans (the status line, the activity spinner and
//! `ui.settle` read it so).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{jpeg, open_with};
use dalica_core::{Timestamp, VirtualClock};
use dalica_engine::{JobKind, Session};
use dalica_fs::Expect;
use dalica_meta::Limits;
use serde_json::json;

const WAIT: Duration = Duration::from_secs(30);

/// A session on a virtual clock watching one folder holding `a.jpg`, its first scan done.
fn one_photo(clock: Arc<VirtualClock>) -> (tempfile::TempDir, Session, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    dalica_fs::replace(&folder.join("a.jpg"), &jpeg(1), Expect::Absent).unwrap();
    let mut s = open_with(&dir.path().join("library"), clock);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 1);
    (dir, s, folder)
}

fn caption(path: &std::path::Path) -> Option<String> {
    dalica_meta::read(&fs::read(path).unwrap(), &Limits::default()).unwrap().owned.caption
}

/// Busy while the caption's write is queued (its debounce running, no job yet) and while its job
/// writes it; not busy once it has landed in the file.
#[test]
fn busy_while_a_metadata_write_is_queued_and_while_it_runs() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
    let (_dir, mut s, folder) = one_photo(clock.clone());
    let id = s.library().photos().next().unwrap().id;
    s.poll();
    assert!(!s.metadata_writes_busy(), "nothing to do after the scan");

    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Tram 28" })).unwrap();
    assert_eq!(s.pending_metadata_writes(), 1);
    assert!(s.metadata_writes_busy(), "busy while the write is queued");
    s.poll();
    assert!(s.metadata_writes_busy() && s.jobs().iter().all(|j| j.kind != JobKind::MetadataWrite), "still busy through the debounce, before any job");

    clock.advance(Duration::from_secs(2));
    let start = Instant::now();
    let mut saw_job = false;
    while s.pending_metadata_writes() > 0 || s.jobs().iter().any(|j| j.kind == JobKind::MetadataWrite && !j.state.is_finished()) {
        assert!(start.elapsed() < WAIT, "the write lands");
        saw_job |= s.jobs().iter().any(|j| j.kind == JobKind::MetadataWrite && !j.state.is_finished());
        assert!(s.metadata_writes_busy(), "busy until the write has landed");
        s.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(saw_job, "busy was checked while its job ran");
    assert_eq!(caption(&folder.join("a.jpg")).as_deref(), Some("Tram 28"));
    assert!(!s.metadata_writes_busy(), "not busy once it has landed");

    // A write waiting for its folder to come back does not keep the session busy.
    let away = folder.with_file_name("Lisbon (away)");
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Later" })).unwrap();
    fs::rename(&folder, &away).unwrap();
    assert!(s.flush_metadata(WAIT));
    assert_eq!(s.pending_metadata_writes(), 1, "still queued, waiting");
    assert!(!s.metadata_writes_busy(), "a waiting write is not busy work");
    fs::rename(&away, &folder).unwrap();
}
