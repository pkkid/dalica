//! Save, Undo Save and Revert through the engine's commands (`roadmap.md` M8.15; `data-model.md`
//! §6 and §10.9, `editing.md` §5.1, `control-protocol.md` §5.5) on temporary folders: Save → Undo
//! Save → Save → Revert returns the exact original bytes; the first-time explanation is asked
//! until `dontAsk`; Save clears the edited badge and the folder's count, Undo Save brings both
//! back and the "Undo Save" label goes; a second Save renders from the kept original; Cancel after
//! the first of three photos saves only that one; the thumbnails show the saved pixels; a rescan
//! imports nothing from Dalica's own write; the owner thread makes no `fs` call; a PNG with
//! transparency gets a JPEG beside it as a new photo with the source's owned fields, and a
//! rotate-only Save turns the photo with Undo Save giving the original back; each command's
//! contract and graceful failures (unknown photos, wrong types, nothing to save, a recipe Dalica
//! cannot render, an offline folder, a read-only file).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_edit::Recipe;
use dalica_engine::cmd::save::{self as save_cmd, Action, NOTHING_SAVED, NOTHING_TO_SAVE, OFFLINE, REVERT_QUESTION, SAVE_EXPLANATION};
use dalica_engine::offline::OnlineCheck;
use dalica_engine::{CacheDir, CmdError, Effect, History, JobEntry, JobState, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_image::Limits;
use dalica_library::SaveState;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);
const W: u32 = 320;
const H: u32 = 240;
const AUTOFIX: &str = "autofix()";
const TUNING: &str = "tuning(0.25,0,0,0,none)";

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    ids: Vec<PhotoId>,
}

impl Lib {
    /// A library watching `Pictures/Lisbon` with `n` photos `IMG_0001.jpg`… (ids in name order),
    /// with `sidecar` as the folder's `.dalica.json` when given, scanned.
    fn new(n: usize, sidecar: Option<Value>) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(30 + i as u64, W, H), 85, None).unwrap();
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
        }
        if let Some(file) = sidecar {
            dalica_fs::replace(&lisbon.join(".dalica.json"), &serde_json::to_vec_pretty(&file).unwrap(), Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        Lib { dir, s, ids: photos.into_iter().map(|(_, id)| id).collect() }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn path(&self, id: PhotoId) -> PathBuf {
        self.lisbon().join(&self.s.library().photo(id).unwrap().name)
    }

    fn kept(&self, id: PhotoId) -> PathBuf {
        self.lisbon().join(".dalica-originals").join(&self.s.library().photo(id).unwrap().name)
    }

    fn edit(&mut self, id: PhotoId, op: &str) {
        self.s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
    }

    fn edits(&self, id: PhotoId) -> Vec<String> {
        self.s.library().info(id).edits.clone()
    }

    /// Runs `cmd` with `params` and waits for its job.
    fn job(&mut self, cmd: &str, params: Value) -> JobEntry {
        let reply = self.s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e:?}"));
        assert_eq!(reply["pending"], json!(true), "{reply}");
        let entry = self.s.wait_job(job_id(&reply), PATIENCE).unwrap();
        assert!(entry.state.is_finished(), "{cmd}: {entry:?}");
        entry
    }

    fn save(&mut self, ids: &[PhotoId]) -> JobEntry {
        self.job("photo.save", json!({ "ids": ids, "confirm": true }))
    }

    fn history(&mut self, id: PhotoId) -> Value {
        self.s.execute("photo.history.state", &json!({ "id": id })).unwrap()
    }

    fn folder(&self) -> dalica_core::FolderId {
        self.s.library().folder_by_path(&self.lisbon()).unwrap()
    }
}

