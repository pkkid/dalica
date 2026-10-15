//! Save, Undo Save and Revert in the app (`roadmap.md` M8.19; `ui-layout.md` §5.2,
//! `data-model.md` §6, `editing.md` §5.1), driven headlessly by widget id on a temporary library:
//!
//! - Two edited photos: the section header shows "2 edited" and the Save text button; a click asks
//!   the first-time explanation once, Save saves both, and the count, the Save button and the
//!   edited badges go. With "Don't show this again" ticked, the next Save asks nothing; the disk
//!   button and Ctrl+S on the selection save without asking.
//! - In the edit view Ctrl+S saves the open photo; the Undo button then reads "Undo Save", and a
//!   click brings the edits back as unsaved edits and the original's bytes back.
//! - The Revert question: Cancel changes nothing, Undo Save undoes the save and keeps the edits,
//!   Revert brings the original back and drops the edits; Enter reverts nothing.
//!
//! Fingerprints at 1400 × 900 (at 900 × 600 they only have to fit): the header with its count
//! (`region-header-edited-1400x900.json`), the explanation (`dialog-save-explain-1400x900.json`),
//! the Revert question (`dialog-revert-1400x900.json`) and the edit panel's Undo Save
//! (`edit-undo-save-1400x900.json`). Renders go to `target/test-output/ui/`; `DALICA_BLESS=1`
//! stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_rect, check_small_and_back, new_driver};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_ui::dialogs::save_explain::DONT_SHOW_ID;
use dalica_ui::panels::edit::history::{UNDO_ID, UNDO_SAVE};
use dalica_ui::panels::light_table::header::edited_text;
use dalica_ui::state::{SaveExplain, Section, View};
use dalica_ui::{App, Driver, DriverError};
use dalica_ui_kit::WidgetInfo;
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(300);
const AUTOFIX: &str = "autofix()";
const TUNING: &str = "tuning(0.25,0,0,0,none)";

struct T {
    dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<PhotoId>,
    folder: FolderId,
}

