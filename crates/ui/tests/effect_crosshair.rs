//! The crosshair on the photo (`editing.md` §8.1), headless and by id: dragging Soft Focus's
//! centre moves the sharp circle in the preview and Apply stores that point in the stored frame,
//! upright and on a photo turned 90°, while Cancel records nothing; Graduated Tint's handle sets
//! its angle (measured in the stored frame, so the same drag gives another angle on a turned
//! photo); on a turned photo with a Border the centre cannot leave the photo; the arrows nudge the
//! centre; fingerprints of Soft Focus's and Graduated Tint's crosshair at 1400 × 900 (on a flat
//! photo with the effect set to change nothing, so only the crosshair is pinned).
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_rect, new_driver, shot, widget};
use dalica_core::PhotoId;
use dalica_engine::{Quality, Session};
use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata};
use dalica_ui::commands::effect_session;
use dalica_ui::headless::{Drag, PointerEvent, PointerKind, Target};
use dalica_ui::panels::edit::canvas::{self, View};
use dalica_ui::panels::edit::crosshair;
use dalica_ui::state::{CrosshairPart, EffectValue, ToolKind};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, pos2, vec2};
use serde_json::json;

const SIZE: (u32, u32) = (480, 320);
const HANDLE: &str = "crosshair:effect/handle";

/// A fine checker in red (2-pixel squares: any blur flattens it) over a green ramp.
fn textured(x: u32, y: u32) -> [u8; 3] {
    let v = if ((x / 2) + (y / 2)).is_multiple_of(2) { 50 } else { 210 };
    [v, (x * 255 / SIZE.0) as u8, 255 - v]
}

fn flat(_: u32, _: u32) -> [u8; 3] {
    [128, 128, 128]
}

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    photo: PhotoId,
}

/// A library holding one `SIZE` photo painted by `paint`, open in the edit view at `LARGE`.
fn setup(paint: fn(u32, u32) -> [u8; 3]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    let mut data = Vec::with_capacity((SIZE.0 * SIZE.1 * 4) as usize);
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let c = paint(x, y);
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    let png = encode::png(&Image::rgba8(&Rgba8 { width: SIZE.0, height: SIZE.1, data }), &Metadata::default()).unwrap();
    std::fs::write(folder.join("Photo.png"), png).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let photo = s.library().photos().next().unwrap().id;
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.run("view.edit", &json!({ "id": photo })).unwrap();
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, photo }
}

fn settle(t: &mut T) {
    t.d.settle(FRAMES).unwrap();
}

/// Turns the photo a quarter clockwise (the stored left edge shows along the top).
fn turn(t: &mut T) {
    t.d.run("photo.history.apply", &json!({ "id": t.photo, "op": "rotate(90)" })).unwrap();
    settle(t);
}

/// The stored point under the shown point `s` of a photo turned a quarter clockwise.
fn stored_of_turned(s: [f64; 2]) -> [f64; 2] {
    [s[1], 1.0 - s[0]]
}

/// Opens effect `name` (tab 3 or 5) from its tile.
fn open(t: &mut T, name: &str) {
    let tab = if name == "focal_zoom" { "effects3" } else { "effects" };
    t.d.run("edit.tab", &json!({ "tab": tab })).unwrap();
    settle(t);
    t.d.click(&format!("effect:{name}")).unwrap();
    settle(t);
    assert_eq!(t.d.state().edit.effect.as_ref().map(|e| e.name.as_str()), Some(name));
}

fn set(t: &mut T, params: serde_json::Value) {
    t.d.run("edit.tool.action", &json!({ "name": "set", "params": params })).unwrap();
    settle(t);
}

fn param(t: &T, p: &str) -> f64 {
    match t.d.state().edit.effect.as_ref().and_then(|e| e.params.get(p)) {
        Some(EffectValue::Number(v)) => *v,
        other => panic!("{p}: {other:?}"),
    }
}

