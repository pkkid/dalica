//! Save a Copy and Save As (`roadmap.md` M8.17; `data-model.md` §6) on temporary folders:
//! `photo.saveCopy` and `photo.saveTo` through `Session::execute`, each a file job on M8.3's
//! runner. Two copies of one photo are `-001` and `-002` with its edits applied, the photo's file
//! and recipe unchanged and each copy a new, unedited photo with the photo's caption, tags, star
//! and date; Save As PNG into an unwatched folder adds it as Scan Always and the photo appears,
//! with the folder and format remembered; an existing path is refused and unchanged, a missing
//! folder too; the owner thread makes no `fs` call; and the commands' contracts and graceful
//! failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::names;
use dalica_core::{DateTime, PhotoId, SystemClock};
use dalica_engine::settings::SaveFormat;
use dalica_engine::{CacheDir, CmdError, JobEntry, JobKind, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
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
    /// A library watching `Photos`, whose `Lisbon` folder holds `n` photos of 400 × 300.
    fn new(n: usize) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Photos").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64, 400, 300), Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        Lib { dir, s, ids: photos.into_iter().map(|(_, id)| id).collect() }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Photos").join("Lisbon")
    }

    /// Runs `cmd`, then waits for its job and for whatever follows it (a scan), with the owner
    /// thread marked as the UI thread: it must make no `fs` call.
    fn run(&mut self, cmd: &str, params: Value) -> (Value, JobEntry) {
        let refused = dalica_fs::guard::refused();
        let ui = dalica_fs::mark_ui_thread();
        let reply = self.s.execute(cmd, &params).unwrap();
        let job = reply["job"].as_u64().unwrap();
        let entry = self.s.wait_job(job, PATIENCE).unwrap();
        drop(ui);
        assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
        assert!(entry.state.is_finished(), "{entry:?}");
        self.s.wait_idle();
        (reply, entry)
    }

    fn photo_named(&self, folder: &Path, name: &str) -> Option<PhotoId> {
        let f = self.s.library().folder_by_path(folder)?;
        self.s.library().photo_by_name(f, name).map(|p| p.id)
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

#[test]
fn two_copies_are_001_and_002_with_the_edits_and_the_photo_untouched() {
    let mut lib = Lib::new(2);
    let id = lib.ids[0];
    let s = &mut lib.s;
    s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(90)" })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Tram 28" })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [id], "tags": "Lisbon, trams" })).unwrap();
    s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    s.execute("photo.dates.set", &json!({ "ids": [id], "set": "2025-05-01T10:00:00" })).unwrap();
    s.wait_idle();
    assert!(s.flush_metadata(PATIENCE));
    s.wait_idle();
    let source = lib.lisbon().join("IMG_0001.jpg");
    let original = std::fs::read(&source).unwrap();
    let (info_before, photo_before) = (lib.s.library().info(id).clone(), lib.s.library().photo(id).unwrap().clone());
    let undo_before = lib.s.undo_stack().len();

    let (reply, first) = lib.run("photo.saveCopy", json!({ "ids": [id] }));
    assert_eq!((reply["pending"].clone(), reply["total"].clone()), (json!(true), json!(1)));
    assert_eq!((first.state, first.kind, first.label.as_str()), (JobState::Done, JobKind::Save, "Saved 1 copy"));
    let (_, second) = lib.run("photo.saveCopy", json!({ "ids": [id] }));
    let copies = [lib.lisbon().join("IMG_0001-001.jpg"), lib.lisbon().join("IMG_0001-002.jpg")];
    for (done, path) in [(&first, &copies[0]), (&second, &copies[1])] {
        let result = done.result.clone().unwrap();
        assert_eq!(result["written"], json!([path]));
        assert_eq!((result["done"].clone(), result["failed"].clone(), result["cancelled"].clone()), (json!(1), json!([]), json!(false)));
        let copy = lib.photo_named(&lib.lisbon(), path.file_name().unwrap().to_str().unwrap()).expect("the copy is in the library");
        assert_eq!(result["photos"], json!([copy]));
        // The edits are applied: the quarter turn makes it portrait.
        assert_eq!(decoded_size(path), (300, 400));
        // A new, unedited photo, holding the photo's owned fields.
        let (info, photo) = (lib.s.library().info(copy), lib.s.library().photo(copy).unwrap());
        assert!(info.edits.is_empty() && info.redo.is_empty(), "{info:?}");
        assert_eq!((photo.width, photo.height), (300, 400));
        assert_eq!(info.caption, "Tram 28");
        assert_eq!(info.tags, ["Lisbon", "trams"]);
        assert!(photo.star);
        assert_eq!(info.date, Some("2025-05-01T10:00:00".parse::<DateTime>().unwrap()));
        // The file carries them too.
        let owned = dalica_meta::read(&std::fs::read(path).unwrap(), &dalica_meta::Limits::default()).unwrap().owned;
        assert_eq!(owned.caption.as_deref(), Some("Tram 28"));
        assert_eq!(owned.rating, Some(5), "the star as a rating, whatever the write-back options say");
    }
    // Every sidecar write has landed (a debounced one leaves a temporary file while it is written).
    lib.s.flush().unwrap();
    assert_eq!(names(&lib.lisbon()), [".dalica.json", "IMG_0001-001.jpg", "IMG_0001-002.jpg", "IMG_0001.jpg", "IMG_0002.jpg"]);
    // The photo, its file and its recipe are untouched; neither copy is an undo step.
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(lib.s.library().info(id), &info_before);
    assert_eq!(lib.s.library().photo(id).unwrap(), &photo_before);
    assert_eq!(lib.s.library().info(id).edits, ["rotate(90)"]);
    assert!(lib.s.library().saved(id).is_none(), "no originals record");
    assert!(!lib.lisbon().join(".dalica-originals").exists());
    assert_eq!(lib.s.undo_stack().len(), undo_before);
    let event = lib.s.file_job_events().last().unwrap().clone();
    assert_eq!((event.message.as_str(), event.done, event.total), ("Saved 1 copy", 1, 1));

    // A gap is filled first; a photo without edits is left out.
    std::fs::remove_file(&copies[0]).unwrap();
    lib.s.start_verification();
    lib.s.wait_idle();
    let (reply, third) = lib.run("photo.saveCopy", json!({ "ids": [lib.ids[1], id] }));
    assert_eq!(reply["total"], 1, "the unedited photo is left out");
    assert_eq!(third.result.unwrap()["written"], json!([copies[0]]));
}

