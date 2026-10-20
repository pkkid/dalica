//! The slideshow's control bar, zoom and settings (`viewing.md` §1.2–§1.5, `roadmap.md` M11.21),
//! headless on a virtual session clock: the bar shows at the start, hides after 2 s and comes back
//! on a mouse move; Previous is greyed on the first photo without Loop; Wipe picked in the drop-up
//! is the next change's transition and is still the setting after a restart; + and − of the display
//! time stay within 1–60; zooming pauses and a drag pans; the edit view's caption toggle and the
//! bar's change one setting; Options ▸ Slideshow's Loop wraps the show; fingerprints of the bar and
//! of the tab at 1400 × 900 (at 900 × 600 they only have to fit).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_region, check_sizes, new_driver, shot, try_widget, widget};
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_create::slideshow::Transition;
use dalica_engine::Session;
use dalica_ui::commands::slideshow as cmd;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::slideshow::bar;
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);
const STEP: Duration = Duration::from_millis(100);

struct Fx {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    d: Driver,
    order: Vec<PhotoId>,
    section: String,
}

fn library(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("library")
}

/// A library holding one folder of `n` scenes photos of `w × h` pixels, on a virtual clock, in the
/// large window.
fn setup(n: usize, (w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    for k in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(70 + k as u64, w, h), 85, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{k:04}.jpg")), bytes).unwrap();
    }
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_780_000_000_000)));
    let mut s = Session::open(&library(&dir), clock.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let any = s.execute("library.query", &json!({ "limit": 1 })).unwrap()["photos"][0]["id"].as_u64().unwrap();
    let section = format!("folder:{}", s.library().photo(PhotoId::new(any)).unwrap().folder);
    let list = s.execute("slideshow.list", &json!({ "section": section })).unwrap();
    let order: Vec<PhotoId> = serde_json::from_value(list["photos"].clone()).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, clock, d, order, section }
}

fn session(f: &mut Fx) -> &mut Session {
    f.d.app_mut().session_mut().unwrap()
}

/// Lets `ms` pass on the session's clock, a frame every [`STEP`].
fn pass(f: &mut Fx, ms: u64) {
    for _ in 0..ms / STEP.as_millis() as u64 {
        f.clock.advance(STEP);
        assert!(session(f).wait_preview(PATIENCE));
        f.d.step();
    }
}

fn show(f: &Fx) -> dalica_ui::state::Slideshow {
    f.d.state().slideshow.clone().expect("a slideshow plays")
}

fn run(f: &mut Fx, id: &str, params: Value) -> Value {
    let v = f.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    f.d.step();
    v
}

/// Plays the folder from its photo `at`.
fn play(f: &mut Fx, at: usize) {
    let p = json!({ "section": f.section, "ids": [f.order[at]] });
    run(f, "section.slideshow", p);
}

fn click(f: &mut Fx, id: &str) {
    f.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    f.d.step();
}

/// A mouse move over the photo, away from the bar.
fn wiggle(f: &mut Fx) {
    let moves =
        [PointerEvent { kind: PointerKind::Move, pos: pos2(300.0, 200.0) }, PointerEvent { kind: PointerKind::Move, pos: pos2(330.0, 220.0) }];
    f.d.pointer(&moves, PointerButton::Primary, Modifiers::NONE).unwrap();
}

fn settings(f: &mut Fx) -> dalica_engine::settings::SlideshowSettings {
    session(f).settings().slideshow()
}

