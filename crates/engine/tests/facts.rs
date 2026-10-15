//! Search facts read on scan (`roadmap.md` M9.7; `organizing.md` §8) while the session runs:
//! after a scan each photo's camera and GPS location are in the library and its first-indexed time
//! is the session's clock, with no undo step; a file rewritten with another camera model updates
//! them, through the verification pass and through the watcher; a photo the watcher adds is
//! indexed too; a rebuild reads the facts again and records no first-indexed time. The owner
//! thread is marked as the UI thread: it makes no `fs` call. The test's own file changes run on a
//! helper thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::off_owner;
use dalica_core::{Clock, PhotoId, Timestamp, VirtualClock};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_library::Facts;
use dalica_meta::{Changes, Place, WriteOptions};
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(60);

fn time(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(981_173_106 + n)
}

/// A JPEG from `make` / `model`'s camera, located in Lisbon when `place`.
fn camera_jpeg(seed: u64, make: &str, model: &str, place: bool) -> Vec<u8> {
    let exif = dalica_scenes::Exif { make: Some(make.into()), model: Some(model.into()), ..dalica_scenes::Exif::default() };
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 85, Some(&exif)).unwrap();
    if !place {
        return jpeg;
    }
    let changes = Changes { place: Some(Place::new(38.71, -9.14)), ..Changes::default() };
    dalica_meta::write_owned(&jpeg, &changes, &WriteOptions::default()).unwrap()
}

fn plain_jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 85, None).unwrap()
}

/// Writes `bytes` at `path` with modification time `time(n)`, on a helper thread.
fn put(path: &Path, bytes: Vec<u8>, n: u64) {
    let path = path.to_path_buf();
    off_owner(move || {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(time(n)).unwrap();
    });
}

fn facts(make: &str, model: &str, geotagged: bool) -> Facts {
    Facts { make: make.into(), model: model.into(), video: false, geotagged }
}

struct Fx {
    _tmp: tempfile::TempDir,
    library: PathBuf,
    /// The watched root, holding `Lisbon` with `camera.jpg` (a camera and a location) and
    /// `plain.jpg` (no EXIF).
    root: PathBuf,
    clock: Arc<VirtualClock>,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Pictures");
        put(&root.join("Lisbon").join("camera.jpg"), camera_jpeg(1, "Dalica", "Scenes 1", true), 0);
        put(&root.join("Lisbon").join("plain.jpg"), plain_jpeg(2), 0);
        Fx { library: tmp.path().join("library"), root, _tmp: tmp, clock: Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000))) }
    }

    fn lisbon(&self) -> PathBuf {
        self.root.join("Lisbon")
    }

    fn open(&self, mode: WatchMode) -> Session {
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&self.library, self.clock.clone(), &options).unwrap();
        s.set_watching(mode);
        s
    }

    /// Drives the polling watcher on virtual time: a few one-second ticks, each waited for.
    fn settle(&self, s: &mut Session) {
        for _ in 0..4 {
            self.clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(PATIENCE), "the watcher answered");
            s.wait_idle();
        }
    }
}

fn photo(s: &Session, folder: &Path, name: &str) -> PhotoId {
    let f = s.library().folder_by_path(folder).unwrap();
    s.library().photos_in(f).into_iter().find(|p| p.name == name).unwrap().id
}

