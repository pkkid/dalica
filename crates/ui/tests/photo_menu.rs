//! The photo context menu (M9.18, `organizing.md` §4.3 and §9.1), driven headlessly: its rows in
//! order with Add to Album ▸ listing the albums by name then New Album…, and their enabled states
//! in a folder and in an album; in an album Remove from Album asks (unless "Remove from albums
//! without asking" is on), removes and leaves the files, Set as Album Cover changes the cover the
//! tree shows (`albums.list`), and Show in Folder selects the photo in its folder. The open menu
//! with Add to Album ▸ open is fingerprinted at 1400 × 900 as `menu-photo-1400x900.json` (its label
//! column over the open submenu, the same on every OS)
//! (`DALICA_BLESS=1` stores it; renders go to `target/test-output/ui/`).
//!
//! In an album the tests click and right-click the photo's copy in the album's section
//! (`thumb:<PhotoId>/in/<album>`), as a person does; the enabled-state checks of cases a click cannot
//! reach (a photo of the selection outside the album) put the selection there directly.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_pixels, new_driver, out_dir, try_widget};
use dalica_core::{AlbumId, PhotoId};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::headless::Click;
use dalica_ui::menus::{self, Platform};
use dalica_ui::state::{AlbumKey, AlbumSection, Section, Selection};
use dalica_ui::{App, Driver, ScriptedPicker};
use egui::PointerButton;
use serde_json::{Value, json};

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

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    /// The light table's sections' photos, in order.
    s: Vec<Vec<PhotoId>>,
    /// "Trams" (the first folder's first two photos) and the empty "Beach".
    trams: AlbumId,
    beach: AlbumId,
}

fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    let found = session.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    assert_eq!(found["photos"].as_array().unwrap().len(), 8);
    let mut d = new_driver(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let s: Vec<Vec<PhotoId>> = d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect();
    let trams = d.run("album.create", &json!({ "name": "Trams", "ids": [s[0][0], s[0][1]] })).unwrap()["album"].as_str().unwrap().to_string();
    let beach = d.run("album.create", &json!({ "name": "Beach" })).unwrap()["album"].as_str().unwrap().to_string();
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, s, trams: AlbumId::parse(&trams).unwrap(), beach: AlbumId::parse(&beach).unwrap() }
}

fn section(album: &AlbumId) -> Section {
    Section::Album(AlbumSection::User(AlbumKey::of(album)))
}

/// Puts the selection in `section` (as selecting an album section does) with `ids`, `focus` focused.
fn select_in(t: &mut T, section: Section, ids: &[PhotoId], focus: PhotoId) {
    t.d.app_mut().state_mut().selection = Selection::new(ids.to_vec(), Some(focus), Some(section), false);
    t.d.settle(FRAMES).unwrap();
}

/// Right-clicks photo `id`'s cell in its folder: the photo menu opens.
fn open_menu(t: &mut T, id: PhotoId) {
    open_menu_on(t, &format!("thumb:{}", id.get()));
}

/// The cell of photo `id`'s copy in album `album`'s section.
fn cell_in(id: PhotoId, album: &AlbumId) -> String {
    format!("thumb:{}/in/{album}", id.get())
}

