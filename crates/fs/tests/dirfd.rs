//! Writes through a held folder (`security.md` §5.6, §5.12): a folder swapped for a link, or for
//! another folder, between validation and write never receives the write; every step's fault
//! leaves the old or the new file.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all, points, run_at};
use dalica_fs::{Error, Expect, atomic::is_temp_name, identity, read, replace};

const NAME: &str = ".dalica.json";
const OLD: &[u8] = b"{ \"photos\": {} }\n";
const NEW: &[u8] = b"{ \"photos\": { \"a.jpg\": { \"star\": true } } }\n";

/// Makes `link` a symbolic link to the folder `target`; `false` where the platform refuses
/// (Windows without the symlink privilege), so the caller skips.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    match made {
        Ok(()) => true,
        Err(e) => {
            eprintln!("skipping: cannot create a folder link here: {e}");
            false
        }
    }
}

/// Every name in `dir`.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// A root with the folder `photos` (holding `OLD` when `existed`) and an empty folder `elsewhere`
/// that a swap points the path at.
struct Setup {
    _root: tempfile::TempDir,
    photos: PathBuf,
    moved: PathBuf,
    elsewhere: PathBuf,
    expect: Expect,
}

fn setup(existed: bool) -> Setup {
    let root = tempfile::tempdir().unwrap();
    let photos = root.path().join("photos");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&photos).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    let expect = if existed { Expect::Unchanged(replace(&photos.join(NAME), OLD, Expect::Absent).unwrap()) } else { Expect::Absent };
    let moved = root.path().join("photos.moved");
    Setup { _root: root, photos, moved, elsewhere, expect }
}

/// Swaps `photos` for a link to `elsewhere` when the write reaches `point`; returns whether the
/// swap happened (Windows cannot rename a folder holding an open file).
fn plant_link_swap(s: &Setup, point: &'static str) -> Rc<Cell<bool>> {
    let swapped = Rc::new(Cell::new(false));
    let (photos, moved, elsewhere, flag) = (s.photos.clone(), s.moved.clone(), s.elsewhere.clone(), Rc::clone(&swapped));
    run_at(point, move || {
        if fs::rename(&photos, &moved).is_ok() {
            if link_dir(&elsewhere, &photos) {
                flag.set(true);
            } else {
                fs::rename(&moved, &photos).unwrap();
            }
        }
    });
    swapped
}

#[test]
fn a_folder_swapped_for_a_link_never_receives_the_write() {
    {
        // Can this machine make folder links at all?
        let probe = tempfile::tempdir().unwrap();
        if !link_dir(probe.path(), &probe.path().join("probe")) {
            return;
        }
    }
    for point in ATOMIC_POINTS {
        for existed in [true, false] {
            let s = setup(existed);
            let swapped = plant_link_swap(&s, point);
            let result = replace(&s.photos.join(NAME), NEW, s.expect);
            disarm_all();
            let case = format!("{point} existed={existed}: {result:?}");
            if !swapped.get() {
                // The swap could not happen at this point (Windows: a file in the folder was open).
                assert!(result.is_ok(), "{case}");
                assert_eq!(fs::read(s.photos.join(NAME)).unwrap(), NEW, "{case}");
                continue;
            }
            // The folder the link points at never sees the file or a temporary file.
            assert!(names(&s.elsewhere).is_empty(), "{case}: {:?}", names(&s.elsewhere));
            // Whatever was written is in the original folder, whole.
            let landed = fs::read(s.moved.join(NAME)).ok();
            match &result {
                Ok(_) => assert_eq!(landed.as_deref(), Some(NEW), "{case}"),
                Err(_) => assert!(landed.as_deref() == Some(NEW) || landed.as_deref() == existed.then_some(OLD), "{case}"),
            }
            // Unix holds the folder: a swap before it is opened is refused as a link, and a swap
            // after it cannot stop the write landing in the original folder.
            #[cfg(unix)]
            {
                if point == points::OPEN_DIR {
                    assert!(matches!(result, Err(Error::Link(_))), "{case}");
                    assert_eq!(landed.as_deref(), existed.then_some(OLD), "{case}");
                } else {
                    assert!(result.is_ok(), "{case}");
                }
                assert!(names(&s.moved).iter().all(|n| !is_temp_name(n.as_ref())), "{case}: {:?}", names(&s.moved));
            }
        }
    }
}