fn point(t: &T) -> [f64; 2] {
    [param(t, "x"), param(t, "y")]
}

fn edits(t: &T) -> Vec<String> {
    t.d.app().session().unwrap().library().info(t.photo).edits.clone()
}

fn view(t: &T) -> View {
    canvas::view(t.d.ctx()).expect("the canvas was drawn")
}

/// The screen point of the shown point `s` (normalised to the displayed picture).
fn screen(t: &T, s: [f64; 2]) -> Pos2 {
    let v = view(t);
    pos2(v.image.min.x + s[0] as f32 * v.image.width(), v.image.min.y + s[1] as f32 * v.image.height())
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
}

/// Drags widget `id` to the screen point `to`.
fn drag(t: &mut T, id: &str, to: Pos2) {
    t.d.drag(id, Target::Point(to), &Drag::default()).unwrap();
    settle(t);
}

/// The preview's final picture of the open panel's candidate.
fn final_candidate(t: &mut T) -> Rgba8 {
    let (_, want) = effect_session::candidate(t.d.app()).expect("a panel is open");
    t.d.step_until(
        |app| {
            std::thread::sleep(Duration::from_millis(1));
            app.session().and_then(|s| s.preview()).is_some_and(|p| p.recipe == want && p.quality == Quality::Final)
        },
        FRAMES,
    )
    .unwrap();
    settle(t);
    (*t.d.app().session().unwrap().preview().unwrap().image).clone()
}

/// How much the red checker shows round the shown point `s` of `img`: the mean step between
/// neighbouring pixels in a 9 × 9 window (80 sharp: every other step crosses the checker; near 0 blurred).
fn checker(img: &Rgba8, s: [f64; 2]) -> f32 {
    let (cx, cy) = ((s[0] * f64::from(img.width)) as i64, (s[1] * f64::from(img.height)) as i64);
    let red = |x: i64, y: i64| f32::from(img.data[((y as u32 * img.width + x as u32) * 4) as usize]);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in cy - 4..=cy + 4 {
        for x in cx - 4..cx + 4 {
            sum += (red(x + 1, y) - red(x, y)).abs();
            n += 1.0;
        }
    }
    sum / n
}

/// Soft Focus's centre dragged to a shown point: the sharp circle moves there in the preview, the
/// recipe is untouched, Cancel records nothing and a second drag's Apply stores the point in the
/// stored frame. Upright, then on a photo turned 90°.
#[test]
fn soft_focus_follows_its_drag_and_applies_in_the_stored_frame() {
    for turned in [false, true] {
        let mut t = setup(textured);
        if turned {
            turn(&mut t);
        }
        let before = edits(&t);
        open(&mut t, "soft_focus");
        set(&mut t, json!({ "size": 0.15, "amount": 1 }));
        let middle = [0.5, 0.5];
        let corner = [0.25, 0.3];
        let img = final_candidate(&mut t);
        assert!(
            checker(&img, middle) > 60.0 && checker(&img, corner) < 10.0,
            "turned {turned}: sharp only in the middle: {} {} {}x{}",
            checker(&img, middle),
            checker(&img, corner),
            img.width,
            img.height
        );

        let to = screen(&t, corner);
        drag(&mut t, crosshair::ID, to);
        let want = if turned { stored_of_turned(corner) } else { corner };
        assert!(near(point(&t), want), "turned {turned}: {:?}, not {want:?}", point(&t));
        let img = final_candidate(&mut t);
        assert!(checker(&img, corner) > 60.0, "turned {turned}: the sharp circle moved to the corner");
        assert!(checker(&img, middle) < 10.0, "turned {turned}: the middle blurs");
        assert_eq!(edits(&t), before, "nothing reaches the recipe before Apply");
        t.d.click("button:panel.cancel").unwrap();
        settle(&mut t);
        assert_eq!(edits(&t), before, "Cancel records nothing");

        open(&mut t, "soft_focus");
        drag(&mut t, crosshair::ID, to);
        assert_eq!(t.d.state().edit.effect.as_ref().unwrap().drag, None, "no drag under way once dropped");
        t.d.click("button:panel.apply").unwrap();
        settle(&mut t);
        let last = edits(&t).last().cloned().unwrap();
        let inner = last.strip_prefix("soft_focus(").and_then(|s| s.strip_suffix(')')).unwrap();
        let v: Vec<f64> = inner.split(',').map(|n| n.parse().unwrap()).collect();
        assert!(near([v[0], v[1]], want), "turned {turned}: {last}");
        assert_eq!(edits(&t).len(), before.len() + 1);
    }
}