impl T {
    /// A library watching `Pictures/Lisbon` with `n` JPEGs `IMG_0001.jpg`… (ids in name order), the
    /// folder dated June 4, 2026, in a `size` window.
    fn new(n: usize, (w, h): (u32, u32)) -> T {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..n {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i as u64, 320, 240), 85, None).unwrap();
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
        }
        let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
        s.wait_idle();
        let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        photos.sort();
        let folder = s.library().folder_by_path(&lisbon).unwrap();
        // A fixed date, so the header's fingerprint does not follow the files' times.
        s.execute("folder.info.set", &json!({ "id": folder, "date": "2026-06-04", "place": "Lisbon, Portugal" })).unwrap();
        let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
        d.settle(FRAMES).unwrap();
        T { dir, d, ids: photos.into_iter().map(|(_, id)| id).collect(), folder }
    }

    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    fn path(&self, id: PhotoId) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon").join(&self.session().library().photo(id).unwrap().name)
    }

    fn bytes(&self, id: PhotoId) -> Vec<u8> {
        std::fs::read(self.path(id)).unwrap()
    }

    fn edit(&mut self, id: PhotoId, op: &str) {
        self.d.run("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    fn edits(&self, id: PhotoId) -> Vec<String> {
        self.session().library().info(id).edits.clone()
    }

    fn unsaved(&self, id: PhotoId) -> bool {
        self.session().library().has_unsaved_edits(id)
    }

    fn saved(&self, id: PhotoId) -> bool {
        self.session().library().can_undo_save(id)
    }

    /// Steps frames (letting the workers run between them) until `done` holds and no file job runs,
    /// then settles.
    fn until(&mut self, what: &str, mut done: impl FnMut(&T) -> bool) {
        let start = Instant::now();
        while !(done(self) && !self.session().file_jobs_running()) {
            if start.elapsed() > PATIENCE {
                let jobs = self.session().jobs();
                panic!("timed out waiting for {what}: status {:?} jobs {jobs:?} dialogs {:?}", self.d.state().status, self.d.state().dialogs);
            }
            self.d.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        self.d.settle(FRAMES).unwrap();
    }

    fn part(&self, part: &str) -> String {
        format!("section:{}/{part}", Section::Folder(self.folder))
    }

    fn widget(&self, id: &str) -> Option<WidgetInfo> {
        self.d.widgets().into_iter().find(|w| w.id == id)
    }

    fn click(&mut self, id: &str) {
        self.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        self.d.settle(FRAMES).unwrap();
    }

    /// The header's "n edited" as last drawn (0: none).
    fn edited(&self) -> usize {
        self.d.app().panels().unwrap().light_table.headers.edited(Section::Folder(self.folder)).unwrap_or(0)
    }

    fn badge(&self, id: PhotoId) -> bool {
        self.d.app().panels().unwrap().light_table.cells.get(id).unwrap().badges.edited
    }

    fn dialog(&self) -> Option<String> {
        self.d.state().dialogs.last().map(|d| d.kind.name().to_string())
    }
}

/// Checks the region `rect` (points, at scale 1) of the last frame against `<name>.json`, and
/// writes it and the whole window to the test output.
fn check_region(d: &Driver, rect: [f32; 4], name: &str) {
    check_rect(d, name, rect);
}

fn exists(p: &Path) -> bool {
    p.try_exists().unwrap()
}

/// The section header's count and Save, the explanation asked once, saved, and asked no more.
#[test]
fn the_header_saves_the_edited_photos_and_the_explanation_is_asked_until_dont_show() {
    let (w, h) = LARGE;
    let case = format!("{w}×{h}");
    let mut t = T::new(4, (w, h));
    let [a, b, c, e] = [t.ids[0], t.ids[1], t.ids[2], t.ids[3]];
    assert!(t.widget(&t.part("saveEdited")).is_none(), "{case}: no unsaved edits, no Save at the right");
    assert!(!t.widget(&t.part("save")).unwrap().enabled, "{case}: the disk button waits for an edit");
    let originals = [t.bytes(a), t.bytes(b)];

    // Two edited photos: "2 edited" and Save, the disk button live, the badges on.
    t.edit(a, AUTOFIX);
    t.edit(b, TUNING);
    assert_eq!((t.edited(), edited_text(2).as_str()), (2, "2 edited"), "{case}");
    let save = t.widget(&t.part("saveEdited")).expect("the Save text button");
    assert!(save.enabled, "{case}");
    assert_eq!(save.label, "Save");
    assert!(t.widget(&t.part("save")).unwrap().enabled, "{case}: the disk button too");
    assert!(t.badge(a) && t.badge(b) && !t.badge(c), "{case}");
    let block = t.widget(&format!("section:{}", Section::Folder(t.folder))).unwrap();
    check_region(&t.d, block.rect, &format!("region-header-edited-{w}x{h}"));

    // A click asks once: the explanation, its checkbox unticked.
    t.click(&t.part("saveEdited"));
    assert_eq!(t.dialog().as_deref(), Some("saveExplain"), "{case}");
    assert_eq!(t.d.state().save_explain, Some(SaveExplain { dont_show: false }));
    let check = t.widget(DONT_SHOW_ID).expect("the checkbox");
    assert!(check.enabled && !check.active, "{case}");
    let dialog = t.widget("dialog:saveExplain").unwrap();
    check_region(&t.d, dialog.rect, &format!("dialog-save-explain-{w}x{h}"));
    check_small_and_back(&mut t.d, "dialog:saveExplain");
    assert!(!t.session().file_jobs_running() && t.bytes(a) == originals[0], "{case}: nothing saved before the answer");

    // Save: both saved, the count, the Save button and the badges go; nothing asks again.
    t.click("button:dialog.save");
    assert_eq!(t.dialog(), None, "{case}: asked once");
    t.until("both saved", |t| t.saved(a) && t.saved(b));
    assert!(!t.unsaved(a) && !t.unsaved(b), "{case}");
    assert_eq!(t.edited(), 0, "{case}");
    assert!(t.widget(&t.part("saveEdited")).is_none(), "{case}: the Save text button goes");
    assert!(!t.widget(&t.part("save")).unwrap().enabled, "{case}");
    assert!(!t.badge(a) && !t.badge(b), "{case}: the badges go");
    assert_ne!(t.bytes(a), originals[0], "{case}: the edits are in the file");
    let kept = t.dir.path().join("Pictures").join("Lisbon").join(".dalica-originals");
    assert_eq!(std::fs::read(kept.join("IMG_0001.jpg")).unwrap(), originals[0], "{case}: the original is kept");
    assert_eq!(std::fs::read(kept.join("IMG_0002.jpg")).unwrap(), originals[1], "{case}");
    assert!(t.session().settings().save().explain, "{case}: still on, nothing was ticked");

    // The next Save asks again; ticked, it saves and asks no more.
    t.edit(c, AUTOFIX);
    t.click(&t.part("saveEdited"));
    assert_eq!(t.dialog().as_deref(), Some("saveExplain"), "{case}");
    t.click(DONT_SHOW_ID);
    assert_eq!(t.d.state().save_explain, Some(SaveExplain { dont_show: true }), "{case}");
    assert!(t.widget(DONT_SHOW_ID).unwrap().active, "{case}: ticked");
    t.click("button:dialog.save");
    t.until("c saved", |t| t.saved(c));
    assert!(!t.session().settings().save().explain, "{case}: Don't show this again turned it off");

    // The disk button saves without asking.
    t.edit(a, TUNING);
    t.click(&t.part("save"));
    assert_eq!(t.dialog(), None, "{case}: no question any more");
    t.until("a saved again", |t| !t.unsaved(a));
    assert_eq!(std::fs::read(kept.join("IMG_0001.jpg")).unwrap(), originals[0], "{case}: the first original stays the one kept");

    // Ctrl+S in the library saves the selection, and only it.
    t.edit(e, AUTOFIX);
    t.edit(b, AUTOFIX);
    t.click(&format!("thumb:{e}"));
    assert_eq!(t.d.state().selection.ids, [e], "{case}");
    t.d.key("Ctrl+S").unwrap();
    t.until("e saved", |t| t.saved(e));
    assert!(t.unsaved(b), "{case}: b was not selected");
    assert_eq!(t.edited(), 1, "{case}");
}

/// Ctrl+S in the edit view saves the open photo; the Undo button reads "Undo Save" and brings the
/// edits back.
#[test]
fn ctrl_s_in_the_edit_view_saves_the_open_photo_and_undo_save_brings_the_edits_back() {
    let (w, h) = LARGE;
    let case = format!("{w}×{h}");
    let mut t = T::new(2, (w, h));
    let [a, b] = [t.ids[0], t.ids[1]];
    let original = t.bytes(a);
    t.edit(b, AUTOFIX);
    t.d.run("view.edit", &json!({ "id": a })).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.state().view, View::Edit);
    t.edit(a, AUTOFIX);
    assert_eq!(t.widget(UNDO_ID).unwrap().label, "Undo Auto Fix", "{case}");

    // Ctrl+S: the explanation (Enter is its Save), then the open photo alone is saved.
    t.d.key("Ctrl+S").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.dialog().as_deref(), Some("saveExplain"), "{case}");
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    t.until("a saved", |t| t.saved(a));
    assert!(t.unsaved(b), "{case}: only the open photo");
    assert_ne!(t.bytes(a), original, "{case}");

    // Undo reads "Undo Save"; a click puts the original back and the edits as unsaved edits.
    let undo = t.widget(UNDO_ID).unwrap();
    assert_eq!((undo.label.as_str(), undo.enabled), (UNDO_SAVE, true), "{case}");
    assert_eq!(t.d.run("photo.history.state", &json!({ "id": a })).unwrap()["undoLabel"], json!(UNDO_SAVE), "{case}: as the engine says");
    let panel = t.widget("button:edit.redo").unwrap();
    let both = [undo.rect[0], undo.rect[1], panel.rect[0] + panel.rect[2] - undo.rect[0], undo.rect[3]];
    check_region(&t.d, both, &format!("edit-undo-save-{w}x{h}"));
    t.click(UNDO_ID);
    t.until("the save undone", |t| !t.saved(a) && t.unsaved(a));
    assert_eq!(t.edits(a), [AUTOFIX], "{case}: the edits are back, unsaved");
    assert_eq!(t.bytes(a), original, "{case}: the original's bytes are back");
    assert_eq!(t.widget(UNDO_ID).unwrap().label, "Undo Auto Fix", "{case}");
}

