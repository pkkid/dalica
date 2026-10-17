//! The edit view's photo canvas, zoom and pan (`editing.md` §2–§3), headless, driven through the
//! control channel as an agent would (and, for the notched wheel and the cursor the driver cannot
//! send or keep, through egui directly): the photo drawn fitted from the session's preview; a wheel
//! step and Ctrl+= keep the image point under the pointer and land exactly on 100 % when crossing
//! it; click-and-hold zooms to 100 % at the point, a click returns to fit, a double-click returns
//! to the library, and "Single-click to leave the edit view" makes one click leave and turns the
//! hold off; drags pan with the hand cursor and stop at the photo's edges; zooming in asks for the
//! refined window and the frames go on without it (perf counters); Fit and 1:1 animate; the
//! read-out and the navigator while zoomed; the bottom area's zoom controls and its star and rotate
//! on the photo shown; the canvas's fingerprints fitted and at 200 % at 1400 × 900 (at 900 × 600
//! they only have to fit).
//!
//! `DALICA_SHOTS=<dir>` writes the screenshots there too.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_rect, new_driver};
use dalica_engine::Session;
use dalica_scenes::Scene;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::headless::{Click, Drag, PointerEvent, PointerKind, Target};
use dalica_ui::panels::edit::canvas::{self, View};
use dalica_ui::{App, Driver, DriverError};
use egui::{Modifiers, PointerButton, Pos2, pos2, vec2};
use serde_json::{Value, json};

/// The test photos: larger than the photo area at both window sizes, so fit is below 100 %.
const PHOTO: (u32, u32) = (1500, 1000);

/// A session on a library under `dir` holding one folder of three `PHOTO`-sized scenes JPEGs.
fn session(dir: &Path) -> (Session, Vec<u64>) {
    let folder = dir.join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(40 + i, PHOTO.0, PHOTO.1), 90, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 3);
    (s, ids)
}

/// A session on a library under `dir` holding one large scenes JPEG (4000 × 3000), whose preview
/// takes many frames to decode and render.
fn large_session(dir: &Path) -> (Session, Vec<u64>) {
    let folder = dir.join("photos").join("Large");
    std::fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&Scene::new(7, 4000, 3000), 90, None).unwrap();
    std::fs::write(folder.join("IMG_0001.jpg"), bytes).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10 })).unwrap();
    let ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 1);
    (s, ids)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    ids: Vec<u64>,
}

/// The window at `size` showing the first photo in the edit view, its preview drawn.
fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, ids };
    let photo = t.ids[0];
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
    settle_driver(&mut t.d);
}

/// Frames until nothing is pending (`ui.settle`'s rule), the open photo's preview awaited in real
/// time rather than counted in frames: while `Session::preview_busy` lists jobs, the session waits
/// for them (bounded) before the frames run, and a settle that ends with only the preview still busy
/// (`edit preview: N`, with the frames a tool animates meanwhile) waits for it and settles again. A
/// loaded machine's slow render is then never mistaken for a hang; anything else still busy fails.
fn settle_driver(d: &mut Driver) {
    for _ in 0..10 {
        let pending = d.app().session().map_or(0, Session::preview_busy);
        if pending > 0 {
            let session = d.app_mut().session_mut().unwrap();
            assert!(session.wait_preview(Duration::from_secs(60)), "the preview's {pending} jobs did not finish within 60 s");
        }
        let busy = match d.settle(FRAMES) {
            Ok(_) => return,
            Err(DriverError::NotSettled { busy, .. }) => busy,
            Err(e) => panic!("{e}"),
        };
        let pending = d.app().session().map_or(0, Session::preview_busy);
        let preview_only = busy.iter().all(|b| b.starts_with("edit preview: ") || b == "animation running");
        assert!(pending > 0 && preview_only, "not settled: {busy:?}, {pending} preview jobs pending");
    }
    panic!("not settled after ten waits for the preview");
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = ok(t, "engine.execute", json!({ "command": id, "params": params }));
    settle(t);
    v
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    settle(t);
}

fn set_zoom(t: &mut T, zoom: Value) {
    ok(t, "ui.set", json!({ "edit": { "zoom": zoom } }));
    settle(t);
}

