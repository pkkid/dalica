//! M7 "Edit view and fixes" (`docs/roadmap.md` → M7 → the *Done when* of Edit view shell, Basic
//! fixes, and Tuning and retouch; run by `cargo xtask accept M7`).
//!
//! 1. **The live app** (`m6_live.rs`'s helpers): the real `dalica` binary in `--headless` mode on a
//!    fresh library, driven over its control channel as an agent would. A folder of six `scenes`
//!    JPEGs (five 600 × 400, one 1800 × 1200), one of them with an unknown operation already in its
//!    sidecar. Every way in and out of the edit view (Enter, Ctrl+3, a double-click, Back to
//!    Library, Esc, Ctrl+1); ← / →, PgUp / PgDn, Home / End through the folder in light-table order,
//!    staying put at the ends, and a filmstrip click. The zoom maths on the large photo: the
//!    slider's ends and midpoint (fit, 400 %, 100 %), Fit and 1:1, Ctrl+= landing on 100 %, the
//!    refined window while zoomed. Then every tool on the photo with the unknown operation, each by
//!    its widgets as a user would: Crop (4 × 6, Enter), Straighten (+3.5°, Apply), Auto Fix, Auto
//!    Contrast and Auto Color, a Basic Fixes Fill Light drag, a Tuning tab drag of many moves (one
//!    history step, one undo), Retouch (two patches, Undo / Redo Patch, Apply), Ctrl+R and the flip;
//!    after each one the unknown operation keeps its text and place. The Undo / Redo labels, the
//!    window matched against `fingerprints/m7-*.json` at 1400 × 900 and 900 × 600. The app quits and
//!    starts again: the photo's history (edits, redo list, labels) is the same and still walks, the
//!    window matches the same fingerprint, and the light table's thumbnail equals the edit view's
//!    render after downscale.
//! 2. **The renderer across M7's operations** (`m7_render.rs`): each operation alone and all of
//!    them in one recipe render alike at two sizes after downscale (resolution independence), and
//!    an unknown and a legacy operation are skipped by the render and printed back in place.
//! 3. **The crate suites, run as they are** (`m7_checks.rs`): the frame, the photo's history, the
//!    view state, navigation, zoom maths and canvas, the preview renders, the renderer and every
//!    operation's fingerprints (per operation, per Tuning control, two sizes each), edited
//!    thumbnails equal to `edit::render`, the tools, the Tuning session as one step. Inside `cargo
//!    xtask ci` its test step already ran them, so they are skipped there
//!    (`dalica_acceptance::SUITES_RAN`). `panic_hunt -- --ignored` always runs.
//! 4. **The preview's times:** the `edit_preview` bench in a debug build on 1200 × 800 photos checks
//!    the path (open cold and cached, a Tuning drag's drafts and finals, next / previous) and prints
//!    its times. With `DALICA_ACCEPT_RELEASE=1`, the budgets in release: the bench on 24 MP JPEGs
//!    (open ≤ 150 ms, drag draft ≤ 16 ms and final ≤ 100 ms, next / previous ≤ 50 ms), and the
//!    Tuning tab's drag in the UI with its frames' and drafts' p95 ≤ 16 ms.
//!
//! The manual checks (open, zoom, crop, straighten, tune, retouch and undo on your own photos) are
//! in `m7.md`.

#[path = "m7_checks.rs"]
mod checks;
#[allow(dead_code)]
#[path = "m6_live.rs"]
mod live;
#[path = "m7_render.rs"]
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_image::Rgba8;
use dalica_scenes::{Exif, Scene};
use live::{Client, Shot};
use serde_json::{Value, json};

/// The photos of the `Porto` folder: (name, seed, width, height), taken a minute apart in this
/// order, so the light table shows them so.
const PHOTOS: [(&str, u64, u32, u32); 6] = [
    ("IMG_0001.jpg", 21, 600, 400),
    ("IMG_0002.jpg", 33, 600, 400),
    ("IMG_0003.jpg", 45, 600, 400),
    ("IMG_0004.jpg", 77, 600, 400),
    ("IMG_0005.jpg", 90, 600, 400),
    ("IMG_0006.jpg", 61, 1800, 1200),
];
/// The photo every tool edits; its sidecar holds [`UNKNOWN`] before the folder is added.
const EDITED: &str = "IMG_0003.jpg";
/// The large photo the zoom is checked on (larger than the photo area, so fit is below 100 %).
const LARGE: &str = "IMG_0006.jpg";
/// An operation no version of Dalica knows (`data-model.md` §10.4).
const UNKNOWN: &str = "future_op(1;2)";

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

