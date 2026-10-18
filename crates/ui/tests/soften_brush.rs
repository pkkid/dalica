//! Soften's brush and eraser (`editing.md` §8.4), headless and by id: with no stroke the whole
//! photo previews softened; one painted stroke softens only under it; an erase stroke over it gives
//! the photo back there; Apply records one "Soften" step whose strokes land where they were drawn
//! on a photo turned 90°; + and − resize the brush; a fingerprint of the brush over a painted
//! stroke and of the panel's brush rows at 1400 × 900 (Softness 0, so no renderer's pixels show:
//! the photo as it is, the painted area's wash and the circle), and at 900 × 600 the rows fit.
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::edit::canvas;
use dalica_ui::state::SoftenStroke;
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, pos2};
use serde_json::json;

/// The photo: 480 × 320, its left half red, its right half blue.
const SIZE: (u32, u32) = (480, 320);

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    photo: PhotoId,
}

fn halves() -> Vec<u8> {
    let mut data = Vec::with_capacity((SIZE.0 * SIZE.1 * 4) as usize);
    for _ in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let c: [u8; 3] = if x < SIZE.0 / 2 { [200, 40, 30] } else { [30, 60, 190] };
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    encode::png(&Image::rgba8(&Rgba8 { width: SIZE.0, height: SIZE.1, data }), &Metadata::default()).unwrap()
}

/// A library holding the photo, its recipe `recipe`, open in the edit view with Soften's panel.
fn setup((w, h): (u32, u32), recipe: &[&str]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Halves.png"), halves()).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let photo = s.library().photos().next().unwrap().id;
    for op in recipe {
        s.execute("photo.history.apply", &json!({ "id": photo, "op": op })).unwrap();
    }
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": photo })).unwrap();
    d.settle(FRAMES).unwrap();
    d.run("edit.tab", &json!({ "tab": "effects3" })).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("effect:soften").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().edit.effect.as_ref().map(|e| e.name.as_str()), Some("soften"));
    T { _dir: dir, d, photo }
}

fn edits(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().library().info(t.photo).edits.clone()
}

fn strokes(t: &T) -> Vec<SoftenStroke> {
    t.d.state().edit.effect.as_ref().map(|e| e.strokes.clone()).unwrap_or_default()
}

/// The screen point of the displayed point `at` (normalised to the photo as it shows).
fn screen(t: &T, at: [f32; 2]) -> Pos2 {
    let v = canvas::view(t.d.ctx()).expect("the canvas was drawn");
    pos2(v.image.min.x + at[0] * v.image.width(), v.image.min.y + at[1] * v.image.height())
}

/// A drag across the photo from the displayed point `from` to `to`, in 12 moves.
fn drag(t: &mut T, from: [f32; 2], to: [f32; 2]) {
    let (a, b) = (screen(t, from), screen(t, to));
    let mut events = vec![PointerEvent { kind: PointerKind::Down, pos: a }];
    events.extend((1..=12).map(|i| PointerEvent { kind: PointerKind::Drag, pos: a.lerp(b, i as f32 / 12.0) }));
    events.push(PointerEvent { kind: PointerKind::Up, pos: b });
    t.d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.settle(FRAMES).unwrap();
}

/// The pointer rested on the displayed point `at`.
fn hover(t: &mut T, at: [f32; 2]) {
    let p = screen(t, at);
    t.d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.settle(FRAMES).unwrap();
}

/// The preview once every render is in: its recipe and its pixels.
fn preview(t: &mut T) -> (Vec<String>, Rgba8) {
    assert!(t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(30)));
    t.d.settle(FRAMES).unwrap();
    let p = t.d.app().session().unwrap().preview().unwrap();
    (p.recipe.clone(), (*p.image).clone())
}

/// The pixel of `img` at the displayed point `at`.
fn pixel(img: &Rgba8, at: [f32; 2]) -> [i32; 3] {
    let x = ((at[0] * img.width as f32) as usize).min(img.width as usize - 1);
    let y = ((at[1] * img.height as f32) as usize).min(img.height as usize - 1);
    let i = (y * img.width as usize + x) * 4;
    [i32::from(img.data[i]), i32::from(img.data[i + 1]), i32::from(img.data[i + 2])]
}

fn apart(a: [i32; 3], b: [i32; 3]) -> i32 {
    (0..3).map(|i| (a[i] - b[i]).abs()).max().unwrap()
}

/// On the stroke and off it, both on the red half.
const ON: [f32; 2] = [0.25, 0.3];
const OFF: [f32; 2] = [0.25, 0.8];