fn state(t: &mut T) -> Value {
    ok(t, "ui.state", json!({}))
}

fn zoom(t: &mut T) -> Value {
    state(t)["edit"]["zoom"].clone()
}

fn view(t: &T) -> View {
    canvas::view(t.d.ctx()).expect("the canvas was drawn")
}

fn has(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

/// The image point (0 … 1 of the photo) under screen point `p`.
fn under(v: &View, p: Pos2) -> Pos2 {
    pos2((p.x - v.image.min.x) / v.image.width(), (p.y - v.image.min.y) / v.image.height())
}

fn near(a: Pos2, b: Pos2, tol: f32) -> bool {
    (a - b).length() <= tol
}

/// Moves the pointer to `p` (one frame) and leaves it there.
fn hover_at(t: &mut T, p: Pos2) {
    t.d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
}

#[test]
fn the_photo_is_fitted_with_padding_never_above_100_percent() {
    let mut t = setup(LARGE);
    for (w, h) in [LARGE, SMALL] {
        t.d.resize(w, h);
        settle(&mut t);
        let v = view(&t);
        assert_eq!(v.size, (PHOTO.0 as f32, PHOTO.1 as f32), "{w}×{h}");
        assert!(v.fit < 1.0 && !v.zoomed && !v.pannable, "{w}×{h}: {v:?}");
        let inner = v.area.shrink(canvas::PADDING);
        assert!(inner.contains_rect(v.image.shrink(0.01)), "{w}×{h}: {:?} inside {inner:?}", v.image);
        let touches = (v.image.width() - inner.width()).abs() < 0.5 || (v.image.height() - inner.height()).abs() < 0.5;
        assert!(touches, "{w}×{h}: the photo fills the padded box on one side");
        assert!((v.image.center() - v.area.center()).length() < 0.5, "{w}×{h}: centred");
        assert!(!has(&t, "text:zoom") && !has(&t, "navigator:edit"), "{w}×{h}: no read-out or navigator when fitted");
        // The bottom area has the zoom controls in place of the size slider and the loupe.
        for id in ["button:zoom.fit", "button:zoom.1to1", "slider:zoom"] {
            assert!(has(&t, id), "{w}×{h}: {id}");
        }
        assert!(!has(&t, "slider:thumbSize") && !has(&t, "button:loupe"), "{w}×{h}");
    }
}

/// `ui.settle` waits for the open photo's preview (`control-protocol.md` §6.12): right after the
/// edit view opens the photo it lists the preview as busy, and once settled the final is shown and
/// its time reported.
#[test]
fn settling_waits_for_the_open_photos_final_preview() {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, ids };
    let photo = t.ids[2];
    ok(&mut t, "engine.execute", json!({ "command": "view.edit", "params": { "id": photo } }));
    // The frame that opened the photo asked for its preview; the answer is applied in a later one.
    let busy = t.d.busy(false);
    assert!(busy.iter().any(|b| b.starts_with("edit preview")), "{busy:?}");
    let settled = ok(&mut t, "ui.settle", json!({ "timeoutMs": 60000 }));
    assert_eq!(settled["settled"], true, "{settled}");
    let shown = t.d.app().session().unwrap().preview().map(|p| (p.photo.get(), p.quality));
    assert_eq!(shown, Some((photo, dalica_engine::Quality::Final)));
    assert!(ok(&mut t, "ui.inspect", json!({}))["perf"]["previewFinalMs"].as_f64().is_some());
}

