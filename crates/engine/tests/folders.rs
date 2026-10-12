//! The Folder Manager's commands on temporary libraries (`roadmap.md` M5.12; `data-model.md` §4,
//! §7; `organizing.md` §2): modes inherited by subfolders and overridden; Remove drops a subtree
//! from the library without touching a file, and undo brings it back with its mode; a rules-only
//! change is one undo step; Scan Once is frozen after its first scan; a v1 `folders.json` is
//! written back as v2; the confirmations over automation; browsing on a worker; File Types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dalica_core::SystemClock;
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_scan::{FileType, FileTypes, Mode, Places, Volume, VolumeKind};
use serde_json::{Value, json};

fn put_jpeg(path: &Path, seed: u64) {
    dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap();
    dalica_fs::replace(path, &bytes, Expect::Absent).unwrap();
}

/// The tree: `p/{a/1, a/x/2, b/3, b/4, b/y/5, c/6}`.
fn tree(root: &Path) -> PathBuf {
    let p = root.join("p");
    for (i, f) in ["a/1.jpg", "a/x/2.jpg", "b/3.jpg", "b/4.jpg", "b/y/5.jpg", "c/6.jpg"].iter().enumerate() {
        put_jpeg(&p.join(f), i as u64);
    }
    p
}

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, Arc::new(SystemClock), &options).unwrap();
    s.wait_idle();
    s
}

/// Every photo in the library, relative to `p`, with `/`.
fn photos(s: &Session, p: &Path) -> Vec<String> {
    let lib = s.library();
    let mut out: Vec<String> = lib
        .photos()
        .map(|ph| lib.folder(ph.folder).unwrap().path.join(&ph.name))
        .map(|f| f.strip_prefix(p).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .collect();
    out.sort();
    out
}

/// Every file under `dir` with its bytes and modification time.
fn files(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                out.insert(e.path(), (std::fs::read(e.path()).unwrap(), m.modified().unwrap()));
            }
        }
    }
    out
}

fn set(s: &mut Session, changes: Value) -> Value {
    let r = s.execute("folders.set", &json!({ "changes": changes })).unwrap();
    s.wait_idle();
    r
}

fn mode(s: &Session, path: &Path) -> (Mode, bool) {
    let e = s.folder_rules().effective(path);
    (e.mode, e.explicit(path))
}

/// Asks `folders.browse` until a worker has answered (polling like the UI does each frame).
fn browse(s: &mut Session, params: Value) -> Result<Value, CmdError> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let r = s.execute("folders.browse", &params);
        if !matches!(&r, Ok(v) if v["pending"] == true) || Instant::now() > deadline {
            return r;
        }
        std::thread::sleep(Duration::from_millis(5));
        s.poll();
    }
}

#[test]
fn subfolders_inherit_their_mode_unless_they_set_their_own() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let mut s = open(lib.path());
    set(&mut s, json!([{ "path": p, "mode": "always" }]));
    assert_eq!(photos(&s, &p).len(), 6);
    assert_eq!(s.undo_stack().len(), 1);
    // Remove `b` but keep `b/y` (Scan Once), and remove `a/x`: one change, one step.
    let r = set(
        &mut s,
        json!([{ "path": p.join("b"), "mode": "remove" }, { "path": p.join("b/y"), "mode": "once" }, { "path": p.join("a/x"), "mode": "remove" }]),
    );
    assert_eq!((r["changed"].as_u64(), r["removedPhotos"].as_u64()), (Some(3), Some(3)));
    assert_eq!(photos(&s, &p), ["a/1.jpg", "b/y/5.jpg", "c/6.jpg"]);
    assert_eq!(s.undo_stack().len(), 2);
    assert_eq!(mode(&s, &p.join("a")), (Mode::Always, false), "inherited");
    assert_eq!(mode(&s, &p.join("b")), (Mode::Remove, true));
    assert_eq!(mode(&s, &p.join("b/y")), (Mode::Once, true));
    assert_eq!(mode(&s, &p.join("a/x")), (Mode::Remove, true));
    // The watched list holds only the folders set to Scan Always.
    let list = s.execute("folders.list", &json!({})).unwrap();
    assert_eq!(list["watched"], json!([{ "path": p, "name": p.display().to_string() }]));
    assert_eq!(list["settings"].as_array().unwrap().len(), 4);
    // Clearing `a/x` gives it back to `p`: it is scanned again.
    set(&mut s, json!([{ "path": p.join("a/x"), "mode": null }]));
    assert_eq!(mode(&s, &p.join("a/x")), (Mode::Always, false));
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "b/y/5.jpg", "c/6.jpg"]);
    // A rebuild indexes the same: both units, nothing of the removed folder itself.
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "b/y/5.jpg", "c/6.jpg"]);
}

