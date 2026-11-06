//! Stats for Nerds' query and Clear Thumbnail Cache (`roadmap.md` M16.36, issue #756;
//! `control-protocol.md` §6.18): `app.diagnostics` gives every section on a small library, the
//! disk's facts measured off the owner thread and the backups counted only when asked;
//! `app.clearThumbnailCache` empties only the cache's own files, in a background job, and
//! thumbnails come back as photos are looked at. The owner thread makes no `fs` call throughout.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{jpeg_of, off_owner};
use dalica_core::SystemClock;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

/// A library watching `Lisbon` (two photos, a kept original in `.dalica-originals/`) with its
/// thumbnail cache in `cache`, scanned.
fn setup(lib: &Path, disk: &Path, cache: &Path) -> Session {
    let folder = disk.join("Lisbon");
    off_owner(|| {
        dalica_fs::create_dir_all(&folder.join(".dalica-originals")).unwrap();
        for i in 0..2u64 {
            dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg_of(i, 320, 240), Expect::Absent).unwrap();
        }
        dalica_fs::replace(&folder.join(".dalica-originals").join("IMG_0009.jpg"), &[7u8; 1000], Expect::Absent).unwrap();
    });
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(lib, Arc::new(SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

/// `app.diagnostics` once `done` holds for its answer (it measures in the background).
fn until(s: &mut Session, params: &Value, done: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        s.poll();
        let v = s.execute("app.diagnostics", params).unwrap();
        if done(&v) {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "never settled: {v:#}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn measured(v: &Value) -> bool {
    !v["measuring"].as_bool().unwrap()
}

#[test]
fn diagnostics_give_every_section_and_count_backups_only_when_asked() {
    let (lib, disk, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = setup(lib.path(), disk.path(), cache.path());
    let _ui = dalica_fs::mark_ui_thread();
    let v = until(&mut s, &json!({}), measured);
    for section in ["memory", "thumbnailCache", "libraryFolder", "photoFolders", "library", "work", "build"] {
        assert!(v[section].is_object(), "{section}: {v:#}");
    }
    assert_eq!(v["library"]["photos"], 2);
    assert_eq!(v["library"]["watched"], 1);
    assert_eq!(v["thumbnailCache"]["path"], json!(cache.path()));
    assert_eq!(v["thumbnailCache"]["temporary"], false);
    assert!(v["thumbnailCache"]["entries"].as_u64().unwrap() >= 4, "128 and 256 of each photo: {v:#}");
    assert_eq!(v["thumbnailCache"]["sizes"].as_array().unwrap().len(), 5);
    assert_eq!(v["libraryFolder"]["path"], json!(lib.path()));
    let files: Vec<&str> = v["libraryFolder"]["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(files.contains(&"folders.json") && files.contains(&"index.log"), "{files:?}");
    assert_eq!(v["build"]["version"], dalica_core::build::VERSION);
    // The backups wait to be asked for.
    assert_eq!(v["photoFolders"], json!({ "counting": false, "counted": false, "folders": null, "backups": null, "sidecars": null }));
    let v = until(&mut s, &json!({ "countBackups": true }), |v| v["photoFolders"]["counted"] == true);
    assert_eq!(v["photoFolders"]["backups"], json!({ "folders": 1, "files": 1, "bytes": 1000 }));
    assert_eq!(v["photoFolders"]["folders"], 1);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread touched the disk");
}

#[test]
fn clearing_the_cache_empties_only_its_own_files_and_thumbnails_come_back() {
    let (lib, disk, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = setup(lib.path(), disk.path(), cache.path());
    let stranger = cache.path().join("notes.txt");
    off_owner(|| dalica_fs::replace(&stranger, b"mine", Expect::Absent).unwrap());
    let ids: Vec<_> = s.library().photos().map(|p| p.id).collect();
    let _ui = dalica_fs::mark_ui_thread();
    let before = until(&mut s, &json!({}), measured)["thumbnailCache"]["entries"].as_u64().unwrap();
    assert!(before >= 4);
    let r = s.execute("app.clearThumbnailCache", &json!({})).unwrap();
    let job = r["job"].as_u64().unwrap();
    let done = s.wait_job(job, Duration::from_secs(60)).unwrap();
    assert_eq!(done.result.as_ref().unwrap()["removed"].as_u64().unwrap(), before, "{done:?}");
    let v = until(&mut s, &json!({}), |v| measured(v) && v["thumbnailCache"]["entries"] == 0);
    assert_eq!(v["thumbnailCache"]["liveBytes"], 0);
    assert_eq!(off_owner(|| std::fs::read(&stranger).unwrap()), b"mine", "a file not of the cache's shape stays");
    // Looking at a photo again makes what the cell asks for again.
    for id in &ids {
        let _ = s.thumbnail(*id, 1000);
    }
    s.wait_idle();
    until(&mut s, &json!({}), |v| measured(v) && v["thumbnailCache"]["entries"].as_u64().unwrap() >= 2);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread touched the disk");
}
