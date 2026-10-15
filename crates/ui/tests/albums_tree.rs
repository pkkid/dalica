//! Albums in the library tree and on the light table (M9.17, `organizing.md` §4, `ui-layout.md`
//! §4 and §5), driven headlessly on a temp library of two folders:
//!
//! - New Album (the toolbar's button) with two photos in the tray opens Album Properties; OK
//!   creates the album with both, selects it in the tree as a unit, and its section shows both,
//!   each as `thumb:<id>/in/<album>` beside its folder's `thumb:<id>`; a click on such a copy
//!   selects the photo in the album;
//! - on an album section the fourth menu reads Album, with Album Properties… and Delete Album…
//!   live; Delete Album… asks, deletes (the photos stay) and Edit ▸ Undo brings the album back;
//! - Starred Photos lists the starred photos (a row and a section); Recently Updated waits for the
//!   M9 review (`light_table::SHOW_RECENTLY_UPDATED`);
//! - in search mode the search album's row comes first and the headers show a magnifier;
//! - the tree region matches its fingerprint at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/region-tree-albums-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{AlbumId, Date, DateTime, FolderId, PhotoId};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::headless::Click;
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::panels::folders;
use dalica_ui::state::{AlbumKey, AlbumSection, DialogKind, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry;
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A JPEG of `seed` taken on `day` at 10:00.
fn put_jpeg(path: &Path, seed: u64, (y, m, d): (u16, u8, u8)) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let taken = DateTime::new(Date::new(y, m, d).unwrap(), 10, 0, 0).unwrap();
    let exif = dalica_scenes::Exif { date_time_original: Some(taken), ..Default::default() };
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, Some(&exif)).unwrap();
    std::fs::write(path, jpeg).unwrap();
}

struct Fx {
    _dir: tempfile::TempDir,
    /// `Lisbon Weekend` (three photos, 2026-06-04 to 06) and `Porto` (two, 2026-05-01).
    lisbon: FolderId,
    porto: FolderId,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    for (i, day) in [(2026, 6, 5), (2026, 6, 4), (2026, 6, 6)].into_iter().enumerate() {
        put_jpeg(&photos.join("Lisbon Weekend").join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    for i in 0..2 {
        put_jpeg(&photos.join("Porto").join(format!("IMG_{i:04}.jpg")), 10 + i, (2026, 5, 1));
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in ["Lisbon Weekend", "Porto"] {
        s.execute("folders.add", &json!({ "path": photos.join(f) })).unwrap();
    }
    s.wait_idle();
    let lisbon = s.library().folder_by_path(&photos.join("Lisbon Weekend")).unwrap();
    let porto = s.library().folder_by_path(&photos.join("Porto")).unwrap();
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, lisbon, porto, d }
}

fn widget(d: &Driver, id: &str) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == id)
}

fn has(d: &Driver, id: &str) -> bool {
    widget(d, id).is_some()
}

/// The photos of `section` as the light table shows them; `None` when it has no section.
fn photos_of(d: &Driver, section: Section) -> Option<Vec<PhotoId>> {
    d.app().panels().unwrap().sections.get(section).map(|s| s.photos.clone())
}

fn album_section(id: &AlbumId) -> Section {
    Section::Album(AlbumSection::User(AlbumKey::of(id)))
}

/// The only user album the library has.
fn the_album(d: &Driver) -> AlbumId {
    let library = d.app().session().unwrap().library();
    let ids: Vec<AlbumId> = library.albums().map(|(id, _)| id.clone()).collect();
    assert_eq!(ids.len(), 1, "{ids:?}");
    ids[0].clone()
}

/// The tree's album rows, top to bottom.
fn album_rows(d: &Driver) -> Vec<String> {
    let mut rows: Vec<WidgetInfo> = d.widgets().into_iter().filter(|w| w.id.starts_with("tree:album:")).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.into_iter().map(|w| w.id).collect()
}

