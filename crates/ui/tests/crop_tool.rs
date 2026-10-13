//! The Crop tool (`editing.md` §6.1), driven headlessly by widget id and by pointer gestures on the
//! photo as a user would: a size drawn at 4:3 keeps its ratio, Manual is free; corner, edge and
//! move drags; Rotate swaps the orientation; Preview shows the cropped photo; Apply is one "Crop"
//! step and the thumbnail follows; on a rotated photo the stored rectangle is in the stored frame;
//! a suggestion click draws its rectangle; the Add Custom Ratio dialog refuses 0 × 5 and adds
//! 3 × 2; Esc cancels; a re-opened crop shows the whole photo with the current rectangle; the
//! panel's and the dialog's fingerprints at 1400 × 900 and 900 × 600.
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too; `DALICA_BLESS=1` stores new
//! fingerprints (look at the PNGs in `target/test-output/ui/` first).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_edit::{Dihedral, Rect as NRect};
use dalica_engine::Session;
use dalica_scenes::{Scene, fingerprint};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::edit::canvas::{self, View};
use dalica_ui::panels::edit::crop;
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use egui::{Modifiers, PointerButton, Pos2, pos2};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
/// The photos' size (landscape 3:2).
const PHOTO: (u32, u32) = (600, 400);

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    ids: Vec<u64>,
}

/// A library of two landscape scenes JPEGs, the window at `size` showing the first in the edit
/// view.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..2u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(40 + i, PHOTO.0, PHOTO.1), 90, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, d, ids };
    let id = t.ids[0];
    run(&mut t, "view.edit", json!({ "id": id }));
    t
}

fn settle(t: &mut T) {
    t.d.settle(FRAMES).unwrap();
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    settle(t);
    v
}

fn action(t: &mut T, name: &str, params: Value) -> Value {
    run(t, "edit.tool.action", json!({ "name": name, "params": params }))
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap_or_else(|e| panic!("click {id}: {e}"));
    settle(t);
}

fn key(t: &mut T, chord: &str) {
    t.d.key(chord).unwrap();
    settle(t);
}

fn find(t: &T, id: &str) -> Option<WidgetInfo> {
    t.d.widgets().into_iter().find(|w| w.id == id)
}

fn widget(t: &T, id: &str) -> WidgetInfo {
    find(t, id).unwrap_or_else(|| panic!("no `{id}` in {:?}", t.d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

fn view(t: &T) -> View {
    canvas::view(t.d.ctx()).expect("the canvas was drawn")
}

/// The draft's rectangle `[x, y, w, h]` (stored frame).
fn rect(t: &T) -> [f64; 4] {
    let r = t.d.state().edit.crop.as_ref().and_then(|c| c.rect).expect("a rectangle");
    r.map(f64::from)
}

fn ratio(t: &T) -> String {
    t.d.state().edit.crop.as_ref().map(|c| c.ratio.clone()).unwrap()
}

/// The draft's rectangle on the photo as shown, in pixels: width / height.
fn shown_ratio(t: &T) -> f64 {
    let [_, _, w, h] = rect(t);
    let v = view(t);
    (w * f64::from(v.size.0)) / (h * f64::from(v.size.1))
}

/// A screen point at `(x, y)` normalised to the photo shown.
fn at(t: &T, x: f32, y: f32) -> Pos2 {
    let i = view(t).image;
    pos2(i.min.x + x * i.width(), i.min.y + y * i.height())
}

/// A press at `from`, moved in steps to `to`, released there (screen points).
fn gesture(t: &mut T, from: Pos2, to: Pos2) {
    let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: from }, PointerEvent { kind: PointerKind::Down, pos: from }];
    for i in 1..=8 {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: from + (to - from) * (i as f32 / 8.0) });
    }
    events.push(PointerEvent { kind: PointerKind::Up, pos: to });
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    settle(t);
}

/// The screen rect of the draft's rectangle.
fn screen_rect(t: &T) -> egui::Rect {
    let r = widget(t, crop::RECT_ID).rect;
    egui::Rect::from_min_size(pos2(r[0], r[1]), egui::vec2(r[2], r[3]))
}

