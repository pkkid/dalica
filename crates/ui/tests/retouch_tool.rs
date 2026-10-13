//! The Retouch tool (`editing.md` §6.4), driven headlessly as a user and an agent would: clicks on
//! the photo pick the blemish and then the source, each a patch inside the tool; Undo Patch and Redo
//! Patch walk them and a new patch drops the ones undone; Apply records the patches in effect as one
//! "Retouch" step of the photo's history, in the stored frame on a rotated photo; Cancel and Esc
//! leave the recipe and the preview as they were; + / − (and =) resize the brush within what the
//! operation stores; the target previews the texture under the pointer, and a planted blemish is
//! gone from the preview; the actions refuse bad parameters; the panel's fingerprints at
//! 1400 × 900 and 900 × 600.
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::edit::canvas::{self, View};
use dalica_ui::panels::edit::{histogram, retouch};
use dalica_ui::state::{BRUSH_DEFAULT, Patch};
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use egui::{Modifiers, PointerButton, Pos2, pos2};
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];
/// The test photo and its planted blemish: a dark disc at `BLEMISH` (normalised) of radius
/// `BLEMISH_R` pixels.
const PHOTO: (u32, u32) = (600, 400);
const BLEMISH: [f32; 2] = [0.3, 0.6];
const BLEMISH_R: f32 = 8.0;
/// A clean place to copy from, on the same row.
const CLEAN: [f32; 2] = [0.45, 0.6];

/// The grey of the photo at pixel `(x, y)`: a gentle slope with a fine texture, and the blemish.
fn grey(x: u32, y: u32, blemish: bool) -> u8 {
    let (bx, by) = (BLEMISH[0] * PHOTO.0 as f32, BLEMISH[1] * PHOTO.1 as f32);
    if blemish && (x as f32 + 0.5 - bx).hypot(y as f32 + 0.5 - by) <= BLEMISH_R {
        return 25;
    }
    let texture = (x.wrapping_mul(7919) ^ y.wrapping_mul(104_729)).wrapping_mul(2_654_435_761) >> 28;
    (120 + x / 30 + texture) as u8
}

/// The photo as a PNG.
fn png_bytes(blemish: bool) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, PHOTO.0, PHOTO.1);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        let mut data = Vec::new();
        for y in 0..PHOTO.1 {
            for x in 0..PHOTO.0 {
                let g = grey(x, y, blemish);
                data.extend_from_slice(&[g, g, g]);
            }
        }
        w.write_image_data(&data).unwrap();
    }
    out
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    photo: u64,
}

/// The window at `size` showing the blemished photo in the edit view, its preview drawn.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Spots");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Spot.png"), png_bytes(true)).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10 })).unwrap();
    let photo = found["photos"][0]["id"].as_u64().unwrap();
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

fn try_run(t: &mut T, id: &str, params: Value) -> Reply {
    let r = call(t, "engine.execute", json!({ "command": id, "params": params }));
    settle(t);
    r
}

fn action(t: &mut T, name: &str, params: Value) -> Value {
    run(t, "edit.tool.action", json!({ "name": name, "params": params }))
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    settle(t);
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap();
    settle(t);
}

fn state(t: &mut T) -> Value {
    ok(t, "ui.state", json!({}))
}

