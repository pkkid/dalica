//! Incremental verification (`data-model.md` §7): only the dirty folders are scanned again, the
//! result equals the full diff restricted to them, renamed folders keep their ids, and nothing
//! under a root that cannot be walked is ever removed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use dalica_library::{Library, Op, Photo, Sketch};
use dalica_scan::watch::{DirtyFolder, Rescan, Scope, known_folders, plan, rescan};
use dalica_scan::{Options, Scan, diff, diff_rescan, scan};
use dalica_scenes::{Scene, jpeg};

/// Writes a photo whose size depends on `seed` and whose modification time is `seed` seconds after
/// a fixed moment, so renames are recognised by (size, time) whatever the file system's clock does.
fn put(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let w = 64 + u32::try_from(seed % 50).unwrap();
    std::fs::write(path, jpeg(&Scene::new(seed, w, 64), 80, None).unwrap()).unwrap();
    touch(path, seed);
}

fn touch(path: &Path, seed: u64) {
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seed);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

/// root/a/{1,2,3}.jpg, root/a/b/4.jpg, root/c/5.jpg, root/c/d/6.jpg.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().join("root");
    put(&r.join("a/1.jpg"), 1);
    put(&r.join("a/2.jpg"), 2);
    put(&r.join("a/3.jpg"), 3);
    put(&r.join("a/b/4.jpg"), 4);
    put(&r.join("c/5.jpg"), 5);
    put(&r.join("c/d/6.jpg"), 6);
    (dir, r)
}

fn apply_all(lib: &mut Library, ops: Vec<Op>) {
    for op in ops {
        lib.apply(op).unwrap();
    }
}

fn library(root: &Path) -> Library {
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan(&[root.to_path_buf()], None));
    apply_all(&mut lib, ops);
    lib
}

fn dirty(path: PathBuf, subtree: bool) -> DirtyFolder {
    DirtyFolder { path, subtree }
}

/// The full scan cut down to what `verified` covers: the full diff restricted to those scopes.
fn restricted(full: &Scan, verified: &[Scope]) -> Rescan {
    let covers = |p: &Path| verified.iter().any(|s| s.covers(p));
    Rescan {
        scan: Scan {
            walked: verified.iter().map(|s| s.folder.clone()).collect(),
            folders: full.folders.iter().filter(|f| covers(&f.path)).cloned().collect(),
            ..Scan::default()
        },
        verified: verified.to_vec(),
    }
}

/// `(folder path, name, size, star)` of every photo, sorted.
fn photos(lib: &Library) -> Vec<(PathBuf, String, u64, bool)> {
    let mut v: Vec<_> = lib.photos().map(|p| (lib.folder(p.folder).unwrap().path.clone(), p.name.clone(), p.size, p.star)).collect();
    v.sort();
    v
}

fn ids(lib: &Library, folder: &Path) -> Vec<(String, dalica_core::PhotoId)> {
    let id = lib.folder_by_path(folder).unwrap();
    let mut v: Vec<_> = lib.photos_in(id).into_iter().map(|p: &Photo| (p.name.clone(), p.id)).collect();
    v.sort();
    v
}

#[test]
fn a_dirty_folder_is_verified_alone_and_equals_the_full_diff_restricted_to_it() {
    let (_dir, r) = tree();
    let lib = library(&r);
    // Inside `a`: an add, a delete, a rename and a rewrite. Elsewhere: changes the dirty folder
    // must not see.
    put(&r.join("a/new.jpg"), 10);
    std::fs::remove_file(r.join("a/2.jpg")).unwrap();
    std::fs::rename(r.join("a/3.jpg"), r.join("a/three.jpg")).unwrap();
    put(&r.join("a/1.jpg"), 11);
    put(&r.join("a/b/7.jpg"), 12);
    std::fs::remove_file(r.join("c/5.jpg")).unwrap();

    let roots = vec![r.clone()];
    let known = known_folders(&lib);
    let inc = rescan(&known, &roots, &[dirty(r.join("a"), false)], &Options::default());
    // Only `a` itself: its known subfolder `b` is left as it is.
    assert_eq!(inc.verified, [Scope { folder: r.join("a"), kept: vec![r.join("a/b")] }]);
    assert_eq!(inc.scan.folders.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), [r.join("a")]);

    let full = scan(std::slice::from_ref(&r), None);
    let inc_ops = diff_rescan(&mut lib.clone(), &inc);
    assert!(!inc_ops.is_empty());
    assert_eq!(inc_ops, diff_rescan(&mut lib.clone(), &restricted(&full, &inc.verified)));

    // Applied, `a` matches the full verification and nothing else moved.
    let mut by_inc = lib.clone();
    apply_all(&mut by_inc, inc_ops);
    let mut by_full = lib.clone();
    let full_ops = diff(&mut by_full, &full);
    apply_all(&mut by_full, full_ops);
    let in_a = |v: Vec<(PathBuf, String, u64, bool)>| v.into_iter().filter(|p| p.0 == r.join("a")).collect::<Vec<_>>();
    assert_eq!(in_a(photos(&by_inc)), in_a(photos(&by_full)));
    let outside = |v: Vec<(PathBuf, String, u64, bool)>| v.into_iter().filter(|p| p.0 != r.join("a")).collect::<Vec<_>>();
    assert_eq!(outside(photos(&by_inc)), outside(photos(&lib)));
    // The rename kept its id.
    assert_eq!(
        ids(&by_inc, &r.join("a")).iter().find(|(n, _)| n == "three.jpg").map(|x| x.1),
        ids(&lib, &r.join("a")).iter().find(|(n, _)| n == "3.jpg").map(|x| x.1)
    );
    // Verifying every changed folder gives what the full verification gives.
    let all = rescan(&known, &roots, &[dirty(r.join("a"), false), dirty(r.join("a/b"), false), dirty(r.join("c"), false)], &Options::default());
    let mut by_all = lib.clone();
    let ops = diff_rescan(&mut by_all, &all);
    apply_all(&mut by_all, ops);
    assert_eq!(photos(&by_all), photos(&by_full));
    assert_eq!(by_all.model(), by_full.model());
}