/// While the open photo's preview renders and nothing moves, the canvas rests: past the frames of
/// the view's change (the window's new title asks for one at once, and egui runs one more after
/// any such request), every frame asks for the next one no sooner than 16 ms later, never at once,
/// so a live window draws at most one frame per 16 ms and the wait leaves the CPU to the workers.
/// `ui.settle` still waits for the preview: every one of those frames lists it as busy
/// (`Session::preview_busy`), and the settle after it ends with the Final shown.
///
/// The render is held structurally, not raced: `Session::hold_preview` keeps its finished jobs
/// unapplied for exactly [`RENDERING`] frames, so a fast machine (the macOS runners decoded the
/// photo within the two opening frames, leaving none to check) and a loaded one see the same
/// frames, and every one of them is checked.
#[test]
fn the_canvas_rests_while_a_preview_renders_and_settling_still_waits_for_it() {
    const OPENING: usize = 2;
    const RENDERING: usize = 40;
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = large_session(dir.path());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, ids };
    let photo = t.ids[0];
    t.d.app_mut().session_mut().unwrap().hold_preview(true);
    ok(&mut t, "engine.execute", json!({ "command": "view.edit", "params": { "id": photo } }));
    let busy = |t: &T| t.d.app().session().map_or(0, Session::preview_busy);
    assert!(busy(&t) > 0, "the preview is on its way");
    // Frames while it renders, a millisecond apart in real time.
    let mut rested = 0;
    for n in 0..OPENING + RENDERING {
        t.d.step();
        assert!(busy(&t) > 0, "frame {n}: the held preview is still on its way");
        let delay = t.d.repaint_delay();
        if n >= OPENING {
            let causes = t.d.ctx().repaint_causes();
            assert!(delay.is_none_or(|d| d.as_secs_f64() >= 0.016), "frame {n}: the next frame after {delay:?}, asked by {causes:?}");
            rested += 1;
        }
        let listed = t.d.busy(false);
        assert!(listed.iter().any(|b| b.starts_with("edit preview: ")), "frame {n}: settling would not wait: {listed:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(rested, RENDERING, "every frame while the preview rendered rested");
    // Released: a slow machine's render is awaited in real time, so the settle's frame budget never
    // runs out.
    let session = t.d.app_mut().session_mut().unwrap();
    session.hold_preview(false);
    assert!(session.wait_preview(Duration::from_secs(60)), "the preview finished");
    let settled = ok(&mut t, "ui.settle", json!({ "timeoutMs": 60000 }));
    assert_eq!(settled["settled"], true, "{settled}");
    let shown = t.d.app().session().unwrap().preview().map(|p| (p.photo.get(), p.quality));
    assert_eq!(shown, Some((photo, dalica_engine::Quality::Final)));
}

#[test]
fn ctrl_equals_steps_about_the_pointer_and_snaps_to_100_percent() {
    let mut t = setup((1400, 900));
    let fit = view(&t).fit;
    let p = view(&t).area.center() + vec2(140.0, -90.0);
    hover_at(&mut t, p);
    let mut seen = Vec::new();
    for _ in 0..6 {
        let before = view(&t);
        let q = under(&before, p);
        key(&mut t, "Ctrl+=");
        let z = zoom(&mut t).as_f64().unwrap() as f32;
        seen.push(z);
        let after = view(&t);
        // The anchor holds wherever the photo still covers the area (it is centred while smaller).
        if after.pannable && before.pannable {
            assert!(near(under(&after, p), q, 1e-3), "{z}: {:?} vs {q:?}", under(&after, p));
        }
    }
    assert!(seen.contains(&1.0), "crossing 100 % lands on it: {seen:?}");
    assert!((seen[0] - fit * 1.25).abs() < 1e-4, "{seen:?}");
    for _ in 0..12 {
        key(&mut t, "Ctrl+-");
    }
    assert_eq!(zoom(&mut t), json!("fit"));
    assert_eq!(view(&t).image.center(), view(&t).area.center());
}

/// A mouse wheel notch arrives in lines, which the driver does not send: these frames are run on
/// egui directly.
#[test]
fn a_wheel_notch_keeps_the_image_point_under_the_pointer_and_snaps() {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let mut app = App::new(Some(s));
    let ctx = egui::Context::default();
    app.install(&ctx);
    let mut time = 0.0;
    let mut frame = |app: &mut App, events: Vec<egui::Event>| {
        let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
        input.time = Some(time);
        input.events = events;
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        time += 1.0 / 60.0;
    };
    app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
    for _ in 0..2000 {
        frame(&mut app, vec![]);
        if app.session().unwrap().preview().is_some() && app.session().unwrap().preview_pending() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let v = canvas::view(&ctx).unwrap();
    // Zoom to 95 %, the pointer off-centre.
    app.state_mut().edit.zoom = dalica_ui::state::Zoom::Scale(0.95);
    let p = v.area.center() + vec2(-120.0, 70.0);
    frame(&mut app, vec![egui::Event::PointerMoved(p)]);
    frame(&mut app, vec![]);
    let notch = |delta: f32| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, delta),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    };
    let before = canvas::view(&ctx).unwrap();
    let q = under(&before, p);
    frame(&mut app, vec![notch(1.0)]);
    frame(&mut app, vec![]);
    assert_eq!(app.state().edit.zoom, dalica_ui::state::Zoom::Scale(1.0), "95 % × 1.1 crosses 100 % and lands on it");
    let after = canvas::view(&ctx).unwrap();
    assert!(near(under(&after, p), q, 1e-3), "{:?} vs {q:?}", under(&after, p));
    frame(&mut app, vec![notch(1.0)]);
    frame(&mut app, vec![]);
    assert_eq!(app.state().edit.zoom, dalica_ui::state::Zoom::Scale(1.1));
    assert!(near(under(&canvas::view(&ctx).unwrap(), p), q, 1e-3));
    // Back out notch by notch: through 100 % exactly, then to fit.
    let mut hit = false;
    for _ in 0..12 {
        frame(&mut app, vec![notch(-1.0)]);
        hit |= app.state().edit.zoom == dalica_ui::state::Zoom::Scale(1.0);
    }
    assert!(hit);
    assert_eq!(app.state().edit.zoom, dalica_ui::state::Zoom::Fit);
}

