//! Move to New Folder and Split Folder Here (M9.19, `organizing.md` §9), driven headlessly by
//! widget id on a temp library watching `photos/`, whose `Lisbon` holds `IMG_0001.jpg` …
//! `IMG_0004.jpg` taken on June 4 to 7, 2026:
//!
//! - File ▸ Move to New Folder… opens the new-folder dialog for the selection (Name "New Folder",
//!   the oldest photo's date with its box on); a name, a place and a description typed, OK makes
//!   the folder beside `Lisbon` with that information and the photos in it; Edit ▸ Undo moves
//!   them back and removes the folder;
//! - the photo menu's Split Folder Here… opens the same dialog, titled "New Folder" (not "Folder
//!   Properties"), and moves that photo and every photo after it; a date typed with the box off is
//!   the folder's; undo removes it;
//! - Cancel makes nothing; OK waits for a name and a date that reads as a day;
//! - the dialog matches its fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//!   (`tests/fingerprints/dialog-new-folder-1400x900.json`; `DALICA_BLESS=1` stores new ones).

use std::path::PathBuf;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, put_dated_jpeg, try_widget};
use dalica_core::{Date, PhotoId};
use dalica_engine::Session;
use dalica_ui::dialogs::new_folder::{DATE_ID, DESCRIPTION_ID, NAME_ID, PLACE_ID, USE_OLDEST_ID};
use dalica_ui::dialogs::{folder_properties, new_folder};
use dalica_ui::headless::Click;
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::json;

struct Fx {
    _dir: tempfile::TempDir,
    photos: PathBuf,
    /// IMG_0001 … IMG_0004, oldest first.
    ids: Vec<PhotoId>,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    for i in 1..=4u8 {
        put_dated_jpeg(&photos.join("Lisbon").join(format!("IMG_000{i}.jpg")), u64::from(i), (2026, 6, 3 + i));
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    let lisbon = s.library().folder_by_path(&photos.join("Lisbon")).unwrap();
    let mut named: Vec<(String, PhotoId)> = s.library().photos_in(lisbon).into_iter().map(|p| (p.name.clone(), p.id)).collect();
    named.sort();
    let ids = named.into_iter().map(|(_, id)| id).collect();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, photos, ids, d }
}

impl Fx {
    fn session(&self) -> &Session {
        self.d.app().session().unwrap()
    }

    fn folder_of(&self, id: PhotoId) -> PathBuf {
        let lib = self.session().library();
        lib.folder(lib.photo(id).unwrap().folder).unwrap().path.clone()
    }

    fn top(&self) -> Option<DialogKind> {
        self.d.state().dialogs.last().map(|d| d.kind)
    }

    fn undo_len(&self) -> usize {
        self.session().undo_stack().len()
    }

    /// Selects `ids`, then File ▸ Move to New Folder….
    fn move_to_new_folder(&mut self, ids: &[PhotoId]) {
        self.d.run("select.set", &json!({ "ids": ids })).unwrap();
        self.d.click("menu:File").unwrap();
        self.d.click("menuitem:photo.moveToNewFolder").unwrap();
        self.d.settle(FRAMES).unwrap();
        assert_eq!(self.top(), Some(DialogKind::NewFolder));
    }

    /// Split Folder Here… from `id`'s context menu.
    fn split_here(&mut self, id: PhotoId) {
        self.d.click_with(&format!("thumb:{id}"), &Click { button: egui::PointerButton::Secondary, ..Click::default() }).unwrap();
        self.d.settle(FRAMES).unwrap();
        self.d.click("menuitem:photo.splitFolder").unwrap();
        self.d.settle(FRAMES).unwrap();
        assert_eq!(self.top(), Some(DialogKind::NewFolder));
    }

    /// Replaces the text of a field: click, select all, type.
    fn retype(&mut self, id: &str, text: &str) {
        self.d.click(id).unwrap();
        self.d.key("Ctrl+A").unwrap();
        self.d.type_text(text).unwrap();
        self.d.settle(FRAMES).unwrap();
    }

    /// Clicks `id`, then waits for what it started (file jobs included).
    fn press(&mut self, id: &str) {
        self.d.click(id).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }

    fn run_and_wait(&mut self, id: &str) {
        self.d.run(id, &json!({})).unwrap();
        let s = self.d.settle_for(FRAMES, true);
        assert!(s.settled, "{s:?}");
    }
}

fn ok_enabled(d: &Driver) -> bool {
    try_widget(d, "button:dialog.ok").unwrap().enabled
}

