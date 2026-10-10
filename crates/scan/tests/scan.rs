//! One-shot scans of temporary trees (`architecture.md` §11): nested folders, skip rules, content
//! sniffing, and a verification pass that emits exactly the ops for what changed on disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use dalica_library::{Library, Op};
use dalica_scan::{diff, scan};
use dalica_scenes::{Exif, Scene, demo_library, jpeg, png};

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn camera(seed: u64, o: u8) -> Vec<u8> {
    let exif = Exif { orientation: o, date_time_original: "2026-06-04T19:42:10".parse().ok(), ..Exif::default() };
    jpeg(&Scene::new(seed, 120, 80), 80, Some(&exif)).unwrap()
}

/// A tree with nested folders, things to skip and a lying extension.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    put(&r.join("top.jpg"), &camera(1, 1));
    put(&r.join("a/1.jpg"), &camera(2, 6));
    put(&r.join("a/b/2.png"), &png(&Scene::new(3, 64, 48)).unwrap());
    put(&r.join("a/b/really-a-png.jpg"), &png(&Scene::new(4, 40, 30)).unwrap());
    put(&r.join("a/notes.txt"), b"not a photo, and long enough to be sniffed as nothing at all");
    put(&r.join("a/fake.jpg"), b"text pretending to be a JPEG file, long enough to sniff");
    put(&r.join(".hidden/3.jpg"), &camera(5, 1));
    put(&r.join("a/.dalica-originals/1.jpg"), &camera(6, 1));
    put(&r.join("@eaDir/thumb.jpg"), &camera(7, 1));
    put(&r.join("a/.dot.jpg"), &camera(8, 1));
    std::fs::create_dir_all(r.join("empty")).unwrap();
    dir
}

fn names(lib: &Library, root: &Path) -> Vec<String> {
    let mut v: Vec<String> = lib
        .photos()
        .map(|p| {
            let folder = &lib.folder(p.folder).unwrap().path;
            let rel = folder.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
            [rel, vec![p.name.clone()]].concat().join("/")
        })
        .collect();
    v.sort();
    v
}

fn apply_all(lib: &mut Library, ops: Vec<Op>) {
    for op in ops {
        lib.apply(op).unwrap();
    }
}

#[test]
fn finds_nested_folders_and_skips_what_it_should() {
    let dir = tree();
    let s = scan(&[dir.path().to_path_buf()], None);
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    assert_eq!(names(&lib, dir.path()), ["a/1.jpg", "a/b/2.png", "a/b/really-a-png.jpg", "top.jpg"]);
    assert_eq!(lib.folders().count(), 3);
    // Size as displayed: orientation 6 swaps the stored 80 × 120 back to 120 × 80.
    let one = lib.photos().find(|p| p.name == "1.jpg").unwrap();
    assert_eq!((one.width, one.height), (120, 80));
    assert_eq!(one.date.unwrap().to_string(), "2026-06-04T19:42:10");
    assert_eq!(lib.photos().find(|p| p.name == "really-a-png.jpg").unwrap().width, 40);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    // A second pass over an unchanged tree finds nothing to do.
    assert_eq!(diff(&mut lib, &scan(&[dir.path().to_path_buf()], None)), []);
}