#[test]
fn click_and_hold_zooms_to_100_percent_a_click_fits_and_a_double_click_leaves() {
    let mut t = setup((1400, 900));
    let photo = t.ids[0];
    let p = view(&t).image.center() + vec2(-150.0, 60.0);
    let q = under(&view(&t), p);
    // A quick click on the fitted photo does nothing.
    t.d.pointer(
        &[PointerEvent { kind: PointerKind::Down, pos: p }, PointerEvent { kind: PointerKind::Up, pos: p }],
        PointerButton::Primary,
        Modifiers::NONE,
    )
    .unwrap();
    settle(&mut t);
    assert_eq!((zoom(&mut t), state(&mut t)["view"].clone()), (json!("fit"), json!("edit")));
    // Pressed and held: 100 % at that point, kept after the release.
    t.d.pointer(&[PointerEvent { kind: PointerKind::Down, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.wait(200).unwrap();
    assert_eq!(zoom(&mut t), json!("fit"), "not yet");
    t.d.wait(200).unwrap();
    assert_eq!(zoom(&mut t), json!(1.0));
    t.d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    settle(&mut t);
    let v = view(&t);
    assert_eq!((zoom(&mut t), v.scale), (json!(1.0), 1.0), "the release is no click");
    assert!(near(under(&v, p), q, 2e-3), "{:?} vs {q:?}", under(&v, p));
    assert!(has(&t, "text:zoom") && has(&t, "navigator:edit"));
    let label = t.d.widgets().into_iter().find(|w| w.id == "text:zoom").unwrap().label;
    assert_eq!(label, "Zoomed to 100 %");
    // A click on the zoomed photo returns to fit (animated).
    t.d.click("canvas:edit").unwrap();
    assert!(view(&t).animating, "Fit animates");
    settle(&mut t);
    assert_eq!((zoom(&mut t), view(&t).scale), (json!("fit"), view(&t).fit));
    // A double-click returns to the library, and the preview is closed.
    t.d.wait(500).unwrap();
    // TODO(maintainer): the frame that leaves the edit view returns before the canvas registers its
    // id; once it registers first, keep the id check on here.
    t.d.check_ids(false);
    t.d.click_with("canvas:edit", &Click { count: 2, ..Click::default() }).unwrap();
    settle(&mut t);
    assert_eq!(state(&mut t)["view"], json!("library"));
    assert_eq!(t.d.app().session().unwrap().preview_photo(), None);
    // With "Single-click to leave the edit view" one click leaves and holding does not zoom.
    t.d.app_mut().session_mut().unwrap().change_settings(|st| {
        let mut e = st.edit();
        e.single_click_leaves = true;
        st.set_edit(&e);
    });
    run(&mut t, "view.edit", json!({ "id": photo }));
    let p = view(&t).image.center();
    t.d.pointer(&[PointerEvent { kind: PointerKind::Down, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.wait(400).unwrap();
    assert_eq!(zoom(&mut t), json!("fit"), "no hold zoom with the setting");
    t.d.pointer(&[PointerEvent { kind: PointerKind::Up, pos: p }], PointerButton::Primary, Modifiers::NONE).unwrap();
    settle(&mut t);
    assert_eq!(state(&mut t)["view"], json!("library"), "the click left");
    run(&mut t, "view.edit", json!({ "id": photo }));
    t.d.click("canvas:edit").unwrap();
    settle(&mut t);
    assert_eq!(state(&mut t)["view"], json!("library"), "one click leaves");
}

#[test]
fn drags_pan_and_stop_at_the_photos_edges() {
    let mut t = setup((1400, 900));
    // Fitted, a drag moves nothing.
    let before = view(&t).image;
    t.d.drag("canvas:edit", Target::Delta(vec2(200.0, 120.0)), &Drag::default()).unwrap();
    settle(&mut t);
    assert_eq!(view(&t).image, before);
    set_zoom(&mut t, json!(2.0));
    let v = view(&t);
    assert!(v.pannable && v.zoomed, "{v:?}");
    assert!((v.image.center() - v.area.center()).length() < 0.5, "zoomed about the centre");
    // A small drag moves the photo with the pointer.
    t.d.drag("canvas:edit", Target::Delta(vec2(-60.0, 40.0)), &Drag { steps: 30, ..Drag::default() }).unwrap();
    settle(&mut t);
    // (egui starts a drag a few points in: those are not part of it.)
    let moved = view(&t).image.min - v.image.min;
    assert!((moved.x + 60.0).abs() < 6.0 && (moved.y - 40.0).abs() < 6.0 && (moved.y / moved.x + 40.0 / 60.0).abs() < 0.02, "{moved:?}");
    // Far right and down: the left and top edges stop at the area's.
    t.d.drag("canvas:edit", Target::Delta(vec2(4000.0, 4000.0)), &Drag::default()).unwrap();
    settle(&mut t);
    let v = view(&t);
    assert!((v.image.min.x - v.area.min.x).abs() < 0.01 && (v.image.min.y - v.area.min.y).abs() < 0.01, "{v:?}");
    // Far left and up: the right and bottom edges stop at the area's.
    t.d.drag("canvas:edit", Target::Delta(vec2(-4000.0, -4000.0)), &Drag::default()).unwrap();
    settle(&mut t);
    let v = view(&t);
    assert!((v.image.max.x - v.area.max.x).abs() < 0.01 && (v.image.max.y - v.area.max.y).abs() < 0.01, "{v:?}");
    // The navigator shows where: a click on its centre brings the photo's centre back.
    t.d.click("navigator:edit").unwrap();
    settle(&mut t);
    let v = view(&t);
    assert!((v.image.center() - v.area.center()).length() < 2.0, "{v:?}");
}

/// The hand cursor: over a zoomed photo the open hand, while a drag pans it the closed hand, and
/// over the fitted photo (nothing to pan) the arrow. The cursor is in egui's frame output, which
/// the driver does not keep: these frames are run on egui directly.
#[test]
fn panning_a_zoomed_photo_shows_the_hand_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let mut app = App::new(Some(s));
    let ctx = egui::Context::default();
    app.install(&ctx);
    let mut time = 0.0;
    let mut frame = |app: &mut App, events: Vec<egui::Event>| {
        let mut input = dalica_ui_kit::raw_input(1400, 900, 1.0);
        input.time = Some(time);
        input.events = events;
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        time += 1.0 / 60.0;
        out.platform_output.cursor_icon
    };
    app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
    for _ in 0..2000 {
        frame(&mut app, vec![]);
        if app.session().unwrap().preview().is_some() && app.session().unwrap().preview_pending() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let p = canvas::view(&ctx).unwrap().area.center();
    let button = |pressed: bool, pos: Pos2| egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame(&mut app, vec![egui::Event::PointerMoved(p)]);
    assert_eq!(frame(&mut app, vec![]), egui::CursorIcon::Default, "fitted: nothing to pan");
    app.state_mut().edit.zoom = dalica_ui::state::Zoom::Scale(2.0);
    for _ in 0..30 {
        frame(&mut app, vec![]);
    }
    assert!(canvas::view(&ctx).unwrap().pannable);
    assert_eq!(frame(&mut app, vec![]), egui::CursorIcon::Grab, "zoomed: the open hand");
    frame(&mut app, vec![button(true, p)]);
    let mut during = Vec::new();
    for i in 1..=10 {
        during.push(frame(&mut app, vec![egui::Event::PointerMoved(p + vec2(-8.0 * i as f32, 4.0 * i as f32))]));
    }
    assert_eq!(during.last(), Some(&egui::CursorIcon::Grabbing), "panning: the closed hand ({during:?})");
    let end = p + vec2(-80.0, 40.0);
    frame(&mut app, vec![button(false, end)]);
    assert_eq!(frame(&mut app, vec![]), egui::CursorIcon::Grab, "let go: the open hand again");
}

/// Waits for the preview's jobs in real time and runs one frame, until `done` holds (at most ten
/// rounds): the frame that shows or reports what landed, however long the render took.
fn until_preview(t: &mut T, what: &str, mut done: impl FnMut(&mut T) -> bool) {
    for _ in 0..10 {
        if done(t) {
            return;
        }
        let session = t.d.app_mut().session_mut().unwrap();
        assert!(session.wait_preview(Duration::from_secs(60)), "{what}: the preview's jobs did not finish within 60 s");
        t.d.step();
    }
    assert!(done(t), "{what}: not after ten waits for the preview");
}

fn perf_has(t: &mut T, key: &str) -> bool {
    ok(t, "ui.inspect", json!({}))["perf"][key].is_number()
}

#[test]
fn zooming_in_asks_for_the_refined_window_and_no_frame_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let mut t = T { _dir: dir, control, conn: Connection::new(), d, ids };
    let photo = t.ids[0];
    // The open's final preview time is reported by the frame that shows it (read then, as the perf
    // window holds the last 240 frames), and a perf reset keeps it.
    ok(&mut t, "engine.execute", json!({ "command": "view.edit", "params": { "id": photo } }));
    until_preview(&mut t, "the open's final", |t| perf_has(t, "previewFinalMs"));
    ok(&mut t, "ui.inspect", json!({ "resetPerf": true }));
    let perf = ok(&mut t, "ui.inspect", json!({}))["perf"].clone();
    assert!(perf["previewFinalMs"].is_number(), "the open's final preview time: {perf}");
    settle(&mut t);
    // One frame after the zoom: the window is asked for and the frame drew the scaled preview.
    ok(&mut t, "ui.set", json!({ "edit": { "zoom": 2.0 } }));
    t.d.step();
    let v = view(&t);
    assert!(v.zoomed && v.refining && !v.refined, "{v:?}");
    assert!(t.d.app().session().unwrap().preview_refined().is_none(), "the frame did not wait");
    // The frame that draws the refined window reports its time; the frames before it waited.
    until_preview(&mut t, "the refined window", |t| view(t).refined);
    let perf = ok(&mut t, "ui.inspect", json!({}))["perf"].clone();
    assert!(perf["previewWaitingFrames"].as_u64().unwrap() >= 1, "{perf}");
    assert!(perf["previewRefinedMs"].is_number(), "{perf}");
    settle(&mut t);
    let v = view(&t);
    assert!(v.refined && !v.refining, "{v:?}");
    let r = t.d.app().session().unwrap().preview_refined().unwrap().region;
    assert_eq!(r.scale, 2.0);
    let visible = v.image.intersect(v.area);
    assert_eq!((r.width as f32, r.height as f32), (visible.width().ceil(), visible.height().ceil()), "the visible window");
    // Back to fit: the window and the whole decode are let go.
    key(&mut t, "Ctrl+-");
    set_zoom(&mut t, json!("fit"));
    assert!(t.d.app().session().unwrap().preview_refined().is_none());
}

#[test]
fn fit_and_1to1_animate_and_the_slider_drives_the_zoom() {
    let mut t = setup((1400, 900));
    let fit = view(&t).fit;
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:zoom.1to1" }));
    for _ in 0..4 {
        t.d.step();
    }
    let v = view(&t);
    assert!(v.animating && v.scale > fit && v.scale < 1.0, "on its way: {v:?}");
    settle(&mut t);
    assert_eq!((zoom(&mut t), view(&t).scale), (json!(1.0), 1.0));
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:zoom.fit" }));
    t.d.step();
    assert!(view(&t).animating);
    t.d.wait(240).unwrap();
    assert_eq!(view(&t).scale, fit, "done after 0.22 s");
    // The slider: its middle is 100 %, its right end 400 %.
    let slider = t.d.widgets().into_iter().find(|w| w.id == "slider:zoom").unwrap();
    let inset = 2.5;
    let mid = slider.rect[0] + slider.rect[2] / 2.0;
    ok(
        &mut t,
        "ui.pointer",
        json!({ "events": [{ "kind": "down", "x": mid, "y": slider.rect[1] + 10.0 }, { "kind": "up", "x": mid, "y": slider.rect[1] + 10.0 }] }),
    );
    settle(&mut t);
    assert!((zoom(&mut t).as_f64().unwrap() - 1.0).abs() < 0.02, "{}", zoom(&mut t));
    let right = slider.rect[0] + slider.rect[2] - inset;
    ok(
        &mut t,
        "ui.pointer",
        json!({ "events": [{ "kind": "down", "x": right, "y": slider.rect[1] + 10.0 }, { "kind": "up", "x": right, "y": slider.rect[1] + 10.0 }] }),
    );
    settle(&mut t);
    assert_eq!(zoom(&mut t), json!(4.0));
    assert_eq!(t.d.widgets().into_iter().find(|w| w.id == "text:zoom").unwrap().label, "Zoomed to 400 %");
}

#[test]
fn the_bottom_areas_star_and_rotate_act_on_the_photo_shown() {
    let mut t = setup((1400, 900));
    let (a, b) = (t.ids[0], t.ids[1]);
    // Two photos selected, the first shown.
    run(&mut t, "view.library", json!({}));
    run(&mut t, "select.set", json!({ "ids": [a, b] }));
    run(&mut t, "view.edit", json!({ "id": a }));
    assert_eq!(state(&mut t)["selection"]["ids"], json!([a, b]));
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:star" }));
    settle(&mut t);
    let library = t.d.app().session().unwrap().library();
    let star = |id: u64| library.photo(dalica_core::PhotoId::new(id)).unwrap().star;
    assert!(star(a) && !star(b), "only the photo shown");
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:rotate.cw" }));
    settle(&mut t);
    let edits = |t: &mut T, id: u64| run(t, "photo.history.state", json!({ "id": id }))["edits"].clone();
    assert_eq!(edits(&mut t, a), json!(["rotate(90)"]), "a step of its history");
    assert_eq!(edits(&mut t, b), json!([]));
    // The photo turned: the canvas lays it out portrait.
    let v = view(&t);
    assert_eq!(v.size, (PHOTO.1 as f32, PHOTO.0 as f32));
}

/// Checks the pixels of `rect` of the window against `tests/fingerprints/<name>.json`
/// (`DALICA_BLESS=1` writes it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: [f32; 4]) {
    check_rect(&t.d, name, rect);
}

/// The photo area fitted and at 200 % (the refined window drawn, the read-out and the navigator).
#[test]
fn the_canvas_matches_its_fingerprints_fitted_and_at_200_percent() {
    let (w, h) = LARGE;
    let mut t = setup((w, h));
    let rect = t.d.widgets().into_iter().find(|wi| wi.id == "region:canvas").unwrap().rect;
    check_region(&t, &format!("edit-canvas-fit-{w}x{h}"), rect);
    set_zoom(&mut t, json!(2.0));
    assert!(view(&t).refined, "{w}×{h}");
    check_region(&t, &format!("edit-canvas-200-{w}x{h}"), rect);
}