fn open_crop(t: &mut T) {
    click(t, "tool:crop");
    assert_eq!(t.d.state().edit.tool.map(|k| k.name()), Some("crop"));
    assert!(find(t, crop::RATIO_ID).is_some(), "the Crop panel replaced the grid");
}

fn choose_row(t: &mut T, row: &str) {
    click(t, crop::RATIO_ID);
    click(t, row);
}

fn edits(t: &mut T, id: u64) -> Vec<String> {
    let v = run(t, "photo.history.state", json!({ "id": id }));
    v["edits"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn a_ratio_holds_while_drawing_and_manual_is_free() {
    let mut t = setup((1400, 900));
    open_crop(&mut t);
    assert_eq!(ratio(&t), "manual");
    assert_eq!(widget(&t, crop::RATIO_ID).label, "Manual: 600 × 400", "the live row shows the whole photo");
    assert!(!widget(&t, crop::ROTATE_ID).enabled, "Rotate waits for a rectangle");
    assert!(find(&t, crop::RECT_ID).is_none());

    // 4:3 draws its largest rectangle, centred.
    choose_row(&mut t, "choice:crop.ratio/4:3");
    assert_eq!(ratio(&t), "4:3");
    assert_eq!(widget(&t, crop::RATIO_ID).label, "4:3 (Standard screen)");
    assert!(near(shown_ratio(&t), 4.0 / 3.0, 1e-3), "{}", shown_ratio(&t));
    let [x, y, w, h] = rect(&t);
    assert!(near(x + w / 2.0, 0.5, 1e-4) && near(y, 0.0, 1e-6) && near(h, 1.0, 1e-6));
    assert!(widget(&t, crop::ROTATE_ID).enabled);

    // A press inside the rectangle moves it; outside it draws a new one, which keeps 4:3 whichever
    // way the pointer goes.
    for (from, to) in [((0.2, 0.2), (0.6, 0.9)), ((0.8, 0.7), (0.3, 0.6)), ((0.1, 0.9), (0.15, 0.2))] {
        action(&mut t, "draw", json!({ "from": [0.0, 0.0], "to": [0.05, 0.05] }));
        let (a, b) = (at(&t, from.0, from.1), at(&t, to.0, to.1));
        gesture(&mut t, a, b);
        assert!(near(shown_ratio(&t), 4.0 / 3.0, 1e-3), "{from:?} → {to:?}: {}", shown_ratio(&t));
        let [x, y, w, h] = rect(&t);
        assert!(x >= 0.0 && y >= 0.0 && x + w <= 1.0 + 1e-6 && y + h <= 1.0 + 1e-6);
        // It grows from where the press began.
        let anchored = |v: f64| near(v, f64::from(from.0), 0.005);
        assert!(anchored(x) || anchored(x + w), "{from:?}: {:?}", rect(&t));
    }

    // Manual draws what the pointer spans.
    choose_row(&mut t, "choice:crop.ratio/manual");
    action(&mut t, "draw", json!({ "from": [0.95, 0.95], "to": [1.0, 1.0] }));
    assert_eq!(ratio(&t), "manual");
    let (a, b) = (at(&t, 0.1, 0.2), at(&t, 0.7, 0.5));
    gesture(&mut t, a, b);
    let [x, y, w, h] = rect(&t);
    assert!(near(x, 0.1, 0.005) && near(y, 0.2, 0.005) && near(w, 0.6, 0.005) && near(h, 0.3, 0.005), "{:?}", rect(&t));
    assert_eq!(widget(&t, crop::RATIO_ID).label, format!("Manual: {} × {}", (w * 600.0).round(), (h * 400.0).round()));
    assert!(find(&t, crop::RECT_ID).is_some());
}

#[test]
fn corners_edges_and_the_inside_drag() {
    let mut t = setup((1400, 900));
    open_crop(&mut t);
    action(&mut t, "draw", json!({ "from": [0.25, 0.25], "to": [0.75, 0.75] }));
    let start = rect(&t);
    assert!(near(start[0], 0.25, 1e-6) && near(start[2], 0.5, 1e-6));

    // The bottom-right corner, free in Manual.
    let s = screen_rect(&t);
    let to = at(&t, 0.9, 0.85);
    gesture(&mut t, s.right_bottom(), to);
    let r = rect(&t);
    assert!(near(r[0], 0.25, 1e-4) && near(r[1], 0.25, 1e-4), "the opposite corner stays: {r:?}");
    assert!(near(r[0] + r[2], 0.9, 0.005) && near(r[1] + r[3], 0.85, 0.005), "{r:?}");

    // The left edge: only x moves.
    let s = screen_rect(&t);
    let to = at(&t, 0.1, 0.5);
    gesture(&mut t, s.left_center(), pos2(to.x, s.center().y));
    let e = rect(&t);
    assert!(near(e[0], 0.1, 0.005) && near(e[1], r[1], 1e-4) && near(e[3], r[3], 1e-4), "{e:?}");

    // The inside moves it whole, kept inside the photo.
    let s = screen_rect(&t);
    let v = view(&t);
    gesture(&mut t, s.center(), s.center() + egui::vec2(-0.5 * v.image.width(), 0.0));
    let m = rect(&t);
    assert!(near(m[0], 0.0, 1e-6) && near(m[2], e[2], 1e-4) && near(m[3], e[3], 1e-4), "stopped at the edge: {m:?}");

    // With 4:3 locked, a corner keeps the ratio.
    choose_row(&mut t, "choice:crop.ratio/4:3");
    let s = screen_rect(&t);
    gesture(&mut t, s.left_top(), s.left_top() + egui::vec2(60.0, 10.0));
    assert!(near(shown_ratio(&t), 4.0 / 3.0, 1e-3), "{}", shown_ratio(&t));

    // Rotate turns it portrait, Rotate again back.
    click(&mut t, crop::ROTATE_ID);
    assert!(near(shown_ratio(&t), 3.0 / 4.0, 1e-3), "portrait: {}", shown_ratio(&t));
    let s = screen_rect(&t);
    gesture(&mut t, s.right_bottom(), s.right_bottom() + egui::vec2(-20.0, -40.0));
    assert!(near(shown_ratio(&t), 3.0 / 4.0, 1e-3), "a drag keeps the portrait ratio: {}", shown_ratio(&t));
    action(&mut t, "rotate", json!({}));
    assert!(near(shown_ratio(&t), 4.0 / 3.0, 1e-3));
}

#[test]
fn preview_apply_and_the_thumbnail_and_a_reopened_crop() {
    let mut t = setup((1400, 900));
    let id = t.ids[0];
    let whole = view(&t).size;
    open_crop(&mut t);
    action(&mut t, "draw", json!({ "from": [0.5, 0.25], "to": [1.0, 0.75] }));
    assert_eq!(view(&t).size, whole, "the tool shows the whole photo");

    // Preview: the photo area shows the cropped photo, without the rectangle; again shows the whole.
    click(&mut t, crop::PREVIEW_ID);
    assert!(widget(&t, crop::PREVIEW_ID).active);
    assert_eq!(view(&t).size, (300.0, 200.0), "previewed at the crop's size");
    assert!(find(&t, crop::RECT_ID).is_none());
    click(&mut t, crop::PREVIEW_ID);
    assert_eq!(view(&t).size, whole);
    assert!(find(&t, crop::RECT_ID).is_some());

    // Apply: one "Crop" step, the tool closes, the photo shows cropped, its thumbnail follows.
    let remade = t.d.app().session().unwrap().thumbnail_stats().remade;
    click(&mut t, crop::APPLY_ID);
    assert_eq!(t.d.state().edit.tool, None);
    let after = edits(&mut t, id);
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].starts_with("crop(0.5,0.25,1,0.75"), "{after:?}");
    assert_eq!(widget(&t, "button:edit.undo").label, "Undo Crop");
    assert_eq!(view(&t).size, (300.0, 200.0));
    run(&mut t, "view.library", json!({}));
    let painted = t.d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo.get() == id).map(|p| p.rect).unwrap();
    assert!(near(f64::from(painted.width() / painted.height()), 1.5, 0.02), "{painted:?}");
    assert!(t.d.app().session().unwrap().thumbnail_stats().remade > remade, "the thumbnail was made again");

    // Re-opened: the whole photo with the rectangle in effect; Reset comes back to it.
    run(&mut t, "view.edit", json!({ "id": id }));
    open_crop(&mut t);
    assert_eq!(view(&t).size, whole);
    let r = rect(&t);
    assert!(near(r[0], 0.5, 1e-5) && near(r[1], 0.25, 1e-5) && near(r[2], 0.5, 1e-5) && near(r[3], 0.5, 1e-5), "{r:?}");
    action(&mut t, "draw", json!({ "from": [0.0, 0.0], "to": [0.3, 0.3] }));
    click(&mut t, crop::RESET_ID);
    assert!(near(rect(&t)[0], 0.5, 1e-5));
    // Enter applies a moved rectangle: a second "Crop" step after the first.
    action(&mut t, "drag", json!({ "handle": "move", "from": [0.75, 0.5], "to": [0.5, 0.5] }));
    key(&mut t, "Enter");
    let again = edits(&mut t, id);
    assert_eq!(again.len(), 2, "{again:?}");
    assert!(again[1].starts_with("crop(0.25,0.25,0.75,0.75"), "{again:?}");
    // Undo brings the first crop back.
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, id), after);
}

