//! A failed journal append never loses the records after it (`journal` module docs): the failed
//! op is not counted and leaves nothing on disk, so the ops appended after it are replayed on the
//! next open, with nothing to cut off.

use dalica_core::Timestamp;
use dalica_fs::faults::{arm_full, disarm_all, points};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_store::{Store, journal};

use crate::common::NOW;

#[test]
fn a_full_disk_on_one_append_keeps_every_later_op() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    let add = Op::AddFolder { folder: Folder { id: folder, path: d.join("photos") } };
    lib.apply(add.clone()).unwrap();
    let photo = |n: u64| {
        let id = dalica_core::PhotoId::new(n);
        Photo { id, folder, name: format!("IMG_{n:04}.jpg"), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false }
    };
    let photos: Vec<Photo> = (1..=3).map(photo).collect();
    let ids: Vec<_> = photos.iter().map(|p| p.id).collect();
    let caption = |i: usize| Op::SetCaptions { captions: vec![(ids[i], format!("caption {i}"))] };

    let mut store = Store::open(d, NOW).unwrap().store;
    store.append(&add).unwrap();
    store.append(&Op::AddPhotos { photos }).unwrap();
    arm_full(points::APPEND_WRITE);
    assert!(store.append(&caption(0)).is_err());
    disarm_all();
    assert_eq!(store.seq(), 2, "the failed op is not counted");
    assert_eq!(store.append(&caption(1)).unwrap(), 3);
    assert_eq!(store.append(&caption(2)).unwrap(), 4);
    drop(store);

    let bytes = std::fs::read(d.join(journal::FILE)).unwrap();
    assert_eq!(journal::read(&bytes).stop, None);
    let opened = Store::open(d, NOW).unwrap();
    assert!(opened.report.is_empty(), "{:?}", opened.report);
    assert_eq!(opened.store.seq(), 4);
    assert_eq!(opened.library.caption(ids[0]), "");
    assert_eq!(opened.library.caption(ids[1]), "caption 1");
    assert_eq!(opened.library.caption(ids[2]), "caption 2");
}