/// The bar shows for 2 s at the start and then fades away; a mouse move brings it back while the
/// show plays on. Previous is greyed on the first photo without Loop and live on the next. The
/// bar's fingerprint at 1400 × 900; at 900 × 600 it fits.
#[test]
fn the_bar_shows_at_start_hides_after_2_s_and_comes_back_on_a_mouse_move() {
    let mut f = setup(4, (640, 420));
    play(&mut f, 0);
    assert!(try_widget(&f.d, bar::BAR_ID).is_some(), "shown at the start");
    let prev = widget(&f.d, bar::PREVIOUS_ID);
    assert!(!prev.enabled, "Previous is greyed on the first photo without Loop: {prev:?}");
    check_region(&f.d, bar::BAR_ID, "slideshow-bar-1400x900");
    pass(&mut f, 1_800);
    assert!(try_widget(&f.d, bar::BAR_ID).is_some(), "still shown before 2 s");
    pass(&mut f, 500);
    assert!(try_widget(&f.d, bar::BAR_ID).is_none(), "hidden (faded out) after 2 s");
    assert!(!show(&f).bar);
    wiggle(&mut f);
    f.d.step();
    let s = show(&f);
    assert!(s.bar && s.playing, "a mouse move shows the bar and the show plays on: {s:?}");
    pass(&mut f, 300);
    assert!(try_widget(&f.d, bar::BAR_ID).is_some(), "faded in");
    click(&mut f, bar::NEXT_ID);
    assert_eq!(show(&f).index, 1);
    assert!(widget(&f.d, bar::PREVIOUS_ID).enabled, "Previous is live past the first photo");
    f.d.resize(SMALL.0, SMALL.1);
    wiggle(&mut f);
    pass(&mut f, 200);
    let r = widget(&f.d, bar::BAR_ID).rect;
    assert!(r[0] >= 0.0 && r[0] + r[2] <= SMALL.0 as f32, "the bar fits 900 wide: {r:?}");
    check_fits(&f.d, "slideshow-bar-900x600");
    shot(&f.d, "slideshow-bar-900x600");
}

/// Wipe picked in the Transition drop-up is the setting, the next change runs it, and it is still
/// the setting when the library is opened again.
#[test]
fn wipe_picked_in_the_drop_up_is_the_next_transition_and_survives_a_restart() {
    let mut f = setup(3, (96, 64));
    play(&mut f, 0);
    click(&mut f, bar::TRANSITION_ID);
    click(&mut f, &bar::transition_item_id(Transition::Wipe));
    assert_eq!(settings(&mut f).transition.id(), "wipe");
    assert_eq!(widget(&f.d, bar::TRANSITION_ID).label, "Transition");
    click(&mut f, bar::NEXT_ID);
    let frame = cmd::frame().unwrap();
    assert_eq!((frame.view.change.map(|c| c.0), frame.view.transition), (Some(0), Transition::Wipe), "the next change wipes");
    run(&mut f, "slideshow.exit", json!({}));
    let Fx { dir, clock, d, .. } = f;
    drop(d);
    let s = Session::open(&library(&dir), clock).unwrap();
    assert_eq!(s.settings().slideshow().transition.id(), "wipe", "kept in settings.json");
}

/// The display time's − and + step by one second and stop at 1 and 60.
#[test]
fn plus_and_minus_change_the_display_time_within_1_to_60() {
    let mut f = setup(2, (96, 64));
    play(&mut f, 0);
    let (minus, plus) = dalica_ui_kit::widgets::stepper_ids(bar::DISPLAY_TIME_ID);
    click(&mut f, &plus);
    assert_eq!(settings(&mut f).display_time, 4);
    assert_eq!(widget(&f.d, bar::DISPLAY_TIME_ID).label, "4 seconds");
    for _ in 0..3 {
        click(&mut f, &minus);
    }
    assert_eq!(settings(&mut f).display_time, 1);
    assert!(!widget(&f.d, &minus).enabled, "− stops at 1 second");
    assert_eq!(widget(&f.d, bar::DISPLAY_TIME_ID).label, "1 second");
    run(&mut f, "slideshow.displayTime", json!({ "seconds": 59 }));
    click(&mut f, &plus);
    assert_eq!(settings(&mut f).display_time, 60);
    assert!(!widget(&f.d, &plus).enabled, "+ stops at 60 seconds");
    assert!(f.d.run("slideshow.displayTime", &json!({ "seconds": 61 })).is_err());
    assert_eq!(settings(&mut f).display_time, 60);
}

