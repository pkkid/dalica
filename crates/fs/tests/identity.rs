//! `Identity` (M8.27): one version of a file is the object on disk (device and inode on Unix,
//! volume serial number and file index on Windows), its size and its modification time. These run
//! on every OS; on Windows they are what the file index changed (before M8.27 the creation time
//! stood in for it, and NTFS tunnels a replaced file's creation time onto the file renamed over
//! its name).

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_fs::{Error, Expect, identity, read, replace, stat};

/// 2001-02-03 04:05:06.789 UTC: a time no test file has by accident.
fn old_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(981_173_106_789)
}

/// Writes `bytes` to `p` and gives it [`old_time`].
fn old_file(p: &Path, bytes: &[u8]) {
    fs::write(p, bytes).unwrap();
    fs::File::options().write(true).open(p).unwrap().set_modified(old_time()).unwrap();
}

/// Another program replaces the file the way editors and sync clients do (a new file renamed over
/// the name) with bytes of the same size and the old modification time: the identity changes, so
/// a write expecting the version read is refused and their bytes stay.
#[test]
fn a_same_size_replacement_with_the_old_time_is_a_new_identity() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"our bytes");
    let ours = read(&p, 1 << 20).unwrap().unwrap().identity;
    // At once, well inside NTFS's tunnelling window (15 s), so a tunnelled creation time would
    // have made it look like the same file.
    let theirs = dir.path().join("photo.jpg.part");
    old_file(&theirs, b"THEIR BYT");
    fs::rename(&theirs, &p).unwrap();
    let now = identity(&p).unwrap().unwrap();
    assert_eq!(now.size(), ours.size(), "same size");
    assert!(now.modified_at(old_time()) && ours.modified_at(old_time()), "same time");
    assert_ne!(now, ours, "another file");
    assert!(matches!(replace(&p, b"our edit", Expect::Unchanged(ours)), Err(Error::Changed(_))));
    assert_eq!(fs::read(&p).unwrap(), b"THEIR BYT");
    // The version that is there now can be written.
    replace(&p, b"our edit", Expect::Unchanged(now)).unwrap();
}

/// Two files of the same size and time are two identities; a file has one identity whichever
/// way it is looked at (by name, while reading, from `stat`, from the write that made it).
#[test]
fn identity_is_the_file_not_its_size_and_time() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
    old_file(&a, b"same bytes");
    old_file(&b, b"same bytes");
    let (ia, ib) = (identity(&a).unwrap().unwrap(), identity(&b).unwrap().unwrap());
    assert_ne!(ia, ib);
    assert_eq!(read(&a, 1 << 20).unwrap().unwrap().identity, ia);
    assert_eq!(stat(&a).unwrap().unwrap().identity, ia);
    let written = replace(&b, b"other bytes", Expect::Unchanged(ib)).unwrap();
    assert_eq!(identity(&b).unwrap(), Some(written));
    assert_eq!(read(&b, 1 << 20).unwrap().unwrap().identity, written);
}

/// Two names of one file (a hard link) are one identity, and an edit through either changes it.
#[test]
fn hard_links_share_one_identity() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
    old_file(&a, b"bytes");
    fs::hard_link(&a, &b).unwrap();
    let id = identity(&a).unwrap().unwrap();
    assert_eq!(identity(&b).unwrap(), Some(id));
    fs::OpenOptions::new().append(true).open(&b).unwrap().write_all(b" more").unwrap();
    let after = identity(&a).unwrap().unwrap();
    assert_ne!(after, id);
    assert_eq!(identity(&b).unwrap(), Some(after));
}

/// A folder replaced by another real folder of the same name is another identity (the held-folder
/// check on Windows compares this).
#[test]
fn a_folder_replaced_by_another_is_a_new_identity() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("Holiday");
    fs::create_dir(&sub).unwrap();
    let before = identity(&sub).unwrap().unwrap();
    assert_eq!(identity(&sub).unwrap(), Some(before), "stable while nothing changes");
    fs::rename(&sub, dir.path().join("Holiday (old)")).unwrap();
    fs::create_dir(&sub).unwrap();
    assert_ne!(identity(&sub).unwrap().unwrap(), before);
}

/// What the guard does not see, on every OS (`security.md` §5.3): an edit in place that keeps the
/// size and puts the modification time back is the same file with the same size and time.
#[test]
fn an_in_place_edit_that_restores_size_and_time_keeps_the_identity() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"our bytes");
    let ours = identity(&p).unwrap().unwrap();
    fs::OpenOptions::new().write(true).open(&p).unwrap().write_all(b"THEIR BYT").unwrap();
    fs::File::options().write(true).open(&p).unwrap().set_modified(old_time()).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"THEIR BYT");
    assert_eq!(identity(&p).unwrap(), Some(ours));
}

#[test]
fn a_missing_name_has_no_identity() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(identity(&dir.path().join("missing.jpg")).unwrap(), None);
    assert!(stat(&dir.path().join("missing.jpg")).unwrap().is_none());
}
