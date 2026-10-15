//! The OS trash (`security.md` §5.7): a file or folder sent there comes back on Linux and Windows
//! (and restoring is `Unsupported` on macOS), a volume without a trash answers `NoTrash` with the
//! item in place, nothing at an old path is ever replaced, a link goes as itself, odd paths are
//! refused.

use std::fs;
use std::path::Path;

use dalica_fs::faults::{self, Fault, TRASH_POINTS, arm, disarm_all, points};
use dalica_fs::trash::{Restore, Sent, Trashed, restore, send};
use dalica_fs::{Error, guard, identity};

/// Whether this platform can list its trash, so restoring works.
const RESTORES: bool = cfg!(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))));

/// What the platform's trash lists for items that came from under `dir` (for failure messages).
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn listing(dir: &Path) -> String {
    let mark = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ::trash::os_limited::list() {
        Ok(items) => items
            .iter()
            .filter(|t| t.original_parent.to_string_lossy().to_lowercase().contains(&mark))
            .map(|t| format!("{{id {:?}, name {:?}, parent {:?}, deleted {}}}", t.id, t.name, t.original_parent, t.time_deleted))
            .collect::<Vec<_>>()
            .join("; "),
        Err(e) => format!("listing failed: {e}"),
    }
}

#[cfg(not(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android")))))]
fn listing(_dir: &Path) -> String {
    String::from("(this platform cannot list its trash)")
}

fn trashed(path: &Path) -> Trashed {
    match send(path).unwrap() {
        Sent::Trashed(t) => t,
        Sent::NoTrash => panic!("{} has no trash", path.display()),
    }
}

/// A file, a folder (with everything in it) and a link go to the trash and come back where the
/// platform allows (`Unsupported` on macOS); a second restore finds nothing; nothing at an old path
/// is ever replaced; where the trashed object can be looked at, a newer item of the same name is
/// never taken for an older one (the identity decides, not the time). Everything sent comes back,
/// so the trash is left as it was (where it can be listed).
fn round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    fs::write(&photo, b"the photo").unwrap();
    let before = identity(&photo).unwrap().unwrap();
    let t = trashed(&photo);
    assert!(!photo.exists());
    assert_eq!(t.original, fs::canonicalize(dir.path()).unwrap().join("photo.jpg"));
    assert_eq!(t.identity, before);
    assert!(!t.folder);
    if !RESTORES {
        assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Unsupported]);
        assert!(!photo.exists());
        return;
    }
    // Something at the old path stays, and the item stays in the trash until the name is free.
    fs::write(&photo, b"a new file of the same name").unwrap();
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Exists], "trashed {t:?}; the trash listed: {}", listing(dir.path()));
    assert_eq!(fs::read(&photo).unwrap(), b"a new file of the same name");
    fs::rename(&photo, dir.path().join("kept.jpg")).unwrap();
    let listed = listing(dir.path());
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored], "trashed {t:?}; the trash listed: {listed}");
    assert_eq!(fs::read(&photo).unwrap(), b"the photo");
    // Restored once; a second time it is no longer in the trash.
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::NotInTrash]);

    let trip = dir.path().join("Trip");
    fs::create_dir_all(trip.join("Day 2")).unwrap();
    fs::write(trip.join("a.jpg"), b"a").unwrap();
    fs::write(trip.join("Day 2").join("b.jpg"), b"b").unwrap();
    let t = trashed(&trip);
    assert!(t.folder);
    assert!(!trip.exists());
    let listed = listing(dir.path());
    assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored], "trashed {t:?}; the trash listed: {listed}");
    assert_eq!(fs::read(trip.join("a.jpg")).unwrap(), b"a");
    assert_eq!(fs::read(trip.join("Day 2").join("b.jpg")).unwrap(), b"b");

    #[cfg(unix)]
    {
        let link = dir.path().join("link.jpg");
        std::os::unix::fs::symlink(&photo, &link).unwrap();
        // A link goes to the trash as itself; its target stays.
        let t = trashed(&link);
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(fs::read(&photo).unwrap(), b"the photo");
        assert!(!t.folder);
        assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored]);
        assert_eq!(fs::read_link(&link).unwrap(), photo);
    }

    // Where the trashed object can be looked at (not the Windows shell's display names).
    if cfg!(all(unix, not(target_vendor = "apple"), not(target_os = "android"))) {
        fs::remove_file(&photo).unwrap();
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
}

/// The OS trash itself, opted in to from this test only (a test build uses a stand-in elsewhere;
/// the `faults` module docs).
#[test]
fn the_os_trash_round_trips() {
    faults::real_trash(true);
    round_trips();
    faults::real_trash(false);
}

/// Linux and the BSDs: a 255-byte name, too long for the record the `trash` crate would make
/// (`<name>.trashinfo`), goes to the OS trash under a shortened name, is listed under its own, and
/// comes back with its bytes (a file and a folder).
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
#[test]
fn a_255_byte_name_goes_to_the_os_trash_and_comes_back() {
    faults::real_trash(true);
    let dir = tempfile::tempdir().unwrap();
    let parent = fs::canonicalize(dir.path()).unwrap();
    for folder in [false, true] {
        let name = format!("{}{}", if folder { "f" } else { "p" }, "é".repeat(125)) + if folder { "abcd" } else { ".jpg" };
        assert_eq!(name.len(), 255);
        let path = dir.path().join(&name);
        if folder {
            fs::create_dir(&path).unwrap();
            fs::write(path.join("a.jpg"), b"inside").unwrap();
        } else {
            fs::write(&path, b"the photo").unwrap();
        }
        let t = trashed(&path);
        assert!(fs::symlink_metadata(&path).is_err(), "{name}");
        assert_eq!((t.original.clone(), t.folder), (parent.join(&name), folder));
        let listed = ::trash::os_limited::list().unwrap();
        assert!(
            listed.iter().any(|i| i.name == std::ffi::OsStr::new(&name) && i.original_parent == parent),
            "listed under its own name: {}",
            listing(dir.path())
        );
        assert_eq!(restore(std::slice::from_ref(&t)).unwrap(), [Restore::Restored], "the trash listed: {}", listing(dir.path()));
        if folder {
            assert_eq!(fs::read(path.join("a.jpg")).unwrap(), b"inside");
        } else {
            assert_eq!(fs::read(&path).unwrap(), b"the photo");
        }
    }
    faults::real_trash(false);
}

/// The stand-in trash every other test uses gives the OS trash's answers.
#[test]
fn the_stand_in_trash_round_trips_as_the_os_trash_does() {
    round_trips();
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
fn odd_paths_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    for bad in [Path::new("relative.jpg"), Path::new("/"), &dir.path().join(".."), &dir.path().join("missing.jpg")] {
        assert!(matches!(send(bad), Err(Error::Io { .. })), "{}", bad.display());
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