/// The + key zooms the photo in and pauses the show (the clock then moves nothing), a drag pans it, − zooms
/// back out and the show plays on; the magnifier zooms too.
#[test]
fn zooming_pauses_the_show_and_a_drag_pans() {
    let mut f = setup(3, (640, 420));
    play(&mut f, 0);
    pass(&mut f, 2_500);
    f.d.key("Plus").unwrap();
    f.d.step();
    let s = show(&f);
    assert!(s.zoom > 1.0 && !s.playing, "zoomed and paused: {s:?}");
    pass(&mut f, 4_000);
    assert_eq!(show(&f).index, 0, "paused while zoomed");
    let drag = [
        PointerEvent { kind: PointerKind::Down, pos: pos2(700.0, 300.0) },
        PointerEvent { kind: PointerKind::Drag, pos: pos2(650.0, 280.0) },
        PointerEvent { kind: PointerKind::Drag, pos: pos2(600.0, 260.0) },
        PointerEvent { kind: PointerKind::Up, pos: pos2(600.0, 260.0) },
    ];
    f.d.pointer(&drag, PointerButton::Primary, Modifiers::NONE).unwrap();
    f.d.step();
    let pan = show(&f).pan;
    assert!(pan[0] > 0.5 && pan[1] > 0.5, "dragged up and left, the photo shows more of its lower right: {pan:?}");
    assert_eq!(show(&f).index, 0, "a drag is no click");
    f.d.key("-").unwrap();
    f.d.step();
    let s = show(&f);
    assert!(s.zoom == 1.0 && s.pan == [0.5, 0.5] && s.playing, "fitted and playing again: {s:?}");
    wiggle(&mut f);
    pass(&mut f, 300);
    click(&mut f, bar::ZOOM_ID);
    let s = show(&f);
    assert!(s.zoom == dalica_ui::panels::slideshow::zoom::MAGNIFIER_ZOOM && !s.playing, "the magnifier zooms: {s:?}");
    assert!(widget(&f.d, bar::ZOOM_ID).active);
}

/// The edit view's caption-bar toggle and the bar's Captions change one setting, which the show's
/// caption follows.
#[test]
fn the_captions_toggles_of_the_edit_view_and_of_the_bar_change_one_setting() {
    let mut f = setup(2, (96, 64));
    let o = f.order.clone();
    session(&mut f).execute("photo.caption.set", &json!({ "ids": [o[0]], "caption": "Harbour" })).unwrap();
    run(&mut f, "view.edit", json!({ "id": o[0] }));
    f.d.settle(FRAMES).unwrap();
    assert!(settings(&mut f).captions, "on by default");
    f.d.click("button:caption.toggle").unwrap();
    f.d.settle(FRAMES).unwrap();
    assert!(!settings(&mut f).captions, "the edit view's toggle turns them off");
    f.d.click("button:edit.play").unwrap();
    f.d.step();
    assert!(!widget(&f.d, bar::CAPTIONS_ID).active, "the bar shows the same setting");
    assert!(try_widget(&f.d, dalica_ui::panels::slideshow::show::CAPTION_ID).is_none());
    click(&mut f, bar::CAPTIONS_ID);
    assert!(settings(&mut f).captions && widget(&f.d, bar::CAPTIONS_ID).active, "the bar turns them on again");
    assert_eq!(widget(&f.d, dalica_ui::panels::slideshow::show::CAPTION_ID).label, "Harbour");
}

/// Options ▸ Slideshow: its Loop wraps the show (Next on the last photo shows the first, and
/// Previous is live on the first); the tab's fingerprint at 1400 × 900, and it fits 900 × 600.
#[test]
fn options_slideshow_loop_wraps_the_show() {
    let mut f = setup(3, (96, 64));
    f.d.run("app.preferences", &json!({ "tab": "slideshow" })).unwrap();
    f.d.settle(FRAMES).unwrap();
    for id in ["check:option.slideshow.music", "check:option.slideshow.videoSound"] {
        assert!(widget(&f.d, id).enabled, "{id} is live now that Dalica plays sound (M15.27)");
    }
    check_sizes(&mut f.d, "dialog:options", "dialog-options-slideshow");
    f.d.click("check:option.slideshow.loop").unwrap();
    f.d.settle(FRAMES).unwrap();
    shot(&f.d, "dialog-options-slideshow-loop-1400x900");
    f.d.click("button:dialog.ok").unwrap();
    f.d.settle(FRAMES).unwrap();
    assert!(settings(&mut f).looping);
    play(&mut f, 2);
    run(&mut f, "slideshow.next", json!({}));
    assert_eq!(show(&f).index, 0, "Loop wraps from the last photo to the first");
    assert!(widget(&f.d, bar::PREVIOUS_ID).enabled, "with Loop, Previous is live on the first photo");
    run(&mut f, "slideshow.previous", json!({}));
    assert_eq!(show(&f).index, 2);
}
