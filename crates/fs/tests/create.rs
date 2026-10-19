//! New files and folders that never replace one (`security.md` §5.4, `data-model.md` §6): the
//! first free collision name, races for one name, every fault point, hostile names; a file created
//! under its exact name or not at all (`exact_file`).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};

use crate::common::{names, temps};
use dalica_fs::create::{EXACT_FILE_POINTS, VERIFY_TEMP, exact_file, new_file, new_folder};
use dalica_fs::faults::{Fault, NEW_FILE_DIRECT_POINTS, NEW_FILE_POINTS, NEW_FOLDER_POINTS, arm, disarm_all, points, run_at};
use dalica_fs::{Error, guard, identity};

const JPEG: &[u8] = b"\xff\xd8\xff\xe0 a new rendering \xff\xd9";

/// A folder holding `taken.jpg` and `taken-001.jpg`.
fn taken() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("taken.jpg"), b"first").unwrap();
    fs::write(dir.path().join("taken-001.jpg"), b"second").unwrap();
    dir
}

fn assert_taken_intact(dir: &Path, case: &str) {
    assert_eq!(fs::read(dir.join("taken.jpg")).unwrap(), b"first", "{case}");
    assert_eq!(fs::read(dir.join("taken-001.jpg")).unwrap(), b"second", "{case}");
}

#[test]
fn a_new_file_takes_the_first_free_name_and_never_replaces() {
    let dir = taken();
    let made = new_file(dir.path(), "taken.jpg", JPEG).unwrap();
    assert_eq!(made.name, "taken-002.jpg");
    assert_eq!(fs::read(dir.path().join("taken-002.jpg")).unwrap(), JPEG);
    assert_eq!(identity(&dir.path().join("taken-002.jpg")).unwrap(), Some(made.identity));
    assert_taken_intact(dir.path(), "first write");
    // A free name is used as given; the next copy of the same name goes on counting.
    assert_eq!(new_file(dir.path(), "free.png", b"png").unwrap().name, "free.png");
    assert_eq!(new_file(dir.path(), "taken.jpg", b"again").unwrap().name, "taken-003.jpg");
    assert_eq!(names(dir.path()), ["free.png", "taken-001.jpg", "taken-002.jpg", "taken-003.jpg", "taken.jpg"]);
    assert_eq!(temps(dir.path()), 0);
}

#[test]
fn a_folder_or_a_link_under_the_name_is_never_replaced_either() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("x.jpg")).unwrap();
    assert_eq!(new_file(dir.path(), "x.jpg", JPEG).unwrap().name, "x-001.jpg");
    assert!(dir.path().join("x.jpg").is_dir());
    #[cfg(unix)]
    {
        fs::write(dir.path().join("target.jpg"), b"target").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target.jpg"), dir.path().join("l.jpg")).unwrap();
        assert_eq!(new_file(dir.path(), "l.jpg", JPEG).unwrap().name, "l-001.jpg");
        assert_eq!(fs::read(dir.path().join("target.jpg")).unwrap(), b"target");
        assert!(fs::symlink_metadata(dir.path().join("l.jpg")).unwrap().file_type().is_symlink());
    }
}

#[test]
fn threads_racing_for_one_name_get_one_name_each() {
    for _round in 0..5 {
        let dir = taken();
        let threads = 8;
        let barrier = Arc::new(Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|i| {
                let (barrier, path) = (Arc::clone(&barrier), dir.path().to_path_buf());
                std::thread::spawn(move || {
                    let bytes = format!("writer {i}").into_bytes();
                    barrier.wait();
                    let made = new_file(&path, "taken.jpg", &bytes).unwrap();
                    (made.name, bytes)
                })
            })
            .collect();
        let results: Vec<(String, Vec<u8>)> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let distinct: BTreeSet<&String> = results.iter().map(|(n, _)| n).collect();
        assert_eq!(distinct.len(), threads, "{results:?}");
        for (name, bytes) in &results {
            assert_eq!(&fs::read(dir.path().join(name)).unwrap(), bytes, "{name}");
        }
        assert_taken_intact(dir.path(), "race");
        assert_eq!(temps(dir.path()), 0);
    }
}

#[test]
fn two_threads_racing_for_one_folder_name_get_two_folders() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Export")).unwrap();
    fs::write(dir.path().join("Export").join("old.jpg"), b"old").unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (barrier, path) = (Arc::clone(&barrier), dir.path().to_path_buf());
            std::thread::spawn(move || {
                barrier.wait();
                new_folder(&path, "Export").unwrap()
            })
        })
        .collect();
    let mut got: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    got.sort();
    assert_eq!(got, ["Export-001", "Export-002"]);
    assert_eq!(names(&dir.path().join("Export")), ["old.jpg"]);
    for name in &got {
        assert!(names(&dir.path().join(name)).is_empty());
    }
    // A file of the name counts as taken too, and a dotted name is numbered at its end.
    fs::write(dir.path().join("2026.06"), b"file").unwrap();
    assert_eq!(new_folder(dir.path(), "2026.06").unwrap(), "2026.06-001");
}

