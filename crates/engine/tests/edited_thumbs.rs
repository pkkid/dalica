//! Edits shown on every thumbnail (`roadmap.md` M7.9; `architecture.md` §6.3–§6.5, `editing.md`
//! §9) in a session on a temporary library: a recipe change from either history makes the photo's
//! 128 and 256 entries and its sketch again, and the new entry is `edit::render` at that size; undo
//! brings the unedited picture back; a rotation remakes nothing; a change nobody hands over is
//! caught up when the thumbnail is asked for; a new version of one operation remakes only the
//! photos that use it; an unedited photo on screen still shows its embedded thumbnail first, an
//! edited one never does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_edit::{Recipe, Target};
use dalica_engine::thumbs::{ScanHook, Size};
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_image::Limits;
use dalica_scenes::{Exif, Scene};
use dalica_thumbs::Rgba8;
use serde_json::json;

const W: u32 = 640;
const H: u32 = 480;

fn write(path: &Path, bytes: &[u8]) {
    dalica_fs::replace(path, bytes, Expect::Absent).unwrap();
}

/// `n` photos `IMG_0001.jpg`… of `W × H` (with an embedded thumbnail of `exif` pixels, if any) in
/// `dir/Lisbon`.
fn photos(dir: &Path, n: usize, exif: Option<u32>) -> PathBuf {
    let folder = dir.join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    let exif = exif.map(|t| Exif { thumbnail: Some(t), ..Exif::default() });
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&Scene::new(40 + i as u64, W, H), 90, exif.as_ref()).unwrap();
        write(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes);
    }
    folder
}

fn open(library: &Path, workers: usize) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers, ..ThumbOptions::default() } };
    Session::open_with(library, Arc::new(SystemClock), &options).unwrap()
}

/// A session watching `n` photos, scanned; ids in name order.
fn setup(n: usize) -> (tempfile::TempDir, Session, Vec<PhotoId>) {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), n, None);
    let mut s = open(&dir.path().join("library"), 2);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = ids(&s);
    (dir, s, ids)
}

fn ids(s: &Session) -> Vec<PhotoId> {
    let mut v: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    v.sort();
    v.into_iter().map(|(_, id)| id).collect()
}

fn path_of(s: &Session, id: PhotoId) -> PathBuf {
    let p = s.library().photo(id).unwrap();
    s.library().folder(p.folder).unwrap().path.join(&p.name)
}

/// The image photo `id` draws in a cell of `need` pixels once everything asked for has landed.
fn settled(s: &mut Session, id: PhotoId, need: u32) -> (Size, Arc<Rgba8>) {
    for _ in 0..4 {
        if let Thumb::Image { size, image, covers: true } = s.thumbnail(id, need) {
            return (size, image);
        }
        s.wait_idle();
    }
    panic!("photo {id} has no covering thumbnail: {:?}", s.thumbnail(id, need));
}

/// `edit::render` of photo `id`'s file with `edits` (its operation strings) fitted to `side`.
fn oracle(s: &Session, id: PhotoId, edits: &[&str], side: u32) -> Rgba8 {
    let full = dalica_image::decode(&std::fs::read(path_of(s, id)).unwrap(), &Limits::default()).unwrap();
    let recipe = Recipe::parse(edits).unwrap();
    dalica_edit::render(dalica_edit::Source::upright(&full), &recipe, Target::Fit { width: side, height: side }).unwrap()
}

/// Mean absolute difference per channel of two images of one size.
fn difference(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let sum: u64 = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>()).sum();
    sum as f64 / (f64::from(a.width) * f64::from(a.height) * 3.0)
}

fn apply(s: &mut Session, id: PhotoId, op: &str) {
    s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
}

#[test]
fn a_recipe_change_remakes_the_photos_entries_and_sketch_as_the_renderer_draws_them() {
    let (_dir, mut s, ids) = setup(2);
    let (a, b) = (ids[0], ids[1]);
    let before = settled(&mut s, a, 256).1;
    let other = settled(&mut s, b, 256).1;
    let start = s.thumbnail_stats();

    // Auto Fix from the photo's own history: the hook hands the change over, the entries are made
    // dead and made again at once.
    apply(&mut s, a, "autofix()");
    assert_eq!(s.update_thumbnail_recipes(), 1);
    let queued = s.thumbnail_stats();
    assert_eq!(queued.remade, start.remade + 1);
    assert!(queued.jobs >= 1, "its scan is queued");
    s.wait_idle();
    let made = s.thumbnail_stats();
    assert_eq!(made.decodes, start.decodes + 1, "one decode made the 128 and 256 entries");
    assert_eq!(made.sketches, start.sketches + 1, "and the sketch");
    assert!(made.forgotten > start.forgotten, "the old entries are dead");
    for (side, size) in [(128, Size::S128), (256, Size::S256)] {
        let (got, image) = settled(&mut s, a, side);
        assert_eq!(got, size);
        assert!(!Arc::ptr_eq(&image, &before), "{size:?}: a new image");
        let d = difference(&image, &oracle(&s, a, &["autofix()"], side));
        assert!(d < 3.0, "{size:?}: the entry is edit::render at that size (mean difference {d:.2})");
    }
    assert!(s.library().sketch(a).is_some());
    assert!(Arc::ptr_eq(&settled(&mut s, b, 256).1, &other), "the other photo is untouched");

    // Undo in the photo's history: the unedited picture again.
    s.execute("photo.history.undo", &json!({ "id": a })).unwrap();
    assert_eq!(s.update_thumbnail_recipes(), 1);
    s.wait_idle();
    let undone = settled(&mut s, a, 256).1;
    assert!(difference(&undone, &before) < 0.01, "undo: the unedited thumbnail");
    assert_eq!(s.thumbnail_stats().remade, start.remade + 2);

    // A rotation, from the library: the same pixels, turned by the cells; nothing is made again.
    let still = s.thumbnail_stats();
    s.execute("photo.rotate.cw", &json!({ "ids": [a] })).unwrap();
    s.update_thumbnail_recipes();
    s.wait_idle();
    assert_eq!(s.library().info(a).edits, vec!["rotate(90)".to_string()]);
    assert!(Arc::ptr_eq(&settled(&mut s, a, 256).1, &undone), "the cached image");
    let after = s.thumbnail_stats();
    assert_eq!((after.remade, after.decodes), (still.remade, still.decodes));
}

