//! Importing (`roadmap.md` M11.8; `organizing.md` §3 items 4–6) from a temporary card: dated
//! folders holding the source's bytes and times, a planted clash renamed and never overwritten,
//! the room's stars and rotations in the destination's sidecar, a second import that finds only
//! duplicates, Cancel keeping what was copied as one undo step, undo and redo, and the commands'
//! graceful failures. The owner thread is marked as the UI thread while the commands run: it never
//! touches the disk.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::common::{files, off_owner, open, read, read_json, shared, wait_files};
use dalica_core::DateTime;
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_scenes::{Exif, Scene};
use serde_json::{Value, json};

/// How often the test's device list is listed again.
const REFRESH: Duration = Duration::from_millis(50);
/// Long enough for any machine; nothing that ends waits for it.
const PATIENCE: Duration = Duration::from_secs(120);

/// A 64 × 48 JPEG of scene `seed` taken `secs` after 2026-06-04T08:00:00.
pub fn shot(seed: u64, secs: i64) -> Vec<u8> {
    let at = DateTime::from_unix_secs(1_780_560_000 + secs).unwrap();
    let exif = Exif { date_time_original: Some(at), ..Exif::default() };
    dalica_scenes::jpeg(&Scene::new(seed, 64, 48), 80, Some(&exif)).unwrap()
}

/// Writes `files` (`"100CANON/IMG_0001.JPG"` → bytes) under `root`, off the owner thread.
pub fn put_all(root: &Path, files: Vec<(String, Vec<u8>)>) {
    let root = root.to_path_buf();
    off_owner(move || {
        for (rel, bytes) in files {
            let path = rel.split('/').fold(root.clone(), |p, c| p.join(c));
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
        }
    });
}

pub fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

pub fn code(s: &mut Session, id: &str, params: Value) -> &'static str {
    match s.execute(id, &params) {
        Ok(v) => panic!("{id} {params} should fail, gave {v}"),
        Err(e) => e.code(),
    }
}

/// A session on a fresh library whose device list is the folders inside `mounts`.
pub fn session_on(lib: &Path, mounts: &Path) -> Session {
    let mut s = open(lib);
    s.set_import_device_roots(Some(std::env::join_paths([mounts]).unwrap()), REFRESH);
    s
}

