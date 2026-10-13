//! The Straighten tool (`editing.md` §6.2), headless, driven through the control channel as an
//! agent would: the dashed grid and the floating bar over the photo; a drag of the wide slider to
//! +3.5° turns the photo live through the preview's draft while the recipe stays as it was, and
//! Apply stores it as one "Straighten" step; the arrows nudge 0.1° and stop at ±10°; Esc and Cancel
//! leave the recipe unchanged and the preview back on it; a straightened photo has no empty corner
//! and keeps its size; the tool's actions refuse bad parameters; fingerprints at 0° and +5° at
//! 1400 × 900 and 900 × 600.
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_edit::{Geometry, Point, Recipe};
use dalica_engine::Session;
use dalica_image::Orientation;
use dalica_scenes::{Scene, fingerprint};
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::edit::canvas::{self, View};
use dalica_ui::panels::edit::straighten;
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, Rect, pos2};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
/// The test photo: larger than the photo area at both window sizes.
const PHOTO: (u32, u32) = (1500, 1000);

/// A session on a library under `dir` holding one folder of one `PHOTO`-sized scenes JPEG.
fn session(dir: &Path) -> (Session, u64) {
    let folder = dir.join("photos").join("Horizon");
    std::fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&Scene::new(21, PHOTO.0, PHOTO.1), 90, None).unwrap();
    std::fs::write(folder.join("IMG_0001.jpg"), bytes).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10 })).unwrap();
    let id = found["photos"][0]["id"].as_u64().unwrap();
    (s, id)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    photo: u64,
}

/// The window at `size` showing the photo in the edit view, its preview drawn.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, photo) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = Driver::new(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, photo };
    run(&mut t, "view.edit", json!({ "id": photo }));
    assert!(t.d.app().session().unwrap().preview().is_some(), "settling waits for the preview");
    t
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn settle(t: &mut T) {
    t.d.settle(FRAMES).unwrap();
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = ok(t, "engine.execute", json!({ "command": id, "params": params }));
    settle(t);
    v
}

fn click(t: &mut T, id: &str) {
    ok(t, "ui.clickWidget", json!({ "id": id }));
    settle(t);
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    settle(t);
}

fn state(t: &mut T) -> Value {
    ok(t, "ui.state", json!({}))
}

/// The draft's angle (an f32 in the state).
fn angle(t: &mut T) -> f32 {
    state(t)["edit"]["straighten"]["angle"].as_f64().expect("the tool is open") as f32
}

fn history(t: &mut T) -> Value {
    let id = t.photo;
    run(t, "photo.history.state", json!({ "id": id }))
}

fn edits(t: &mut T) -> Value {
    history(t)["edits"].clone()
}

/// The recipe the preview shows now.
fn shown(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().preview().expect("a preview").recipe.clone()
}

fn view(t: &T) -> View {
    canvas::view(t.d.ctx()).expect("the canvas was drawn")
}

fn rect(t: &T, id: &str) -> Rect {
    let [x, y, w, h] = t.d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("{id} is drawn")).rect;
    Rect::from_min_size(pos2(x, y), egui::vec2(w, h))
}

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

fn open(t: &mut T) {
    click(t, "tool:straighten");
    assert_eq!(state(t)["edit"]["tool"], "straighten");
}

fn pointer(t: &mut T, events: &[(PointerKind, Pos2)]) {
    let events: Vec<PointerEvent> = events.iter().map(|&(kind, pos)| PointerEvent { kind, pos }).collect();
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
}

/// The slider's track ends (the ring's radius and a point in from its rect's edges).
fn track(t: &T) -> (f32, f32, f32) {
    let r = rect(t, straighten::SLIDER_ID);
    let ring = t.d.app().tokens().metrics.slider_ring;
    (r.min.x + ring + 1.0, r.max.x - ring - 1.0, r.center().y)
}