#[test]
fn save_as_png_into_an_unwatched_folder_adds_it_and_the_photo_appears() {
    let mut lib = Lib::new(1);
    let id = lib.ids[0];
    lib.s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(270)" })).unwrap();
    lib.s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Alfama" })).unwrap();
    lib.s.wait_idle();
    let elsewhere = lib.dir.path().join("Elsewhere");
    dalica_fs::create_dir_all(&elsewhere).unwrap();
    let roots_before = lib.s.roots().to_vec();
    assert!(!roots_before.iter().any(|r| elsewhere.starts_with(r)));
    assert_eq!(lib.s.settings().save().last_folder, None);

    let (reply, done) = lib.run("photo.saveTo", json!({ "id": id, "path": elsewhere.join("Alfama"), "format": "png" }));
    let written = elsewhere.join("Alfama.png");
    assert_eq!((reply["path"].clone(), reply["format"].clone(), reply["total"].clone()), (json!(written), json!("png"), json!(1)));
    assert_eq!((done.state, done.label.as_str()), (JobState::Done, "Saved as Alfama.png"));
    assert_eq!(done.result.unwrap()["written"], json!([written]));
    // A PNG with the edits applied.
    assert_eq!(
        dalica_image::read_header(&std::fs::read(&written).unwrap(), &dalica_image::Limits::default()).unwrap().format,
        dalica_image::Format::Png
    );
    assert_eq!(decoded_size(&written), (300, 400));
    // The folder lay under no watched folder: it is added as Scan Always and the photo appears.
    assert!(lib.s.roots().contains(&elsewhere), "{:?}", lib.s.roots());
    let copy = lib.photo_named(&elsewhere, "Alfama.png").expect("the new file is in the library");
    assert!(lib.s.library().info(copy).edits.is_empty());
    let event = lib.s.file_job_events().last().unwrap().clone();
    assert_eq!(event.folder.as_deref(), Some(elsewhere.as_path()));
    // The folder and the format are remembered for the next Save As.
    let remembered = lib.s.settings().save();
    assert_eq!((remembered.last_folder, remembered.last_format), (Some(elsewhere.clone()), SaveFormat::Png));
    // The photo is untouched.
    assert_eq!(lib.s.library().info(id).edits, ["rotate(270)"]);

    // A name without an extension now takes PNG; a relative path is taken from the photo's
    // folder, and the copy there gets the photo's caption by op.
    let (reply, _) = lib.run("photo.saveTo", json!({ "id": id, "path": "Alfama 2" }));
    let second = lib.lisbon().join("Alfama 2.png");
    assert_eq!(reply["path"], json!(second));
    let copy = lib.photo_named(&lib.lisbon(), "Alfama 2.png").expect("in the photo's folder");
    assert_eq!(lib.s.library().info(copy).caption, "Alfama");
    let (_, jpeg) = lib.run("photo.saveTo", json!({ "id": id, "path": lib.lisbon().join("Alfama 3.JPG") }));
    assert_eq!(jpeg.label, "Saved as Alfama 3.JPG");
    assert_eq!(lib.s.settings().save().last_format, SaveFormat::Jpeg);
}

