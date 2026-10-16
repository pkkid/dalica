//! Hide and Unhide on a temporary library (`roadmap.md` M6.14; `organizing.md` §9,
//! `data-model.md` §3, §8): `photo.hide` / `photo.unhide` and `folder.hide` / `folder.unhide` set
//! the flag as one library undo step (none when nothing changes); Unhide is disabled unless
//! something named is hidden; undo and redo are exact; the sidecar after the command and after
//! undo (`"hidden": true` on the photo's entry, or in the folder's own `"folder"` object); nothing
//! else on disk changes; the flags survive a restart and a rebuild of the index.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

/// A library watching `Trips/` with two folders `Lisbon` (3 photos) and `Porto` (2 photos).
fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = disk.path().join("Trips");
    for (i, name) in
        ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg", "Porto/IMG_0101.jpg", "Porto/IMG_0102.jpg"].iter().enumerate()
    {
        let path = root.join(name);
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    (lib, disk, root, s)
}

fn photo(s: &Session, name: &str) -> PhotoId {
    s.library().photos().find(|p| p.name == name).unwrap().id
}

fn folder(s: &Session, root: &Path, name: &str) -> FolderId {
    s.library().folder_by_path(&root.join(name)).unwrap()
}

fn section(f: FolderId) -> String {
    format!("folder:{f}")
}

/// The sidecar of `root/name` after every pending write landed without its `"dalica"` header (an empty object when there is no file:
/// a file left with only the header says the same as none).
fn sidecar(s: &mut Session, root: &Path, name: &str) -> Value {
    s.flush().unwrap();
    let mut v: Value = std::fs::read(root.join(name).join(".dalica.json")).map_or(json!({}), |b| serde_json::from_slice(&b).unwrap());
    if let Some(o) = v.as_object_mut() {
        o.remove("dalica");
    }
    v
}

/// The hidden photos by name, and the hidden folders by name (ids change in a rebuild).
fn hidden(s: &Session) -> (Vec<String>, Vec<String>) {
    let lib = s.library();
    let mut photos: Vec<String> = lib.photos().filter(|p| lib.is_hidden(p.id)).map(|p| p.name.clone()).collect();
    let mut folders: Vec<String> =
        lib.folders().filter(|f| lib.is_folder_hidden(f.id)).map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    photos.sort();
    folders.sort();
    (photos, folders)
}

