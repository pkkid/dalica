//! Add to Album (M9.18, `organizing.md` §4.2–§4.3), driven headlessly through the photo menu: Add
//! to Album ▸ <album> adds the selected photos in one step and Edit ▸ Undo takes them out again;
//! New Album… opens Album Properties for the photos; a row with nothing to add is refused with its
//! reason.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, try_widget};
use dalica_core::{AlbumId, PhotoId};
use dalica_engine::Session;
use dalica_ui::headless::Click;
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::{App, Driver, DriverError, ScriptedPicker};
use egui::PointerButton;
use serde_json::json;

/// A library in `root` with two folders of 4 scenes photos each, added and scanned.
fn session(root: &Path) -> Session {
    let files = crate::common::demo_library(2, 8, 13).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

/// Selects `ids`, right-clicks the first one's cell in its folder's section (scrolled to: the
/// album sections above it grow as photos change) and opens Add to Album ▸.
fn open_add_to_album(d: &mut Driver, ids: &[PhotoId]) {
    let folder = dalica_ui::state::Section::Folder(d.app().session().unwrap().library().photo(ids[0]).unwrap().folder);
    d.run("section.scrollTo", &json!({ "section": folder })).unwrap();
    d.run("select.set", &json!({ "ids": ids.iter().map(|id| id.get()).collect::<Vec<_>>() })).unwrap();
    d.settle(FRAMES).unwrap();
    d.click_with(&format!("thumb:{}", ids[0].get()), &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("submenu:Photo/Add to Album").unwrap();
    d.settle(FRAMES).unwrap();
}

fn members(d: &Driver, album: &AlbumId) -> Vec<PhotoId> {
    d.app().session().unwrap().library().album_members(album).to_vec()
}

/// Edit ▸ Undo as the menu bar shows it now.
fn undo_label(d: &Driver) -> String {
    let bar = menus::bar(d.app(), Platform::current());
    menus::find(&bar, &Lookup::Id { id: "edit.undo".into(), params: json!({}) }).unwrap().label.clone()
}

#[test]
fn add_to_album_adds_the_photos_and_edit_undo_takes_them_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = new_driver(App::with_picker(Some(session(dir.path())), Arc::new(ScriptedPicker::new())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let s: Vec<Vec<PhotoId>> = d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect();
    let album = d.run("album.create", &json!({ "name": "Lisbon", "ids": [s[1][0].get()] })).unwrap()["album"].as_str().unwrap().to_string();
    let album = AlbumId::parse(&album).unwrap();
    let steps = d.app().session().unwrap().undo_stack().len();

    // Two photos of the first folder: added after the album's photo, in the selection's order.
    open_add_to_album(&mut d, &s[0][1..3]);
    let row = format!("menuitem:album.add/{album}");
    assert_eq!(try_widget(&d, &row).expect("the album is listed").label, "Lisbon");
    d.click(&row).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, &row).is_none(), "the menu closed");
    assert_eq!(members(&d, &album), [s[1][0], s[0][1], s[0][2]]);
    assert_eq!(d.app().session().unwrap().undo_stack().len(), steps + 1, "one step");
    assert_eq!(undo_label(&d), "Undo Add to Album");

    // Edit ▸ Undo: the photos leave the album again; the files never moved.
    let undo =
        menus::find(&menus::bar(d.app(), Platform::current()), &Lookup::Path(vec!["Edit".into(), "Undo Add to Album".into()])).cloned().unwrap();
    menus::activate(d.app_mut(), &undo, &json!(null)).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(members(&d, &album), [s[1][0]]);
    let folder = dalica_ui::state::Section::Folder(d.app().session().unwrap().library().photo(s[0][0]).unwrap().folder);
    assert_eq!(d.app().panels().unwrap().sections.get(folder).unwrap().photos, s[0], "the folder keeps its photos");

    // A photo already in the album stays where it is.
    open_add_to_album(&mut d, &s[1][0..2]);
    d.click(&row).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(members(&d, &album), [s[1][0], s[1][1]]);
}

#[test]
fn new_album_from_the_menu_opens_album_properties_with_the_photos() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = new_driver(App::with_picker(Some(session(dir.path())), Arc::new(ScriptedPicker::new())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let s: Vec<Vec<PhotoId>> = d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect();
    // With no albums yet, Add to Album ▸ holds New Album… alone.
    open_add_to_album(&mut d, &s[0][0..3]);
    assert!(!d.widgets().iter().any(|w| w.id.starts_with("menuitem:album.add/")));
    d.click("menuitem:album.new").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "dialog:albumProperties").is_some(), "Album Properties opens");
    let draft = d.state().album_properties.clone().expect("a draft for a new album");
    assert_eq!((draft.album, draft.ids.as_slice(), draft.name.as_str()), (None, &s[0][0..3], "New Album"));
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty() && d.app().session().unwrap().library().albums().next().is_none(), "Cancel makes nothing");
}

#[test]
fn an_album_row_with_nothing_to_add_is_refused_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = new_driver(App::with_picker(Some(session(dir.path())), Arc::new(ScriptedPicker::new())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let album = d.run("album.create", &json!({ "name": "Empty" })).unwrap()["album"].as_str().unwrap().to_string();
    d.run("select.none", &json!({})).unwrap();
    let menu = menus::photo_menu(d.app(), Platform::current());
    let item = menus::find_id(&menu, "album.add", &json!({ "album": album })).cloned().unwrap();
    assert!(!item.enabled && item.reason.is_some(), "{item:?}");
    let before = d.app().session().unwrap().undo_stack().len();
    assert!(menus::activate(d.app_mut(), &item, &json!(null)).is_err());
    assert_eq!(d.state().status, item.reason.clone().unwrap(), "the reason is on the status line");
    assert_eq!(d.app().session().unwrap().undo_stack().len(), before);
    // The command refuses hostile parameters from any surface, without a change.
    for bad in [json!({ "album": album, "ids": "all" }), json!({ "album": 7, "ids": [1] }), json!({ "album": album, "ids": [1], "x": 1 })] {
        assert!(matches!(d.run("album.add", &bad), Err(DriverError::Command(_))), "{bad}");
    }
    assert_eq!(d.app().session().unwrap().undo_stack().len(), before);
}
