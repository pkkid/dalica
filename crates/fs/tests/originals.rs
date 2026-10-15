//! The originals backup (`data-model.md` §6, `security.md` §5.3, §5.6): a photo moves into
//! `.dalica-originals/` and back byte for byte under its own name; a changed photo, a link in place
//! of the backup folder and a backup that is not the recorded one are refused with nothing moved;
//! every fault point leaves the original's bytes under exactly one name.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::common::names;
use dalica_fs::faults::{BACKUP_POINTS, Fault, RESTORE_POINTS, arm, disarm_all, points};
use dalica_fs::originals::{Backup, DIR_NAME, Recorded, back_up, ensure_dir, restore};
use dalica_fs::{Error, Expect, Identity, guard, identity, replace};

const ORIGINAL: &[u8] = b"\xff\xd8\xff\xe1 the original photo, never to be lost \xff\xd9";
const SAVED: &[u8] = b"\xff\xd8\xff\xe0 the saved rendering \xff\xd9";
const NAME: &str = "beach.jpg";

/// A record of `bytes`: their size and a digest check comparing the bytes read with them (the real
/// digest is the record owner's choice; any function of the bytes does here).
fn digest_of(bytes: &'static [u8]) -> impl Fn(&mut dyn Read) -> io::Result<bool> {
    move |r: &mut dyn Read| {
        let mut got = Vec::new();
        r.read_to_end(&mut got)?;
        Ok(got == bytes)
    }
}

fn recorded<'a>(bytes: &[u8], check: &'a dyn Fn(&mut dyn Read) -> io::Result<bool>) -> Recorded<'a> {
    Recorded { size: bytes.len() as u64, digest_matches: check }
}

struct Setup {
    _root: tempfile::TempDir,
    folder: PathBuf,
    photo: PathBuf,
    backup: PathBuf,
    id: Identity,
}

/// A folder holding the photo `NAME` with the `ORIGINAL` bytes.
fn setup() -> Setup {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("photos");
    fs::create_dir(&folder).unwrap();
    let photo = folder.join(NAME);
    let id = replace(&photo, ORIGINAL, Expect::Absent).unwrap();
    let backup = folder.join(DIR_NAME).join(NAME);
    Setup { _root: root, folder, photo, backup, id }
}

fn read(p: &Path) -> Option<Vec<u8>> {
    fs::read(p).ok()
}

#[test]
fn a_move_into_the_backup_and_back_gives_identical_bytes_and_the_same_name() {
    let s = setup();
    let moved = back_up(&s.folder, NAME, s.id, None).unwrap();
    assert_eq!(moved, Backup::Moved(s.id));
    assert_eq!(read(&s.backup).as_deref(), Some(ORIGINAL));
    assert!(!s.photo.exists());
    // The saved rendering takes the photo's name.
    let saved = replace(&s.photo, SAVED, Expect::Absent).unwrap();
    // A second Save finds the recorded backup and moves nothing.
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    assert_eq!(back_up(&s.folder, NAME, saved, Some(&kept)).unwrap(), Backup::AlreadyKept);
    assert_eq!(read(&s.photo).as_deref(), Some(SAVED));
    // Undo Save: the original is back under its own name, byte for byte, with its identity.
    let back = restore(&s.folder, NAME, Expect::Unchanged(saved), &kept).unwrap();
    assert_eq!(back, s.id);
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    assert_eq!(identity(&s.photo).unwrap(), Some(s.id));
    assert!(!s.backup.exists());
    assert_eq!(names(&s.folder), [DIR_NAME, NAME]);
    // And again: the cycle repeats.
    back_up(&s.folder, NAME, s.id, None).unwrap();
    assert_eq!(read(&s.backup).as_deref(), Some(ORIGINAL));
}

#[test]
fn a_save_that_stopped_before_writing_is_repaired_with_expect_absent() {
    let s = setup();
    back_up(&s.folder, NAME, s.id, None).unwrap();
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    // A saved file appearing in between is never replaced by an `Absent` restore.
    let saved = replace(&s.photo, SAVED, Expect::Absent).unwrap();
    assert!(matches!(restore(&s.folder, NAME, Expect::Absent, &kept), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(SAVED));
    assert_eq!(read(&s.backup).as_deref(), Some(ORIGINAL));
    fs::remove_file(&s.photo).unwrap();
    let _ = saved;
    assert_eq!(restore(&s.folder, NAME, Expect::Absent, &kept).unwrap(), s.id);
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
}

#[test]
fn a_changed_photo_is_refused_with_nothing_moved() {
    let s = setup();
    // Another program rewrote the photo since it was read.
    fs::write(&s.photo, b"someone else's edit").unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(&b"someone else's edit"[..]));
    assert!(!s.backup.exists());
    // Gone, or replaced by a folder.
    fs::remove_file(&s.photo).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Changed(_))));
    fs::create_dir(&s.photo).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Changed(_))));
    assert!(names(&s.folder.join(DIR_NAME)).is_empty());
}