/// Every photo file under `root` with its bytes (sidecars left out).
fn photo_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for dir in ["Lisbon", "Porto"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "jpg") {
                out.insert(p.clone(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

#[test]
fn hiding_photos_is_one_step_written_to_the_sidecar_and_undo_redo_are_exact() {
    let (_l, _d, root, mut s) = setup();
    let files = photo_files(&root);
    let (a, b, c, p) = (photo(&s, "IMG_0001.jpg"), photo(&s, "IMG_0002.jpg"), photo(&s, "IMG_0003.jpg"), photo(&s, "IMG_0101.jpg"));
    let m0 = s.library().model().clone();
    let unhide = *s.spec("photo.unhide").unwrap();
    assert_eq!((unhide.enabled)(&s, &json!({ "ids": [a, b] })).map_err(|d| d.0), Err("None of these photos is hidden.".into()));

    let v = s.execute("photo.hide", &json!({ "ids": [a, c, p] })).unwrap();
    assert_eq!(v, json!({ "ids": [a, c, p], "hidden": true, "changed": 3 }));
    assert_eq!(hidden(&s).0, ["IMG_0001.jpg", "IMG_0003.jpg", "IMG_0101.jpg"]);
    assert_eq!((s.undo_stack().len(), s.undo_stack().undo_label()), (1, Some("Hide")));
    let lisbon = sidecar(&mut s, &root, "Lisbon");
    assert_eq!(lisbon["photos"]["IMG_0001.jpg"]["hidden"], json!(true));
    assert_eq!(lisbon["photos"]["IMG_0003.jpg"]["hidden"], json!(true));
    assert!(lisbon["photos"].get("IMG_0002.jpg").is_none());
    assert_eq!(sidecar(&mut s, &root, "Porto")["photos"]["IMG_0101.jpg"]["hidden"], json!(true));
    let m1 = s.library().model().clone();

    // Hiding what is hidden already records nothing.
    assert_eq!(s.execute("photo.hide", &json!({ "ids": [a] })).unwrap()["changed"], json!(0));
    assert_eq!(s.undo_stack().len(), 1);

    // Unhide is enabled with one hidden photo in a mixed selection, and unhides only it.
    assert!((unhide.enabled)(&s, &json!({ "ids": [a, b] })).is_ok());
    let v = s.execute("photo.unhide", &json!({ "ids": [a, b] })).unwrap();
    assert_eq!((v["changed"].clone(), s.undo_stack().undo_label()), (json!(1), Some("Unhide")));
    assert_eq!(hidden(&s).0, ["IMG_0003.jpg", "IMG_0101.jpg"]);
    let m2 = s.library().model().clone();

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m0);
    for name in ["Lisbon", "Porto"] {
        let v = sidecar(&mut s, &root, name);
        assert!(v["photos"].as_object().is_none_or(|o| o.is_empty()), "after undo {name} holds no hidden flag: {v}");
    }
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m2);
    assert_eq!(photo_files(&root), files, "the photos' files never change");
}

#[test]
fn hiding_folders_writes_the_folder_object_and_unhide_follows_the_flag() {
    let (_l, _d, root, mut s) = setup();
    let (lisbon, porto) = (folder(&s, &root, "Lisbon"), folder(&s, &root, "Porto"));
    let m0 = s.library().model().clone();
    let file0 = sidecar(&mut s, &root, "Lisbon");
    let unhide = *s.spec("folder.unhide").unwrap();
    assert_eq!((unhide.enabled)(&s, &json!({ "section": section(lisbon) })).map_err(|d| d.0), Err("The folder is not hidden.".into()));

    let v = s.execute("folder.hide", &json!({ "section": section(lisbon) })).unwrap();
    assert_eq!(v, json!({ "folders": [lisbon], "hidden": true, "changed": 1 }));
    assert_eq!(hidden(&s), (vec![], vec!["Lisbon".to_string()]));
    assert_eq!(s.undo_stack().undo_label(), Some("Hide Folder"));
    assert_eq!(sidecar(&mut s, &root, "Lisbon")["folder"], json!({ "hidden": true }));
    assert_eq!(sidecar(&mut s, &root, "Porto")["folder"], Value::Null);
    let m1 = s.library().model().clone();

    // From the menu with a selection: the folders of the selected photos.
    assert!((unhide.enabled)(&s, &json!({ "ids": [photo(&s, "IMG_0001.jpg"), photo(&s, "IMG_0101.jpg")] })).is_ok());
    s.execute("folder.hide", &json!({ "ids": [photo(&s, "IMG_0002.jpg"), photo(&s, "IMG_0102.jpg")] })).unwrap();
    assert_eq!(hidden(&s).1, ["Lisbon", "Porto"]);
    assert_eq!(s.undo_stack().len(), 2);
    s.execute("folder.unhide", &json!({ "section": section(porto), "ids": [photo(&s, "IMG_0003.jpg")] })).unwrap();
    assert!(hidden(&s).1.is_empty());
    assert_eq!(sidecar(&mut s, &root, "Lisbon"), file0, "unhidden, the folder object goes again");

    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m0);
    assert_eq!(sidecar(&mut s, &root, "Lisbon"), file0);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    // The photos inside a hidden folder keep their own flags.
    assert!(hidden(&s).0.is_empty());
}