/// Graduated Tint's handle sets the angle, in the stored frame: dragged straight down it is 180°
/// upright and 90° on a photo turned a quarter clockwise. While it is dragged the drag is the
/// handle's; Apply stores the angle. Its crosshair matches the fingerprint and the window is
/// written as a screenshot.
#[test]
fn graduated_tints_handle_sets_its_angle() {
    for turned in [false, true] {
        let mut t = setup(textured);
        if turned {
            turn(&mut t);
        }
        open(&mut t, "graduated_tint");
        let centre = widget(&t.d, crosshair::ID).center();
        let handle = widget(&t.d, HANDLE).center();
        if turned {
            assert!(handle.x > centre.x + 20.0 && (handle.y - centre.y).abs() < 1.0, "angle 0 points along the stored up: right");
        } else {
            assert!(handle.y < centre.y - 20.0 && (handle.x - centre.x).abs() < 1.0, "angle 0 points up");
        }
        let below = centre + vec2(0.0, 120.0);
        let ev = |kind, pos| PointerEvent { kind, pos };
        t.d.pointer(
            &[ev(PointerKind::Move, handle), ev(PointerKind::Down, handle), ev(PointerKind::Move, handle.lerp(below, 0.5))],
            PointerButton::Primary,
            Modifiers::NONE,
        )
        .unwrap();
        let under_way = t.d.state().edit.effect.as_ref().unwrap().drag.expect("a drag is under way");
        assert_eq!(under_way.part, CrosshairPart::Extent);
        t.d.pointer(
            &[ev(PointerKind::Move, below), ev(PointerKind::Up, below), ev(PointerKind::Move, pos2(2.0, 2.0))],
            PointerButton::Primary,
            Modifiers::NONE,
        )
        .unwrap();
        settle(&mut t);
        let want = if turned { 90.0 } else { 180.0 };
        assert!((param(&t, "angle") - want).abs() < 0.5, "turned {turned}: {}", param(&t, "angle"));
        assert!(near(point(&t), [0.5, 0.5]), "the start stays");
        if !turned {
            final_candidate(&mut t);
            shot(&t.d, &format!("effect-crosshair-graduated-tint-window-{}x{}", LARGE.0, LARGE.1));
        }
        t.d.click("button:panel.apply").unwrap();
        settle(&mut t);
        let last = edits(&t).last().cloned().unwrap();
        let fields: Vec<&str> = last.strip_prefix("graduated_tint(").unwrap().split(',').collect();
        assert_eq!(fields[..2], ["0.5", "0.5"], "{last}");
        assert!((fields[2].parse::<f64>().unwrap() - want).abs() < 0.5, "{last}");
    }

    // The fingerprint: a flat photo, a white tint (which changes nothing), the handle at 135°.
    let mut t = setup(flat);
    open(&mut t, "graduated_tint");
    set(&mut t, json!({ "color": "#ffffff", "angle": 135 }));
    final_candidate(&mut t);
    let r = view(&t).image;
    check_rect(&t.d, &format!("effect-crosshair-graduated-tint-{}x{}", LARGE.0, LARGE.1), [r.min.x, r.min.y, r.width(), r.height()]);
}

