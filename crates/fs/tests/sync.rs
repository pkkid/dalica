//! Real `fsync`. A test build skips the system call (the `faults` module docs: a simulated crash
//! never drops the page cache, so a real sync proves nothing there); this test turns real syncing
//! on for its thread and checks that every kind of sync the crate makes works on this OS (a written
//! file, an appended one, a pack, a copy, a folder) and that an atomic replace makes them in their
//! order: the temporary file, then the rename, then the folder (Unix; Windows journals renames).

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use dalica_fs::atomic::is_temp_name;
use dalica_fs::faults::{self, Fault, arm, disarm_all, points, run_at};
use dalica_fs::transfer::{Moved, Outcome, move_file};
use dalica_fs::{Appender, Error, Expect, PackFile, TempFile, create, identity, replace};

#[test]
fn real_syncs_work_and_come_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_path_buf();
    let target = folder.join("photo.jpg");
    faults::real_syncs(true);

    // An atomic replace: the temporary file is synced before the rename, the folder after it.
    let at_rename: Rc<RefCell<Option<Vec<PathBuf>>>> = Rc::default();
    let seen = Rc::clone(&at_rename);
    run_at(points::RENAME, move || *seen.borrow_mut() = Some(faults::synced()));
    replace(&target, b"new bytes", Expect::Absent).unwrap();
    disarm_all();
    let before_rename = at_rename.borrow_mut().take().expect("the rename was reached");
    let synced = faults::synced();
    assert_eq!(before_rename.len(), 1, "the temporary file, before the rename: {before_rename:?}");
    let tmp = &before_rename[0];
    assert!(tmp.file_name().is_some_and(is_temp_name) && tmp.parent() == Some(folder.as_path()), "{tmp:?}");
    if cfg!(unix) {
        assert_eq!(synced.len(), 2, "then the folder, after the rename: {synced:?}");
        assert_eq!(fs::canonicalize(&synced[1]).unwrap(), fs::canonicalize(&folder).unwrap());
    } else {
        assert_eq!(synced, before_rename, "Windows journals the rename itself");
    }
    // An armed error at a sync point still fails it with real syncing on.
    let id = identity(&target).unwrap().unwrap();
    arm(points::FSYNC, Fault::Error);
    assert!(matches!(replace(&target, b"never", Expect::Unchanged(id)), Err(Error::Io { op: "fsync", .. })));
    disarm_all();
    assert_eq!(fs::read(&target).unwrap(), b"new bytes");

    // Every other kind of sync, on real handles.
    let count = || faults::synced().len();
    let n = count();
    create::new_file(&folder, "new.jpg", b"a new file").unwrap();
    assert!(count() > n, "a new file");
    let n = count();
    let mut log = Appender::open(&folder.join("log.jsonl"), 1 << 20).unwrap();
    log.append(b"{}\n").unwrap();
    assert!(count() > n, "an append");
    let n = count();
    let mut pack = PackFile::open(&folder.join("1.pack")).unwrap();
    pack.append(b"thumbnail").unwrap();
    pack.sync().unwrap();
    assert!(count() > n, "a pack");
    let n = count();
    let mut temp = TempFile::create(&folder.join("2.pack")).unwrap();
    temp.write(b"rewritten pack").unwrap();
    temp.commit().unwrap();
    assert!(count() > n, "a rewritten pack");
    let n = count();
    let dest = folder.join("dest");
    fs::create_dir(&dest).unwrap();
    let id = identity(&folder.join("new.jpg")).unwrap().unwrap();
    arm(points::MOVE_CROSS_VOLUME, Fault::Error);
    let moved = move_file(&folder, "new.jpg", id, &dest, "new.jpg").unwrap();
    disarm_all();
    assert!(matches!(moved, Outcome::Done(Moved::Copied { .. })), "{moved:?}");
    assert_eq!(fs::read(dest.join("new.jpg")).unwrap(), b"a new file");
    assert!(count() > n, "a copy");

    faults::real_syncs(false);
    let n = count();
    replace(&target, b"skipped", Expect::Unchanged(identity(&target).unwrap().unwrap())).unwrap();
    assert_eq!(count(), n, "off again");
}
