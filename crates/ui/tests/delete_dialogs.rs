//! The delete questions (M9.20, `organizing.md` §9), driven headlessly by widget id on temporary
//! folders under a watched folder:
//!
//! - Delete in a folder, Ctrl+Delete and File ▸ Delete from Disk… ask "Move the selected picture
//!   to the trash?" / "Move the %d selected pictures to the trash?" with [Move to Trash] [Cancel];
//!   Cancel keeps the photos, Move to Trash sends them to the OS trash as one undo step, which
//!   undo takes back where the system's trash can give them back (Linux, Windows);
//! - Folder ▸ Delete… asks "Move the folder "%s" and everything in it (including non-picture
//!   files) to the trash?"; Cancel keeps it, Move to Trash trashes it with its non-picture files
//!   and subfolders;
//! - with Options → "Delete from disk without asking" on, Delete trashes at once;
//! - the edit view's trash button asks the same question for the photo shown; Cancel keeps it, Move
//!   to Trash trashes it and the edit view shows the next photo of its section, else the previous
//!   one, else the library;
//! - both questions match their fingerprints at 1400 × 900 and fit 900 × 600
//!   (`tests/fingerprints/dialog-delete-<photos|folder>-1400x900.json`;
//!   `DALICA_BLESS=1` stores new ones, renders go to `target/test-output/ui/`).
//!
//! These tests use the real OS trash and put back what they sent there where the system allows
//! it, as the engine's own delete tests do.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, try_widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_engine::cmd::delete::{RESTORES, UNDO_UNSUPPORTED};
use dalica_ui::dialogs::{self, confirm, delete};
use dalica_ui::state::{DeleteFolder, DeletePhotos, DialogKind, View};
use dalica_ui::{App, Driver};
use serde_json::json;

const TRASH: &str = "button:dialog.moveToTrash";

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()
}

struct Fx {
    _dir: tempfile::TempDir,
    /// `photos/Lisbon`: IMG_0001 … IMG_0003 and `notes.txt`.
    lisbon: PathBuf,
    /// `photos/Old`: OLD_0001, `notes.txt` and `Sub/SUB_0001.jpg`.
    old: PathBuf,
    /// The Lisbon photos by name.
    ids: Vec<PhotoId>,
    old_id: FolderId,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    let (lisbon, old) = (photos.join("Lisbon"), photos.join("Old"));
    std::fs::create_dir_all(old.join("Sub")).unwrap();
    std::fs::create_dir_all(&lisbon).unwrap();
    for i in 1..=3 {
        std::fs::write(lisbon.join(format!("IMG_{i:04}.jpg")), jpeg(i)).unwrap();
    }
    std::fs::write(lisbon.join("notes.txt"), b"not a picture").unwrap();
    std::fs::write(old.join("OLD_0001.jpg"), jpeg(10)).unwrap();
    std::fs::write(old.join("notes.txt"), b"not a picture").unwrap();
    std::fs::write(old.join("Sub").join("SUB_0001.jpg"), jpeg(11)).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    let lib = s.library();
    let mut lisbon_photos: Vec<_> = lib.photos().filter(|p| p.name.starts_with("IMG_")).map(|p| (p.name.clone(), p.id)).collect();
    lisbon_photos.sort();
    let ids = lisbon_photos.into_iter().map(|(_, id)| id).collect();
    let old_id = lib.folder_by_path(&old).unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, old, ids, old_id, d }
}

fn exists(path: &Path) -> bool {
    path.exists()
}

fn photo_count(d: &Driver) -> usize {
    d.app().session().unwrap().library().photos().count()
}

fn undo_steps(d: &Driver) -> (usize, Option<&'static str>) {
    let u = d.app().session().unwrap().undo_stack();
    (u.len(), u.undo_label())
}

/// The open question: its kind, its title and its text.
fn question(d: &Driver) -> (DialogKind, String, String) {
    let top = dialogs::top(d.app()).expect("a question is open");
    let v = dialogs::inspect(d.app());
    (top.kind, v["title"].as_str().unwrap().to_string(), confirm::question(d.app(), top))
}

