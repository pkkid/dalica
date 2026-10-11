//! The speed-control scroll bar and the scroll keys (M4.11, `ui-layout.md` §5.6), on the driver's
//! virtual clock: the speed is a pure function of the thumb's distance from the centre (nothing in
//! the dead zone, quadratic, the top speed at the track's end, the curve from the settings); a
//! release stops the view and the thumb is back at the centre 150 ms later; ⇑ / ⇓ and Ctrl+↑ /
//! Ctrl+↓ land headers at the top (⇑ first snaps to the current section's start) and presses made
//! while the view eases add up; the buttons repeat while held; the track pages by one row + 90; the
//! marker tracks the position; PgUp / PgDn and Ctrl+Home / Ctrl+End; the keymap has no conflicts;
//! every scroll command fails gracefully.

use std::path::Path;
use std::sync::Arc;

use dalica_engine::settings::ThumbSpeed;
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CmdError, Context, Key, Session};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::light_table::layout::Layout;
use dalica_ui::panels::light_table::scrollbar::{self, DEAD_ZONE, PAGE_EXTRA, SPRING_SECONDS, Scrollbar, THUMB_H};
use dalica_ui::panels::light_table::{FrameInfo, SCROLL_SECONDS};
use dalica_ui::state::Section;
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::WidgetInfo;
use egui::{Modifiers, PointerButton, Pos2, vec2};
use serde_json::json;

const FRAMES: usize = 60 * 60;
const SCROLL_COMMANDS: [&str; 6] = ["scroll.pageUp", "scroll.pageDown", "scroll.prevSection", "scroll.nextSection", "scroll.top", "scroll.bottom"];