/// On a turned photo with a Border, Focal B&W's centre dragged onto the frame's corner stops at
/// the photo's corner: the point is the stored frame's edge and the crosshair stands where the
/// photo begins inside the bands. Screenshots of Focal B&W on the turned photo at both sizes.
#[test]
fn on_a_framed_photo_the_centre_cannot_leave_the_photo() {
    let mut t = setup(textured);
    turn(&mut t);
    open(&mut t, "focal_bw");
    let to = screen(&t, [0.3, 0.35]);
    drag(&mut t, crosshair::ID, to);
    assert!(near(point(&t), stored_of_turned([0.3, 0.35])), "{:?}", point(&t));
    final_candidate(&mut t);
    shot(&t.d, &format!("effect-crosshair-focal-bw-turned-{}x{}", LARGE.0, LARGE.1));
    t.d.resize(SMALL.0, SMALL.1);
    settle(&mut t);
    shot(&t.d, &format!("effect-crosshair-focal-bw-turned-{}x{}", SMALL.0, SMALL.1));
    t.d.resize(LARGE.0, LARGE.1);
    t.d.click("button:panel.cancel").unwrap();
    settle(&mut t);

    // Bands of 100 + 100 thousandths of the shorter side (320): 64 pixels round the photo.
    t.d.run("photo.history.apply", &json!({ "id": t.photo, "op": "border(#000000,100,#ffffff,100,0,0)" })).unwrap();
    settle(&mut t);
    open(&mut t, "focal_bw");
    let image = view(&t).image;
    drag(&mut t, crosshair::ID, image.min + vec2(3.0, 3.0));
    // The shown top-left of a photo turned a quarter clockwise is its stored bottom-left.
    assert!(near(point(&t), [0.0, 1.0]), "{:?}", point(&t));
    let at = widget(&t.d, crosshair::ID).center();
    let band = 64.0 * image.width() / (SIZE.1 as f32 + 128.0);
    assert!(
        (at.x - image.min.x - band).abs() < 2.0 && (at.y - image.min.y - band).abs() < 2.0,
        "the crosshair at {at:?}, the photo inside {band} of {image:?}"
    );
    t.d.click("button:panel.apply").unwrap();
    settle(&mut t);
    assert!(edits(&t).last().unwrap().starts_with("focal_bw(0,1,"), "{:?}", edits(&t));
}

/// The arrows nudge the centre by a hundredth of the photo (Shift: a tenth), kept on the photo; the
/// photo and the panel stay. Soft Focus's crosshair matches its fingerprint (a flat photo, Amount
/// 0: the effect changes nothing).
#[test]
fn the_arrows_nudge_the_centre() {
    let mut t = setup(flat);
    open(&mut t, "soft_focus");
    t.d.key("Right").unwrap();
    settle(&mut t);
    assert!(near(point(&t), [0.51, 0.5]), "{:?}", point(&t));
    t.d.key("Shift+Down").unwrap();
    t.d.key("Up").unwrap();
    settle(&mut t);
    assert!(near(point(&t), [0.51, 0.59]), "{:?}", point(&t));
    for _ in 0..8 {
        t.d.key("Shift+Left").unwrap();
    }
    settle(&mut t);
    assert!(near(point(&t), [0.0, 0.59]), "kept on the photo: {:?}", point(&t));
    assert_eq!((t.d.state().edit.photo, t.d.state().edit.tool), (Some(t.photo), Some(ToolKind::Effect)), "the photo and the panel stay");

    set(&mut t, json!({ "x": 0.35, "y": 0.4, "amount": 0 }));
    final_candidate(&mut t);
    let r = view(&t).image;
    check_rect(&t.d, &format!("effect-crosshair-soft-focus-{}x{}", LARGE.0, LARGE.1), [r.min.x, r.min.y, r.width(), r.height()]);
    assert!(edits(&t).is_empty());
}
