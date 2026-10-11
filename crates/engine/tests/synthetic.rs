//! Synthetic libraries (`dalica_engine::synthetic`): the same seed gives the same library, the
//! 100 k-photo preset opens with every photo sketched, the plan has the promised shape, and bad
//! specs and occupied folders are refused without touching anything.
//!
//! Timings: `cargo test --release -p dalica-engine --test synthetic -- --nocapture` prints the
//! generation and open times of the 100 k-photo library.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dalica_core::{Clock, DateTime, SystemClock};
use dalica_engine::Session;
use dalica_engine::synthetic::{self, Spec, SyntheticError};
use dalica_library::Library;

fn clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

/// One photo as a test sees it: (id, folder id, folder path under `root`, name, size, width,
/// height, date, star).
type Row = (u64, u64, PathBuf, String, u64, u32, u32, Option<DateTime>, bool);
/// A [`Row`] without the folder id and star: what a scan of the files gives back.
type Scanned = (u64, PathBuf, String, u64, u32, u32, Option<DateTime>);
/// (sort date, name, id) of a folder's photos.
type Order<'a> = Vec<(Option<DateTime>, &'a str, u64)>;

/// Every photo by id, folder paths relative to `root`.
fn rows(lib: &Library, root: &Path) -> Vec<Row> {
    let folders: HashMap<u64, PathBuf> = lib.folders().map(|f| (f.id.get(), f.path.strip_prefix(root).unwrap().to_path_buf())).collect();
    lib.photos()
        .map(|p| (p.id.get(), p.folder.get(), folders[&p.folder.get()].clone(), p.name.clone(), p.size, p.width, p.height, p.date, p.star))
        .collect()
}

/// Checks the id rule in one pass: folders 1, 2, … newest first; photos 1, 2, … in light-table
/// order (folder by folder, each oldest first). Returns the folder sizes in tree order.
fn assert_ids_in_tree_order(lib: &Library) -> Vec<usize> {
    let mut by_folder: BTreeMap<u64, Order> = BTreeMap::new();
    for p in lib.photos() {
        by_folder.entry(p.folder.get()).or_default().push((p.sort_date(), p.name.as_str(), p.id.get()));
    }
    assert_eq!(by_folder.keys().copied().collect::<Vec<_>>(), (1..=lib.folders().count() as u64).collect::<Vec<_>>());
    let mut next = 1;
    let mut newer: Option<DateTime> = None;
    let mut sizes = Vec::new();
    for photos in by_folder.values_mut() {
        photos.sort();
        let date = photos[0].0.unwrap();
        if let Some(newer) = newer {
            assert!(date < newer, "folders are numbered newest first");
        }
        newer = Some(date);
        for (_, _, id) in photos.iter() {
            assert_eq!(*id, next, "photos are numbered in light-table order");
            next += 1;
        }
        sizes.push(photos.len());
    }
    sizes
}

#[test]
fn the_same_seed_gives_the_same_library() {
    let root = Path::new("/synthetic/photos");
    let spec = Spec::new(5_000, 120, 7);
    let a = synthetic::library_at(root, &spec).unwrap();
    let b = synthetic::library_at(root, &spec).unwrap();
    assert_eq!(a.model(), b.model());
    assert_eq!(a.photos().count(), 5_000);
    assert_eq!(a.folders().count(), 120);
    // Another seed is another library; another size another one again.
    let c = synthetic::library_at(root, &Spec { seed: 8, ..spec }).unwrap();
    assert_ne!(rows(&a, root), rows(&c, root));
    assert_ne!(a.sketches().map(|(_, s)| *s).collect::<Vec<_>>(), c.sketches().map(|(_, s)| *s).collect::<Vec<_>>());

    // Written and opened in two folders: the same rows and sketches under each folder.
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s1 = synthetic::open(d1.path(), &spec, clock()).unwrap();
    let s2 = synthetic::open(d2.path(), &spec, clock()).unwrap();
    let (r1, r2) = (synthetic::photos_dir(d1.path()), synthetic::photos_dir(d2.path()));
    assert_eq!(rows(s1.library(), &r1), rows(&a, root));
    assert_eq!(rows(s1.library(), &r1), rows(s2.library(), &r2));
    assert_eq!(s1.library().model().clone(), {
        // The opened model is the in-memory one with the folders under the session's root.
        let lib = synthetic::library(d1.path(), &spec).unwrap();
        lib.model().clone()
    });
    assert_eq!(s1.roots(), [r1]);
    assert!(s1.recovery().is_empty(), "{:?}", s1.recovery());
}