#[test]
fn esc_cancels_and_records_nothing() {
    let mut t = setup((1400, 900));
    let id = t.ids[0];
    run(&mut t, "photo.crop.apply", json!({ "id": id, "rect": [0.0, 0.0, 0.5, 0.5] }));
    let cropped = view(&t).size;
    assert_eq!(cropped, (300.0, 200.0));
    let before = edits(&mut t, id);
    open_crop(&mut t);
    assert_eq!(view(&t).size, (600.0, 400.0), "the whole photo while cropping");
    action(&mut t, "draw", json!({ "from": [0.1, 0.1], "to": [0.9, 0.9] }));
    key(&mut t, "Esc");
    assert_eq!(t.d.state().edit.tool, None);
    assert_eq!(t.d.state().edit.crop, None);
    assert_eq!(edits(&mut t, id), before, "nothing recorded");
    assert_eq!(view(&t).size, cropped, "the photo's own crop shows again");
    assert_eq!(t.d.state().view, dalica_ui::state::View::Edit, "Esc closed the tool, not the edit view");
    // The panel's Cancel does the same.
    open_crop(&mut t);
    click(&mut t, crop::CANCEL_ID);
    assert_eq!((t.d.state().edit.tool, view(&t).size), (None, cropped));
}

#[test]
fn on_a_rotated_photo_the_rectangle_is_stored_in_the_stored_frame() {
    let mut t = setup((1400, 900));
    let id = t.ids[0];
    run(&mut t, "photo.history.apply", json!({ "id": id, "op": "rotate(90)" }));
    open_crop(&mut t);
    let v = view(&t);
    assert_eq!(v.size, (400.0, 600.0), "shown upright after the turn");
    // The top half of the photo as shown.
    let (a, b) = (at(&t, 0.0, 0.0), at(&t, 1.0, 0.5));
    gesture(&mut t, a, b);
    let [x, y, w, h] = rect(&t);
    let shown = NRect::new(x, y, x + w, y + h).to_display(Dihedral::ROTATE_90);
    assert!(near(shown.x0, 0.0, 0.005) && near(shown.y0, 0.0, 0.005) && near(shown.x1, 1.0, 0.005) && near(shown.y1, 0.5, 0.005), "{shown:?}");
    // In the stored frame (before the quarter turn) the top half shown is the left half.
    assert!(near(x, 0.0, 0.005) && near(y, 0.0, 0.005) && near(w, 0.5, 0.005) && near(h, 1.0, 0.005), "{:?}", rect(&t));
    click(&mut t, crop::APPLY_ID);
    let e = edits(&mut t, id);
    assert_eq!(e.first().map(String::as_str), Some("rotate(90)"));
    assert!(e.get(1).is_some_and(|c| c.starts_with("crop(0,0,0.5")), "{e:?}");
    assert_eq!(view(&t).size, (400.0, 300.0));
}

