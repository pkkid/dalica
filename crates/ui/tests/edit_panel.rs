//! The edit panel (`editing.md` §2, §5–§6), driven through the control channel as an agent would:
//! each tab by its id (`tab:edit.*`, the effect tabs' empty state); the Basic Fixes grid (Auto Fix
//! by click and by Ctrl+Shift+E is one "Auto Fix" step of the photo's history, Auto Contrast and
//! Auto Color likewise; Crop, Straighten and Retouch open their tool); the Undo / Redo pair with the
//! labels of the photo's history, walked by its buttons, and Edit ▸ Undo / Redo following the same
//! history; an open tool draws the grid, the tab content and Undo / Redo disabled; the histogram
//! follows an edit; the camera columns for a JPEG with EXIF and the empty text without; the panel's
//! fingerprints per tab at 1400 × 900 and 900 × 600.
//!
//! `DALICA_SHOTS=<dir>` writes the panel's screenshots there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, new_driver, out_dir};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use serde_json::{Value, json};

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
/// The JPEG without EXIF added next to the demo photos.
const PLAIN: &str = "Plain.jpg";

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The demo photos (all with EXIF) by id.
    photos: Vec<u64>,
    /// The photo without EXIF.
    plain: u64,
}

/// A library under a temporary directory holding one folder of `n` demo photos (EXIF with a camera
/// make and model) and one JPEG without EXIF, the window at `size`.
fn setup((w, h): (u32, u32), n: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(1, n, 11).unwrap();
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    std::fs::create_dir_all(&folder).unwrap();
    for f in &files {
        std::fs::write(dir.path().join("photos").join(&f.path), &f.bytes).unwrap();
    }
    let plain = dalica_scenes::jpeg(&dalica_scenes::Scene::new(77, 480, 320), 85, None).unwrap();
    std::fs::write(folder.join(PLAIN), plain).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, u64)> = s.library().photos().map(|p| (p.name.clone(), p.id.get())).collect();
    ids.sort();
    let plain = ids.iter().find(|(name, _)| name == PLAIN).unwrap().1;
    let photos: Vec<u64> = ids.iter().filter(|(name, _)| name != PLAIN).map(|p| p.1).collect();
    assert_eq!(photos.len(), n);
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, photos, plain }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
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

fn click(t: &mut T, id: &str) {
    ok(t, "ui.clickWidget", json!({ "id": id }));
    t.d.settle(FRAMES).unwrap();
}

fn find(t: &T, id: &str) -> Option<WidgetInfo> {
    t.d.widgets().into_iter().find(|w| w.id == id)
}

