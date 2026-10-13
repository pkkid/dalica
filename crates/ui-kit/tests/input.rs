//! The canvas input helpers (`ui-kit::input`, `editing.md` §3): the wheel, the trackpad and the
//! pinch classify as they do in PhotoCraft, whose tests these are, ported from
//! `crates/ui-egui/src/wheel_nav.rs` and `crates/ui-egui/src/flick_pan.rs` at revision 10fb9233
//! (MIT OR Apache-2.0; `NOTICE`), with Dalica's rules in place of PhotoCraft's preferences: a
//! notched wheel zooms, a trackpad scroll pans, ⌥/Alt + wheel pans; and a flick glides and stops.

use dalica_ui_kit::input::flick::{FLICK_MAX_DT, FLICK_MAX_SPEED, FLICK_TAU, Flick, frame};
use dalica_ui_kit::input::wheel::{Input, Wheel, classify, configure, fold_legacy_pinch, legacy_pinch_to_zoom, read};
use egui::{Context, Event, Id, Modifiers, MouseWheelUnit, PointerButton, RawInput, TouchPhase, Vec2, pos2, vec2};

// ---- The wheel ----

/// A notched wheel's input (`notched`) or a trackpad's.
fn input(scroll: Vec2, alt: bool, notched: bool) -> Input {
    Input { scroll, raw: scroll, zoom_delta: 1.0, alt, notched, notch: 40.0 }
}

fn wheel(unit: MouseWheelUnit, delta: Vec2, modifiers: Modifiers) -> Event {
    Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers }
}

fn notch(delta: Vec2, modifiers: Modifiers) -> Event {
    wheel(MouseWheelUnit::Line, delta, modifiers)
}

const CTRL: Modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
const CMD: Modifiers = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };

/// Feed `frames` of events through an egui context set up as the app's (`legacy`: the Windows
/// pinch fold on) and add up what the wheel did to the view over the following second.
fn gesture(frames: Vec<Vec<Event>>, legacy: bool) -> (Vec2, f32) {
    let ctx = Context::default();
    configure(&ctx);
    let (mut pan, mut zoom) = (Vec2::ZERO, 1.0);
    let mut frames = frames.into_iter();
    for n in 0..60 {
        let mut raw =
            RawInput { time: Some(f64::from(n) / 60.0), predicted_dt: 1.0 / 60.0, events: frames.next().unwrap_or_default(), ..Default::default() };
        if legacy {
            fold_legacy_pinch(&ctx, &mut raw);
        }
        let mut out = ctx.run_ui(raw, |ui| match read(ui.ctx()) {
            Some(Wheel::Pan(d)) => pan += d,
            Some(Wheel::Zoom(f)) => zoom *= f,
            None => {}
        });
        out.textures_delta.clear();
    }
    (pan, zoom)
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).length() < 0.01
}

fn zoom_of(w: Option<Wheel>) -> f32 {
    match w {
        Some(Wheel::Zoom(f)) => f,
        other => panic!("expected a zoom, got {other:?}"),
    }
}

#[test]
fn a_notched_wheel_zooms_ten_percent_per_notch() {
    let f = zoom_of(classify(input(vec2(0.0, 40.0), false, true)));
    assert!((f - 1.1).abs() < 1e-5, "{f}");
    let f = zoom_of(classify(input(vec2(0.0, -40.0), false, true)));
    assert!((f - 1.0 / 1.1).abs() < 1e-5, "{f}");
    // Two notches in one frame are two steps, ×1.1 each.
    let f = zoom_of(classify(input(vec2(0.0, 80.0), false, true)));
    assert!((f - 1.21).abs() < 1e-4, "{f}");
}

#[test]
fn a_trackpad_scroll_pans() {
    assert_eq!(classify(input(vec2(3.0, -40.0), false, false)), Some(Wheel::Pan(vec2(3.0, -40.0))));
    assert_eq!(classify(input(Vec2::ZERO, false, false)), None);
    // Sideways on a notched wheel (⇧ or ⌘/Ctrl folded into x) pans too.
    assert_eq!(classify(Input { raw: Vec2::ZERO, ..input(vec2(40.0, 0.0), false, true) }), Some(Wheel::Pan(vec2(40.0, 0.0))));
}