#[test]
fn a_suggestion_click_draws_its_rectangle() {
    let mut t = setup((1400, 900));
    open_crop(&mut t);
    choose_row(&mut t, "choice:crop.ratio/1:1");
    t.d.step_until(|_| crop::suggestions().len() == 3, FRAMES).unwrap();
    settle(&mut t);
    let list = crop::suggestions();
    for (i, s) in list.iter().enumerate() {
        let w = widget(&t, &crop::suggestion_id(i + 1));
        assert!(w.enabled && w.label.starts_with("Suggested crop"), "{w:?}");
        let ratio = f64::from(s.rect[2]) * 600.0 / (f64::from(s.rect[3]) * 400.0);
        assert!(near(ratio, 1.0, 0.02), "suggested in the ratio chosen: {ratio}");
    }
    click(&mut t, &crop::suggestion_id(2));
    assert_eq!(t.d.state().edit.crop.as_ref().and_then(|c| c.rect), Some(list[1].rect));
    action(&mut t, "suggestion", json!({ "index": 0 }));
    assert_eq!(t.d.state().edit.crop.as_ref().and_then(|c| c.rect), Some(list[0].rect));
}

#[test]
fn the_custom_ratio_dialog_refuses_0_by_5_and_adds_3_by_2() {
    let mut t = setup((1400, 900));
    open_crop(&mut t);
    assert!(!widget(&t, crop::REMOVE_ID).enabled, "no custom ratio chosen");
    choose_row(&mut t, "choice:crop.ratio/add");
    assert!(find(&t, "dialog:customRatio").is_some());
    for (id, text) in [("field:dialog.width", "0"), ("field:dialog.height", "5"), ("field:dialog.name", "Thin")] {
        click(&mut t, id);
        t.d.type_text(text).unwrap();
        settle(&mut t);
    }
    assert!(!widget(&t, "button:dialog.ok").enabled, "0 × 5 is refused");
    assert!(dalica_ui::dialogs::answer(t.d.app_mut(), None).is_err(), "OK refuses");
    assert!(dalica_ui::dialogs::top(t.d.app()).is_some(), "and the dialog stays");
    // 3 × 2 named Print.
    let d = t.d.app_mut().state_mut().dialogs.last_mut().unwrap();
    d.fields.insert("width".into(), json!("3"));
    d.fields.insert("height".into(), json!("2"));
    d.fields.insert("name".into(), json!("Print"));
    settle(&mut t);
    assert!(widget(&t, "button:dialog.ok").enabled);
    click(&mut t, "button:dialog.ok");
    assert!(find(&t, "dialog:customRatio").is_none());
    assert_eq!(ratio(&t), "3:2", "the new ratio is chosen");
    assert_eq!(widget(&t, crop::RATIO_ID).label, "Print (3 × 2)");
    assert!(widget(&t, crop::REMOVE_ID).enabled);
    let list = run(&mut t, "crop.ratios.list", json!({}));
    assert_eq!(list["custom"], json!([{ "name": "Print", "ratio": "3:2", "width": 3.0, "height": 2.0 }]));
    // Its row is in the dropdown under Custom ratios; the trash deletes it.
    click(&mut t, crop::RATIO_ID);
    assert!(widget(&t, "choice:crop.ratio/custom/Print").active);
    click(&mut t, crop::RATIO_ID);
    click(&mut t, crop::REMOVE_ID);
    assert_eq!(ratio(&t), "manual");
    assert_eq!(run(&mut t, "crop.ratios.list", json!({}))["custom"], json!([]));
}

