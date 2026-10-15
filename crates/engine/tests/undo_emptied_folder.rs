//! Undo of a file operation that emptied a folder, after a rescan took the empty folder out of the
//! library (found by `cargo xtask accept M9`, M9.25): a folder that holds no photos any more leaves
//! the library at the watcher's next rescan, so the undo's library change named a folder the model
//! no longer had ("the files were changed, but the library could not follow: no folder 2"); the
//! files went back on disk and the next rescan imported them as new photos, with new ids. The
//! folder now comes back with its id and what was said about it, and the photos with theirs.
//!
//! On temporary folders with a polling watcher on a virtual clock (as `lessons_m9.rs`); the delete
//! case uses the real OS trash where the system restores from it, and puts nothing back by hand
//! (its undo does).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dalica_core::{FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::cmd::delete::RESTORES;
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, FileOpEvent, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// One test at a time: they use the real trash and the watcher's sweeps.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()
}

/// A library watching `Pictures` (`Lisbon` with two photos, `Porto` with one) on a virtual clock,
/// the watcher polling.
struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    s: Session,
}

impl Lib {
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Pictures");
        for (i, rel) in ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Porto/PORTO_0001.jpg"].iter().enumerate() {
            let path = rel.split('/').fold(pictures.clone(), |p, c| p.join(c));
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &jpeg(i as u64), Expect::Absent).unwrap();
        }
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
        let library = dir.path().join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&library, clock.clone(), &options).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": pictures })).unwrap();
        s.wait_idle();
        s.set_watching(WatchMode::Polling);
        let mut l = Lib { dir, clock, s };
        l.settle_watcher();
        l
    }

    fn path(&self, folder: &str) -> PathBuf {
        self.dir.path().join("Pictures").join(folder)
    }

    fn folder(&self, folder: &str) -> Option<FolderId> {
        self.s.library().folder_by_path(&self.path(folder))
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        self.s.library().photo_by_name(self.folder(folder).unwrap(), name).unwrap().id
    }

    /// Where photo `id` is: (its folder's path, its name).
    fn where_is(&self, id: PhotoId) -> Option<(PathBuf, String)> {
        let p = self.s.library().photo(id)?;
        Some((self.s.library().folder(p.folder)?.path.clone(), p.name.clone()))
    }

    /// Lets the polling watcher sweep a few times on the virtual clock and applies what it found.
    fn settle_watcher(&mut self) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(self.s.wait_watching(Duration::from_secs(60)), "the watcher answered");
            self.s.wait_idle();
        }
    }

    /// Runs `id`, waits for its file job and what it wrote; the operation's event, which must be
    /// clean.
    fn run(&mut self, id: &str, p: Value) -> FileOpEvent {
        let before = self.s.file_op_events().count();
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e:?}"));
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        self.s.wait_idle();
        self.s.flush().unwrap();
        let events: Vec<FileOpEvent> = self.s.file_op_events().cloned().collect();
        assert!(events.len() > before, "{id} ran a file operation");
        let e = events.last().cloned().unwrap();
        assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{id}: {e:?}");
        e
    }
}

fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    std::thread::scope(|s| s.spawn(move || path.exists()).join().unwrap())
}

/// Both photos of `Lisbon` moved to `Porto`: the watcher's rescan takes the emptied `Lisbon` out
/// of the library; Edit ▸ Undo moves the files back and brings `Lisbon` back with its id and its
/// description, and the photos with their ids, caption and album place; redo and undo again.
#[test]
fn undoing_a_move_that_emptied_a_folder_after_a_rescan_brings_the_folder_and_the_photos_back() {
    let _serial = serial();
    let mut l = Lib::new();
    let (lisbon, porto) = (l.folder("Lisbon").unwrap(), l.folder("Porto").unwrap());
    let ids = [l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg")];
    l.s.execute("folder.info.set", &json!({ "id": lisbon, "description": "Tram 28" })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Alfama" })).unwrap();
    let album = l.s.execute("album.create", &json!({ "name": "Trip", "ids": [ids[1], ids[0]] })).unwrap()["album"].clone();
    let (lisbon_path, porto_path) = (l.path("Lisbon"), l.path("Porto"));

    l.run("photo.moveTo", json!({ "ids": ids, "folder": porto, "confirm": true }));
    l.settle_watcher();
    assert_eq!(l.folder("Lisbon"), None, "the emptied folder left the library at the rescan");
    assert!(exists(&lisbon_path), "it is still on disk");

    let check_home = |l: &mut Lib, case: &str| {
        assert_eq!(l.folder("Lisbon"), Some(lisbon), "{case}: Lisbon is back with its id");
        assert_eq!(l.s.library().folder_info(lisbon).description, "Tram 28", "{case}: and its description");
        for (id, name) in ids.iter().zip(["IMG_0001.jpg", "IMG_0002.jpg"]) {
            assert_eq!(l.where_is(*id), Some((lisbon_path.clone(), name.to_string())), "{case}: {name} keeps its id");
            assert!(exists(&lisbon_path.join(name)) && !exists(&porto_path.join(name)), "{case}: {name} on disk");
        }
        assert_eq!(l.s.library().info(ids[0]).caption, "Alfama", "{case}: the caption");
        let order = l.s.execute("library.query", &json!({ "section": format!("album:{}", album.as_str().unwrap()) })).unwrap();
        let order: Vec<u64> = order["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
        assert_eq!(order, [ids[1].get(), ids[0].get()], "{case}: the album's order");
        assert_eq!(l.s.library().photos().count(), 3, "{case}: no photo new or lost");
    };
    l.run("edit.undo", json!({}));
    check_home(&mut l, "undo");
    l.settle_watcher();
    check_home(&mut l, "undo, after the next rescan");

    l.run("edit.redo", json!({}));
    l.settle_watcher();
    assert_eq!(l.folder("Lisbon"), None, "redo: emptied again, gone at the rescan");
    assert_eq!(l.where_is(ids[0]).map(|w| w.0), Some(porto_path.clone()));
    l.run("edit.undo", json!({}));
    l.settle_watcher();
    check_home(&mut l, "undo again");
}

/// Both photos of `Lisbon` deleted to the trash: the rescan takes the emptied folder out; Edit ▸
/// Undo restores them from the trash into `Lisbon`, back with its id, and the photos with theirs.
/// Where the system cannot restore from its trash, the undo is disabled anyway (`delete.rs`).
#[test]
fn undoing_a_delete_that_emptied_a_folder_after_a_rescan_brings_the_folder_back() {
    if !RESTORES {
        return;
    }
    let _serial = serial();
    let mut l = Lib::new();
    let lisbon = l.folder("Lisbon").unwrap();
    let ids = [l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg")];
    l.s.execute("folder.info.set", &json!({ "id": lisbon, "description": "Tram 28" })).unwrap();
    l.run("photo.delete", json!({ "ids": ids, "confirm": true }));
    l.settle_watcher();
    assert_eq!(l.folder("Lisbon"), None, "the emptied folder left the library at the rescan");
    l.run("edit.undo", json!({}));
    l.settle_watcher();
    assert_eq!(l.folder("Lisbon"), Some(lisbon), "Lisbon is back with its id");
    assert_eq!(l.s.library().folder_info(lisbon).description, "Tram 28");
    for (id, name) in ids.iter().zip(["IMG_0001.jpg", "IMG_0002.jpg"]) {
        assert_eq!(l.where_is(*id).map(|w| w.1), Some(name.to_string()), "{name} keeps its id");
    }
    assert_eq!(l.s.library().photos().count(), 3);
}