fn widget(t: &T, id: &str) -> WidgetInfo {
    t.d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}`"))
}

fn view(t: &T) -> View {
    canvas::view(t.d.ctx()).expect("the canvas was drawn")
}

/// The screen point of the displayed point `at` (normalised to the photo as it shows).
fn screen(t: &T, at: [f32; 2]) -> Pos2 {
    let v = view(t);
    pos2(v.image.min.x + at[0] * v.image.width(), v.image.min.y + at[1] * v.image.height())
}

/// A click of the photo at the displayed point `at`.
fn click_photo(t: &mut T, at: [f32; 2]) {
    let p = screen(t, at);
    t.d.pointer(
        &[PointerEvent { kind: PointerKind::Down, pos: p }, PointerEvent { kind: PointerKind::Up, pos: p }],
        PointerButton::Primary,
        Modifiers::NONE,
    )
    .unwrap();
    settle(t);
}

/// The pointer moved to the displayed point `at`.
fn hover_photo(t: &mut T, at: [f32; 2]) {
    let p = screen(t, at);
    t.d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    settle(t);
}

fn draft(t: &mut T) -> Value {
    state(t)["edit"]["retouch"].clone()
}

fn patches(t: &mut T) -> Vec<Patch> {
    serde_json::from_value(draft(t)["patches"].clone()).unwrap()
}

fn edits(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().library().info(PhotoId::new(t.photo)).edits.clone()
}

fn undo_text(t: &T) -> String {
    t.d.app().session().unwrap().photo_history(PhotoId::new(t.photo)).unwrap().undo_text()
}

/// The recipe the preview shows, once every render is in.
fn shown_recipe(t: &mut T) -> Vec<String> {
    assert!(t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(30)));
    settle(t);
    t.d.app().session().unwrap().preview().unwrap().recipe.clone()
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
}

/// The stored-frame point of the displayed point `d` on a photo turned a quarter clockwise
/// (`rotate(90)`: the stored left edge shows along the top).
fn stored_of_turned(d: [f32; 2]) -> [f32; 2] {
    [d[1], 1.0 - d[0]]
}

/// Two patches by clicks on a turned photo, Undo Patch, Redo Patch, a new patch dropping the one
/// undone, then Apply: one "Retouch" entry with the patches in effect, in the stored frame.
#[test]
fn patches_walk_inside_the_tool_and_apply_is_one_retouch_step() {
    let mut t = setup((1400, 900));
    let p = t.photo;
    run(&mut t, "photo.history.apply", json!({ "id": p, "op": "rotate(90)" }));
    let before = edits(&t);
    click(&mut t, "tool:retouch");
    assert_eq!(state(&mut t)["edit"]["tool"], "retouch");
    let panel = |t: &T| (widget(t, retouch::UNDO_ID).enabled, widget(t, retouch::REDO_ID).enabled);
    assert_eq!(panel(&t), (false, false));

    // Two patches: blemish, then source, twice.
    let clicks = [[0.2, 0.3], [0.6, 0.3], [0.4, 0.7], [0.4, 0.2]];
    click_photo(&mut t, clicks[0]);
    let waiting = retouch::pending(PhotoId::new(p)).expect("the target waits for its source");
    assert!(near(waiting, stored_of_turned(clicks[0])), "{waiting:?}");
    assert_eq!(draft(&mut t)["patches"], json!([]), "no patch before the source");
    click_photo(&mut t, clicks[1]);
    click_photo(&mut t, clicks[2]);
    click_photo(&mut t, clicks[3]);
    let made = patches(&mut t);
    assert_eq!((made.len(), draft(&mut t)["position"].clone()), (2, json!(2)));
    for (patch, pair) in made.iter().zip(clicks.chunks(2)) {
        assert!(near(patch.target, stored_of_turned(pair[0])), "{patch:?}");
        assert!(near(patch.source, stored_of_turned(pair[1])), "{patch:?}");
        assert_eq!(patch.radius, BRUSH_DEFAULT);
    }
    assert_eq!(retouch::pending(PhotoId::new(p)), None);
    assert_eq!(panel(&t), (true, false));
    assert_eq!(edits(&t), before, "nothing recorded before Apply");

    // Undo Patch, Redo Patch.
    click(&mut t, retouch::UNDO_ID);
    assert_eq!(draft(&mut t)["position"], 1);
    assert_eq!(panel(&t), (true, true));
    click(&mut t, retouch::REDO_ID);
    assert_eq!(draft(&mut t)["position"], 2);
    // Undo Patch, then a new patch drops the one undone.
    click(&mut t, retouch::UNDO_ID);
    action(&mut t, "patch", json!({ "target": [0.8, 0.8], "source": [0.7, 0.9] }));
    let made = patches(&mut t);
    assert_eq!(made.len(), 2);
    assert!(near(made[1].target, stored_of_turned([0.8, 0.8])));
    assert_eq!(panel(&t), (true, false));

    // Apply: one "Retouch" step, two strokes in the stored frame.
    click(&mut t, retouch::APPLY_ID);
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    let after = edits(&t);
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    assert_eq!(after[..before.len()], before[..]);
    let entry = after.last().unwrap();
    let strokes: Vec<Vec<f64>> = entry
        .strip_prefix("retouch(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap()
        .split(';')
        .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect())
        .collect();
    assert_eq!(strokes.len(), 2, "{entry}");
    for (stroke, patch) in strokes.iter().zip(&made) {
        let [sx, sy, tx, ty, r] = stroke[..] else { panic!("{entry}") };
        assert!(near([sx as f32, sy as f32], patch.source) && near([tx as f32, ty as f32], patch.target), "{entry}");
        // The brush's 0.03 of the longer side is 0.045 of the shorter.
        assert!((r - 0.045).abs() < 1e-4, "{entry}");
    }
    assert_eq!(undo_text(&t), "Undo Retouch");
    assert_eq!(shown_recipe(&mut t), after, "the preview shows the photo's own recipe");
    // One step: undone at once.
    run(&mut t, "photo.history.undo", json!({ "id": p }));
    assert_eq!(edits(&t), before);
}

/// Cancel, Esc and an empty Apply leave the recipe as it was, and the preview shows it again.
#[test]
fn cancel_and_esc_leave_the_recipe_unchanged() {
    let mut t = setup((1400, 900));
    let before = edits(&t);
    for how in ["button", "esc", "empty apply"] {
        click(&mut t, "tool:retouch");
        if how != "empty apply" {
            action(&mut t, "patch", json!({ "target": BLEMISH, "source": CLEAN }));
            let shown = shown_recipe(&mut t);
            assert!(shown.last().is_some_and(|op| op.starts_with("retouch(")), "{how}: the preview shows the patch: {shown:?}");
        }
        match how {
            "button" => click(&mut t, retouch::CANCEL_ID),
            "esc" => key(&mut t, "Esc"),
            _ => {
                let r = run(&mut t, "edit.tool.apply", json!({}));
                assert_eq!(r["applied"], false);
            }
        }
        let s = state(&mut t);
        assert_eq!((s["edit"]["tool"].clone(), s["edit"]["retouch"].clone(), s["view"].clone()), (Value::Null, Value::Null, json!("edit")), "{how}");
        assert_eq!(edits(&t), before, "{how}");
        assert_eq!(shown_recipe(&mut t), before, "{how}: the preview shows the photo's own recipe again");
    }
}

/// The brush keys: + and = make the brush larger, − smaller, by 1.2 a press, within what the
/// operation stores; the slider shows it in pixels; Reset keeps the brush and drops the patches;
/// the edit view's Undo and Redo walk the patches.
#[test]
fn the_brush_keys_resize_the_brush() {
    let mut t = setup((1400, 900));
    click(&mut t, "tool:retouch");
    let brush = |t: &mut T| draft(t)["brush"].as_f64().unwrap() as f32;
    let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
    assert!(close(brush(&mut t), BRUSH_DEFAULT));
    key(&mut t, "Plus");
    assert!(close(brush(&mut t), BRUSH_DEFAULT * 1.2), "{}", brush(&mut t));
    key(&mut t, "-");
    assert!(close(brush(&mut t), BRUSH_DEFAULT), "{}", brush(&mut t));
    key(&mut t, "=");
    assert!(close(brush(&mut t), BRUSH_DEFAULT * 1.2));
    // The slider (its value the brush across in pixels of the photo) can be typed into.
    assert!(widget(&t, retouch::BRUSH_ID).enabled && widget(&t, &format!("{}/value", retouch::BRUSH_ID)).enabled);
    // Never past the operation's 0.1 of the shorter side (0.0667 of the longer), never below its
    // smallest.
    for _ in 0..20 {
        key(&mut t, "Plus");
    }
    assert!(close(brush(&mut t), 0.1 * 400.0 / 600.0), "{}", brush(&mut t));
    for _ in 0..40 {
        key(&mut t, "-");
    }
    assert!(close(brush(&mut t), dalica_ui::state::BRUSH_MIN), "{}", brush(&mut t));
    // A patch takes the brush; Reset drops the patches and keeps the brush.
    action(&mut t, "brush", json!({ "size": 0.05 }));
    action(&mut t, "patch", json!({ "target": BLEMISH, "source": CLEAN }));
    assert_eq!(patches(&mut t)[0].radius, 0.05);
    click(&mut t, retouch::RESET_ID);
    let d = draft(&mut t);
    assert_eq!((d["patches"].clone(), d["position"].clone()), (json!([]), json!(0)));
    assert!(close(brush(&mut t), 0.05));
    // The edit view's Undo and Redo walk the patches while the tool is open.
    action(&mut t, "patch", json!({ "target": BLEMISH, "source": CLEAN }));
    let history = edits(&t);
    key(&mut t, "Ctrl+Z");
    assert_eq!(draft(&mut t)["position"], 0);
    assert_eq!(edits(&t), history, "the photo's history is untouched");
    key(&mut t, if cfg!(target_os = "macos") { "Ctrl+Shift+Z" } else { "Ctrl+Y" });
    assert_eq!(draft(&mut t)["position"], 1);
}

/// The mean grey of the displayed preview in the disc of `radius` (a fraction of the shorter side)
/// around the normalised point `at`.
fn disc_mean(t: &T, at: [f32; 2], r0: f32, r1: f32) -> f32 {
    let p = t.d.app().session().unwrap().preview().unwrap().image.clone();
    let short = p.width.min(p.height) as f32;
    let (cx, cy) = (at[0] * p.width as f32, at[1] * p.height as f32);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in 0..p.height {
        for x in 0..p.width {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / short;
            if d >= r0 && d <= r1 {
                sum += f32::from(p.data[((y * p.width + x) * 4) as usize]);
                n += 1.0;
            }
        }
    }
    sum / n
}

/// The target previews the texture under the pointer before the source is picked, and the
/// planted blemish is gone from the preview once the patch is made.
#[test]
fn a_planted_blemish_is_gone_in_the_preview() {
    let mut t = setup((1400, 900));
    click(&mut t, "tool:retouch");
    let disc = BLEMISH_R / PHOTO.1 as f32;
    let blemish = |t: &T| disc_mean(t, BLEMISH, 0.0, disc * 0.8);
    let around = |t: &T| disc_mean(t, BLEMISH, disc * 1.5, disc * 3.0);
    let (spot, clean) = (blemish(&t), around(&t));
    assert!(clean - spot > 40.0, "the blemish shows: {spot} vs {clean}");
    // The target picked: a circle stays there; the preview is still the photo's own.
    click_photo(&mut t, BLEMISH);
    assert!(retouch::pending(PhotoId::new(t.photo)).is_some());
    // The pointer over a clean area: the target shows its texture (a draft of the patch).
    hover_photo(&mut t, CLEAN);
    let shown = shown_recipe(&mut t);
    assert!(shown.last().is_some_and(|op| op.starts_with("retouch(")), "{shown:?}");
    assert!((blemish(&t) - clean).abs() < 15.0, "previewed: {} vs {clean}", blemish(&t));
    assert_eq!(draft(&mut t)["patches"], json!([]), "only a preview");
    // The source clicked: the patch is made and the blemish stays gone.
    click_photo(&mut t, CLEAN);
    assert_eq!(patches(&mut t).len(), 1);
    shown_recipe(&mut t);
    let healed = blemish(&t);
    println!("blemish {spot:.1}, healed {healed:.1}, surroundings {clean:.1}");
    assert!((healed - clean).abs() < 15.0, "healed: {healed} vs {clean}");
    // Undo Patch brings it back in the preview.
    click(&mut t, retouch::UNDO_ID);
    shown_recipe(&mut t);
    assert!(clean - blemish(&t) > 40.0);
}

/// A patch on a slow machine previews as a smaller draft and then the final of the same
/// generation: the histogram box shows the final's bins, not the draft's.
#[test]
fn the_histogram_follows_the_final_after_a_smaller_draft() {
    let mut t = setup((1400, 900));
    click(&mut t, "tool:retouch");
    {
        let s = t.d.app_mut().session_mut().unwrap();
        assert!(s.wait_preview(Duration::from_secs(30)));
        // A full render taking 100 ms: the draft is a quarter of the size.
        s.plant_preview_cost(100.0);
    }
    action(&mut t, "patch", json!({ "target": BLEMISH, "source": CLEAN }));
    shown_recipe(&mut t);
    let s = t.d.app().session().unwrap();
    let p = s.preview().unwrap();
    assert_eq!(p.quality, dalica_engine::Quality::Final);
    let shown = histogram::shown_bins(t.d.ctx()).expect("the histogram box drew bins");
    let widen = |c: &[u32]| c.iter().map(|&n| u64::from(n)).collect::<Vec<u64>>();
    assert_eq!(shown[0], widen(&p.histogram.red), "the plot shows the final's bins");
    let total: u64 = shown[0].iter().sum();
    assert_eq!(total, u64::from(p.image.width) * u64::from(p.image.height));
}

/// The actions refuse bad parameters and change nothing; with Retouch closed they are `disabled`.
#[test]
fn the_actions_refuse_bad_parameters() {
    let mut t = setup((900, 600));
    let r = try_run(&mut t, "edit.tool.action", json!({ "name": "patch", "params": { "target": [0.5, 0.5], "source": [0.5, 0.5] } }));
    assert!(r.is_err(), "no tool open");
    click(&mut t, "tool:retouch");
    let before = draft(&mut t);
    for (name, params) in [
        ("patch", json!({})),
        ("patch", json!({ "target": [0.5, 0.5] })),
        ("patch", json!({ "target": [0.5, 1.5], "source": [0.5, 0.5] })),
        ("patch", json!({ "target": [-0.1, 0.5], "source": [0.5, 0.5] })),
        ("patch", json!({ "target": [0.5], "source": [0.5, 0.5] })),
        ("patch", json!({ "target": "x", "source": [0.5, 0.5] })),
        ("patch", json!({ "target": [0.5, 0.5], "source": [0.5, 0.5], "radius": 2 })),
        ("target", json!({ "at": [1e300, 0.5] })),
        ("source", json!({ "at": [0.5, 0.5] })),
        ("undo", json!({})),
        ("redo", json!({})),
        ("undo", json!({ "x": 1 })),
        ("drop", json!({ "x": 1 })),
        ("brush", json!({})),
        ("brush", json!({ "size": 0.5 })),
        ("brush", json!({ "size": 0.0 })),
        ("brush", json!({ "step": 0 })),
        ("brush", json!({ "step": 11 })),
        ("brush", json!({ "size": 0.05, "step": 1 })),
        ("lasso", json!({})),
    ] {
        let r = try_run(&mut t, "edit.tool.action", json!({ "name": name, "params": params }));
        assert!(r.is_err(), "{name} {params}");
        assert_eq!(draft(&mut t), before, "{name} {params}: nothing changed");
    }
    // A target, then `drop` forgets it.
    action(&mut t, "target", json!({ "at": [0.5, 0.5] }));
    assert!(retouch::pending(PhotoId::new(t.photo)).is_some());
    action(&mut t, "drop", json!({}));
    assert_eq!(retouch::pending(PhotoId::new(t.photo)), None);
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the pixels of `rect` of the window against `tests/fingerprints/<name>.json`
/// (`DALICA_BLESS=1` writes it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: [f32; 4]) {
    let shot = t.d.screenshot().unwrap();
    let [x0, y0] = [rect[0].round() as u32, rect[1].round() as u32];
    let [x1, y1] = [((rect[0] + rect[2]).round() as u32).min(shot.width), ((rect[1] + rect[3]).round() as u32).min(shot.height)];
    let mut rgba = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    let (w, h) = (x1 - x0, y1 - y0);
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

/// The Retouch panel with one patch made, and the photo with a target waiting and its circle, at
/// both sizes.
#[test]
fn the_retouch_panel_matches_its_fingerprints() {
    for (w, h) in SIZES {
        let mut t = setup((w, h));
        click(&mut t, "tool:retouch");
        action(&mut t, "patch", json!({ "target": [0.7, 0.3], "source": [0.8, 0.3] }));
        click_photo(&mut t, BLEMISH);
        hover_photo(&mut t, [0.9, 0.9]);
        shown_recipe(&mut t);
        let panel = widget(&t, "region:editPanel").rect;
        check_region(&t, &format!("edit-retouch-panel-{w}x{h}"), panel);
        let photo = widget(&t, "region:canvas").rect;
        check_region(&t, &format!("edit-retouch-photo-{w}x{h}"), photo);
    }
}
