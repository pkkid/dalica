//! Hidden photos and folders in the library (M6.24, `ui-layout.md` §4 and §5.3), driven headlessly.
//!
//! - Hiding a photo removes its cell, Ctrl+A skips it and it leaves the selection; View ▸ Hidden
//!   Pictures shows it again at 40 % (and then it may stay selected).
//! - Hiding a folder takes its row out of the tree and its section out of the light table; with
//!   Hidden Pictures its row is in the Hidden Folders group (after Folders) and its section is back.
//!   The tree region is checked against `region-tree-hidden-1400.json` at 1400 × 900 (at 900 × 600 they only have to fit).
//! - A double-click on a tree row opens that folder's Folder Properties.
//!
//! Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores new fingerprints.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_tree, new_driver, widget};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::headless::Click;
use dalica_ui::panels::light_table::cell::HIDDEN_OPACITY;
use dalica_ui::state::{DialogKind, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use serde_json::json;

/// `folders` folders of `photos` demo photos each under `dir`, watched and scanned.
fn library(dir: &Path, folders: usize, photos: usize) -> Session {
    let root = dir.join("photos");
    for f in dalica_scenes::demo_library(folders, photos, 11).unwrap() {
        let path = root.join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    s
}

fn driver(session: Session, (w, h): (u32, u32)) -> Driver {
    let mut d = new_driver(App::new(Some(session)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn has(d: &Driver, id: &str) -> bool {
    d.widgets().iter().any(|w| w.id == id)
}

/// The photos of the light table's only (or first) section, in display order.
fn section_photos(d: &Driver) -> Vec<PhotoId> {
    d.app().panels().unwrap().sections.list.first().unwrap().photos.clone()
}

fn every_photo(d: &Driver) -> Vec<PhotoId> {
    let mut ids: Vec<PhotoId> = d.app().session().unwrap().library().photos().map(|p| p.id).collect();
    ids.sort();
    ids
}

fn sorted(mut v: Vec<PhotoId>) -> Vec<PhotoId> {
    v.sort();
    v
}

fn opacity(d: &Driver, id: PhotoId) -> f32 {
    d.app().panels().unwrap().light_table.cells.get(id).unwrap_or_else(|| panic!("{id} not drawn")).opacity
}

/// Clicks the first cell, then Ctrl+A.
fn select_all(d: &mut Driver) {
    let first = section_photos(d)[0];
    d.click(&format!("thumb:{first}")).unwrap();
    d.key("Ctrl+A").unwrap();
    d.settle(FRAMES).unwrap();
}

#[test]
fn a_hidden_photo_leaves_the_cells_the_selection_and_ctrl_a_until_hidden_pictures_shows_it_at_40_percent() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(library(dir.path(), 1, 5), (1400, 900));
    let all = every_photo(&d);
    assert_eq!(sorted(section_photos(&d)), all);
    select_all(&mut d);
    assert_eq!(sorted(d.state().selection.ids.clone()), all);
    let hidden = section_photos(&d)[2];
    let anchor = d.state().selection.anchor;

    // Hidden: the cell goes, and so does its place in the selection (the anchor stays).
    d.run("photo.hide", &json!({ "ids": [hidden] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.app().session().unwrap().library().is_hidden(hidden));
    assert!(!has(&d, &format!("thumb:{hidden}")), "its cell is gone");
    let rest: Vec<PhotoId> = all.iter().copied().filter(|&id| id != hidden).collect();
    assert_eq!(sorted(section_photos(&d)), rest, "out of the layout");
    assert_eq!(sorted(d.state().selection.ids.clone()), rest, "out of the selection");
    assert_eq!(d.state().selection.anchor, anchor);
    assert!(!dalica_ui::tray::photos(d.state()).contains(&hidden), "out of the tray");
    // Ctrl+A selects the photos shown, and only them.
    d.key("Ctrl+D").unwrap();
    select_all(&mut d);
    assert_eq!(sorted(d.state().selection.ids.clone()), rest, "Ctrl+A skips it");
    // A band over every cell does not reach it either: the section's grid has no cell for it.
    assert_eq!(d.app().panels().unwrap().light_table.layout().unwrap().count(0), rest.len());

    // Hidden Pictures: back at 40 %, the others opaque; Ctrl+A takes it too.
    d.run("view.hiddenPictures", &json!({ "on": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(has(&d, &format!("thumb:{hidden}")));
    assert_eq!(opacity(&d, hidden), HIDDEN_OPACITY);
    assert!(rest.iter().all(|&id| opacity(&d, id) == 1.0));
    select_all(&mut d);
    assert_eq!(sorted(d.state().selection.ids.clone()), all, "with Hidden Pictures, Ctrl+A takes it");
    // With the option on, hiding a selected photo keeps it selected.
    let second = rest[0];
    d.run("photo.hide", &json!({ "ids": [second] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(sorted(d.state().selection.ids.clone()), all);
    assert_eq!(opacity(&d, second), HIDDEN_OPACITY);
    // Turning the option off drops both from the selection.
    d.run("view.hiddenPictures", &json!({ "on": false })).unwrap();
    d.settle(FRAMES).unwrap();
    let shown: Vec<PhotoId> = rest.iter().copied().filter(|&id| id != second).collect();
    assert_eq!(sorted(d.state().selection.ids.clone()), shown);

    // Unhidden: its cell is back, opaque.
    d.run("photo.unhide", &json!({ "ids": [hidden, second] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(sorted(section_photos(&d)), all);
    assert_eq!(opacity(&d, hidden), 1.0);
}

#[test]
fn hiding_every_selected_photo_leaves_nothing_selected() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(library(dir.path(), 1, 3), (900, 600));
    let first = section_photos(&d)[0];
    d.click(&format!("thumb:{first}")).unwrap();
    d.settle(FRAMES).unwrap();
    d.run("photo.hide", &json!({ "ids": [first] })).unwrap();
    d.settle(FRAMES).unwrap();
    let sel = &d.state().selection;
    assert!(sel.ids.is_empty() && sel.anchor.is_none() && sel.section.is_none(), "{sel:?}");
}

/// The folder rows on screen, top to bottom.
fn rows(d: &Driver) -> Vec<String> {
    let mut rows: Vec<WidgetInfo> = d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.into_iter().map(|w| w.id).collect()
}

fn shows_section(d: &Driver, folder: FolderId) -> bool {
    d.app().panels().unwrap().sections.get(Section::Folder(folder)).is_some()
}

#[test]
fn a_hidden_folder_moves_to_hidden_folders_and_its_section_shows_only_with_hidden_pictures() {
    let size = LARGE;
    let w = size.0;
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(library(dir.path(), 3, 6), size);
    let folders: Vec<FolderId> = d.app().panels().unwrap().sections.list.iter().filter_map(|s| s.folder).collect();
    assert_eq!(folders.len(), 3, "{w}");
    let target = folders[1];
    let row = format!("tree:folder:{target}");
    assert!(has(&d, &row) && !has(&d, "tree:group:hidden"), "{w}");
    // Its photos selected (the tree row selects the section as a unit).
    d.click(&row).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().selection.section, Some(Section::Folder(target)), "{w}");

    // Hidden: the row and the section go, and the selection with them.
    d.run("folder.hide", &json!({ "section": format!("folder:{target}") })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.app().session().unwrap().library().is_folder_hidden(target), "{w}");
    assert!(!has(&d, &row) && !has(&d, "tree:group:hidden"), "{w}: no row, no Hidden Folders group");
    assert!(!shows_section(&d, target), "{w}: no section");
    assert_eq!(rows(&d).len(), 2, "{w}");
    assert!(d.state().selection.ids.is_empty(), "{w}: its photos left the selection");

    // Hidden Pictures: the row is in Hidden Folders, under the other folders; its section is back.
    d.run("view.hiddenPictures", &json!({ "on": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(has(&d, &row) && has(&d, "tree:group:hidden"), "{w}");
    let (header, folder_rows) = (widget(&d, "tree:group:hidden").rect, rows(&d));
    assert_eq!(folder_rows.last(), Some(&row), "{w}: last, in Hidden Folders: {folder_rows:?}");
    assert!(widget(&d, &row).rect[1] > header[1], "{w}: under the Hidden Folders header");
    assert!(widget(&d, "tree:group:folders").rect[1] < header[1], "{w}: Hidden Folders comes after Folders");
    assert!(shows_section(&d, target), "{w}: its section is back");
    check_tree(&d, &format!("region-tree-hidden-{w}"));

    // A double-click on its row opens its Folder Properties.
    d.click_with(&row, &Click { count: 2, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    let dialog = d.state().dialogs.last().unwrap_or_else(|| panic!("{w}: no dialog"));
    assert_eq!(dialog.kind, DialogKind::FolderProperties, "{w}");
    assert_eq!(dialog.fields.get("folder"), Some(&json!(target)), "{w}: {:?}", dialog.fields);
    assert!(has(&d, "dialog:folderProperties"), "{w}");
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();

    // Unhidden: back among the Folders, the group gone.
    d.run("folder.unhide", &json!({ "section": format!("folder:{target}") })).unwrap();
    d.run("view.hiddenPictures", &json!({ "on": false })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(has(&d, &row) && !has(&d, "tree:group:hidden") && shows_section(&d, target), "{w}");
}
