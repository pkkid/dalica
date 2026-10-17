//! Removing a card's copied files (`organizing.md` §3, `security.md` §5.7): only a source file
//! unchanged since its copy and whose verified copy is still in place, to the trash when its
//! volume has one and otherwise deleted permanently; links, folders, paths outside the card and
//! hostile names refused; every fault point.

use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{names, old_file};
use dalica_fs::faults::{Fault, REMOVE_DELETE_POINTS, REMOVE_POINTS, arm, disarm_all, points};
use dalica_fs::remove::{Removed, remove_copied};
use dalica_fs::transfer::{Imported, Progress, import_copy};
use dalica_fs::trash::{self, Trashed};
use dalica_fs::{Error, Identity, guard, identity};

const JPEG: &[u8] = b"\xff\xd8\xff\xe0 a photo straight from the camera \xff\xd9";

fn id(path: &Path) -> Identity {
    identity(path).unwrap().unwrap()
}

/// Puts a trashed test item back (inside a temporary folder removed after the test), so the tests
/// leave nothing in the trash where the platform can restore.
fn untrash(t: &Trashed) {
    let _ = trash::restore(std::slice::from_ref(t));
}

/// A card (`<tmp>/card/DCIM`, the root) with `100TEST/IMG_0001.JPG` imported into `<tmp>/Pictures`.
struct Card {
    _dir: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    id: Identity,
    copy: PathBuf,
    copy_id: Identity,
}

impl Card {
    fn new() -> Card {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("card").join("DCIM");
        let folder = root.join("100TEST");
        let dest = dir.path().join("Pictures");
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir(&dest).unwrap();
        let source = folder.join("IMG_0001.JPG");
        old_file(&source, JPEG);
        let id = id(&source);
        let Imported::Copied(copy_id) = import_copy(&folder, "IMG_0001.JPG", id, &dest, "IMG_0001.JPG", &Progress::new()).unwrap() else {
            panic!("not copied")
        };
        Card { _dir: dir, root, source, id, copy: dest.join("IMG_0001.JPG"), copy_id }
    }

    fn remove(&self) -> dalica_fs::Result<Removed> {
        remove_copied(&self.root, &self.source, self.id, &self.copy, self.copy_id)
    }

    fn source_whole(&self, case: &str) {
        assert_eq!(fs::read(&self.source).unwrap(), JPEG, "{case}");
        assert_eq!(id(&self.source), self.id, "{case}");
    }

    fn copy_verified(&self, case: &str) {
        assert_eq!(fs::read(&self.copy).unwrap(), JPEG, "{case}");
        assert_eq!(id(&self.copy), self.copy_id, "{case}");
    }
}

#[test]
fn a_volume_with_a_trash_gets_the_file_there() {
    let card = Card::new();
    let r = card.remove().unwrap();
    let Removed::Trashed(t) = r else { panic!("{r:?}") };
    assert!(!card.source.exists());
    assert_eq!(t.original, fs::canonicalize(card.source.parent().unwrap()).unwrap().join("IMG_0001.JPG"));
    assert_eq!(t.identity, card.id);
    card.copy_verified("trashed");
    untrash(&t);
    assert_eq!(fs::read(&card.source).unwrap(), JPEG, "back from the trash");
}

#[test]
fn a_volume_without_a_trash_gets_the_file_deleted() {
    let card = Card::new();
    arm(points::TRASH_NO_TRASH, Fault::Error);
    let r = card.remove();
    disarm_all();
    assert_eq!(r.unwrap(), Removed::Deleted);
    assert!(!card.source.exists());
    assert!(names(card.source.parent().unwrap()).is_empty());
    card.copy_verified("deleted");
}

#[test]
fn a_file_changed_since_its_copy_or_whose_copy_changed_is_refused() {
    let card = Card::new();
    // The source rewritten on the card after its copy.
    fs::write(&card.source, b"a different picture").unwrap();
    let r = card.remove();
    assert!(matches!(r, Err(Error::Changed(_))), "{r:?}");
    assert_eq!(fs::read(&card.source).unwrap(), b"a different picture");

    // The copy rewritten, then gone.
    let card = Card::new();
    fs::write(&card.copy, b"edited elsewhere").unwrap();
    let r = card.remove();
    assert!(matches!(r, Err(Error::Changed(_))), "{r:?}");
    fs::remove_file(&card.copy).unwrap();
    arm(points::TRASH_NO_TRASH, Fault::Error);
    let r = card.remove();
    disarm_all();
    assert!(matches!(r, Err(Error::Changed(_))), "{r:?}");
    card.source_whole("copy gone");
}

