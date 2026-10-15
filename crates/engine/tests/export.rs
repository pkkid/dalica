//! Export (`roadmap.md` M8.16; `creating.md` §2, `data-model.md` §6) on temporary folders:
//! `export.defaults` and `export.run` through `Session::execute`, the export job on M8.3's runner.
//! An export of edited and unedited photos writes decodable files of the right size with the edits
//! (and the owned fields) applied; Cancel leaves only complete files and the job reports cancelled;
//! an export into an unwatched folder adds it as Scan Always and its photos appear, one under a
//! watched folder is only scanned; the folder lists in Projects and keeps its mark after a rebuild;
//! a second export of the same name makes `<name>-001`; the options used are the next defaults;
//! the owner thread makes no `fs` call; and the commands' contracts and graceful failures.
//! (Videos skipped and counted: `cmd::export`'s unit test, since a scan lists no video yet.)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::names;
use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, CmdError, JobEntry, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_library::{Group, Project, Row, Tree, TreeOptions};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(300);

fn jpeg(seed: u64, width: u32, height: u32) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, width, height), 85, None).unwrap()
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    ids: Vec<PhotoId>,
}

impl Lib {
    /// A library watching `Photos` (its `Lisbon` folder holds `n` photos of `w` × `h`), with
    /// `Pictures` as the user's Pictures folder (so the default export location lies outside every
    /// watched folder).
    fn new(n: usize, (w, h): (u32, u32)) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Photos").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64, w, h), Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        Lib { dir, s, ids: photos.into_iter().map(|(_, id)| id).collect() }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Photos").join("Lisbon")
    }

    fn exports(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Dalica").join("Exports")
    }

    /// Runs `export.run`, then waits for its job and for the scan that follows it.
    fn export(&mut self, params: Value) -> (Value, JobEntry) {
        let reply = self.s.execute("export.run", &params).unwrap();
        let job = reply["job"].as_u64().unwrap();
        let entry = self.s.wait_job(job, PATIENCE).unwrap();
        assert!(entry.state.is_finished(), "{entry:?}");
        self.s.wait_idle();
        (reply, entry)
    }

    fn names_in(&self, folder: &Path) -> Vec<String> {
        let id = self.s.library().folder_by_path(folder).unwrap();
        let mut names: Vec<String> = self.s.library().photos_in(id).iter().map(|p| p.name.clone()).collect();
        names.sort();
        names
    }
}

/// `path` decoded whole: its pixel size.
fn decoded_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    let img = dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap();
    (img.width, img.height)
}

fn code(r: Result<Value, CmdError>) -> &'static str {
    match r {
        Ok(v) => panic!("expected an error, got {v}"),
        Err(e) => e.code(),
    }
}

fn projects(s: &Session) -> Vec<PathBuf> {
    let options = TreeOptions { projects_dir: s.projects_dir(), ..TreeOptions::default() };
    let tree = Tree::build(s.library(), s.roots(), &options);
    let Some(group) = tree.group(Group::Projects) else { return Vec::new() };
    group
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Folder(f) => Some(f.path.clone()),
            Row::Year(_) => None,
        })
        .collect()
}

