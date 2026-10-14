//! The OS trash (`security.md` §5.7): a file or folder sent there comes back on Linux and Windows
//! (and restoring is `Unsupported` on macOS), a volume without a trash answers `NoTrash` with the
//! item in place, nothing at an old path is ever replaced, a link goes as itself, odd paths are
//! refused.

use std::fs;
use std::path::Path;

use dalica_fs::faults::{Fault, TRASH_POINTS, arm, disarm_all, points};
use dalica_fs::trash::{Restore, Sent, Trashed, restore, send};
use dalica_fs::{Error, guard, identity};

/// Whether this platform can list its trash, so restoring works.
const RESTORES: bool = cfg!(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))));

fn trashed(path: &Path) -> Trashed {
    match send(path).unwrap() {
        Sent::Trashed(t) => t,
        Sent::NoTrash => panic!("{} has no trash", path.display()),
    }
}

#[test]
fn a_trashed_file_comes_back_where_the_platform_allows() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"the photo").unwrap();
    let before = identity(&photo).unwrap().unwrap();
    let t = trashed(&photo);
    assert!(!photo.exists());
    assert_eq!(t.original, fs::canonicalize(dir.path()).unwrap().join("photo.jpg"));
    assert_eq!(t.identity, before);
    assert!(!t.folder);
    let back = restore(std::slice::from_ref(&t)).unwrap();
    if RESTORES {
        assert_eq!(back, [Restore::Restored]);
        assert_eq!(fs::read(&photo).unwrap(), b"the photo");
        // Restored once; a second time it is no longer in the trash.
        assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::NotInTrash]);
    } else {
        assert_eq!(back, [Restore::Unsupported]);
        assert!(!photo.exists());
    }
}

#[test]
fn a_trashed_folder_comes_back_with_everything_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let trip = dir.path().join("Trip");
    fs::create_dir_all(trip.join("Day 2")).unwrap();
    fs::write(trip.join("a.jpg"), b"a").unwrap();
    fs::write(trip.join("Day 2").join("b.jpg"), b"b").unwrap();
    let t = trashed(&trip);
    assert!(t.folder);
    assert!(!trip.exists());
    let back = restore(std::slice::from_ref(&t)).unwrap();
    if RESTORES {
        assert_eq!(back, [Restore::Restored]);
        assert_eq!(fs::read(trip.join("a.jpg")).unwrap(), b"a");
        assert_eq!(fs::read(trip.join("Day 2").join("b.jpg")).unwrap(), b"b");
    } else {
        assert_eq!(back, [Restore::Unsupported]);
    }
}

#[test]
fn restoring_never_replaces_what_is_at_the_old_path() {
    if !RESTORES {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"trashed").unwrap();
    let t = trashed(&photo);
    fs::write(&photo, b"a new file of the same name").unwrap();
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Exists]);
    assert_eq!(fs::read(&photo).unwrap(), b"a new file of the same name");
    // Once the name is free again, it comes back (and leaves the trash).
    fs::rename(&photo, dir.path().join("kept.jpg")).unwrap();
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored]);
    assert_eq!(fs::read(&photo).unwrap(), b"trashed");
}

/// Where the trashed object can be looked at, a newer item of the same name is never taken for an
/// older one (the identity decides, not the time).
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
#[test]
fn each_item_comes_back_as_itself_among_items_of_one_name() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"first").unwrap();
    let first = trashed(&photo);
    fs::write(&photo, b"second, trashed later").unwrap();
    let second = trashed(&photo);
    assert_eq!(restore(&[first.clone(), second.clone()]).unwrap(), [Restore::Restored, Restore::Exists]);
    assert_eq!(fs::read(&photo).unwrap(), b"first");
    fs::remove_file(&photo).unwrap();
    assert_eq!(restore(&[second, first]).unwrap(), [Restore::Restored, Restore::NotInTrash]);
    assert_eq!(fs::read(&photo).unwrap(), b"second, trashed later");
}

#[test]
fn a_volume_without_a_trash_answers_no_trash_and_the_item_stays() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"kept").unwrap();
    arm(points::TRASH_NO_TRASH, Fault::Error);
    let r = send(&photo);
    disarm_all();
    assert_eq!(r.unwrap(), Sent::NoTrash);
    assert_eq!(fs::read(&photo).unwrap(), b"kept");
    arm(points::TRASH_NO_TRASH, Fault::Crash);
    let r = send(&photo);
    disarm_all();
    assert!(matches!(r, Err(Error::Crashed(_))), "{r:?}");
    assert_eq!(fs::read(&photo).unwrap(), b"kept");
}

#[test]
fn each_fault_point_leaves_the_item_where_it_was() {
    for fault in [Fault::Error, Fault::Crash] {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.jpg");
        fs::write(&photo, b"photo").unwrap();
        arm(points::TRASH_SEND, fault);
        let r = send(&photo);
        disarm_all();
        assert!(r.is_err(), "{fault:?}: {r:?}");
        assert_eq!(fs::read(&photo).unwrap(), b"photo");
        // Restoring with its point armed leaves the item in the trash.
        let t = trashed(&photo);
        arm(points::TRASH_RESTORE, fault);
        let r = restore(std::slice::from_ref(&t));
        disarm_all();
        assert!(r.is_err(), "{fault:?}: {r:?}");
        assert!(!photo.exists());
        let _ = restore(std::slice::from_ref(&t));
    }
    assert_eq!(TRASH_POINTS, [points::TRASH_SEND, points::TRASH_RESTORE]);
}

#[test]
fn a_link_goes_as_itself_and_odd_paths_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [Path::new("relative.jpg"), Path::new("/"), &dir.path().join(".."), &dir.path().join("missing.jpg")] {
        assert!(matches!(send(bad), Err(Error::Io { .. })), "{}", bad.display());
    }
    #[cfg(unix)]
    {
        let target = dir.path().join("target.jpg");
        fs::write(&target, b"target").unwrap();
        let link = dir.path().join("link.jpg");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // A link goes to the trash as itself; its target stays.
        let t = trashed(&link);
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"target");
        assert!(!t.folder);
        if RESTORES {
            assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored]);
            assert_eq!(fs::read_link(&link).unwrap(), target);
        }
    }
    assert_eq!(restore(&[]).unwrap(), []);
}

#[test]
fn the_ui_thread_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"photo").unwrap();
    let _ui = guard::mark_ui_thread();
    assert!(matches!(send(&photo), Err(Error::OnUiThread)));
    assert!(matches!(restore(&[]), Err(Error::OnUiThread)));
    assert!(photo.exists());
}
