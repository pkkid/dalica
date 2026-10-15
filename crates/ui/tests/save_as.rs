//! Save As and Save a Copy in the app (M8.22, `data-model.md` §6, `menus-and-shortcuts.md` §1),
//! driven headlessly with the scripted picker.
//!
//! - File ▸ Save As… asks the picker with the photo's name (numbered when the library knows that
//!   name in the folder), the folder and the format Save As last wrote; the path chosen runs
//!   `photo.saveTo`, the file is written and the new photo appears, in a folder of the library or in
//!   an unwatched one (added as Scan Always).
//! - A taken name is never replaced: the picker opens again ("Choose Another Name"), and the next
//!   name chosen is written; a folder that is gone and a name that is not JPEG or PNG ask again too.
//! - Cancel writes nothing.
//! - File ▸ Save a Copy writes `<stem>-001.jpg` from the edit view (the open photo) and from the
//!   library (the selection).
//! - The menu rows' enabled states, in the library and the edit view.
//! - After a Save in the edit view, Edit ▸ Undo reads "Undo Save" and runs it.
//!
//! The flow is captured at 1400 × 900 and 900 × 600 into `target/test-output/ui/save-as-*.png`
//! (no fingerprints: the native picker is not drawn by the app).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, new_driver, out_dir};
use dalica_core::PhotoId;
use dalica_engine::{JobKind, Session};
use dalica_fs::Expect;
use dalica_scenes::fingerprint;
use dalica_ui::commands::save_as::{self, ANOTHER_NAME, NO_EDITS, ONE_PHOTO, TITLE};
use dalica_ui::menus::{self, ItemNode, Lookup, Platform};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(300);
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

struct T {
    dir: tempfile::TempDir,
    d: Driver,
    picker: ScriptedPicker,
    /// The photos of `Lisbon`, by name.
    ids: Vec<PhotoId>,
}

impl T {
    /// A library watching `Photos` (its `Lisbon` folder holds `n` photos `IMG_0001.jpg` …), with
    /// `Pictures` as the Pictures folder, in a `size` window; the first photo is rotated (an edit).
    fn new(n: usize, size: (u32, u32)) -> T {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Photos").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 240, 160), 85, None).unwrap();
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
        }
        let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        let ids: Vec<PhotoId> = photos.into_iter().map(|(_, id)| id).collect();
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
        let picker = ScriptedPicker::new();
        let mut d = new_driver(App::with_picker(Some(s), Arc::new(picker.clone())), size.0, size.1, 1.0);
        d.settle(FRAMES).unwrap();
        T { dir, d, picker, ids }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Photos").join("Lisbon")
    }

    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    /// Steps frames (letting the workers run between them) until `done` holds.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Driver) -> bool) {
        let start = Instant::now();
        while !done(&self.d) {
            assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
            self.d.step();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Until no picker is open or waiting to open again, no Save As job is followed and the session
    /// is idle.
    fn until_done(&mut self) {
        self.until("Save As to finish", |d| {
            let app = d.app();
            app.save_dialog().is_none()
                && app.save_as().job().is_none()
                && !app.save_as().asking_again()
                && !app.session().unwrap().file_jobs_running()
                && !app.session().unwrap().busy()
        });
    }

    /// The menu row at `path` as the bar shows it now.
    fn row(&self, path: &[&str]) -> ItemNode {
        let bar = menus::bar(self.d.app(), Platform::Other);
        menus::find(&bar, &Lookup::Path(path.iter().map(|s| s.to_string()).collect())).cloned().unwrap_or_else(|| panic!("no row {path:?}"))
    }

    /// Clicks the menu row at `path`, as the menu does.
    fn click(&mut self, path: &[&str]) -> Value {
        let row = self.row(path);
        let r = menus::activate(self.d.app_mut(), &row, &Value::Null).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        self.d.step();
        r
    }

    /// The library's photo named `name` in `folder`, if any.
    fn photo_named(&self, folder: &Path, name: &str) -> Option<PhotoId> {
        let lib = self.session().library();
        lib.photo_by_name(lib.folder_by_path(folder)?, name).map(|p| p.id)
    }

    fn select(&mut self, ids: &[PhotoId]) {
        self.d.run("select.set", &json!({ "ids": ids })).unwrap();
    }

    fn toasts(&self) -> Vec<String> {
        let now = self.d.ctx().input(|i| i.time);
        let list = self.d.app().notices().inspect(now);
        list.as_array().unwrap().iter().map(|t| t["text"].as_str().unwrap().to_string()).collect()
    }
}

