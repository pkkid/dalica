//! The repair of an interrupted Save through the session (`roadmap.md` M8.18; `data-model.md` §6
//! and §10.5): a session reopened after a Save that crashed at a planted step repairs the folder
//! on a worker and shows the photo with its edits, rolled back (the original at its name, the
//! edits unsaved again) or forward (the edits in the file, the saved record done); an interrupted
//! Undo Save is finished; a pending record a scan reads in a running session is repaired too; the
//! owner thread makes no `fs` call, and a folder with nothing to repair starts no job.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::shared;
use dalica_core::{PhotoId, SystemClock, Timestamp};
use dalica_create::save::{self, Seen};
use dalica_edit::Recipe;
use dalica_engine::save_recovery::LABEL;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_fs::faults::{self, Fault, points};
use dalica_library::SaveState;
use dalica_meta::Changes;
use dalica_sidecar::originals::{Originals, State};
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(120);
const NAME: &str = "IMG_0001.jpg";
const AUTOFIX: &str = "autofix()";
const TUNING: &str = "tuning(0.25,0,0,0,none)";

struct Lib {
    dir: tempfile::TempDir,
    s: Option<Session>,
    id: PhotoId,
    original: Vec<u8>,
}

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    Session::open_with(library, Arc::new(SystemClock), &options).unwrap()
}

impl Lib {
    /// A library watching `Pictures/Lisbon` with one photo, scanned.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        let original = dalica_scenes::jpeg(&dalica_scenes::Scene::new(31, 320, 240), 85, None).unwrap();
        dalica_fs::replace(&lisbon.join(NAME), &original, Expect::Absent).unwrap();
        let mut s = open(&dir.path().join("library"));
        s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
        s.wait_idle();
        let id = s.library().photos().next().unwrap().id;
        Lib { dir, s: Some(s), id, original }
    }

    fn s(&mut self) -> &mut Session {
        self.s.as_mut().unwrap()
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn edit(&mut self, op: &str) {
        let id = self.id;
        self.s().execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
    }

    fn edits(&mut self) -> Vec<String> {
        let id = self.id;
        self.s().library().info(id).edits.clone()
    }

    /// Saves the photo through `photo.save` and waits for it.
    fn save(&mut self) {
        let id = self.id;
        let reply = self.s().execute("photo.save", &json!({ "ids": [id], "confirm": true })).unwrap();
        let job = serde_json::from_value(reply["job"].clone()).unwrap();
        self.s().wait_job(job, PATIENCE).unwrap();
    }

    /// Closes the session once every write has landed.
    fn close(&mut self) {
        let mut s = self.s.take().unwrap();
        s.flush().unwrap();
        drop(s);
    }

    /// Reopens the session and waits for the repair with the owner thread marked: it may make
    /// no `fs` call.
    fn reopen(&mut self) {
        let mut s = open(&self.dir.path().join("library"));
        let ui = dalica_fs::mark_ui_thread();
        let before = dalica_fs::guard::refused();
        assert!(s.wait_file_jobs(PATIENCE));
        s.poll();
        assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
        drop(ui);
        self.s = Some(s);
    }

    /// What the session knew of the photo's file (the size and time a Save checks).
    fn seen(&mut self) -> Seen {
        let id = self.id;
        let p = self.s().library().photo(id).unwrap().clone();
        Seen { size: p.size, mtime: p.mtime }
    }

    /// Runs a Save of the photo with `edits` straight through `create`, stopped at the `nth` time
    /// it reaches `point` by `fault`, as the job's worker would (the session closed for a crash).
    fn plant(&self, edits: &[&str], seen: Seen, point: &'static str, nth: usize, fault: Fault) -> save::Error {
        arm_nth(point, nth, fault);
        let options = save::Options { seen: Some(seen), ..save::Options::new(Timestamp::from_system_time(std::time::SystemTime::now())) };
        let result = save::in_place(&self.lisbon(), NAME, &Recipe::parse(edits).unwrap(), &Changes::default(), &options);
        faults::disarm_all();
        result.unwrap_err()
    }

    fn photo_bytes(&self) -> Vec<u8> {
        std::fs::read(self.lisbon().join(NAME)).unwrap()
    }

    fn kept(&self) -> Option<Vec<u8>> {
        std::fs::read(self.lisbon().join(".dalica-originals").join(NAME)).ok()
    }

    /// The record's entry for the photo: its state and recipe.
    fn entry(&self) -> Option<(State, Vec<String>)> {
        let bytes = std::fs::read(self.lisbon().join(".dalica-originals").join(".dalica.json")).ok()?;
        match Originals::parse(&bytes, &dalica_sidecar::Limits::default()).unwrap().entry(NAME) {
            dalica_sidecar::Field::Value(e) => Some((e.state, e.recipe)),
            _ => None,
        }
    }

    fn repair_events(&mut self) -> Vec<String> {
        self.s().file_job_events().filter(|e| e.message.starts_with("Repaired")).map(|e| e.message.clone()).collect()
    }

    /// The temporary files in the folder and its `.dalica-originals/`, once the session's writes
    /// have landed (the sidecar writer uses the same shape while it writes).
    fn temporaries(&mut self) -> Vec<PathBuf> {
        self.s().flush().unwrap();
        let mut out = Vec::new();
        for d in [self.lisbon(), self.lisbon().join(".dalica-originals")] {
            if let Ok(list) = std::fs::read_dir(&d) {
                out.extend(list.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".dalica-tmp-")));
            }
        }
        out
    }
}

