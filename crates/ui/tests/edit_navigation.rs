//! Entering, leaving and moving between photos in the edit view (`editing.md` §1–§2, §12), driven
//! through the control channel as an agent would: every way in (by id, Enter, Ctrl+3, a
//! double-click on a thumbnail, Picture ▸ View and Edit) and out (Esc, Ctrl+1, Back to Library,
//! View ▸ Library View); the library keeps its scroll and selection and focuses the photo last
//! viewed; ← / →, PgUp / PgDn, Home / End step through the section's photos in light-table order
//! and stay put at the ends (hidden photos only while shown); a filmstrip click opens that photo and
//! recentres the strip; Ctrl+R, Ctrl+Z, Ctrl+Y, the flips and Ctrl+Shift+E are steps of the photo's
//! own history and leave the library undo stack alone; a caption typed in the caption bar is one
//! library step; the status line says "Folder › name … (n of N)"; the top bar, caption bar and
//! status line match their fingerprints at 1400 × 900 and 900 × 600.
//!
//! `DALICA_SHOTS=<dir>` writes the regions' screenshots there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, out_dir};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use serde_json::{Value, json};

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The photos in light-table order.
    order: Vec<u64>,
}

/// A library under a temporary directory holding one folder of `n` scenes photos, the window at
/// `size`, and the photos in the order the light table shows them.
fn setup((w, h): (u32, u32), n: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, n, 11).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let control = Control::new(Config::default());
    // Tall enough for every cell to be laid out on screen while the order is read.
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 2000, 1.0);
    d.settle(FRAMES).unwrap();
    let mut cells: Vec<(f32, f32, u64)> = d
        .widgets()
        .into_iter()
        .filter_map(|w| w.id.strip_prefix("thumb:").and_then(|id| id.parse().ok()).map(|id| (w.rect[1], w.rect[0], id)))
        .collect();
    cells.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let order: Vec<u64> = cells.into_iter().map(|c| c.2).collect();
    assert_eq!(order.len(), n, "every photo has a cell");
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, order };
    ok(&mut t, "ui.resize", json!({ "width": w, "height": h }));
    t.d.settle(FRAMES).unwrap();
    t
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

fn state(t: &mut T) -> Value {
    ok(t, "ui.state", json!({}))
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = ok(t, "engine.execute", json!({ "command": id, "params": params }));
    t.d.settle(FRAMES).unwrap();
    v
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    t.d.settle(FRAMES).unwrap();
}

fn click(t: &mut T, id: &str, count: u32) {
    ok(t, "ui.clickWidget", json!({ "id": id, "count": count }));
    t.d.settle(FRAMES).unwrap();
}

fn menu(t: &mut T, path: &str) {
    ok(t, "ui.menu.invoke", json!({ "path": path, "wait": true }));
    t.d.settle(FRAMES).unwrap();
}

