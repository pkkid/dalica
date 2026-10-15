//! The repair of interrupted file operations at open (`roadmap.md` M9.21; `data-model.md` §8 and
//! §10.5): a library left with a planted crash — files part-way through a move, a copy, a rename,
//! a trash and a folder move, their operations still in `file-ops.json` — is reopened, and the
//! repair (on a worker) rolls each item forward or back from what is on disk, through `store`'s
//! `intents::plan`: the folders are repaired, each photo is shown at its path with its id and data,
//! the repair is reported, and nothing is left in the journal. A side that cannot be looked at
//! waits in the journal; an undone copy is never taken for a move; the owner thread makes no `fs`
//! call. And a running batch rename whose rename failed after the file got its new name puts it
//! back with the others.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{entry, jpeg, shared};
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::fileops_recovery::RepairOutcome;
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_store::intents::{Direction, Intent, Intents, Item, Kind, State};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

fn open(library: &Path) -> Session {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, clock, &options).unwrap();
    s.set_watching(WatchMode::Off);
    s
}

struct Lib {
    dir: tempfile::TempDir,
}

impl Lib {
    /// `Pictures/{Lisbon, Porto, Archive/Old}` with the photos named in `files` (path under
    /// `Pictures`), each starred and captioned "The caption of <name>", scanned, the session closed.
    fn new(files: &[&str]) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let l = Lib { dir };
        for (i, rel) in files.iter().enumerate() {
            let path = l.path(rel);
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &jpeg(i as u64 + 1), Expect::Absent).unwrap();
        }
        let mut s = open(&l.library());
        s.execute("folders.add", &json!({ "path": l.path("") })).unwrap();
        s.wait_idle();
        let ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
        s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
        for id in ids {
            let name = s.library().photo(id).unwrap().name.clone();
            s.execute("photo.caption.set", &json!({ "ids": [id], "caption": format!("The caption of {name}") })).unwrap();
        }
        assert!(s.flush_metadata(PATIENCE));
        s.flush().unwrap();
        l
    }

    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/').filter(|c| !c.is_empty()).fold(self.dir.path().join("Pictures"), |p, c| p.join(c))
    }

    fn library(&self) -> PathBuf {
        self.dir.path().join("library")
    }

    /// The id the library gives the photo at `rel`, with the session closed.
    fn ids(&self, rels: &[&str]) -> Vec<PhotoId> {
        let s = open(&self.library());
        rels.iter().map(|rel| photo_at(&s, &self.path(rel)).unwrap_or_else(|| panic!("{rel} is not listed"))).collect()
    }

    /// Plants `ops` in the journal, as a session that stopped would have left them.
    fn plant(&self, ops: Vec<Intent>) {
        let mut j = Intents::open(&self.library(), Timestamp::from_millis(0)).unwrap().intents;
        for op in ops {
            j.begin(op).unwrap();
        }
    }

    /// Reopens the library and waits for the repair and the scans it started, with the owner
    /// thread marked: it may make no `fs` call.
    fn reopen(&self) -> Session {
        let mut s = open(&self.library());
        let ui = dalica_fs::mark_ui_thread();
        let refused = dalica_fs::guard::refused();
        assert!(s.wait_file_jobs(PATIENCE), "the repair ended");
        s.wait_idle();
        s.flush().unwrap();
        assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
        drop(ui);
        s
    }

    fn journal(&self) -> Value {
        std::fs::read(self.library().join("file-ops.json")).map_or(json!({ "ops": [] }), |b| serde_json::from_slice(&b).unwrap())
    }
}

fn photo_at(s: &Session, path: &Path) -> Option<PhotoId> {
    let folder = s.library().folder_by_path(path.parent()?)?;
    Some(s.library().photo_by_name(folder, path.file_name()?.to_str()?)?.id)
}

fn op(id: u64, kind: Kind, direction: Direction, items: Vec<(PathBuf, Option<PathBuf>, State)>) -> Intent {
    Intent { id, kind, direction, items: items.into_iter().map(|(from, to, state)| Item { from, to, state }).collect() }
}

