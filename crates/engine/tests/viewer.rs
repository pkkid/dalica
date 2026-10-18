//! The photo viewer's session (`viewing.md` §3, `roadmap.md` M11.12): `viewer.list` reads the
//! viewed file's folder on a worker and lists its photos in name order with the file's place; a
//! file Dalica cannot show is refused with its reason; a photo of a watched folder is the
//! library's own; the first change to a photo of an unwatched folder adds the folder as Scan Once
//! in the same undo step; a library another Dalica holds gives a read-only viewer.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::{jpeg, lisbon, open, options};
use dalica_core::SystemClock;
use dalica_engine::{CmdError, OpenError, Session};
use dalica_fs::Expect;
use dalica_scan::Mode;
use serde_json::{Value, json};

/// Writes `bytes` at `path` (making its folder).
fn put(path: &Path, bytes: &[u8]) {
    dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
    dalica_fs::replace(path, bytes, Expect::Absent).unwrap();
}

/// A folder `Downloads` on a fresh disk with `n` photos named `IMG_0001.jpg` … written in reverse
/// order, a text file, a photo in a subfolder and a sidecar starring `IMG_0002.jpg`.
fn downloads(n: u64) -> (tempfile::TempDir, PathBuf) {
    let disk = tempfile::tempdir().unwrap();
    let folder = disk.path().join("Downloads");
    for i in (1..=n).rev() {
        put(&folder.join(format!("IMG_{i:04}.jpg")), &jpeg(i));
    }
    put(&folder.join("notes.txt"), b"not a picture");
    put(&folder.join("Inner").join("IMG_9999.jpg"), &jpeg(99));
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "IMG_0002.jpg": { "star": true } } });
    put(&folder.join(".dalica.json"), &serde_json::to_vec_pretty(&sidecar).unwrap());
    (disk, folder)
}

/// Pins the modification times of `downloads(n)`'s photos to whole seconds in the order they were
/// written, `IMG_{n}` oldest, each the next a second newer: the folder's display order (sort date,
/// then name) then puts a later-named photo first, as a second boundary while writing them would.
fn pin_times(folder: &Path, n: u64) {
    let start = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for i in 1..=n {
        let at = start + Duration::from_secs(n - i);
        std::fs::File::options().write(true).open(folder.join(format!("IMG_{i:04}.jpg"))).unwrap().set_modified(at).unwrap();
    }
}

/// `viewer.list` with `params`, asked again until it is no longer pending.
fn listed(s: &mut Session, params: &Value) -> Result<Value, CmdError> {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let r = s.execute("viewer.list", params)?;
        if r.get("pending") != Some(&json!(true)) {
            return Ok(r);
        }
        assert!(Instant::now() < end, "viewer.list stayed pending");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn names(listing: &Value) -> Vec<String> {
    listing["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_folder_lists_in_name_order_with_the_files_place_and_changes_nothing() {
    let (_disk, folder) = downloads(12);
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    s.open_viewer(&folder.join("IMG_0007.jpg")).unwrap();
    assert_eq!(s.viewer().file(), Some(folder.join("IMG_0007.jpg").as_path()));
    let l = listed(&mut s, &json!({})).unwrap();
    let want: Vec<String> = (1..=12).map(|i| format!("IMG_{i:04}.jpg")).collect();
    assert_eq!(names(&l), want, "the photos alone, in name order, without the subfolder's");
    assert_eq!(l["index"], 6);
    assert_eq!(l["inLibrary"], false);
    assert_eq!(l["readOnly"], false);
    let photos = l["photos"].as_array().unwrap();
    assert!(photos.iter().all(|p| p["id"].is_null()), "shown from the files");
    assert_eq!(photos[1]["star"], true, "the folder's sidecar is read");
    assert_eq!((photos[0]["width"].clone(), photos[0]["height"].clone()), (json!(96), json!(64)));
    // Viewing adds nothing to the library and sets no rule.
    assert_eq!(s.library().photos().count(), 0);
    assert!(s.folder_rules().get(&folder).is_none());
    assert!(s.undo_stack().is_empty());
    // Another folder neither the viewer's nor watched is not read.
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(s.execute("viewer.list", &json!({ "path": other.path().join("a.jpg") })), Err(CmdError::Disabled(_))));
}

#[test]
fn a_file_dalica_cannot_show_is_refused_with_its_reason() {
    let (_disk, folder) = downloads(2);
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    s.open_viewer(&folder.join("notes.txt")).unwrap();
    match listed(&mut s, &json!({})) {
        Err(CmdError::BadParams(why)) => assert!(why.contains("notes.txt") && why.contains("not a picture"), "{why}"),
        other => panic!("{other:?}"),
    }
    match listed(&mut s, &json!({ "path": folder.join("gone.jpg") })) {
        Err(CmdError::BadParams(why)) => assert!(why.contains("gone.jpg") && why.contains("no such file"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(s.open_viewer(Path::new("relative.jpg")), Err(CmdError::BadParams(_))));
    // A photo of the folder still lists.
    assert_eq!(names(&listed(&mut s, &json!({ "path": folder.join("IMG_0001.jpg") })).unwrap()).len(), 2);
}

#[test]
fn a_photo_of_a_watched_folder_is_the_librarys_own() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2), jpeg(3)], None);
    s.open_viewer(&folder.join("IMG_0002.jpg")).unwrap();
    let l = listed(&mut s, &json!({})).unwrap();
    assert_eq!(l["inLibrary"], true);
    assert_eq!(l["index"], 1);
    let got: Vec<Value> = l["photos"].as_array().unwrap().iter().map(|p| p["id"].clone()).collect();
    assert_eq!(got, ids.iter().map(|id| json!(id)).collect::<Vec<_>>());
    let rules_before = s.folder_rules().clone();
    s.execute("viewer.star", &json!({ "path": folder.join("IMG_0002.jpg"), "on": true })).unwrap();
    assert!(s.library().photo(ids[1]).unwrap().star);
    assert_eq!(s.folder_rules(), &rules_before, "a watched folder keeps its rule");
    assert_eq!(listed(&mut s, &json!({})).unwrap()["photos"][1]["star"], true, "the library's star");
}