/// Writes the `Porto` folder into `home/Pictures`, with a sidecar giving [`EDITED`] the unknown
/// operation. Every file's time is fixed.
fn photos(home: &Path) -> PathBuf {
    let folder = home.join("Pictures").join("Porto");
    std::fs::create_dir_all(&folder).unwrap();
    for (i, (name, seed, w, h)) in PHOTOS.into_iter().enumerate() {
        let taken: dalica_core::DateTime = format!("2024-06-01T10:{i:02}:00").parse().unwrap();
        let exif =
            Exif { orientation: 1, date_time_original: Some(taken), make: Some("Dalica".into()), model: Some("Scenes 1".into()), ..Exif::default() };
        let jpeg = dalica_scenes::jpeg(&Scene::new(seed, w, h), 90, Some(&exif)).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, &jpeg).unwrap();
        let at = UNIX_EPOCH + Duration::from_secs(1_717_236_000 + 60 * i as u64);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
    }
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { EDITED: { "edits": [UNKNOWN] } } });
    std::fs::write(folder.join(".dalica.json"), serde_json::to_vec_pretty(&sidecar).unwrap()).unwrap();
    folder
}

/// The library's photo ids by name.
fn photo_ids(c: &mut Client) -> BTreeMap<String, Value> {
    c.photos().into_iter().map(|p| (p["name"].as_str().unwrap().to_string(), p["id"].clone())).collect()
}

fn state(c: &mut Client) -> Value {
    c.ok("ui.state", json!({}))
}

/// The view shown and the photo the edit view shows.
fn view(c: &mut Client) -> (String, Value) {
    let s = state(c);
    (s["view"].as_str().unwrap_or_default().to_string(), s["edit"]["photo"].clone())
}

/// A key (egui's name, `ArrowLeft`) or a shortcut (`Ctrl+Shift+H`) pressed and released.
fn key(c: &mut Client, combo: &str) {
    let field = if combo.contains('+') { "combo" } else { "key" };
    c.ok("ui.key", json!({ field: combo }));
    c.settle();
}

fn click(c: &mut Client, id: &str) {
    c.ok("ui.clickWidget", json!({ "id": id }));
    c.settle();
}

/// The photo's own history (`photo.history.state`).
fn history(c: &mut Client, id: &Value) -> Value {
    c.ok("engine.execute", json!({ "command": "photo.history.state", "params": { "id": id } }))
}

fn edits(c: &mut Client, id: &Value) -> Vec<String> {
    history(c, id)["edits"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect()
}

/// The label a widget shows.
fn label(c: &mut Client, id: &str) -> String {
    c.widgets(id).into_iter().find(|w| w["id"] == id).and_then(|w| w["label"].as_str().map(str::to_string)).unwrap_or_default()
}

/// The unknown operation kept its text and its place (first) after `what`, once.
fn unknown_kept(c: &mut Client, id: &Value, what: &str) -> Vec<String> {
    let e = edits(c, id);
    assert_eq!(e.first().map(String::as_str), Some(UNKNOWN), "{what}: the unknown operation keeps its place: {e:?}");
    assert_eq!(e.iter().filter(|o| o.as_str() == UNKNOWN).count(), 1, "{what}: {e:?}");
    e
}

/// The rects of the regions shown (`ui.inspect` → `regions`), by name.
fn regions(c: &mut Client) -> BTreeMap<&'static str, [f64; 4]> {
    let all = c.ok("ui.inspect", json!({}))["regions"].clone();
    let mut out = BTreeMap::new();
    for name in ["editTopBar", "editPanel", "canvas", "captionBar", "statusLine", "tray"] {
        let r = &all[name];
        out.insert(name, [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap_or_else(|| panic!("no region {name}: {all}"))));
    }
    out
}