#[test]
fn the_last_ratio_is_remembered() {
    let mut t = setup((1400, 900));
    open_crop(&mut t);
    choose_row(&mut t, "choice:crop.ratio/16:9");
    click(&mut t, crop::APPLY_ID);
    let id = t.ids[1];
    run(&mut t, "view.edit", json!({ "id": id }));
    open_crop(&mut t);
    assert_eq!(ratio(&t), "16:9");
    assert!(t.d.state().edit.crop.as_ref().unwrap().rect.is_none(), "an uncropped photo starts without a rectangle");
}

#[test]
fn the_actions_refuse_bad_params_and_change_nothing() {
    let mut t = setup((1400, 900));
    assert!(t.d.run("edit.tool.action", &json!({ "name": "rotate" })).is_err(), "no tool open");
    open_crop(&mut t);
    assert!(t.d.run("edit.tool.action", &json!({ "name": "rotate" })).is_err(), "no rectangle yet");
    assert!(t.d.run("edit.tool.action", &json!({ "name": "drag", "params": { "handle": "move", "from": [0.5, 0.5], "to": [0.6, 0.6] } })).is_err());
    for bad in [
        json!({ "name": "ratio", "params": {} }),
        json!({ "name": "ratio", "params": { "ratio": "wide" } }),
        json!({ "name": "ratio", "params": { "ratio": "0:3" } }),
        json!({ "name": "ratio", "params": { "ratio": 4 } }),
        json!({ "name": "draw", "params": { "from": [0.1], "to": [0.5, 0.5] } }),
        json!({ "name": "draw", "params": { "from": [0.1, 0.1], "to": [5.0, 0.5] } }),
        json!({ "name": "draw", "params": { "from": ["a", 0.1], "to": [0.5, 0.5] } }),
        json!({ "name": "draw", "params": { "from": [0.1, 0.1], "to": [0.5, 0.5], "extra": 1 } }),
        json!({ "name": "drag", "params": { "handle": "middle", "from": [0.1, 0.1], "to": [0.5, 0.5] } }),
        json!({ "name": "preview", "params": { "on": "yes" } }),
        json!({ "name": "suggestion", "params": { "index": 7 } }),
        json!({ "name": "suggestion", "params": { "index": -1 } }),
        json!({ "name": "removeRatio", "params": {} }),
        json!({ "name": "lasso", "params": {} }),
    ] {
        let before = t.d.state().edit.clone();
        assert!(t.d.run("edit.tool.action", &bad).is_err(), "{bad}");
        assert_eq!(t.d.state().edit, before, "{bad}: nothing changed");
    }
    // The tool is still open, its draft untouched.
    assert_eq!((ratio(&t).as_str(), t.d.state().edit.crop.as_ref().unwrap().rect), ("manual", None));
}