/// Opens the card whose `DCIM` folder is `dcim` (once the device list shows it) and waits for its
/// listing; the listing's last answer.
pub fn open_card(s: &mut Session, dcim: &Path) -> Value {
    let start = Instant::now();
    loop {
        match s.execute("import.openSource", &json!({ "device": dcim })) {
            Ok(_) => break,
            Err(e) => assert!(start.elapsed() < PATIENCE, "the card never showed: {e}"),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    loop {
        let v = run(s, "import.list", json!({ "limit": 0 }));
        if v["state"] != "loading" {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "the listing never ended: {v}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Every candidate, in capture order.
pub fn candidates(s: &mut Session) -> Vec<Value> {
    run(s, "import.list", json!({}))["candidates"].as_array().unwrap().clone()
}

/// Runs `import.run` with `params` and waits for its job; `import.status` after it.
pub fn import(s: &mut Session, params: Value) -> Value {
    let v = run(s, "import.run", params);
    assert_eq!(v["pending"], true, "{v}");
    wait_files(s);
    run(s, "import.status", json!({}))
}

fn modified(path: &Path) -> SystemTime {
    let path = path.to_path_buf();
    off_owner(move || std::fs::metadata(path).unwrap().modified().unwrap())
}

/// The names of the photos the library holds in `folder`, sorted.
fn photos_in(s: &Session, folder: &Path) -> Vec<String> {
    let Some(id) = s.library().folder_by_path(folder) else { return Vec::new() };
    let mut v: Vec<String> = s.library().photos_in(id).into_iter().map(|p| p.name.clone()).collect();
    v.sort();
    v
}

#[test]
fn a_card_of_three_dates_lands_in_dated_folders_verified_and_a_second_import_finds_only_duplicates() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("EOS_DIGITAL").join("DCIM");
    // 120 shots, 40 a day on three days, a minute apart.
    let card: Vec<(String, Vec<u8>)> =
        (0..120u64).map(|i| (format!("100CANON/IMG_{:04}.JPG", i + 1), shot(i, ((i / 40) * 86_400 + (i % 40) * 60) as i64))).collect();
    put_all(&dcim, card.clone());
    // A file already at the first copy's name, which the import must never overwrite.
    let dest = pictures.path().join("Imported");
    let planted = b"not a photo, and never overwritten".to_vec();
    put_all(&dest, vec![("2026-06-04/IMG_0001.JPG".into(), planted.clone())]);

    let ui = dalica_fs::mark_ui_thread();
    let listed = open_card(&mut s, &dcim);
    assert_eq!(listed["total"], 120, "{listed}");
    let list = candidates(&mut s);
    let id_of = |name: &str| list.iter().find(|c| c["name"] == name).unwrap()["id"].clone();
    run(&mut s, "import.mark", json!({ "ids": [id_of("IMG_0002.JPG")], "star": true }));
    run(&mut s, "import.mark", json!({ "ids": [id_of("IMG_0003.JPG")], "rotate": 1 }));
    let status = import(&mut s, json!({ "destination": dest, "folderName": { "dateTaken": true }, "after": "delete" }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");

    let days = ["2026-06-04", "2026-06-05", "2026-06-06"].map(|d| dest.join(d));
    assert_eq!((status["state"].clone(), status["imported"].clone(), status["failed"].clone()), (json!("done"), json!(120), json!([])), "{status}");
    assert_eq!(status["folders"], json!(days), "one folder per capture date, in capture order");
    assert_eq!(status["bytes"], json!(card.iter().map(|(_, b)| b.len() as u64).sum::<u64>()));
    assert_eq!(read(&days[0].join("IMG_0001.JPG")), planted, "the planted file is untouched");
    for (i, (rel, bytes)) in card.iter().enumerate() {
        let name = rel.rsplit('/').next().unwrap();
        let landed = if name == "IMG_0001.JPG" { "IMG_0001-001.JPG".to_string() } else { name.to_string() };
        let copy = days[i / 40].join(&landed);
        assert_eq!(&read(&copy), bytes, "{landed} holds the source's bytes");
        assert_eq!(modified(&copy), modified(&dcim.join("100CANON").join(name)), "{landed} keeps the source's time");
    }
    assert_eq!(files(&days[0]).len(), 41, "40 copies beside the planted file");
    assert_eq!(photos_in(&s, &days[1]).len(), 40, "the copies are library photos");
    assert!(s.roots().contains(&dest), "the destination is watched");
    assert_eq!(s.undo_stack().undo_label(), Some("Import"), "the import is one undo step");
    let event = s.file_job_events().last().unwrap();
    assert_eq!(event.message, "Imported 120 pictures into 3 folders");
    assert_eq!(event.folder.as_deref(), Some(days[0].as_path()), "the toast's folder is the first one");

    s.flush().unwrap();
    let sidecar = read_json(&days[0].join(".dalica.json")).unwrap();
    assert_eq!(sidecar["photos"]["IMG_0002.JPG"]["star"], json!(true), "the room's star: {sidecar}");
    assert_eq!(sidecar["photos"]["IMG_0003.JPG"]["edits"], json!(["rotate(90)"]), "the room's rotation");
    let settings = read_json(&lib.path().join("settings.json")).unwrap();
    assert_eq!(settings["import"]["recentDestinations"], json!([dest]));
    assert_eq!(settings["import"]["recentSources"], json!([dcim]));
    assert_eq!(settings["import"]["after"], "delete");
    // Nothing of Dalica's own copies is imported again once the watcher looks.
    let photos = s.library().photos().count();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), photos);

    let ui = dalica_fs::mark_ui_thread();
    let again = open_card(&mut s, &dcim);
    assert_eq!((again["duplicates"].clone(), again["toImport"].clone()), (json!(120), json!(0)), "{again}");
    let status = import(&mut s, json!({ "destination": dest, "folderName": { "dateTaken": true } }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
    assert_eq!((status["imported"].clone(), status["skipped"]["duplicates"].clone()), (json!(0), json!(120)), "{status}");
    assert_eq!(files(&days[0]).len(), 41, "nothing was copied");
}

/// A gate that holds the import after its file `at` was copied until the test lets it go.
fn hold_at(s: &mut Session, at: usize) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (reached_tx, reached_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (reached_tx, go_rx) = (Mutex::new(reached_tx), Mutex::new(go_rx));
    s.set_import_gate(Some(Arc::new(move |order| {
        if order == at {
            let _ = reached_tx.lock().unwrap().send(());
            let _ = go_rx.lock().unwrap().recv_timeout(PATIENCE);
        }
    })));
    (reached_rx, go_tx)
}

#[test]
fn cancel_after_ten_keeps_ten_as_one_step_that_undo_trashes_and_redo_copies_again() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("CARD").join("DCIM");
    let card: Vec<(String, Vec<u8>)> = (0..20u64).map(|i| (format!("100CANON/IMG_{:04}.JPG", i + 1), shot(i, i as i64 * 60))).collect();
    put_all(&dcim, card.clone());
    let dest = pictures.path().to_path_buf();
    let folder = dest.join("Lisbon trip");

    let ui = dalica_fs::mark_ui_thread();
    open_card(&mut s, &dcim);
    let (reached, go) = hold_at(&mut s, 9);
    let v = run(&mut s, "import.run", json!({ "destination": dest, "folderName": "  Lisbon trip ", "after": "leave" }));
    let job = v["job"].as_u64().unwrap();
    reached.recv_timeout(PATIENCE).expect("the tenth file was copied");
    assert_eq!(s.cancel_jobs(Some(job)).unwrap(), vec![job]);
    go.send(()).unwrap();
    wait_files(&mut s);
    let status = run(&mut s, "import.status", json!({}));
    assert_eq!(
        (status["state"].clone(), status["imported"].clone(), status["total"].clone()),
        (json!("cancelled"), json!(10), json!(20)),
        "{status}"
    );
    assert_eq!(s.file_job_events().last().unwrap().message, "Import cancelled: 10 of 20 pictures copied");
    let first_ten: Vec<String> = (1..=10).map(|i| format!("IMG_{i:04}.JPG")).collect();
    assert_eq!(photos_in(&s, &folder), first_ten, "the ten copied are in the library");
    assert_eq!(s.undo_stack().undo_label(), Some("Import"));
    let steps = s.undo_stack().len();

    run(&mut s, "edit.undo", json!({}));
    wait_files(&mut s);
    assert!(photos_in(&s, &folder).is_empty(), "undo takes the photos out");
    assert_eq!(s.undo_stack().len(), steps - 1, "the one step left the undo stack");
    run(&mut s, "edit.redo", json!({}));
    wait_files(&mut s);
    assert_eq!(photos_in(&s, &folder), first_ten, "redo brings them back");
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
    assert_eq!(files(&folder), first_ten, "redo copied the files again");
    for (rel, bytes) in card.iter().take(10) {
        assert_eq!(&read(&folder.join(rel.rsplit('/').next().unwrap())), bytes);
    }
    s.flush().unwrap();
    let settings = read_json(&lib.path().join("settings.json")).unwrap();
    assert_eq!(settings["import"]["recentFolderNames"], json!(["Lisbon trip"]), "the name typed, trimmed");
}

#[test]
fn undo_trashes_the_copies_and_leaves_a_changed_one() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("CARD").join("DCIM");
    put_all(&dcim, (0..3u64).map(|i| (format!("100CANON/IMG_{i}.JPG"), shot(i, 0))).collect());
    let folder = pictures.path().join("2026-06-04");
    let ui = dalica_fs::mark_ui_thread();
    open_card(&mut s, &dcim);
    let status = import(&mut s, json!({ "destination": pictures.path(), "folderName": { "dateTaken": true } }));
    assert_eq!(status["imported"], 3);
    drop(ui);
    // The user edits one copy outside Dalica.
    let changed = folder.join("IMG_1.JPG");
    let edited = shot(99, 0);
    let target = changed.clone();
    off_owner(move || std::fs::write(target, edited).unwrap());

    run(&mut s, "edit.undo", json!({}));
    wait_files(&mut s);
    assert_eq!(files(&folder), ["IMG_1.JPG"], "the unchanged copies went to the trash");
    let event = s.file_job_events().last().unwrap();
    assert_eq!(event.failures.len(), 1, "{event:?}");
    assert!(event.failures[0].reason.contains("changed since it was imported"), "{event:?}");
    assert_eq!(s.undo_stack().undo_label(), Some("Import"), "the changed copy stays on the undo stack");
    assert_eq!(s.undo_stack().redo_label(), Some("Import"), "the two undone can be redone");
}

#[test]
fn import_commands_fail_gracefully() {
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dest = pictures.path();
    assert_eq!(code(&mut s, "import.run", json!({ "destination": dest, "folderName": "x" })), "disabled", "nothing open");
    assert_eq!(run(&mut s, "import.status", json!({})), json!({ "state": null }));
    assert_eq!(code(&mut s, "import.removeFromSource", json!({ "confirm": true })), "disabled", "nothing imported");
    let dcim = mounts.path().join("CARD").join("DCIM");
    put_all(&dcim, vec![("100CANON/IMG_1.JPG".into(), shot(1, 0))]);
    open_card(&mut s, &dcim);
    for (params, want) in [
        (json!({ "destination": dest }), "bad_params"),
        (json!({ "destination": dest, "folderName": "   " }), "bad_params"),
        (json!({ "destination": dest, "folderName": { "dateTaken": false } }), "bad_params"),
        (json!({ "destination": dest, "folderName": { "tomorrow": true } }), "bad_params"),
        (json!({ "destination": "relative/path", "folderName": "x" }), "bad_params"),
        (json!({ "destination": dcim.join("inside"), "folderName": "x" }), "bad_params"),
        (json!({ "destination": dest, "folderName": "x", "after": "burn" }), "bad_params"),
        (json!({ "destination": dest, "folderName": "x", "ids": [] }), "bad_params"),
        (json!({ "destination": dest, "folderName": "x", "ids": [999] }), "not_found"),
        (json!({ "destination": dest, "folderName": "x", "extra": 1 }), "bad_params"),
    ] {
        assert_eq!(code(&mut s, "import.run", params.clone()), want, "{params}");
    }
    assert_eq!(code(&mut s, "import.status", json!({ "x": 1 })), "bad_params");
    assert!(s.library().photos().next().is_none(), "nothing was imported");
}

#[test]
fn today_names_the_folder_with_the_session_clocks_date() {
    let _faults = shared();
    let (lib, mounts, pictures) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = session_on(lib.path(), mounts.path());
    let dcim = mounts.path().join("CARD").join("DCIM");
    put_all(&dcim, (0..3u64).map(|i| (format!("100CANON/IMG_{:04}.JPG", i + 1), shot(i, i as i64 * 60))).collect());
    open_card(&mut s, &dcim);
    let dest = pictures.path().join("Imported");
    let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let status = import(&mut s, json!({ "destination": dest, "folderName": { "today": true }, "after": "leave" }));
    let after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let days: Vec<String> = [before, after].iter().map(|t| DateTime::from_unix_secs(*t).unwrap().date().to_string()).collect();
    let folder =
        days.iter().map(|d| dest.join(d)).find(|f| !photos_in(&s, f).is_empty()).unwrap_or_else(|| panic!("no folder named {days:?}: {status}"));
    assert_eq!(photos_in(&s, &folder), ["IMG_0001.JPG", "IMG_0002.JPG", "IMG_0003.JPG"]);
}