#[test]
fn remove_drops_a_subtree_without_touching_a_file_and_undo_brings_it_back() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": p })).unwrap();
    s.wait_idle();
    let b = s.library().folder_by_path(&p.join("b")).unwrap();
    let starred = s.library().photos_in(b)[0].id;
    s.execute("photo.star.set", &json!({ "ids": [starred], "on": true })).unwrap();
    s.flush().unwrap();
    let ids: BTreeMap<String, u64> = s.library().photos().map(|ph| (ph.name.clone(), ph.id.get())).collect();
    let disk_before = files(&p);
    let steps = s.undo_stack().len();

    // Over automation it asks first, with the dialog's text, and changes nothing.
    let asked = s.execute("folder.remove", &json!({ "id": b }));
    assert!(matches!(&asked, Err(CmdError::Disabled(m)) if m.contains("Remove Folder") && m.contains("\"b\"") && m.contains("confirm")), "{asked:?}");
    assert_eq!(s.undo_stack().len(), steps);
    let r = s.execute("folder.remove", &json!({ "id": b, "confirm": true })).unwrap();
    assert_eq!((r["removedFolders"].as_u64(), r["removedPhotos"].as_u64()), (Some(2), Some(3)));
    s.flush().unwrap();
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "c/6.jpg"]);
    assert_eq!(mode(&s, &p.join("b/y")), (Mode::Remove, false));
    assert_eq!(s.undo_stack().len(), steps + 1);
    assert_eq!(files(&p), disk_before, "Remove never touches a file");

    s.execute("edit.undo", &json!({})).unwrap();
    s.flush().unwrap();
    assert_eq!(photos(&s, &p).len(), 6);
    assert_eq!(s.folder_rules().get(&p.join("b")), None, "the mode is as it was before");
    assert_eq!(mode(&s, &p.join("b")), (Mode::Always, false));
    let back: BTreeMap<String, u64> = s.library().photos().map(|ph| (ph.name.clone(), ph.id.get())).collect();
    assert_eq!(back, ids, "the same photos, ids and all");
    assert!(s.library().photo(starred).unwrap().star);
    assert_eq!(files(&p), disk_before, "undo touches no file either");

    s.execute("edit.redo", &json!({})).unwrap();
    s.flush().unwrap();
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "c/6.jpg"]);
    assert_eq!(mode(&s, &p.join("b")), (Mode::Remove, true));
    assert_eq!(files(&p), disk_before);

    // The rules survive a restart.
    drop(s);
    let s = open(lib.path());
    assert_eq!(mode(&s, &p.join("b")), (Mode::Remove, true));
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "c/6.jpg"]);
}