#[test]
fn a_mismatched_existing_backup_is_refused_with_nothing_moved() {
    let s = setup();
    fs::create_dir(s.folder.join(DIR_NAME)).unwrap();
    fs::write(&s.backup, b"an older original").unwrap();
    // No record: an unrecorded backup is never replaced, and the photo stays.
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Changed(p)) if p == s.backup));
    // A record that does not match it: other bytes of the same size, or another size.
    let check = digest_of(b"an older origin!");
    let same_size = recorded(b"an older origin!", &check);
    assert!(matches!(back_up(&s.folder, NAME, s.id, Some(&same_size)), Err(Error::Changed(_))));
    let check = digest_of(ORIGINAL);
    let other_size = recorded(ORIGINAL, &check);
    assert!(matches!(back_up(&s.folder, NAME, s.id, Some(&other_size)), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    assert_eq!(read(&s.backup).as_deref(), Some(&b"an older original"[..]));
    // Nor is a mismatched backup ever restored over the photo.
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(s.id), &other_size), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    // A recorded backup that is missing is refused too (the photo is not taken for it).
    fs::remove_file(&s.backup).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, Some(&other_size)), Err(Error::Changed(_))));
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(s.id), &other_size), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    // A digest that cannot be read is an error, not a match.
    fs::write(&s.backup, ORIGINAL).unwrap();
    let failing = |_: &mut dyn Read| -> io::Result<bool> { Err(io::Error::other("unreadable")) };
    let kept = recorded(ORIGINAL, &failing);
    assert!(matches!(back_up(&s.folder, NAME, s.id, Some(&kept)), Err(Error::Io { .. })));
}

#[test]
fn restore_refuses_a_saved_file_that_changed() {
    let s = setup();
    back_up(&s.folder, NAME, s.id, None).unwrap();
    let saved = replace(&s.photo, SAVED, Expect::Absent).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&s.photo, b"edited elsewhere after the save").unwrap();
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(saved), &kept), Err(Error::Changed(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(&b"edited elsewhere after the save"[..]));
    assert_eq!(read(&s.backup).as_deref(), Some(ORIGINAL));
}

#[test]
fn a_file_in_place_of_the_backup_folder_is_refused() {
    let s = setup();
    fs::write(s.folder.join(DIR_NAME), b"not a folder").unwrap();
    assert!(back_up(&s.folder, NAME, s.id, None).is_err());
    assert!(ensure_dir(&s.folder).is_err());
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    assert_eq!(read(&s.folder.join(DIR_NAME)).as_deref(), Some(&b"not a folder"[..]));
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_backup_folder_or_a_backup_is_refused_with_nothing_moved() {
    let s = setup();
    let elsewhere = s._root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, s.folder.join(DIR_NAME)).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Link(_))));
    assert!(matches!(ensure_dir(&s.folder), Err(Error::Link(_))));
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(s.id), &kept), Err(Error::Link(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    assert!(names(&elsewhere).is_empty());

    // A link in place of the backup itself, or of the photo.
    fs::remove_file(s.folder.join(DIR_NAME)).unwrap();
    fs::create_dir(s.folder.join(DIR_NAME)).unwrap();
    fs::write(elsewhere.join("decoy.jpg"), ORIGINAL).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("decoy.jpg"), &s.backup).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, Some(&kept)), Err(Error::Link(_))));
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(s.id), &kept), Err(Error::Link(_))));
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
    fs::remove_file(&s.backup).unwrap();
    let real = s.folder.join("real.jpg");
    fs::rename(&s.photo, &real).unwrap();
    std::os::unix::fs::symlink(&real, &s.photo).unwrap();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::Link(_))));
    assert_eq!(read(&real).as_deref(), Some(ORIGINAL));
    assert!(names(&s.folder.join(DIR_NAME)).is_empty());
}

#[cfg(unix)]
#[test]
fn the_backup_folder_swapped_for_a_link_mid_move_never_receives_the_photo() {
    use dalica_fs::faults::run_at;
    let s = setup();
    let elsewhere = s._root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let (dir, aside, target) = (s.folder.join(DIR_NAME), s.folder.join("aside"), elsewhere.clone());
    run_at(points::BACKUP_MOVE, move || {
        fs::rename(&dir, &aside).unwrap();
        std::os::unix::fs::symlink(&target, &dir).unwrap();
    });
    let result = back_up(&s.folder, NAME, s.id, None);
    disarm_all();
    // The move went to the folder that was opened (now `aside`), never through the link.
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(read(&s.folder.join("aside").join(NAME)).as_deref(), Some(ORIGINAL));
    assert!(names(&elsewhere).is_empty());
}