/// The bar's item at `path` (the fourth menu's title as shown: `Album` in an album).
fn menu_item(d: &Driver, path: &[&str]) -> menus::ItemNode {
    let bar = menus::bar(d.app(), Platform::Other);
    menus::find(&bar, &Lookup::Path(path.iter().map(|p| p.to_string()).collect())).unwrap_or_else(|| panic!("no {path:?}")).clone()
}

fn fourth_title(d: &Driver) -> &'static str {
    menus::bar(d.app(), Platform::Other).iter().find(|m| m.key == "Folder").unwrap().title
}

fn undo_len(d: &Driver) -> usize {
    d.app().session().unwrap().undo_stack().len()
}

/// No unregistered interactive widget, unique ids in the grammar.
fn check_ids(d: &Driver, case: &str) {
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{case}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for w in d.widgets() {
        assert!(registry::is_valid_id(&w.id), "{case}: `{}` breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{case}: `{}` is registered twice", w.id);
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the region widget `id` covers against `tests/fingerprints/<name>.json`, writing it and
/// the whole window to the test output (and the window to `$DALICA_SHOTS` when set).
fn check_region(d: &Driver, id: &str, name: &str) {
    let s = d.screenshot().unwrap();
    let window = fingerprint::png_rgba(s.width, s.height, &s.rgba);
    std::fs::write(out_dir().join(format!("{name}-window.png")), &window).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}-window.png")), &window).unwrap();
    }
    let r = widget(d, id).unwrap_or_else(|| panic!("no widget {id}")).rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&s.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// New Album with two photos in the tray: Album Properties, OK, the album selected in the tree and
/// its section showing both photos.
#[test]
fn new_album_with_two_photos_in_the_tray_creates_selects_and_shows_it() {
    let mut fx = fixture((1400, 900));
    let lisbon = photos_of(&fx.d, Section::Folder(fx.lisbon)).unwrap();
    let two = [lisbon[0], lisbon[2]];
    fx.d.run("select.set", &json!({ "ids": [two[0].get(), two[1].get()] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(dalica_ui::tray::photos(fx.d.state()), two, "the tray shows the selection");

    // The toolbar's New Album opens Album Properties for the tray's photos, Name selected.
    fx.d.click("button:toolbar.newAlbum").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::AlbumProperties));
    let draft = fx.d.state().album_properties.clone().unwrap();
    assert_eq!((draft.album, draft.ids.clone(), draft.name.as_str(), draft.use_newest), (None, two.to_vec(), "New Album", true));
    assert_eq!(draft.date, "2026-06-06", "the newest photo's date");
    for id in [
        "dialog:albumProperties",
        "field:dialog.name",
        "field:dialog.date",
        "check:dialog.useNewest",
        "field:dialog.place",
        "field:dialog.description",
    ] {
        assert!(has(&fx.d, id), "{id}");
    }
    check_ids(&fx.d, "Album Properties");
    // Typing replaces the selected name.
    fx.d.type_text("Lisbon Best").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().album_properties.as_ref().unwrap().name, "Lisbon Best");
    let steps = undo_len(&fx.d);
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!(undo_len(&fx.d), steps + 1, "one undo step");

    // The album, with both photos in the order given.
    let album = the_album(&fx.d);
    let library = fx.d.app().session().unwrap().library();
    assert_eq!(library.album(&album).unwrap().name, "Lisbon Best");
    assert_eq!(library.album_members(&album), two);
    // Selected in the tree as a unit, its section showing both.
    let section = album_section(&album);
    let row = widget(&fx.d, &format!("tree:album:{album}")).expect("its tree row");
    assert_eq!(row.label, "Lisbon Best (2)");
    assert!(row.active, "its row is the selected one");
    assert!(!widget(&fx.d, &format!("tree:folder:{}", fx.lisbon)).unwrap().active, "no folder row is");
    let sel = &fx.d.state().selection;
    assert_eq!((sel.section, sel.whole, sel.ids.clone()), (Some(section), true, two.to_vec()));
    assert_eq!(photos_of(&fx.d, section), Some(two.to_vec()));
    assert_eq!(fx.d.app().panels().unwrap().light_table.frame_info().headers[0].0, section, "albums come first");
    assert!(widget(&fx.d, &format!("section:album:{album}")).unwrap().active, "its header is the selected one");
    for id in two {
        assert!(has(&fx.d, &format!("thumb:{id}/in/{album}")), "{id}: its copy in the album");
    }
    check_ids(&fx.d, "album created");

    // On an album section the fourth menu reads Album, its rows live.
    assert_eq!(fourth_title(&fx.d), "Album");
    for path in [["Album", "Album Properties…"], ["Album", "Delete Album…"]] {
        let item = menu_item(&fx.d, &path);
        assert!(item.enabled, "{path:?}: {:?}", item.reason);
    }

    // A click on a copy in the album selects the photo there, not in its folder.
    fx.d.click(&format!("thumb:{}/in/{album}", two[1])).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let sel = &fx.d.state().selection;
    assert_eq!((sel.ids.clone(), sel.section, sel.whole), (vec![two[1]], Some(section), false));
    assert!(widget(&fx.d, &format!("thumb:{}/in/{album}", two[1])).unwrap().active, "the album's copy is the selected one");
    assert!(!widget(&fx.d, &format!("thumb:{}", two[1])).unwrap().active, "its folder's copy is not");
    assert_eq!(fourth_title(&fx.d), "Album");
    // And on its folder's copy, in the folder.
    fx.d.click(&format!("thumb:{}", two[1])).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().selection.section, Some(Section::Folder(fx.lisbon)));
    assert_eq!(fourth_title(&fx.d), "Folder");
}