/// Move to New Folder: the folder beside Lisbon with its information and the photos; undo removes
/// it.
#[test]
fn move_to_new_folder_makes_the_folder_beside_with_its_info_and_undo_removes_it() {
    let mut fx = fixture((1400, 900));
    let (a, b) = (fx.ids[1], fx.ids[2]);
    fx.move_to_new_folder(&[b, a]);
    let draft = fx.d.state().new_folder.clone().unwrap();
    assert_eq!((draft.name.as_str(), draft.date.as_str(), draft.use_oldest), ("New Folder", "2026-06-05", true), "the oldest of the two");
    assert!(!try_widget(&fx.d, DATE_ID).unwrap().enabled, "the date follows the oldest photo");
    assert!(ok_enabled(&fx.d));
    fx.retype(NAME_ID, "  ");
    assert!(!ok_enabled(&fx.d), "a blank name");
    fx.retype(NAME_ID, "Lisbon Day 2");
    fx.retype(PLACE_ID, "Lisbon, Portugal");
    fx.retype(DESCRIPTION_ID, "Trams and tiles");
    let steps = fx.undo_len();
    fx.press("button:dialog.ok");
    assert_eq!(fx.top(), None);
    let made = fx.photos.join("Lisbon Day 2");
    assert!(made.join("IMG_0002.jpg").is_file() && made.join("IMG_0003.jpg").is_file());
    assert!(!fx.photos.join("Lisbon").join("IMG_0002.jpg").exists());
    assert_eq!((fx.folder_of(a), fx.folder_of(b)), (made.clone(), made.clone()), "the photos keep their ids");
    assert_eq!(fx.folder_of(fx.ids[0]), fx.photos.join("Lisbon"));
    let id = fx.session().library().folder_by_path(&made).unwrap();
    let info = fx.session().library().folder_info(id).clone();
    assert_eq!(
        (info.date, info.place.as_str(), info.description.as_str()),
        (None, "Lisbon, Portugal", "Trams and tiles"),
        "dated by its oldest photo"
    );
    assert_eq!(fx.undo_len(), steps + 1, "one undo step");
    // Edit ▸ Undo moves them back and removes the folder.
    fx.run_and_wait("edit.undo");
    assert!(!made.exists(), "the folder went to the trash");
    assert!(fx.session().library().folder_by_path(&made).is_none());
    assert!(fx.ids.iter().all(|id| fx.folder_of(*id) == fx.photos.join("Lisbon")));
    assert!(fx.photos.join("Lisbon").join("IMG_0002.jpg").is_file());
}

/// Split Folder Here: the dialog titled "New Folder"; the photo and every photo after it; a date
/// typed with the box off.
#[test]
fn split_folder_here_moves_the_photo_and_every_photo_after_it() {
    let mut fx = fixture((1400, 900));
    let lisbon = fx.photos.join("Lisbon");
    fx.split_here(fx.ids[2]);
    let title = dalica_ui::dialogs::inspect(fx.d.app())["title"].clone();
    assert_eq!(title, "New Folder", "a folder still to be made");
    assert_ne!(new_folder::TITLE, folder_properties::TITLE);
    assert_eq!(fx.d.state().new_folder.as_ref().map(|d| d.ids.clone()), Some(fx.ids[2..].to_vec()));
    assert_eq!(fx.d.state().new_folder.as_ref().map(|d| d.date.clone()), Some("2026-06-06".into()));
    fx.d.click(USE_OLDEST_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DATE_ID).unwrap().enabled);
    fx.retype(DATE_ID, "June 1");
    assert!(!ok_enabled(&fx.d), "a date that is no day");
    fx.retype(DATE_ID, "2026-06-01");
    fx.retype(NAME_ID, "Sintra");
    assert!(ok_enabled(&fx.d));
    fx.press("button:dialog.ok");
    let sintra = fx.photos.join("Sintra");
    assert_eq!(fx.folder_of(fx.ids[0]), lisbon);
    assert_eq!(fx.folder_of(fx.ids[1]), lisbon);
    assert_eq!((fx.folder_of(fx.ids[2]), fx.folder_of(fx.ids[3])), (sintra.clone(), sintra.clone()));
    let id = fx.session().library().folder_by_path(&sintra).unwrap();
    assert_eq!(fx.session().library().folder_info(id).date, Some(Date::new(2026, 6, 1).unwrap()));
    fx.run_and_wait("edit.undo");
    assert!(!sintra.exists() && fx.session().library().folder_by_path(&sintra).is_none());
    assert!(fx.ids.iter().all(|id| fx.folder_of(*id) == lisbon));
}

/// Cancel and Esc make nothing.
#[test]
fn cancel_makes_nothing() {
    let mut fx = fixture((1400, 900));
    let steps = fx.undo_len();
    fx.move_to_new_folder(&[fx.ids[0]]);
    fx.retype(NAME_ID, "Nope");
    fx.press("button:dialog.cancel");
    fx.split_here(fx.ids[3]);
    fx.d.key("Escape").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.top(), None);
    assert!(!fx.photos.join("Nope").exists() && !fx.photos.join("New Folder").exists());
    assert_eq!(fx.undo_len(), steps);
    assert!(fx.ids.iter().all(|id| fx.folder_of(*id) == fx.photos.join("Lisbon")));
}

/// The dialog for two photos, as it opens, matches its fingerprint, and fits the small window.
#[test]
fn the_dialog_matches_its_fingerprint_and_fits_the_small_window() {
    let mut fx = fixture(LARGE);
    let two = [fx.ids[0], fx.ids[1]];
    fx.move_to_new_folder(&two);
    check_sizes(&mut fx.d, "dialog:newFolder", "dialog-new-folder");
}