#[test]
fn ensure_dir_makes_the_backup_folder_once() {
    let s = setup();
    let dir = ensure_dir(&s.folder).unwrap();
    assert_eq!(dir, s.folder.join(DIR_NAME));
    assert!(dir.is_dir());
    fs::write(dir.join(".dalica.json"), b"{}").unwrap();
    assert_eq!(ensure_dir(&s.folder).unwrap(), dir);
    assert_eq!(names(&dir), [".dalica.json"]);
}

/// Every backup point, as an error and as a crash: the original's bytes are under exactly one of
/// the two names (the photo before the move, the backup after it), and the next try finishes.
#[test]
fn each_backup_fault_point_keeps_the_original_under_one_name() {
    let mv = BACKUP_POINTS.iter().position(|p| *p == points::BACKUP_MOVE).unwrap();
    for (i, point) in BACKUP_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            let s = setup();
            let case = format!("{point} {fault:?}");
            arm(point, fault);
            let err = back_up(&s.folder, NAME, s.id, None).unwrap_err();
            disarm_all();
            match fault {
                Fault::Crash => assert!(matches!(err, Error::Crashed(p) if p == point), "{case}: {err}"),
                Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}: {err}"),
            }
            let (photo, backup) = (read(&s.photo), read(&s.backup));
            if i > mv {
                assert_eq!((photo.as_deref(), backup.as_deref()), (None, Some(ORIGINAL)), "{case}");
            } else {
                assert_eq!((photo.as_deref(), backup.as_deref()), (Some(ORIGINAL), None), "{case}");
                assert_eq!(back_up(&s.folder, NAME, s.id, None).unwrap(), Backup::Moved(s.id), "{case}");
            }
            assert_eq!(read(&s.backup).as_deref(), Some(ORIGINAL), "{case}");
        }
    }
}

/// Every restore point, as an error and as a crash: the original's bytes are under exactly one
/// name, the saved file is replaced only by the move itself, and the next try finishes.
#[test]
fn each_restore_fault_point_keeps_the_original_under_one_name() {
    let mv = RESTORE_POINTS.iter().position(|p| *p == points::RESTORE_MOVE).unwrap();
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    for (i, point) in RESTORE_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            let s = setup();
            back_up(&s.folder, NAME, s.id, None).unwrap();
            let saved = replace(&s.photo, SAVED, Expect::Absent).unwrap();
            let case = format!("{point} {fault:?}");
            arm(point, fault);
            let err = restore(&s.folder, NAME, Expect::Unchanged(saved), &kept).unwrap_err();
            disarm_all();
            assert!(matches!(err, Error::Crashed(_) | Error::Io { .. }), "{case}: {err}");
            let (photo, backup) = (read(&s.photo), read(&s.backup));
            if i > mv {
                assert_eq!((photo.as_deref(), backup.as_deref()), (Some(ORIGINAL), None), "{case}");
            } else {
                assert_eq!((photo.as_deref(), backup.as_deref()), (Some(SAVED), Some(ORIGINAL)), "{case}");
                restore(&s.folder, NAME, Expect::Unchanged(saved), &kept).unwrap();
            }
            assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL), "{case}");
        }
    }
}

#[test]
fn hostile_names_are_refused_with_nothing_moved() {
    let s = setup();
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    for name in [
        "",
        ".",
        "..",
        "../photos/beach.jpg",
        "sub/beach.jpg",
        "a\\b.jpg",
        "C:beach.jpg",
        "beach.jpg:x",
        "CON",
        "nul.jpg",
        ".dalica.json",
        DIR_NAME,
        "a\0b",
    ] {
        assert!(matches!(back_up(&s.folder, name, s.id, None), Err(Error::Io { .. })), "{name:?}");
        assert!(matches!(restore(&s.folder, name, Expect::Unchanged(s.id), &kept), Err(Error::Io { .. })), "{name:?}");
    }
    assert_eq!(names(&s.folder), [NAME]);
    assert_eq!(read(&s.photo).as_deref(), Some(ORIGINAL));
}

#[test]
fn every_call_on_the_ui_thread_is_refused() {
    let s = setup();
    let check = digest_of(ORIGINAL);
    let kept = recorded(ORIGINAL, &check);
    let _ui = guard::mark_ui_thread();
    assert!(matches!(back_up(&s.folder, NAME, s.id, None), Err(Error::OnUiThread)));
    assert!(matches!(restore(&s.folder, NAME, Expect::Unchanged(s.id), &kept), Err(Error::OnUiThread)));
    assert!(matches!(ensure_dir(&s.folder), Err(Error::OnUiThread)));
    drop(_ui);
    assert_eq!(names(&s.folder), [NAME]);
}
