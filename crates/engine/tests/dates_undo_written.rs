//! Undoing Adjust Date after its write has landed (`roadmap.md` M7.30; `organizing.md` §9,
//! `xmp-interop.md` §1): once the new date is in the file and a rescan has read it back as the
//! file's own date, undo still puts the original date back, in the library and in the file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{DateTime, PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_meta::Limits;
use serde_json::json;

const WAIT: Duration = Duration::from_secs(30);

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

struct Fx {
    _dir: tempfile::TempDir,
    path: PathBuf,
    id: PhotoId,
    s: Session,
}

/// One JPEG taken at `taken` (EXIF DateTimeOriginal) in a watched folder next to the library.
fn fixture(taken: DateTime) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    fs::create_dir_all(&folder).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, 96, 64), 80, Some(&exif)).unwrap();
    let path = folder.join("IMG_0001.jpg");
    fs::write(&path, jpeg).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(SystemClock)).unwrap();
    s.wait_idle();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    Fx { _dir: dir, path, id, s }
}

fn file_date(path: &Path) -> Option<DateTime> {
    dalica_meta::read(&fs::read(path).unwrap(), &Limits::default()).unwrap().owned.date_taken.map(|d| d.date_time())
}

/// Polls until `done`, at most [`WAIT`].
fn poll_until(s: &mut Session, mut done: impl FnMut(&Session) -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < WAIT {
        s.poll();
        if done(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The library's effective date and the file's date.
fn both(fx: &Fx) -> (Option<DateTime>, Option<DateTime>) {
    (fx.s.library().effective_date(fx.id), file_date(&fx.path))
}

#[test]
fn undo_after_the_write_landed_and_was_read_back_puts_the_original_date_back() {
    let (taken, new) = (dt("2026-06-04T10:00:00"), dt("2026-06-04T11:30:00"));
    let mut fx = fixture(taken);
    fx.s.execute("photo.dates.set", &json!({ "ids": [fx.id], "set": "2026-06-04T11:30:00" })).unwrap();
    // The write lands through its job, and the watcher's rescan reads the new date back as the
    // file's own.
    let id = fx.id;
    assert!(poll_until(&mut fx.s, |s| s.pending_metadata_writes() == 0), "the write landed");
    assert!(poll_until(&mut fx.s, |s| s.library().photo(id).unwrap().date == Some(new)), "the rescan read the written date");
    assert_eq!(both(&fx), (Some(new), Some(new)));

    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(taken), Some(taken)), "undo: the library and the file read the original date");

    fx.s.execute("edit.redo", &json!({})).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(new), Some(new)), "redo: both read the new date");
    // The redo's write is read back too; undo once more still finds the original.
    assert!(poll_until(&mut fx.s, |s| s.library().photo(id).unwrap().date == Some(new)), "the rescan read the redo's date");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(taken), Some(taken)), "undo again: both read the original date");
}

#[test]
fn undo_with_the_write_still_queued_puts_the_original_date_back() {
    let (taken, new) = (dt("2026-06-04T10:00:00"), dt("2026-06-04T11:30:00"));
    let mut fx = fixture(taken);
    let m0 = fx.s.library().model().clone();
    fx.s.execute("photo.dates.set", &json!({ "ids": [fx.id], "set": "2026-06-04T11:30:00" })).unwrap();
    assert_eq!(fx.s.pending_metadata_writes(), 1, "the write waits in its queue");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(fx.s.library().model(), &m0, "nothing was read back: undo restores the library exactly");
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(taken), Some(taken)), "undo: the library and the file read the original date");

    fx.s.execute("edit.redo", &json!({})).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(new), Some(new)), "redo: both read the new date");
    fx.s.execute("edit.undo", &json!({})).unwrap();
    assert!(fx.s.flush_metadata(WAIT));
    assert_eq!(both(&fx), (Some(taken), Some(taken)), "undo again after the redo's write landed");
}