#[test]
fn the_brush_softens_where_painted_and_the_eraser_gives_the_photo_back() {
    let mut t = setup(LARGE, &[]);
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": { "softness": 0 } })).unwrap();
    let (_, plain) = preview(&mut t);
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": { "softness": 50 } })).unwrap();

    // No stroke: the whole photo softens.
    let (recipe, all) = preview(&mut t);
    assert_eq!(recipe, ["soften(50,50)"]);
    assert!(apart(pixel(&all, ON), pixel(&plain, ON)) >= 3 && apart(pixel(&all, OFF), pixel(&plain, OFF)) >= 3);
    assert!(!widget(&t.d, "button:effect.erase").enabled, "nothing to erase before a paint stroke");
    assert!(widget(&t.d, "button:effect.paint").active);

    // One stroke along y = 0.3: softened under it only.
    drag(&mut t, [0.1, 0.3], [0.4, 0.3]);
    let s = strokes(&t);
    assert_eq!(s.len(), 1);
    assert!(!s[0].erase && s[0].points.len() >= 2, "{s:?}");
    let (recipe, painted) = preview(&mut t);
    assert!(recipe[0].starts_with("soften(50,50;paint,0.05,"), "{recipe:?}");
    assert!(apart(pixel(&painted, ON), pixel(&plain, ON)) >= 3, "softened under the stroke");
    assert!(apart(pixel(&painted, OFF), pixel(&plain, OFF)) <= 1, "untouched away from it");
    hover(&mut t, [0.45, 0.3]);
    shot(&t.d, "soften-brush-painted-window");

    // An erase stroke over it: the photo again there.
    t.d.click("button:effect.erase").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().edit.effect.as_ref().unwrap().erase);
    drag(&mut t, [0.1, 0.3], [0.4, 0.3]);
    assert!(strokes(&t)[1].erase);
    let (recipe, erased) = preview(&mut t);
    assert!(recipe[0].contains(";erase,0.05,"), "{recipe:?}");
    assert!(apart(pixel(&erased, ON), pixel(&plain, ON)) <= 1, "erased back to the photo");

    // + twice, − once: 1.2 times larger.
    for k in ["Plus", "=", "-"] {
        t.d.key(k).unwrap();
    }
    t.d.settle(FRAMES).unwrap();
    let brush = t.d.state().edit.effect.as_ref().unwrap().brush;
    assert!((brush - 0.05 * 1.2).abs() < 1e-4, "{brush}");

    // Cancel records nothing.
    t.d.click("button:panel.cancel").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(edits(&t).is_empty());
}

#[test]
fn apply_stores_the_strokes_where_they_were_drawn_on_a_turned_photo() {
    let mut t = setup(LARGE, &["rotate(90)"]);
    drag(&mut t, [0.2, 0.25], [0.6, 0.25]);
    t.d.click("button:panel.apply").unwrap();
    t.d.settle(FRAMES).unwrap();
    let e = edits(&t);
    assert_eq!(e.len(), 2, "{e:?}");
    assert_eq!(widget(&t.d, "button:edit.undo").label, "Undo Soften");
    // `soften(50,50;paint,r,x,y,…)`: on the turned photo the drawn row is the stored column
    // x = 0.25, from y = 0.8 up to y = 0.4 (displayed (x, y) is stored (y, 1 − x)).
    let body = e[1].strip_prefix("soften(50,50;paint,0.05,").and_then(|b| b.strip_suffix(')')).unwrap_or_else(|| panic!("{}", e[1]));
    let v: Vec<f32> = body.split(',').map(|n| n.parse().unwrap()).collect();
    assert!(v.len() >= 4 && v.len().is_multiple_of(2), "{v:?}");
    for p in v.chunks(2) {
        assert!((p[0] - 0.25).abs() < 0.01 && (0.39..=0.81).contains(&p[1]), "{p:?}");
    }
    assert!((v[1] - 0.8).abs() < 0.01 && (v[v.len() - 1] - 0.4).abs() < 0.01, "{v:?}");
}

#[test]
fn the_brush_over_a_painted_stroke_and_the_panels_rows_match_their_fingerprints() {
    let mut t = setup(LARGE, &[]);
    // Softness 0: the photo shows as it is, so only the brush's own drawing is pinned.
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": { "softness": 0 } })).unwrap();
    drag(&mut t, [0.2, 0.3], [0.8, 0.7]);
    hover(&mut t, [0.5, 0.5]);
    preview(&mut t);
    shot(&t.d, "soften-brush-window");
    let area = widget(&t.d, "canvas:edit").rect;
    check_rect(&t.d, &format!("soften-brush-canvas-{}x{}", LARGE.0, LARGE.1), area);
    // The panel from its title to its buttons: the rows sit between the sliders and the buttons.
    let [softness, paint, size, apply] =
        ["slider:effect.softness", "button:effect.paint", "slider:soften.brush", "button:panel.apply"].map(|id| widget(&t.d, id).rect);
    assert!(softness[1] < paint[1] && paint[1] + paint[3] <= size[1] && size[1] + size[3] <= apply[1], "{paint:?} {size:?} {apply:?}");
    let panel = widget(&t.d, "region:editPanel").rect;
    // From under the tab strip (its title row) to just below the buttons, across the panel.
    let top = panel[1] + 36.0;
    check_rect(&t.d, &format!("soften-brush-panel-{}x{}", LARGE.0, LARGE.1), [panel[0], top, panel[2], apply[1] + apply[3] + 4.0 - top]);

    // At 900 × 600 the controls scroll: Brush size is reached by scrolling them, and the rows stay
    // inside the panel.
    t.d.resize(SMALL.0, SMALL.1);
    t.d.settle(FRAMES).unwrap();
    t.d.scroll(Some("slider:effect.softness"), egui::vec2(0.0, -400.0)).unwrap();
    t.d.settle(FRAMES).unwrap();
    let panel = widget(&t.d, "region:editPanel").rect;
    for id in ["button:effect.paint", "button:effect.erase", "slider:soften.brush"] {
        let r = widget(&t.d, id).rect;
        assert!(r[0] >= panel[0] && r[0] + r[2] <= panel[0] + panel[2] && r[1] >= panel[1] && r[1] + r[3] <= panel[1] + panel[3], "{id}: {r:?}");
    }
    check_fits(&t.d, "soften");
}