/// An index-only synthetic library of `photos` photos in `folders` folders, shown at `size`.
fn driver(dir: &Path, photos: usize, folders: usize, size: (u32, u32)) -> Driver {
    let session = synthetic::open(dir, &Spec::new(photos, folders, synthetic::SEED), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = Driver::new(App::new(Some(session)), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn info(d: &Driver) -> FrameInfo {
    d.app().panels().unwrap().light_table.frame_info().clone()
}

fn layout(d: &Driver) -> Layout {
    d.app().panels().unwrap().light_table.layout().unwrap().clone()
}

fn bar(d: &Driver) -> &Scrollbar {
    &d.app().panels().unwrap().light_table.scrollbar
}

fn offset(d: &Driver) -> f64 {
    info(d).offset
}

fn view_h(d: &Driver) -> f32 {
    info(d).area.unwrap().height()
}

fn widget(d: &Driver, id: &str) -> WidgetInfo {
    d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}`"))
}

fn center(w: &WidgetInfo) -> Pos2 {
    Pos2::new(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0)
}

/// How far the thumb's centre can move either way.
fn reach(d: &Driver) -> f32 {
    (widget(d, "scroll:track").rect[3] - THUMB_H) / 2.0
}

/// Presses the thumb and pulls it `dy` points; it stays held.
fn hold_thumb(d: &mut Driver, dy: f32) {
    let c = center(&widget(d, "scroll:thumb"));
    let events = [(PointerKind::Move, 0.0), (PointerKind::Down, 0.0), (PointerKind::Drag, dy / 2.0), (PointerKind::Drag, dy)]
        .map(|(kind, y)| PointerEvent { kind, pos: c + vec2(0.0, y) });
    d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
}

/// Presses widget `id` at `fy` of its height and keeps it held.
fn press(d: &mut Driver, id: &str, fy: f32) {
    let w = widget(d, id);
    let pos = Pos2::new(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] * fy);
    let events = [PointerEvent { kind: PointerKind::Move, pos }, PointerEvent { kind: PointerKind::Down, pos }];
    d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
}

/// Releases the button where the pointer is.
fn release(d: &mut Driver) {
    let pos = d.pointer_pos().unwrap();
    d.pointer(&[PointerEvent { kind: PointerKind::Up, pos }], PointerButton::Primary, Modifiers::NONE).unwrap();
}

/// Writes the last frame to `target/test-output/ui/<name>.png`, to look at.
fn save(d: &Driver, name: &str) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// The on-screen top of `section`'s header block in the last frame, if drawn.
fn header_top(d: &Driver, section: Section) -> Option<f32> {
    info(d).headers.iter().find(|(s, _)| *s == section).map(|(_, r)| r.min.y)
}

/// The content offset the scroll anchor asks for.
fn wanted(d: &Driver) -> f64 {
    layout(d).offset_of(&d.state().scroll).unwrap()
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn the_speed_is_a_pure_function_of_the_distance() {
    let q = ThumbSpeed::default();
    assert_eq!((q.exponent, q.top_speed), (2.0, 40.0), "quadratic, 40 screen heights per second");
    let reach = 284.0;
    // Nothing inside the dead zone, either way.
    for d in [0.0, 1.0, -2.5, DEAD_ZONE, -DEAD_ZONE] {
        assert_eq!(scrollbar::speed(d, reach, &q), 0.0, "{d}");
    }
    // Quadratic in the share of the way from the dead zone to the track's end.
    for (u, want) in [(0.1, 0.4), (0.25, 2.5), (0.5, 10.0), (0.75, 22.5), (1.0, 40.0)] {
        let d = DEAD_ZONE + u as f32 * (reach - DEAD_ZONE);
        assert!(close(scrollbar::speed(d, reach, &q), want, 1e-3), "u {u}: {}", scrollbar::speed(d, reach, &q));
        assert!(close(scrollbar::speed(-d, reach, &q), -want, 1e-3), "up, u {u}");
    }
    // The top speed at the track's end and no faster beyond it; faster the further out.
    assert_eq!(scrollbar::speed(reach + 500.0, reach, &q), 40.0);
    let samples: Vec<f64> = (5..=284).map(|d| scrollbar::speed(d as f32, reach, &q)).collect();
    assert!(samples.windows(2).all(|w| w[1] > w[0]), "strictly faster further out");
    // Another curve: linear, 10 at the end.
    let linear = ThumbSpeed { exponent: 1.0, top_speed: 10.0 };
    assert!(close(scrollbar::speed(DEAD_ZONE + 0.5 * (reach - DEAD_ZONE), reach, &linear), 5.0, 1e-4));
}

#[test]
fn holding_the_thumb_scrolls_at_the_speed_of_its_distance() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let (r, h) = (reach(&d), f64::from(view_h(&d)));
    let q = ThumbSpeed::default();
    assert!(layout(&d).max_offset(view_h(&d)) > 25.0 * h, "tall enough for half a second at the top speed");
    for (dy, label) in [(3.0, "dead zone"), (r / 3.0, "a third"), (r * 2.0 / 3.0, "two thirds"), (r, "the end"), (r + 300.0, "beyond the end")] {
        hold_thumb(&mut d, dy);
        let want = scrollbar::speed(dy.min(r), r, &q) * h;
        assert!(close(bar(&d).speed(), want, want * 1e-5), "{label}: {} pt/s, want {want}", bar(&d).speed());
        let from = offset(&d);
        d.wait(500).unwrap();
        let moved = offset(&d) - from;
        assert!(close(moved, want * 0.5, 1.0), "{label}: moved {moved} in 0.5 s, want {}", want * 0.5);
        if dy <= DEAD_ZONE {
            assert_eq!(moved, 0.0, "nothing inside the dead zone");
        }
        release(&mut d);
        d.run("scroll.top", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(offset(&d), 0.0);
    }
    // The top speed is 40 screen heights per second; pulled up, the view goes up as fast.
    assert!(close(scrollbar::speed(r, r, &q) * h, 40.0 * h, 1e-6));
    d.run("scroll.bottom", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let bottom = offset(&d);
    hold_thumb(&mut d, -r);
    let from = offset(&d);
    d.wait(250).unwrap();
    assert!(close(from - offset(&d), 40.0 * h * 0.25, 1.0), "up at the top speed: {}", from - offset(&d));
    assert!(offset(&d) < bottom);
    release(&mut d);
}

#[test]
fn the_thumb_curve_is_read_from_the_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let curve = ThumbSpeed { exponent: 1.0, top_speed: 10.0 };
    d.app_mut().session_mut().unwrap().change_settings(|s| s.set_thumb_speed(&curve));
    let (r, h) = (reach(&d), f64::from(view_h(&d)));
    hold_thumb(&mut d, r / 2.0);
    let want = 10.0 * f64::from((r / 2.0 - DEAD_ZONE) / (r - DEAD_ZONE)) * h;
    assert!(close(bar(&d).speed(), want, want * 1e-5), "linear: {} pt/s, want {want}", bar(&d).speed());
    let from = offset(&d);
    d.wait(500).unwrap();
    assert!(close(offset(&d) - from, want * 0.5, 1.0));
    release(&mut d);
}

#[test]
fn a_release_stops_the_view_and_the_thumb_springs_back_in_150_ms() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let track = widget(&d, "scroll:track");
    let rest = widget(&d, "scroll:thumb");
    assert_eq!(center(&rest).y, center(&track).y, "at rest the thumb is at the centre of the track");
    let r = reach(&d);
    hold_thumb(&mut d, r / 2.0);
    d.wait(200).unwrap();
    assert_eq!(widget(&d, "scroll:thumb").rect[1], rest.rect[1] + r / 2.0, "the thumb follows the pointer while held");
    save(&d, "scrollbar-thumb-held-1400x900");
    assert!(offset(&d) > 0.0);

    // The frame of the release: the speed is 0 and the view stops there.
    release(&mut d);
    let stopped = offset(&d);
    assert_eq!(bar(&d).speed(), 0.0);
    let after_release = bar(&d).displacement();
    assert!(after_release > 0.0 && after_release < r / 2.0, "springing back: {after_release}");
    d.wait(100).unwrap();
    assert!(bar(&d).displacement() > 0.0, "not back yet at ~133 ms");
    assert_eq!(offset(&d), stopped, "the view does not drift");
    // 150 ms after the release (the release frame plus nine more at 60 fps).
    d.wait(17).unwrap();
    const { assert!(SPRING_SECONDS <= 10.0 / 60.0) };
    assert_eq!(bar(&d).displacement(), 0.0);
    assert_eq!(widget(&d, "scroll:thumb").rect, rest.rect, "back at the centre");
    assert_eq!(offset(&d), stopped);
    d.settle(FRAMES).unwrap();
    assert_eq!(offset(&d), stopped, "nothing moves once settled");
}

#[test]
fn prev_and_next_section_land_headers_at_the_top() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let l = layout(&d);
    let order: Vec<Section> = (0..l.len()).filter_map(|i| l.section(i)).collect();
    let max = l.max_offset(view_h(&d));
    assert!(l.top(4).unwrap() < max, "the first sections are reachable");
    let area_top = info(&d).area.unwrap().min.y;

    // ⇓ by the button: the next header at the top, eased in 0.2 s.
    d.click("scroll:nextSection").unwrap();
    assert!(info(&d).animating);
    d.wait((SCROLL_SECONDS * 1000.0) as u64).unwrap();
    assert_eq!(header_top(&d, order[1]), Some(area_top));
    // Two presses while the view eases add up.
    d.click("scroll:nextSection").unwrap();
    d.click("scroll:nextSection").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[3]), Some(area_top));
    // Ctrl+↓ does the same.
    d.key("Ctrl+Down").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[4]), Some(area_top));

    // Mid-section, ⇑ first snaps to the section's start, then goes to the one before.
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    d.scroll(Some(&cell.id), vec2(0.0, -150.0)).unwrap();
    assert_ne!(header_top(&d, order[4]), Some(area_top));
    d.click("scroll:prevSection").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[4]), Some(area_top), "snapped to the current section's start");
    d.click("scroll:prevSection").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[3]), Some(area_top));
    d.key("Ctrl+Up").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(header_top(&d, order[2]), Some(area_top));
    // From the first section, ⇑ goes to the top of the library and stays there.
    for _ in 0..3 {
        d.key("Ctrl+Up").unwrap();
    }
    d.settle(FRAMES).unwrap();
    assert_eq!(offset(&d), 0.0);
    assert_eq!(header_top(&d, order[0]), Some(area_top));
}

#[test]
fn the_buttons_repeat_while_held() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let row = layout(&d).row_pitch();
    // ▼ acts on the press: one row.
    press(&mut d, "scroll:down", 0.5);
    assert!(close(wanted(&d), row, 0.01), "one row on the press: {}", wanted(&d));
    // Nothing more before the repeat delay (0.4 s)…
    d.wait(300).unwrap();
    assert!(close(wanted(&d), row, 0.01), "no repeat before 0.4 s");
    // …then a row every 0.06 s while held.
    d.wait(700).unwrap();
    let rows = wanted(&d) / row;
    assert!((10.0..=14.0).contains(&rows), "after 1 s held: {rows} rows");
    release(&mut d);
    let held = wanted(&d);
    d.wait(500).unwrap();
    assert_eq!(wanted(&d), held, "no repeat after the release");
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), held, 1.0), "the view arrives where the presses asked");
    // ▲ moves back the same way.
    press(&mut d, "scroll:up", 0.5);
    assert!(close(wanted(&d), held - row, 0.01));
    release(&mut d);
    // ⇓ repeats too.
    press(&mut d, "scroll:nextSection", 0.5);
    d.wait(500).unwrap();
    release(&mut d);
    d.settle(FRAMES).unwrap();
    let l = layout(&d);
    let index = l.index_of(d.state().scroll.section.unwrap()).unwrap();
    assert!(index >= 3, "⇓ repeated: section {index}");
}

#[test]
fn the_track_pages_by_one_row_plus_90_towards_the_press() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let page = layout(&d).row_pitch() + PAGE_EXTRA;
    // Below the thumb: a page down, animated.
    d.click_with("scroll:track", &dalica_ui::headless::Click { fy: 0.9, ..Default::default() }).unwrap();
    assert!(close(wanted(&d), page, 0.01), "{} vs {page}", wanted(&d));
    assert!(info(&d).animating && offset(&d) < page);
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), page, 1.0));
    // Clicks add up, even while the view eases.
    for _ in 0..3 {
        d.click_with("scroll:track", &dalica_ui::headless::Click { fy: 0.9, ..Default::default() }).unwrap();
    }
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), 4.0 * page, 1.0), "{}", offset(&d));
    // Above the thumb: a page up.
    d.click_with("scroll:track", &dalica_ui::headless::Click { fy: 0.1, ..Default::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), 3.0 * page, 1.0));
}

#[test]
fn the_marker_tracks_the_position() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let track = widget(&d, "scroll:track").rect;
    let marker = || bar(&d).marker_rect().unwrap();
    let m = marker();
    assert_eq!((m.min.y, m.height(), m.width()), (track[1], 3.0, 16.0), "at the top of the track at the top");
    let max = layout(&d).max_offset(view_h(&d));
    for target in [0.25, 0.5, 0.8] {
        let to = (max * target).round();
        d.app_mut().state_mut().scroll = layout(&d).anchor_at(to);
        d.settle(FRAMES).unwrap();
        let y = track[1] + (offset(&d) / max) as f32 * (track[3] - 3.0);
        assert!((bar(&d).marker_rect().unwrap().min.y - y).abs() < 0.01, "at {target}: {:?} vs {y}", bar(&d).marker_rect());
    }
    d.key("Ctrl+End").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(bar(&d).marker_rect().unwrap().max.y, track[1] + track[3], "at the bottom of the track at the end");
    // Speed scrolling moves it down while the thumb stays put.
    d.key("Ctrl+Home").unwrap();
    d.settle(FRAMES).unwrap();
    let before = bar(&d).marker_rect().unwrap().min.y;
    let r = reach(&d);
    hold_thumb(&mut d, r);
    d.wait(100).unwrap();
    assert!(bar(&d).marker_rect().unwrap().min.y > before);
    release(&mut d);
}

#[test]
fn the_scroll_keys_page_and_jump_to_the_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 4000, 40, (1400, 900));
    let l = layout(&d);
    let page = f64::from(view_h(&d)) - l.row_pitch();
    d.key("PageDown").unwrap();
    assert!(close(wanted(&d), page, 0.01), "a page is the view less one row: {}", wanted(&d));
    d.key("PageDown").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), 2.0 * page, 1.0));
    d.key("PageUp").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(close(offset(&d), page, 1.0));
    d.key("Ctrl+End").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(offset(&d), l.max_offset(view_h(&d)).round());
    d.key("PageDown").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(offset(&d), l.max_offset(view_h(&d)).round(), "no further than the end");
    d.key("Ctrl+Home").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(offset(&d), 0.0);
    // The commands answer the anchor they asked for.
    let r = d.run("scroll.nextSection", &json!({})).unwrap();
    assert_eq!(r, json!({ "scroll": { "section": l.section(1).unwrap().to_string(), "offset": 0.0 } }));
}

#[test]
fn the_keymap_has_no_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let d = driver(dir.path(), 200, 4, (900, 600));
    let keymap = d.app().session().unwrap().keymap();
    assert_eq!(keymap.conflicts(), vec![], "no two bindings share a key");
    for (chord, id) in [
        ("PageUp", "scroll.pageUp"),
        ("PageDown", "scroll.pageDown"),
        ("Ctrl+Up", "scroll.prevSection"),
        ("Ctrl+Down", "scroll.nextSection"),
        ("Ctrl+Home", "scroll.top"),
        ("Ctrl+End", "scroll.bottom"),
    ] {
        assert_eq!(keymap.resolve(&Key::parse(chord).unwrap(), Context::Library, false).as_deref(), Some(id), "{chord}");
    }
}

#[test]
fn the_scroll_commands_fail_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 400, 8, (900, 600));
    d.run("scroll.nextSection", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let before = d.state().scroll;
    for id in SCROLL_COMMANDS {
        for bad in [json!({ "a": 1 }), json!([]), json!("x"), json!(0), json!({ "offset": 3 })] {
            let e = d.app_mut().run(id, &bad).unwrap_err();
            assert!(matches!(e, CmdError::BadParams(_)), "{id} {bad}: {e:?}");
            d.step();
            assert_eq!(d.state().scroll, before, "{id} {bad}: nothing changed");
        }
        // `null` is `{}`.
        assert!(d.app_mut().run(id, &json!(null)).is_ok(), "{id}");
        d.app_mut().state_mut().scroll = before;
        d.settle(FRAMES).unwrap();
    }
    // Without a library, and with nothing to scroll: refused, never a panic.
    let mut opening = App::new(None);
    let dir = tempfile::tempdir().unwrap();
    let mut empty = Driver::new(App::new(Some(Session::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap())), 900, 600, 1.0);
    empty.settle(FRAMES).unwrap();
    for id in SCROLL_COMMANDS {
        assert!(matches!(opening.run(id, &json!({})), Err(CmdError::Disabled(_))), "{id}");
        assert!(matches!(empty.app_mut().run(id, &json!({})), Err(CmdError::Disabled(_))), "{id}");
    }
}

#[test]
fn a_library_that_fits_shows_a_disabled_bar() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), 3, 1, (1400, 900));
    for id in ["scroll:prevSection", "scroll:up", "scroll:track", "scroll:thumb", "scroll:down", "scroll:nextSection"] {
        assert!(!widget(&d, id).enabled, "{id}");
    }
    let before = offset(&d);
    let c = center(&widget(&d, "scroll:thumb"));
    let events = [(PointerKind::Move, 0.0), (PointerKind::Down, 0.0), (PointerKind::Drag, 200.0)]
        .map(|(kind, y)| PointerEvent { kind, pos: c + vec2(0.0, y) });
    d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    d.wait(200).unwrap();
    release(&mut d);
    assert_eq!((offset(&d), bar(&d).displacement()), (before, 0.0));
}