#[test]
fn a_100k_library_opens_with_every_photo_sketched() {
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec::PHOTOS_100K;
    let t = Instant::now();
    synthetic::write_index(dir.path(), &spec).unwrap();
    let generated = t.elapsed();
    let t = Instant::now();
    let session = Session::open(&synthetic::library_dir(dir.path()), clock()).unwrap();
    let opened = t.elapsed();
    let snap = std::fs::metadata(synthetic::library_dir(dir.path()).join(dalica_store::snapshot::FILE)).unwrap().len();
    eprintln!("synthetic 100 k photos in 1 k folders: generated and written in {generated:.2?}, opened in {opened:.2?}; index.snap {snap} bytes");

    let lib = session.library();
    assert!(session.recovery().is_empty(), "{:?}", session.recovery());
    assert_eq!(lib.photos().count(), 100_000);
    assert_eq!(lib.folders().count(), 1_000);
    assert_eq!(lib.sketch_count(), 100_000);
    for p in lib.photos() {
        let sketch = lib.sketch(p.id).unwrap_or_else(|| panic!("photo {} has no sketch", p.id));
        let decoded = dalica_thumbs::sketch::decode(sketch.as_bytes()).unwrap();
        let (aspect, want) = (decoded.aspect(), f64::from(p.width) / f64::from(p.height));
        assert!((aspect / want - 1.0).abs() < 0.06, "photo {}: sketch aspect {aspect:.3}, photo {want:.3}", p.id);
    }

    // The shape of the plan.
    let sizes = assert_ids_in_tree_order(lib);
    assert!(sizes.iter().all(|&n| n >= 1));
    let (least, most) = (*sizes.iter().min().unwrap(), *sizes.iter().max().unwrap());
    assert!(most >= 5 * 100 && least <= 20, "uneven folders: {least}..={most} photos");
    let aspects: BTreeSet<(u32, u32)> = lib.photos().map(|p| (p.width, p.height)).collect();
    assert_eq!(aspects.len(), 7, "{aspects:?}");
    let years: BTreeSet<u16> = lib.photos().filter_map(|p| p.date).map(|d| d.date().year()).collect();
    assert!(years.len() >= 10, "{years:?}");
    assert_eq!(years.last(), Some(&2026));
    let undated = lib.photos().filter(|p| p.date.is_none()).count();
    let starred = lib.photos().filter(|p| p.star).count();
    assert!((1_000..3_000).contains(&undated), "{undated} undated");
    assert!((2_500..4_200).contains(&starred), "{starred} starred");
    let mut names = BTreeSet::new();
    assert!(lib.photos().all(|p| names.insert(&p.name)), "names are unique across the library");
    let paths: BTreeSet<&PathBuf> = lib.folders().map(|f| &f.path).collect();
    assert_eq!(paths.len(), 1_000);
    let root = synthetic::photos_dir(dir.path());
    for f in lib.folders() {
        let rel = f.path.strip_prefix(&root).unwrap();
        assert_eq!(rel.components().count(), 2, "<year>/<date> <place>: {}", rel.display());
    }
}

#[test]
fn the_5k_folder_preset_has_5k_folders() {
    let spec = Spec::FOLDERS_5K;
    let lib = synthetic::library_at(Path::new("/synthetic"), &spec).unwrap();
    assert_eq!(lib.folders().count(), 5_000);
    assert_eq!(lib.photos().count(), 100_000);
    assert_eq!(lib.sketch_count(), 100_000);
    let sizes = assert_ids_in_tree_order(&lib);
    assert_eq!(sizes.iter().sum::<usize>(), 100_000);
    assert!(sizes.iter().all(|&n| n >= 1));
}

#[test]
fn edge_sizes_work() {
    let root = Path::new("/synthetic");
    let empty = synthetic::library_at(root, &Spec::new(0, 0, 1)).unwrap();
    assert_eq!((empty.photos().count(), empty.folders().count(), empty.sketch_count()), (0, 0, 0));
    let one = synthetic::library_at(root, &Spec::new(1, 1, 1)).unwrap();
    assert_eq!((one.photos().count(), one.folders().count(), one.sketch_count()), (1, 1, 1));
    // As many folders as photos: one photo each.
    let flat = synthetic::library_at(root, &Spec::new(300, 300, 1)).unwrap();
    assert_eq!(assert_ids_in_tree_order(&flat), vec![1; 300]);
    // One folder holds them all.
    let single = synthetic::library_at(root, &Spec::new(2_000, 1, 1)).unwrap();
    assert_eq!(assert_ids_in_tree_order(&single), [2_000]);
    let dir = tempfile::tempdir().unwrap();
    let s = synthetic::open(dir.path(), &Spec::new(0, 0, 1), clock()).unwrap();
    assert_eq!(s.library().photos().count(), 0);
}