/// The Revert question's three buttons: Cancel nothing, Undo Save keeps the edits, Revert drops
/// them; Enter reverts nothing.
#[test]
fn each_revert_button_does_its_action_and_cancel_nothing() {
    let (w, h) = LARGE;
    let case = format!("{w}×{h}");
    let mut t = T::new(2, (w, h));
    let a = t.ids[0];
    let original = t.bytes(a);
    let save = |t: &mut T| {
        t.d.run("photo.save", &json!({ "ids": [a], "confirm": true })).unwrap();
        t.until("saved", |t| t.saved(a));
    };
    // Never saved: Revert says why and asks nothing.
    assert!(matches!(t.d.run("photo.revert", &json!({ "ids": [a] })), Err(DriverError::Command(_))));
    assert_eq!(t.dialog(), None, "{case}");

    t.edit(a, AUTOFIX);
    save(&mut t);
    let saved = t.bytes(a);
    assert_eq!(t.d.run("photo.revert", &json!({ "ids": [a] })).unwrap(), json!({ "dialog": "revert" }));
    t.d.settle(FRAMES).unwrap();
    let dialog = t.widget("dialog:revert").unwrap();
    check_region(&t.d, dialog.rect, &format!("dialog-revert-{w}x{h}"));
    check_small_and_back(&mut t.d, "dialog:revert");
    // Enter reverts nothing; Cancel closes it and changes nothing.
    t.d.key("Enter").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.dialog().as_deref(), Some("revert"), "{case}: Enter reverts nothing");
    t.click("button:dialog.cancel");
    assert_eq!(t.dialog(), None, "{case}");
    t.d.settle(FRAMES).unwrap();
    assert!(t.saved(a) && !t.unsaved(a) && t.bytes(a) == saved, "{case}: Cancel changes nothing");

    // Undo Save: the original back, the edits back as unsaved edits.
    t.d.run("photo.revert", &json!({ "ids": [a] })).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.click("button:dialog.undoSave");
    t.until("the save undone", |t| !t.saved(a));
    assert_eq!((t.bytes(a), t.edits(a)), (original.clone(), vec![AUTOFIX.to_string()]), "{case}");

    // Revert: the original back, the edits gone.
    save(&mut t);
    t.d.run("photo.revert", &json!({ "ids": [a] })).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.click("button:dialog.revert");
    t.until("reverted", |t| !t.saved(a));
    assert_eq!((t.bytes(a), t.edits(a)), (original.clone(), Vec::<String>::new()), "{case}");
    assert!(!t.unsaved(a), "{case}");
    assert!(!exists(&t.dir.path().join("Pictures").join("Lisbon").join(".dalica-originals").join("IMG_0001.jpg")), "{case}");
}