#[test]
fn verification_emits_exactly_the_ops_for_what_changed() {
    let dir = tree();
    let r = dir.path().to_path_buf();
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    let id_of = |lib: &Library, name: &str| lib.photos().find(|p| p.name == name).unwrap().id;
    let (one, two, top) = (id_of(&lib, "1.jpg"), id_of(&lib, "2.png"), id_of(&lib, "top.jpg"));
    lib.apply(Op::SetStars { stars: vec![(one, true)] }).unwrap();

    // Add, delete, rename (keeps size and time) and rewrite (new size).
    put(&r.join("a/new.jpg"), &camera(9, 3));
    std::fs::remove_file(r.join("a/b/2.png")).unwrap();
    std::fs::rename(r.join("a/1.jpg"), r.join("a/renamed.jpg")).unwrap();
    put(&r.join("top.jpg"), &camera(10, 8));

    let s = scan(std::slice::from_ref(&r), None);
    let before_next = lib.clone().next_photo_id();
    let ops = diff(&mut lib, &s);
    let a = lib.folder_by_path(&r.join("a")).unwrap();
    let kinds: Vec<String> = ops
        .iter()
        .map(|op| match op {
            Op::RemovePhotos { ids } => format!("remove {ids:?}"),
            Op::UpdatePhotos { photos } => format!("update {:?}", photos.iter().map(|p| (p.id, p.name.as_str(), p.star)).collect::<Vec<_>>()),
            Op::AddPhotos { photos } => format!("add {:?}", photos.iter().map(|p| (p.id, p.folder, p.name.as_str())).collect::<Vec<_>>()),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            format!("remove {:?}", [two]),
            format!("update {:?}", [(top, "top.jpg", false), (one, "renamed.jpg", true)]),
            format!("add {:?}", [(before_next, a, "new.jpg")]),
        ]
    );
    apply_all(&mut lib, ops);
    assert_eq!(names(&lib, &r), ["a/b/really-a-png.jpg", "a/new.jpg", "a/renamed.jpg", "top.jpg"]);
    // The rename kept the photo's id and star; the rewrite took the new orientation's size.
    assert!(lib.photo(one).unwrap().star);
    assert_eq!((lib.photo(top).unwrap().width, lib.photo(top).unwrap().height), (120, 80));
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), []);

    // A folder that lost its last photo is removed.
    std::fs::remove_file(r.join("a/b/really-a-png.jpg")).unwrap();
    let b = lib.folder_by_path(&r.join("a/b")).unwrap();
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), [Op::RemoveFolder { id: b }]);
}

#[test]
fn an_offline_root_removes_nothing() {
    let dir = tree();
    let r = dir.path().join("card");
    put(&r.join("x.jpg"), &camera(11, 1));
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    std::fs::rename(&r, dir.path().join("card-unplugged")).unwrap();
    let s = scan(std::slice::from_ref(&r), None);
    assert!(s.walked.is_empty() && s.warnings.len() == 1, "{s:?}");
    assert_eq!(diff(&mut lib, &s), []);
}

#[test]
fn stars_come_from_the_folder_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("p/x.jpg"), &camera(12, 1));
    put(&dir.path().join("p/y.jpg"), &camera(13, 1));
    put(&dir.path().join("p/.dalica.json"), br#"{"photos": {"y.jpg": {"star": true}, "gone.jpg": {"star": true}}}"#);
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(&[dir.path().to_path_buf()], None));
    apply_all(&mut lib, ops);
    assert_eq!(lib.starred().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["y.jpg"]);
    // A damaged sidecar is reported, not fatal.
    put(&dir.path().join("p/.dalica.json"), b"{oops");
    let s = scan(&[dir.path().to_path_buf()], None);
    assert_eq!(s.folders[0].photos.len(), 2);
    assert!(s.warnings.iter().any(|w| w.path.ends_with(".dalica.json")), "{:?}", s.warnings);
}

#[test]
fn a_set_cancel_flag_stops_the_scan() {
    let dir = tree();
    let cancel = AtomicBool::new(true);
    let s = scan(&[dir.path().to_path_buf()], Some(&cancel));
    assert!(s.folders.is_empty());
    assert!(s.warnings.iter().any(|w| w.reason.contains("Cancelled")), "{:?}", s.warnings);
}

#[test]
fn scanning_1000_scenes_jpegs() {
    let dir = tempfile::tempdir().unwrap();
    let files = demo_library(10, 1000, 7).unwrap();
    for f in &files {
        put(&dir.path().join(&f.path), &f.bytes);
    }
    let roots: Vec<PathBuf> = vec![dir.path().to_path_buf()];
    let start = Instant::now();
    let s = scan(&roots, None);
    let scanned = start.elapsed();
    let mut lib = Library::new();
    let ops = diff(&mut lib, &s);
    apply_all(&mut lib, ops);
    let total = start.elapsed();
    println!("scan of 1000 JPEGs in 10 folders: walk + headers {scanned:?}, with diff and apply {total:?}");
    assert_eq!((lib.folders().count(), lib.photos().count()), (10, 1000));
    // Every orientation was read: portrait and landscape both appear as displayed.
    assert!(lib.photos().all(|p| (p.width, p.height) == (480, 320) || (p.width, p.height) == (320, 480)));
    assert!(total.as_secs() < 20, "{total:?}");
}
