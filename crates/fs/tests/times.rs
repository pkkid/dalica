//! A file's times (`times`): `stat` without following links, `set_times` only on the version the
//! caller wrote, never through a link, with its fault point; and the sequence a metadata write
//! makes (atomic replace, then the old time put back) leaves the old or the new file at every
//! fault point, with the old time once it is done.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all};
use dalica_fs::times::{SET_TIMES, TIMES_POINTS};
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

#[test]
fn stat_describes_a_file_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    assert_eq!(stat(&p).unwrap(), None);
    old_file(&p, b"jpeg");
    let s = stat(&p).unwrap().unwrap();
    assert_eq!(Some(s.identity), identity(&p).unwrap());
    assert_eq!(s.times.modified, old_time());
    assert!(s.is_file && s.writable);
    let d = stat(dir.path()).unwrap().unwrap();
    assert!(!d.is_file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        assert!(!stat(&p).unwrap().unwrap().writable, "a file without write permission is read-only");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.path().join("link.jpg");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(matches!(stat(&link), Err(Error::Link(_))));
    }
    #[cfg(not(unix))]
    {
        let mut perms = fs::metadata(&p).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&p, perms.clone()).unwrap();
        assert!(!stat(&p).unwrap().unwrap().writable, "a read-only file");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&p, perms).unwrap();
    }
}

#[test]
fn set_times_puts_the_old_time_back_on_the_version_written() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"old bytes");
    let before = stat(&p).unwrap().unwrap();
    let written = replace(&p, b"new bytes", Expect::Unchanged(before.identity)).unwrap();
    assert_ne!(modified(&p), old_time(), "a replace gives the file the time of the write");
    let after = set_times(&p, &before.times, written).unwrap();
    assert_eq!(modified(&p), old_time());
    assert_eq!(identity(&p).unwrap(), Some(after), "the identity returned is the file's now");
    assert_ne!(after, written, "the time is part of the identity");
    assert_eq!(fs::read(&p).unwrap(), b"new bytes");
    // Only a time: no access time given leaves it to the system.
    let later = Times { modified: old_time() + Duration::from_secs(3600), accessed: None };
    set_times(&p, &later, after).unwrap();
    assert_eq!(modified(&p), later.modified);
}

#[test]
fn set_times_refuses_a_file_that_changed_vanished_or_is_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"mine");
    let mine = identity(&p).unwrap().unwrap();
    let times = Times { modified: old_time() + Duration::from_secs(60), accessed: None };
    // Another program rewrote it.
    fs::write(dir.path().join("other"), b"theirs!").unwrap();
    fs::rename(dir.path().join("other"), &p).unwrap();
    let theirs = modified(&p);
    assert!(matches!(set_times(&p, &times, mine), Err(Error::Changed(_))));
    assert_eq!(modified(&p), theirs, "their file keeps its time");
    // It is gone.
    let id = identity(&p).unwrap().unwrap();
    fs::remove_file(&p).unwrap();
    assert!(matches!(set_times(&p, &times, id), Err(Error::Changed(_))));
    assert!(!p.exists(), "nothing is created");
    #[cfg(unix)]
    {
        let real = dir.path().join("real.jpg");
        old_file(&real, b"real");
        let id = identity(&real).unwrap().unwrap();
        std::os::unix::fs::symlink(&real, &p).unwrap();
        assert!(matches!(set_times(&p, &times, id), Err(Error::Link(_))));
        assert_eq!(modified(&real), old_time(), "never through a link");
    }
}

#[test]
fn the_ui_thread_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("photo.jpg");
    old_file(&p, b"x");
    let id = identity(&p).unwrap().unwrap();
    let times = Times { modified: SystemTime::now(), accessed: None };
    let _ui = mark_ui_thread();
    assert!(matches!(stat(&p), Err(Error::OnUiThread)));
    assert!(matches!(set_times(&p, &times, id), Err(Error::OnUiThread)));
    drop(_ui);
    assert_eq!(modified(&p), old_time());
}

#[test]
fn a_fault_at_set_times_leaves_the_file_and_its_time_as_they_were() {
    assert_eq!(TIMES_POINTS, [SET_TIMES]);
    for fault in [Fault::Error, Fault::Crash] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("photo.jpg");
        fs::write(&p, b"new").unwrap();
        let id = identity(&p).unwrap().unwrap();
        let was = modified(&p);
        arm(SET_TIMES, fault);
        let err = set_times(&p, &Times { modified: old_time(), accessed: None }, id).unwrap_err();
        disarm_all();
        match fault {
            Fault::Error => assert!(matches!(err, Error::Io { op: "set times", .. }), "{err}"),
            Fault::Crash => assert!(matches!(err, Error::Crashed(SET_TIMES)), "{err}"),
        }
        assert_eq!((fs::read(&p).unwrap(), modified(&p)), (b"new".to_vec(), was));
        // Tried again, it works.
        set_times(&p, &Times { modified: old_time(), accessed: None }, id).unwrap();
        assert_eq!(modified(&p), old_time());
    }
}

/// The metadata write's sequence, `replace` then `set_times`, with an error or a crash at each of
/// its points: the file holds exactly the old or the new bytes, and once the write is made again
/// (as the metadata write queue does after a failure: it re-reads the file, and remembers the old
/// time of a write it started until the time is back) it holds the new bytes with the old time.
#[test]
fn every_fault_point_of_a_write_that_keeps_the_time_leaves_the_old_or_the_new_file() {
    let old: &[u8] = b"\xff\xd8\xff\xe1 old metadata, then the scan data";
    let new: &[u8] = b"\xff\xd8\xff\xe1 the new metadata, a little longer, then the scan data";
    let points = ATOMIC_POINTS.iter().chain(TIMES_POINTS.iter());
    for &point in points {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("photo.jpg");
            old_file(&p, old);
            let case = format!("{point} {fault:?}");
            let write = |p: &Path| -> Result<(), Error> {
                let before = stat(p)?.ok_or_else(|| Error::Changed(p.to_path_buf()))?;
                let bytes = dalica_fs::read(p, 1 << 20)?.map(|r| r.bytes).unwrap_or_default();
                if bytes == new {
                    // Made already; the time (remembered from the first try) may still be the write's.
                    set_times(p, &Times { modified: old_time(), accessed: None }, before.identity)?;
                    return Ok(());
                }
                let written = replace(p, new, Expect::Unchanged(before.identity))?;
                set_times(p, &before.times, written)?;
                Ok(())
            };
            arm(point, fault);
            let first = write(&p);
            disarm_all();
            assert!(first.is_err(), "{case}: the armed point fired");
            let now = fs::read(&p).unwrap();
            assert!(now == old || now == new, "{case}: never a torn file");
            if now == old {
                assert_eq!(modified(&p), old_time(), "{case}: the old file keeps its time");
            }
            // The write again: the new bytes, the old time.
            write(&p).unwrap();
            assert_eq!(fs::read(&p).unwrap(), new, "{case}");
            assert_eq!(modified(&p), old_time(), "{case}");
        }
    }
}