#[test]
fn the_grid_and_the_floating_bar_show_over_the_photo() {
    for size in SIZES {
        let mut t = setup(size);
        assert!(!has(&t, straighten::SLIDER_ID), "{size:?}: nothing before the tool opens");
        open(&mut t);
        let v = view(&t);
        let bar = rect(&t, straighten::SLIDER_ID).union(rect(&t, straighten::APPLY_ID)).union(rect(&t, straighten::CANCEL_ID));
        assert!(v.area.contains_rect(bar), "{size:?}: the bar is inside the photo area");
        assert!(v.image.contains_rect(bar), "{size:?}: and over the photo");
        // Centred on the photo, 10 pt above its bottom edge (the bar's own padding around its parts).
        assert!((bar.center().x - v.image.center().x).abs() < 12.0, "{size:?}: {bar:?} centred on {:?}", v.image);
        let gap = v.image.max.y - bar.max.y;
        assert!(gap > straighten::BAR_LIFT && gap < straighten::BAR_LIFT + 12.0, "{size:?}: {gap}");
        // Apply right of the slider, Cancel right of Apply; the thumb at the middle is 0°.
        let (apply, cancel) = (rect(&t, straighten::APPLY_ID), rect(&t, straighten::CANCEL_ID));
        assert!(rect(&t, straighten::SLIDER_ID).max.x <= apply.min.x && apply.max.x <= cancel.min.x);
        assert_eq!(angle(&mut t), 0.0);
        assert_eq!(edits(&mut t), json!([]));
    }
}

/// A drag of the slider to +3.5° turns the photo live (the preview's candidate, its recipe left
/// as it was) and Apply stores it as one "Straighten" step.
#[test]
fn a_drag_to_3_5_degrees_previews_live_and_apply_stores_it() {
    let mut t = setup((1400, 900));
    open(&mut t);
    let before = view(&t);
    let (x0, x1, y) = track(&t);
    let from = pos2((x0 + x1) / 2.0, y);
    let to = pos2(from.x + 3.5 / 20.0 * (x1 - x0), y);
    let mut gesture = vec![(PointerKind::Move, from), (PointerKind::Down, from)];
    for i in 1..=10 {
        gesture.push((PointerKind::Drag, from.lerp(to, i as f32 / 10.0)));
    }
    pointer(&mut t, &gesture);
    settle(&mut t);
    // Still held: the draft and the preview follow, the recipe does not.
    assert_eq!(angle(&mut t), 3.5);
    assert_eq!(shown(&t), ["straighten(3.5)"], "the preview shows the draft");
    assert_eq!(edits(&mut t), json!([]), "nothing is stored while dragging");
    let live = view(&t);
    assert_eq!((live.size, live.image), (before.size, before.image), "turned in place, the same size");
    pointer(&mut t, &[(PointerKind::Up, to)]);
    settle(&mut t);
    assert_eq!(angle(&mut t), 3.5);

    click(&mut t, straighten::APPLY_ID);
    let h = history(&mut t);
    assert_eq!(h["edits"], json!(["straighten(3.5)"]));
    assert_eq!(h["undoLabel"], "Undo Straighten");
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null, "Apply closes the tool");
    assert!(!has(&t, straighten::SLIDER_ID));
    assert_eq!(shown(&t), ["straighten(3.5)"], "the preview shows the stored recipe");
    // One step: one undo takes it all back.
    let id = t.photo;
    run(&mut t, "photo.history.undo", json!({ "id": id }));
    assert_eq!(edits(&mut t), json!([]));
    assert_eq!(history(&mut t)["canUndo"], false);

    // Re-opened, the tool starts at the photo's angle.
    run(&mut t, "photo.history.redo", json!({ "id": id }));
    open(&mut t);
    assert_eq!(angle(&mut t), 3.5);
    // A double-click on the slider goes back to 0°.
    ok(&mut t, "ui.clickWidget", json!({ "id": straighten::SLIDER_ID, "count": 2 }));
    settle(&mut t);
    assert_eq!(angle(&mut t), 0.0);
    assert_eq!(shown(&t), ["straighten(3.5)", "straighten(0)"]);
}