/// Arms `point` to fail with `fault` the `nth` time this thread reaches it.
fn arm_nth(point: &'static str, nth: usize, fault: Fault) {
    fn wait(point: &'static str, left: usize, fault: Fault) {
        if left == 0 {
            faults::arm(point, fault);
        } else {
            faults::run_at(point, move || wait(point, left - 1, fault));
        }
    }
    if nth <= 1 {
        faults::arm(point, fault);
    } else {
        faults::run_at(point, move || wait(point, nth - 1, fault));
    }
}

fn crashed(e: &save::Error) -> bool {
    matches!(e, save::Error::Fs(dalica_fs::Error::Crashed(_)))
}

/// A first Save that crashed right after moving the original into the backup (nothing at the
/// photo's name): the reopened session moves the original back and the photo keeps its edits,
/// unsaved.
#[test]
fn a_crash_after_the_original_moved_is_rolled_back_and_the_edits_stay() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    let seen = lib.seen();
    lib.close();
    assert!(crashed(&lib.plant(&[AUTOFIX], seen, points::BACKUP_FSYNC_DIR, 1, Fault::Crash)));
    assert!(!lib.lisbon().join(NAME).exists(), "the photo's name is empty");
    assert_eq!(lib.entry().map(|e| e.0), Some(State::Pending));

    lib.reopen();
    assert_eq!(lib.photo_bytes(), lib.original, "the original is back");
    assert_eq!(lib.kept(), None);
    assert_eq!(lib.entry(), None);
    assert!(lib.temporaries().is_empty());
    let id = lib.id;
    assert_eq!(lib.edits(), [AUTOFIX], "the edits are still there");
    assert!(lib.s().library().has_unsaved_edits(id));
    assert_eq!(lib.s().library().saved(id), None);
    assert_eq!(lib.repair_events().len(), 1, "{:?}", lib.repair_events());
    assert!(lib.s().scan_warnings().iter().any(|w| w.contains(NAME) && w.contains("taken back")), "{:?}", lib.s().scan_warnings());
}

/// A first Save that crashed after its new file was committed, before the record was marked done:
/// the reopened session finishes it, and the photo shows the edits from its file.
#[test]
fn a_crash_after_the_commit_is_rolled_forward_and_the_file_holds_the_edits() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    let seen = lib.seen();
    lib.close();
    // The atomic replace's renames: the pending record, the new file, the record marked done.
    assert!(crashed(&lib.plant(&[AUTOFIX], seen, points::RENAME, 3, Fault::Crash)));
    let saved = lib.photo_bytes();
    assert_ne!(saved, lib.original);

    lib.reopen();
    assert_eq!(lib.photo_bytes(), saved);
    assert_eq!(lib.kept().as_deref(), Some(lib.original.as_slice()));
    assert_eq!(lib.entry(), Some((State::Done, vec![AUTOFIX.to_string()])));
    let id = lib.id;
    assert!(lib.edits().is_empty(), "the edits are in the file");
    let record = lib.s().library().saved(id).cloned().unwrap();
    assert_eq!((record.state, record.recipe), (SaveState::Done, vec![AUTOFIX.to_string()]));
    let photo = lib.s().library().photo(id).unwrap().clone();
    assert_eq!(photo.size, saved.len() as u64, "the entry follows the saved file");
    // Undo Save works on the repaired photo and gives the exact original back.
    let reply = lib.s().execute("photo.undoSave", &json!({ "ids": [id] })).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    lib.s().wait_job(job, PATIENCE).unwrap();
    assert_eq!(lib.photo_bytes(), lib.original);
    assert_eq!(lib.edits(), [AUTOFIX]);
}