/// Checks that the photo `id` is listed at `path` with its caption, its star and its sidecar entry.
fn shown(s: &Session, id: PhotoId, path: &Path, caption: &str) {
    assert_eq!(photo_at(s, path), Some(id), "{} is photo {id}", path.display());
    assert_eq!(s.library().info(id).caption, caption, "{}", path.display());
    assert!(s.library().photo(id).unwrap().star, "{}", path.display());
    let e = entry(path.parent().unwrap(), path.file_name().unwrap().to_str().unwrap()).unwrap_or_else(|| panic!("no entry for {}", path.display()));
    assert_eq!((e["caption"].clone(), e["star"].clone()), (json!(caption), json!(true)), "{}", path.display());
}

/// A session that stopped part-way through a move (one photo moved, one copied across volumes but
/// its source not yet in the trash, one whose destination holds another file, one not started), a
/// copy, a trash, a rename and a folder move: the reopened session repairs every folder and shows
/// each photo where it is, with its id and its data; the repair is reported and the journal is
/// empty.
#[test]
fn a_reopened_session_repairs_a_planted_crash_and_shows_each_photo_with_its_data() {
    let _faults = shared();
    let l = Lib::new(&[
        "Lisbon/IMG_0001.jpg",
        "Lisbon/IMG_0002.jpg",
        "Lisbon/IMG_0003.jpg",
        "Lisbon/IMG_0004.jpg",
        "Lisbon/IMG_0005.jpg",
        "Porto/PORTO_0001.jpg",
        "Porto/PORTO_0002.jpg",
        "Archive/Old/OLD_0001.jpg",
    ]);
    let p = |rel: &str| l.path(rel);
    let names = [
        "Lisbon/IMG_0001.jpg",
        "Lisbon/IMG_0002.jpg",
        "Lisbon/IMG_0003.jpg",
        "Lisbon/IMG_0004.jpg",
        "Porto/PORTO_0001.jpg",
        "Porto/PORTO_0002.jpg",
        "Archive/Old/OLD_0001.jpg",
    ];
    let ids = l.ids(&names);
    let bytes = |rel: &str| std::fs::read(p(rel)).unwrap();
    let (two, three) = (bytes("Lisbon/IMG_0002.jpg"), bytes("Lisbon/IMG_0003.jpg"));
    // The disk as the crash left it.
    std::fs::rename(p("Lisbon/IMG_0001.jpg"), p("Porto/IMG_0001.jpg")).unwrap();
    std::fs::copy(p("Lisbon/IMG_0002.jpg"), p("Porto/IMG_0002.jpg")).unwrap();
    std::fs::write(p("Porto/IMG_0003.jpg"), b"another program's file").unwrap();
    std::fs::copy(p("Porto/PORTO_0001.jpg"), p("Lisbon/PORTO_0001.jpg")).unwrap();
    std::fs::remove_file(p("Lisbon/IMG_0005.jpg")).unwrap(); // in the trash
    std::fs::rename(p("Porto/PORTO_0002.jpg"), p("Porto/Tram.jpg")).unwrap();
    std::fs::rename(p("Archive/Old"), p("Old")).unwrap();
    let (d, pend) = (State::Done, State::Pending);
    l.plant(vec![
        op(
            7,
            Kind::Move,
            Direction::Do,
            vec![
                (p("Lisbon/IMG_0001.jpg"), Some(p("Porto/IMG_0001.jpg")), d),
                (p("Lisbon/IMG_0002.jpg"), Some(p("Porto/IMG_0002.jpg")), pend),
                (p("Lisbon/IMG_0003.jpg"), Some(p("Porto/IMG_0003.jpg")), pend),
                (p("Lisbon/IMG_0004.jpg"), Some(p("Porto/IMG_0004.jpg")), pend),
            ],
        ),
        op(8, Kind::Copy, Direction::Do, vec![(p("Porto/PORTO_0001.jpg"), Some(p("Lisbon/PORTO_0001.jpg")), pend)]),
        op(9, Kind::Trash, Direction::Do, vec![(p("Lisbon/IMG_0005.jpg"), None, pend)]),
        op(10, Kind::Rename, Direction::Do, vec![(p("Porto/PORTO_0002.jpg"), Some(p("Porto/Tram.jpg")), pend)]),
        op(11, Kind::MoveFolder, Direction::Do, vec![(p("Archive/Old"), Some(p("Old")), pend)]),
    ]);

    let s = l.reopen();
    // The move: the moved photo and the verified copy (its source now in the trash) are in Porto
    // with their ids and data; the other two stay in Lisbon.
    shown(&s, ids[0], &p("Porto/IMG_0001.jpg"), "The caption of IMG_0001.jpg");
    shown(&s, ids[1], &p("Porto/IMG_0002.jpg"), "The caption of IMG_0002.jpg");
    assert_eq!(std::fs::read(p("Porto/IMG_0002.jpg")).unwrap(), two);
    assert!(!p("Lisbon/IMG_0002.jpg").exists(), "the source of the verified copy went to the trash");
    assert_eq!(entry(&p("Lisbon"), "IMG_0001.jpg"), None, "the entry left the source");
    shown(&s, ids[2], &p("Lisbon/IMG_0003.jpg"), "The caption of IMG_0003.jpg");
    assert_eq!(std::fs::read(p("Lisbon/IMG_0003.jpg")).unwrap(), three);
    assert_eq!(std::fs::read(p("Porto/IMG_0003.jpg")).unwrap(), b"another program's file", "never touched");
    shown(&s, ids[3], &p("Lisbon/IMG_0004.jpg"), "The caption of IMG_0004.jpg");
    // The copy: a second photo with the source's data.
    shown(&s, ids[4], &p("Porto/PORTO_0001.jpg"), "The caption of PORTO_0001.jpg");
    let copy = photo_at(&s, &p("Lisbon/PORTO_0001.jpg")).expect("the copy is listed");
    assert_ne!(copy, ids[4]);
    shown(&s, copy, &p("Lisbon/PORTO_0001.jpg"), "The caption of PORTO_0001.jpg");
    // The trash: the photo left the library.
    assert_eq!(photo_at(&s, &p("Lisbon/IMG_0005.jpg")), None);
    // The rename, and the folder move with its photo.
    shown(&s, ids[5], &p("Porto/Tram.jpg"), "The caption of PORTO_0002.jpg");
    shown(&s, ids[6], &p("Old/OLD_0001.jpg"), "The caption of OLD_0001.jpg");
    assert_eq!(s.library().folder_by_path(&p("Archive/Old")), None);
    // Nothing in the library but what is on disk.
    for photo in s.library().photos() {
        let path = s.library().folder(photo.folder).unwrap().path.join(&photo.name);
        assert!(path.exists(), "{} is listed but not there", path.display());
    }
    // Reported, and nothing left.
    let outcomes: Vec<(u64, RepairOutcome)> = s.file_op_repairs().map(|r| (r.op, r.outcome)).collect();
    use RepairOutcome::{Back, Forward};
    assert_eq!(outcomes, [(7, Forward), (7, Forward), (7, Back), (7, Back), (8, Forward), (9, Forward), (10, Forward), (11, Forward)]);
    let event = s.file_job_events().find(|e| e.message.starts_with("Repaired")).expect("the repair's notice");
    assert_eq!(event.message, "Repaired 5 interrupted file operations");
    assert!(s.scan_warnings().iter().any(|w| w.contains("interrupted file operation 7") && w.contains("IMG_0002.jpg") && w.contains("trash")));
    assert!(s.file_ops_left().is_empty());
    assert_eq!(l.journal()["ops"], json!([]));
    drop(s);
    // A second open finds nothing to repair and starts no job.
    let s = open(&l.library());
    assert!(!s.file_jobs_running() && s.file_op_repairs().next().is_none());
}

