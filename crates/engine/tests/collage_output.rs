//! Create Collage, Save Draft and Edit Collage (`roadmap.md` M14.17; `creating.md` §1 and §3):
//! Create writes a JPEG of the page's ratio at 3000 px on the long side with its recipe, a second
//! one takes `-001`, and the photo is in the Collages project; a Current Display page has the
//! display's size; Save Draft writes a draft; Cancel leaves no file; Edit Collage reopens with the
//! same clips, settings and placements, reports a clip deleted meanwhile, and a Create from it
//! writes beside it; a recipe's path outside the library is never opened and a hostile recipe is
//! refused; over automation a destination outside the write roots is refused; the owner thread
//! makes no file system call.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::{jpeg_of, lisbon, open_with, read, shared, virtual_clock};
use dalica_core::PhotoId;
use dalica_create::collage::{Collage as Doc, Recipe};
use dalica_engine::automation::{Output, Policy};
use dalica_engine::cmd::collage::NOT_OPEN;
use dalica_engine::collage_output::{COLLAGES, NOTHING_ON_PAGE, UNSAVED_OPEN};
use dalica_engine::watching::FIRST_VERIFICATION;
use dalica_engine::{JobEntry, JobState, Session};
use dalica_fs::Expect;
use dalica_library::Project;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refusal(s: &mut Session, id: &str, p: Value) -> (String, String) {
    let e = s.execute(id, &p).map(|v| panic!("{id} {p} ran: {v}")).unwrap_err();
    (e.code().to_string(), e.to_string())
}

/// Waits for the job a reply names, then for the scans it started.
fn wait(s: &mut Session, reply: &Value) -> JobEntry {
    let job = reply["job"].as_u64().unwrap_or_else(|| panic!("no job in {reply}"));
    let entry = s.wait_job(job, PATIENCE).unwrap();
    assert!(entry.state.is_finished(), "{entry:?}");
    s.wait_idle();
    entry
}

fn size_of(path: &Path) -> (u32, u32) {
    dalica_image::read_header_file(path, &dalica_image::Limits::default()).unwrap().0.display_size()
}

fn recipe_at(path: &Path) -> Recipe {
    Recipe::parse(&read(path)).unwrap()
}