/// The arrows nudge by 0.1° and stop at ±10°; the preview follows.
#[test]
fn arrows_nudge_by_a_tenth_within_ten_degrees() {
    let mut t = setup((900, 600));
    open(&mut t);
    for _ in 0..3 {
        key(&mut t, "Right");
    }
    assert_eq!(angle(&mut t), 0.3);
    assert_eq!(shown(&t), ["straighten(0.3)"]);
    for _ in 0..5 {
        key(&mut t, "Left");
    }
    assert_eq!(angle(&mut t), -0.2);
    key(&mut t, "Up");
    key(&mut t, "Up");
    key(&mut t, "Down");
    assert_eq!(angle(&mut t), -0.1);
    ok(&mut t, "ui.set", json!({ "edit": { "straighten": { "angle": 9.95 } } }));
    settle(&mut t);
    assert_eq!(shown(&t), ["straighten(9.95)"], "a draft set from outside is previewed too");
    key(&mut t, "Right");
    assert_eq!(angle(&mut t), 10.0);
    key(&mut t, "Right");
    assert_eq!(angle(&mut t), 10.0, "never past +10°");
    ok(&mut t, "ui.set", json!({ "edit": { "straighten": { "angle": -9.9 } } }));
    settle(&mut t);
    key(&mut t, "Left");
    key(&mut t, "Left");
    assert_eq!(angle(&mut t), -10.0, "never past −10°");
    assert_eq!(shown(&t), ["straighten(-10)"]);
    assert_eq!(edits(&mut t), json!([]), "nudges store nothing");
    assert_eq!(state(&mut t)["view"], "edit", "the arrows did not move to another photo");
}

/// Esc and Cancel close the tool with the recipe as it was, and the preview back on it.
#[test]
fn esc_and_cancel_leave_the_recipe_unchanged() {
    let mut t = setup((1400, 900));
    let id = t.photo;
    run(&mut t, "photo.straighten.apply", json!({ "id": id, "degrees": 2.0 }));
    let before = history(&mut t);
    assert_eq!(before["edits"], json!(["straighten(2)"]));

    open(&mut t);
    assert_eq!(angle(&mut t), 2.0, "opens at the photo's angle");
    for _ in 0..5 {
        key(&mut t, "Right");
    }
    assert_eq!(shown(&t), ["straighten(2)", "straighten(2.5)"]);
    key(&mut t, "Esc");
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    assert_eq!(state(&mut t)["view"], "edit", "Esc cancelled the tool and stayed");
    assert_eq!(history(&mut t), before);
    assert_eq!(shown(&t), ["straighten(2)"], "the preview is the photo's own again");

    open(&mut t);
    ok(&mut t, "engine.execute", json!({ "command": "edit.tool.action", "params": { "name": "set", "params": { "angle": -6 } } }));
    settle(&mut t);
    assert_eq!(shown(&t), ["straighten(2)", "straighten(-6)"]);
    click(&mut t, straighten::CANCEL_ID);
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    assert_eq!(history(&mut t), before);
    assert_eq!(shown(&t), ["straighten(2)"]);

    // `edit.tool.cancel` sent directly (a script) also puts the photo's own recipe back.
    open(&mut t);
    key(&mut t, "Left");
    assert_eq!(shown(&t), ["straighten(2)", "straighten(1.9)"]);
    run(&mut t, "edit.tool.cancel", json!({}));
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    assert_eq!(history(&mut t), before);
    assert_eq!(shown(&t), ["straighten(2)"]);
}