#[test]
fn a_notch_zooms_the_frame_it_arrives_and_its_smoothed_tail_does_nothing() {
    // egui keeps feeding a smoothed delta for several frames after the wheel event. Zooming must
    // not follow it (that would ease the zoom), nor turn it into a pan.
    let tail = Input { raw: Vec2::ZERO, ..input(vec2(0.0, 12.0), false, true) };
    assert_eq!(classify(tail), None);
    // A trackpad's smoothed scroll still pans.
    let pan = Input { raw: Vec2::ZERO, ..input(vec2(0.0, 12.0), false, false) };
    assert_eq!(classify(pan), Some(Wheel::Pan(vec2(0.0, 12.0))));
}

#[test]
fn alt_and_the_wheel_pans() {
    assert_eq!(classify(input(vec2(0.0, 40.0), true, true)), Some(Wheel::Pan(vec2(0.0, 40.0))));
    assert_eq!(classify(input(vec2(0.0, 40.0), true, false)), Some(Wheel::Pan(vec2(0.0, 40.0))));
}

#[test]
fn a_pinch_zooms_and_wins_over_everything() {
    let i = Input { zoom_delta: 1.2, ..input(Vec2::ZERO, false, false) };
    assert_eq!(classify(i), Some(Wheel::Zoom(1.2)));
    let i = Input { zoom_delta: 0.8, ..input(vec2(0.0, 40.0), true, true) };
    assert_eq!(classify(i), Some(Wheel::Zoom(0.8)));
}

#[test]
fn each_wheel_and_modifier_maps_as_the_edit_view_wants() {
    let one = |e: Event| gesture(vec![vec![e]], false);
    let (pan, zoom) = one(notch(vec2(0.0, 1.0), Modifiers::NONE));
    assert!(pan == Vec2::ZERO && (zoom - 1.1).abs() < 1e-4, "a notch zooms 10 %, at once: {pan:?} {zoom}");
    let (pan, zoom) = one(notch(vec2(0.0, -2.0), Modifiers::NONE));
    assert!(pan == Vec2::ZERO && (zoom - 1.0 / 1.21).abs() < 1e-4, "two notches out: {pan:?} {zoom}");
    for m in [Modifiers::SHIFT, CTRL, CMD, Modifiers::CTRL] {
        let (pan, zoom) = one(notch(vec2(0.0, 1.0), m));
        assert!(near(pan, vec2(40.0, 0.0)) && zoom == 1.0, "{m:?} + wheel scrolls sideways: {pan:?} {zoom}");
    }
    let (pan, zoom) = one(notch(vec2(0.0, 1.0), Modifiers::ALT));
    assert!(pan.y.abs() > 1.0 && zoom == 1.0, "⌥ + wheel pans: {pan:?} {zoom}");
    let (pan, zoom) = one(wheel(MouseWheelUnit::Point, vec2(0.0, 12.0), Modifiers::NONE));
    assert!(near(pan, vec2(0.0, 12.0)) && zoom == 1.0, "a trackpad scroll pans: {pan:?} {zoom}");
    let (pan, zoom) = one(wheel(MouseWheelUnit::Point, vec2(-7.0, 0.0), Modifiers::NONE));
    assert!(near(pan, vec2(-7.0, 0.0)) && zoom == 1.0, "sideways too: {pan:?} {zoom}");
}

#[test]
fn a_trackpad_after_a_wheel_pans_and_a_wheel_after_a_trackpad_zooms() {
    let (pan, zoom) = gesture(
        vec![vec![notch(vec2(0.0, 1.0), Modifiers::NONE)], vec![], vec![wheel(MouseWheelUnit::Point, vec2(0.0, 5.0), Modifiers::NONE)]],
        false,
    );
    assert!((zoom - 1.1).abs() < 1e-4, "{zoom}");
    // From the trackpad's event on, the gesture is a trackpad's: it pans (and so does what is left
    // of the notch's smoothed tail, which a hand cannot tell apart from it).
    assert!(pan.y >= 5.0 && pan.x == 0.0, "the trackpad pans: {pan:?}");
    let (pan, zoom) =
        gesture(vec![vec![wheel(MouseWheelUnit::Point, vec2(0.0, 5.0), Modifiers::NONE)], vec![notch(vec2(0.0, 1.0), Modifiers::NONE)]], false);
    assert!((zoom - 1.1).abs() < 1e-4, "{zoom}");
    assert!(pan.y <= 5.01, "{pan:?}");
}