fn select(d: &mut Driver, ids: &[PhotoId]) {
    d.run("select.set", &json!({ "ids": ids })).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Runs frames until the file jobs are done and their results are in the library.
fn settle_jobs(d: &mut Driver) {
    let s = d.settle_for(FRAMES, true);
    assert!(s.settled, "still busy: {:?}", s.busy);
}

/// Edit ▸ Undo, then the job's end; where the trash cannot give items back it is refused with the
/// engine's reason. Returns whether it undid.
fn undo(d: &mut Driver) -> bool {
    let r = d.app_mut().run("edit.undo", &json!({}));
    settle_jobs(d);
    if RESTORES {
        r.unwrap();
        true
    } else {
        assert_eq!(r.unwrap_err().to_string(), UNDO_UNSUPPORTED);
        false
    }
}

/// Delete, Ctrl+Delete and File ▸ Delete from Disk… ask first, with the spec's words; Cancel and Esc
/// keep the photos; Move to Trash trashes them as one undo step, which undo takes back.
#[test]
fn the_photo_question_reads_as_specified_and_move_to_trash_trashes() {
    let mut fx = fixture((1400, 900));
    let ids = fx.ids.clone();
    let file = |i: usize| fx.lisbon.join(format!("IMG_{:04}.jpg", i + 1));
    let files: Vec<PathBuf> = (0..3).map(file).collect();

    // Delete in a folder, one photo: the question, and Cancel keeps it.
    select(&mut fx.d, &ids[..1]);
    fx.d.key("Delete").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d), (DialogKind::DeletePhotos, "Delete".to_string(), "Move the selected picture to the trash?".to_string()));
    assert_eq!(fx.d.state().delete_photos, Some(DeletePhotos { ids: ids[..1].to_vec() }));
    let v = dialogs::inspect(fx.d.app());
    assert_eq!((v["buttons"].clone(), v["default"].clone()), (json!(["button:dialog.cancel", TRASH]), json!(TRASH)));
    assert_eq!(try_widget(&fx.d, TRASH).unwrap().label, "Move to Trash");
    fx.d.click("button:dialog.cancel").unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty() && fx.d.state().delete_photos.is_none());
    assert!(files.iter().all(|f| exists(f)) && photo_count(&fx.d) == 5, "Cancel keeps the photo");
    assert_eq!(undo_steps(&fx.d).0, 0);

    // File ▸ Delete from Disk…: the same question; Esc keeps it.
    fx.d.click("menu:File").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("menuitem:photo.delete").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d).2, "Move the selected picture to the trash?");
    fx.d.key("Escape").unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty() && exists(&files[0]));

    // Ctrl+Delete, two photos: Move to Trash sends them to the trash, one undo step.
    select(&mut fx.d, &ids[..2]);
    fx.d.key("Ctrl+Delete").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d).2, "Move the 2 selected pictures to the trash?");
    fx.d.click(TRASH).unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty());
    assert!(!exists(&files[0]) && !exists(&files[1]) && exists(&files[2]), "two photos went to the trash");
    assert!(exists(&fx.lisbon.join("notes.txt")));
    let lib = fx.d.app().session().unwrap().library();
    assert!(lib.photo(ids[0]).is_none() && lib.photo(ids[1]).is_none() && lib.photo(ids[2]).is_some());
    assert_eq!(undo_steps(&fx.d), (1, Some("Delete")));

    // Undo brings them back from the trash where the system allows it.
    if undo(&mut fx.d) {
        assert!(files.iter().all(|f| exists(f)) && photo_count(&fx.d) == 5, "undo restores them");
    }
}

/// The edit view's trash button (`button:edit.trash`) asks the delete question for the photo shown;
/// Cancel keeps it; Move to Trash trashes it and the view shows the next photo of its section, the
/// previous one at the end, and the library once the section is empty.
#[test]
fn the_edit_views_trash_button_asks_and_moves_on() {
    let mut fx = fixture((1400, 900));
    let ids = fx.ids.clone();
    let files: Vec<PathBuf> = (1..=3).map(|i| fx.lisbon.join(format!("IMG_{i:04}.jpg"))).collect();
    let shown = |d: &Driver| (d.state().view, d.state().edit.photo);
    fx.d.app_mut().run("view.edit", &json!({ "id": ids[1] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, "button:edit.trash").unwrap().enabled, "the trash button is enabled");

    // The question for the photo shown; Cancel keeps it, and the view stays on it.
    fx.d.click("button:edit.trash").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d), (DialogKind::DeletePhotos, "Delete".to_string(), "Move the selected picture to the trash?".to_string()));
    assert_eq!(fx.d.state().delete_photos, Some(DeletePhotos { ids: vec![ids[1]] }));
    fx.d.click("button:dialog.cancel").unwrap();
    settle_jobs(&mut fx.d);
    assert!(fx.d.state().dialogs.is_empty() && files.iter().all(|f| exists(f)), "Cancel keeps the photo");
    assert_eq!(shown(&fx.d), (View::Edit, Some(ids[1])));

    // Move to Trash: the photo goes to the trash; the view shows the next one.
    fx.d.click("button:edit.trash").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click(TRASH).unwrap();
    settle_jobs(&mut fx.d);
    assert!(!exists(&files[1]) && exists(&files[0]) && exists(&files[2]), "the photo shown went to the trash");
    assert_eq!(shown(&fx.d), (View::Edit, Some(ids[2])), "the next photo of the section");
    // At the section's end: the previous one; the section empty: the library.
    for (gone, then) in [(2, (View::Edit, Some(ids[0]))), (0, (View::Library, None))] {
        fx.d.click("button:edit.trash").unwrap();
        fx.d.settle(FRAMES).unwrap();
        fx.d.click(TRASH).unwrap();
        settle_jobs(&mut fx.d);
        assert!(!exists(&files[gone]), "IMG_{:04} went to the trash", gone + 1);
        assert_eq!(shown(&fx.d), then, "after IMG_{:04}", gone + 1);
    }
    assert_eq!(undo_steps(&fx.d).0, 3);
    if (0..3).all(|_| undo(&mut fx.d)) {
        assert!(files.iter().all(|f| exists(f)), "undo restores them");
    }
}

