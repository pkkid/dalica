//! The slideshow in the window (`viewing.md` §1.1–§1.4, `roadmap.md` M11.19), headless on a
//! virtual session clock: Ctrl+4 with a photo selected plays its section from it; three display
//! times advance three photos; Space pauses and a mouse move does not; → ← Home End; Esc returns
//! to the library with the last photo shown selected and gives the window back; started from the
//! edit view's Play it returns to the edit view on that photo; Ctrl+8 stars and Ctrl+R rotates the
//! photo shown, one undo step each; with every decode slowed by 1.5 s no change waits for its
//! preview over 30 advances; a fingerprint of each transition at its midpoint and of a captioned
//! photo at 1400 × 900 (at 900 × 600 the caption only has to fit).
//!
//! `DALICA_SHOTS=<dir>` writes the frames there too.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, shot, widget};
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_ui::commands::slideshow as cmd;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);
/// One step of the virtual clock.
const STEP: Duration = Duration::from_millis(100);
/// The default display time (Options ▸ Slideshow).
const DISPLAY_MS: u64 = 3_000;

struct Fx {
    _dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    d: Driver,
    /// The folder's photos in display order.
    order: Vec<PhotoId>,
    /// The folder's section (`folder:<id>`).
    section: String,
}

/// A library holding one folder of `n` scenes photos of `w × h` pixels, on a virtual clock, in a
/// window of `size`.
fn setup(n: usize, (w, h): (u32, u32), size: (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    for k in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + k as u64, w, h), 85, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{k:04}.jpg")), bytes).unwrap();
    }
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_780_000_000_000)));
    let mut s = Session::open(&dir.path().join("library"), clock.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let any = s.execute("library.query", &json!({ "limit": 1 })).unwrap()["photos"][0]["id"].as_u64().unwrap();
    let f = s.library().photo(PhotoId::new(any)).unwrap().folder;
    let section = format!("folder:{f}");
    let list = s.execute("slideshow.list", &json!({ "section": section })).unwrap();
    let order: Vec<PhotoId> = serde_json::from_value(list["photos"].clone()).unwrap();
    assert_eq!(order.len(), n);
    let mut d = new_driver(App::new(Some(s)), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, clock, d, order, section }
}

fn session(f: &mut Fx) -> &mut Session {
    f.d.app_mut().session_mut().unwrap()
}

/// Lets `ms` pass on the session's clock, a frame every [`STEP`], each after the preview's work
/// has landed (the show itself never waits for it).
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

fn index(f: &Fx) -> usize {
    show(f).index
}

fn key(f: &mut Fx, combo: &str) {
    f.d.key(combo).unwrap();
    f.d.step();
}

fn run(f: &mut Fx, id: &str, params: Value) -> Value {
    let v = f.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    f.d.step();
    v
}