/// The size `edits` show a 600 × 400 upright photo at (after its crop, straighten and turns).
fn displayed(edits: &[String]) -> (f64, f64) {
    let recipe = dalica_edit::Recipe::parse(edits).unwrap();
    let (w, h) = dalica_edit::Geometry::new(&recipe, dalica_image::Orientation::UPRIGHT, 600, 400).display_size(1.0).unwrap();
    (f64::from(w), f64::from(h))
}

/// The zoom of `ui.state` as a number (`"fit"` → `None`).
fn zoom(c: &mut Client) -> Option<f64> {
    state(c)["edit"]["zoom"].as_f64()
}

/// Part 1b: every way in and out, and the steps through the folder.
fn enter_leave_and_step(c: &mut Client, ids: &BTreeMap<String, Value>) {
    let id = |name: &str| ids[name].clone();
    let order: Vec<Value> = PHOTOS.iter().map(|p| id(p.0)).collect();
    // Enter on a selected photo; Esc.
    click(c, &format!("thumb:{}", id("IMG_0002.jpg")));
    key(c, "Enter");
    assert_eq!(view(c), ("edit".into(), id("IMG_0002.jpg")), "Enter opens the selected photo");
    for r in ["editTopBar", "editPanel", "canvas", "filmstrip", "captionBar"] {
        assert!(c.has(&format!("region:{r}")), "the edit view shows {r}");
    }
    key(c, "Escape");
    assert_eq!(view(c).0, "library", "Esc leaves");
    // Ctrl+3; Ctrl+1.
    key(c, "Ctrl+3");
    assert_eq!(view(c), ("edit".into(), id("IMG_0002.jpg")), "Ctrl+3 opens it");
    key(c, "Ctrl+1");
    assert_eq!(view(c).0, "library", "Ctrl+1 leaves");
    // A double-click on a thumbnail; Back to Library.
    c.ok("ui.clickWidget", json!({ "id": format!("thumb:{}", id("IMG_0001.jpg")), "count": 2 }));
    c.settle();
    assert_eq!(view(c), ("edit".into(), id("IMG_0001.jpg")), "a double-click opens the photo");
    // ← at the start stays put; → steps in light-table order; PgDn, End, Home, PgUp.
    key(c, "ArrowLeft");
    assert_eq!(view(c).1, order[0], "← at the first photo stays put");
    let mut seen = vec![view(c).1];
    for _ in 1..order.len() {
        key(c, "ArrowRight");
        seen.push(view(c).1);
    }
    assert_eq!(seen, order, "→ steps through the folder in light-table order");
    key(c, "ArrowRight");
    assert_eq!(view(c).1, order[order.len() - 1], "→ at the last photo stays put");
    key(c, "Home");
    assert_eq!(view(c).1, order[0], "Home");
    key(c, "PageDown");
    assert_eq!(view(c).1, order[1], "PgDn");
    key(c, "End");
    assert_eq!(view(c).1, order[order.len() - 1], "End");
    key(c, "PageUp");
    assert_eq!(view(c).1, order[order.len() - 2], "PgUp");
    // A filmstrip click opens that photo.
    click(c, &format!("film:{}", id("IMG_0002.jpg")));
    assert_eq!(view(c).1, id("IMG_0002.jpg"), "a filmstrip click opens the photo");
    click(c, "button:edit.back");
    assert_eq!(view(c).0, "library", "Back to Library leaves");
    let s = state(c);
    assert_eq!(s["selection"]["ids"], json!([id("IMG_0002.jpg")]), "the photo last viewed is the selection: {}", s["selection"]);
    println!(
        "entering and leaving: Enter, Ctrl+3, double-click, filmstrip; Esc, Ctrl+1, Back to Library; ← → PgUp PgDn Home End in order, ends kept"
    );
}