fn files_in(folder: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// Photos of four shapes.
fn photos(n: usize) -> Vec<Vec<u8>> {
    let shapes = [(96, 64), (64, 96), (80, 80), (120, 40)];
    (0..n).map(|i| jpeg_of(i as u64 + 7, shapes[i % 4].0, shapes[i % 4].1)).collect()
}

struct Lib {
    lib: tempfile::TempDir,
    disk: tempfile::TempDir,
    pictures: tempfile::TempDir,
    s: Session,
    ids: Vec<PhotoId>,
}

impl Lib {
    /// `n` photos in a watched `Lisbon`, and a Pictures folder of the test's own.
    fn new(n: usize) -> Lib {
        Lib::with(&photos(n))
    }

    /// These photos in a watched `Lisbon`, and a Pictures folder of the test's own.
    fn with(photos: &[Vec<u8>]) -> Lib {
        let (lib, disk, _folder, mut s, ids) = lisbon(photos, None);
        let pictures = tempfile::tempdir().unwrap();
        s.set_pictures_dir(Some(pictures.path().to_path_buf()));
        Lib { lib, disk, pictures, s, ids }
    }

    fn collages(&self) -> PathBuf {
        self.pictures.path().join("Dalica").join(COLLAGES)
    }

    /// Opens the room on every photo with seed `seed`.
    fn room(&mut self, seed: &str) {
        let ids = self.ids.clone();
        run(&mut self.s, "create.collage", json!({ "ids": ids, "seed": seed }));
    }

    /// Runs `id` (Create or Save Draft) and waits for its job.
    fn output(&mut self, id: &str, p: Value) -> (Value, JobEntry) {
        let reply = run(&mut self.s, id, p);
        let entry = wait(&mut self.s, &reply);
        (reply, entry)
    }
}

#[test]
fn create_writes_a_jpeg_of_the_pages_ratio_and_its_recipe_twice_gives_001_and_the_photo_is_in_the_collages_project() {
    let _faults = shared();
    let mut l = Lib::new(6);
    l.room("c011a6e");
    run(&mut l.s, "collage.set", json!({ "style": "grid", "shadows": false }));
    assert!(l.s.collage().is_changed());
    let doc = l.s.collage().document().cloned().unwrap();
    let page = run(&mut l.s, "collage.inspect", json!({}))["page"].clone();
    assert_eq!((page["width"].clone(), page["height"].clone()), (json!(3000), json!(1688)), "16:9 at 3000 px");

    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let (reply, entry) = l.output("collage.create", json!({}));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!((reply["pending"].clone(), reply["total"].clone(), reply["draft"].clone()), (json!(true), json!(2), json!(false)));
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    let result = entry.result.unwrap();
    let jpeg = PathBuf::from(result["path"].as_str().unwrap());
    let name = jpeg.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("Collage 20") && name.ends_with(".jpg"), "{name}");
    assert_eq!(jpeg.parent(), Some(l.collages().as_path()));
    assert_eq!(entry.label, format!("Created {name}"));
    assert_eq!(size_of(&jpeg), (3000, 1688));

    // The recipe beside it: the collage as it was, each clip's path, the output's size.
    let stem = name.trim_end_matches(".jpg");
    let recipe_path = l.collages().join(format!("{stem}.dalica-collage"));
    assert_eq!(result["recipe"], json!(recipe_path));
    let recipe = recipe_at(&recipe_path);
    assert_eq!(recipe.collage, doc);
    assert_eq!((recipe.draft, recipe.output), (false, Some((3000, 1688))));
    let lisbon = l.disk.path().join("Lisbon");
    assert_eq!(recipe.paths[0], lisbon.join("IMG_0001.jpg").to_str().unwrap());
    assert!(!l.s.collage().is_changed(), "the written collage counts as saved");

    // The photo is in the library, in the Collages project, and can be edited as a collage.
    let made = l.s.collage_made().unwrap();
    let photo = made.photo.expect("the collage is in the library");
    assert_eq!((made.path.clone(), made.draft), (jpeg.clone(), false));
    assert_eq!(result["photo"], Value::Null, "the new folder was scanned after the job");
    let lib = l.s.library();
    let folder = lib.photo(photo).unwrap().folder;
    assert_eq!(lib.folder(folder).unwrap().path, l.collages());
    assert_eq!(lib.project(folder), Some(Project::Collages));
    assert_eq!(run(&mut l.s, "collage.canEdit", json!({ "id": photo })), json!({ "id": photo, "canEdit": true, "pending": false }));

    // Again: the next free pair of names.
    let (_, again) = l.output("collage.create", json!({}));
    let again = again.result.unwrap();
    let second = PathBuf::from(again["path"].as_str().unwrap());
    assert_eq!(again["photo"], json!(l.s.collage_made().unwrap().photo), "the folder is indexed now: the photo is known at once");
    assert_eq!(second, l.collages().join(format!("{stem}-001.jpg")));
    let mut want = vec![
        ".dalica.json".to_string(),
        name.clone(),
        format!("{stem}.dalica-collage"),
        format!("{stem}-001.jpg"),
        format!("{stem}-001.dalica-collage"),
    ];
    want.sort();
    assert_eq!(files_in(&l.collages()), want);
    assert_eq!(l.s.collage_made().unwrap().seq, made.seq + 1);
}

#[test]
fn a_current_display_page_is_written_at_the_displays_size() {
    let _faults = shared();
    let mut l = Lib::new(3);
    l.room("d15");
    run(&mut l.s, "collage.set", json!({ "pageFormat": "Current Display", "display": [1280, 800] }));
    let (reply, entry) = l.output("collage.create", json!({}));
    assert_eq!(reply["size"], json!([1280, 800]));
    assert_eq!(size_of(Path::new(entry.result.unwrap()["path"].as_str().unwrap())), (1280, 800));
    // Turned to portrait, the same display gives the page's other way round.
    run(&mut l.s, "collage.set", json!({ "orientation": "portrait" }));
    let (_, entry) = l.output("collage.create", json!({ "display": [1280, 800] }));
    assert_eq!(size_of(Path::new(entry.result.unwrap()["path"].as_str().unwrap())), (800, 1280));
}