/// A batch rename stopped between two renames in one folder: the renamed photos keep their data
/// under their new names, the others under their old ones.
#[test]
fn a_batch_rename_stopped_half_way_keeps_every_photo_with_its_data() {
    let _faults = shared();
    let l = Lib::new(&["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg"]);
    let p = |rel: &str| l.path(rel);
    let ids = l.ids(&["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg"]);
    std::fs::rename(p("Lisbon/IMG_0001.jpg"), p("Lisbon/Trip-001.jpg")).unwrap();
    std::fs::rename(p("Lisbon/IMG_0002.jpg"), p("Lisbon/Trip-002.jpg")).unwrap();
    let items = (1..=3).map(|i| (p(&format!("Lisbon/IMG_{i:04}.jpg")), Some(p(&format!("Lisbon/Trip-{i:03}.jpg"))), State::Pending)).collect();
    l.plant(vec![op(3, Kind::Rename, Direction::Do, items)]);
    let s = l.reopen();
    shown(&s, ids[0], &p("Lisbon/Trip-001.jpg"), "The caption of IMG_0001.jpg");
    shown(&s, ids[1], &p("Lisbon/Trip-002.jpg"), "The caption of IMG_0002.jpg");
    shown(&s, ids[2], &p("Lisbon/IMG_0003.jpg"), "The caption of IMG_0003.jpg");
    assert_eq!(entry(&p("Lisbon"), "IMG_0001.jpg"), None);
    assert_eq!(l.journal()["ops"], json!([]));
}