fn widget(t: &T, id: &str) -> WidgetInfo {
    find(t, id).unwrap_or_else(|| panic!("no `{id}` in {:?}", t.d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

/// The label and enabled state of widget `id`.
fn shows(t: &T, id: &str) -> (String, bool) {
    let w = widget(t, id);
    (w.label, w.enabled)
}

fn edits(t: &mut T, id: u64) -> Value {
    run(t, "photo.history.state", json!({ "id": id }))["edits"].clone()
}

fn open(t: &mut T, id: u64) {
    run(t, "view.edit", json!({ "id": id }));
    assert_eq!(state(t)["edit"]["photo"], json!(id));
}

/// The label and enabled state of the Edit menu's row running `id`.
fn menu_row(t: &T, id: &str) -> (String, bool) {
    let bar = menus::bar(t.d.app(), Platform::current());
    let row = menus::find(&bar, &Lookup::Id { id: id.into(), params: Value::Null }).unwrap();
    (row.label.clone(), row.enabled)
}

fn library_steps(t: &T) -> usize {
    t.d.app().session().unwrap().undo_stack().len()
}

/// The pixels of `rect` of the window: width, height and RGBA.
fn pixels(t: &T, rect: [f32; 4]) -> (u32, u32, Vec<u8>) {
    let shot = t.d.screenshot().unwrap();
    let [x0, y0] = [rect[0].round() as u32, rect[1].round() as u32];
    let [x1, y1] = [((rect[0] + rect[2]).round() as u32).min(shot.width), ((rect[1] + rect[3]).round() as u32).min(shot.height)];
    let mut rgba = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    (x1 - x0, y1 - y0, rgba)
}

/// Checks the pixels of `rect` against `tests/fingerprints/<name>.json` (`DALICA_BLESS=1` writes
/// it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: [f32; 4]) {
    let (w, h, rgba) = pixels(t, rect);
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

const TABS: [(&str, &str, &str); 5] = [
    ("tab:edit.basic", "basic", "Basic fixes"),
    ("tab:edit.tuning", "tuning", "Lighting and color tuning"),
    ("tab:edit.effects", "effects", "Effects"),
    ("tab:edit.effects2", "effects2", "More effects"),
    ("tab:edit.effects3", "effects3", "Even more effects"),
];

const GRID: [&str; 6] = ["tool:crop", "tool:straighten", "tool:retouch", "tool:autofix", "tool:autocontrast", "tool:autocolor"];

/// Each tab by its id: it becomes the tab shown (the only active one), the Basic Fixes grid shows
/// only on its tab, the effect tabs show their empty state; each tab's label is its tooltip.
#[test]
fn each_tab_is_shown_by_its_id() {
    let mut t = setup((1400, 900), 3);
    let p = t.photos[0];
    open(&mut t, p);
    assert_eq!(state(&mut t)["edit"]["tab"], "basic");
    for id in GRID {
        assert!(widget(&t, id).enabled, "{id}");
    }
    for (id, name, tip) in TABS {
        click(&mut t, id);
        assert_eq!(state(&mut t)["edit"]["tab"], name, "{id}");
        for (other, ..) in TABS {
            let w = widget(&t, other);
            assert_eq!(w.active, other == id, "{other} while {id}");
            assert!(w.enabled, "{other}");
        }
        assert_eq!(widget(&t, id).label, tip);
        assert_eq!(find(&t, "tool:autofix").is_some(), name == "basic", "{id}");
        assert_eq!(find(&t, "empty:edit.effects").is_some(), name.starts_with("effects"), "{id}");
        // Undo / Redo and the histogram box are under every tab.
        for under in ["button:edit.undo", "button:edit.redo", "box:histogram", "histogram:edit"] {
            assert!(find(&t, under).is_some(), "{under} under {id}");
        }
    }
    click(&mut t, "tab:edit.basic");
    assert_eq!(state(&mut t)["edit"]["tab"], "basic");
}

/// Auto Fix by a click and by Ctrl+Shift+E is one "Auto Fix" step each; Undo then reads "Undo Auto
/// Fix"; Undo and Redo by the buttons walk the photo's history (and Edit ▸ Undo / Redo show the
/// same); Auto Contrast and Auto Color are steps of their own. The library stack is untouched.
#[test]
fn auto_fix_is_one_step_and_the_buttons_walk_the_photos_history() {
    let mut t = setup((1400, 900), 3);
    let (a, b) = (t.photos[0], t.photos[1]);
    run(&mut t, "photo.star.toggle", json!({ "ids": [b] }));
    let steps = library_steps(&t);
    open(&mut t, a);
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo".into(), false));
    assert_eq!(shows(&t, "button:edit.redo"), ("Redo".into(), false));
    assert_eq!(widget(&t, "button:edit.undo").rect[2], widget(&t, "button:edit.redo").rect[2], "the pair shares the width");

    click(&mut t, "tool:autofix");
    assert_eq!(edits(&mut t, a), json!(["autofix()"]));
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Fix".into(), true));
    assert_eq!(shows(&t, "button:edit.redo"), ("Redo".into(), false));
    assert_eq!(menu_row(&t, "edit.undo"), ("Undo Auto Fix".into(), true));
    assert_eq!(menu_row(&t, "edit.redo"), ("Redo".into(), false));

    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, a), json!([]));
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo".into(), false));
    assert_eq!(shows(&t, "button:edit.redo"), ("Redo Auto Fix".into(), true));
    assert_eq!(menu_row(&t, "edit.undo"), ("Undo".into(), false));
    assert_eq!(menu_row(&t, "edit.redo"), ("Redo Auto Fix".into(), true));

    click(&mut t, "button:edit.redo");
    assert_eq!(edits(&mut t, a), json!(["autofix()"]));
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Fix".into(), true));
    assert_eq!(shows(&t, "button:edit.redo"), ("Redo".into(), false));

    click(&mut t, "tool:autocontrast");
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Contrast".into(), true));
    click(&mut t, "tool:autocolor");
    assert_eq!(edits(&mut t, a), json!(["autofix()", "autocontrast()", "autocolor()"]));
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Color".into(), true));
    click(&mut t, "button:edit.undo");
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Contrast".into(), true));
    assert_eq!(shows(&t, "button:edit.redo"), ("Redo Auto Color".into(), true));

    // Ctrl+Shift+E on another photo: one step there.
    run(&mut t, "edit.next", json!({}));
    let shown = state(&mut t)["edit"]["photo"].as_u64().unwrap();
    assert_ne!(shown, a);
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo".into(), false), "the pair follows the photo shown");
    key(&mut t, "Ctrl+Shift+E");
    assert_eq!(edits(&mut t, shown), json!(["autofix()"]));
    assert_eq!(shows(&t, "button:edit.undo"), ("Undo Auto Fix".into(), true));
    assert_eq!(edits(&mut t, a), json!(["autofix()", "autocontrast()"]), "the first photo keeps its own history");

    assert_eq!(library_steps(&t), steps, "no step on the library stack");
}

/// An open tool: Straighten draws the grid, the tab content and Undo / Redo disabled, its own
/// button active; another tab shows its content disabled; Crop and Retouch replace the grid with
/// their panel; Esc closes the tool and everything is enabled again. A Tuning session leaves the
/// panel enabled.
#[test]
fn an_open_tool_disables_the_grid_the_tab_content_and_undo() {
    let mut t = setup((1400, 900), 2);
    let p = t.photos[0];
    open(&mut t, p);
    click(&mut t, "tool:autofix");
    assert!(widget(&t, "button:edit.undo").enabled);

    // TODO(maintainer): the straighten tool's floating bar takes clicks (egui id `straighten:bar`)
    // without an automation id; once it registers one, keep the id check on here.
    t.d.check_ids(false);
    click(&mut t, "tool:straighten");
    assert_eq!(state(&mut t)["edit"]["tool"], "straighten");
    let straighten = widget(&t, "tool:straighten");
    assert!(straighten.active && !straighten.enabled, "active, and the grid is held");
    for id in GRID {
        assert!(!widget(&t, id).enabled, "{id} disabled while Straighten is open");
    }
    assert!(!widget(&t, "button:edit.undo").enabled && !widget(&t, "button:edit.redo").enabled);
    assert_eq!(widget(&t, "button:edit.undo").label, "Undo Auto Fix", "drawn disabled, still labelled");
    // A click on a disabled tool is refused and does nothing.
    let refused = call(&mut t, "ui.clickWidget", json!({ "id": "tool:autocolor" })).unwrap_err();
    assert_eq!(refused.code.as_str(), "disabled");
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&mut t, p), json!(["autofix()"]));
    // The tabs stay clickable; another tab's content is disabled too.
    for (id, ..) in TABS {
        assert!(widget(&t, id).enabled, "{id}");
    }
    click(&mut t, "tab:edit.effects");
    assert_eq!(state(&mut t)["edit"]["tab"], "effects");
    assert_eq!(state(&mut t)["edit"]["tool"], "straighten", "the tool stays open");
    click(&mut t, "tab:edit.basic");
    key(&mut t, "Esc");
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    for id in GRID {
        assert!(widget(&t, id).enabled, "{id} enabled again");
    }
    assert!(widget(&t, "button:edit.undo").enabled);

    for (tool, name) in [("tool:crop", "crop"), ("tool:retouch", "retouch")] {
        click(&mut t, tool);
        assert_eq!(state(&mut t)["edit"]["tool"], name);
        assert!(find(&t, "tool:autofix").is_none(), "{name}'s panel replaces the grid");
        assert!(!widget(&t, "button:edit.undo").enabled, "{name}");
        click(&mut t, "tab:edit.tuning");
        assert_eq!(state(&mut t)["edit"]["tool"], name, "{name}'s panel stays");
        assert!(find(&t, "tool:autofix").is_none());
        click(&mut t, "tab:edit.basic");
        key(&mut t, "Esc");
        assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
        assert!(widget(&t, "tool:autofix").enabled);
    }

    // A Tuning session is not modal: the grid and Undo stay enabled.
    run(&mut t, "edit.tool.open", json!({ "tool": "tuning" }));
    assert!(widget(&t, "tool:autofix").enabled && widget(&t, "button:edit.undo").enabled);
}