#[test]
fn save_draft_writes_a_draft_and_can_close_the_room() {
    let _faults = shared();
    let mut l = Lib::new(3);
    l.room("d4af7");
    run(&mut l.s, "collage.set", json!({ "style": "mosaic" }));
    let doc = l.s.collage().document().cloned().unwrap();
    let (reply, entry) = l.output("collage.saveDraft", json!({ "close": true }));
    assert_eq!(reply["draft"], json!(true));
    assert!(!l.s.collage().is_open(), "Close's Save Draft closes the room");
    let result = entry.result.unwrap();
    let recipe = recipe_at(Path::new(result["recipe"].as_str().unwrap()));
    assert_eq!((recipe.draft, recipe.collage), (true, doc));
    let made = l.s.collage_made().unwrap();
    assert!(made.draft && made.photo.is_some(), "a draft is in the library too, and is not opened: {made:?}");
    assert!(entry.label.starts_with("Saved the collage draft Collage "), "{}", entry.label);
    assert_eq!(refusal(&mut l.s, "collage.create", json!({ "close": true })).0, "disabled", "no room open");
}

#[test]
fn cancel_leaves_no_file() {
    let _faults = shared();
    // Large photos, so the render runs well past the cancel sent right after the job starts.
    let big: Vec<Vec<u8>> = (0..12).map(|i| jpeg_of(i, 1600, 1200)).collect();
    let mut l = Lib::with(&big);
    l.room("cace1");
    run(&mut l.s, "collage.set", json!({ "shadows": false }));
    let reply = run(&mut l.s, "collage.create", json!({}));
    let job = reply["job"].as_u64().unwrap();
    assert_eq!(l.s.cancel_jobs(Some(job)).unwrap(), vec![job]);
    let entry = wait(&mut l.s, &reply);
    assert_eq!(entry.state, JobState::Cancelled, "{entry:?}");
    assert_eq!(entry.label, "Collage cancelled: nothing was written");
    assert!(!l.collages().exists(), "no folder and no file");
    assert!(l.s.collage().is_changed(), "the room still has its unsaved changes");
    assert!(l.s.collage_made().is_none());
}