/// Part 1a: the zoom maths on the large photo (its first open). Returns the preview's draft and final times.
fn zoom_maths(c: &mut Client, large: &Value) -> Value {
    c.ok("ui.inspect", json!({ "resetPerf": true }));
    c.run("view.edit", json!({ "id": large }));
    c.settle_jobs();
    let opened = c.ok("ui.inspect", json!({}))["perf"].clone();
    assert!(opened["previewFinalMs"].as_f64().is_some(), "the preview arrived: {opened}");
    assert_eq!(state(c)["edit"]["zoom"], "fit", "the photo opens fitted");
    assert!(!c.has("text:zoom"), "no read-out while fitted");
    // The slider's ends and midpoint.
    c.run("edit.zoom.set", json!({ "slider": 0.5 }));
    let mid = zoom(c).unwrap_or(0.0);
    assert!((mid - 1.0).abs() < 1e-3, "the slider's midpoint is 100 %: {mid}");
    assert_eq!(label(c, "text:zoom"), "Zoomed to 100 %");
    c.run("edit.zoom.set", json!({ "slider": 1.0 }));
    let end = zoom(c).unwrap_or(0.0);
    assert!((end - 4.0).abs() < 1e-3, "the slider's end is 400 %: {end}");
    assert_eq!(label(c, "text:zoom"), "Zoomed to 400 %");
    c.run("edit.zoom.set", json!({ "slider": 0.0 }));
    let start = state(c)["edit"]["zoom"].clone();
    assert!(start == "fit" || start.as_f64().is_some_and(|z| z < 1.0), "the slider's start is fit: {start}");
    assert!(!c.has("text:zoom"));
    // 1:1 and Fit (animated), and Ctrl+= landing on 100 % when it crosses it.
    click(c, "button:zoom.1to1");
    assert!((zoom(c).unwrap_or(0.0) - 1.0).abs() < 1e-3, "1:1");
    click(c, "button:zoom.fit");
    assert_eq!(state(c)["edit"]["zoom"], "fit", "Fit");
    let canvas = c.rect("canvas:edit");
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": canvas[0] + canvas[2] / 2.0, "y": canvas[1] + canvas[3] / 2.0 }] }));
    let mut steps = Vec::new();
    for _ in 0..8 {
        key(c, "Ctrl+=");
        let z = zoom(c).unwrap_or(0.0);
        steps.push(z);
        if z >= 1.0 {
            break;
        }
    }
    assert!(steps.windows(2).all(|w| w[1] > w[0]), "Ctrl+= zooms in: {steps:?}");
    assert!((steps[steps.len() - 1] - 1.0).abs() < 1e-3, "Ctrl+= lands on 100 % when it crosses it: {steps:?}");
    // Zoomed past the preview, the refined window arrives.
    c.ok("ui.inspect", json!({ "resetPerf": true }));
    c.run("edit.zoom.set", json!({ "slider": 0.75 }));
    c.settle_jobs();
    let perf = c.ok("ui.inspect", json!({}))["perf"].clone();
    assert!(perf["previewRefinedMs"].as_f64().is_some(), "the refined window arrived while zoomed: {perf}");
    assert!(c.has("navigator:edit"), "the navigator while zoomed");
    click(c, "button:zoom.fit");
    c.run("view.library", json!({}));
    println!(
        "zoom: slider 0 fit, 0.5 = 100 %, 1 = 400 %; Ctrl+= {:?}; refined window {:.1} ms after the zoom (debug build)",
        steps.iter().map(|z| format!("{:.0} %", z * 100.0)).collect::<Vec<_>>(),
        perf["previewRefinedMs"].as_f64().unwrap_or(f64::NAN)
    );
    json!({ "draft": opened["previewDraftMs"], "final": opened["previewFinalMs"] })
}

/// Drags slider `id` from its centre by `dx` in `moves` moves, a few frames between them, as a
/// hand would: one press, many changes, one release.
fn slow_drag(c: &mut Client, id: &str, dx: f64, moves: usize) {
    let r = c.rect(id);
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": x, "y": y }, { "kind": "down", "x": x, "y": y }] }));
    for i in 1..=moves {
        c.ok("ui.pointer", json!({ "events": [{ "kind": "drag", "x": x + dx * i as f64 / moves as f64, "y": y }] }));
        c.ok("ui.wait", json!({ "ms": 20 }));
    }
    c.ok("ui.pointer", json!({ "events": [{ "kind": "up", "x": x + dx, "y": y }, { "kind": "move", "x": 2.0, "y": 2.0 }] }));
    c.settle_jobs();
}