/// Every fault point, as an error and as a crash: the existing files keep their bytes, an error
/// leaves no temporary file, and a crash leaves at most one; afterwards a new write succeeds.
#[test]
fn each_fault_point_leaves_every_existing_file_intact() {
    let commit = NEW_FILE_POINTS.iter().position(|p| *p == points::NEW_COMMIT).unwrap();
    for (i, point) in NEW_FILE_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = taken();
            let case = format!("{point} {fault:?}");
            arm(point, fault);
            let err = new_file(dir.path(), "taken.jpg", JPEG).unwrap_err();
            disarm_all();
            match fault {
                Fault::Crash => assert!(matches!(err, Error::Crashed(p) if p == point), "{case}: {err}"),
                Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}: {err}"),
            }
            assert_taken_intact(dir.path(), &case);
            // Before the commit no new name exists; after it the new file is whole.
            let new = fs::read(dir.path().join("taken-002.jpg")).ok();
            assert_eq!(new.as_deref(), (i > commit).then_some(JPEG), "{case}");
            let left = temps(dir.path());
            match fault {
                Fault::Error => assert_eq!(left, 0, "{case}"),
                Fault::Crash => assert!(left <= 1, "{case}"),
            }
            let next = new_file(dir.path(), "taken.jpg", b"next").unwrap();
            assert_eq!(fs::read(dir.path().join(&next.name)).unwrap(), b"next", "{case}");
        }
    }
}

/// A volume without hard links or a non-replacing rename (simulated): the bytes go into the first
/// name exclusive creation gives, and every later fault still leaves the existing files intact.
#[test]
fn without_links_a_new_file_is_created_exclusively_and_faults_spare_existing_files() {
    let dir = taken();
    arm(points::NEW_NO_LINKS, Fault::Error);
    let made = new_file(dir.path(), "taken.jpg", JPEG).unwrap();
    disarm_all();
    assert_eq!(made.name, "taken-002.jpg");
    assert_eq!(fs::read(dir.path().join("taken-002.jpg")).unwrap(), JPEG);
    assert_taken_intact(dir.path(), "no links");
    assert_eq!(temps(dir.path()), 0);

    for point in std::iter::once(points::NEW_NO_LINKS).chain(NEW_FILE_DIRECT_POINTS) {
        for fault in [Fault::Error, Fault::Crash] {
            if point == points::NEW_NO_LINKS && fault == Fault::Error {
                continue; // that is the simulated volume itself, above
            }
            let dir = taken();
            let case = format!("{point} {fault:?}");
            if point != points::NEW_NO_LINKS {
                arm(points::NEW_NO_LINKS, Fault::Error);
            }
            arm(point, fault);
            let err = new_file(dir.path(), "taken.jpg", JPEG).unwrap_err();
            disarm_all();
            assert!(matches!(err, Error::Crashed(_) | Error::Io { .. }), "{case}: {err}");
            assert_taken_intact(dir.path(), &case);
            // An error removes its own half-written file; a crash may leave it torn, but only
            // under the new name.
            let new = fs::read(dir.path().join("taken-002.jpg")).ok();
            match (point, fault) {
                (points::NEW_DIRECT_WRITE | points::NEW_DIRECT_FSYNC, Fault::Error) => assert_eq!(new, None, "{case}"),
                (points::NEW_FSYNC_DIR | points::NEW_VERIFY, _) => assert_eq!(new.as_deref(), Some(JPEG), "{case}"),
                _ => {}
            }
            if fault == Fault::Error {
                assert_eq!(temps(dir.path()), 0, "{case}");
            }
            assert!(names(dir.path()).iter().all(|n| n.starts_with("taken") || n.starts_with(".taken")), "{case}");
        }
    }
}

#[test]
fn each_new_folder_fault_point_leaves_existing_folders_intact() {
    for point in NEW_FOLDER_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir(dir.path().join("Export")).unwrap();
            fs::write(dir.path().join("Export").join("old.jpg"), b"old").unwrap();
            arm(point, fault);
            assert!(new_folder(dir.path(), "Export").is_err(), "{point} {fault:?}");
            disarm_all();
            assert_eq!(names(&dir.path().join("Export")), ["old.jpg"]);
            assert_eq!(fs::read(dir.path().join("Export").join("old.jpg")).unwrap(), b"old");
            let made = dir.path().join("Export-001").is_dir();
            assert_eq!(made, point == points::NEW_MKDIR_FSYNC, "{point} {fault:?}");
        }
    }
}