#[test]
fn a_mode_only_change_is_one_undo_step_and_redo_reapplies_it() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": p })).unwrap();
    s.wait_idle();
    assert_eq!(s.undo_stack().len(), 0, "folders.add records no step");
    let model = s.library().model().clone();
    // Scan Always → Scan Once on `c`, and faces off for `a`: nothing in the library changes.
    let r = set(&mut s, json!([{ "path": p.join("c"), "mode": "once" }, { "path": p.join("a"), "faces": false }]));
    assert_eq!(r["changed"], 2);
    assert_eq!(s.library().model(), &model);
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Change Folder Settings"));
    assert!(!s.folder_rules().faces(&p.join("a/x")));
    // The same change again changes nothing and records nothing.
    let again = set(&mut s, json!([{ "path": p.join("c"), "mode": "once" }]));
    assert_eq!(again["changed"], 0);
    assert_eq!(s.undo_stack().len(), 1);

    s.execute("edit.undo", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(s.folder_rules().get(&p.join("c")), None);
    assert!(s.folder_rules().faces(&p.join("a/x")));
    assert_eq!(s.library().model(), &model);
    s.execute("edit.redo", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(mode(&s, &p.join("c")), (Mode::Once, true));
    assert!(!s.folder_rules().faces(&p.join("a")));
    assert_eq!(s.library().model(), &model);

    // Undoing a new watched folder takes it out of the library again; redo scans it.
    let q = disk.path().join("q");
    put_jpeg(&q.join("7.jpg"), 7);
    set(&mut s, json!([{ "path": q, "mode": "always" }]));
    assert_eq!(s.library().photos().count(), 7);
    s.execute("edit.undo", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 6);
    s.execute("edit.redo", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 7);
}

#[test]
fn scan_once_is_not_rescanned_by_verification() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let mut s = open(lib.path());
    set(&mut s, json!([{ "path": p, "mode": "always" }, { "path": p.join("b"), "mode": "once" }]));
    assert_eq!(photos(&s, &p).len(), 6);
    assert_eq!(s.folder_rules().get(&p.join("b")).map(|r| r.indexed), Some(true), "indexed by its first scan");
    // Files change on disk in both units.
    put_jpeg(&p.join("a/new.jpg"), 10);
    put_jpeg(&p.join("b/new.jpg"), 11);
    std::fs::remove_file(p.join("b/3.jpg")).unwrap();
    let jobs = s.start_verification();
    assert_eq!(jobs.len(), 1, "only the Scan Always unit");
    s.wait_idle();
    let found = photos(&s, &p);
    assert!(found.contains(&"a/new.jpg".to_string()));
    assert!(!found.contains(&"b/new.jpg".to_string()) && found.contains(&"b/3.jpg".to_string()), "frozen: {found:?}");
    // Still frozen after a restart.
    s.flush().unwrap();
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(s.start_verification().len(), 1);
    s.wait_idle();
    assert!(!photos(&s, &p).contains(&"b/new.jpg".to_string()));
    // Switching it to Scan Always scans it: watched folders are kept up to date.
    set(&mut s, json!([{ "path": p.join("b"), "mode": "always" }]));
    let found = photos(&s, &p);
    assert!(found.contains(&"b/new.jpg".to_string()) && !found.contains(&"b/3.jpg".to_string()), "{found:?}");
}

#[test]
fn a_v1_folders_json_opens_and_is_written_back_as_v2() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let file = lib.path().join(dalica_engine::session::FOLDERS_FILE);
    let v1 = json!({ "version": 1, "folders": [{ "path": p, "note": "kept" }], "future": true });
    dalica_fs::replace(&file, &serde_json::to_vec(&v1).unwrap(), Expect::Absent).unwrap();
    let mut s = open(lib.path());
    assert_eq!(s.roots(), std::slice::from_ref(&p));
    assert_eq!(mode(&s, &p), (Mode::Always, true));
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(written["version"], 2);
    assert_eq!(written["future"], true);
    assert_eq!(written["folders"], json!([{ "path": p, "mode": "always", "note": "kept" }]));
    assert!(!s.is_new_library());
    // It still works as before: a rebuild scans the watched folder.
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(photos(&s, &p).len(), 6);
    // Changes are written as v2 through the I/O worker.
    set(&mut s, json!([{ "path": p.join("c"), "mode": "remove", "faces": false }]));
    s.flush().unwrap();
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert!(written["folders"].as_array().unwrap().contains(&json!({ "path": p.join("c"), "mode": "remove", "faces": false })));
    let leftovers: Vec<_> =
        std::fs::read_dir(lib.path()).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n.to_string_lossy().contains("tmp")).collect();
    assert!(leftovers.is_empty(), "atomic writes leave no temp file: {leftovers:?}");
}

#[test]
fn watching_a_drive_or_the_home_folder_asks_first_over_automation() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let home = disk.path().join("home");
    let volume = disk.path().join("volume");
    put_jpeg(&home.join("Pictures/1.jpg"), 1);
    put_jpeg(&volume.join("2.jpg"), 2);
    let mut s = open(lib.path());
    s.set_places(Places::under(&home), vec![Volume { name: "volume".into(), path: volume.clone(), kind: VolumeKind::Mounted }]);
    for path in [&home, &volume] {
        for (id, params) in [
            ("folders.set", json!({ "changes": [{ "path": path, "mode": "always" }] })),
            ("folders.add", json!({ "path": path })),
            ("folders.set", json!({ "changes": [{ "path": path, "mode": "always" }], "confirm": false })),
        ] {
            let r = s.execute(id, &params);
            assert!(
                matches!(&r, Err(CmdError::Disabled(m)) if m.contains("Watch the whole drive anyway?") && m.contains("\"confirm\": true")),
                "{id} {r:?}"
            );
        }
    }
    assert!(s.folder_rules().is_empty() && s.undo_stack().is_empty());
    // Scan Once or Remove asks nothing; Pictures is no drive.
    set(&mut s, json!([{ "path": volume, "mode": "once" }]));
    set(&mut s, json!([{ "path": home.join("Pictures"), "mode": "always" }]));
    assert_eq!(s.library().photos().count(), 2);
    let list = s.execute("folders.list", &json!({})).unwrap();
    assert_eq!(list["watched"], json!([{ "path": home.join("Pictures"), "name": "Pictures" }]));
    // With confirm it goes ahead.
    let r = s.execute("folders.set", &json!({ "changes": [{ "path": home, "mode": "always" }], "confirm": true })).unwrap();
    assert_eq!(r["changed"], 1);
    s.wait_idle();
    assert_eq!(mode(&s, &home), (Mode::Always, true));
}