/// The files in `dir` but the folder's own (`.dalica.json`), sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    names
}

fn decoded_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    let img = dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap();
    (img.width, img.height)
}

/// Writes the last frame to `target/test-output/ui/<name>.png`.
fn save_window(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// Save As to a new name in the photo's folder writes it (the edits applied) and the new photo
/// appears; the photo and its edits are untouched. Save As into a folder outside the library in
/// PNG adds that folder and is remembered: the next picker opens there, suggesting a PNG.
#[test]
fn save_as_to_a_new_path_writes_it_and_the_photo_appears() {
    let mut t = T::new(2, (1400, 900));
    let (photo, lisbon) = (t.ids[0], t.lisbon());
    let source = std::fs::read(lisbon.join("IMG_0001.jpg")).unwrap();
    t.select(&[photo]);

    // The picker suggests the photo's name, numbered since the library knows that name there, in
    // its folder, as a JPEG.
    let first = save_as::request(t.d.app(), photo).unwrap();
    assert_eq!((first.title, first.name.as_str(), first.folder.as_deref()), (TITLE, "IMG_0001-001.jpg", Some(lisbon.as_path())));
    assert_eq!(first.filters.first().map(|f| f.name), Some("JPEG"));

    t.picker.push_save(lisbon.join("Lisbon Bridge.jpg"));
    assert_eq!(t.click(&["File", "Save As…"]), json!({ "dialog": "filePicker" }));
    t.until_done();
    assert_eq!(t.picker.last_save(), Some(first.clone()), "the picker was asked what request() offers");
    let written = lisbon.join("Lisbon Bridge.jpg");
    assert_eq!(decoded_size(&written), (160, 240), "the rotation is applied");
    let copy = t.photo_named(&lisbon, "Lisbon Bridge.jpg").expect("the new photo is in the library");
    assert!(t.session().library().info(copy).edits.is_empty(), "the copy has no edits");
    assert_eq!(std::fs::read(lisbon.join("IMG_0001.jpg")).unwrap(), source, "the original is untouched");
    assert_eq!(t.session().library().info(photo).edits, ["rotate(90)"], "its edits are kept");

    // Into a folder the library does not watch, as a PNG.
    let elsewhere = t.dir.path().join("Elsewhere");
    dalica_fs::create_dir_all(&elsewhere).unwrap();
    t.picker.push_save(elsewhere.join("bridge.png"));
    t.click(&["File", "Save As…"]);
    t.until_done();
    let png = std::fs::read(elsewhere.join("bridge.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG"), "written as a PNG");
    t.until("the folder's scan", |d| !d.app().session().unwrap().busy());
    assert!(t.session().roots().iter().any(|r| r == &elsewhere), "the folder is added: {:?}", t.session().roots());
    assert!(t.photo_named(&elsewhere, "bridge.png").is_some(), "the PNG appears");

    // The next Save As starts where this one wrote, in its format.
    let next = save_as::request(t.d.app(), photo).unwrap();
    assert_eq!((next.name.as_str(), next.folder.as_deref()), ("IMG_0001.png", Some(elsewhere.as_path())));
    assert_eq!(next.filters.first().map(|f| f.name), Some("PNG"));
}

/// A name that is taken is refused (the file is left as it was) and the picker opens again with
/// "Choose Another Name"; the next name chosen is written. A folder that is gone and a name that
/// is not JPEG or PNG open the picker again too; cancelling it then writes nothing.
#[test]
fn a_taken_name_is_refused_and_the_picker_returns() {
    for size in SIZES {
        let mut t = T::new(2, size);
        let (photo, lisbon) = (t.ids[0], t.lisbon());
        t.select(&[photo]);
        let taken = lisbon.join("IMG_0002.jpg");
        let before = std::fs::read(&taken).unwrap();

        // The taken name, then a free one.
        t.picker.push_save(&taken);
        t.picker.push_save(lisbon.join("Second Try.jpg"));
        t.click(&["File", "Save As…"]);
        t.until("the second answer", |d| d.app().session().unwrap().library().photos().any(|p| p.name == "Second Try.jpg"));
        t.until_done();
        assert_eq!(std::fs::read(&taken).unwrap(), before, "never replaced");
        let asked = t.picker.last_save().unwrap();
        assert_eq!((asked.title, asked.name.as_str(), asked.folder.as_deref()), (ANOTHER_NAME, "IMG_0002.jpg", Some(lisbon.as_path())));
        assert_eq!(asked.params, json!({ "id": photo }));
        assert!(lisbon.join("Second Try.jpg").is_file());
        assert!(t.toasts().iter().any(|m| m.contains("already exists")), "the toast says why: {:?}", t.toasts());
        let taken_toast = t.toasts().into_iter().find(|m| m.contains("already exists")).unwrap();
        assert!(taken_toast.contains("IMG_0002.jpg") && !taken_toast.contains("IMG_0001.jpg"), "it names the file tried: {taken_toast}");

        // The taken name again, then cancel: the status line keeps the reason, nothing is written.
        t.picker.push_save(&taken);
        t.click(&["File", "Save As…"]);
        t.until_done();
        assert_eq!(t.picker.last_save().unwrap().title, ANOTHER_NAME);
        assert!(t.d.state().status.contains("already exists"), "{}", t.d.state().status);
        assert_eq!(std::fs::read(&taken).unwrap(), before);
        t.until("the new photo's thumbnail", |d| d.app().idle());
        save_window(&t.d, &format!("save-as-another-name-{}", size.0));

        // A folder that is gone: asked again in the photo's folder, with the name tried.
        let gone = t.dir.path().join("Gone").join("x.jpg");
        t.picker.push_save(&gone);
        t.click(&["File", "Save As…"]);
        t.until_done();
        let asked = t.picker.last_save().unwrap();
        assert_eq!((asked.title, asked.name.as_str(), asked.folder.as_deref()), (TITLE, "x.jpg", Some(lisbon.as_path())));
        assert!(!t.dir.path().join("Gone").exists());

        // A TIFF name is refused by the command: asked again where it was tried.
        t.picker.push_save(lisbon.join("a.tif"));
        t.click(&["File", "Save As…"]);
        t.until_done();
        let asked = t.picker.last_save().unwrap();
        assert_eq!((asked.title, asked.name.as_str(), asked.folder.as_deref()), (TITLE, "a.tif", Some(lisbon.as_path())));
        assert_eq!(files(&lisbon), ["IMG_0001.jpg", "IMG_0002.jpg", "Second Try.jpg"]);
    }
}

/// Cancelling the picker writes nothing and starts no job.
#[test]
fn cancel_writes_nothing() {
    let mut t = T::new(2, (1400, 900));
    t.select(&[t.ids[0]]);
    let before = files(&t.lisbon());
    t.click(&["File", "Save As…"]);
    t.until_done();
    assert!(t.picker.last_save().is_some(), "the picker was asked");
    assert_eq!(files(&t.lisbon()), before);
    assert!(t.session().jobs().iter().all(|j| j.kind != JobKind::Save), "no save job ran");
    assert!(t.d.state().status.is_empty(), "{}", t.d.state().status);
}

/// File ▸ Save a Copy writes `<stem>-001.jpg` beside the open photo in the edit view, and beside
/// each selected photo with edits in the library.
#[test]
fn save_a_copy_writes_numbered_copies_from_the_edit_view_and_the_library() {
    let mut t = T::new(3, (1400, 900));
    let lisbon = t.lisbon();
    t.d.run("view.edit", &json!({ "id": t.ids[0] })).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.click(&["File", "Save a Copy"]);
    t.until_done();
    assert_eq!(decoded_size(&lisbon.join("IMG_0001-001.jpg")), (160, 240));
    assert!(t.photo_named(&lisbon, "IMG_0001-001.jpg").is_some(), "the copy appears");

    t.d.run("view.library", &json!({})).unwrap();
    t.d.run("photo.history.apply", &json!({ "id": t.ids[2], "op": "flip_h()" })).unwrap();
    t.select(&[t.ids[0], t.ids[1], t.ids[2]]);
    t.click(&["File", "Save a Copy"]);
    t.until_done();
    assert_eq!(files(&lisbon), ["IMG_0001-001.jpg", "IMG_0001-002.jpg", "IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003-001.jpg", "IMG_0003.jpg"]);
}

/// The rows light up for one photo with edits (Save As) and photos with edits (Save a Copy), in the
/// library and the edit view; otherwise they are greyed with the reason.
#[test]
fn the_menu_rows_follow_the_photos_and_their_edits() {
    for size in SIZES {
        let mut t = T::new(2, size);
        let state = |t: &T| {
            let (a, c) = (t.row(&["File", "Save As…"]), t.row(&["File", "Save a Copy"]));
            ((a.enabled, a.reason), (c.enabled, c.reason))
        };
        // Nothing selected.
        let ((a, why), (c, _)) = state(&t);
        assert!(!a && !c);
        assert_eq!(why.as_deref(), Some(ONE_PHOTO));
        // A photo without edits.
        t.select(&[t.ids[1]]);
        let ((a, why), (c, copy_why)) = state(&t);
        assert!(!a && !c);
        assert_eq!(why.as_deref(), Some(NO_EDITS));
        assert_eq!(copy_why.as_deref(), Some(dalica_engine::cmd::save_copy::NO_EDITS));
        // Two photos, one with edits: Save a Copy only.
        t.select(&[t.ids[0], t.ids[1]]);
        let ((a, _), (c, _)) = state(&t);
        assert!(!a && c);
        // One photo with edits.
        t.select(&[t.ids[0]]);
        let ((a, _), (c, _)) = state(&t);
        assert!(a && c);
        // Captured with the File menu open.
        t.d.click("menu:File").unwrap();
        t.d.settle(FRAMES).unwrap();
        save_window(&t.d, &format!("save-as-file-menu-{}", size.0));
        t.d.key("Escape").unwrap();
        t.d.settle(FRAMES).unwrap();
        // While the picker is open (until the next frame sees its answer) Save As waits for it.
        let row = t.row(&["File", "Save As…"]);
        menus::activate(t.d.app_mut(), &row, &Value::Null).unwrap();
        let open = t.row(&["File", "Save As…"]);
        assert_eq!((open.enabled, open.reason.as_deref()), (false, Some("A file dialog is already open.")), "one dialog at a time");
        t.until_done();
        // The edit view: the open photo decides.
        t.d.run("view.edit", &json!({ "id": t.ids[0] })).unwrap();
        t.d.settle(FRAMES).unwrap();
        let ((a, _), (c, _)) = state(&t);
        assert!(a && c, "the open photo has edits");
        t.d.run("edit.next", &json!({})).unwrap();
        t.d.settle(FRAMES).unwrap();
        assert_eq!(t.d.state().edit.photo, Some(t.ids[1]));
        let ((a, why), (c, _)) = state(&t);
        assert!(!a && !c, "the open photo has none");
        assert_eq!(why.as_deref(), Some(NO_EDITS));
    }
}

/// After a Save in the edit view, Edit ▸ Undo reads "Undo Save" and is live (`photo.history.state`
/// answers `undoSave`); it runs Undo Save, which brings the edits back as unsaved, and the row
/// reads "Undo Rotate" again.
#[test]
fn edit_undo_reads_undo_save_after_a_save_in_the_edit_view() {
    let mut t = T::new(1, (1400, 900));
    let photo = t.ids[0];
    t.d.run("view.edit", &json!({ "id": photo })).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.row(&["Edit", "Undo"]).label, "Undo Rotate");
    t.d.run("photo.save", &json!({ "confirm": true })).unwrap();
    t.until_done();
    assert!(t.session().library().info(photo).edits.is_empty(), "saved");
    let state = t.d.run("photo.history.state", &json!({ "id": photo })).unwrap();
    assert_eq!(state["undoSave"], json!(true));

    let undo = t.row(&["Edit", "Undo Save"]);
    assert_eq!((undo.label.as_str(), undo.enabled, undo.widget.as_str()), ("Undo Save", true, "menuitem:edit.undo"));
    let bar = menus::bar(t.d.app(), Platform::Other);
    assert_eq!(menus::find(&bar, &Lookup::Id { id: "edit.undo".into(), params: Value::Null }).map(|i| i.label.as_str()), Some("Undo Save"));
    t.click(&["Edit", "Undo Save"]);
    t.until_done();
    assert_eq!(t.session().library().info(photo).edits, ["rotate(90)"], "the edits are back, unsaved");
    assert_eq!(t.row(&["Edit", "Undo"]).label, "Undo Rotate");
    // In the library the row walks the library's stack, as before.
    t.d.run("view.library", &json!({})).unwrap();
    assert_ne!(t.row(&["Edit", "Undo"]).label, "Undo Save");
}