fn widget(t: &T, id: &str) -> WidgetInfo {
    t.d.widgets()
        .into_iter()
        .find(|w| w.id == id)
        .unwrap_or_else(|| panic!("no `{id}` in {:?}", t.d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

/// The view and the photo shown (`null` in the library).
fn shown(t: &mut T) -> (String, Value) {
    let s = state(t);
    (s["view"].as_str().unwrap().to_string(), s["edit"]["photo"].clone())
}

fn editing(t: &mut T, photo: u64) {
    assert_eq!(shown(t), ("edit".to_string(), json!(photo)));
}

fn in_library(t: &mut T) {
    assert_eq!(shown(t), ("library".to_string(), Value::Null));
}

/// The selection's ids and anchor.
fn selection(t: &mut T) -> (Value, Value) {
    let s = state(t);
    (s["selection"]["ids"].clone(), s["selection"]["anchor"].clone())
}

fn select(t: &mut T, ids: &[u64]) {
    run(t, "select.set", json!({ "ids": ids }));
}

fn library_caption(t: &T, id: u64) -> String {
    t.d.app().session().unwrap().library().caption(dalica_core::PhotoId::new(id)).to_string()
}

fn photo_edits(t: &mut T, id: u64) -> Value {
    run(t, "photo.history.state", json!({ "id": id }))["edits"].clone()
}

/// Every way in and out: by id, Enter, Ctrl+3, a double-click, Picture ▸ View and Edit; Esc,
/// Ctrl+1, Back to Library, View ▸ Library View. Each way out leaves the photo shown focused.
#[test]
fn each_way_in_and_out_by_id_key_and_menu() {
    let mut t = setup((1400, 900), 8);
    let o = t.order.clone();

    run(&mut t, "view.edit", json!({ "id": o[1] }));
    editing(&mut t, o[1]);
    run(&mut t, "view.library", json!({}));
    in_library(&mut t);

    select(&mut t, &[o[2]]);
    key(&mut t, "Enter");
    editing(&mut t, o[2]);
    key(&mut t, "Esc");
    in_library(&mut t);

    key(&mut t, "Ctrl+3");
    editing(&mut t, o[2]);
    key(&mut t, "Ctrl+1");
    in_library(&mut t);
    assert_eq!(state(&mut t)["thumbSize"], 144.0, "Ctrl+1 in the edit view is not Small Thumbnails");

    menu(&mut t, "Picture/View and Edit");
    editing(&mut t, o[2]);
    menu(&mut t, "View/Library View");
    in_library(&mut t);

    click(&mut t, &format!("thumb:{}", o[4]), 2);
    editing(&mut t, o[4]);
    assert_eq!(selection(&mut t), (json!([o[4]]), json!(o[4])), "the double-click's first click selected it");
    click(&mut t, "button:edit.back", 1);
    in_library(&mut t);
    assert_eq!(selection(&mut t), (json!([o[4]]), json!(o[4])));

    // An open tool takes Esc first; the second Esc leaves.
    key(&mut t, "Enter");
    run(&mut t, "edit.tool.open", json!({ "tool": "crop" }));
    key(&mut t, "Esc");
    assert_eq!((shown(&mut t).0, state(&mut t)["edit"]["tool"].clone()), ("edit".to_string(), Value::Null));
    key(&mut t, "Esc");
    in_library(&mut t);
}

/// Leaving keeps the library's scroll and selection and focuses the photo last viewed; a photo
/// stepped to outside the selection becomes the selection and is scrolled into view.
#[test]
fn the_library_keeps_its_scroll_and_selection_and_focuses_the_last_photo() {
    let mut t = setup((900, 600), 16);
    let o = t.order.clone();
    ok(&mut t, "ui.scroll", json!({ "dy": -150.0, "widget": "lightTable" }));
    t.d.settle(FRAMES).unwrap();
    // Two photos well inside the view (not under the pinned section header).
    let table = widget(&t, "region:lightTable").rect;
    let visible: Vec<u64> =
        t.d.widgets()
            .iter()
            .filter(|w| w.rect[1] > table[1] + 120.0 && w.rect[3] > 100.0)
            .filter_map(|w| w.id.strip_prefix("thumb:").and_then(|id| id.parse().ok()))
            .collect();
    let (a, b) = (visible[0], visible[1]);
    select(&mut t, &[a, b]);
    let before = state(&mut t);
    assert_ne!(before["scroll"], json!({ "section": null, "offset": 0.0 }), "scrolled: {}", before["scroll"]);

    run(&mut t, "view.edit", json!({ "id": a }));
    assert_eq!(selection(&mut t), (json!([a, b]), json!(a)), "a selected photo is focused, the selection kept");
    key(&mut t, "Esc");
    let after = state(&mut t);
    assert_eq!((&after["scroll"], &after["selection"]["ids"]), (&before["scroll"], &before["selection"]["ids"]));
    assert_eq!(after["selection"]["anchor"], json!(a), "the last photo viewed is focused");

    // Stepping to the last photo, out of the selection and out of sight.
    run(&mut t, "view.edit", json!({ "id": a }));
    key(&mut t, "End");
    let last = *o.last().unwrap();
    editing(&mut t, last);
    assert_eq!(selection(&mut t), (json!([last]), json!(last)), "the light table follows the photo shown");
    key(&mut t, "Ctrl+1");
    assert_eq!(selection(&mut t), (json!([last]), json!(last)));
    let table = widget(&t, "region:lightTable").rect;
    let cell = widget(&t, &format!("thumb:{last}")).rect;
    assert!(cell[1] >= table[1] && cell[1] + cell[3] <= table[1] + table[3] + 0.5, "{cell:?} in {table:?}");
}

/// ← / → and PgUp / PgDn step, Home / End jump; at the ends they stay put; hidden photos are
/// skipped unless View ▸ Hidden Pictures shows them; the status line counts the photos shown.
#[test]
fn arrows_page_keys_home_and_end_step_through_the_section() {
    let mut t = setup((1400, 900), 8);
    let o = t.order.clone();
    run(&mut t, "view.edit", json!({ "id": o[0] }));
    let status = |t: &mut T| state(t)["status"].as_str().unwrap().to_string();
    assert!(status(&mut t).ends_with("(1 of 8)"), "{}", status(&mut t));
    assert!(status(&mut t).contains(" › "), "the folder prefix: {}", status(&mut t));
    for (combo, want) in [
        ("Left", 0),
        ("PageUp", 0),
        ("Right", 1),
        ("PageDown", 2),
        ("PageUp", 1),
        ("Left", 0),
        ("End", 7),
        ("Right", 7),
        ("PageDown", 7),
        ("Home", 0),
    ] {
        key(&mut t, combo);
        editing(&mut t, o[want]);
        assert!(status(&mut t).ends_with(&format!("({} of 8)", want + 1)), "{combo}: {}", status(&mut t));
    }
    let r = run(&mut t, "edit.next", json!({}));
    assert_eq!(r, json!({ "photo": o[1], "nth": 2, "of": 8 }));
    run(&mut t, "edit.first", json!({}));

    // Hidden photos are stepped over, and shown again with View ▸ Hidden Pictures.
    run(&mut t, "photo.hide", json!({ "ids": [o[1]] }));
    key(&mut t, "Right");
    editing(&mut t, o[2]);
    assert!(status(&mut t).ends_with("(2 of 7)"), "{}", status(&mut t));
    run(&mut t, "view.hiddenPictures", json!({ "on": true }));
    key(&mut t, "Left");
    editing(&mut t, o[1]);
    assert!(status(&mut t).ends_with("(2 of 8)"), "{}", status(&mut t));

    // Outside the edit view the steps refuse; bad parameters change nothing.
    key(&mut t, "Esc");
    for id in ["edit.next", "edit.previous", "edit.first", "edit.last", "edit.history.undo", "edit.history.redo", "edit.autoFix"] {
        assert_eq!(code(&mut t, "engine.execute", json!({ "command": id, "params": {} })), "disabled", "{id}");
        assert_eq!(code(&mut t, "engine.execute", json!({ "command": id, "params": { "n": 1 } })), "bad_params", "{id}");
    }
    in_library(&mut t);
}

/// A filmstrip click opens that photo and recentres the strip on it; the arrows step and are
/// greyed at the section's ends.
#[test]
fn a_filmstrip_click_opens_that_photo_and_recentres() {
    for size in SIZES {
        let mut t = setup(size, 8);
        let o = t.order.clone();
        run(&mut t, "view.edit", json!({ "id": o[0] }));
        let strip = widget(&t, "region:filmstrip").rect;
        let centre = strip[0] + strip[2] / 2.0;
        let mid = |w: &WidgetInfo| w.rect[0] + w.rect[2] / 2.0;
        let cur = widget(&t, &format!("film:{}", o[0]));
        assert!((mid(&cur) - centre).abs() <= 0.5 && cur.active, "{size:?}: the photo shown is centred and marked");
        assert!(!widget(&t, "button:film.previous").enabled && widget(&t, "button:film.next").enabled, "{size:?}");
        let film: Vec<String> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("film:")).map(|w| w.id).collect();
        assert_eq!(film.len(), if size.0 == 1400 { 5 } else { 4 }, "{size:?}: the photo and those after it: {film:?}");

        click(&mut t, &format!("film:{}", o[3]), 1);
        editing(&mut t, o[3]);
        let cur = widget(&t, &format!("film:{}", o[3]));
        assert!((mid(&cur) - centre).abs() <= 0.5 && cur.active, "{size:?}: recentred");
        let left = widget(&t, &format!("film:{}", o[2]));
        assert!(mid(&left) < mid(&cur), "{size:?}: the one before on the left");

        click(&mut t, "button:film.next", 1);
        editing(&mut t, o[4]);
        click(&mut t, "button:film.previous", 1);
        editing(&mut t, o[3]);
        key(&mut t, "End");
        assert!(widget(&t, "button:film.previous").enabled && !widget(&t, "button:film.next").enabled, "{size:?}");
        // Play and the compare modes wait for M11; "A" is on.
        assert!(!widget(&t, "button:edit.play").enabled);
        let one = widget(&t, "button:compare.one");
        assert!(one.enabled && one.active);
        assert!(!widget(&t, "button:compare.two").enabled && !widget(&t, "button:compare.same").enabled);
    }
}

/// In the edit view Ctrl+R, the flips, Ctrl+Shift+E, Ctrl+Z and Ctrl+Y walk the photo's own
/// history; the library undo stack keeps its last step.
#[test]
fn ctrl_z_in_the_edit_view_undoes_the_photos_rotation_and_leaves_the_library_stack_alone() {
    let mut t = setup((1400, 900), 4);
    let o = t.order.clone();
    run(&mut t, "photo.star.toggle", json!({ "ids": [o[3]] }));
    run(&mut t, "view.edit", json!({ "id": o[0] }));
    let redo = if cfg!(target_os = "macos") { "Ctrl+Shift+Z" } else { "Ctrl+Y" };

    key(&mut t, "Ctrl+R");
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)"]));
    key(&mut t, "Ctrl+Shift+R");
    assert_eq!(photo_edits(&mut t, o[0]), json!([]), "a quarter back merges into the turn");
    key(&mut t, "Ctrl+Z");
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)"]), "undone one step at a time");
    key(&mut t, "Ctrl+Z");
    assert_eq!(photo_edits(&mut t, o[0]), json!([]));
    key(&mut t, redo);
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)"]));
    key(&mut t, "Ctrl+Shift+H");
    key(&mut t, "Ctrl+Shift+V");
    key(&mut t, "Ctrl+Shift+E");
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)", "flip_h()", "flip_v()", "autofix()"]));
    for _ in 0..4 {
        key(&mut t, "Ctrl+Z");
    }
    assert_eq!(photo_edits(&mut t, o[0]), json!([]));
    assert!(t.d.app().session().unwrap().library().info(dalica_core::PhotoId::new(o[1])).edits.is_empty(), "other photos untouched");

    // The library stack still ends with the star.
    key(&mut t, "Esc");
    assert_eq!(run(&mut t, "edit.undo", json!({})), json!({ "undone": "Star / Unstar" }));
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.undo", "params": {} })), "disabled", "nothing else on it");
}