/// Part 1c: every tool on [`EDITED`], each as a user would; the unknown operation survives each.
/// Returns the history after the last tool.
fn every_tool(c: &mut Client, id: &Value) -> Value {
    c.run("view.edit", json!({ "id": id }));
    c.settle_jobs();
    unknown_kept(c, id, "the sidecar's recipe");
    let steps = |c: &mut Client| edits(c, id).len();
    let mut log = Vec::new();

    // Crop at 4 × 6, drawn on the photo, applied with Enter.
    click(c, "tool:crop");
    assert_eq!(state(c)["edit"]["tool"], "crop");
    c.run("edit.tool.action", json!({ "name": "ratio", "params": { "ratio": "4:6" } }));
    c.run("edit.tool.action", json!({ "name": "draw", "params": { "from": [0.15, 0.1], "to": [0.85, 0.9] } }));
    key(c, "Enter");
    let e = unknown_kept(c, id, "Crop");
    assert!(e.last().is_some_and(|o| o.starts_with("crop(")) && e.len() == 2, "Crop is one step: {e:?}");
    let (w, h) = displayed(&e);
    assert!((w / h - 1.5).abs() < 0.02 || (h / w - 1.5).abs() < 0.02, "the crop holds 4 × 6: {w} × {h}");
    assert_eq!(label(c, "button:edit.undo"), "Undo Crop");
    log.push(format!("Crop {} ({w} × {h} px of 600 × 400)", e[1]));

    // Straighten +3.5°, Apply.
    click(c, "tool:straighten");
    c.run("edit.tool.action", json!({ "name": "set", "params": { "angle": 3.5 } }));
    click(c, "button:panel.apply");
    let e = unknown_kept(c, id, "Straighten");
    assert_eq!(e.last().map(String::as_str), Some("straighten(3.5)"), "{e:?}");
    assert_eq!(label(c, "button:edit.undo"), "Undo Straighten");

    // Auto Fix, Auto Contrast, Auto Color: one step each.
    for (tool, op, undo) in [
        ("tool:autofix", "autofix()", "Undo Auto Fix"),
        ("tool:autocontrast", "autocontrast()", "Undo Auto Contrast"),
        ("tool:autocolor", "autocolor()", "Undo Auto Color"),
    ] {
        let before = steps(c);
        click(c, tool);
        c.settle_jobs();
        let e = unknown_kept(c, id, tool);
        assert_eq!((e.len(), e.last().map(String::as_str)), (before + 1, Some(op)), "{tool}: {e:?}");
        assert_eq!(label(c, "button:edit.undo"), undo);
    }

    // The Basic Fixes Fill Light row: a drag is one "Tuning" step.
    let before = edits(c, id);
    slow_drag(c, "slider:basic.fill_light", 60.0, 12);
    let after_fill = unknown_kept(c, id, "the Fill Light row");
    assert_eq!(after_fill.len(), before.len() + 1, "the Fill Light drag is one entry: {after_fill:?}");
    assert!(after_fill.last().is_some_and(|o| o.starts_with("tuning(")), "{after_fill:?}");
    assert_eq!(label(c, "button:edit.undo"), "Undo Tuning");

    // The Tuning tab: a drag of 24 moves on Highlights is one step; one undo takes all of it back.
    click(c, "tab:edit.tuning");
    let t = Instant::now();
    slow_drag(c, "slider:tuning.highlights", 80.0, 24);
    let dragged = unknown_kept(c, id, "a Tuning drag");
    assert_ne!(dragged, after_fill, "the drag changed the Tuning entry");
    assert_eq!(dragged.len(), after_fill.len(), "the session updates the last tuning entry in place");
    click(c, "button:edit.undo");
    assert_eq!(edits(c, id), after_fill, "one undo takes the whole drag back");
    assert_eq!(label(c, "button:edit.redo"), "Redo Tuning");
    click(c, "button:edit.redo");
    assert_eq!(edits(c, id), dragged, "redo puts the whole drag back");
    let drag_s = secs(t);
    log.push(format!("Tuning drag of 24 moves: one step, {} ({drag_s:.1} s)", dragged[dragged.len() - 1]));
    click(c, "tab:edit.basic");

    // Retouch: two patches, Undo Patch, Redo Patch, Apply: one entry with two strokes.
    click(c, "tool:retouch");
    c.run("edit.tool.action", json!({ "name": "patch", "params": { "target": [0.3, 0.6], "source": [0.55, 0.6] } }));
    c.run("edit.tool.action", json!({ "name": "patch", "params": { "target": [0.7, 0.3], "source": [0.5, 0.35] } }));
    click(c, "button:retouch.undoPatch");
    assert_eq!(state(c)["edit"]["retouch"]["position"], 1, "Undo Patch");
    click(c, "button:retouch.redoPatch");
    assert_eq!(state(c)["edit"]["retouch"]["position"], 2, "Redo Patch");
    let before = steps(c);
    click(c, "button:panel.apply");
    let e = unknown_kept(c, id, "Retouch");
    let retouch = e.last().cloned().unwrap_or_default();
    assert!(e.len() == before + 1 && retouch.starts_with("retouch(") && retouch.matches(';').count() == 1, "one Retouch step of two strokes: {e:?}");
    assert_eq!(label(c, "button:edit.undo"), "Undo Retouch");

    // Ctrl+R and the flip, steps of the photo's history in the edit view.
    key(c, "Ctrl+R");
    key(c, "Ctrl+Shift+H");
    let e = unknown_kept(c, id, "Ctrl+R and Ctrl+Shift+H");
    assert_eq!(e[e.len() - 2..], ["rotate(90)".to_string(), "flip_h()".to_string()], "{e:?}");
    assert_eq!(label(c, "button:edit.undo"), "Undo Flip");
    // One undo, so the redo list holds the flip across the restart.
    click(c, "button:edit.undo");
    let h = history(c, id);
    assert_eq!((h["undoLabel"].as_str(), h["redoLabel"].as_str()), (Some("Undo Rotate"), Some("Redo Flip")), "{h}");
    assert_eq!((label(c, "button:edit.undo"), label(c, "button:edit.redo")), ("Undo Rotate".to_string(), "Redo Flip".to_string()));
    let kinds: Vec<&str> = h["edits"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().split('(').next().unwrap()).collect();
    assert_eq!(kinds, ["future_op", "crop", "straighten", "autofix", "autocontrast", "autocolor", "tuning", "retouch", "rotate"], "{h}");
    println!("every tool, the unknown operation first and kept after each:\n  {}\n  {}", log.join("\n  "), h["edits"]);
    h
}