// --- fingerprints ---------------------------------------------------------------------------------

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// The whole window as a PNG in the test output (and `$DALICA_SHOTS`), for the eye; no fingerprint.
fn window_shot(t: &T, name: &str) {
    let (w, h, _) = t.d.size();
    let (w, h, rgba) = pixels(t, [0.0, 0.0, w as f32, h as f32]);
    let png = fingerprint::png_rgba(w, h, &rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
}

#[test]
fn the_crop_panel_the_photo_and_the_dialog_match_their_fingerprints() {
    for size in SIZES {
        let mut t = setup(size);
        let tag = format!("{}x{}", size.0, size.1);
        open_crop(&mut t);
        choose_row(&mut t, "choice:crop.ratio/4:3");
        t.d.step_until(|_| crop::suggestions().len() == 3, FRAMES).unwrap();
        action(&mut t, "draw", json!({ "from": [0.15, 0.2], "to": [0.7, 0.9] }));
        // The pointer away from every widget, so nothing shows hovered.
        let corner = pos2(size.0 as f32 - 2.0, size.1 as f32 / 2.0);
        t.d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: corner }], PointerButton::Primary, Modifiers::NONE).unwrap();
        settle(&mut t);
        window_shot(&t, &format!("crop-window-{tag}"));
        check_region(&t, &format!("edit-crop-panel-{tag}"), widget(&t, "region:editPanel").rect);
        check_region(&t, &format!("edit-crop-photo-{tag}"), widget(&t, "region:canvas").rect);
        // The dialog over the whole window, its draft filled.
        choose_row(&mut t, "choice:crop.ratio/add");
        let d = t.d.app_mut().state_mut().dialogs.last_mut().unwrap();
        d.fields.insert("width".into(), json!("3"));
        d.fields.insert("height".into(), json!("2"));
        d.fields.insert("name".into(), json!("Print"));
        // No field keeps the focus, so no caret blinks in the picture.
        t.d.step();
        t.d.ctx().memory_mut(|m| m.stop_text_input());
        t.d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: corner }], PointerButton::Primary, Modifiers::NONE).unwrap();
        settle(&mut t);
        window_shot(&t, &format!("crop-dialog-window-{tag}"));
        check_region(&t, &format!("dialog-custom-ratio-{tag}"), widget(&t, "dialog:customRatio").rect);
    }
}