/// A caption typed in the caption bar is set on Return as one library step; Esc gives the text back.
#[test]
fn a_caption_typed_in_the_bar_is_one_library_step() {
    let mut t = setup((1400, 900), 4);
    let o = t.order.clone();
    run(&mut t, "view.edit", json!({ "id": o[1] }));
    let field = widget(&t, "field:caption");
    assert_eq!(field.label, "Add a caption");

    click(&mut t, "field:caption", 1);
    ok(&mut t, "ui.type", json!({ "text": "Harbour at dusk" }));
    key(&mut t, "Right");
    editing(&mut t, o[1]);
    key(&mut t, "Enter");
    assert_eq!(library_caption(&t, o[1]), "Harbour at dusk");
    editing(&mut t, o[1]);

    // Esc in the field gives the caption back and leaves the edit view open.
    click(&mut t, "field:caption", 1);
    ok(&mut t, "ui.type", json!({ "text": " again" }));
    key(&mut t, "Esc");
    assert_eq!(library_caption(&t, o[1]), "Harbour at dusk");
    editing(&mut t, o[1]);

    key(&mut t, "Esc");
    let undone = run(&mut t, "edit.undo", json!({}));
    assert!(undone["undone"].is_string(), "{undone}");
    assert_eq!(library_caption(&t, o[1]), "");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.undo", "params": {} })), "disabled", "one step");
}

