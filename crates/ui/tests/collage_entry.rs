//! Where a collage starts (M14.20, `creating.md` §1 and §3, `ui-layout.md` §5–§6), driven
//! headlessly by automation id on a library of eight scenes photos, Ana's face on the first two:
//!
//! - the bottom bar's Collage with three held photos (nothing selected) opens the room with those
//!   three clips, and Create ▸ Picture Collage… takes the same tray (held and selected);
//! - a folder header's Collage with nothing selected takes the whole folder, with two of its photos
//!   selected those two; a person section's takes the person's photos;
//! - a created collage shows Edit Collage in the edit view's top bar, which reopens the room with
//!   its clips; an ordinary photo shows none; the Collages folder sits in the Projects group with
//!   the project icon;
//! - at 1400 × 900 the top bar's right end with Edit Collage matches
//!   `tests/fingerprints/collage-entry-edit-collage-1400x900.json` (`DALICA_BLESS=1` stores a new
//!   one; renders go to `target/test-output/ui/`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::{FolderId, PersonId, PhotoId};
use dalica_engine::Session;
use dalica_ui::panels::collage::settings;
use dalica_ui::panels::edit::top_bar;
use dalica_ui::panels::folders;
use dalica_ui::state::{Room, View};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    /// The photos in id order.
    photos: Vec<PhotoId>,
    folder: FolderId,
    ana: PersonId,
}

/// Eight scenes photos in one folder, Ana's face on the first two; the pictures folder (where
/// collages go) inside the temporary directory; the window at 1400 × 900.
fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files = crate::common::demo_library(1, 8, 21).unwrap();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder_path = root.join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(root.join("Pictures")));
    s.execute("folders.add", &json!({ "path": folder_path })).unwrap();
    s.wait_idle();
    let mut photos: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    photos.sort_unstable();
    assert_eq!(photos.len(), 8);
    let folder = s.library().folder_by_path(&folder_path).expect("the folder");
    let r = s.execute("people.create", &json!({ "name": "Ana" })).unwrap();
    let ana = PersonId::parse(r["person"].as_str().unwrap()).unwrap();
    for id in &photos[..2] {
        s.execute("faces.add", &json!({ "id": id, "rect": [0.3, 0.15, 0.65, 0.75], "person": ana })).unwrap();
    }
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, photos, folder, ana }
}

fn run(d: &mut Driver, id: &str, params: Value) -> Value {
    let v = d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    d.settle(FRAMES).unwrap();
    v
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    d.settle(FRAMES).unwrap();
}

/// The open collage's clips, in order; empty with none open.
fn clips(d: &mut Driver) -> Vec<PhotoId> {
    let c = d.app_mut().session_mut().unwrap().execute("collage.inspect", &json!({})).unwrap();
    c["clips"].as_array().map_or_else(Vec::new, |a| a.iter().map(|c| PhotoId::new(c["id"].as_u64().unwrap())).collect())
}

/// The room is in front with the collage of exactly `want` (in any order); then it is closed,
/// its changes discarded.
fn opened_with(d: &mut Driver, want: &[PhotoId]) {
    assert_eq!(d.state().room, Room::Collage, "the room is in front");
    let mut got = clips(d);
    got.sort_unstable();
    let mut want = want.to_vec();
    want.sort_unstable();
    assert_eq!(got, want);
    run(d, "collage.closeRoom", json!({ "discard": true }));
    assert!(!d.state().rooms.contains(&Room::Collage));
}