#[test]
fn edit_collage_reopens_the_same_collage_reports_a_clip_deleted_meanwhile_and_creates_beside_it() {
    let _faults = shared();
    let mut l = Lib::new(6);
    l.room("ed17");
    run(&mut l.s, "collage.set", json!({ "style": "pile", "borders": "instant", "captions": true, "backgroundColor": "#203040" }));
    run(&mut l.s, "collage.move", json!({ "item": 2, "centre": [0.3, 0.6], "angle": 12 }));
    run(&mut l.s, "collage.unplace", json!({ "ids": [l.ids[4]] }));
    let ids = l.ids.clone();
    run(&mut l.s, "collage.background", json!({ "id": ids[1] }));
    let before = run(&mut l.s, "collage.inspect", json!({}));
    let elsewhere = l.disk.path().join("Elsewhere");
    let (_, entry) = l.output("collage.create", json!({ "path": elsewhere }));
    let jpeg = PathBuf::from(entry.result.unwrap()["path"].as_str().unwrap());
    assert_eq!(jpeg.parent(), Some(elsewhere.as_path()));
    let photo = l.s.collage_made().unwrap().photo.unwrap();
    assert_ne!(l.s.library().project(l.s.library().photo(photo).unwrap().folder), Some(Project::Collages), "only the Collages folder is marked");
    run(&mut l.s, "collage.close", json!({}));

    // Reopened: the same clips, settings and placements, no unsaved changes, an empty history.
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let reply = run(&mut l.s, "collage.edit", json!({ "id": photo }));
    let entry = wait(&mut l.s, &reply);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    assert_eq!(entry.result.unwrap(), json!({ "opened": true, "clips": 6, "missing": [] }));
    let after = run(&mut l.s, "collage.inspect", json!({}));
    for key in ["document", "placements", "clips", "page"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    assert_eq!((after["unsaved"].clone(), after["undo"].clone()), (json!(false), json!(0)));

    // Create Collage from the reopened collage writes a new one beside it.
    let (reply, entry) = l.output("collage.create", json!({}));
    assert_eq!(reply["folder"], json!(elsewhere));
    let next = PathBuf::from(entry.result.unwrap()["path"].as_str().unwrap());
    assert_eq!(next.parent(), Some(elsewhere.as_path()));
    assert_ne!(next, jpeg);
    run(&mut l.s, "collage.set", json!({ "shadows": false }));
    assert_eq!(refusal(&mut l.s, "collage.edit", json!({ "id": photo })), ("disabled".into(), UNSAVED_OPEN.into()));
    run(&mut l.s, "collage.close", json!({ "confirm": true }));

    // A clip's photo deleted meanwhile (gone from the disk while the library was closed).
    let Lib { lib, disk, pictures, s, ids } = l;
    drop(s);
    let gone = disk.path().join("Lisbon").join("IMG_0004.jpg");
    std::fs::remove_file(&gone).unwrap();
    let clock = virtual_clock();
    let mut s = open_with(lib.path(), clock.clone());
    s.set_pictures_dir(Some(pictures.path().to_path_buf()));
    s.poll();
    clock.advance(FIRST_VERIFICATION);
    s.poll();
    s.wait_idle();
    assert!(s.library().photo(ids[3]).is_none(), "the verification found it gone");
    let photo = s.library().photos().find(|p| p.name == jpeg.file_name().unwrap().to_str().unwrap()).unwrap().id;
    // Can Edit looks on a worker first, then knows.
    let looking = run(&mut s, "collage.canEdit", json!({ "id": photo }));
    assert_eq!((looking["canEdit"].clone(), looking["pending"].clone()), (json!(false), json!(true)));
    s.wait_job(looking["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(run(&mut s, "collage.canEdit", json!({ "id": photo }))["canEdit"], json!(true));
    let plain = ids[0];
    let first = run(&mut s, "collage.canEdit", json!({ "id": plain }));
    s.wait_job(first["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(run(&mut s, "collage.canEdit", json!({ "id": plain }))["canEdit"], json!(false), "a photo without a recipe");

    let reply = run(&mut s, "collage.edit", json!({ "id": photo }));
    let entry = wait(&mut s, &reply);
    assert_eq!(entry.result.unwrap(), json!({ "opened": true, "clips": 5, "missing": [{ "id": ids[3], "path": gone }] }));
    assert_eq!(entry.label, "Opened the collage; 1 of its pictures is no longer in the library");
    let doc = s.collage().document().cloned().unwrap();
    let kept: Vec<PhotoId> = ids.iter().copied().filter(|id| *id != ids[3]).collect();
    assert_eq!(doc.clips.iter().map(|c| c.id).collect::<Vec<_>>(), kept);
    assert_eq!(doc.background.picture, Some(ids[1]));
    assert_eq!(serde_json::to_value(doc.settings).unwrap(), before["document"]["settings"]);
}

#[test]
fn a_recipes_path_outside_the_library_is_never_opened_and_a_hostile_recipe_is_refused() {
    let _faults = shared();
    let mut l = Lib::new(2);
    let (outside, lisbon) = (l.disk.path().join("Outside"), l.disk.path().join("Lisbon"));
    dalica_fs::create_dir_all(&outside).unwrap();
    dalica_fs::replace(&outside.join("secret.jpg"), &jpeg_of(99, 64, 64), Expect::Absent).unwrap();
    // A recipe beside IMG_0001.jpg: one clip of the library, one unknown id whose path lies outside.
    let stranger = PhotoId::new(9_999);
    let doc = Doc::new([l.ids[1], stranger], 5);
    let paths = vec![lisbon.join("IMG_0002.jpg").to_str().unwrap().to_string(), outside.join("secret.jpg").to_str().unwrap().to_string()];
    let bytes = Recipe::new(doc, paths, false, None).unwrap().print();
    dalica_fs::replace(&lisbon.join("IMG_0001.dalica-collage"), &bytes, Expect::Absent).unwrap();
    let photos = l.s.library().photos().count();
    let ids = l.ids.clone();
    let reply = run(&mut l.s, "collage.edit", json!({ "id": ids[0] }));
    let entry = wait(&mut l.s, &reply);
    assert_eq!(entry.result.unwrap(), json!({ "opened": true, "clips": 1, "missing": [{ "id": stranger, "path": outside.join("secret.jpg") }] }));
    assert_eq!(l.s.library().photos().count(), photos, "nothing was added");
    assert!(l.s.library().folder_by_path(&outside).is_none(), "the outside folder was never read");
    run(&mut l.s, "collage.close", json!({}));

    // Hostile recipes: refused by the job, never a panic; Can Edit says no.
    let nan = String::from_utf8(bytes).unwrap().replacen("\"seed\"", "\"zzz\": NaN, \"seed\"", 1);
    for (i, text) in [nan.as_str(), "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]", "{\"dalica\": {\"version\": 99}}"]
        .iter()
        .enumerate()
    {
        let path = lisbon.join("IMG_0002.dalica-collage");
        let _ = std::fs::remove_file(&path);
        dalica_fs::replace(&path, text.as_bytes(), Expect::Absent).unwrap();
        let reply = run(&mut l.s, "collage.edit", json!({ "id": ids[1] }));
        let entry = wait(&mut l.s, &reply);
        assert_eq!(entry.state, JobState::Failed, "case {i}: {entry:?}");
        assert!(!l.s.collage().is_open(), "case {i}");
    }
    let looking = run(&mut l.s, "collage.canEdit", json!({ "id": ids[1] }));
    l.s.wait_job(looking["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(run(&mut l.s, "collage.canEdit", json!({ "id": ids[1] }))["canEdit"], json!(false));
}

#[test]
fn over_automation_a_destination_outside_the_write_roots_is_refused_and_bad_destinations_never_run() {
    let _faults = shared();
    let mut l = Lib::new(2);
    let out = l.disk.path().join("Out");
    dalica_fs::create_dir_all(&out).unwrap();
    let out = dalica_engine::automation::plain(&std::fs::canonicalize(&out).unwrap());
    let policy = Policy::with_roots(&[], std::slice::from_ref(&out));
    let watched = l.s.roots().to_vec();
    let code = |r: Result<PathBuf, dalica_engine::automation::Refused>| r.map_or_else(|e| e.code(), |_| "ok");
    l.room("a070");
    let inside = l.s.collage_destination(Some(&out.join("Collages"))).unwrap();
    assert_eq!(code(policy.check_output(&inside, "path", &watched, Output::Folder)), "ok");
    let elsewhere = l.s.collage_destination(Some(&l.disk.path().join("Elsewhere"))).unwrap();
    assert_eq!(code(policy.check_output(&elsewhere, "path", &watched, Output::Folder)), "path_denied");
    let default = l.s.collage_destination(None).unwrap();
    assert_eq!(default, l.collages());
    assert_eq!(code(policy.check_output(&default, "path", &watched, Output::Folder)), "path_denied", "Pictures is no write root");

    // The engine's own refusals: nothing starts, nothing is written.
    assert_eq!(refusal(&mut l.s, "collage.create", json!({ "path": "relative/Collages" })).0, "bad_params");
    let up = out.join("a").join("..").join("b");
    assert_eq!(refusal(&mut l.s, "collage.create", json!({ "path": up })).0, "bad_params");
    assert_eq!(refusal(&mut l.s, "collage.create", json!({ "path": out.join(".dalica-originals") })).0, "bad_params");
    assert_eq!(refusal(&mut l.s, "collage.create", json!({ "display": [0, 10] })).0, "bad_params");
    let ids = l.ids.clone();
    run(&mut l.s, "collage.unplace", json!({ "ids": ids }));
    assert_eq!(refusal(&mut l.s, "collage.create", json!({})), ("disabled".into(), NOTHING_ON_PAGE.into()));
    run(&mut l.s, "collage.close", json!({ "confirm": true }));
    assert_eq!(refusal(&mut l.s, "collage.saveDraft", json!({})), ("disabled".into(), NOT_OPEN.into()));
    assert_eq!(refusal(&mut l.s, "collage.edit", json!({ "id": 424_242 })).0, "not_found");
    assert_eq!(refusal(&mut l.s, "collage.canEdit", json!({ "id": 424_242 })).0, "not_found");
    assert!(!l.collages().exists() && files_in(&out).is_empty(), "nothing was written");
}