/// Checks the pixels of `rect` of the window against `tests/fingerprints/<name>.json`
/// (`DALICA_BLESS=1` writes it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: [f32; 4]) {
    let shot = t.d.screenshot().unwrap();
    let [x0, y0] = [rect[0].round() as u32, rect[1].round() as u32];
    let [x1, y1] = [((rect[0] + rect[2]).round() as u32).min(shot.width), ((rect[1] + rect[3]).round() as u32).min(shot.height)];
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    let png = fingerprint::png_rgba(w, h, &rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
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

/// The top bar, the caption bar (with a caption) and the status line at both sizes.
#[test]
fn the_top_bar_caption_bar_and_status_line_match_their_fingerprints() {
    for (w, h) in SIZES {
        let mut t = setup((w, h), 8);
        let o = t.order.clone();
        run(&mut t, "photo.caption.set", json!({ "ids": [o[2]], "caption": "Boats in the harbour" }));
        run(&mut t, "view.edit", json!({ "id": o[2] }));
        let status = state(&mut t)["status"].as_str().unwrap().to_string();
        assert!(status.ends_with("(3 of 8)") && status.contains(" › "), "{w}×{h}: {status}");
        check_region(&t, &format!("edit-topbar-{w}x{h}"), widget(&t, "region:editTopBar").rect);
        check_region(&t, &format!("edit-caption-{w}x{h}"), widget(&t, "region:captionBar").rect);
        check_region(&t, &format!("region-status-edit-{w}x{h}"), widget(&t, "region:statusLine").rect);
    }
}

/// Edit ▸ Undo / Redo and Picture ▸ Rotate / Flip clicked in the menu bar while in the edit view
/// are steps of the open photo's history, and the Undo / Redo rows name and enable by it; the
/// library undo stack keeps its own last step.
#[test]
fn menu_bar_undo_rotate_and_flip_walk_the_photos_history_in_the_edit_view() {
    let mut t = setup((1400, 900), 4);
    let o = t.order.clone();
    run(&mut t, "photo.star.toggle", json!({ "ids": [o[3]] }));
    run(&mut t, "view.edit", json!({ "id": o[0] }));
    menu(&mut t, "Picture/Rotate Clockwise");
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)"]));
    menu(&mut t, "Picture/Flip Horizontal");
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)", "flip_h()"]));
    ok(&mut t, "ui.menu.invoke", json!({ "id": "edit.undo", "wait": true }));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)"]), "Edit ▸ Undo undid the flip");
    // Edit ▸ Redo follows the photo's history too (the library stack has nothing to redo): it
    // names the flip and puts it back.
    let bar = dalica_ui::menus::bar(t.d.app(), dalica_ui::menus::Platform::current());
    let redo = dalica_ui::menus::find(&bar, &dalica_ui::menus::Lookup::Id { id: "edit.redo".into(), params: Value::Null }).unwrap();
    assert_eq!((redo.label.as_str(), redo.enabled), ("Redo Flip", true));
    ok(&mut t, "ui.menu.invoke", json!({ "id": "edit.redo", "wait": true }));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(photo_edits(&mut t, o[0]), json!(["rotate(90)", "flip_h()"]), "Edit ▸ Redo redid the flip");
    ok(&mut t, "ui.menu.invoke", json!({ "id": "edit.undo", "wait": true }));
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.app().session().unwrap().library().photo(dalica_core::PhotoId::new(o[3])).unwrap().star, "the star stays");
    key(&mut t, "Esc");
    assert_eq!(run(&mut t, "edit.undo", json!({})), json!({ "undone": "Star / Unstar" }), "the library stack ends with the star");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "edit.undo", "params": {} })), "disabled");
}