/// Ctrl+Z in the edit view: with no Save it undoes the last edit; after a Save (Undo reads "Undo
/// Save") it is Undo Save, as the Undo button and Edit ▸ Undo are: the original's exact bytes come
/// back and the edits come back as unsaved edits; the next Ctrl+Z undoes an edit again.
#[test]
fn ctrl_z_in_the_edit_view_undoes_a_save_and_otherwise_the_last_edit() {
    let mut t = T::new(1, LARGE);
    let a = t.ids[0];
    let original = t.bytes(a);
    t.d.run("view.edit", &json!({ "id": a })).unwrap();
    t.d.settle(FRAMES).unwrap();
    t.edit(a, AUTOFIX);
    t.edit(a, TUNING);

    // No Save: Ctrl+Z undoes the last edit.
    t.d.key("Ctrl+Z").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.edits(a), [AUTOFIX]);
    assert_eq!(t.widget(UNDO_ID).unwrap().label, "Undo Auto Fix");

    // Saved: Undo reads "Undo Save", and Ctrl+Z does what it says.
    t.d.run("photo.save", &json!({ "ids": [a], "confirm": true })).unwrap();
    t.until("a saved", |t| t.saved(a));
    assert_ne!(t.bytes(a), original);
    assert_eq!(t.widget(UNDO_ID).unwrap().label, UNDO_SAVE);
    t.d.key("Ctrl+Z").unwrap();
    t.until("the save undone", |t| !t.saved(a) && t.unsaved(a));
    assert!(!t.d.state().status.contains("Nothing to undo"), "{}", t.d.state().status);
    assert_eq!(t.bytes(a), original, "the original's exact bytes are back");
    assert_eq!(t.edits(a), [AUTOFIX], "the edits are back, unsaved");
    assert_eq!(t.widget(UNDO_ID).unwrap().label, "Undo Auto Fix");

    // Nothing saved any more: Ctrl+Z undoes the edit.
    t.d.key("Ctrl+Z").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.edits(a), Vec::<String>::new());
    assert_eq!(t.bytes(a), original);
}