#[test]
fn an_export_of_edited_and_unedited_photos_writes_decodable_files_with_the_edits() {
    let mut lib = Lib::new(3, (400, 300));
    let ids = lib.ids.clone();
    lib.s.execute("photo.rotate.cw", &json!({ "ids": [ids[1]] })).unwrap();
    lib.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    lib.s.execute("photo.star.set", &json!({ "ids": [ids[2]], "on": true })).unwrap();
    lib.s.wait_idle();
    let original = std::fs::read(lib.lisbon().join("IMG_0002.jpg")).unwrap();
    let undo_before = lib.s.undo_stack().len();

    let defaults = lib.s.execute("export.defaults", &json!({ "ids": ids })).unwrap();
    assert_eq!(defaults["location"], json!(lib.exports()));
    assert_eq!(defaults["folderName"], "Lisbon");
    let location = lib.exports();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let (reply, done) = lib.export(json!({ "ids": ids, "dir": location, "folderName": "Lisbon", "size": 200, "quality": "maximum" }));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");

    assert_eq!((reply["pending"].clone(), reply["total"].clone(), reply["videos"].clone()), (json!(true), json!(3), json!(0)));
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.label, "Exported 3 pictures");
    let out = location.join("Lisbon");
    let result = done.result.unwrap();
    let written: Vec<PathBuf> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"].iter().map(|n| out.join(n)).collect();
    assert_eq!(result["written"], json!(written));
    assert_eq!((result["done"].clone(), result["skipped"].clone(), result["failed"].clone()), (json!(3), json!([]), json!([])));
    assert_eq!((result["folder"].clone(), result["name"].clone(), result["cancelled"].clone()), (json!(out), json!("Lisbon"), json!(false)));
    lib.s.flush().unwrap(); // the folder's sidecar is written on a debounce: list it once it has landed
    assert_eq!(names(&out), [".dalica.json", "IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);

    // Decodable, of the size asked for, the unsaved rotation applied.
    assert_eq!(decoded_size(&written[0]), (200, 150));
    assert_eq!(decoded_size(&written[1]), (150, 200), "the unsaved quarter turn is exported");
    assert_eq!(decoded_size(&written[2]), (200, 150));
    let meta = |p: &Path| dalica_meta::read(&std::fs::read(p).unwrap(), &dalica_meta::Limits::default()).unwrap().owned;
    assert_eq!(meta(&written[0]).caption.as_deref(), Some("Tram 28"), "the library's caption is carried");
    assert_eq!(meta(&written[2]).rating, Some(5), "a star is written as a rating");
    // The photo and its edits are untouched, and an export is no undo step.
    assert_eq!(std::fs::read(lib.lisbon().join("IMG_0002.jpg")).unwrap(), original);
    assert_eq!(lib.s.library().info(ids[1]).edits, ["rotate(90)"]);
    assert_eq!(lib.s.undo_stack().len(), undo_before);

    // The folder lay under no watched folder: it is added as Scan Always and its photos appear,
    // marked Exported and listed in Projects.
    assert!(lib.s.roots().contains(&out), "{:?}", lib.s.roots());
    assert_eq!(lib.names_in(&out), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    let folder = lib.s.library().folder_by_path(&out).unwrap();
    assert_eq!(lib.s.library().project(folder), Some(Project::Exported));
    assert!(projects(&lib.s).contains(&out));
    let event = lib.s.file_job_events().last().unwrap().clone();
    assert_eq!((event.message.as_str(), event.folder.as_deref(), event.done, event.total), ("Exported 3 pictures", Some(out.as_path()), 3, 3));

    // The mark is in the folder's sidecar, so a rebuild keeps it.
    lib.s.flush().unwrap();
    lib.s.execute("library.rebuild", &json!({})).unwrap();
    lib.s.wait_idle();
    let folder = lib.s.library().folder_by_path(&out).unwrap();
    assert_eq!(lib.s.library().project(folder), Some(Project::Exported));
    assert_eq!(lib.names_in(&out), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert!(projects(&lib.s).contains(&out));
}

#[test]
fn a_second_export_of_the_same_name_makes_name_001_with_the_remembered_options() {
    let mut lib = Lib::new(2, (400, 300));
    let ids = lib.ids.clone();
    let dir = lib.dir.path().join("Out");
    let (_, first) =
        lib.export(json!({ "ids": ids, "dir": dir, "folderName": "Lisbon", "size": 120, "quality": 70, "numbers": true, "watermark": "© Ana" }));
    assert_eq!(first.result.unwrap()["name"], "Lisbon");
    lib.s.flush().unwrap();
    assert_eq!(names(&dir.join("Lisbon")), [".dalica.json", "001-IMG_0001.jpg", "002-IMG_0002.jpg"]);
    // The options used are the next defaults.
    let d = lib.s.execute("export.defaults", &json!({ "section": format!("folder:{}", lib.s.library().photo(ids[0]).unwrap().folder) })).unwrap();
    assert_eq!(d["location"], json!(dir));
    assert_eq!((d["resize"].clone(), d["size"].clone()), (json!(true), json!(120)));
    assert_eq!((d["quality"].clone(), d["customQuality"].clone()), (json!("custom"), json!(70)));
    assert_eq!((d["numbers"].clone(), d["watermark"].clone(), d["watermarkText"].clone()), (json!(true), json!(true), json!("© Ana")));
    assert_eq!((d["folderName"].clone(), d["count"].clone(), d["videos"].clone()), (json!("Lisbon"), json!(2), json!(false)));

    // Left out, they come from the settings; the taken name gets -001.
    let (_, second) = lib.export(json!({ "ids": [ids[1]], "dir": dir, "folderName": "Lisbon", "watermark": null }));
    let second_dir = dir.join("Lisbon-001");
    assert_eq!(second.result.unwrap()["name"], "Lisbon-001");
    lib.s.flush().unwrap();
    assert_eq!(names(&second_dir), [".dalica.json", "001-IMG_0002.jpg"]);
    assert_eq!(decoded_size(&second_dir.join("001-IMG_0002.jpg")), (120, 90));
    assert_eq!(lib.s.execute("export.defaults", &json!({})).unwrap()["watermark"], false);
    // Both folders are in the library, each marked.
    for out in [dir.join("Lisbon"), second_dir] {
        let folder = lib.s.library().folder_by_path(&out).unwrap();
        assert_eq!(lib.s.library().project(folder), Some(Project::Exported), "{}", out.display());
    }
    // The original size, and a mixed tray's name.
    let porto = lib.dir.path().join("Photos").join("Porto");
    dalica_fs::create_dir_all(&porto).unwrap();
    dalica_fs::replace(&porto.join("P.jpg"), &jpeg(9, 64, 48), Expect::Absent).unwrap();
    lib.s.start_verification();
    lib.s.wait_idle();
    let p = lib.s.library().photos().find(|p| p.name == "P.jpg").unwrap().id;
    let d = lib.s.execute("export.defaults", &json!({ "ids": [ids[0], p] })).unwrap();
    assert!(d["folderName"].as_str().unwrap().starts_with("Export 20"), "{d}");
    assert_eq!(d["folderName"], d["fallbackName"]);
    let (_, third) = lib.export(json!({ "ids": [p], "dir": dir, "folderName": "", "size": "original", "watermark": false }));
    let name = third.result.unwrap()["name"].as_str().unwrap().to_string();
    assert_eq!(name, d["fallbackName"].as_str().unwrap(), "an empty name takes the fallback");
    assert_eq!(decoded_size(&dir.join(&name).join("001-P.jpg")), (64, 48), "never enlarged, the original size");
}

#[test]
fn cancel_leaves_only_complete_files_and_the_job_reports_cancelled() {
    let mut lib = Lib::new(12, (1200, 900));
    let ids = lib.ids.clone();
    let dir = lib.dir.path().join("Out");
    let reply = lib.s.execute("export.run", &json!({ "ids": ids, "dir": dir, "folderName": "Big", "size": "original" })).unwrap();
    let job = reply["job"].as_u64().unwrap();
    // Cancel once the first photo is written.
    let start = Instant::now();
    while lib.s.job(job).unwrap().progress.done == 0 {
        assert!(start.elapsed() < PATIENCE, "timed out");
        lib.s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(lib.s.cancel_jobs(Some(job)).unwrap(), [job]);
    let done = lib.s.wait_job(job, PATIENCE).unwrap();
    lib.s.wait_idle();
    assert_eq!(done.state, JobState::Cancelled);
    let result = done.result.unwrap();
    assert_eq!(result["cancelled"], true);
    let written = result["written"].as_array().unwrap().len();
    assert!((1..12).contains(&written), "{written} written");
    assert!(done.label.starts_with("Export cancelled: "), "{}", done.label);
    // Only complete files: each one decodes whole at full size; no temporary file is left.
    let out = dir.join("Big");
    lib.s.flush().unwrap(); // the folder's sidecar has landed: its temporary file is not mistaken for the export's
    let names = names(&out);
    assert_eq!(names.len(), written + 1, "{names:?}");
    for name in names.iter().filter(|n| *n != ".dalica.json") {
        assert!(name.starts_with("IMG_") && name.ends_with(".jpg"), "{name}");
        assert_eq!(decoded_size(&out.join(name)), (1200, 900));
    }
    let event = lib.s.file_job_events().last().unwrap().clone();
    assert!(event.cancelled);
    assert_eq!(lib.names_in(&out).len(), written, "what was written is in the library");
}

#[test]
fn an_export_under_a_watched_folder_is_scanned_not_added() {
    let mut lib = Lib::new(2, (200, 150));
    let roots = lib.s.roots().to_vec();
    let dir = lib.dir.path().join("Photos").join("Exports");
    let section = format!("folder:{}", lib.s.library().photo(lib.ids[0]).unwrap().folder);
    let (_, done) = lib.export(json!({ "section": section, "dir": dir, "folderName": "Lisbon", "size": 100 }));
    assert_eq!(done.state, JobState::Done);
    let out = dir.join("Lisbon");
    assert_eq!(lib.s.roots(), roots.as_slice(), "no new watched folder");
    assert_eq!(lib.names_in(&out), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    let folder = lib.s.library().folder_by_path(&out).unwrap();
    assert_eq!(lib.s.library().project(folder), Some(Project::Exported));
    assert!(projects(&lib.s).contains(&out));
}

#[test]
fn the_commands_keep_their_contract_and_fail_gracefully() {
    let mut lib = Lib::new(1, (64, 48));
    let id = lib.ids[0];
    assert!((lib.s.spec("export.run").unwrap().enabled)(&lib.s, &json!({})).is_err(), "nothing to export");
    assert!((lib.s.spec("export.run").unwrap().enabled)(&lib.s, &json!({ "ids": [id] })).is_ok());

    let dir = lib.dir.path().join("Out");
    let run = |extra: Value| {
        let mut p = json!({ "ids": [id], "dir": dir, "folderName": "X" });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    let long = "w".repeat(2000);
    let cases: Vec<(Value, &str)> = vec![
        (json!({ "ids": [id], "folderName": "X" }), "bad_params"),
        (json!({ "ids": [id], "dir": dir }), "bad_params"),
        (run(json!({ "dir": "relative/out" })), "bad_params"),
        (run(json!({ "dir": dir.join("..").join("Out") })), "bad_params"),
        (run(json!({ "dir": dir.join(".dalica-originals") })), "bad_params"),
        (run(json!({ "size": 99 })), "bad_params"),
        (run(json!({ "size": 4001 })), "bad_params"),
        (run(json!({ "size": "big" })), "bad_params"),
        (run(json!({ "size": -1 })), "bad_params"),
        (run(json!({ "quality": 0 })), "bad_params"),
        (run(json!({ "quality": 101 })), "bad_params"),
        (run(json!({ "quality": "best" })), "bad_params"),
        (run(json!({ "watermark": 7 })), "bad_params"),
        (run(json!({ "watermark": "  " })), "bad_params"),
        (run(json!({ "watermark": true })), "bad_params"),
        (run(json!({ "watermark": long })), "limit"),
        (run(json!({ "numbers": "yes" })), "bad_params"),
        (run(json!({ "ids": [999_999] })), "not_found"),
        (run(json!({ "ids": [] })), "disabled"),
        (run(json!({ "section": "folder:1" })), "bad_params"),
        (json!({ "section": "folder:999", "dir": dir, "folderName": "X" }), "not_found"),
        (json!({ "section": "album:1", "dir": dir, "folderName": "X" }), "bad_params"),
        (json!({ "album": "a91c0e", "dir": dir, "folderName": "X" }), "not_found"),
        (json!({ "dir": dir, "folderName": "X" }), "disabled"),
    ];
    for (params, want) in cases {
        assert_eq!(code(lib.s.execute("export.run", &params)), want, "{params}");
    }
    for (params, want) in [
        (json!({ "ids": [999_999] }), "not_found"),
        (json!({ "section": "folder:x" }), "bad_params"),
        (json!({ "ids": [id], "section": "folder:1" }), "bad_params"),
    ] {
        assert_eq!(code(lib.s.execute("export.defaults", &params)), want, "{params}");
    }
    assert!(!dir.exists(), "nothing was made");
    assert!(lib.s.jobs().iter().all(|j| j.kind != dalica_engine::JobKind::Export), "no job started");
    // With nothing named the defaults still answer.
    let d = lib.s.execute("export.defaults", &json!({})).unwrap();
    assert_eq!((d["count"].clone(), d["size"].clone(), d["quality"].clone()), (json!(0), json!(1600), json!("automatic")));
    assert!(d["folderName"].as_str().unwrap().starts_with("Export "));
}