#[test]
fn bad_specs_are_refused() {
    let root = Path::new("/synthetic");
    for spec in [
        Spec::new(synthetic::MAX_PHOTOS + 1, 10, 1),
        Spec::new(10, 11, 1),
        Spec::new(10, 0, 1),
        Spec::new(usize::MAX, usize::MAX, 1),
        Spec::new(synthetic::MAX_FOLDERS + 1, synthetic::MAX_FOLDERS + 1, 1),
    ] {
        assert!(matches!(spec.check(), Err(SyntheticError::Spec(_))), "{spec:?}");
        assert!(matches!(synthetic::library_at(root, &spec), Err(SyntheticError::Spec(_))), "{spec:?}");
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(synthetic::open(dir.path(), &spec, clock()), Err(SyntheticError::Spec(_))), "{spec:?}");
        assert!(matches!(synthetic::write_photos(dir.path(), &spec, 64), Err(SyntheticError::Spec(_))), "{spec:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing written for {spec:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec::new(10, 2, 1);
    for side in [0, synthetic::FILE_SIDE.0 - 1, synthetic::FILE_SIDE.1 + 1, u32::MAX] {
        assert!(matches!(synthetic::write_photos(dir.path(), &spec, side), Err(SyntheticError::Spec(_))), "{side}");
    }
    let too_many = Spec::new(synthetic::MAX_SCANNED_PHOTOS + 1, 10, 1);
    assert!(matches!(synthetic::write_photos(dir.path(), &too_many, 64), Err(SyntheticError::Spec(_))));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn an_existing_library_or_photo_folder_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec::new(50, 5, 3);
    drop(synthetic::open(dir.path(), &spec, clock()).unwrap());
    let snap = synthetic::library_dir(dir.path()).join(dalica_store::snapshot::FILE);
    let before = std::fs::read(&snap).unwrap();
    for other in [spec, Spec::new(60, 6, 4)] {
        assert!(matches!(synthetic::write_index(dir.path(), &other), Err(SyntheticError::Exists(_))));
        assert!(matches!(synthetic::open(dir.path(), &other, clock()), Err(SyntheticError::Exists(_))));
        assert!(matches!(synthetic::open_scanned(dir.path(), &other, 64, clock()), Err(SyntheticError::Exists(_))));
    }
    assert_eq!(std::fs::read(&snap).unwrap(), before);
    assert!(!synthetic::photos_dir(dir.path()).exists(), "an index-only library has no photo files");

    // A photo folder that is already there is left alone.
    let dir = tempfile::tempdir().unwrap();
    let photos = synthetic::photos_dir(dir.path());
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("mine.jpg"), b"not yours").unwrap();
    assert!(matches!(synthetic::write_photos(dir.path(), &spec, 64), Err(SyntheticError::Exists(_))));
    assert_eq!(std::fs::read_dir(&photos).unwrap().count(), 1);
    assert_eq!(std::fs::read(photos.join("mine.jpg")).unwrap(), b"not yours");
}

#[test]
fn the_scanned_case_writes_the_same_plan_as_files() {
    let spec = Spec::new(120, 6, 11);
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = Instant::now();
    let s1 = synthetic::open_scanned(d1.path(), &spec, 64, clock()).unwrap();
    eprintln!("synthetic scanned 120 photos at 64 px: written and scanned in {:.2?}", t.elapsed());
    let s2 = synthetic::open_scanned(d2.path(), &spec, 64, clock()).unwrap();
    assert!(s1.scan_warnings().is_empty(), "{:?}", s1.scan_warnings());
    let (r1, r2) = (synthetic::photos_dir(d1.path()), synthetic::photos_dir(d2.path()));
    assert_eq!(s1.roots(), std::slice::from_ref(&r1));

    // Paths, names and dates come from the plan: the index-only library of the same
    // spec lists the same files. Sizes are the files' own, at the plan's aspect.
    let plan = synthetic::library_at(&r1, &spec).unwrap();
    let key = |lib: &Library, root: &Path| -> BTreeSet<(PathBuf, String, Option<DateTime>)> {
        rows(lib, root).into_iter().map(|r| (r.2, r.3, r.7)).collect()
    };
    assert_eq!(key(s1.library(), &r1), key(&plan, &r1));
    for p in s1.library().photos() {
        let planned = plan.photos().find(|q| q.name == p.name).unwrap();
        assert_eq!(p.width.max(p.height), 64, "{}", p.name);
        let (a, b) = (f64::from(p.width) / f64::from(p.height), f64::from(planned.width) / f64::from(planned.height));
        assert!((a / b - 1.0).abs() < 0.05, "{}: {a:.3} against {b:.3}", p.name);
    }
    // The same seed scans to the same library (file times aside).
    let strip = |lib: &Library, root: &Path| -> Vec<Scanned> { rows(lib, root).into_iter().map(|r| (r.0, r.2, r.3, r.4, r.5, r.6, r.7)).collect() };
    assert_eq!(strip(s1.library(), &r1), strip(s2.library(), &r2));
}