/// Delete Album… asks, deletes the album (the photos stay), and Edit ▸ Undo brings it back.
#[test]
fn delete_album_asks_deletes_and_undo_brings_it_back() {
    let mut fx = fixture((1400, 900));
    let porto = photos_of(&fx.d, Section::Folder(fx.porto)).unwrap();
    let made = fx.d.run("album.create", &json!({ "name": "Porto Days", "ids": [porto[0].get(), porto[1].get()] })).unwrap();
    let album = AlbumId::parse(made["album"].as_str().unwrap()).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let row = format!("tree:album:{album}");
    fx.d.click(&row).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().selection.section, Some(album_section(&album)));

    // Album ▸ Delete Album… asks through the host, naming the album.
    let item = menu_item(&fx.d, &["Album", "Delete Album…"]);
    assert_eq!(item.dialog, Some("deleteAlbum"));
    let reply = menus::activate(fx.d.app_mut(), &item, &Value::Null).unwrap();
    assert_eq!(reply, json!({ "dialog": "deleteAlbum" }));
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::DeleteAlbum));
    let text = dalica_ui::dialogs::confirm::question(fx.d.app(), fx.d.state().dialogs.last().unwrap());
    assert!(text.contains("\"Porto Days\""), "the question names the album: {text}");
    assert!(fx.d.app().session().unwrap().library().album(&album).is_some(), "nothing is deleted before the answer");
    fx.d.click("button:dialog.deleteAlbum").unwrap();
    fx.d.settle(FRAMES).unwrap();
    let library = fx.d.app().session().unwrap().library();
    assert!(library.album(&album).is_none(), "deleted");
    assert!(porto.iter().all(|id| library.photo(*id).is_some()), "the photos stay");
    assert!(!has(&fx.d, &row) && photos_of(&fx.d, album_section(&album)).is_none(), "its row and section are gone");

    // Edit ▸ Undo brings it back, with its photos.
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.app().session().unwrap().library().album_members(&album), porto);
    assert_eq!(widget(&fx.d, &row).unwrap().label, "Porto Days (2)");
    assert_eq!(photos_of(&fx.d, album_section(&album)), Some(porto));
}