/// Undoing a copy sends the copy to the trash: interrupted, the copy still there beside its source
/// is rolled back (it stays, and nothing is sent anywhere), never taken for a move to finish.
#[test]
fn an_interrupted_undo_of_a_copy_never_trashes_the_source() {
    let _faults = shared();
    let l = Lib::new(&["Lisbon/IMG_0001.jpg", "Porto/IMG_0001.jpg"]);
    let p = |rel: &str| l.path(rel);
    // The copy is byte for byte the source.
    std::fs::copy(p("Lisbon/IMG_0001.jpg"), p("Porto/IMG_0001.jpg")).unwrap();
    let bytes = std::fs::read(p("Lisbon/IMG_0001.jpg")).unwrap();
    l.plant(vec![op(4, Kind::Copy, Direction::Undo, vec![(p("Porto/IMG_0001.jpg"), Some(p("Lisbon/IMG_0001.jpg")), State::Pending)])]);
    let s = l.reopen();
    assert_eq!(std::fs::read(p("Lisbon/IMG_0001.jpg")).unwrap(), bytes);
    assert_eq!(std::fs::read(p("Porto/IMG_0001.jpg")).unwrap(), bytes);
    let outcomes: Vec<RepairOutcome> = s.file_op_repairs().map(|r| r.outcome).collect();
    assert_eq!(outcomes, [RepairOutcome::Back]);
    assert_eq!(l.journal()["ops"], json!([]));
}

/// Neither end there: reported as lost, nothing to do, the operation leaves the journal; an item
/// naming one folder at both ends (Move to New Folder's folder) has nothing to repair.
#[test]
fn lost_items_are_reported_and_a_made_folder_needs_nothing() {
    let _faults = shared();
    let l = Lib::new(&["Lisbon/IMG_0001.jpg"]);
    let p = |rel: &str| l.path(rel);
    std::fs::create_dir(p("New Folder")).unwrap();
    l.plant(vec![op(
        5,
        Kind::Move,
        Direction::Do,
        vec![(p("New Folder"), Some(p("New Folder")), State::Done), (p("Lisbon/GONE.jpg"), Some(p("Porto/GONE.jpg")), State::Pending)],
    )]);
    let s = l.reopen();
    let outcomes: Vec<RepairOutcome> = s.file_op_repairs().map(|r| r.outcome).collect();
    assert_eq!(outcomes, [RepairOutcome::Nothing, RepairOutcome::Lost]);
    assert!(p("New Folder").is_dir(), "the made folder stays (an empty folder is no library folder)");
    assert!(p("Lisbon/IMG_0001.jpg").exists());
    assert_eq!(l.journal()["ops"], json!([]));
}