/// `ui.settle` with `jobs` over the control channel returns only once a running export and a
/// running Save have ended and their results are in the library; without `jobs` they are not
/// waited for.
#[test]
fn ui_settle_with_jobs_waits_for_a_running_export_and_save() {
    use dalica_ui::control::{Config, Connection, Control, MAX_WAIT_MS, Request, serve};
    let mut t = T::new(3, SMALL);
    let a = t.ids[0];
    t.edit(a, AUTOFIX);
    let out = t.dir.path().join("Out");
    let export =
        t.d.run("export.run", &json!({ "ids": t.ids, "dir": out, "folderName": "Lisbon", "size": "original" })).unwrap()["job"].as_u64().unwrap();
    let save = t.d.run("photo.save", &json!({ "ids": [a], "confirm": true })).unwrap()["job"].as_u64().unwrap();

    // No frame has run: both jobs are still running, and only `jobs` counts them.
    let busy = t.d.busy(true);
    assert!(busy.iter().any(|b| b.starts_with("file jobs: ")), "{busy:?}");
    assert!(!t.d.busy(false).iter().any(|b| b.starts_with("file jobs")), "{:?}", t.d.busy(false));

    let control = Control::new(Config::default());
    let s = serve(&control, &mut Connection::new(), &mut t.d, &Request::new("ui.settle", json!({ "timeoutMs": MAX_WAIT_MS, "jobs": true }))).unwrap();
    assert_eq!(s["settled"], json!(true), "{s}");
    assert!(!t.session().file_jobs_running(), "no file job runs once settled");
    for job in [export, save] {
        assert!(t.session().job(job).is_none_or(|e| e.state.is_finished()), "job {job}: {:?}", t.session().job(job));
    }
    assert!(t.saved(a) && !t.unsaved(a), "the Save's result is in the library");
    let written =
        std::fs::read_dir(out.join("Lisbon")).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "jpg")).count();
    assert_eq!(written, 3, "every exported file is written");
}