/// Unix: a folder swapped for another real folder (same name, different folder) is just as
/// harmless: refused before the open, unseen after it.
#[cfg(unix)]
#[test]
fn a_folder_swapped_for_another_folder_never_receives_the_write() {
    for point in ATOMIC_POINTS {
        for existed in [true, false] {
            let s = setup(existed);
            let (photos, moved) = (s.photos.clone(), s.moved.clone());
            run_at(point, move || {
                fs::rename(&photos, &moved).unwrap();
                fs::create_dir(&photos).unwrap();
            });
            let result = replace(&s.photos.join(NAME), NEW, s.expect);
            disarm_all();
            let case = format!("{point} existed={existed}: {result:?}");
            assert!(names(&s.photos).is_empty(), "{case}: {:?}", names(&s.photos));
            let landed = fs::read(s.moved.join(NAME)).ok();
            if point == points::OPEN_DIR {
                assert!(matches!(result, Err(Error::Changed(_))), "{case}");
                assert_eq!(landed.as_deref(), existed.then_some(OLD), "{case}");
            } else {
                assert!(result.is_ok(), "{case}");
                assert_eq!(landed.as_deref(), Some(NEW), "{case}");
            }
        }
    }
}

/// A folder whose own name is a link when the write starts (a watched folder linked to another
/// drive) is followed once and written like any other; the target itself may never be a link.
#[test]
fn a_folder_that_is_a_link_from_the_start_is_written_in_its_target() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let linked = root.path().join("linked");
    if !link_dir(&real, &linked) {
        return;
    }
    let id = replace(&linked.join(NAME), OLD, Expect::Absent).unwrap();
    let id = replace(&linked.join(NAME), NEW, Expect::Unchanged(id)).unwrap();
    assert_eq!(fs::read(real.join(NAME)).unwrap(), NEW);
    assert_eq!(read(&real.join(NAME), 1 << 20).unwrap().unwrap().identity, id);
    assert_eq!(names(&real), [NAME]);
}

/// The target must be a folder that exists.
#[test]
fn a_missing_folder_or_a_file_as_folder_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(replace(&root.path().join("gone").join(NAME), NEW, Expect::Absent), Err(Error::Io { .. })));
    let file = root.path().join("file");
    fs::write(&file, b"x").unwrap();
    assert!(matches!(replace(&file.join(NAME), NEW, Expect::Absent), Err(Error::Io { .. })));
    assert_eq!(fs::read(&file).unwrap(), b"x");
}

/// `data-model.md` §10.5 with the new step: an error or a crash at every point, the folder open
/// included, leaves exactly the old or the new file, and the next write lands cleanly.
#[test]
fn each_fault_point_leaves_the_old_or_the_new_file() {
    let rename_at = ATOMIC_POINTS.iter().position(|p| *p == points::RENAME).unwrap();
    for (i, point) in ATOMIC_POINTS.into_iter().enumerate() {
        for fault in [Fault::Error, Fault::Crash] {
            for existed in [true, false] {
                let s = setup(existed);
                let p = s.photos.join(NAME);
                arm(point, fault);
                let err = replace(&p, NEW, s.expect).unwrap_err();
                disarm_all();
                let case = format!("{point} {fault:?} existed={existed}: {err}");
                match fault {
                    Fault::Crash => assert!(matches!(err, Error::Crashed(c) if c == point), "{case}"),
                    Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}"),
                }
                let want = if i > rename_at { Some(NEW) } else { existed.then_some(OLD) };
                assert_eq!(fs::read(&p).ok().as_deref(), want, "{case}");
                let temps = names(&s.photos).into_iter().filter(|n| is_temp_name(n.as_ref())).count();
                match fault {
                    Fault::Error => assert_eq!(temps, 0, "{case}"),
                    Fault::Crash => assert!(temps <= 1, "{case}"),
                }
                let expect = identity(&p).unwrap().map_or(Expect::Absent, Expect::Unchanged);
                replace(&p, b"after", expect).unwrap();
                assert_eq!(fs::read(&p).unwrap(), b"after", "{case}");
            }
        }
    }
}

/// The open-folder point is the first step: nothing has been created yet when it fails.
#[test]
fn the_folder_is_opened_before_anything_is_created() {
    assert_eq!(ATOMIC_POINTS.first(), Some(&points::OPEN_DIR));
    let s = setup(false);
    arm(points::OPEN_DIR, Fault::Crash);
    assert!(matches!(replace(&s.photos.join(NAME), NEW, Expect::Absent), Err(Error::Crashed(points::OPEN_DIR))));
    disarm_all();
    assert!(names(&s.photos).is_empty());
}
