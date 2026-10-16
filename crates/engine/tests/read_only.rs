//! Read-only sessions (`roadmap.md` M10.16; `mcp.md` §1 `--read-only`): starring and captioning
//! change the library in memory and no byte on disk, with every write-back option on; an export is
//! refused with `read_only`; the owner thread makes no `fs` call; a session made read-only once
//! open keeps what it wrote before and writes nothing after.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{jpeg, off_owner, open, options};
use dalica_core::SystemClock;
use dalica_engine::settings::MetadataWrites;
use dalica_engine::{CmdError, Session};
use serde_json::json;

/// One file as [`snapshot`] sees it: its bytes (`None` for the library's lock file), its size and its
/// modification time.
type Seen = (Option<Vec<u8>>, u64, Option<std::time::SystemTime>);

/// Every file under `root` with its bytes, size and modification time, by path relative to `root`
/// (read off the owner thread). Bytes are compared whole: stronger than comparing digests. The
/// library's lock file at the top (`dalica_fs::lock::LOCK_FILE`) is seen by size and time only:
/// a read-only session still holds the lock (one process per library), and on Windows a locked
/// file cannot be read by anyone, this test included.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Seen> {
    let root = root.to_path_buf();
    off_owner(move || {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let path = e.unwrap().path();
                let meta = std::fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(&root).unwrap().to_path_buf();
                    let bytes = (rel != Path::new(dalica_fs::lock::LOCK_FILE)).then(|| std::fs::read(&path).unwrap());
                    out.insert(rel, (bytes, meta.len(), meta.modified().ok()));
                }
            }
        }
        out
    })
}

/// A disk with `Lisbon/IMG_0001.jpg`…`IMG_000n.jpg` and a sidecar starring the second photo.
fn disk(n: u64) -> (tempfile::TempDir, PathBuf) {
    let disk = tempfile::tempdir().unwrap();
    let folder = disk.path().join("Lisbon");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), jpeg(i)).unwrap();
    }
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "IMG_0002.jpg": { "star": true } } });
    std::fs::write(folder.join(".dalica.json"), serde_json::to_vec_pretty(&sidecar).unwrap()).unwrap();
    (disk, folder)
}

fn ids_by_name(s: &Session) -> Vec<dalica_core::PhotoId> {
    let mut v: Vec<(String, dalica_core::PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    v.sort();
    v.into_iter().map(|(_, id)| id).collect()
}

#[test]
fn starring_and_captioning_change_no_byte_on_disk_and_an_export_is_refused() {
    let (disk, folder) = disk(3);
    let (lib, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let photos_before = snapshot(disk.path());
    let mut s = Session::open_read_only(lib.path(), Arc::new(SystemClock), &options()).unwrap();
    assert!(s.is_read_only());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = ids_by_name(&s);
    assert_eq!(ids.len(), 3);
    assert!(s.library().photo(ids[1]).unwrap().star, "the sidecar was read");
    // Every write-back option on: captions and stars would go into the photo files.
    s.change_settings(|st| {
        st.set_metadata_writes(&MetadataWrites { stars_as_ratings: true, update_modification_time: true, ..MetadataWrites::default() })
    });
    s.flush().unwrap();
    let library_before = snapshot(lib.path());

    let ui = dalica_fs::mark_ui_thread();
    s.execute("photo.star.toggle", &json!({ "ids": [ids[0]] })).unwrap();
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": false })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[2]], "caption": "Tram 28" })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    let refused = s.execute("export.run", &json!({ "ids": ids, "dir": out.path(), "folderName": "Lisbon", "size": 120 }));
    s.flush().unwrap();
    s.poll();
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    drop(ui);

    assert!(matches!(&refused, Err(e @ CmdError::ReadOnly(_)) if e.code() == "read_only"), "{refused:?}");
    assert!(s.library().photo(ids[0]).unwrap().star && !s.library().photo(ids[1]).unwrap().star, "changed in memory");
    assert_eq!(s.library().caption(ids[2]), "Tram 28");
    s.wait_idle();
    // Give a debounced write every chance to land before looking.
    s.flush().unwrap();
    drop(s);
    assert_eq!(snapshot(disk.path()), photos_before, "no byte of the photo folder changed");
    assert_eq!(snapshot(lib.path()).keys().collect::<Vec<_>>(), library_before.keys().collect::<Vec<_>>(), "no file made in the library");
    assert_eq!(snapshot(lib.path()), library_before, "no byte of the library changed");
    assert!(off_owner(|| std::fs::read_dir(out.path()).unwrap().next().is_none()), "nothing exported");
}

#[test]
fn a_session_made_read_only_keeps_what_it_wrote_and_writes_nothing_after() {
    let (_disk, folder) = disk(2);
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = ids_by_name(&s);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "kept" })).unwrap();
    s.make_read_only();
    s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "memory only" })).unwrap();
    s.flush().unwrap();
    drop(s);
    let s = open(lib.path());
    let ids = ids_by_name(&s);
    assert_eq!((s.library().caption(ids[0]), s.library().caption(ids[1])), ("kept", ""));
}