#[test]
fn a_pinch_zooms_and_ctrl_wheel_scrolls_sideways() {
    // macOS trackpads and browsers: the pinch is its own event.
    let (pan, zoom) = gesture(vec![vec![Event::Zoom(1.1)], vec![Event::Zoom(1.1)]], false);
    assert!(pan == Vec2::ZERO && (zoom - 1.21).abs() < 1e-4, "{pan:?} {zoom}");
    // Windows: a touchpad pinch is Ctrl + wheel in fractions of a notch, a mouse wheel whole notches.
    let (pan, zoom) = gesture(vec![vec![notch(vec2(0.0, 0.25), CTRL)], vec![notch(vec2(0.0, 0.5), CTRL)]], true);
    let speed = egui::InputOptions::default().scroll_zoom_speed;
    assert!(pan == Vec2::ZERO && (zoom - (speed * 40.0 * 0.75).exp()).abs() < 1e-4, "the folded pinch zooms: {pan:?} {zoom}");
    let (pan, zoom) = gesture(vec![vec![notch(vec2(0.0, 1.0), CTRL)]], true);
    assert!(near(pan, vec2(40.0, 0.0)) && zoom == 1.0, "Ctrl + mouse wheel scrolls sideways: {pan:?} {zoom}");
}

#[test]
fn a_whole_notch_inside_a_pinch_stays_a_pinch() {
    let mut ev = [notch(vec2(0.0, 0.5), CTRL)];
    let t = legacy_pinch_to_zoom(&mut ev, 1.0, None, 40.0, 0.005);
    assert_eq!(t, Some(1.0));
    assert!(matches!(ev[0], Event::Zoom(f) if (f - 0.1f32.exp()).abs() < 1e-5), "{ev:?}");
    // The next step lands on a whole notch: still the pinch.
    let mut ev = [notch(vec2(0.0, -1.0), CTRL)];
    assert_eq!(legacy_pinch_to_zoom(&mut ev, 1.1, t, 40.0, 0.005), Some(1.1));
    assert!(matches!(ev[0], Event::Zoom(f) if f < 1.0), "{ev:?}");
    // Long after: a mouse wheel notch, untouched.
    let mut ev = [notch(vec2(0.0, -1.0), CTRL)];
    assert_eq!(legacy_pinch_to_zoom(&mut ev, 2.0, Some(1.1), 40.0, 0.005), Some(1.1));
    assert!(matches!(ev[0], Event::MouseWheel { .. }));
    // Without Ctrl nothing is a pinch.
    let mut ev = [notch(vec2(0.0, 0.5), Modifiers::NONE)];
    assert_eq!(legacy_pinch_to_zoom(&mut ev, 2.0, None, 40.0, 0.005), None);
    assert!(matches!(ev[0], Event::MouseWheel { .. }));
}

#[test]
fn hostile_numbers_never_produce_a_bad_zoom() {
    for s in [f32::NAN, f32::INFINITY, -f32::INFINITY, 1e30, -1e30] {
        for alt in [false, true] {
            for notched in [false, true] {
                for notch_points in [0.0, -1.0, f32::NAN, 40.0] {
                    for zd in [f32::NAN, 0.0, -1.0, f32::INFINITY, 1.0] {
                        let i = Input { scroll: vec2(0.0, s), raw: vec2(0.0, s), zoom_delta: zd, alt, notched, notch: notch_points };
                        match classify(i) {
                            Some(Wheel::Zoom(f)) => assert!(f.is_finite() && f > 0.0, "{f} from {i:?}"),
                            Some(Wheel::Pan(d)) => assert!(d.is_finite(), "{d:?} from {i:?}"),
                            None => {}
                        }
                    }
                    let mut ev = [notch(vec2(s, 0.5), CTRL)];
                    let _ = legacy_pinch_to_zoom(&mut ev, f64::NAN, Some(f64::NAN), notch_points, 0.005);
                    let _ = legacy_pinch_to_zoom(&mut ev, 0.0, None, notch_points, f32::NAN);
                    if let Event::Zoom(f) = ev[0] {
                        assert!(f.is_finite() && f > 0.0, "{f}");
                    }
                }
            }
        }
    }
}

