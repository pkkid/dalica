//! `atomic::replace_keeping_times` (M7.25): the times go onto the temporary file before the
//! rename, so the new file appears under the target's name already carrying them. A fault at
//! every point leaves the old file with its old time or the new file with the old time; the
//! target checks of a plain replace still apply.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_fs::atomic::{KEEP_TIMES, KEEPING_TIMES_POINTS, is_temp_name, replace_keeping_times};
use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all, points};
use dalica_fs::{Error, Expect, Times, identity, mark_ui_thread, replace, set_times, stat};

/// 2001-02-03 04:05:06.789 UTC: a time no test file has by accident.
fn old_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(981_173_106_789)
}

fn modified(p: &Path) -> SystemTime {
    fs::symlink_metadata(p).unwrap().modified().unwrap()
}

/// Writes `bytes` to `p` and gives it [`old_time`].
fn old_file(p: &Path, bytes: &[u8]) {
    fs::write(p, bytes).unwrap();
    fs::File::options().write(true).open(p).unwrap().set_modified(old_time()).unwrap();
}

fn temps(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().filter(|e| is_temp_name(&e.as_ref().unwrap().file_name())).count()
}

#[test]
fn the_new_file_appears_with_the_times_kept() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"old bytes");
    let before = stat(&p).unwrap().unwrap();
    let id = replace_keeping_times(&p, b"new bytes", Expect::Unchanged(before.identity), &before.times).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"new bytes");
    assert_eq!(modified(&p), old_time(), "the time is kept through the rename");
    assert!(id.modified_at(old_time()), "the identity returned says so");
    assert_eq!(identity(&p).unwrap(), Some(id), "the identity returned is the file's now");
    // The rename brings a new file (a new inode on Unix, a new file index on Windows), so the
    // identity is a new one even though the size and the time are the old ones (M8.27).
    assert_ne!(id, before.identity, "a new version");
    // On every platform a version of another size is a new identity, times kept or not.
    let longer = replace_keeping_times(&p, b"new bytes, longer", Expect::Unchanged(id), &before.times).unwrap();
    assert!(longer.modified_at(old_time()));
    assert_ne!(longer, id, "a new version");
    assert_eq!(identity(&p).unwrap(), Some(longer));
    let id = longer;
    assert_eq!(temps(dir.path()), 0);
    // A plain replace gives the time of the write, and its identity says so.
    let id = replace(&p, b"plain", Expect::Unchanged(id)).unwrap();
    assert!(!id.modified_at(old_time()));
    // A new file can be given a time too; the access time, when given, is set as well.
    let q = dir.path().join("new.jpg");
    let times = Times { modified: old_time(), accessed: Some(old_time() + Duration::from_secs(60)) };
    replace_keeping_times(&q, b"made", Expect::Absent, &times).unwrap();
    assert_eq!(modified(&q), old_time());
}

#[test]
fn the_target_checks_of_a_replace_still_apply() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"mine");
    let mine = stat(&p).unwrap().unwrap();
    // Another program rewrote it.
    fs::write(dir.path().join("other"), b"theirs!").unwrap();
    fs::rename(dir.path().join("other"), &p).unwrap();
    let theirs = modified(&p);
    assert!(matches!(replace_keeping_times(&p, b"x", Expect::Unchanged(mine.identity), &mine.times), Err(Error::Changed(_))));
    assert_eq!((fs::read(&p).unwrap(), modified(&p)), (b"theirs!".to_vec(), theirs));
    assert!(matches!(replace_keeping_times(&p, b"x", Expect::Absent, &mine.times), Err(Error::Changed(_))));
    assert_eq!(temps(dir.path()), 0);
    let id = identity(&p).unwrap().unwrap();
    let _ui = mark_ui_thread();
    assert!(matches!(replace_keeping_times(&p, b"x", Expect::Unchanged(id), &mine.times), Err(Error::OnUiThread)));
    drop(_ui);
    #[cfg(unix)]
    {
        let real = dir.path().join("real.jpg");
        old_file(&real, b"real");
        let link = dir.path().join("link.jpg");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let id = identity(&real).unwrap().unwrap();
        assert!(matches!(replace_keeping_times(&link, b"x", Expect::Unchanged(id), &mine.times), Err(Error::Link(_))));
        assert_eq!((fs::read(&real).unwrap(), modified(&real)), (b"real".to_vec(), old_time()));
    }
}

#[test]
fn the_points_are_a_replace_s_with_keep_times_before_the_fsync() {
    let mut expected: Vec<&str> = ATOMIC_POINTS.to_vec();
    let fsync = expected.iter().position(|p| *p == points::FSYNC).unwrap();
    expected.insert(fsync, KEEP_TIMES);
    assert_eq!(KEEPING_TIMES_POINTS.to_vec(), expected);
}

/// An error or a crash at every point: the target holds the old bytes with the old time or the
/// new bytes with the old time. The one exception is an error at [`KEEP_TIMES`] (a platform that
/// refuses the times on the temporary file): the write goes on, the identity returned says the
/// time was not kept, and the caller's `set_times` after the rename puts it back.
#[test]
fn every_fault_point_leaves_the_old_or_the_new_file_with_the_old_time() {
    let old: &[u8] = b"\xff\xd8\xff\xe1 old metadata, then the scan data";
    let new: &[u8] = b"\xff\xd8\xff\xe1 the new metadata, a little longer, then the scan data";
    let rename = KEEPING_TIMES_POINTS.iter().position(|p| *p == points::RENAME).unwrap();
    for (i, point) in KEEPING_TIMES_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("photo.jpg");
            old_file(&p, old);
            let before = stat(&p).unwrap().unwrap();
            let case = format!("{point} {fault:?}");
            arm(point, fault);
            let result = replace_keeping_times(&p, new, Expect::Unchanged(before.identity), &before.times);
            disarm_all();
            let now = fs::read(&p).unwrap();
            if point == KEEP_TIMES && fault == Fault::Error {
                let id = result.unwrap();
                assert_eq!(now, new, "{case}");
                assert!(!id.modified_at(old_time()), "{case}: the identity says the time was not kept");
                set_times(&p, &before.times, id).unwrap();
                assert_eq!(modified(&p), old_time(), "{case}: the fallback puts it back");
                continue;
            }
            let err = result.unwrap_err();
            match fault {
                Fault::Crash => assert!(matches!(err, Error::Crashed(c) if c == point), "{case}: {err}"),
                Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}: {err}"),
            }
            assert_eq!(now, if i > rename { new } else { old }, "{case}: the rename is when the new file appears");
            assert_eq!(modified(&p), old_time(), "{case}: old or new, with the old time");
            match fault {
                Fault::Error => assert_eq!(temps(dir.path()), 0, "{case}: an error cleans up"),
                Fault::Crash => assert!(temps(dir.path()) <= 1, "{case}"),
            }
        }
    }
}