/// A list with one unknown photo, too many photos, and a section where photos are asked for
/// (`cmd::hide`'s tests: the disabled reasons, unknown photos and folders, bad sections).
#[test]
fn unknown_and_too_many_change_nothing() {
    let (_l, _d, root, mut s) = setup();
    let lisbon = folder(&s, &root, "Lisbon");
    let a = photo(&s, "IMG_0001.jpg");
    let m = s.library().model().clone();
    for cmd in ["photo.hide", "photo.unhide"] {
        for bad in [json!({ "ids": [a, 999] }), json!({ "section": section(lisbon) })] {
            assert!(s.execute(cmd, &bad).is_err(), "{cmd} {bad}");
        }
        let too_many: Vec<u64> = vec![a.get(); 100_001];
        assert!(matches!(s.execute(cmd, &json!({ "ids": too_many })), Err(CmdError::Limit(_))));
    }
    for cmd in ["folder.hide", "folder.unhide"] {
        for bad in [json!({ "section": lisbon.get() }), json!({ "ids": [999] }), json!({ "id": lisbon })] {
            assert!(s.execute(cmd, &bad).is_err(), "{cmd} {bad}");
        }
    }
    assert_eq!(s.library().model(), &m);
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn hidden_flags_survive_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, root, mut s) = setup();
    s.execute("photo.hide", &json!({ "ids": [photo(&s, "IMG_0002.jpg"), photo(&s, "IMG_0102.jpg")] })).unwrap();
    s.execute("folder.hide", &json!({ "section": section(folder(&s, &root, "Porto")) })).unwrap();
    let want = hidden(&s);
    assert_eq!(want, (vec!["IMG_0002.jpg".to_string(), "IMG_0102.jpg".to_string()], vec!["Porto".to_string()]));
    let porto = sidecar(&mut s, &root, "Porto");
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(hidden(&s), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    s.execute("photo.star.set", &json!({ "ids": [photo(&s, "IMG_0101.jpg")], "on": true })).unwrap();
    let after = sidecar(&mut s, &root, "Porto");
    assert_eq!(
        (&after["folder"], &after["photos"]["IMG_0102.jpg"]),
        (&porto["folder"], &porto["photos"]["IMG_0102.jpg"]),
        "a write after a rebuild keeps the flags"
    );
    assert_eq!(hidden(&s), want, "after a rebuild of the index");
}

/// `library.query` with no section and no search (`{}`, or only `folder`) leaves out hidden
/// photos and the photos of hidden folders, as a section and a search do, until the view shows
/// hidden pictures (MCP `query_photos {}` asks this).
#[test]
fn hidden_photos_leave_a_plain_library_query_until_the_view_shows_them() {
    let (_l, _d, root, mut s) = setup();
    let (lisbon, porto) = (folder(&s, &root, "Lisbon"), folder(&s, &root, "Porto"));
    s.execute("photo.hide", &json!({ "ids": [photo(&s, "IMG_0002.jpg")] })).unwrap();
    s.execute("folder.hide", &json!({ "section": section(porto) })).unwrap();
    let names = |s: &mut Session, p: Value| -> Vec<String> {
        let v = s.execute("library.query", &p).unwrap();
        let mut names: Vec<String> = v["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(v["total"], names.len(), "{v}");
        names
    };
    assert_eq!(names(&mut s, json!({})), ["IMG_0001.jpg", "IMG_0003.jpg"]);
    assert_eq!(names(&mut s, json!({ "folder": lisbon })), ["IMG_0001.jpg", "IMG_0003.jpg"]);
    assert!(names(&mut s, json!({ "folder": porto })).is_empty(), "a hidden folder's photos");

    s.change_settings(|st| {
        let mut v = st.view_options();
        v.hidden_pictures = true;
        st.set_view_options(&v);
    });
    let all = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0101.jpg", "IMG_0102.jpg"];
    assert_eq!(names(&mut s, json!({})), all);
    assert_eq!(names(&mut s, json!({ "folder": lisbon })), all[..3]);
    assert_eq!(names(&mut s, json!({ "folder": porto })), all[3..]);
}