#[test]
fn a_folder_a_path_outside_the_root_and_hostile_names_are_refused() {
    let card = Card::new();
    let folder = card.source.parent().unwrap().to_path_buf();
    let folder_id = id(&folder);
    let outside = card.root.parent().unwrap().join("outside.jpg");
    old_file(&outside, JPEG);
    let outside_id = id(&outside);
    let cases: Vec<(PathBuf, Identity)> = vec![
        (folder.clone(), folder_id),
        (outside.clone(), outside_id),
        (card.root.join("..").join("outside.jpg"), outside_id),
        (folder.join("..").join("100TEST").join("IMG_0001.JPG"), card.id),
        (card.root.clone(), folder_id),
        (folder.join(".dalica.json"), card.id),
    ];
    for (path, expect) in cases {
        arm(points::TRASH_NO_TRASH, Fault::Error);
        let r = remove_copied(&card.root, &path, expect, &card.copy, card.copy_id);
        disarm_all();
        assert!(matches!(&r, Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::InvalidInput), "{path:?}: {r:?}");
    }
    card.source_whole("refusals");
    assert_eq!(fs::read(&outside).unwrap(), JPEG);
    assert!(folder.is_dir());
}

#[cfg(unix)]
#[test]
fn a_link_is_refused_and_never_followed() {
    use std::os::unix::fs::symlink;
    let card = Card::new();
    let folder = card.source.parent().unwrap().to_path_buf();
    // The file itself a link.
    symlink(&card.source, folder.join("link.jpg")).unwrap();
    let r = remove_copied(&card.root, &folder.join("link.jpg"), card.id, &card.copy, card.copy_id);
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    assert!(fs::symlink_metadata(folder.join("link.jpg")).unwrap().file_type().is_symlink());
    // A folder between the root and the file a link.
    symlink(&folder, card.root.join("200LINK")).unwrap();
    let r = remove_copied(&card.root, &card.root.join("200LINK").join("IMG_0001.JPG"), card.id, &card.copy, card.copy_id);
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    // The root a link.
    let root_link = card.root.with_file_name("DCIM-link");
    symlink(&card.root, &root_link).unwrap();
    let r = remove_copied(&root_link, &root_link.join("100TEST").join("IMG_0001.JPG"), card.id, &card.copy, card.copy_id);
    assert!(matches!(r, Err(Error::Link(_))), "{r:?}");
    card.source_whole("links");
}

#[test]
fn every_fault_point_leaves_the_source_whole_or_removed_beside_its_verified_copy() {
    for (no_trash, list) in [(false, &REMOVE_POINTS[..]), (true, &REMOVE_DELETE_POINTS[..])] {
        for &point in list {
            for fault in [Fault::Error, Fault::Crash] {
                let card = Card::new();
                let case = format!("no trash {no_trash}, {point} as {fault:?}");
                if no_trash {
                    arm(points::TRASH_NO_TRASH, Fault::Error);
                }
                arm(point, fault);
                let r = card.remove();
                disarm_all();
                assert!(r.is_err(), "{case}: {r:?}");
                if card.source.exists() {
                    card.source_whole(&case);
                }
                card.copy_verified(&case);
                let removed_already = [points::REMOVE_FSYNC_DIR, points::REMOVE_VERIFY];
                if !removed_already.contains(&point) {
                    card.source_whole(&format!("{case}: the source stays until the step that removes it"));
                }
            }
        }
    }
}

#[test]
fn the_ui_thread_is_refused() {
    let card = Card::new();
    let _ui = guard::mark_ui_thread();
    let r = card.remove();
    if cfg!(debug_assertions) {
        assert!(matches!(r, Err(Error::OnUiThread)), "{r:?}");
    }
}
