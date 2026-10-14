//! Moves, renames and copies that never replace a file (`security.md` §5.3, §5.7, `data-model.md`
//! §8): the bytes and the time kept, a target created at the last moment never replaced, the
//! cross-volume path (copy, verify, then the source to the trash), folders, links, every fault
//! point, hostile names.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime};

use dalica_fs::faults::{
    COPY_POINTS, FINISH_POINTS, FOLDER_MOVE_POINTS, FOLDER_NEW_POINTS, Fault, MOVE_POINTS, TREE_POINTS, arm, disarm_all, points, run_at,
};
use dalica_fs::transfer::{Finished, FolderMoved, Moved, Outcome, copy_file, finish_move, move_file, move_folder, new_folder};
use dalica_fs::trash::{self, Trashed};
use dalica_fs::{Error, Identity, atomic::is_temp_name, guard, identity};

const JPEG: &[u8] = b"\xff\xd8\xff\xe0 the photo's own bytes \xff\xd9";

/// A time well in the past, to see that a move keeps it.
fn old_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_500_000_000)
}

fn write_old(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::File::options().write(true).open(path).unwrap().set_modified(old_time()).unwrap();
}

fn id(path: &Path) -> Identity {
    identity(path).unwrap().unwrap()
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

/// Every name in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn temps(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().filter(|e| is_temp_name(&e.as_ref().unwrap().file_name())).count()
}

/// Puts a trashed test item back where it was (inside a temporary folder that is removed after the
/// test), so the tests leave nothing in the trash where the platform can restore.
fn untrash(t: &Trashed) {
    let _ = trash::restore(std::slice::from_ref(t));
}

/// `src/photo.jpg` (an old modification time) and `dest/` holding a bystander.
struct World {
    _dir: tempfile::TempDir,
    src: PathBuf,
    dest: PathBuf,
    id: Identity,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let (src, dest) = (dir.path().join("src"), dir.path().join("dest"));
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dest).unwrap();
        write_old(&src.join("photo.jpg"), JPEG);
        fs::write(dest.join("bystander.jpg"), b"bystander").unwrap();
        let id = id(&src.join("photo.jpg"));
        World { _dir: dir, src, dest, id }
    }

    fn bystander_intact(&self, case: &str) {
        assert_eq!(fs::read(self.dest.join("bystander.jpg")).unwrap(), b"bystander", "{case}");
    }
}

#[test]
fn a_move_keeps_the_bytes_the_time_and_the_file() {
    let w = World::new();
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg").unwrap();
    assert_eq!(r, Outcome::Done(Moved::Renamed(w.id)));
    let moved = w.dest.join("photo.jpg");
    assert_eq!(fs::read(&moved).unwrap(), JPEG);
    assert_eq!(mtime(&moved), old_time());
    assert_eq!(id(&moved), w.id);
    assert!(!w.src.join("photo.jpg").exists());
    // A rename in its own folder.
    let r = move_file(&w.dest, "photo.jpg", w.id, &w.dest, "renamed.jpg").unwrap();
    assert_eq!(r, Outcome::Done(Moved::Renamed(w.id)));
    assert_eq!(names(&w.dest), ["bystander.jpg", "renamed.jpg"]);
    w.bystander_intact("rename");
}

#[test]
fn a_taken_name_is_exists_and_nothing_changes() {
    let w = World::new();
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "bystander.jpg").unwrap();
    assert_eq!(r, Outcome::Exists);
    w.bystander_intact("file at the target");
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    // A folder of the name is taken too.
    fs::create_dir(w.dest.join("photo.jpg")).unwrap();
    assert_eq!(move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg").unwrap(), Outcome::Exists);
    assert!(w.dest.join("photo.jpg").is_dir());
    // And across volumes.
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    assert_eq!(move_file(&w.src, "photo.jpg", w.id, &w.dest, "bystander.jpg").unwrap(), Outcome::Exists);
    disarm_all();
    w.bystander_intact("across volumes");
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    assert_eq!(temps(&w.dest), 0);
}