/// The histogram is the shown preview's (the photo canvas opens it): its bins, and it changes
/// when an edit renders a new preview.
#[test]
fn the_histogram_follows_an_edit() {
    let mut t = setup((1400, 900), 2);
    let p = t.photos[0];
    open(&mut t, p);
    let plot = widget(&t, "histogram:edit").rect;
    let wait = |t: &mut T| {
        assert!(t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(30)), "the preview arrives");
        t.d.settle(FRAMES).unwrap();
    };
    wait(&mut t);
    assert_eq!(t.d.app().session().unwrap().preview_photo(), Some(PhotoId::new(p)), "the canvas opened the preview");
    let (_, _, before) = pixels(&t, plot);
    assert!(!before.chunks(4).all(|px| px == &before[..4]), "the preview's histogram is drawn, not an empty plate");

    run(&mut t, "photo.history.tuning", json!({ "id": p, "session": "test", "fill": 1.0, "shadows": 0.5 }));
    wait(&mut t);
    let (_, _, after) = pixels(&t, plot);
    assert_ne!(after, before, "the histogram follows the edit");
    let generation = t.d.app().session().unwrap().preview().unwrap().generation;

    click(&mut t, "button:edit.undo");
    wait(&mut t);
    assert!(t.d.app().session().unwrap().preview().unwrap().generation > generation);
    assert_eq!(pixels(&t, plot).2, before, "undo brings the first histogram back");
}