fn job_id(reply: &Value) -> dalica_engine::JobId {
    serde_json::from_value(reply["job"].clone()).unwrap()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn decode(bytes: &[u8]) -> dalica_image::Rgba8 {
    dalica_image::decode(bytes, &Limits::default()).unwrap()
}

/// Mean absolute difference per channel of two images of one size.
fn difference(a: (&[u8], u32, u32), b: (&[u8], u32, u32)) -> f64 {
    assert_eq!((a.1, a.2), (b.1, b.2));
    let sum: u64 = a.0.chunks(4).zip(b.0.chunks(4)).map(|(p, q)| (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>()).sum();
    sum as f64 / (f64::from(a.1) * f64::from(a.2) * 3.0)
}

/// `data-model.md` §10.9 through the commands, with the badge, the count and the "Undo Save"
/// label at each step.
#[test]
fn save_undo_save_save_revert_returns_the_exact_original_bytes() {
    let mut lib = Lib::new(2, None);
    let (a, b) = (lib.ids[0], lib.ids[1]);
    let original = read(&lib.path(a));
    lib.edit(a, AUTOFIX);
    lib.edit(b, AUTOFIX);
    let folder = lib.folder();
    assert_eq!(lib.s.library().unsaved_count(folder), 2);

    // Save: the file holds the edits, the original is kept, the badge and the count go.
    let done = lib.save(&[a]);
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.label, "Saved 1 photo");
    let result = done.result.unwrap();
    assert_eq!((result["done"].clone(), result["photos"].clone(), result["failed"].clone()), (json!(1), json!([a]), json!([])));
    assert_ne!(read(&lib.path(a)), original, "the edits are in the file");
    assert_eq!(read(&lib.kept(a)), original, "the original is kept byte for byte");
    assert!(lib.edits(a).is_empty());
    assert!(!lib.s.library().has_unsaved_edits(a));
    assert_eq!(lib.s.library().unsaved_count(folder), 1);
    let record = lib.s.library().saved(a).unwrap().clone();
    assert_eq!((record.recipe.clone(), record.state, record.size), (vec![AUTOFIX.to_string()], SaveState::Done, original.len() as u64));
    assert!(lib.s.undo_stack().is_empty(), "Save is no library undo step");
    let on_disk = std::fs::metadata(lib.path(a)).unwrap();
    assert_eq!(lib.s.library().photo(a).unwrap().size, on_disk.len(), "the photo's entry is refreshed");
    let h = lib.history(a);
    assert_eq!((h["undoLabel"].clone(), h["canUndo"].clone(), h["undoSave"].clone()), (json!("Undo Save"), json!(true), json!(true)));

    // Undo Save: the original back, the edits back as unsaved, the label gone.
    let done = lib.job("photo.undoSave", json!({ "ids": [a] }));
    assert_eq!((done.state, done.label.as_str()), (JobState::Done, "Undid the save of 1 photo"));
    assert_eq!(read(&lib.path(a)), original);
    assert!(!lib.kept(a).exists());
    assert_eq!(lib.edits(a), [AUTOFIX]);
    assert!(lib.s.library().saved(a).is_none());
    assert!(lib.s.library().has_unsaved_edits(a));
    assert_eq!(lib.s.library().unsaved_count(folder), 2);
    let h = lib.history(a);
    assert_eq!(h["undoLabel"], json!("Undo Auto Fix"));
    assert!(h.get("undoSave").is_none(), "{h}");

    // Save again, then Revert: the exact original, no edits, no record.
    assert_eq!(lib.save(&[a]).state, JobState::Done);
    assert_ne!(read(&lib.path(a)), original);
    let refused = lib.s.execute("photo.revert", &json!({ "ids": [a] }));
    assert!(matches!(&refused, Err(CmdError::Disabled(m)) if m.starts_with(REVERT_QUESTION)), "{refused:?}");
    let done = lib.job("photo.revert", json!({ "ids": [a], "confirm": true }));
    assert_eq!((done.state, done.label.as_str()), (JobState::Done, "Reverted 1 photo"));
    assert_eq!(read(&lib.path(a)), original, "§10.9: the exact original bytes");
    assert!(lib.edits(a).is_empty() && lib.s.library().saved(a).is_none());
    assert!(!lib.kept(a).exists());
    assert_eq!(lib.history(a)["undoLabel"], json!("Undo"));
    assert_eq!(lib.s.library().unsaved_count(folder), 1, "the other photo still has its edits");
    assert!(lib.s.undo_stack().is_empty());

    // The Revert question's Undo Save button: `action: "undoSave"`.
    lib.edit(a, TUNING);
    assert_eq!(lib.save(&[a]).state, JobState::Done);
    let done = lib.job("photo.revert", json!({ "ids": [a], "confirm": true, "action": "undoSave" }));
    assert_eq!(done.label, "Undid the save of 1 photo");
    assert_eq!((read(&lib.path(a)), lib.edits(a)), (original, vec![TUNING.to_string()]));
}

#[test]
fn the_explanation_is_asked_until_dont_ask() {
    let mut lib = Lib::new(3, None);
    let ids = lib.ids.clone();
    for &id in &ids {
        lib.edit(id, AUTOFIX);
    }
    let before = read(&lib.path(ids[0]));
    assert!(lib.s.settings().save().explain, "on by default");
    let asked = lib.s.execute("photo.save", &json!({ "ids": [ids[0]] }));
    assert!(matches!(&asked, Err(CmdError::Disabled(m)) if m.starts_with(SAVE_EXPLANATION)), "{asked:?}");
    assert!(!lib.s.file_jobs_running() && read(&lib.path(ids[0])) == before, "nothing ran");
    // Asked again while it is on.
    assert!(matches!(lib.s.execute("photo.save", &json!({ "ids": [ids[0]], "confirm": false })), Err(CmdError::Disabled(_))));
    // Confirmed without dontAsk: saved, and the next one asks again.
    assert_eq!(lib.save(&[ids[0]]).state, JobState::Done);
    assert!(lib.s.settings().save().explain);
    assert!(matches!(lib.s.execute("photo.save", &json!({ "ids": [ids[1]] })), Err(CmdError::Disabled(_))));
    // "Don't show this again": confirmed with dontAsk, and never asked again.
    lib.job("photo.save", json!({ "ids": [ids[1]], "confirm": true, "dontAsk": true }));
    assert!(!lib.s.settings().save().explain);
    let done = lib.job("photo.save", json!({ "ids": [ids[2]] }));
    assert_eq!(done.state, JobState::Done);
    assert!(lib.s.library().saved(ids[2]).is_some());
}

/// A second Save renders the recorded recipe followed by the new edits from the kept original,
/// and records the whole recipe; the first original stays the one kept.
#[test]
fn a_resave_after_a_tune_renders_from_the_original() {
    let mut lib = Lib::new(1, None);
    let a = lib.ids[0];
    let original = read(&lib.path(a));
    lib.edit(a, AUTOFIX);
    lib.save(&[a]);
    lib.edit(a, TUNING);
    assert!(lib.s.library().has_unsaved_edits(a));
    let done = lib.save(&[a]);
    assert_eq!(done.state, JobState::Done, "{:?}", done.result);
    assert_eq!(read(&lib.kept(a)), original, "the first original stays the one kept");
    let record = lib.s.library().saved(a).unwrap();
    assert_eq!(record.recipe, [AUTOFIX, TUNING]);
    assert!(lib.edits(a).is_empty());
    // The file is the whole recipe rendered from the original.
    let whole = Recipe::parse([AUTOFIX, TUNING]).unwrap();
    let spec = dalica_create::output::Spec::new(dalica_create::output::Purpose::InPlace);
    let oracle = decode(&dalica_create::output::render(&original, &whole, &spec).unwrap().bytes);
    let saved = decode(&read(&lib.path(a)));
    let d = difference((&saved.data, saved.width, saved.height), (&oracle.data, oracle.width, oracle.height));
    assert!(d < 0.5, "rendered from the original (mean difference {d:.3})");
}

/// Lets items through one at a time, and records which started.
#[derive(Default)]
struct Gate {
    state: Mutex<(usize, Vec<usize>)>,
    cv: Condvar,
}

impl Gate {
    fn open(&self, n: usize) {
        self.state.lock().unwrap().0 = n;
        self.cv.notify_all();
    }

    fn pass(&self, i: usize) {
        let mut st = self.state.lock().unwrap();
        st.1.push(i);
        self.cv.notify_all();
        let _ = self.cv.wait_timeout_while(st, Duration::from_secs(30), |st| st.0 <= i).unwrap();
    }

    fn wait_started(&self, i: usize) {
        let st = self.state.lock().unwrap();
        let _ = self.cv.wait_timeout_while(st, Duration::from_secs(30), |st| !st.1.contains(&i)).unwrap();
    }

    fn started(&self) -> Vec<usize> {
        self.state.lock().unwrap().1.clone()
    }
}

#[test]
fn cancel_after_the_first_of_three_photos_saves_only_that_one() {
    let mut lib = Lib::new(3, None);
    let ids = lib.ids.clone();
    let before: Vec<Vec<u8>> = ids.iter().map(|id| read(&lib.path(*id))).collect();
    for &id in &ids {
        lib.edit(id, AUTOFIX);
    }
    let gate = Arc::new(Gate::default());
    let hook = Arc::clone(&gate);
    let reply = save_cmd::start(&mut lib.s, Action::Save, &ids, Some(Arc::new(move |i| hook.pass(i)))).unwrap();
    assert_eq!(reply["total"], json!(3));
    let job = job_id(&reply);
    gate.wait_started(0);
    assert_eq!(lib.s.cancel_jobs(Some(job)).unwrap(), [job]);
    gate.open(3);
    let done = lib.s.wait_job(job, PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Cancelled);
    assert_eq!(gate.started(), [0], "no photo started after Cancel");
    assert_eq!(done.label, "Save cancelled: 1 of 3 photos saved");
    let result = done.result.unwrap();
    assert_eq!((result["done"].clone(), result["photos"].clone(), result["cancelled"].clone()), (json!(1), json!([ids[0]]), json!(true)));
    assert!(lib.s.library().saved(ids[0]).is_some() && lib.edits(ids[0]).is_empty());
    assert_ne!(read(&lib.path(ids[0])), before[0]);
    for i in 1..3 {
        assert_eq!(read(&lib.path(ids[i])), before[i], "photo {i} untouched");
        assert_eq!(lib.edits(ids[i]), [AUTOFIX], "photo {i} keeps its edits");
        assert!(lib.s.library().saved(ids[i]).is_none());
    }
    let event = lib.s.file_job_events().last().unwrap().clone();
    assert!(event.cancelled && event.done == 1 && event.total == 3);
}

/// The image photo `id` draws in a cell of 256 pixels once everything asked for has landed.
fn settled(s: &mut Session, id: PhotoId) -> Arc<dalica_thumbs::Rgba8> {
    for _ in 0..8 {
        s.update_thumbnail_recipes();
        if let Thumb::Image { image, covers: true, .. } = s.thumbnail(id, 256) {
            return image;
        }
        s.wait_idle();
    }
    panic!("photo {id} has no covering thumbnail: {:?}", s.thumbnail(id, 256));
}

#[test]
fn the_thumbnails_show_the_saved_pixels_and_a_rescan_imports_nothing() {
    let mut lib = Lib::new(1, None);
    let a = lib.ids[0];
    lib.edit(a, AUTOFIX);
    let edited = settled(&mut lib.s, a);
    lib.save(&[a]);
    let after = settled(&mut lib.s, a);
    // The saved file drawn without edits, at the thumbnail's size.
    let full = decode(&read(&lib.path(a)));
    let fit = dalica_edit::Target::Fit { width: 256, height: 256 };
    let oracle = dalica_edit::render(dalica_edit::Source::upright(&full), &Recipe::default(), fit).unwrap();
    let d = difference((&after.data, after.width, after.height), (&oracle.data, oracle.width, oracle.height));
    assert!(d < 3.0, "the thumbnail is the saved file (mean difference {d:.2})");
    let d = difference((&after.data, after.width, after.height), (&edited.data, edited.width, edited.height));
    assert!(d < 3.0, "and looks as the edited one did (mean difference {d:.2})");
    assert!(lib.s.library().sketch(a).is_some());

    // A rescan of the folder finds Dalica's own write and record as the model has them.
    let model = lib.s.library().model().clone();
    lib.s.start_verification();
    lib.s.wait_idle();
    assert_eq!(lib.s.library().model(), &model, "nothing imported from Dalica's own write");
    assert!(lib.s.undo_stack().is_empty());
}

#[test]
fn the_owner_thread_makes_no_fs_call() {
    let mut lib = Lib::new(2, None);
    let ids = lib.ids.clone();
    for &id in &ids {
        lib.edit(id, AUTOFIX);
    }
    let ui = dalica_fs::mark_ui_thread();
    let mut run = |cmd: &str, params: Value| {
        let reply = lib.s.execute(cmd, &params).unwrap();
        let entry = lib.s.wait_job(job_id(&reply), PATIENCE).unwrap();
        assert_eq!(entry.state, JobState::Done, "{cmd}: {entry:?}");
        assert!(entry.result.unwrap()["failed"].as_array().unwrap().is_empty());
    };
    run("photo.save", json!({ "ids": ids, "confirm": true, "dontAsk": true }));
    run("photo.undoSave", json!({ "ids": [ids[0]] }));
    run("photo.revert", json!({ "ids": [ids[1]], "confirm": true }));
    drop(ui);
    assert_eq!(lib.edits(ids[0]), [AUTOFIX]);
    assert!(lib.edits(ids[1]).is_empty());
}

#[test]
fn each_command_states_its_contract() {
    let lib = Lib::new(1, None);
    for (id, label, menu) in [("photo.save", "Save", "File/Save"), ("photo.undoSave", "Undo Save", ""), ("photo.revert", "Revert…", "File/Revert…")]
    {
        let spec = lib.s.spec(id).unwrap();
        assert_eq!((spec.label, spec.menu, spec.effect, spec.history), (label, menu, Effect::Files, History::None), "{id}");
    }
    let keys: Vec<String> = lib.s.spec("photo.save").unwrap().keys.iter().map(|(_, k)| k.to_string()).collect();
    assert_eq!(keys, ["Ctrl+S", "Ctrl+S"]);
}

/// Unknown photos, wrong types and nothing to do are errors that change nothing.
#[test]
fn bad_parameters_and_nothing_to_do_fail_gracefully() {
    let mut lib = Lib::new(2, None);
    let (a, b) = (lib.ids[0], lib.ids[1]);
    lib.edit(a, AUTOFIX);
    let model = lib.s.library().model().clone();
    let bad = |r: Result<Value, CmdError>| matches!(r, Err(CmdError::BadParams(_)));
    for cmd in ["photo.save", "photo.undoSave", "photo.revert"] {
        let s = &mut lib.s;
        assert!(matches!(s.execute(cmd, &json!({ "ids": [999] })), Err(CmdError::NotFound(_))), "{cmd}");
        assert!(bad(s.execute(cmd, &json!({}))), "{cmd}: ids are required");
        assert!(bad(s.execute(cmd, &json!({ "ids": "x" }))), "{cmd}");
        // An unknown key: refused, as disabled first when the photo has nothing to do.
        assert!(matches!(s.execute(cmd, &json!({ "ids": [a], "zzz": 1 })), Err(CmdError::BadParams(_) | CmdError::Disabled(_))), "{cmd}");
        assert!(bad(s.execute(cmd, &json!([a]))), "{cmd}");
        assert!(matches!(s.execute(cmd, &json!({ "ids": [] })), Err(CmdError::Disabled(_))), "{cmd}");
    }
    let s = &mut lib.s;
    assert!(bad(s.execute("photo.save", &json!({ "ids": [a], "zzz": 1 }))));
    assert!(bad(s.execute("photo.save", &json!({ "ids": [a], "confirm": "yes" }))));
    assert!(bad(s.execute("photo.save", &json!({ "ids": [a], "dontAsk": 1 }))));
    assert_eq!(s.execute("photo.save", &json!({ "ids": [b], "confirm": true })), Err(CmdError::Disabled(NOTHING_TO_SAVE.into())));
    assert_eq!(s.execute("photo.undoSave", &json!({ "ids": [a, b] })), Err(CmdError::Disabled(NOTHING_SAVED.into())));
    assert_eq!(s.execute("photo.revert", &json!({ "ids": [a], "confirm": true })), Err(CmdError::Disabled(NOTHING_SAVED.into())));
    let too_many: Vec<u64> = (0..=save_cmd::MAX_IDS as u64).collect();
    assert!(matches!(s.execute("photo.save", &json!({ "ids": too_many, "confirm": true })), Err(CmdError::Limit(_) | CmdError::NotFound(_))));
    assert!(matches!(s.execute("photo.undoSave", &json!({ "ids": [a], "confirm": true })), Err(CmdError::Disabled(_) | CmdError::BadParams(_))));
    assert_eq!(s.library().model(), &model);
    assert!(!s.file_jobs_running() && s.undo_stack().is_empty());

    // Once saved: a Revert action Dalica does not know, and a confirm Undo Save does not take.
    lib.save(&[a]);
    let s = &mut lib.s;
    let model = s.library().model().clone();
    assert!(bad(s.execute("photo.revert", &json!({ "ids": [a], "confirm": true, "action": "delete" }))));
    assert!(bad(s.execute("photo.revert", &json!({ "ids": [a], "confirm": true, "action": 3 }))));
    assert!(bad(s.execute("photo.revert", &json!({ "ids": [a], "confirm": 1 }))));
    assert!(bad(s.execute("photo.undoSave", &json!({ "ids": [a], "confirm": true }))));
    assert_eq!(s.library().model(), &model);
    assert!(!s.file_jobs_running());
}

/// A recipe holding an operation Dalica cannot render is refused by its name: alone, the command
/// is disabled; beside another photo, that photo fails with the reason and the other is saved.
#[test]
fn an_operation_that_cannot_be_rendered_is_refused_by_name() {
    let sidecar = json!({
        "dalica": { "version": 1 },
        "photos": { "IMG_0002.jpg": { "edits": [AUTOFIX, "legacy:redeye(1)"] } },
    });
    let mut lib = Lib::new(2, Some(sidecar));
    let (a, b) = (lib.ids[0], lib.ids[1]);
    assert_eq!(lib.edits(b), [AUTOFIX, "legacy:redeye(1)"]);
    let before = read(&lib.path(b));
    let alone = lib.s.execute("photo.save", &json!({ "ids": [b], "confirm": true }));
    assert!(matches!(&alone, Err(CmdError::Disabled(m)) if m.contains("legacy:redeye(1)") && m.starts_with("The edit")), "{alone:?}");
    lib.edit(a, AUTOFIX);
    let done = lib.save(&[a, b]);
    assert_eq!(done.label, "Saved 1 of 2 photos; 1 failed");
    let failed = done.result.unwrap()["failed"].clone();
    assert_eq!(failed[0]["file"], json!(lib.path(b)));
    assert!(failed[0]["reason"].as_str().unwrap().contains("legacy:redeye(1)"), "{failed}");
    assert_eq!(read(&lib.path(b)), before, "untouched");
    assert!(!lib.lisbon().join(".dalica-originals").join("IMG_0002.jpg").exists());
    assert_eq!(lib.edits(b), [AUTOFIX, "legacy:redeye(1)"], "its edits stay");
    assert!(lib.s.library().saved(a).is_some());
}

/// A read-only file fails with its reason, nothing changed; then Undo Save of a saved photo made
/// read-only fails the same way.
#[test]
fn a_read_only_file_fails_with_its_reason() {
    let mut lib = Lib::new(2, None);
    let (a, b) = (lib.ids[0], lib.ids[1]);
    lib.edit(a, AUTOFIX);
    lib.edit(b, AUTOFIX);
    lib.save(&[b]);
    let set_read_only = |path: &Path, on: bool| {
        let mut p = std::fs::metadata(path).unwrap().permissions();
        p.set_readonly(on);
        std::fs::set_permissions(path, p).unwrap();
    };
    let (pa, pb) = (lib.path(a), lib.path(b));
    set_read_only(&pa, true);
    set_read_only(&pb, true);
    let before = (read(&pa), read(&pb));
    let done = lib.save(&[a]);
    let failed = done.result.unwrap()["failed"].clone();
    assert!(failed[0]["reason"].as_str().unwrap().contains("read-only"), "{failed}");
    let done = lib.job("photo.undoSave", json!({ "ids": [b] }));
    let failed = done.result.unwrap()["failed"].clone();
    assert!(failed[0]["reason"].as_str().unwrap().contains("read-only"), "{failed}");
    assert_eq!((read(&pa), read(&pb)), before, "nothing changed");
    assert_eq!(lib.edits(a), [AUTOFIX]);
    assert!(lib.s.library().can_undo_save(b));
    set_read_only(&pa, false);
    set_read_only(&pb, false);
}

/// A folder whose drive is offline: the commands are disabled with the reason, and nothing is
/// read or written.
#[test]
fn an_offline_folder_disables_the_commands() {
    let mut lib = Lib::new(2, None);
    let (a, b) = (lib.ids[0], lib.ids[1]);
    lib.edit(a, AUTOFIX);
    lib.edit(b, AUTOFIX);
    lib.save(&[b]);
    let check: OnlineCheck = Arc::new(|_: &Path| false);
    lib.s.set_online_check(Some(check));
    assert!(lib.s.refresh_volumes(PATIENCE));
    assert!(lib.s.is_folder_offline(lib.folder()));
    assert_eq!(lib.s.execute("photo.save", &json!({ "ids": [a], "confirm": true })), Err(CmdError::Disabled(OFFLINE.into())));
    assert_eq!(lib.s.execute("photo.undoSave", &json!({ "ids": [b] })), Err(CmdError::Disabled(OFFLINE.into())));
    assert_eq!(lib.s.execute("photo.revert", &json!({ "ids": [b], "confirm": true })), Err(CmdError::Disabled(OFFLINE.into())));
    assert!(!lib.s.file_jobs_running());
}

/// A format Dalica cannot write in place (a PNG with transparency) gets a JPEG beside it through
/// `photo.save`: the copy joins the library as a new, unedited photo with the source's caption,
/// tags and star; the source is untouched, keeps its recipe and no longer counts as unsaved.
#[test]
fn a_png_with_transparency_gets_a_jpeg_beside_it_as_a_new_photo() {
    let mut lib = Lib::new(1, None);
    let rgb = dalica_scenes::Scene::new(41, 96, 64).render().unwrap();
    let rgba: Vec<u8> = rgb.data.chunks(3).enumerate().flat_map(|(i, p)| [p[0], p[1], p[2], ((i as u32 % 96) * 255 / 95) as u8]).collect();
    let png = dalica_scenes::fingerprint::png_rgba(96, 64, &rgba);
    let path = lib.lisbon().join("alpha.png");
    dalica_fs::replace(&path, &png, Expect::Absent).unwrap();
    lib.s.start_verification();
    lib.s.wait_idle();
    let id = lib.s.library().photos().find(|p| p.name == "alpha.png").unwrap().id;
    lib.edit(id, AUTOFIX);
    lib.s.execute("photo.caption.set", &json!({ "ids": [id], "caption": "Tiles" })).unwrap();
    lib.s.execute("photo.tags.add", &json!({ "ids": [id], "tags": ["azulejo"] })).unwrap();
    lib.s.execute("photo.star.set", &json!({ "ids": [id], "on": true })).unwrap();
    let done = lib.save(&[id]);
    assert_eq!((done.state, done.label.as_str()), (JobState::Done, "Saved 1 photo"), "{:?}", done.result);
    assert_eq!(read(&path), png, "the source is untouched");
    assert!(!lib.lisbon().join(".dalica-originals").join("alpha.png").exists());
    let copy = lib.s.library().photos().find(|p| p.name == "alpha.jpg").unwrap().clone();
    assert!(copy.star && (copy.width, copy.height) == (96, 64));
    let info = lib.s.library().info(copy.id);
    assert_eq!((info.caption.as_str(), info.tags.clone()), ("Tiles", vec!["azulejo".to_string()]));
    assert!(info.edits.is_empty(), "the copy is an unedited photo");
    assert_eq!(lib.s.library().saved_as(id).map(|c| (c.name.clone(), c.recipe.clone())), Some(("alpha.jpg".into(), vec![AUTOFIX.into()])));
    assert_eq!(lib.edits(id), [AUTOFIX], "the source keeps its recipe");
    assert!(!lib.s.library().has_unsaved_edits(id) && lib.s.library().saved(id).is_none());
    // Nothing to undo: Undo Save applies to saves in place only.
    assert_eq!(lib.s.execute("photo.undoSave", &json!({ "ids": [id] })), Err(CmdError::Disabled(NOTHING_SAVED.into())));
}

/// A rotate-only Save of a JPEG through `photo.save` turns the photo (lossless, M8.14), and Undo
/// Save gives the exact original back.
#[test]
fn a_rotate_only_save_turns_the_photo_and_undo_save_brings_the_original_back() {
    let mut lib = Lib::new(1, None);
    let a = lib.ids[0];
    let original = read(&lib.path(a));
    lib.edit(a, "rotate(90)");
    assert_eq!(lib.save(&[a]).state, JobState::Done);
    let photo = lib.s.library().photo(a).unwrap();
    assert_eq!((photo.width, photo.height), (H, W), "the photo is turned");
    let saved = decode(&read(&lib.path(a)));
    assert_eq!((saved.width, saved.height), (H, W));
    assert!(lib.edits(a).is_empty());
    lib.job("photo.undoSave", json!({ "ids": [a] }));
    assert_eq!(read(&lib.path(a)), original);
    assert_eq!(lib.edits(a), ["rotate(90)"]);
}