#[test]
fn a_new_subfolder_is_scanned_whole_with_its_dirty_parent() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    put(&r.join("a/new/x.jpg"), 20);
    put(&r.join("a/new/deeper/y.jpg"), 21);
    let inc = rescan(&known_folders(&lib), std::slice::from_ref(&r), &[dirty(r.join("a"), false)], &Options::default());
    let ops = diff_rescan(&mut lib, &inc);
    apply_all(&mut lib, ops);
    assert!(lib.folder_by_path(&r.join("a/new")).is_some());
    assert!(lib.folder_by_path(&r.join("a/new/deeper")).is_some());
    // Diffing the whole tree again finds nothing left to do.
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), []);
}

#[test]
fn a_gone_folder_is_verified_through_the_nearest_folder_left() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    std::fs::remove_dir_all(r.join("c/d")).unwrap();
    let known = known_folders(&lib);
    let scopes = plan(&known, std::slice::from_ref(&r), &[dirty(r.join("c/d"), true)], &Options::default());
    assert_eq!(scopes, [Scope { folder: r.join("c"), kept: vec![] }]);
    let d = lib.folder_by_path(&r.join("c/d")).unwrap();
    let inc = rescan(&known, std::slice::from_ref(&r), &[dirty(r.join("c/d"), true)], &Options::default());
    assert_eq!(diff_rescan(&mut lib, &inc), [Op::RemoveFolder { id: d }]);
}

#[test]
fn nested_and_repeated_dirty_folders_make_one_scope() {
    let (_dir, r) = tree();
    let lib = library(&r);
    let known = known_folders(&lib);
    let d =
        [dirty(r.join("a"), true), dirty(r.join("a/b"), false), dirty(r.join("a"), false), dirty(r.join("c"), false), dirty(r.join("c/d"), false)];
    let scopes = plan(&known, std::slice::from_ref(&r), &d, &Options::default());
    assert_eq!(
        scopes,
        [
            Scope { folder: r.join("a"), kept: vec![] },
            Scope { folder: r.join("c"), kept: vec![r.join("c/d")] },
            Scope { folder: r.join("c/d"), kept: vec![] },
        ]
    );
    // Outside every root: nothing.
    assert!(plan(&known, std::slice::from_ref(&r), &[dirty(r.with_file_name("elsewhere"), true)], &Options::default()).is_empty());
}

#[test]
fn a_renamed_folder_keeps_its_ids_stars_sketches_and_undo() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    let a = lib.folder_by_path(&r.join("a")).unwrap();
    let b = lib.folder_by_path(&r.join("a/b")).unwrap();
    let before = ids(&lib, &r.join("a"));
    let before_b = ids(&lib, &r.join("a/b"));
    let one = before[0].1;
    // A star the user can still undo, and a sketch the thumbnailer made.
    let undo = lib.apply(Op::SetStars { stars: vec![(one, true)] }).unwrap();
    lib.apply(Op::SetSketches { sketches: vec![(one, Sketch::new(&[1, 2, 3]))] }).unwrap();

    std::fs::rename(r.join("a"), r.join("z")).unwrap();
    // The watcher reports the parent (an entry renamed in it) and the new folder.
    let inc = rescan(&known_folders(&lib), std::slice::from_ref(&r), &[dirty(r.clone(), false), dirty(r.join("z"), true)], &Options::default());
    let ops = diff_rescan(&mut lib, &inc);
    assert!(ops.iter().all(|op| matches!(op, Op::Batch { .. })), "two folder moves, nothing else: {ops:?}");
    apply_all(&mut lib, ops);

    assert_eq!(lib.folder_by_path(&r.join("z")), Some(a));
    assert_eq!(lib.folder_by_path(&r.join("z/b")), Some(b));
    assert_eq!(lib.folder_by_path(&r.join("a")), None);
    assert_eq!(ids(&lib, &r.join("z")), before);
    assert_eq!(ids(&lib, &r.join("z/b")), before_b);
    assert!(lib.photo(one).unwrap().star);
    assert_eq!(lib.sketch(one), Sketch::new(&[1, 2, 3]).as_ref());
    // The undo recorded before the rename still applies.
    lib.apply(undo).unwrap();
    assert!(!lib.photo(one).unwrap().star);
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), []);
}