/// Over automation the picker is the in-app stub (`control-protocol.md` §5.6): its answer runs
/// `photo.saveTo` before the dialog's thread sees a cancel. A taken name still brings the stub back,
/// titled "Choose Another Name"; `photo.saveTo` run by a script without the picker never opens one.
#[test]
fn the_stub_picker_asks_again_for_a_taken_name() {
    use dalica_ui::control::StubPicker;
    let mut t = T::new(2, (900, 600));
    let stub = Arc::new(StubPicker::new());
    let session = t.d.app_mut().take_session();
    *t.d.app_mut() = App::with_picker(session, stub.clone());
    t.d.settle(FRAMES).unwrap();
    let (photo, lisbon) = (t.ids[0], t.lisbon());
    let taken = lisbon.join("IMG_0002.jpg");

    // A script's own photo.saveTo to a taken name: the job fails, no picker opens.
    let job = t.d.run("photo.saveTo", &json!({ "id": photo, "path": taken })).unwrap()["job"].as_u64().unwrap();
    t.until("the script's job", |d| d.app().session().unwrap().job(job).is_some_and(|j| j.state.is_finished()));
    t.until_done();
    assert!(stub.waiting().is_none() && t.d.app().save_dialog().is_none(), "no picker for a script");

    // Through the picker: the stub waits, its answer runs photo.saveTo as ui.filePicker does.
    t.d.run("photo.saveAs", &json!({ "id": photo })).unwrap();
    t.until("the stub", |_| stub.waiting().is_some());
    let waiting = stub.waiting().unwrap();
    assert_eq!((waiting.command, waiting.title), ("photo.saveTo", TITLE));
    stub.answer(None);
    t.d.app_mut().run(waiting.command, &json!({ "id": photo, "path": taken })).unwrap();
    t.until("the stub again", |_| stub.waiting().is_some());
    let again = stub.waiting().unwrap();
    assert_eq!((again.title, again.name.as_deref(), again.folder.as_deref()), (ANOTHER_NAME, Some("IMG_0002.jpg"), Some(lisbon.as_path())));
    stub.answer(None);
    t.until_done();
    assert!(stub.waiting().is_none());
}