/// Right-clicks the cell `cell`: the photo menu opens (for the selection when the photo is
/// selected, else for the photo, selected first).
fn open_menu_on(t: &mut T, cell: &str) {
    t.d.click_with(cell, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(try_widget(&t.d, "menuitem:view.edit").is_some(), "the photo menu is open");
}

/// Selects the photos of `cells` as a person does: a click on the first, Ctrl-clicks on the rest.
fn select_cells(t: &mut T, cells: &[String]) {
    for (k, cell) in cells.iter().enumerate() {
        let modifiers = if k == 0 { egui::Modifiers::NONE } else { egui::Modifiers::COMMAND };
        t.d.click_with(cell, &Click { modifiers, ..Click::default() }).unwrap();
    }
    t.d.settle(FRAMES).unwrap();
}

/// The photo menu's model as `ui.menu.tree {context: "photo"}` returns it.
fn model(t: &T) -> Value {
    menus::nodes_json(&menus::photo_menu(t.d.app(), Platform::current()))
}

/// The labels of `rows` ("—" for a separator), and of a submenu's rows after `label ▸`.
fn labels(rows: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for r in rows.as_array().unwrap() {
        if r.get("separator").is_some() {
            out.push("—".into());
            continue;
        }
        let label = r["label"].as_str().unwrap().to_string();
        match r.get("children") {
            Some(kids) => {
                out.push(format!("{label} ▸"));
                out.extend(labels(kids).into_iter().map(|l| format!("  {l}")));
            }
            None => out.push(label),
        }
    }
    out
}

/// Every item row, submenus flattened.
fn items(rows: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for r in rows.as_array().unwrap() {
        match r.get("children") {
            Some(kids) => out.extend(items(kids)),
            None if r.get("id").is_some() => out.push(r.clone()),
            None => {}
        }
    }
    out
}

fn row(rows: &Value, widget: &str) -> Value {
    items(rows).into_iter().find(|r| r["widget"] == widget).unwrap_or_else(|| panic!("no {widget} in {rows}"))
}

fn enabled(rows: &Value, widget: &str) -> (bool, Option<String>) {
    let r = row(rows, widget);
    (r["enabled"].as_bool().unwrap(), r["reason"].as_str().map(str::to_string))
}

/// The rows of `organizing.md` §9.1 in order, Add to Album ▸ holding the albums by name then New
/// Album…; in an album Delete from Disk is Remove from Album, and Show in Folder and Set as Album
/// Cover join.
#[test]
fn the_rows_in_order_and_their_states_in_a_folder_and_in_an_album() {
    let mut t = setup((1400, 900));
    let (a, b, c) = (t.s[0][0], t.s[0][1], t.s[0][2]);
    let (trams, beach) = (t.trams.to_string(), t.beach.to_string());
    let tsec = section(&t.trams);
    t.d.run("select.set", &json!({ "ids": [a.get()] })).unwrap();
    let folder = model(&t);
    let head = ["View and Edit", "Add to Album ▸", "  Beach", "  Trams", "  —", "  New Album…", "—", "Rotate Clockwise", "Rotate Counterclockwise"];
    let tail = ["—", "Find Faces Again", "—", "Properties"];
    let mid = ["—", "Undo All Edits…", "—", "Hide", "—", "Move to New Folder…", "Split Folder Here…", "—", "Open in External Editor", "Open With ▸"];
    let got = labels(&folder);
    assert_eq!(got[..head.len()], head.map(String::from));
    assert_eq!(got[head.len()..head.len() + mid.len()], mid.map(String::from));
    assert_eq!(got[got.len() - tail.len()..], tail.map(String::from));
    assert!(got.contains(&"Delete from Disk…".to_string()) && !got.contains(&"Remove from Album".to_string()));
    for w in ["menuitem:view.edit", "menuitem:album.new", "menuitem:pane.properties", "menuitem:photo.rotate.cw"] {
        assert_eq!(enabled(&folder, w), (true, None), "{w}");
    }
    for album in [&trams, &beach] {
        let r = row(&folder, &format!("menuitem:album.add/{album}"));
        assert_eq!((r["id"].clone(), r["params"].clone(), r["enabled"].clone()), (json!("album.add"), json!({ "album": album }), json!(true)));
    }
    assert!(items(&folder).iter().all(|r| r["id"] != "album.setCover" && r["id"] != "photo.showInFolder"));

    // In an album: the album variant, its rows live on the album shown.
    select_in(&mut t, tsec, &[b], b);
    let album = model(&t);
    let got = labels(&album);
    let at = |l: &str| got.iter().position(|g| g == l).unwrap_or_else(|| panic!("{l} in {got:?}"));
    assert!(at("Revert…") < at("Show in Folder"));
    assert_eq!(
        got[at("Show in Folder")..at("Copy Path") + 1],
        ["Show in Folder", "Set as Album Cover", "Locate on Disk", "Remove from Album", "Copy Path"]
    );
    assert!(!got.contains(&"Delete from Disk…".to_string()));
    for w in ["menuitem:photo.showInFolder", "menuitem:album.setCover", "menuitem:album.remove", "menuitem:view.edit"] {
        assert_eq!(enabled(&album, w), (true, None), "{w}");
    }
    assert_eq!(row(&album, "menuitem:album.remove")["shortcut"], json!("Delete"));
    // A photo of the selection that is not in the album cannot be its cover.
    select_in(&mut t, tsec, &[c], c);
    assert_eq!(enabled(&model(&t), "menuitem:album.setCover"), (false, Some("The photo is not in this album.".into())));
    // Starred Photos, Recently Updated and the search results choose their own photos: no cover,
    // no removing.
    for own in [AlbumSection::Starred, AlbumSection::Recent, AlbumSection::Search] {
        select_in(&mut t, Section::Album(own), &[a], a);
        let rows = model(&t);
        for w in ["menuitem:album.setCover", "menuitem:album.remove"] {
            let (on, why) = enabled(&rows, w);
            assert!(!on && why.unwrap().contains("choose their own photos"), "{own:?}: {w}");
        }
        assert_eq!(enabled(&rows, "menuitem:photo.showInFolder"), (true, None), "{own:?}");
    }
    // Nothing selected: the album rows say why.
    t.d.run("select.none", &json!({})).unwrap();
    assert!(!row(&model(&t), &format!("menuitem:album.add/{trams}"))["enabled"].as_bool().unwrap());

    // The open menu shows the same rows; Add to Album ▸ opens on the albums.
    t.d.run("select.set", &json!({ "ids": [a.get()] })).unwrap();
    open_menu(&mut t, a);
    t.d.click("submenu:Photo/Add to Album").unwrap();
    t.d.settle(FRAMES).unwrap();
    let shown = |d: &Driver, id: &str| try_widget(d, id).unwrap_or_else(|| panic!("{id} is not drawn"));
    assert_eq!(shown(&t.d, &format!("menuitem:album.add/{trams}")).label, "Trams");
    assert_eq!(shown(&t.d, &format!("menuitem:album.add/{beach}")).label, "Beach");
    assert!(shown(&t.d, "menuitem:album.new").enabled);
    assert!(shown(&t.d, &format!("menuitem:album.add/{beach}")).rect[1] < shown(&t.d, &format!("menuitem:album.add/{trams}")).rect[1]);
    assert!(shown(&t.d, &format!("menuitem:album.add/{trams}")).rect[1] < shown(&t.d, "menuitem:album.new").rect[1]);
    t.d.key("Escape").unwrap();
}

/// Remove from Album asks, Cancel keeps the photos, Remove takes them out and leaves the files;
/// with "Remove from albums without asking" on it does not ask.
#[test]
fn remove_from_album_asks_removes_and_leaves_the_files() {
    let mut t = setup((1400, 900));
    let (a, b) = (t.s[0][0], t.s[0][1]);
    let trams = t.trams.clone();
    let library = t.d.app().session().unwrap().library();
    let paths: Vec<PathBuf> = [a, b]
        .iter()
        .map(|id| {
            let photo = library.photo(*id).unwrap();
            library.folder(photo.folder).unwrap().path.join(&photo.name)
        })
        .collect();
    assert!(paths.iter().all(|p| p.is_file()));
    let (cell_a, cell_b) = (cell_in(a, &trams), cell_in(b, &trams));
    select_cells(&mut t, &[cell_a.clone(), cell_b.clone()]);
    assert_eq!(t.d.state().selection.section, Some(section(&trams)), "selected in the album");
    open_menu_on(&mut t, &cell_a);
    t.d.click("menuitem:album.remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    let dialog = try_widget(&t.d, "dialog:removeFromAlbum").expect("Remove from Album asks first");
    assert_eq!(dialog.label, "Remove from Album");
    assert_eq!(t.d.app().session().unwrap().library().album_members(&trams), [a, b], "nothing removed yet");
    t.d.click("button:dialog.cancel").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(try_widget(&t.d, "dialog:removeFromAlbum").is_none());
    assert_eq!(t.d.app().session().unwrap().library().album_members(&trams), [a, b]);
    // Again, and Remove: the album's section empties.
    open_menu_on(&mut t, &cell_a);
    t.d.click("menuitem:album.remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    t.d.click("button:dialog.remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    let library = t.d.app().session().unwrap().library();
    assert!(library.album_members(&trams).is_empty(), "both left the album");
    assert!(library.photo(a).is_some() && library.photo(b).is_some(), "the photos stay in the library");
    for p in &paths {
        assert!(p.is_file(), "{} is still on disk", p.display());
    }
    assert!(
        try_widget(&t.d, &cell_a).is_none() && try_widget(&t.d, &format!("thumb:{}", a.get())).is_some(),
        "out of the album, still in its folder"
    );
    // Undo brings them back; with the setting on, the photos leave without a question.
    t.d.key("Ctrl+Z").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(t.d.app().session().unwrap().library().album_members(&trams), [a, b]);
    let session = t.d.app_mut().session_mut().unwrap();
    let mut o = session.settings().organize();
    o.remove_from_albums_without_asking = true;
    session.change_settings(|st| st.set_organize(&o));
    t.d.settle(FRAMES).unwrap();
    select_cells(&mut t, std::slice::from_ref(&cell_b));
    open_menu_on(&mut t, &cell_b);
    t.d.click("menuitem:album.remove").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(try_widget(&t.d, "dialog:removeFromAlbum").is_none(), "no question");
    assert_eq!(t.d.app().session().unwrap().library().album_members(&trams), [a]);
    assert!(paths[1].is_file());
}

/// Set as Album Cover changes the album's cover (what the tree's album row and `albums.list` show);
/// Show in Folder selects the photo in its folder.
#[test]
fn set_as_album_cover_and_show_in_folder() {
    let mut t = setup((1400, 900));
    let (a, b) = (t.s[0][0], t.s[0][1]);
    let trams = t.trams.clone();
    let cover = |t: &mut T| {
        let list = t.d.run("albums.list", &json!({})).unwrap();
        let row = list.as_array().unwrap().iter().find(|r| r["id"] == trams.to_string()).unwrap().clone();
        row["cover"].as_u64().unwrap()
    };
    assert_eq!(cover(&mut t), a.get(), "the first photo until one is set");
    let cell_b = cell_in(b, &trams);
    open_menu_on(&mut t, &cell_b);
    assert_eq!(t.d.state().selection.section, Some(section(&trams)), "the right-click selected it in the album");
    t.d.click("menuitem:album.setCover").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(cover(&mut t), b.get());
    assert_eq!(t.d.app().session().unwrap().library().album_cover(&trams), Some(b));
    // The album selected as a unit (its tree row) shows the new cover in the tray's token.
    t.d.click(&format!("tree:album:{trams}")).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(try_widget(&t.d, "tray:token").expect("the album token").label, "Album selected — 2 photos");
    let state = t.d.state().clone();
    assert_eq!(dalica_ui::panels::bottom::tray::token_cover(&state, t.d.app().session().unwrap()), Some(b));
    // Show in Folder: the photo alone, selected in its folder's section.
    select_cells(&mut t, std::slice::from_ref(&cell_b));
    open_menu_on(&mut t, &cell_b);
    t.d.click("menuitem:photo.showInFolder").unwrap();
    t.d.settle(FRAMES).unwrap();
    let folder = t.d.app().session().unwrap().library().photo(b).unwrap().folder;
    let sel = &t.d.state().selection;
    assert_eq!((sel.ids.as_slice(), sel.section), ([b].as_slice(), Some(Section::Folder(folder))));
}

/// The pixels of `shot` in `[x0, x1) × [y0, y1)` (clamped to the image): width, height, RGBA.
fn crop(shot: &dalica_ui_kit::Image, [x0, y0, x1, y1]: [f32; 4]) -> (u32, u32, Vec<u8>) {
    let (x0, y0) = (x0.max(0.0).round() as u32, y0.max(0.0).round() as u32);
    let (x1, y1) = ((x1.max(0.0).round() as u32).min(shot.width).max(x0), (y1.max(0.0).round() as u32).min(shot.height).max(y0));
    let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    (x1 - x0, y1 - y0, rgba)
}

/// The bounding box of `rects`, with a margin.
fn bounds(rects: &[[f32; 4]], margin: f32) -> [f32; 4] {
    let x0 = rects.iter().map(|r| r[0]).fold(f32::MAX, f32::min) - margin;
    let y0 = rects.iter().map(|r| r[1]).fold(f32::MAX, f32::min) - margin;
    let x1 = rects.iter().map(|r| r[0] + r[2]).fold(0.0, f32::max) + margin;
    let y1 = rects.iter().map(|r| r[1] + r[3]).fold(0.0, f32::max) + margin;
    [x0, y0, x1, y1]
}

/// The width of the photo menu's label column that is fingerprinted: the shortcut column is left
/// out, as macOS writes Cmd and so draws the menu wider.
const LABELS: f32 = 182.0;

/// Checks the open menus of the last frame against `tests/fingerprints/<name>.json`: the photo
/// menu's label column (its rows' left part, [`LABELS`] wide) over the open Add to Album ▸ (its
/// rows' bounding box), inside the menus' frames so nothing behind them shows, stacked into one
/// image.
fn check_menu(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let rows: Vec<(String, [f32; 4])> =
        d.widgets().into_iter().filter(|w| w.id.starts_with("menuitem:") || w.id.starts_with("submenu:")).map(|w| (w.id, w.rect)).collect();
    let in_sub = |id: &str| id == "menuitem:album.new" || id.starts_with("menuitem:album.add/");
    let main: Vec<[f32; 4]> = rows.iter().filter(|(id, _)| !in_sub(id)).map(|(_, r)| *r).collect();
    let sub: Vec<[f32; 4]> = rows.iter().filter(|(id, _)| in_sub(id)).map(|(_, r)| *r).collect();
    let mut m = bounds(&main, 0.0);
    m[2] = m[0] + LABELS;
    let parts = [crop(&shot, m), crop(&shot, bounds(&sub, 0.0))];
    let w = parts.iter().map(|p| p.0).max().unwrap();
    let h: u32 = parts.iter().map(|p| p.1).sum();
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for (pw, ph, px) in &parts {
        for y in 0..*ph {
            let row = &px[(y * pw * 4) as usize..((y + 1) * pw * 4) as usize];
            rgba.extend_from_slice(row);
            rgba.extend(std::iter::repeat_n([0u8, 0, 0, 255], (w - pw) as usize).flatten());
        }
    }
    check_pixels(name, w, h, &rgba);
}

/// The photo menu with Add to Album ▸ open: View and Edit bold, the albums by name over New
/// Album…. (One size: the menu does not depend on the window's.)
#[test]
fn the_open_menu_matches_its_fingerprint() {
    let mut t = setup(LARGE);
    let a = t.s[0][0];
    t.d.run("select.set", &json!({ "ids": [a.get()] })).unwrap();
    open_menu(&mut t, a);
    t.d.click("submenu:Photo/Add to Album").unwrap();
    t.d.settle(FRAMES).unwrap();
    // Rest the pointer on the submenu's first album, as a person choosing one would.
    t.d.hover(&format!("menuitem:album.add/{}", t.beach), 0).unwrap();
    t.d.settle(FRAMES).unwrap();
    check_menu(&t.d, "menu-photo-1400x900");
}