// ---- The glide ----

const DT: f32 = 1.0 / 60.0;

fn flung(speed_per_frame: f32) -> (Flick, f64) {
    let mut f = Flick::default();
    let mut t = 0.0;
    for _ in 0..10 {
        t += f64::from(DT);
        f.track(t, DT, vec2(speed_per_frame, 0.0));
    }
    (f, t + f64::from(DT))
}

#[test]
fn velocity_comes_from_the_last_moments_of_the_drag() {
    let (f, now) = flung(20.0);
    let v = f.release_velocity(now);
    assert!((v.x - 1200.0).abs() < 1.0, "{v:?}");
    assert_eq!(v.y, 0.0);
    // Held still for a while before letting go: no velocity.
    assert_eq!(f.release_velocity(now + 0.5), Vec2::ZERO);
}

#[test]
fn a_flick_glides_slows_and_stops() {
    let (mut f, now) = flung(20.0);
    assert!(f.release(now));
    let mut total = 0.0;
    let mut last = f32::INFINITY;
    let mut frames = 0;
    while let Some(d) = f.step(DT) {
        assert!(d.x > 0.0 && d.x < last, "decelerates: {d:?}");
        last = d.x;
        total += d.x;
        frames += 1;
        assert!(frames < 200, "stops");
    }
    // About a second of glide at most, covering about v·τ.
    assert!((20..=70).contains(&frames), "{frames} frames");
    assert!(total > 150.0 && total < 1200.0 * FLICK_TAU + 1.0, "{total}");
    assert!(!f.gliding());
}

#[test]
fn a_slow_release_does_not_glide() {
    let (mut f, now) = flung(1.0);
    assert!(!f.release(now), "60 pt/s is a stop, not a flick");
    assert_eq!(f.step(DT), None);
}

#[test]
fn stop_and_hostile_numbers() {
    let (mut f, now) = flung(20.0);
    f.release(now);
    f.stop();
    assert_eq!(f.step(DT), None);
    // Huge deltas are clamped; NaN deltas and steps are ignored.
    let (mut f, now) = flung(1.0e9);
    f.track(now - 0.001, DT, vec2(f32::NAN, 0.0));
    assert!(f.release_velocity(now).length() <= FLICK_MAX_SPEED + 1.0);
    f.release(now);
    let d = f.step(f32::NAN);
    assert!(d.is_none_or(|d| d.is_finite()));
    let d = f.step(f32::INFINITY).unwrap_or_default();
    assert!(d.is_finite() && d.length() <= FLICK_MAX_SPEED * FLICK_MAX_DT);
}

/// Through egui: a drag that pans, let go while moving, keeps the view moving over the following
/// frames and comes to rest; a click stops a glide at once.
#[test]
fn a_released_drag_glides_through_frames_and_a_click_stops_it() {
    let run = |click_at: Option<usize>| -> (Vec2, usize) {
        let ctx = Context::default();
        let id = Id::new("canvas");
        let (mut moved, mut gliding_frames) = (Vec2::ZERO, 0);
        for n in 0..120usize {
            let dragging = n < 10;
            let mut events = vec![];
            if Some(n) == click_at {
                events.push(Event::PointerButton { pos: pos2(5.0, 5.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            }
            let raw = RawInput { time: Some(n as f64 / 60.0), predicted_dt: DT, events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                if let Some(d) = frame(ui.ctx(), id, dragging, if dragging { vec2(20.0, -10.0) } else { Vec2::ZERO }) {
                    moved += d;
                    gliding_frames += 1;
                }
            });
            out.textures_delta.clear();
        }
        (moved, gliding_frames)
    };
    let (moved, frames) = run(None);
    assert!(moved.x > 100.0 && moved.y < -50.0, "glides the way it was flung: {moved:?}");
    assert!((10..=80).contains(&frames), "and stops: {frames} frames");
    let (stopped, frames) = run(Some(12));
    assert!(frames <= 2 && stopped.length() < moved.length() / 2.0, "a click stops it: {stopped:?} after {frames} frames");
}
