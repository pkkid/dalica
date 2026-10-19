//! Undo Save and Revert from a legacy original (`roadmap.md` M14.14; `data-model.md` §6 and §9):
//! the original kept by the previous organizer is copied back with its exact bytes and time, the
//! saved file goes to the (test) trash, the legacy originals folder is left byte for byte and time
//! for time as it was; the identity checks refuse with nothing changed; a fault before the copy
//! and one after it leave the photo readable and the saved bytes in place or in the trash.

use crate::common;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use common::*;
use dalica_create::save::legacy::{Kept, restore};
use dalica_create::save::{Error, Options, digest};
use dalica_fs::faults::{self, Fault, points};
use dalica_fs::trash::{self, Restore, Trashed};
use dalica_sidecar::legacy::{originals_dir_name, sidecar_names};

/// A photo the previous organizer saved: the saved file at `NAME`, its original in the legacy
/// originals folder with an older time, and that folder's sidecar.
struct Saved {
    _dir: dalica_fs::TempDir,
    folder: PathBuf,
    saved: Vec<u8>,
    original: Vec<u8>,
}

impl Saved {
    fn new() -> Saved {
        let dir = folder();
        let folder = dir.path().to_path_buf();
        let original = jpeg(1, 64, 48, 90, 1);
        let saved = jpeg(2, 40, 30, 80, 1);
        std::fs::write(folder.join(NAME), &saved).unwrap();
        let legacy = folder.join(originals_dir_name());
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join(NAME), &original).unwrap();
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_500_000_000);
        std::fs::File::options().write(true).open(legacy.join(NAME)).unwrap().set_modified(old).unwrap();
        std::fs::write(legacy.join(&sidecar_names()[0]), format!("[{NAME}]\r\nfilters=sepia=1;\r\n")).unwrap();
        Saved { _dir: dir, folder, saved, original }
    }

    fn photo(&self) -> PathBuf {
        self.folder.join(NAME)
    }

    fn kept(&self) -> Kept {
        Kept { size: self.original.len() as u64, digest: digest(&self.original) }
    }

    /// Options that expect the saved file as it is now.
    fn options(&self) -> Options {
        Options { seen: Some(seen(&self.photo())), ..options() }
    }

    /// Every file of the legacy originals folder with its bytes and time.
    fn legacy(&self) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
        let mut out: Vec<_> = std::fs::read_dir(self.folder.join(originals_dir_name()))
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.clone(), read(&p), std::fs::metadata(&p).unwrap().modified().unwrap())
            })
            .collect();
        out.sort();
        out
    }
}

/// The saved bytes are in the trash: with the photo's name set free, `trashed` comes back from it
/// holding them (where the platform can list its trash).
fn in_trash(photo: &Path, trashed: &Trashed, saved: &[u8]) {
    let aside = photo.with_file_name("aside.jpg");
    std::fs::rename(photo, &aside).unwrap();
    let back = trash::restore(std::slice::from_ref(trashed)).unwrap();
    if back == [Restore::Unsupported] {
        std::fs::rename(&aside, photo).unwrap();
        return;
    }
    assert_eq!(back, [Restore::Restored]);
    assert_eq!(read(photo), saved, "the saved file, whole, from the trash");
    std::fs::remove_file(photo).unwrap();
    std::fs::rename(&aside, photo).unwrap();
}

#[test]
fn revert_gives_the_originals_exact_bytes_and_the_saved_file_goes_to_the_trash() {
    faults::disarm_all();
    let s = Saved::new();
    let legacy = s.legacy();
    let original_time = std::fs::metadata(s.folder.join(originals_dir_name()).join(NAME)).unwrap().modified().unwrap();

    let done = restore(&s.folder, NAME, &s.kept(), &s.options()).unwrap();
    assert_eq!(read(s.photo()), s.original, "the original's exact bytes");
    assert_eq!(std::fs::metadata(s.photo()).unwrap().modified().unwrap(), original_time, "and its time");
    assert_eq!(done.identity.size(), s.original.len() as u64);
    assert_eq!(s.legacy(), legacy, "the legacy originals folder is byte for byte and time for time as it was");
    let trashed = done.trashed.expect("the saved file went to the trash");
    in_trash(&s.photo(), &trashed, &s.saved);
}

#[test]
fn a_changed_photo_or_original_is_refused_with_nothing_changed() {
    faults::disarm_all();
    let s = Saved::new();
    let legacy = s.legacy();
    let unchanged = |s: &Saved| {
        assert_eq!(read(s.photo()), s.saved);
        assert_eq!(s.legacy(), legacy);
    };
    // The kept original is not the one the scan found.
    let other = Kept { digest: digest(b"something else"), ..s.kept() };
    assert!(matches!(restore(&s.folder, NAME, &other, &s.options()), Err(Error::Record(_))));
    let unknown = Kept { digest: String::new(), ..s.kept() };
    assert!(matches!(restore(&s.folder, NAME, &unknown, &s.options()), Err(Error::Record(_))));
    unchanged(&s);
    // The saved file is not the version the library saw.
    let stale = Options { seen: Some(dalica_create::save::Seen { size: 1, ..seen(&s.photo()) }), ..options() };
    assert!(matches!(restore(&s.folder, NAME, &s.kept(), &stale), Err(Error::Changed(_))));
    unchanged(&s);
    // Names that are not one plain component, and a photo with no legacy original.
    for name in ["../x.jpg", "", ".dalica-originals"] {
        assert!(matches!(restore(&s.folder, name, &s.kept(), &s.options()), Err(Error::Name(_))), "{name:?}");
    }
    std::fs::write(s.folder.join("other.jpg"), b"x").unwrap();
    assert!(matches!(restore(&s.folder, "other.jpg", &s.kept(), &options()), Err(Error::NotSaved(_))));
    unchanged(&s);
}

/// One fault before the copy is committed (its write) and one after it (its verification).
#[test]
fn a_fault_before_or_after_the_copy_leaves_the_photo_readable_and_the_saved_bytes_kept() {
    faults::disarm_all();
    // Before: the saved file comes back from the trash; nothing changed.
    let s = Saved::new();
    let legacy = s.legacy();
    faults::arm(points::WRITE, Fault::Error);
    let e = restore(&s.folder, NAME, &s.kept(), &s.options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(e, Error::Fs(_)), "{e:?}");
    assert_eq!(read(s.photo()), s.saved, "the saved file is back in place");
    assert_eq!(s.legacy(), legacy);
    // And the restore still works afterwards.
    restore(&s.folder, NAME, &s.kept(), &s.options()).unwrap();
    assert_eq!(read(s.photo()), s.original);

    // After: the copy is at the name, the saved file in the trash, and the error says so.
    let s = Saved::new();
    let legacy = s.legacy();
    let options = s.options();
    let was = dalica_fs::identity(&s.photo()).unwrap().unwrap();
    faults::arm(points::VERIFY, Fault::Error);
    let e = restore(&s.folder, NAME, &s.kept(), &options).unwrap_err();
    faults::disarm_all();
    assert!(e.reason().contains("trash"), "{}", e.reason());
    assert_eq!(read(s.photo()), s.original, "the photo is readable: the original's bytes");
    assert_eq!(s.legacy(), legacy);
    let trashed = Trashed { original: std::fs::canonicalize(&s.folder).unwrap().join(NAME), identity: was, folder: false };
    in_trash(&s.photo(), &trashed, &s.saved);
}