/// Ctrl+4 plays the selected photo's section from it, full screen; three display times advance
/// three photos; a mouse move shows the bar and the show plays on, Space pauses it (and the clock
/// then moves nothing) and plays it again; → ← End Home step; Esc goes back to the library with
/// the last photo shown selected and the window as it was.
#[test]
fn ctrl_4_plays_the_section_and_esc_returns_with_the_last_photo_selected() {
    let mut f = setup(12, (96, 64), LARGE);
    let o = f.order.clone();
    run(&mut f, "select.set", json!({ "ids": [o[2]] }));
    assert!(!f.d.state().full_screen);
    key(&mut f, "Ctrl+4");
    let s = show(&f);
    assert_eq!(f.d.state().view, dalica_ui::state::View::Slideshow);
    assert_eq!((s.photos.clone(), s.index, s.playing), (o.clone(), 2, true), "the folder in display order, from the selected photo");
    assert!(f.d.state().full_screen, "full screen asked through the view state");
    assert!(widget(&f.d, cmd_canvas()).rect[2] >= LARGE.0 as f32, "the show fills the window");

    pass(&mut f, 3 * DISPLAY_MS + 100);
    assert_eq!(index(&f), 5, "three display times, three photos");

    // A mouse move shows the bar and does not pause.
    f.d.pointer(
        &[PointerEvent { kind: PointerKind::Move, pos: pos2(300.0, 300.0) }, PointerEvent { kind: PointerKind::Move, pos: pos2(340.0, 320.0) }],
        PointerButton::Primary,
        Modifiers::NONE,
    )
    .unwrap();
    let s = show(&f);
    assert!(s.playing && s.bar, "{s:?}");
    key(&mut f, "Space");
    assert!(!show(&f).playing, "Space pauses");
    pass(&mut f, 2 * DISPLAY_MS);
    assert_eq!(index(&f), 5, "paused, the clock moves nothing");
    key(&mut f, "Space");
    assert!(show(&f).playing, "and plays again");

    key(&mut f, "Right");
    assert_eq!(index(&f), 6);
    key(&mut f, "Left");
    key(&mut f, "Left");
    assert_eq!(index(&f), 4);
    key(&mut f, "End");
    assert_eq!(index(&f), 11);
    key(&mut f, "Home");
    assert_eq!(index(&f), 0);
    key(&mut f, "Right");
    pass(&mut f, 1_000);

    key(&mut f, "Esc");
    let st = f.d.state();
    assert_eq!((st.view, st.slideshow.is_none()), (dalica_ui::state::View::Library, true));
    assert_eq!(st.selection.ids, vec![o[1]], "the last photo shown is selected");
    assert!(!st.full_screen, "the window is given back as it was");
    assert_eq!(cmd::stats(), None, "the show and its panes are let go");
    assert_eq!((session(&mut f).preview_photo_in(0), session(&mut f).preview_photo_in(1)), (None, None));
}

fn cmd_canvas() -> &'static str {
    dalica_ui::panels::slideshow::ID
}

/// The edit top bar's Play plays the photo's folder from it, and Esc returns to the edit view on
/// the last photo shown.
#[test]
fn started_from_the_edit_views_play_it_returns_to_the_edit_view() {
    let mut f = setup(5, (96, 64), LARGE);
    let o = f.order.clone();
    run(&mut f, "view.edit", json!({ "id": o[1] }));
    f.d.settle(FRAMES).unwrap();
    f.d.click("button:edit.play").unwrap();
    f.d.step();
    let s = show(&f);
    assert_eq!((s.index, s.from), (1, dalica_ui::state::ShowFrom::Edit));
    key(&mut f, "Right");
    key(&mut f, "Esc");
    let st = f.d.state();
    assert_eq!((st.view, st.edit.photo), (dalica_ui::state::View::Edit, Some(o[2])), "back in the edit view, on the last photo shown");
}

/// Ctrl+8 stars and Ctrl+R rotates the photo shown, through the library's commands, one undo step
/// each; the library keeps its own Ctrl+8.
#[test]
fn ctrl_8_stars_and_ctrl_r_rotates_the_photo_shown() {
    let mut f = setup(3, (96, 64), LARGE);
    let o = f.order.clone();
    run(&mut f, "select.set", json!({ "ids": [o[1]] }));
    key(&mut f, "Ctrl+4");
    key(&mut f, "Right");
    let shown = show(&f).photo().unwrap();
    assert_eq!(shown, o[2]);
    let steps = |f: &mut Fx| session(f).undo_stack().len();
    let before = steps(&mut f);
    key(&mut f, "Ctrl+8");
    assert!(session(&mut f).library().photo(shown).unwrap().star, "starred");
    assert!(!session(&mut f).library().photo(o[1]).unwrap().star, "the selection is not what it acts on");
    assert_eq!(steps(&mut f), before + 1);
    key(&mut f, "Ctrl+R");
    assert_eq!(session(&mut f).library().info(shown).edits, ["rotate(90)"]);
    assert_eq!(steps(&mut f), before + 2, "one undo step each");
    let library = session(&mut f).keymap().keys_for("photo.star.toggle", dalica_engine::Context::Library);
    assert!(library.iter().any(|k| k.to_string() == "Ctrl+8"), "the library's Ctrl+8 stays: {library:?}");
}