/// Starred Photos lists the starred photos, by date (a row and a section); the user albums
/// follow it. Recently Updated is left out until the M9 review answers how it counts
/// (`light_table::SHOW_RECENTLY_UPDATED`).
#[test]
fn starred_photos_and_recently_updated_list_as_expected() {
    let mut fx = fixture((1400, 900));
    // Nothing starred and no album: no Albums group; Recently Updated waits for the review.
    const { assert!(!dalica_ui::panels::light_table::SHOW_RECENTLY_UPDATED) };
    assert!(album_rows(&fx.d).is_empty());
    assert!(!has(&fx.d, "tree:group:albums") && !has(&fx.d, "tree:album:recent"));
    assert_eq!(photos_of(&fx.d, Section::Album(AlbumSection::Recent)), None);

    // Two photos starred: Starred Photos, by date (the Porto one is older).
    let lisbon = photos_of(&fx.d, Section::Folder(fx.lisbon)).unwrap();
    let porto = photos_of(&fx.d, Section::Folder(fx.porto)).unwrap();
    fx.d.run("photo.star.set", &json!({ "ids": [lisbon[1].get(), porto[0].get()], "on": true })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(album_rows(&fx.d), ["tree:album:starred"]);
    assert_eq!(widget(&fx.d, "tree:album:starred").unwrap().label, "Starred Photos (2)");
    assert_eq!(widget(&fx.d, "tree:group:albums").unwrap().label, "Albums (1)");
    let starred = Section::Album(AlbumSection::Starred);
    assert_eq!(photos_of(&fx.d, starred), Some(vec![porto[0], lisbon[1]]));
    assert!(!has(&fx.d, "section:album:starred/description"), "the automatic albums have no description");

    // A user album joins after it.
    fx.d.run("album.create", &json!({ "name": "Mixed", "ids": [porto[1].get()] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let album = the_album(&fx.d);
    assert_eq!(album_rows(&fx.d), ["tree:album:starred".to_string(), format!("tree:album:{album}")]);
    assert_eq!(widget(&fx.d, "tree:group:albums").unwrap().label, "Albums (2)");

    // A click on Starred Photos selects it as a unit and scrolls to it.
    fx.d.click("tree:album:starred").unwrap();
    fx.d.settle(FRAMES).unwrap();
    let sel = fx.d.state().selection.clone();
    assert_eq!((sel.section, sel.whole, sel.ids), (Some(starred), true, vec![porto[0], lisbon[1]]));
    assert_eq!(fx.d.state().scroll.section, Some(starred));
    assert!(widget(&fx.d, "tree:album:starred").unwrap().active);
    // ↓ in the focused tree moves to the user album, then to the first folder.
    fx.d.key("Down").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().selection.section, Some(album_section(&album)));
    assert_eq!(fx.d.state().scroll.section, Some(album_section(&album)));
    fx.d.key("Down").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(matches!(fx.d.state().selection.section, Some(Section::Folder(_))));
    assert!(!widget(&fx.d, &format!("tree:album:{album}")).unwrap().active);
    check_ids(&fx.d, "albums");
}

/// Search mode: the search album is the first row, highlighted, and the headers show a magnifier.
#[test]
fn the_search_album_comes_first_while_searching() {
    let mut fx = fixture((1400, 900));
    fx.d.run("search.set", &json!({ "text": "Porto" })).unwrap();
    fx.d.app_mut().session_mut().unwrap().wait_search();
    fx.d.settle(FRAMES).unwrap();
    let rows = album_rows(&fx.d);
    assert_eq!(rows.first().map(String::as_str), Some("tree:album:search"), "{rows:?}");
    let row = widget(&fx.d, "tree:album:search").unwrap();
    assert_eq!(row.label, "Search results for \"Porto\" (2)");
    assert!(folders::current_model(fx.d.ctx()).search, "the headers show a magnifier");
    assert_eq!(photos_of(&fx.d, Section::Album(AlbumSection::Search)).map(|p| p.len()), Some(2));
    assert!(has(&fx.d, "tree:group:albums"), "a header keeps its id in search mode");
    check_ids(&fx.d, "search");
    fx.d.run("search.clear", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(!has(&fx.d, "tree:album:search"));
    assert!(!folders::current_model(fx.d.ctx()).search);
}

/// A double-click on an album's row opens its Album Properties.
#[test]
fn a_double_click_on_an_album_row_opens_its_properties() {
    let mut fx = fixture((1400, 900));
    let porto = photos_of(&fx.d, Section::Folder(fx.porto)).unwrap();
    fx.d.run("album.create", &json!({ "name": "Harbour", "ids": [porto[0].get()] })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let album = the_album(&fx.d);
    fx.d.click_with(&format!("tree:album:{album}"), &Click { count: 2, ..Click::default() }).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::AlbumProperties));
    let draft = fx.d.state().album_properties.clone().unwrap();
    assert_eq!((draft.album, draft.name.as_str()), (Some(album), "Harbour"));
    fx.d.key("Escape").unwrap();
    // The automatic albums have no properties: a double-click opens nothing.
    fx.d.run("photo.star.set", &json!({ "ids": [porto[1].get()], "on": true })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click_with("tree:album:starred", &Click { count: 2, ..Click::default() }).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty());
}

/// The tree with an album selected and Starred Photos, at both sizes.
#[test]
fn the_tree_with_albums_matches_its_fingerprints() {
    for size in SIZES {
        let mut fx = fixture(size);
        let porto = photos_of(&fx.d, Section::Folder(fx.porto)).unwrap();
        fx.d.run("photo.star.set", &json!({ "ids": [porto[0].get()], "on": true })).unwrap();
        fx.d.run("album.create", &json!({ "name": "Weekend in Lisbon and Porto", "ids": [porto[1].get()] })).unwrap();
        fx.d.settle(FRAMES).unwrap();
        let album = the_album(&fx.d);
        fx.d.click(&format!("tree:album:{album}")).unwrap();
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d, &format!("{}×{}", size.0, size.1));
        check_region(&fx.d, "region:tree", &format!("region-tree-albums-{}x{}", size.0, size.1));
    }
}

/// `select.set {section}`: photos selected in an album's section; a section that is not one, not
/// shown, or not showing a photo is refused, and nothing changes.
#[test]
fn select_set_in_a_section_refuses_what_it_does_not_show() {
    use dalica_engine::CmdError;
    let mut fx = fixture((1400, 900));
    let lisbon = photos_of(&fx.d, Section::Folder(fx.lisbon)).unwrap();
    let porto = photos_of(&fx.d, Section::Folder(fx.porto)).unwrap();
    let made = fx.d.run("album.create", &json!({ "name": "Two", "ids": [lisbon[0].get(), lisbon[1].get()] })).unwrap();
    let album = AlbumId::parse(made["album"].as_str().unwrap()).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let section = album_section(&album);
    fx.d.run("select.set", &json!({ "ids": [lisbon[1].get()], "section": section })).unwrap();
    assert_eq!((fx.d.state().selection.ids.clone(), fx.d.state().selection.section), (vec![lisbon[1]], Some(section)));
    // Shift: a range inside the album, from the anchor.
    fx.d.run("select.set", &json!({ "ids": [lisbon[0].get()], "mode": "range", "section": section })).unwrap();
    assert_eq!(fx.d.state().selection.ids, [lisbon[1], lisbon[0]]);
    assert_eq!(fx.d.state().selection.section, Some(section));
    let before = fx.d.state().selection.clone();
    let refused = |d: &mut Driver, params: Value| d.app_mut().run("select.set", &params).unwrap_err();
    assert!(matches!(refused(&mut fx.d, json!({ "ids": [porto[0].get()], "section": section })), CmdError::NotFound(_)), "not in the album");
    assert!(matches!(refused(&mut fx.d, json!({ "ids": [lisbon[0].get()], "section": "album:a0000" })), CmdError::NotFound(_)), "no such album");
    assert!(matches!(refused(&mut fx.d, json!({ "ids": [lisbon[0].get()], "section": "album:recent" })), CmdError::NotFound(_)), "not shown");
    for bad in
        [json!({ "ids": [lisbon[0].get()], "section": "album:" }), json!({ "ids": [1], "section": 12 }), json!({ "ids": [1], "section": "folder:x" })]
    {
        assert!(matches!(refused(&mut fx.d, bad.clone()), CmdError::BadParams(_)), "{bad}");
    }
    assert_eq!(fx.d.state().selection, before, "nothing changed");
}