#[test]
fn browsing_answers_from_a_worker_and_the_owner_thread_touches_no_file() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = tree(disk.path());
    let home = disk.path().join("home");
    dalica_fs::create_dir_all(&home.join("Pictures")).unwrap();
    let mut s = open(lib.path());
    s.set_places(Places::under(&home), vec![Volume { name: "disk".into(), path: disk.path().to_path_buf(), kind: VolumeKind::Mounted }]);
    let ui = dalica_fs::mark_ui_thread();
    let first = s.execute("folders.browse", &json!({})).unwrap();
    assert_eq!(first["pending"], true);
    let places = browse(&mut s, json!({})).unwrap();
    let names: Vec<&str> = places["places"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Pictures", "Home"], "the places that exist");
    assert_eq!(places["volumes"][0]["kind"], "mounted");
    assert_eq!(places["volumes"][0]["mode"], "remove");

    s.execute("folders.set", &json!({ "changes": [{ "path": p, "mode": "always" }, { "path": p.join("b"), "mode": "remove" }] })).unwrap();
    s.wait_idle();
    let children = browse(&mut s, json!({ "path": p })).unwrap();
    let rows: Vec<(String, String, bool)> = children["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["mode"].as_str().unwrap().to_string(), r["explicit"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        rows,
        [
            ("a".to_string(), "always".to_string(), false),
            ("b".to_string(), "remove".to_string(), true),
            ("c".to_string(), "always".to_string(), false)
        ]
    );
    assert_eq!(children["truncated"], false);
    // A folder that is not there is reported once, then read again when asked again.
    let missing = browse(&mut s, json!({ "path": p.join("gone") }));
    assert!(matches!(missing, Err(CmdError::NotFound(_))), "{missing:?}");
    assert_eq!(s.execute("folders.browse", &json!({ "path": p.join("gone") })).unwrap()["pending"], true);
    // The rest of the Folder Manager's commands on the same thread.
    s.execute("folders.list", &json!({})).unwrap();
    let b = s.library().folder_by_path(&p.join("a")).unwrap();
    s.execute("folder.remove", &json!({ "id": b, "confirm": true })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.wait_idle();
    s.flush().unwrap();
    drop(ui);
    assert_eq!(s.browse_pending(), 0);
    assert_eq!(photos(&s, &p), ["a/1.jpg", "a/x/2.jpg", "c/6.jpg"]);
}

#[test]
fn the_file_types_toggles_join_the_scans() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let p = disk.path().join("p");
    put_jpeg(&p.join("1.jpg"), 1);
    dalica_fs::replace(&p.join("2.png"), &dalica_scenes::png(&dalica_scenes::Scene::new(2, 96, 64)).unwrap(), Expect::Absent).unwrap();
    let mut s = open(lib.path());
    s.change_settings(|st| st.set_file_types(FileTypes::all().with(FileType::Png, false)));
    s.execute("folders.add", &json!({ "path": p })).unwrap();
    s.wait_idle();
    assert_eq!(photos(&s, &p), ["1.jpg"]);
    s.change_settings(|st| st.set_file_types(FileTypes::all()));
    s.start_verification();
    s.wait_idle();
    assert_eq!(photos(&s, &p), ["1.jpg", "2.png"]);
    // The toggles are kept in settings.json.
    s.change_settings(|st| st.set_file_types(FileTypes::jpeg_only()));
    s.flush().unwrap();
    drop(s);
    let s = open(lib.path());
    assert_eq!(s.settings().file_types(), FileTypes::jpeg_only());
}