#[test]
fn starring_a_photo_of_an_unwatched_folder_adds_it_as_scan_once_and_one_undo_removes_both() {
    let (_disk, folder) = downloads(4);
    pin_times(&folder, 4);
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    let photo = folder.join("IMG_0003.jpg");
    s.open_viewer(&photo).unwrap();
    listed(&mut s, &json!({})).unwrap();
    let r = s.execute("viewer.star", &json!({ "path": photo })).unwrap();
    assert_eq!(r["star"], true, "toggled on");
    let id: dalica_core::PhotoId = serde_json::from_value(r["id"].clone()).unwrap();
    assert!(s.library().photo(id).unwrap().star);
    assert_eq!(s.folder_rules().get(&folder).and_then(|r| r.mode), Some(Mode::Once));
    let fid = s.library().folder_by_path(&folder).unwrap();
    assert_eq!(s.library().photos_in(fid).len(), 4, "the folder's photos as listed");
    // Which photos are starred, as a set: `photos_in` is the folder's display order (sort date, then
    // name), which the pinned file times make `IMG_0003` before `IMG_0002`; that order is the
    // `library` crate's to test.
    let mut starred: Vec<String> = s.library().photos_in(fid).iter().filter(|p| p.star).map(|p| p.name.clone()).collect();
    starred.sort();
    assert_eq!(starred, ["IMG_0002.jpg", "IMG_0003.jpg"], "the sidecar's star kept, the new one added");
    assert_eq!(s.undo_stack().len(), 1, "one step");
    let l = listed(&mut s, &json!({})).unwrap();
    assert_eq!(l["inLibrary"], true);
    assert_eq!(l["photos"][2]["id"], json!(id));
    // A second change is an ordinary star: the folder is the library's now.
    s.execute("viewer.rotate", &json!({ "path": photo, "direction": "cw" })).unwrap();
    assert_eq!(s.undo_stack().len(), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    // One undo takes back the star and the folder.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.library().folder_by_path(&folder).is_none(), "the folder left the library");
    assert_eq!(s.library().photos().count(), 0);
    assert!(s.folder_rules().get(&folder).is_none_or(|r| r.mode.is_none()), "its rule went with it");
    // Redo brings both back.
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.folder_rules().get(&folder).and_then(|r| r.mode), Some(Mode::Once));
    let fid = s.library().folder_by_path(&folder).unwrap();
    assert!(s.library().photos_in(fid).iter().any(|p| p.name == "IMG_0003.jpg" && p.star));
    // A path the viewer does not list is refused, and changes nothing.
    let before = s.undo_stack().len();
    assert!(matches!(s.execute("viewer.star", &json!({ "path": folder.join("notes.txt") })), Err(CmdError::NotFound(_))));
    assert!(matches!(s.execute("viewer.rotate", &json!({ "path": photo, "direction": "up" })), Err(CmdError::BadParams(_))));
    assert_eq!(s.undo_stack().len(), before);
}