/// Straightened as far as it goes, the photo keeps its size and fills its frame: no corner of the
/// preview is empty.
#[test]
fn the_result_has_no_empty_corner() {
    let mut t = setup((1400, 900));
    let level = view(&t);
    let level_px = {
        let p = t.d.app().session().unwrap().preview().unwrap();
        (p.image.width, p.image.height)
    };
    open(&mut t);
    for a in [10.0, -10.0, 5.0] {
        ok(&mut t, "engine.execute", json!({ "command": "edit.tool.action", "params": { "name": "set", "params": { "angle": a } } }));
        settle(&mut t);
        let preview = t.d.app().session().unwrap().preview().unwrap().clone();
        assert_eq!(preview.recipe.last().map(String::as_str), Some(format!("straighten({a})").as_str()));
        // Every corner of the picture shown comes from inside the photo (the renderer reads the
        // nearest edge pixel past it, so an empty corner shows as smeared edge, not as alpha).
        let recipe = Recipe::parse(&preview.recipe).unwrap();
        let g = Geometry::new(&recipe, Orientation::default(), PHOTO.0, PHOTO.1);
        let mut turned = false;
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let p = g.to_stored(Point::new(x, y)).unwrap();
            let inside = (-1e-9..=1.0 + 1e-9).contains(&p.x) && (-1e-9..=1.0 + 1e-9).contains(&p.y);
            assert!(inside, "{a}°: the corner ({x}, {y}) shows ({}, {}) of the photo", p.x, p.y);
            turned |= (p.x - x).abs() > 0.01;
        }
        assert!(turned, "{a}°: the photo is turned");
        assert_eq!((preview.image.width, preview.image.height), (level_px.0, level_px.1), "{a}°: the preview keeps its size");
        let v = view(&t);
        assert_eq!((v.size, v.image), (level.size, level.image), "{a}°: the same size and place");
    }
    click(&mut t, straighten::APPLY_ID);
    assert_eq!(edits(&mut t), json!(["straighten(5)"]));
}

/// The tool's actions refuse what they do not take, and change nothing.
#[test]
fn the_actions_refuse_bad_params() {
    let mut t = setup((900, 600));
    open(&mut t);
    for (name, params) in [
        ("set", json!({})),
        ("set", json!({ "angle": "level" })),
        ("set", json!({ "angle": 10.5 })),
        ("set", json!({ "angle": -11 })),
        ("set", json!({ "angle": 1, "x": 2 })),
        ("set", json!({ "steps": 1 })),
        ("nudge", json!({ "steps": 1e9 })),
        ("nudge", json!({ "steps": null })),
        ("nudge", json!({})),
        ("rotate", json!({})),
    ] {
        let reply = call(&mut t, "engine.execute", json!({ "command": "edit.tool.action", "params": { "name": name, "params": params } }));
        let e = reply.expect_err(&format!("{name} {params}"));
        assert_eq!(e.code.as_str(), "bad_params", "{name} {params}");
        assert_eq!(angle(&mut t), 0.0, "{name} {params}: nothing changed");
    }
    let r = ok(&mut t, "engine.execute", json!({ "command": "edit.tool.action", "params": { "name": "nudge", "params": { "steps": 25 } } }));
    assert_eq!(r["angle"], json!(2.5));
    settle(&mut t);
    assert_eq!(shown(&t), ["straighten(2.5)"]);
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the pixels of `rect` of the window against `tests/fingerprints/<name>.json`
/// (`DALICA_BLESS=1` writes it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: Rect) {
    let shot = t.d.screenshot().unwrap();
    let [x0, y0] = [rect.min.x.round() as u32, rect.min.y.round() as u32];
    let [x1, y1] = [(rect.max.x.round() as u32).min(shot.width), (rect.max.y.round() as u32).min(shot.height)];
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
        std::fs::write(Path::new(&dir).join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
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

/// The photo area with the tool open at 0° and at +5°, at both sizes.
#[test]
fn the_tool_matches_its_fingerprints_at_0_and_5_degrees() {
    for (w, h) in SIZES {
        let mut t = setup((w, h));
        open(&mut t);
        let area = rect(&t, "region:canvas");
        check_region(&t, &format!("edit-straighten-0-{w}x{h}"), area);
        ok(&mut t, "engine.execute", json!({ "command": "edit.tool.action", "params": { "name": "set", "params": { "angle": 5 } } }));
        settle(&mut t);
        assert_eq!(shown(&t), ["straighten(5)"]);
        check_region(&t, &format!("edit-straighten-5-{w}x{h}"), area);
    }
}