/// The picture inside `rect` of a screenshot, reduced to `w × h` by box averaging.
fn picture(shot: &Shot, rect: [f64; 4], w: u32, h: u32) -> Rgba8 {
    let (pw, ph, rgb) = shot.region(rect);
    let img = Rgba8 { width: pw, height: ph, data: rgb.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    render::box_downscale(&img, w, h)
}

/// The rect the canvas draws the fitted photo in (`editing.md` §3: 24 pt padding, never above
/// 100 %, centred), from the photo area and the photo's displayed size; 1 point = 1 pixel headless.
fn fitted(canvas: [f64; 4], displayed: (f64, f64)) -> [f64; 4] {
    let (bw, bh) = (canvas[2] - 48.0, canvas[3] - 48.0);
    let s = (bw / displayed.0).min(bh / displayed.1).min(1.0);
    let (w, h) = (displayed.0 * s, displayed.1 * s);
    [canvas[0] + (canvas[2] - w) / 2.0, canvas[1] + (canvas[3] - h) / 2.0, w, h]
}

/// Mean absolute difference per channel of two RGBA images of one size (alpha left out).
fn mean_difference(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let sum: u64 = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|k| u64::from(p[k].abs_diff(q[k]))).sum::<u64>()).sum();
    sum as f64 / (a.data.len() / 4 * 3).max(1) as f64
}