/// With every decode slowed to 1.5 s on the default 3 s display time, 30 advances in the window
/// find the next photo's preview ready every time (M11.11's prefetch, driven by the panel).
#[test]
fn no_change_waits_for_a_preview_over_30_advances() {
    let mut f = setup(31, (96, 64), SMALL);
    session(&mut f).plant_decode_delay(Some(Duration::from_millis(1_500)));
    let o = f.order.clone();
    run(&mut f, "select.set", json!({ "ids": [o[0]] }));
    key(&mut f, "Ctrl+4");
    pass(&mut f, 1_500);
    for _ in 0..2_000 {
        if cmd::stats().is_some_and(|s| s.changes >= 30) {
            break;
        }
        pass(&mut f, STEP.as_millis() as u64);
    }
    let stats = cmd::stats().unwrap();
    assert_eq!((stats.changes, stats.waits), (30, 0), "{stats:?}");
    assert_eq!(index(&f), 30);
}

/// Each transition at the midpoint of its change (Pan and Zoom too, its photos moving), and a
/// captioned photo: fingerprints at 1400 × 900; at 900 × 600 the caption plate fits the window.
#[test]
fn each_transition_at_its_midpoint_and_a_captioned_photo() {
    let mut f = setup(2, (640, 420), LARGE);
    let o = f.order.clone();
    let full = [0.0, 0.0, LARGE.0 as f32, LARGE.1 as f32];
    // The header's ▶ passes its section (and the selection inside it).
    let play = json!({ "section": f.section, "ids": [o[0]] });
    for t in ["cut", "panAndZoom", "dissolve", "wipe", "circle", "push", "rectangle"] {
        session(&mut f).execute("options.set", &json!({ "values": { "slideshow.transition": t } })).unwrap();
        run(&mut f, "section.slideshow", play.clone());
        pass(&mut f, DISPLAY_MS + 400);
        let s = show(&f);
        let progress = s.transition.map(|r| (r.from, r.progress));
        if t == "cut" {
            assert_eq!((s.index, progress), (1, None), "Cut takes no time");
        } else {
            assert_eq!((s.index, progress), (1, Some((0, 0.5))), "{t}: half-way from the first photo to the second");
        }
        check_rect(&f.d, &format!("slideshow-{t}-1400x900"), full);
        run(&mut f, "slideshow.exit", json!({}));
    }

    session(&mut f)
        .execute("photo.caption.set", &json!({ "ids": [o[0]], "caption": "Morning light over the harbour, the boats still moored" }))
        .unwrap();
    run(&mut f, "section.slideshow", play.clone());
    pass(&mut f, 500);
    let plate = widget(&f.d, dalica_ui::panels::slideshow::show::CAPTION_ID);
    assert!(plate.label.starts_with("Morning light"), "{plate:?}");
    check_rect(&f.d, "slideshow-caption-1400x900", full);
    f.d.resize(SMALL.0, SMALL.1);
    pass(&mut f, 200);
    let plate = widget(&f.d, dalica_ui::panels::slideshow::show::CAPTION_ID).rect;
    assert!(plate[0] >= 0.0 && plate[1] >= 0.0 && plate[0] + plate[2] <= SMALL.0 as f32 && plate[1] + plate[3] <= SMALL.1 as f32, "{plate:?}");
    assert!(plate[2] <= 0.7 * SMALL.0 as f32 + 1.0, "at most 70 % of the width: {plate:?}");
    check_fits(&f.d, "slideshow-caption-900x600");
    shot(&f.d, "slideshow-caption-900x600");
}