/// A side that cannot be looked at (a folder this user may not read): the operation waits in the
/// journal and nothing changes; once it can be looked at, the next open rolls it forward, and the
/// photo, moved into a folder the library did not have yet, is shown there with its data.
#[cfg(unix)]
#[test]
fn an_operation_that_cannot_be_looked_at_waits_for_the_next_open() {
    let _faults = shared();
    use std::os::unix::fs::PermissionsExt;
    let l = Lib::new(&["Lisbon/IMG_0001.jpg", "Porto/PORTO_0001.jpg"]);
    let p = |rel: &str| l.path(rel);
    std::fs::create_dir(p("Locked")).unwrap();
    std::fs::rename(p("Lisbon/IMG_0001.jpg"), p("Locked/IMG_0001.jpg")).unwrap();
    let bytes = std::fs::read(p("Locked/IMG_0001.jpg")).unwrap();
    l.plant(vec![op(6, Kind::Move, Direction::Do, vec![(p("Lisbon/IMG_0001.jpg"), Some(p("Locked/IMG_0001.jpg")), State::Pending)])]);
    let set = |mode: u32| std::fs::set_permissions(p("Locked"), std::fs::Permissions::from_mode(mode)).unwrap();
    set(0o000);
    // Root ignores permissions: nothing to show then.
    if std::fs::read_dir(p("Locked")).is_ok() {
        set(0o755);
        return;
    }
    let s = l.reopen();
    set(0o755);
    let outcomes: Vec<RepairOutcome> = s.file_op_repairs().map(|r| r.outcome).collect();
    assert_eq!(outcomes, [RepairOutcome::Waits]);
    assert_eq!(s.file_ops_left().len(), 1, "it stays for the next open");
    assert_eq!(l.journal()["ops"].as_array().unwrap().len(), 1);
    assert!(s.file_job_events().any(|e| e.message.contains("wait for the next start")));
    assert_eq!(entry(&p("Lisbon"), "IMG_0001.jpg").unwrap()["caption"], "The caption of IMG_0001.jpg", "nothing carried");
    drop(s);
    let s = l.reopen();
    assert_eq!(s.file_op_repairs().map(|r| r.outcome).collect::<Vec<_>>(), [RepairOutcome::Forward]);
    let id = photo_at(&s, &p("Locked/IMG_0001.jpg")).expect("listed where it is");
    shown(&s, id, &p("Locked/IMG_0001.jpg"), "The caption of IMG_0001.jpg");
    assert_eq!(std::fs::read(p("Locked/IMG_0001.jpg")).unwrap(), bytes);
    assert_eq!(photo_at(&s, &p("Lisbon/IMG_0001.jpg")), None);
    assert_eq!(l.journal()["ops"], json!([]));
}

/// A rename in a folder's batch that fails after the file got its new name (the check after the
/// rename) goes back with the others, so the batch stays all or nothing: every photo keeps its
/// name, its data and its listing, and the next open has nothing to repair.
#[test]
fn a_batch_rename_whose_rename_fails_after_it_happened_puts_it_back_too() {
    let _faults = shared();
    let dir = tempfile::tempdir().unwrap();
    let lisbon = dir.path().join("Pictures").join("Lisbon");
    dalica_fs::create_dir_all(&lisbon).unwrap();
    for i in 1..=3u64 {
        dalica_fs::replace(&lisbon.join(format!("IMG_{i:04}.jpg")), &jpeg(i), Expect::Absent).unwrap();
    }
    let library = dir.path().join("library");
    let mut s = open(&library);
    s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
    s.wait_idle();
    let folder = s.library().folder_by_path(&lisbon).unwrap();
    let ids: Vec<_> = (1..=3).map(|i| s.library().photo_by_name(folder, &format!("IMG_{i:04}.jpg")).unwrap().id).collect();
    s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "Tram 28" })).unwrap();
    assert!(s.flush_metadata(PATIENCE));
    s.flush().unwrap();
    dalica_engine::cmd::rename::planted::fail_after_rename_of(&lisbon.join("IMG_0002.jpg"));
    s.execute("photo.rename", &json!({ "ids": ids, "name": "Trip" })).unwrap();
    assert!(s.wait_file_jobs(PATIENCE));
    s.flush().unwrap();
    let e = s.file_op_events().last().cloned().unwrap();
    assert!(!e.failures.is_empty(), "{e:?}");
    for (i, id) in ids.iter().enumerate() {
        let name = format!("IMG_{:04}.jpg", i + 1);
        assert_eq!(s.library().photo(*id).unwrap().name, name);
        assert!(lisbon.join(&name).exists(), "{name} is where the library shows it");
    }
    assert_eq!(s.library().info(ids[1]).caption, "Tram 28");
    assert_eq!(entry(&lisbon, "IMG_0002.jpg").unwrap()["caption"], "Tram 28");
    drop(s);
    let s = open(&library);
    assert!(!s.file_jobs_running(), "nothing to repair");
}