/// A second Save that crashed before its new file replaced the first one: rolled back, the photo
/// holds the original again and both Saves' edits come back as unsaved edits, in order.
#[test]
fn a_second_save_stopped_before_its_commit_brings_every_edit_back() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    lib.save();
    assert!(lib.edits().is_empty());
    lib.edit(TUNING);
    let seen = lib.seen();
    lib.close();
    assert!(crashed(&lib.plant(&[TUNING], seen, points::RENAME, 2, Fault::Crash)));
    assert_eq!(lib.entry(), Some((State::Pending, vec![AUTOFIX.to_string(), TUNING.to_string()])));

    lib.reopen();
    assert_eq!(lib.photo_bytes(), lib.original);
    assert_eq!(lib.entry(), None);
    let id = lib.id;
    assert_eq!(lib.edits(), [AUTOFIX, TUNING]);
    assert_eq!(lib.s().library().saved(id), None);
    let left = lib.temporaries();
    assert!(left.is_empty(), "{left:?}");
}

/// An Undo Save that crashed after moving the original back, before dropping the record: the
/// reopened session drops it and the saved edits come back.
#[test]
fn an_undo_save_stopped_after_its_move_is_finished() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    lib.save();
    lib.close();
    arm_nth(points::RENAME, 1, Fault::Crash);
    let undone = save::undo(&lib.lisbon(), NAME, &save::Options::new(Timestamp::from_millis(0)));
    faults::disarm_all();
    assert!(crashed(&undone.unwrap_err()));
    assert_eq!(lib.photo_bytes(), lib.original);
    assert_eq!(lib.entry().map(|e| e.0), Some(State::Done), "the record still names the original");

    lib.reopen();
    assert_eq!(lib.entry(), None);
    let id = lib.id;
    assert_eq!(lib.edits(), [AUTOFIX]);
    assert_eq!(lib.s().library().saved(id), None);
    let size = lib.original.len() as u64;
    assert_eq!(lib.s().library().photo(id).unwrap().size, size);
}

/// In a running session, a Save whose last step failed leaves its record pending; the scan that
/// reads it starts the repair, which finishes the Save from the file's content (the scan has
/// already taken the new file's size and time into the model).
#[test]
fn a_pending_record_a_scan_reads_is_repaired() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    let seen = lib.seen();
    let e = lib.plant(&[AUTOFIX], seen, points::RENAME, 3, Fault::Error);
    assert!(matches!(e, save::Error::Incomplete { .. }), "{e:?}");
    assert_eq!(lib.entry().map(|e| e.0), Some(State::Pending));

    // The verification pass rescans the folder and reads the record.
    lib.s().start_verification();
    lib.s().wait_idle();
    lib.s().wait_idle();
    assert_eq!(lib.entry(), Some((State::Done, vec![AUTOFIX.to_string()])));
    let id = lib.id;
    assert!(lib.edits().is_empty(), "{:?}", lib.edits());
    assert_eq!(lib.s().library().saved(id).map(|s| s.state), Some(SaveState::Done));
    assert_eq!(lib.repair_events().len(), 1);
}

/// Nothing to repair: a reopened session with saved and edited photos starts no job.
#[test]
fn nothing_to_repair_starts_no_job() {
    let _faults = shared();
    let mut lib = Lib::new();
    lib.edit(AUTOFIX);
    lib.save();
    lib.edit(TUNING);
    lib.close();
    lib.reopen();
    assert!(!lib.s().file_jobs_running());
    assert_eq!(lib.s().file_job_events().count(), 0);
    assert!(!lib.s().jobs().iter().any(|j| j.label.starts_with(LABEL)), "no repair job");
    assert_eq!(lib.edits(), [TUNING]);
}