#[test]
fn a_folder_moved_and_changed_at_once_keeps_its_ids_through_a_full_verification_too() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    let c = lib.folder_by_path(&r.join("c")).unwrap();
    let five = ids(&lib, &r.join("c"))[0].1;
    // `c` moves into `a` and gains a photo on the way; a copy elsewhere is not a move.
    std::fs::rename(r.join("c"), r.join("a/c-moved")).unwrap();
    put(&r.join("a/c-moved/extra.jpg"), 30);
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    assert_eq!(lib.folder_by_path(&r.join("a/c-moved")), Some(c));
    assert_eq!(ids(&lib, &r.join("a/c-moved")).iter().find(|(n, _)| n == "5.jpg").map(|x| x.1), Some(five));
    assert_eq!(ids(&lib, &r.join("a/c-moved")).len(), 2);
    assert_eq!(diff(&mut lib, &scan(std::slice::from_ref(&r), None)), []);
}

#[test]
fn a_new_folder_sharing_too_few_photos_is_not_a_rename() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    let a = lib.folder_by_path(&r.join("a")).unwrap();
    // `a` (three photos) is gone; a new folder holds only one of them.
    std::fs::create_dir_all(r.join("n")).unwrap();
    std::fs::rename(r.join("a/1.jpg"), r.join("n/1.jpg")).unwrap();
    std::fs::rename(r.join("a/b"), r.join("b-out")).unwrap();
    std::fs::remove_dir_all(r.join("a")).unwrap();
    let ops = diff(&mut lib, &scan(std::slice::from_ref(&r), None));
    apply_all(&mut lib, ops);
    assert!(lib.folder(a).is_none());
    assert_ne!(lib.folder_by_path(&r.join("n")), Some(a));
}

#[test]
fn nothing_is_removed_under_a_root_that_cannot_be_walked() {
    let (dir, r) = tree();
    let mut lib = library(&r);
    let before = lib.model().clone();
    // The volume is unplugged: the root and everything in it vanish.
    std::fs::rename(&r, dir.path().join("unplugged")).unwrap();
    let known = known_folders(&lib);
    let d = [dirty(r.join("a"), false), dirty(r.join("c/d"), true), dirty(r.clone(), true)];
    let inc = rescan(&known, std::slice::from_ref(&r), &d, &Options::default());
    assert!(inc.verified.is_empty(), "{inc:?}");
    assert_eq!(diff_rescan(&mut lib, &inc), []);
    assert_eq!(lib.model(), &before);
}

#[test]
fn a_cancelled_rescan_verifies_nothing_it_did_not_finish() {
    let (_dir, r) = tree();
    let mut lib = library(&r);
    std::fs::remove_dir_all(r.join("c")).unwrap();
    let options = Options { cancel: Some(Arc::new(AtomicBool::new(true))), ..Options::default() };
    let inc = rescan(&known_folders(&lib), std::slice::from_ref(&r), &[dirty(r.clone(), true)], &options);
    assert!(inc.scan.cancelled && inc.verified.is_empty(), "{inc:?}");
    assert_eq!(diff_rescan(&mut lib, &inc), []);
}

#[test]
fn failures_outside_the_verified_scopes_are_kept() {
    let (_dir, r) = tree();
    let lib = library(&r);
    let elsewhere = dalica_scan::Failure {
        path: r.join("c/broken.jpg"),
        size: 3,
        mtime: dalica_core::Timestamp::EPOCH,
        format: None,
        reason: "Damaged file".into(),
    };
    let inside = dalica_scan::Failure { path: r.join("a/gone.jpg"), ..elsewhere.clone() };
    let options = Options { failures: vec![elsewhere.clone(), inside], ..Options::default() };
    let inc = rescan(&known_folders(&lib), std::slice::from_ref(&r), &[dirty(r.join("a"), false)], &options);
    assert_eq!(inc.scan.failures, [elsewhere]);
}