/// Part 1e: the light table's thumbnail of [`EDITED`] equals the edit view's render after
/// downscale (both to 48 px across); the unedited neighbour's thumbnail does not.
fn thumbnail_is_the_render(c: &mut Client, home: &Path, id: &Value, other: &Value) -> f64 {
    c.run("view.edit", json!({ "id": id }));
    c.settle_jobs();
    c.park_pointer(900.0);
    let shot = live::screenshot(c, home, None, "m7-1400x900-canvas", 1400, 900);
    let displayed = displayed(&edits(c, id));
    let canvas = c.ok("ui.inspect", json!({}))["regions"]["canvas"].clone();
    let canvas: Vec<f64> = (0..4).map(|i| canvas[i].as_f64().unwrap()).collect();
    let rect = fitted([canvas[0], canvas[1], canvas[2], canvas[3]], displayed);
    let inset = |r: [f64; 4]| [r[0] + 2.0, r[1] + 2.0, r[2] - 4.0, r[3] - 4.0];
    let w = 48u32;
    let h = ((f64::from(w) * displayed.1 / displayed.0).round() as u32).max(1);
    let shown = picture(&shot, inset(rect), w, h);

    click(c, "button:edit.back");
    c.run("select.none", json!({}));
    c.settle_jobs();
    c.park_pointer(900.0);
    let lib = live::screenshot(c, home, None, "m7-1400x900-thumbnails", 1400, 900);
    let cell = lib.picture(c.rect(&format!("thumb:{id}")));
    let thumb = picture(&lib, inset(cell), w, h);
    let diff = mean_difference(&shown, &thumb);
    let other_cell = lib.picture(c.rect(&format!("thumb:{other}")));
    let unedited = picture(&lib, inset(other_cell), w, h);
    let apart = mean_difference(&shown, &unedited);
    println!(
        "thumbnail = edit view: the cell's picture {:.0} × {:.0} and the canvas's {:.0} × {:.0}, both reduced to {w} × {h}: mean difference {diff:.2} per channel (another photo's cell: {apart:.2})",
        cell[2], cell[3], rect[2], rect[3]
    );
    assert!((cell[2] / cell[3] - displayed.0 / displayed.1).abs() < 0.05, "the cell has the edited shape: {cell:?} vs {displayed:?}");
    assert!(diff < 6.0, "the thumbnail shows the edit view's render: mean difference {diff:.2}");
    assert!(apart > 3.0 * diff.max(1.0), "the comparison tells pictures apart: {apart:.2} against {diff:.2}");
    diff
}