#[test]
fn hostile_names_are_refused_with_nothing_written() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("photos");
    fs::create_dir(&dir).unwrap();
    let long = "x".repeat(256);
    let hostile = [
        "",
        ".",
        "..",
        "../escape.jpg",
        "sub/x.jpg",
        "sub\\x.jpg",
        "C:x.jpg",
        "x.jpg:stream",
        "nul\0.jpg",
        "CON",
        "aux.jpg",
        "com1.png",
        ".dalica.json",
        ".dalica-originals",
        ".x.jpg.dalica-tmp-0123456789abcdef",
        "trailing.",
        "trailing ",
        "line\nbreak.jpg",
        long.as_str(),
    ];
    for name in hostile {
        assert!(matches!(new_file(&dir, name, JPEG), Err(Error::Io { .. })), "{name:?}");
        assert!(matches!(new_folder(&dir, name), Err(Error::Io { .. })), "{name:?}");
    }
    assert!(names(&dir).is_empty());
    assert_eq!(names(root.path()), ["photos"]);
}

#[test]
fn a_missing_folder_and_the_ui_thread_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(new_file(&dir.path().join("gone"), "x.jpg", JPEG).is_err());
    assert!(new_folder(&dir.path().join("gone"), "x").is_err());
    let _ui = guard::mark_ui_thread();
    assert!(matches!(new_file(dir.path(), "x.jpg", JPEG), Err(Error::OnUiThread)));
    assert!(matches!(new_folder(dir.path(), "x"), Err(Error::OnUiThread)));
    drop(_ui);
    assert!(names(dir.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn a_folder_swapped_for_a_link_never_receives_the_new_file() {
    let root = tempfile::tempdir().unwrap();
    let (photos, moved, elsewhere) = (root.path().join("photos"), root.path().join("photos.moved"), root.path().join("elsewhere"));
    fs::create_dir(&photos).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    let (p, m, e) = (photos.clone(), moved.clone(), elsewhere.clone());
    run_at(points::NEW_COMMIT, move || {
        fs::rename(&p, &m).unwrap();
        std::os::unix::fs::symlink(&e, &p).unwrap();
    });
    let made = new_file(&photos, "x.jpg", JPEG).unwrap();
    disarm_all();
    assert_eq!(fs::read(moved.join(&made.name)).unwrap(), JPEG);
    assert!(names(&elsewhere).is_empty());
}

const JSON: &[u8] = b"{\"photos\": {\"a.jpg\": {\"star\": true}}}\n";

#[test]
fn an_exact_file_gets_its_name_or_nothing_and_never_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let id = exact_file(dir.path(), ".dalica.json", JSON).unwrap();
    assert_eq!(fs::read(dir.path().join(".dalica.json")).unwrap(), JSON);
    assert_eq!(identity(&dir.path().join(".dalica.json")).unwrap(), Some(id));
    // Taken by a file (even its own earlier write), a folder or a link: kept as it is.
    assert!(matches!(exact_file(dir.path(), ".dalica.json", b"{}"), Err(Error::Exists(p)) if p == dir.path().join(".dalica.json")));
    assert_eq!(fs::read(dir.path().join(".dalica.json")).unwrap(), JSON);
    fs::create_dir(dir.path().join("x.json")).unwrap();
    assert!(matches!(exact_file(dir.path(), "x.json", JSON), Err(Error::Exists(_))));
    assert!(dir.path().join("x.json").is_dir());
    #[cfg(unix)]
    {
        fs::write(dir.path().join("target.json"), b"target").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target.json"), dir.path().join("l.json")).unwrap();
        assert!(matches!(exact_file(dir.path(), "l.json", JSON), Err(Error::Link(_))));
        assert_eq!(fs::read(dir.path().join("target.json")).unwrap(), b"target");
        std::os::unix::fs::symlink(dir.path().join("gone.json"), dir.path().join("dangling.json")).unwrap();
        assert!(matches!(exact_file(dir.path(), "dangling.json", JSON), Err(Error::Link(_))));
        assert!(!dir.path().join("gone.json").exists(), "never created through a dangling link");
    }
    assert_eq!(temps(dir.path()), 0);
}

/// The name appears between every check and the commit, on a volume with links and on one
/// without: the file there keeps its bytes and the answer is `Exists`; no temporary file is left.
#[test]
fn a_file_that_appears_just_before_the_commit_is_kept_byte_identical() {
    for links in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let planted = dir.path().join(".dalica.json");
        let p = planted.clone();
        let point = if links { points::NEW_COMMIT } else { points::NEW_NO_LINKS };
        run_at(point, move || fs::write(&p, b"made by another program").unwrap());
        if !links {
            arm(points::NEW_NO_LINKS, Fault::Error);
        }
        let err = exact_file(dir.path(), ".dalica.json", JSON).unwrap_err();
        disarm_all();
        assert!(matches!(err, Error::Exists(ref p) if *p == planted), "links {links}: {err}");
        assert_eq!(fs::read(&planted).unwrap(), b"made by another program", "links {links}");
        assert_eq!(names(dir.path()), [".dalica.json"], "links {links}");
    }
}