/// Steps frames until `done`, giving the workers real time (a 3000 px render in a debug build
/// takes seconds), for at most two minutes.
fn until(d: &mut Driver, done: impl Fn(&Driver) -> bool) {
    let start = std::time::Instant::now();
    while !done(d) {
        assert!(start.elapsed() < std::time::Duration::from_secs(120), "timed out: {:?}", d.app().session().unwrap().jobs());
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    d.settle(FRAMES).unwrap();
}

#[test]
fn the_bottom_bar_and_the_section_headers_open_the_room_with_the_right_clips() {
    let mut t = setup();
    let p = t.photos.clone();
    // The bottom bar: three held photos and nothing selected.
    run(&mut t.d, "select.set", json!({ "ids": [p[2], p[4], p[6]] }));
    run(&mut t.d, "tray.hold", json!({}));
    run(&mut t.d, "select.set", json!({ "ids": [] }));
    assert!(widget(&t.d, "action:collage").enabled, "the held photos");
    shot(&t.d, "collage-entry-tray-1400x900");
    click(&mut t.d, "action:collage");
    opened_with(&mut t.d, &[p[2], p[4], p[6]]);
    // Create ▸ Picture Collage… takes the same tray: the held photos and one selected.
    run(&mut t.d, "select.set", json!({ "ids": [p[7]] }));
    click(&mut t.d, "menu:Create");
    click(&mut t.d, "menuitem:create.collage");
    opened_with(&mut t.d, &[p[2], p[4], p[6], p[7]]);
    run(&mut t.d, "tray.clear", json!({ "confirm": true }));

    // A folder header with nothing selected: the whole folder; with two of its photos, those.
    run(&mut t.d, "select.set", json!({ "ids": [] }));
    let header = format!("section:folder:{}/collage", t.folder);
    assert!(widget(&t.d, &header).enabled);
    click(&mut t.d, &header);
    opened_with(&mut t.d, &p);
    run(&mut t.d, "select.set", json!({ "ids": [p[0], p[5]] }));
    click(&mut t.d, &header);
    opened_with(&mut t.d, &[p[0], p[5]]);

    // A person section: her photos.
    click(&mut t.d, &format!("tree:person:{}", t.ana));
    let header = format!("section:person:{}/collage", t.ana);
    assert!(widget(&t.d, &header).enabled, "live in a person's section");
    shot(&t.d, "collage-entry-person-1400x900");
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    assert!(widget(&t.d, &header).enabled);
    shot(&t.d, "collage-entry-person-900x600");
    click(&mut t.d, &header);
    opened_with(&mut t.d, &p[..2]);
}

#[test]
fn a_created_collage_shows_edit_collage_which_reopens_the_room() {
    let mut t = setup();
    let p = t.photos.clone();
    let d = &mut t.d;
    // An ordinary photo shows no Edit Collage, once the engine has looked.
    run(d, "view.edit", json!({ "id": p[0] }));
    until(d, |d| d.app().session().unwrap().jobs().iter().all(|j| j.state.is_finished()));
    assert!(try_widget(d, top_bar::EDIT_COLLAGE_ID).is_none(), "an ordinary photo");
    run(d, "view.library", json!({}));

    // Three photos make a collage; it opens in the edit view with Edit Collage.
    run(d, "select.set", json!({ "ids": [p[1], p[3], p[5]] }));
    click(d, "action:collage");
    click(d, settings::CREATE_ID);
    until(d, |d| d.app().collage().job().is_none() && d.state().view == View::Edit);
    let made = d.state().edit.photo.expect("the new collage in the edit view");
    assert!(!p.contains(&made));
    until(d, |d| try_widget(d, top_bar::EDIT_COLLAGE_ID).is_some());
    let button = widget(d, top_bar::EDIT_COLLAGE_ID);
    assert_eq!((button.label.as_str(), button.enabled), (top_bar::EDIT_COLLAGE, true));
    // The bar's right end: Edit Collage left of the compare toggle.
    let bar = widget(d, "region:editTopBar").rect;
    let r = button.rect;
    check_rect(d, "collage-entry-edit-collage-1400x900", [r[0] - top_bar::GAP, bar[1], bar[0] + bar[2] - (r[0] - top_bar::GAP), bar[3]]);

    // At 900 × 600 it still shows, the filmstrip giving way.
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    check_fits(d, "the edit view of a collage");
    let (b, film) = (widget(d, top_bar::EDIT_COLLAGE_ID).rect, widget(d, "region:filmstrip").rect);
    assert!(film[0] + film[2] <= b[0], "the filmstrip ends left of Edit Collage: {film:?} {b:?}");
    shot(d, "collage-entry-edit-collage-900x600");
    d.resize(LARGE.0, LARGE.1);
    d.settle(FRAMES).unwrap();

    // The room closed, Edit Collage reopens it from the recipe with the three clips.
    run(d, "collage.closeRoom", json!({ "discard": true }));
    assert_eq!(d.state().view, View::Edit);
    click(d, top_bar::EDIT_COLLAGE_ID);
    until(d, |d| d.state().room == Room::Collage && d.app().session().unwrap().collage().is_open());
    opened_with(d, &[p[1], p[3], p[5]]);

    // The Collages folder sits in the Projects group, with the project icon.
    run(d, "view.library", json!({}));
    let lib = d.app().session().unwrap().library();
    let folder = lib.photo(made).unwrap().folder;
    let path = lib.folder(folder).unwrap().path.clone();
    assert_eq!(path, t._dir.path().join("Pictures").join("Dalica").join("Collages"));
    assert_eq!(lib.project(folder), Some(dalica_library::Project::Collages));
    assert_eq!(widget(d, "tree:group:projects").label, "Projects (1)");
    let id = format!("tree:folder:{folder}");
    assert_eq!(widget(d, &id).label, "Collages (1)");
    let model = folders::current_model(d.ctx());
    let row = (0..model.items.len()).filter_map(|i| model.row(i)).find(|r| r.id == id).expect("the Collages row");
    assert!(row.project, "drawn with the project icon");
    shot(d, "collage-entry-projects-1400x900");
    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    shot(d, "collage-entry-projects-900x600");
}