#[test]
fn after_a_scan_the_facts_are_in_the_library_and_a_rewritten_file_updates_them_with_no_undo_step() {
    let fx = Fx::new();
    let mut s = fx.open(WatchMode::Off);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let added_at = fx.clock.now();
    s.execute("folders.add", &json!({ "path": fx.root })).unwrap();
    s.wait_idle();
    let lisbon = fx.lisbon();
    let (camera, plain) = (photo(&s, &lisbon, "camera.jpg"), photo(&s, &lisbon, "plain.jpg"));
    assert_eq!(s.library().facts(camera), &facts("Dalica", "Scenes 1", true));
    assert_eq!(s.library().facts(plain), &Facts::default());
    for id in [camera, plain] {
        assert_eq!(s.library().first_indexed(id), Some(added_at), "first indexed by this scan");
    }
    assert!(s.undo_stack().is_empty(), "facts and times are no step");

    // Rewritten with another camera model (and the location gone): the verification pass reads it
    // again; the first-indexed time stays.
    fx.clock.advance(Duration::from_secs(3600));
    put(&lisbon.join("camera.jpg"), camera_jpeg(1, "Dalica", "Scenes 2", false), 10);
    s.start_verification();
    s.wait_idle();
    assert_eq!(s.library().facts(camera), &facts("Dalica", "Scenes 2", false));
    assert_eq!(s.library().first_indexed(camera), Some(added_at));
    assert!(s.undo_stack().is_empty(), "the undo stack is unchanged");

    // Unchanged files: a scan reads nothing again and changes nothing.
    let model = s.library().model().clone();
    s.start_verification();
    s.wait_idle();
    assert_eq!(s.library().model(), &model);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
}

#[test]
fn the_watcher_brings_the_facts_of_rewritten_and_new_files_and_indexes_new_photos() {
    let fx = Fx::new();
    let mut s = fx.open(WatchMode::Polling);
    s.execute("folders.add", &json!({ "path": fx.root })).unwrap();
    s.wait_idle();
    let lisbon = fx.lisbon();
    let camera = photo(&s, &lisbon, "camera.jpg");
    let first = s.library().first_indexed(camera);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    fx.settle(&mut s);
    assert!(s.watched_roots().iter().any(|(r, _)| *r == fx.root), "{:?}", s.watched_roots());

    fx.clock.advance(Duration::from_secs(3600));
    let later = fx.clock.now();
    put(&lisbon.join("camera.jpg"), camera_jpeg(1, "Other", "Model 7", true), 10);
    put(&lisbon.join("new.jpg"), camera_jpeg(3, "NIKON CORPORATION", "NIKON Z 6", false), 10);
    fx.settle(&mut s);
    assert_eq!(s.library().facts(camera), &facts("Other", "Model 7", true));
    assert_eq!(s.library().first_indexed(camera), first, "a changed file is not indexed again");
    let new = photo(&s, &lisbon, "new.jpg");
    assert_eq!(s.library().facts(new), &facts("NIKON CORPORATION", "NIKON Z 6", false));
    let indexed = s.library().first_indexed(new).unwrap();
    assert!(indexed >= later, "indexed when the watcher found it");
    assert!(s.undo_stack().is_empty());
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
}

/// A rebuild reads every photo's facts again and records no first-indexed time (after a rebuild
/// only the file's time counts for Recently Updated), whether it builds its model to the side or,
/// from an index that cannot be used at open, fills the empty one in place.
#[test]
fn a_rebuild_reads_the_facts_again_and_records_no_first_indexed_time() {
    let fx = Fx::new();
    let mut s = fx.open(WatchMode::Off);
    s.execute("folders.add", &json!({ "path": fx.root })).unwrap();
    s.wait_idle();
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let lisbon = fx.lisbon();
    let camera = photo(&s, &lisbon, "camera.jpg");
    assert_eq!(s.library().facts(camera), &facts("Dalica", "Scenes 1", true));
    assert_eq!(s.library().first_indexed_all().count(), 0, "a rebuild records no first-indexed time");
    s.flush().unwrap();
    drop(s);

    // The index damaged beyond use: the library opens empty and rebuilds in place.
    let index = fx.library.clone();
    off_owner(move || {
        let _ = std::fs::remove_file(index.join("index.snap"));
        std::fs::write(index.join("index.log"), b"not a journal at all").unwrap();
    });
    let mut s = fx.open(WatchMode::Off);
    s.wait_idle();
    let camera = photo(&s, &lisbon, "camera.jpg");
    assert_eq!(s.library().facts(camera), &facts("Dalica", "Scenes 1", true));
    assert_eq!(s.library().photos().count(), 2);
    assert_eq!(s.library().first_indexed_all().count(), 0, "nor does one filling the model in place");
}