/// Every fault point of an exact create, as an error and as a crash: no file under the name before
/// the commit, the whole verified bytes after it; an error leaves no temporary file, a crash at
/// most one; afterwards the write succeeds.
#[test]
fn each_exact_file_fault_point_leaves_no_file_or_the_whole_file() {
    let commit = EXACT_FILE_POINTS.iter().position(|p| *p == points::NEW_COMMIT).unwrap();
    assert!(EXACT_FILE_POINTS.iter().position(|p| *p == VERIFY_TEMP).unwrap() < commit, "the temporary file is verified before the commit");
    for (i, point) in EXACT_FILE_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let case = format!("{point} {fault:?}");
            arm(point, fault);
            let err = exact_file(dir.path(), ".dalica.json", JSON).unwrap_err();
            disarm_all();
            match fault {
                Fault::Crash => assert!(matches!(err, Error::Crashed(p) if p == point), "{case}: {err}"),
                Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}: {err}"),
            }
            let file = fs::read(dir.path().join(".dalica.json")).ok();
            assert_eq!(file.as_deref(), (i > commit).then_some(JSON), "{case}");
            match fault {
                Fault::Error => assert_eq!(temps(dir.path()), 0, "{case}"),
                Fault::Crash => assert!(temps(dir.path()) <= 1, "{case}"),
            }
            if file.is_none() {
                exact_file(dir.path(), ".dalica.json", JSON).unwrap();
                assert_eq!(fs::read(dir.path().join(".dalica.json")).unwrap(), JSON, "{case}");
            }
        }
    }
    // A volume without links: an error removes its own file; after the bytes are durable the file
    // is whole (a crash mid-write there may leave it torn, the one case without a temporary file).
    for point in std::iter::once(points::NEW_NO_LINKS).chain(NEW_FILE_DIRECT_POINTS) {
        for fault in [Fault::Error, Fault::Crash] {
            if point == points::NEW_NO_LINKS && fault == Fault::Error {
                continue;
            }
            let dir = tempfile::tempdir().unwrap();
            let case = format!("{point} {fault:?}");
            if point != points::NEW_NO_LINKS {
                arm(points::NEW_NO_LINKS, Fault::Error);
            }
            arm(point, fault);
            assert!(exact_file(dir.path(), ".dalica.json", JSON).is_err(), "{case}");
            disarm_all();
            let file = fs::read(dir.path().join(".dalica.json")).ok();
            match (point, fault) {
                (points::NEW_NO_LINKS, _) | (points::NEW_DIRECT_WRITE | points::NEW_DIRECT_FSYNC, Fault::Error) => assert_eq!(file, None, "{case}"),
                (points::NEW_DIRECT_FSYNC | points::NEW_FSYNC_DIR | points::NEW_VERIFY, _) => assert_eq!(file.as_deref(), Some(JSON), "{case}"),
                _ => {}
            }
            if fault == Fault::Error {
                assert_eq!(temps(dir.path()), 0, "{case}");
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    arm(points::NEW_NO_LINKS, Fault::Error);
    exact_file(dir.path(), ".dalica.json", JSON).unwrap();
    disarm_all();
    assert_eq!(fs::read(dir.path().join(".dalica.json")).unwrap(), JSON);
    assert_eq!(names(dir.path()), [".dalica.json"]);
}

#[test]
fn an_exact_file_refuses_hostile_names_parent_paths_and_the_ui_thread() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("photos");
    fs::create_dir(&dir).unwrap();
    let long = "x".repeat(256);
    for name in [
        "",
        ".",
        "..",
        "../escape.json",
        "sub/x.json",
        "sub\\x.json",
        "C:x.json",
        "x.json:stream",
        "nul\0.json",
        "CON",
        "aux.json",
        ".x.json.dalica-tmp-0123456789abcdef",
        "trailing.",
        "trailing ",
        "line\nbreak.json",
        long.as_str(),
    ] {
        assert!(matches!(exact_file(&dir, name, JSON), Err(Error::Io { .. })), "{name:?}");
    }
    assert!(matches!(exact_file(&dir.join("..").join("photos"), "x.json", JSON), Err(Error::Io { .. })));
    assert!(exact_file(&root.path().join("gone"), "x.json", JSON).is_err());
    let _ui = guard::mark_ui_thread();
    assert!(matches!(exact_file(&dir, "x.json", JSON), Err(Error::OnUiThread)));
    drop(_ui);
    assert!(names(&dir).is_empty());
    assert_eq!(names(root.path()), ["photos"]);
}