/// Folder ▸ Delete… names the folder; Cancel keeps it; Move to Trash trashes it with everything in
/// it (its non-picture file and its subfolder), as one undo step.
#[test]
fn the_folder_question_names_the_folder_and_takes_everything_in_it() {
    let mut fx = fixture((1400, 900));
    let old = fx.old_id;
    fx.d.run("select.section", &json!({ "section": format!("folder:{old}") })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let open = |fx: &mut Fx| {
        fx.d.click("menu:Folder").unwrap();
        fx.d.settle(FRAMES).unwrap();
        fx.d.click("menuitem:folder.delete").unwrap();
        fx.d.settle(FRAMES).unwrap();
    };
    open(&mut fx);
    assert_eq!(
        question(&fx.d),
        (
            DialogKind::DeleteFolder,
            "Delete Folder".to_string(),
            "Move the folder \"Old\" and everything in it (including non-picture files) to the trash?".to_string()
        )
    );
    assert_eq!(question(&fx.d).2, delete::folder_question("Old"));
    assert_eq!(fx.d.state().delete_folder, Some(DeleteFolder { folder: old }));
    fx.d.click("button:dialog.cancel").unwrap();
    settle_jobs(&mut fx.d);
    assert!(exists(&fx.old.join("OLD_0001.jpg")) && fx.d.app().session().unwrap().library().folder(old).is_some(), "Cancel keeps it");

    open(&mut fx);
    fx.d.click(TRASH).unwrap();
    settle_jobs(&mut fx.d);
    assert!(!exists(&fx.old), "the folder went to the trash with its note and subfolder");
    let lib = fx.d.app().session().unwrap().library();
    assert!(lib.folder(old).is_none() && lib.folder_by_path(&fx.old.join("Sub")).is_none());
    assert_eq!(photo_count(&fx.d), 3, "the Lisbon photos stay");
    assert_eq!(undo_steps(&fx.d), (1, Some("Delete Folder")));
    if undo(&mut fx.d) {
        assert!(exists(&fx.old.join("notes.txt")) && exists(&fx.old.join("Sub").join("SUB_0001.jpg")));
        assert!(fx.d.app().session().unwrap().library().folder(old).is_some(), "it comes back with its id");
    }
}

/// With Options → "Delete from disk without asking" on, Delete trashes the photo at once; with no
/// folder selected Folder ▸ Delete… is greyed with the reason.
#[test]
fn delete_without_asking_skips_the_question() {
    let mut fx = fixture((1400, 900));
    let ids = fx.ids.clone();
    let tree = dalica_ui::menus::bar(fx.d.app(), dalica_ui::menus::Platform::Other);
    let row = tree.iter().find_map(|m| dalica_ui::menus::find_id(&m.children, "folder.delete", &json!({}))).cloned().unwrap();
    assert_eq!((row.enabled, row.reason.as_deref()), (false, Some("Select a folder first.")));

    fx.d.app_mut().session_mut().unwrap().change_settings(|st| {
        let mut o = st.organize();
        o.delete_without_asking = true;
        st.set_organize(&o);
    });
    select(&mut fx.d, &ids[2..]);
    fx.d.key("Delete").unwrap();
    assert!(fx.d.state().dialogs.is_empty(), "no question");
    settle_jobs(&mut fx.d);
    assert!(!exists(&fx.lisbon.join("IMG_0003.jpg")));
    assert_eq!(undo_steps(&fx.d), (1, Some("Delete")));
    if undo(&mut fx.d) {
        assert!(exists(&fx.lisbon.join("IMG_0003.jpg")));
    }
}

/// Both questions match their fingerprints, and fit the small window.
#[test]
fn the_questions_match_their_fingerprints_and_fit_the_small_window() {
    let mut fx = fixture(LARGE);
    let ids = fx.ids.clone();
    select(&mut fx.d, &ids);
    fx.d.key("Delete").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d).2, "Move the 3 selected pictures to the trash?");
    check_sizes(&mut fx.d, "dialog:deletePhotos", "dialog-delete-photos");
    fx.d.key("Escape").unwrap();

    let old = fx.old_id;
    fx.d.run("folder.delete", &json!({ "folder": old })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(question(&fx.d).0, DialogKind::DeleteFolder);
    check_sizes(&mut fx.d, "dialog:deleteFolder", "dialog-delete-folder");
    fx.d.key("Escape").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(exists(&fx.old) && exists(&fx.lisbon.join("IMG_0001.jpg")), "Esc deletes nothing");
}