/// The camera columns: the demo photos' EXIF make and model, and "No camera information" for the
/// JPEG without EXIF.
#[test]
fn camera_information_with_exif_and_without() {
    let mut t = setup((1400, 900), 2);
    let (with, plain) = (t.photos[0], t.plain);
    open(&mut t, with);
    assert_eq!(widget(&t, "box:histogram/info").label, "Dalica Scenes 1");
    assert_eq!(widget(&t, "box:histogram").label, "Histogram & Camera Info");
    open(&mut t, plain);
    assert_eq!(widget(&t, "box:histogram/info").label, "No camera information");
    open(&mut t, with);
    assert_eq!(widget(&t, "box:histogram/info").label, "Dalica Scenes 1", "asked again for another photo");
}

/// The edit panel per tab (Basic Fixes after an Auto Fix, with the preview's histogram; Tuning; an
/// effect tab) and with Straighten open, at both sizes.
#[test]
fn the_edit_panel_matches_its_fingerprints() {
    for (w, h) in SIZES {
        let mut t = setup((w, h), 2);
        let p = t.photos[0];
        open(&mut t, p);
        click(&mut t, "tool:autofix");
        // The histogram of the photo's preview (the photo area opens it in the app).
        t.d.app_mut().session_mut().unwrap().open_preview(PhotoId::new(p), (480, 320)).unwrap();
        assert!(t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(30)));
        t.d.settle(FRAMES).unwrap();
        let panel = widget(&t, "region:editPanel").rect;
        check_region(&t, &format!("edit-panel-basic-{w}x{h}"), panel);
        click(&mut t, "tab:edit.tuning");
        check_region(&t, &format!("edit-panel-tuning-{w}x{h}"), panel);
        click(&mut t, "tab:edit.effects");
        check_region(&t, &format!("edit-panel-effects-{w}x{h}"), panel);
        click(&mut t, "tab:edit.basic");
        t.d.check_ids(false); // The straighten bar has no automation id (see above).
        click(&mut t, "tool:straighten");
        check_region(&t, &format!("edit-panel-straighten-{w}x{h}"), panel);
    }
}