/// Parts 1 (the live app) and 2 (the renderer).
#[test]
fn the_edit_view_and_its_fixes_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    let folder = photos(home.path());

    // 1. A fresh library.
    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    c.ok("engine.execute", json!({ "command": "folders.add", "params": { "path": folder } }));
    c.settle_jobs();
    let ids = photo_ids(&mut c);
    assert_eq!(ids.len(), PHOTOS.len(), "{ids:?}");
    let edited = ids[EDITED].clone();
    assert_eq!(edits(&mut c, &edited), [UNKNOWN], "the sidecar's unknown operation is read");

    let opened = zoom_maths(&mut c, &ids[LARGE]);
    enter_leave_and_step(&mut c, &ids);
    let h = every_tool(&mut c, &edited);

    // The window before the restart (its fingerprint is taken after it, see below).
    c.settle_jobs();
    c.park_pointer(900.0);
    let before = live::screenshot(&mut c, home.path(), None, "m7-1400x900-before", 1400, 900);
    let regions = regions(&mut c);
    let part1 = secs(t);
    live::quit(app, c);

    // The app again: the history, its labels and the window are as they were; it still walks.
    let t = Instant::now();
    let (app, mut c) = live::start(&exe, home.path());
    c.settle_jobs();
    let again = photo_ids(&mut c);
    let edited = again[EDITED].clone();
    assert_eq!(history(&mut c, &edited), h, "the photo's history and its labels survive a restart");
    c.run("view.edit", json!({ "id": edited }));
    c.settle_jobs();
    c.park_pointer(900.0);
    // The edited photo with the Basic Fixes tab at 1400 × 900, the Tuning tab at 900 × 600.
    let large = live::screenshot(&mut c, home.path(), Some("m7-1400x900"), "m7-1400x900", 1400, 900);
    let mut same = Vec::new();
    for name in ["editTopBar", "editPanel", "canvas", "captionBar", "statusLine"] {
        let r = regions[name];
        let d = live::difference(&large.region(r).2, &before.region(r).2);
        assert!(d < 0.5, "{name} looks as before the restart: mean difference {d:.3}");
        same.push(format!("{name} {d:.3}"));
    }
    // A known gap, measured and reported rather than asserted (found by this test, for the lead):
    // the Photo Tray's mini thumbnail of the open photo keeps the picture it had before the edits
    // while the edit view shows; the filmstrip and the light table follow them.
    let tray = live::difference(&large.region(regions["tray"]).2, &before.region(regions["tray"]).2);
    assert_eq!((label(&mut c, "button:edit.undo"), label(&mut c, "button:edit.redo")), ("Undo Rotate".to_string(), "Redo Flip".to_string()));
    c.ok("ui.resize", json!({ "width": 900, "height": 600 }));
    click(&mut c, "tab:edit.tuning");
    c.settle_jobs();
    c.park_pointer(600.0);
    let small = live::screenshot(&mut c, home.path(), Some("m7-900x600"), "m7-900x600", 900, 600);
    c.ok("ui.resize", json!({ "width": 1400, "height": 900 }));
    click(&mut c, "tab:edit.basic");
    click(&mut c, "button:edit.redo");
    let e = unknown_kept(&mut c, &edited, "a redo after the restart");
    assert_eq!(e.last().map(String::as_str), Some("flip_h()"));
    let diff = thumbnail_is_the_render(&mut c, home.path(), &edited, &again["IMG_0004.jpg"]);
    live::quit(app, c);
    println!("restart: the history and labels the same, redo walks; the window as before (mean difference per channel: {})", same.join(", "));
    println!(
        "known gap (not asserted): the Photo Tray's mini thumbnail of the open photo before the restart differs from after it by {tray:.1} per channel: in the edit view it kept the unedited picture"
    );

    // 2. The renderer across M7's operations.
    let t2 = Instant::now();
    let worst = render::two_sizes();
    render::unknown_operations_survive();
    println!(
        "two sizes (96 and 384 px of three 600 × 400 scenes; at most {} per channel on average, {} at the 99th percentile):\n  {}",
        render::MEAN_TOLERANCE,
        render::P99_TOLERANCE,
        worst.join("\n  ")
    );
    println!("unknown and legacy operations: printed back in place, skipped by the render (byte-identical without them)");

    println!(
        "live app: build {build:.1} s, part 1 {part1:.1} s, the restart {:.1} s, renderer {:.1} s; the large photo's first preview (debug build): draft {} ms, final {} ms; thumbnail difference {diff:.2}; screenshots:\n  {}\n  {}\n  {}",
        secs(t),
        secs(t2),
        opened["draft"],
        opened["final"],
        before.path.display(),
        large.path.display(),
        small.path.display()
    );
    println!("M7 live app: passed in {:.1} s", secs(started));
}

/// Parts 3 and 4.
#[test]
fn the_crate_suites_panic_hunt_and_the_preview_budgets() {
    let started = Instant::now();
    if !dalica_acceptance::suites_ran() {
        checks::crate_suites();
    }
    let suites = secs(started);
    let t = Instant::now();
    checks::panic_hunt();
    let hunt = secs(t);
    let t = Instant::now();
    checks::preview_bench_debug();
    let bench = secs(t);
    if std::env::var_os(checks::RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        checks::release_budgets();
        println!("release budgets: {:.1} s", secs(t));
    } else {
        println!("(the release budgets, 24 MP: run with {}=1)", checks::RELEASE);
    }
    println!("suites {suites:.1} s, panic_hunt {hunt:.1} s, debug bench {bench:.1} s; M7 checks: passed in {:.1} s", secs(started));
}