#[test]
fn a_locked_library_gives_a_read_only_viewer_whose_changes_are_disabled() {
    let (_disk, folder) = downloads(3);
    let lib = tempfile::tempdir().unwrap();
    let _holder = open(lib.path());
    assert!(matches!(Session::open_with(lib.path(), Arc::new(SystemClock), &options()), Err(OpenError::Locked(_))));
    let scratch = tempfile::tempdir().unwrap();
    let photo = folder.join("IMG_0002.jpg");
    let mut s = Session::open_locked_viewer(scratch.path(), &photo, Arc::new(SystemClock), &options()).unwrap();
    assert!(s.viewer().read_only());
    let l = listed(&mut s, &json!({})).unwrap();
    assert_eq!(l["readOnly"], true);
    assert_eq!(l["reason"], "Dalica is already open");
    assert_eq!(l["photos"][1]["star"], true, "the edits are shown from the sidecar");
    for (id, params) in [
        ("viewer.star", json!({ "path": photo })),
        ("viewer.rotate", json!({ "path": photo, "direction": "cw" })),
        ("viewer.delete", json!({ "path": photo, "confirm": true })),
        ("viewer.adopt", json!({ "path": photo })),
    ] {
        match s.execute(id, &params) {
            Err(CmdError::Disabled(why)) => assert_eq!(why, "Dalica is already open", "{id}"),
            Err(CmdError::ReadOnly(_)) if id == "viewer.delete" => {}
            other => panic!("{id}: {other:?}"),
        }
    }
    assert!(matches!(s.open_in_dalica(&photo), Err(CmdError::Disabled(_))));
    assert_eq!(s.library().photos().count(), 0);
    // Closing is the window's to carry out.
    assert_eq!(s.viewer().closed(), None);
    s.close_viewer();
    assert_eq!(s.viewer().closed(), Some(dalica_engine::viewer::Closed::Quit));
}

/// A photo outside the library is decoded by its path on the viewer's workers, at the screen size
/// and as a filmstrip thumbnail, with the edits of its folder's sidecar; the owner thread never
/// touches the disk; a file that cannot be decoded gives its reason; an unlisted path is refused.
#[test]
fn a_photo_outside_the_library_is_decoded_by_path_off_the_owner_thread() {
    use dalica_engine::viewer::{Pixels, THUMB_SIDE};
    let disk = tempfile::tempdir().unwrap();
    let folder = disk.path().join("Elsewhere");
    put(&folder.join("a.jpg"), &crate::common::jpeg_of(5, 1200, 800));
    put(&folder.join("b.jpg"), &crate::common::jpeg_of(6, 1200, 800));
    put(&folder.join("c.jpg"), &crate::common::jpeg_of(7, 1200, 800));
    // The sidecar turns b.jpg a quarter turn.
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "b.jpg": { "edits": ["rotate(90)"] } } });
    put(&folder.join(".dalica.json"), &serde_json::to_vec_pretty(&sidecar).unwrap());
    let lib = tempfile::tempdir().unwrap();
    let mut s = open(lib.path());
    s.open_viewer(&folder.join("a.jpg")).unwrap();
    let l = listed(&mut s, &json!({})).unwrap();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let (a, b, c) = (folder.join("a.jpg"), folder.join("b.jpg"), folder.join("c.jpg"));
    assert_eq!(s.viewer_image(&a), Pixels::Pending);
    s.prefetch_viewer_images(std::slice::from_ref(&b));
    assert_eq!(s.viewer_thumbnail(&a), Pixels::Pending);
    assert_eq!(s.viewer_images_pending(), 3);
    assert!(s.wait_viewer_images(Duration::from_secs(60)), "the decodes arrive");
    let got = s.viewer_image(&a);
    let Pixels::Ready(shown) = got else { panic!("a.jpg not decoded: {got:?}") };
    assert_eq!((shown.width, shown.height), (1200, 800), "a photo under the screen size is decoded whole");
    let Pixels::Ready(turned) = s.viewer_image(&b) else { panic!("b.jpg not prefetched") };
    assert_eq!((turned.width, turned.height), (800, 1200), "the sidecar's rotation is drawn");
    let Pixels::Ready(thumb) = s.viewer_thumbnail(&a) else { panic!("no thumbnail") };
    assert_eq!(thumb.width.max(thumb.height), THUMB_SIDE);
    // A file gone since the folder was listed gives the reason it cannot be shown.
    assert_eq!(names(&l), ["a.jpg", "b.jpg", "c.jpg"]);
    std::fs::remove_file(&c).unwrap();
    assert_eq!(s.viewer_image(&c), Pixels::Pending);
    assert!(s.wait_viewer_images(Duration::from_secs(60)));
    assert_eq!(s.viewer_image(&c), Pixels::Failed("the file is missing".into()));
    assert!(matches!(s.viewer_image(&disk.path().join("other.jpg")), Pixels::Failed(_)), "only listed photos are read");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!(s.library().photos().count(), 0, "viewing adds nothing");
}