#[test]
fn a_file_created_between_the_check_and_the_move_is_never_replaced() {
    for (cross, point) in [(false, points::MOVE_RENAME), (true, points::COPY_COMMIT)] {
        let w = World::new();
        let planted = w.dest.join("photo.jpg");
        let p = planted.clone();
        run_at(point, move || fs::write(&p, b"planted at the last moment").unwrap());
        if cross {
            arm(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
        let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg").unwrap();
        disarm_all();
        assert_eq!(r, Outcome::Exists, "{point}");
        assert_eq!(fs::read(&planted).unwrap(), b"planted at the last moment", "{point}");
        assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG, "{point}");
        assert_eq!(temps(&w.dest), 0, "{point}");
    }
}

#[test]
fn threads_racing_for_one_name_never_replace_each_other() {
    for _round in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let threads = 8;
        let barrier = Arc::new(Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|i| {
                let src = dir.path().join(format!("src{i}"));
                fs::create_dir(&src).unwrap();
                let bytes = format!("photo {i}").into_bytes();
                fs::write(src.join("photo.jpg"), &bytes).unwrap();
                let expect = id(&src.join("photo.jpg"));
                let (barrier, dest) = (Arc::clone(&barrier), dest.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    (move_file(&src, "photo.jpg", expect, &dest, "same.jpg").unwrap(), src, bytes)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let winners: Vec<_> = results.iter().filter(|(r, _, _)| matches!(r, Outcome::Done(_))).collect();
        assert_eq!(winners.len(), 1, "{results:?}");
        let (_, _, won) = winners[0];
        assert_eq!(&fs::read(dest.join("same.jpg")).unwrap(), won);
        for (r, src, bytes) in &results {
            if *r == Outcome::Exists {
                assert_eq!(&fs::read(src.join("photo.jpg")).unwrap(), bytes);
            }
        }
        assert_eq!(names(&dest), ["same.jpg"]);
    }
}

#[test]
fn the_cross_volume_path_copies_verifies_then_trashes_the_source() {
    let w = World::new();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "moved.jpg").unwrap();
    disarm_all();
    let moved = w.dest.join("moved.jpg");
    let Outcome::Done(Moved::Copied { identity: copy, source: Finished::Trashed(t) }) = r else { panic!("{r:?}") };
    assert_eq!(fs::read(&moved).unwrap(), JPEG);
    assert_eq!(mtime(&moved), old_time());
    assert_eq!(id(&moved), copy);
    assert_ne!(copy, w.id, "a copy is a new file");
    assert!(!w.src.join("photo.jpg").exists());
    assert_eq!(t.original, fs::canonicalize(&w.src).unwrap().join("photo.jpg"));
    assert_eq!(t.identity, w.id);
    assert!(!t.folder);
    assert_eq!(temps(&w.dest), 0);
    w.bystander_intact("cross volume");
    untrash(&t);
}

#[test]
fn a_flipped_byte_in_the_copy_leaves_the_source_in_place() {
    let w = World::new();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    arm(points::COPY_FLIP, Fault::Error);
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg");
    disarm_all();
    assert!(matches!(r, Err(Error::VerifyFailed(_))), "{r:?}");
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    assert_eq!(id(&w.src.join("photo.jpg")), w.id);
    assert_eq!(names(&w.dest), ["bystander.jpg"]);
    // An empty file too (the flip appends a byte).
    fs::write(w.src.join("empty.jpg"), b"").unwrap();
    let empty = id(&w.src.join("empty.jpg"));
    arm(points::COPY_FLIP, Fault::Error);
    let r = copy_file(&w.src, "empty.jpg", empty, &w.dest, "empty.jpg");
    disarm_all();
    assert!(matches!(r, Err(Error::VerifyFailed(_))), "{r:?}");
    assert_eq!(names(&w.dest), ["bystander.jpg"]);
}

#[test]
fn a_copy_is_the_first_phase_alone_and_finish_is_the_second() {
    let w = World::new();
    let Outcome::Done(copy) = copy_file(&w.src, "photo.jpg", w.id, &w.dest, "copy.jpg").unwrap() else { panic!() };
    assert_eq!(fs::read(w.dest.join("copy.jpg")).unwrap(), JPEG);
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    assert_eq!(mtime(&w.dest.join("copy.jpg")), old_time());
    assert_eq!(copy_file(&w.src, "photo.jpg", w.id, &w.dest, "copy.jpg").unwrap(), Outcome::Exists);
    // The second phase refuses a source or a copy that changed, with nothing changed.
    assert!(matches!(finish_move(&w.src, "photo.jpg", copy, &w.dest, "copy.jpg", copy), Err(Error::Changed(_))));
    assert!(matches!(finish_move(&w.src, "photo.jpg", w.id, &w.dest, "copy.jpg", w.id), Err(Error::Changed(_))));
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    let Finished::Trashed(t) = finish_move(&w.src, "photo.jpg", w.id, &w.dest, "copy.jpg", copy).unwrap() else { panic!() };
    assert!(!w.src.join("photo.jpg").exists());
    assert_eq!(fs::read(w.dest.join("copy.jpg")).unwrap(), JPEG);
    untrash(&t);
}

#[test]
fn a_source_whose_volume_has_no_trash_stays_beside_its_copy() {
    let w = World::new();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    arm(points::TRASH_NO_TRASH, Fault::Error);
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg").unwrap();
    disarm_all();
    assert!(matches!(r, Outcome::Done(Moved::Copied { source: Finished::NoTrash, .. })), "{r:?}");
    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG);
    assert_eq!(fs::read(w.dest.join("photo.jpg")).unwrap(), JPEG);
}

#[test]
fn a_changed_source_is_refused_with_nothing_moved() {
    let w = World::new();
    fs::write(w.src.join("photo.jpg"), b"rewritten by another program").unwrap();
    for cross in [false, true] {
        if cross {
            arm(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
        let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg");
        disarm_all();
        assert!(matches!(r, Err(Error::Changed(_))), "{r:?}");
        assert_eq!(names(&w.dest), ["bystander.jpg"]);
    }
    let r = move_file(&w.src, "missing.jpg", w.id, &w.dest, "missing.jpg");
    assert!(matches!(r, Err(Error::Io { .. })), "{r:?}");
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_either_folder_or_the_file_is_refused_with_nothing_moved() {
    use std::os::unix::fs::symlink;
    let w = World::new();
    let root = w.src.parent().unwrap().to_path_buf();
    symlink(&w.src, root.join("src-link")).unwrap();
    symlink(&w.dest, root.join("dest-link")).unwrap();
    let r = move_file(&root.join("src-link"), "photo.jpg", w.id, &w.dest, "photo.jpg");
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    let r = move_file(&w.src, "photo.jpg", w.id, &root.join("dest-link"), "photo.jpg");
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    let r = copy_file(&w.src, "photo.jpg", w.id, &root.join("dest-link"), "photo.jpg");
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    // A file that is a link is refused too (never followed), with any expected identity.
    symlink(w.src.join("photo.jpg"), w.src.join("link.jpg")).unwrap();
    for forced in [false, true] {
        if forced {
            arm(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
        let r = move_file(&w.src, "link.jpg", w.id, &w.dest, "link.jpg");
        disarm_all();
        assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    }
    assert!(matches!(copy_file(&w.src, "link.jpg", w.id, &w.dest, "copy.jpg"), Err(Error::Link(_))));
    assert!(fs::symlink_metadata(w.src.join("link.jpg")).unwrap().file_type().is_symlink());
    // A folder swapped for a link after the check is refused too.
    let (src, moved_away) = (w.src.clone(), root.join("src-real"));
    run_at(points::MOVE_OPEN_DIR, move || {
        fs::rename(&src, &moved_away).unwrap();
        symlink(&moved_away, &src).unwrap();
    });
    let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg");
    disarm_all();
    assert!(matches!(r, Err(Error::Link(_) | Error::Changed(_))), "{r:?}");
    assert_eq!(names(&w.dest), ["bystander.jpg"]);
    assert_eq!(fs::read(root.join("src-real").join("photo.jpg")).unwrap(), JPEG);
}

#[test]
fn hostile_names_are_refused_with_nothing_changed() {
    let w = World::new();
    for (name, to) in [
        ("../photo.jpg", "photo.jpg"),
        ("photo.jpg", "../photo.jpg"),
        ("photo.jpg", "sub/photo.jpg"),
        ("photo.jpg", ""),
        ("photo.jpg", ".."),
        ("photo.jpg", "CON"),
        ("photo.jpg", "nul.jpg"),
        ("photo.jpg", "photo.jpg:stream"),
        ("photo.jpg", "trailing."),
        ("photo.jpg", "a\0b"),
        ("photo.jpg", ".dalica.json"),
        ("photo.jpg", ".photo.jpg.dalica-tmp-0123456789abcdef"),
        (".dalica.json", "x.json"),
    ] {
        let r = move_file(&w.src, name, w.id, &w.dest, to);
        assert!(matches!(&r, Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::InvalidInput), "{name:?} → {to:?}: {r:?}");
        let r = copy_file(&w.src, name, w.id, &w.dest, to);
        assert!(r.is_err(), "copy {name:?} → {to:?}");
    }
    for bad in ["", "..", "a/b", "CON", ".dalica-originals"] {
        assert!(new_folder(&w.dest, bad).is_err(), "{bad:?}");
        assert!(move_folder(w.src.parent().unwrap(), "src", &w.dest, bad).is_err(), "{bad:?}");
    }
    assert_eq!(names(&w.src), ["photo.jpg"]);
    assert_eq!(names(&w.dest), ["bystander.jpg"]);
}

#[test]
fn a_new_folder_is_made_exclusively() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(new_folder(dir.path(), "Trip").unwrap(), Outcome::Done(()));
    assert!(dir.path().join("Trip").is_dir());
    fs::write(dir.path().join("Trip").join("a.jpg"), b"a").unwrap();
    assert_eq!(new_folder(dir.path(), "Trip").unwrap(), Outcome::Exists);
    assert_eq!(names(&dir.path().join("Trip")), ["a.jpg"]);
    fs::write(dir.path().join("file"), b"x").unwrap();
    assert_eq!(new_folder(dir.path(), "file").unwrap(), Outcome::Exists);
}

/// `root/Trip` holding photos, a sidecar, an originals backup and a subfolder.
fn trip(root: &Path) -> PathBuf {
    let trip = root.join("Trip");
    fs::create_dir_all(trip.join("Day 2").join(".dalica-originals")).unwrap();
    fs::create_dir_all(trip.join(".dalica-originals")).unwrap();
    write_old(&trip.join("a.jpg"), b"photo a");
    write_old(&trip.join("b.jpg"), &vec![0x5a; 3 << 20]);
    fs::write(trip.join(".dalica.json"), b"{\"sidecar\":true}").unwrap();
    fs::write(trip.join(".dalica-originals").join("a.jpg"), b"original a").unwrap();
    write_old(&trip.join("Day 2").join("c.jpg"), b"photo c");
    fs::write(trip.join("Day 2").join("empty.jpg"), b"").unwrap();
    trip
}

/// Every file under `dir` with its bytes, by relative path.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_folder_moves_with_its_subtree() {
    let dir = tempfile::tempdir().unwrap();
    let src = trip(dir.path());
    let before = tree(&src);
    let folder_id = id(&src);
    let lib = dir.path().join("Library");
    fs::create_dir(&lib).unwrap();
    assert_eq!(move_folder(dir.path(), "Trip", &lib, "Trip 2026").unwrap(), Outcome::Done(FolderMoved::Renamed));
    let moved = lib.join("Trip 2026");
    assert_eq!(tree(&moved), before);
    assert_eq!(mtime(&moved.join("a.jpg")), old_time());
    assert!(!src.exists());
    let _ = folder_id;
    // Renamed in place, and refused onto a taken name or into itself.
    assert_eq!(move_folder(&lib, "Trip 2026", &lib, "Trip").unwrap(), Outcome::Done(FolderMoved::Renamed));
    fs::create_dir(lib.join("Other")).unwrap();
    assert_eq!(move_folder(&lib, "Trip", &lib, "Other").unwrap(), Outcome::Exists);
    let r = move_folder(&lib, "Trip", &lib.join("Trip").join("Day 2"), "Trip");
    assert!(matches!(&r, Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::InvalidInput), "{r:?}");
    assert_eq!(tree(&lib.join("Trip")), before);
}

#[test]
fn a_folder_moved_across_volumes_is_copied_verified_then_trashed() {
    let dir = tempfile::tempdir().unwrap();
    let src = trip(dir.path());
    let before = tree(&src);
    let lib = dir.path().join("Library");
    fs::create_dir(&lib).unwrap();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    let r = move_folder(dir.path(), "Trip", &lib, "Trip").unwrap();
    disarm_all();
    let Outcome::Done(FolderMoved::Copied { files, source: Finished::Trashed(t) }) = r else { panic!("{r:?}") };
    assert_eq!(files, 6);
    assert_eq!(tree(&lib.join("Trip")), before);
    assert_eq!(mtime(&lib.join("Trip").join("Day 2").join("c.jpg")), old_time());
    assert!(!src.exists());
    assert!(t.folder);
    assert_eq!(names(&lib), ["Trip"]);
    untrash(&t);
}

#[test]
fn a_tree_copy_that_fails_leaves_the_source_and_removes_its_own_copy() {
    let dir = tempfile::tempdir().unwrap();
    let src = trip(dir.path());
    let before = tree(&src);
    let lib = dir.path().join("Library");
    fs::create_dir(&lib).unwrap();
    // A byte flipped in a copied file.
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    arm(points::COPY_FLIP, Fault::Error);
    let r = move_folder(dir.path(), "Trip", &lib, "Trip");
    disarm_all();
    assert!(matches!(r, Err(Error::VerifyFailed(_))), "{r:?}");
    assert_eq!(tree(&src), before);
    assert!(names(&lib).is_empty(), "{:?}", names(&lib));
    // A taken target name.
    fs::create_dir(lib.join("Trip")).unwrap();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    assert_eq!(move_folder(dir.path(), "Trip", &lib, "Trip").unwrap(), Outcome::Exists);
    disarm_all();
    assert_eq!(names(&lib), ["Trip"]);
    assert_eq!(tree(&src), before);
    #[cfg(unix)]
    {
        // A link inside the tree refuses the copy.
        fs::write(dir.path().join("outside.jpg"), b"outside").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside.jpg"), src.join("Day 2").join("link.jpg")).unwrap();
        let with_link = tree(&src);
        arm(points::MOVE_CROSS_VOLUME, Fault::Error);
        let r = move_folder(dir.path(), "Trip", &lib, "Trip copy");
        disarm_all();
        assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
        assert_eq!(tree(&src), with_link);
        assert_eq!(names(&lib), ["Trip"]);
    }
}

#[cfg(unix)]
#[test]
fn a_folder_link_is_refused_with_nothing_moved() {
    let dir = tempfile::tempdir().unwrap();
    let src = trip(dir.path());
    let lib = dir.path().join("Library");
    fs::create_dir(&lib).unwrap();
    std::os::unix::fs::symlink(&src, dir.path().join("Trip link")).unwrap();
    assert!(matches!(move_folder(dir.path(), "Trip link", &lib, "Trip"), Err(Error::Link(_))));
    std::os::unix::fs::symlink(&lib, dir.path().join("Library link")).unwrap();
    assert!(matches!(move_folder(dir.path(), "Trip", &dir.path().join("Library link"), "Trip"), Err(Error::Link(_))));
    assert!(names(&lib).is_empty());
    assert!(src.join("a.jpg").is_file());
}

/// The photo's bytes are whole at the source, or (after a same-volume rename) at the destination.
fn whole(w: &World, case: &str) {
    let at_src = fs::read(w.src.join("photo.jpg")).ok();
    let at_dest = fs::read(w.dest.join("photo.jpg")).ok();
    assert!(at_src.as_deref() == Some(JPEG) || at_dest.as_deref() == Some(JPEG), "{case}: src {at_src:?} dest {at_dest:?}");
    w.bystander_intact(case);
}

#[test]
fn every_fault_point_leaves_the_source_whole() {
    let cross: Vec<&str> = COPY_POINTS.iter().chain(FINISH_POINTS.iter()).copied().collect();
    let renamed_already = [points::MOVE_FSYNC_DIR, points::MOVE_VERIFY];
    for (forced, list) in [(false, &MOVE_POINTS[..]), (true, &cross[..])] {
        for &point in list {
            for fault in [Fault::Error, Fault::Crash] {
                let w = World::new();
                let case = format!("move, cross volume {forced}, {point} as {fault:?}");
                if forced {
                    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
                }
                arm(point, fault);
                let r = move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg");
                disarm_all();
                assert!(r.is_err(), "{case}: {r:?}");
                whole(&w, &case);
                if forced || !renamed_already.contains(&point) {
                    assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG, "{case}: the source stays until the end");
                }
                if fault == Fault::Error {
                    assert_eq!(temps(&w.dest), 0, "{case}");
                }
            }
        }
    }
    for &point in &COPY_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let w = World::new();
            let case = format!("copy, {point} as {fault:?}");
            arm(point, fault);
            let r = copy_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg");
            disarm_all();
            assert!(r.is_err(), "{case}: {r:?}");
            assert_eq!(fs::read(w.src.join("photo.jpg")).unwrap(), JPEG, "{case}");
            w.bystander_intact(&case);
            if fault == Fault::Error {
                assert_eq!(temps(&w.dest), 0, "{case}");
            }
        }
    }
}

#[test]
fn every_folder_fault_point_leaves_the_source_tree_whole() {
    let renamed_already = [points::FOLDER_FSYNC_DIR];
    for (forced, list) in [(false, &FOLDER_MOVE_POINTS[..]), (true, &TREE_POINTS[..])] {
        for &point in list {
            for fault in [Fault::Error, Fault::Crash] {
                let dir = tempfile::tempdir().unwrap();
                let src = trip(dir.path());
                let before = tree(&src);
                let lib = dir.path().join("Library");
                fs::create_dir(&lib).unwrap();
                let case = format!("folder, cross volume {forced}, {point} as {fault:?}");
                if forced {
                    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
                }
                arm(point, fault);
                let r = move_folder(dir.path(), "Trip", &lib, "Trip");
                disarm_all();
                assert!(r.is_err(), "{case}: {r:?}");
                if !forced && renamed_already.contains(&point) {
                    assert_eq!(tree(&lib.join("Trip")), before, "{case}");
                } else {
                    assert_eq!(tree(&src), before, "{case}: the source stays until the end");
                }
                if fault == Fault::Error && forced && point != points::FINISH_CHECK && point != points::TRASH_SEND {
                    assert!(names(&lib).is_empty(), "{case}: {:?}", names(&lib));
                }
            }
        }
    }
    for &point in &FOLDER_NEW_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            arm(point, fault);
            let r = new_folder(dir.path(), "New");
            disarm_all();
            assert!(r.is_err(), "new folder, {point} as {fault:?}: {r:?}");
        }
    }
}

#[test]
fn the_ui_thread_is_refused() {
    let w = World::new();
    let _ui = guard::mark_ui_thread();
    assert!(matches!(move_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg"), Err(Error::OnUiThread)));
    assert!(matches!(copy_file(&w.src, "photo.jpg", w.id, &w.dest, "photo.jpg"), Err(Error::OnUiThread)));
    assert!(matches!(new_folder(&w.dest, "New"), Err(Error::OnUiThread)));
    assert!(matches!(move_folder(&w.src, "x", &w.dest, "x"), Err(Error::OnUiThread)));
}