#[test]
fn a_change_nobody_hands_over_is_caught_up_when_the_thumbnail_is_asked_for() {
    let (_dir, mut s, ids) = setup(1);
    let a = ids[0];
    let before = settled(&mut s, a, 256).1;
    apply(&mut s, a, "autocolor()");
    // No `update_thumbnail_recipes`: the request itself notices the new recipe.
    assert!(matches!(s.thumbnail(a, 256), Thumb::Sketch(_)), "the old image is no longer answered");
    assert_eq!(s.thumbnail_stats().remade, 1);
    s.wait_idle();
    let after = settled(&mut s, a, 256).1;
    assert!(!Arc::ptr_eq(&after, &before));
    assert!(difference(&after, &oracle(&s, a, &["autocolor()"], 256)) < 3.0);
    // The hook's later report finds nothing left to do.
    s.update_thumbnail_recipes();
    assert_eq!(s.thumbnail_stats().remade, 1);
}

fn bumped(name: &str) -> Option<u32> {
    dalica_edit::ops::version(name).map(|v| if name == "autofix" { v + 1 } else { v })
}

#[test]
fn a_new_version_of_one_operation_remakes_only_the_photos_that_use_it() {
    let (_dir, mut s, ids) = setup(4);
    apply(&mut s, ids[0], "autofix()");
    apply(&mut s, ids[1], "autocolor()");
    s.execute("photo.rotate.cw", &json!({ "ids": [ids[3]] })).unwrap();
    s.update_thumbnail_recipes();
    s.wait_idle();
    for &id in &ids {
        settled(&mut s, id, 256);
    }
    let start = s.thumbnail_stats();

    s.set_thumbnail_versions(Some(bumped));
    for &id in &ids {
        settled(&mut s, id, 256);
    }
    let after = s.thumbnail_stats();
    assert_eq!(after.decodes, start.decodes + 1, "only the Auto Fix photo is drawn again; the rest are read from the cache");
    assert!(after.loads > start.loads);
    assert!(difference(&settled(&mut s, ids[0], 256).1, &oracle(&s, ids[0], &["autofix()"], 256)) < 3.0);

    // Back to edit's versions: the entries made before are still there.
    s.set_thumbnail_versions(None);
    for &id in &ids {
        settled(&mut s, id, 256);
    }
    assert_eq!(s.thumbnail_stats().decodes, after.decodes);
}

/// Holds scan jobs until opened.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn hook(&self) -> ScanHook {
        let gate = self.clone();
        Arc::new(move |_| {
            let (lock, cv) = &*gate.0;
            let g = lock.lock().unwrap();
            let _ = cv.wait_timeout_while(g, Duration::from_secs(60), |open| !*open).unwrap();
        })
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}

#[test]
fn only_an_unedited_photo_shows_its_embedded_thumbnail_while_its_scan_runs() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 2, Some(160));
    // Both scans wait at the gate; a third worker runs the embedded-thumbnail job.
    let mut s = open(&dir.path().join("library"), 3);
    let gate = Gate::default();
    s.set_thumbnail_scan_hook(Some(gate.hook()));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    let start = Instant::now();
    while s.library().photos().count() < 2 {
        s.poll();
        assert!(start.elapsed() < Duration::from_secs(60), "the scan did not find the photos");
        std::thread::sleep(Duration::from_millis(1));
    }
    let ids = ids(&s);
    let (edited, plain) = (ids[0], ids[1]);
    apply(&mut s, edited, "autofix()");
    s.update_thumbnail_recipes();
    // Until the unedited photo's embedded thumbnail is shown.
    let quick = loop {
        s.poll();
        let _ = s.thumbnail(edited, 144);
        if let Thumb::Image { image, .. } = s.thumbnail(plain, 144) {
            break image;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "no embedded thumbnail");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(quick.width, 160, "the embedded thumbnail came first");
    assert!(s.thumbnail_stand_in(plain));
    // A moment more for the edited photo: it never gets one.
    for _ in 0..50 {
        s.poll();
        assert!(!matches!(s.thumbnail(edited, 144), Thumb::Image { .. }), "an edited photo never shows its file's own small copy");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!s.thumbnail_stand_in(edited));
    assert_eq!(s.thumbnail_stats().quick, 1, "one embedded thumbnail, the unedited photo's");
    gate.open();
    s.wait_idle();
    let (_, image) = settled(&mut s, edited, 144);
    assert_eq!(image.width, 256);
    assert!(difference(&image, &oracle(&s, edited, &["autofix()"], 256)) < 3.0);
}