#[test]
fn an_existing_path_is_refused_and_unchanged() {
    let mut lib = Lib::new(2);
    let id = lib.ids[0];
    lib.s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(90)" })).unwrap();
    lib.s.wait_idle();
    lib.s.flush().unwrap();
    let taken = lib.lisbon().join("IMG_0002.jpg");
    let bytes = std::fs::read(&taken).unwrap();
    let before = names(&lib.lisbon());
    let photos_before = lib.s.library().photos().count();

    let (_, done) = lib.run("photo.saveTo", json!({ "id": id, "path": taken }));
    assert_eq!(done.state, JobState::Done);
    let result = done.result.unwrap();
    assert_eq!((result["done"].clone(), result["written"].clone()), (json!(0), json!([])));
    let failed = &result["failed"][0];
    assert_eq!((failed["code"].clone(), failed["file"].clone()), (json!("exists"), json!(lib.lisbon().join("IMG_0001.jpg"))));
    assert!(failed["reason"].as_str().unwrap().contains("choose another name"));
    assert!(done.label.starts_with("Save As failed: "), "{}", done.label);
    assert_eq!(std::fs::read(&taken).unwrap(), bytes, "the existing file is unchanged");
    // The photo's own path is taken too.
    let (_, own) = lib.run("photo.saveTo", json!({ "id": id, "path": "IMG_0001.jpg" }));
    assert_eq!(own.result.unwrap()["failed"][0]["code"], "exists");
    // A folder that does not exist.
    let (_, missing) = lib.run("photo.saveTo", json!({ "id": id, "path": lib.dir.path().join("Nowhere").join("a.jpg") }));
    assert_eq!(missing.result.unwrap()["failed"][0]["code"], "no_folder");
    assert!(!lib.dir.path().join("Nowhere").exists());

    lib.s.flush().unwrap();
    assert_eq!(names(&lib.lisbon()), before, "nothing written");
    assert_eq!(lib.s.library().photos().count(), photos_before);
    assert_eq!(lib.s.settings().save().last_folder, None, "nothing remembered");
}

#[test]
fn the_commands_keep_their_contract_and_fail_gracefully() {
    let mut lib = Lib::new(2);
    let (id, plain) = (lib.ids[0], lib.ids[1]);
    lib.s.execute("photo.history.apply", &json!({ "id": id, "op": "autofix()" })).unwrap();
    let enabled = |s: &Session, cmd: &str, p: Value| (s.spec(cmd).unwrap().enabled)(s, &p);
    assert!(enabled(&lib.s, "photo.saveCopy", json!({ "ids": [id] })).is_ok());
    assert!(enabled(&lib.s, "photo.saveCopy", json!({ "ids": [plain] })).is_err(), "no edits");
    assert!(enabled(&lib.s, "photo.saveCopy", json!({ "ids": [] })).is_err());
    assert!(enabled(&lib.s, "photo.saveTo", json!({ "id": plain })).is_ok(), "Save As writes an unedited photo too");

    let out = lib.dir.path().join("Out");
    let long = "x".repeat(5000);
    let copy_cases: Vec<(Value, &str)> =
        vec![(json!({ "ids": [999_999] }), "not_found"), (json!({ "ids": [] }), "disabled"), (json!({ "ids": [plain] }), "disabled")];
    for (params, want) in copy_cases {
        assert_eq!(code(lib.s.execute("photo.saveCopy", &params)), want, "{params}");
    }
    let to_cases: Vec<(Value, &str)> = vec![
        (json!({ "id": id }), "bad_params"),
        (json!({ "path": out.join("a.jpg") }), "bad_params"),
        (json!({ "id": id, "path": out.join("a.jpg"), "format": "tiff" }), "bad_params"),
        (json!({ "id": id, "path": out.join("a.jpg"), "format": "png" }), "bad_params"),
        (json!({ "id": id, "path": out.join("a.tif") }), "bad_params"),
        (json!({ "id": id, "path": out.join("..").join("a.jpg") }), "bad_params"),
        (json!({ "id": id, "path": lib.lisbon().join(".dalica-originals").join("a.jpg") }), "bad_params"),
        (json!({ "id": id, "path": "" }), "bad_params"),
        (json!({ "id": id, "path": long }), "limit"),
        (json!({ "id": 999_999, "path": out.join("a.jpg") }), "not_found"),
    ];
    for (params, want) in to_cases {
        assert_eq!(code(lib.s.execute("photo.saveTo", &params)), want, "{params}");
    }
    assert!(!out.exists(), "nothing made");
    assert!(lib.s.jobs().iter().all(|j| j.kind != JobKind::Save), "no job started");
    assert_eq!(lib.s.settings().save().last_folder, None);
}
