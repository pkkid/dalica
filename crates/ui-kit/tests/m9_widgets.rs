//! M9's widgets (`design-system.md` §6, `ui-layout.md` §4, §5.5 and §9, `organizing.md` §8.2):
//!
//! - the filter banner: at the light table's width in a 1400 × 900 and a 900 × 600 window, a text
//!   too long for it is cut from its end with "…" short of the right edge while [Show All] stays
//!   whole; a text that fits is shown whole; a click on Show All is reported;
//! - the drag pill: placed next to the pointer and inside the window at every corner and beyond
//!   it, at both window sizes; its text per operation; the + only for a copy;
//! - the age slider: the map from the handle's place to days is monotonic, 0 means every photo,
//!   the labels read "Pictures up to 1 day / 2 weeks / 3 months / 5 years old", snapping holds;
//!   a drag and the arrow keys set it;
//! - the drop-target highlight and the insertion marker paint their tokens;
//! - the sheet (`fingerprints/m9-widgets.json`: the age slider in every state, the banner at three
//!   widths, the pill per operation, the highlight and the marker) and the gallery: a window with
//!   the Albums group's automatic albums, a drop target, the banner, the age slider, an insertion
//!   marker and the pill, at 1400 × 900 (`fingerprints/gallery-m9-1400x900.json`) and 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check, out_dir};
use dalica_ui_kit::icons::{self, AlbumIcon, paint_album};
use dalica_ui_kit::widgets::{
    AGE_MAX_DAYS, BANNER_H, BANNER_PAD, BANNER_TEXT_GAP, BannerOut, DragOp, DragPill, PILL_H, PILL_MARGIN, PILL_OFFSET, PillThumb, age_days,
    age_label, age_position, age_slider, age_snapped, drag_pill, drag_pill_rect, drag_pill_text, drop_target, filter_banner, insertion_marker,
    text_button,
};
use dalica_ui_kit::{Icon, Image, Role, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Color32, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];
const SHORT: &str = "Showing 33 pictures in 4 folders (0.01 s) · 439.8 MB";
const LONG: &str = "Showing 1,204 pictures in 37 folders and 5 albums (0.42 s) · 12.6 GB · matching \u{201c}lisbon harbour at dusk \
                    with the yellow tram, the castle walls and the river ferries\u{201d} in every folder of the library";

fn check_ids(what: &str, widgets: &[WidgetInfo], unregistered: &[registry::Unregistered]) {
    assert_eq!(unregistered, [], "{what}: interactive widgets without an id");
    let mut seen = std::collections::HashSet::new();
    for w in widgets {
        assert!(registry::is_valid_id(&w.id), "{what}: {:?} breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{what}: {} registered twice", w.id);
    }
}

fn area(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
}

fn row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), add);
}

/// A scene drawn each frame with real input.
struct Rig<S> {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    state: S,
    scene: fn(&mut Ui, &Tokens, &mut S),
    widgets: Vec<WidgetInfo>,
    /// The last frame, painted only when a test asks for its pixels.
    canvas: Canvas,
}

impl<S> Rig<S> {
    fn new(size: (u32, u32), state: S, scene: fn(&mut Ui, &Tokens, &mut S)) -> Rig<S> {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig { ctx, t, size, clock: 0.0, state, scene, widgets: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let (t, scene) = (self.t, self.scene);
        let state = &mut self.state;
        let out = self.ctx.run_ui(raw, |ui| scene(ui, &t, state));
        self.widgets = registry::widgets(&self.ctx);
        self.canvas.keep(out);
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame(vec![]);
        }
    }

    fn info(&self, id: &str) -> WidgetInfo {
        self.widgets
            .iter()
            .rev()
            .find(|w| w.id == id)
            .unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>()))
            .clone()
    }

    fn rect(&self, id: &str) -> Rect {
        let r = self.info(id).rect;
        Rect::from_min_size(pos2(r[0], r[1]), vec2(r[2], r[3]))
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(from), Self::press(from, true)]);
        for i in 1..=4 {
            self.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.0))]);
        }
        self.frame(vec![Self::press(to, false)]);
        self.settle();
    }

    fn hover_at(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        self.settle();
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true), ev(false)]);
        self.settle();
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }
}

// ---- The filter banner ----

/// The light table's left edge and width in a window `w` wide: right of the 240 pt tree and the
/// 8 pt splitter, left of the 280 pt side pane when the window is wide enough for one.
fn light_table(t: &Tokens, w: f32) -> (f32, f32) {
    let left = t.metrics.tree_w + t.metrics.splitter_w;
    let pane = if w >= 1400.0 { t.metrics.side_pane_w } else { 0.0 };
    (left, w - left - pane)
}

struct BannerScene {
    text: &'static str,
    left: f32,
    width: f32,
    out: Option<BannerOut>,
    clicks: u32,
}

fn banner_scene(ui: &mut Ui, t: &Tokens, s: &mut BannerScene) {
    let rect = Rect::from_min_size(pos2(s.left, 40.0), vec2(s.width, BANNER_H));
    let mut out = None;
    area(ui, rect, |ui| out = Some(filter_banner(ui, t, "banner:search", "button:search.showAll", s.text)));
    if let Some(o) = &out
        && o.show_all.clicked()
    {
        s.clicks += 1;
    }
    s.out = out;
}

/// Show All's natural width: a `text_button` labelled "Show All" on its own.
fn show_all_width(t: &Tokens) -> f32 {
    let ctx = Context::default();
    t.apply(&ctx);
    let mut w = 0.0;
    for _ in 0..2 {
        ctx.run_ui(dalica_ui_kit::raw_input(400, 100, 1.0), |ui| w = text_button(ui, t, "button:alone", "Show All", false).rect.width())
            .textures_delta
            .clear();
    }
    w
}

#[test]
fn the_banner_cuts_its_text_from_the_end_and_keeps_show_all_whole_at_both_window_sizes() {
    let t = Tokens::default();
    let whole = show_all_width(&t);
    assert!(whole > 37.0, "{whole}");
    for size in WINDOWS {
        let (left, width) = light_table(&t, size.0 as f32);
        for (text, should_elide) in [(LONG, true), (SHORT, false)] {
            let mut rig = Rig::new(size, BannerScene { text, left, width, out: None, clicks: 0 }, banner_scene);
            let out = rig.state.out.as_ref().unwrap();
            let what = format!("{size:?} {}", if should_elide { "long" } else { "short" });
            assert_eq!(out.rect, Rect::from_min_size(pos2(left, 40.0), vec2(width, BANNER_H)), "{what}");
            // Show All: whole, at the left, centred on the strip.
            let button = out.show_all.rect;
            assert_eq!(button.width(), whole, "{what}: Show All is cut");
            assert_eq!(button.min.x, left + BANNER_PAD, "{what}");
            assert!((button.center().y - out.rect.center().y).abs() <= 0.5, "{what}: {button:?}");
            // The text after it, never past the right edge less the gap.
            assert!(out.text_rect.min.x >= button.max.x + BANNER_TEXT_GAP - 0.5, "{what}: {:?}", out.text_rect);
            assert!(out.text_rect.max.x <= out.rect.max.x - BANNER_TEXT_GAP + 0.5, "{what}: {:?} runs past the edge", out.text_rect);
            assert_eq!(out.elided, should_elide, "{what}: {:?}", out.shown);
            if should_elide {
                let kept = out.shown.strip_suffix('…').unwrap_or_else(|| panic!("{what}: {:?} does not end with …", out.shown));
                assert!(kept.len() > 20 && text.starts_with(kept.trim_end()), "{what}: {:?} is not the start of the text", out.shown);
            } else {
                assert_eq!(out.shown, text, "{what}");
            }
            // The ids: the strip labelled with the whole text, and Show All.
            assert_eq!(rig.info("banner:search").label, text);
            assert_eq!(rig.info("banner:search").role, Role::Other);
            assert_eq!(rig.info("button:search.showAll").role, Role::Button);
            check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
            // A click on Show All is reported once.
            let at = button.center();
            rig.click_at(at);
            assert_eq!(rig.state.clicks, 1, "{what}");
        }
    }
    // A banner narrower than Show All still draws it whole (clipped by the caller), with no text.
    let mut rig = Rig::new((400, 200), BannerScene { text: LONG, left: 10.0, width: 40.0, out: None, clicks: 0 }, banner_scene);
    let out = rig.state.out.take().unwrap();
    assert_eq!(out.show_all.rect.width(), whole);
    assert!(out.elided && out.shown.is_empty(), "{:?}", out.shown);
}

// ---- The drag pill ----

#[test]
fn the_pill_says_what_a_drop_would_do() {
    assert_eq!(drag_pill_text(DragOp::Move, 3, None), "Move 3 photos");
    assert_eq!(drag_pill_text(DragOp::Move, 3, Some("Garden")), "Move 3 photos to Garden");
    assert_eq!(drag_pill_text(DragOp::Copy, 3, None), "Copy 3 photos");
    assert_eq!(drag_pill_text(DragOp::Copy, 1, Some("Garden")), "Copy 1 photo to Garden");
    assert_eq!(drag_pill_text(DragOp::AddToAlbum, 3, None), "Add 3 photos to album");
    assert_eq!(drag_pill_text(DragOp::AddToAlbum, 3, Some("Holidays")), "Add 3 photos to Holidays");
    assert_eq!(drag_pill_text(DragOp::Hold, 2, None), "Hold 2 photos in the tray");
    assert_eq!(drag_pill_text(DragOp::Reorder, 1, None), "Reorder 1 photo");
    assert_eq!(drag_pill_text(DragOp::None, 1204, Some("ignored")), "1,204 photos");
    assert_eq!(drag_pill_text(DragOp::Move, 2, Some("")), "Move 2 photos");
}

#[test]
fn the_pill_stays_inside_the_window_at_every_corner() {
    for size in WINDOWS {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        let room = window.shrink(PILL_MARGIN);
        let pill = vec2(220.0, PILL_H);
        let (w, h) = (window.width(), window.height());
        let pointers = [
            pos2(0.0, 0.0),
            pos2(w, 0.0),
            pos2(0.0, h),
            pos2(w, h),
            pos2(w / 2.0, h / 2.0),
            pos2(w - 10.0, h / 2.0),
            pos2(w / 2.0, h - 5.0),
            pos2(-80.0, -80.0),
            pos2(w + 80.0, h + 80.0),
            pos2(-80.0, h + 80.0),
            pos2(f32::NAN, 10.0),
            pos2(f32::INFINITY, f32::NEG_INFINITY),
        ];
        for p in pointers {
            let r = drag_pill_rect(p, pill, window);
            assert!(room.expand(0.01).contains_rect(r), "{size:?} pointer {p:?}: {r:?} leaves {room:?}");
            assert_eq!(r.size(), pill, "{size:?} {p:?}");
        }
        // In the open it sits right of and below the pointer; at the right edge on its left; at the
        // bottom above it; never under the pointer itself.
        let mid = pos2(w / 2.0, h / 2.0);
        assert_eq!(drag_pill_rect(mid, pill, window).min, mid + PILL_OFFSET);
        let right = pos2(w - 10.0, h / 2.0);
        assert_eq!(drag_pill_rect(right, pill, window).max.x, right.x - PILL_OFFSET.x);
        let bottom = pos2(w / 2.0, h - 5.0);
        assert_eq!(drag_pill_rect(bottom, pill, window).max.y, bottom.y - PILL_OFFSET.y);
        for p in [pos2(w, h), pos2(0.0, 0.0), pos2(w, 0.0), pos2(0.0, h)] {
            assert!(!drag_pill_rect(p, pill, window).contains(p), "{size:?}: the pill covers the pointer at {p:?}");
        }
        // A pill wider than the window shrinks to fit it.
        let wide = drag_pill_rect(mid, vec2(w * 2.0, PILL_H), window);
        assert_eq!(wide.width(), room.width());
    }
    assert_eq!(drag_pill_rect(pos2(5.0, 5.0), vec2(100.0, 32.0), Rect::NOTHING), Rect::NOTHING);
}

struct PillScene {
    pointer: Pos2,
    op: DragOp,
    rect: Rect,
}

fn pill_scene(ui: &mut Ui, t: &Tokens, s: &mut PillScene) {
    let pill = DragPill { op: s.op, count: 3, target: Some("Garden"), thumb: None };
    s.rect = drag_pill(ui.ctx(), t, "drag:pill", s.pointer, &pill);
}

/// Pixels close to `accent` inside `rect`.
fn accent_pixels<S>(rig: &mut Rig<S>, rect: Rect) -> usize {
    let a = rig.t.accent.to_array();
    let mut n = 0;
    for y in rect.min.y as u32..rect.max.y as u32 {
        for x in rect.min.x as u32..rect.max.x as u32 {
            let px = rig.pixel(pos2(x as f32, y as f32));
            if (0..3).all(|c| px[c].abs_diff(a[c]) <= 8) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn the_painted_pill_stays_inside_and_shows_a_plus_only_for_a_copy() {
    for size in WINDOWS {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let window = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
        for pointer in [pos2(1.0, 1.0), pos2(w - 1.0, 1.0), pos2(1.0, h - 1.0), pos2(w - 1.0, h - 1.0), pos2(w / 2.0, h / 2.0)] {
            for op in [DragOp::Move, DragOp::Copy, DragOp::AddToAlbum] {
                let mut rig = Rig::new(size, PillScene { pointer, op, rect: Rect::NOTHING }, pill_scene);
                let what = format!("{size:?} {pointer:?} {op:?}");
                let rect = rig.state.rect;
                assert!(window.shrink(PILL_MARGIN).expand(0.01).contains_rect(rect), "{what}: {rect:?}");
                let info = rig.info("drag:pill");
                assert_eq!(info.label, drag_pill_text(op, 3, Some("Garden")), "{what}");
                assert_eq!(info.rect, [rect.min.x, rect.min.y, rect.width(), rect.height()], "{what}");
                let plus = accent_pixels(&mut rig, rect);
                if op == DragOp::Copy {
                    assert!(plus >= 40, "{what}: the + disc has {plus} accent pixels");
                } else {
                    assert_eq!(plus, 0, "{what}: a + on a {op:?}");
                }
                check_ids(&what, &rig.widgets, &registry::unregistered(&rig.ctx));
            }
        }
    }
}

// ---- The age slider ----

#[test]
fn the_age_map_is_monotonic_and_zero_means_everything() {
    assert_eq!(age_days(0.0), None);
    assert_eq!(age_days(-1.0), None);
    assert_eq!(age_days(f32::NAN), None);
    assert_eq!(age_days(1.0), Some(1));
    assert_eq!(age_days(f32::INFINITY), Some(1));
    assert_eq!(age_days(1e-6), Some(AGE_MAX_DAYS), "just right of 0 is 20 years");
    assert_eq!(age_position(None), 0.0);
    assert_eq!(age_position(Some(1)), 1.0);
    // Never a larger age for a place further right ("everything" counts as the largest).
    let mut last = u32::MAX;
    for i in 0..=10_000 {
        let days = age_days(i as f32 / 10_000.0).unwrap_or(u32::MAX);
        assert!(days <= last, "position {}: {days} after {last}", i as f32 / 10_000.0);
        last = days;
    }
    // Snapping: idempotent, monotonic, within the range, and the label's own value.
    let mut last = 0;
    for d in 0..=AGE_MAX_DAYS + 10 {
        let s = age_snapped(d);
        assert!((1..=AGE_MAX_DAYS).contains(&s), "{d} → {s}");
        assert_eq!(age_snapped(s), s, "{d} → {s} is not a fixed point");
        assert!(s >= last, "{d} → {s} after {last}");
        assert_eq!(age_label(Some(s)), age_label(Some(d)), "{d} and its snapped {s} read differently");
        // The handle's place of a snapped age gives that age back.
        assert_eq!(age_days(age_position(Some(s))), Some(s), "{s}");
        last = s;
    }
    // Places further right never show older pictures.
    let mut last = 0.0;
    for d in (1..=AGE_MAX_DAYS).rev() {
        let p = age_position(Some(d));
        assert!(p >= last && p > 0.0 && p <= 1.0, "{d}: {p} after {last}");
        last = p;
    }
}

#[test]
fn the_age_labels_read_as_specified() {
    assert_eq!(age_label(Some(1)), "Pictures up to 1 day old");
    assert_eq!(age_label(Some(14)), "Pictures up to 2 weeks old");
    assert_eq!(age_label(Some(91)), "Pictures up to 3 months old");
    assert_eq!(age_label(Some(1826)), "Pictures up to 5 years old");
    assert_eq!(age_label(Some(3)), "Pictures up to 3 days old");
    assert_eq!(age_label(Some(7)), "Pictures up to 1 week old");
    assert_eq!(age_label(Some(30)), "Pictures up to 1 month old");
    assert_eq!(age_label(Some(365)), "Pictures up to 1 year old");
    assert_eq!(age_label(Some(AGE_MAX_DAYS)), "Pictures up to 20 years old");
    assert_eq!(age_label(Some(0)), "Pictures up to 1 day old");
    assert_eq!(age_label(Some(u32::MAX)), "Pictures up to 20 years old");
    assert_eq!(age_label(None), "All pictures");
    // The four ages of the spec are slider values: the handle's place for each gives it back.
    for (d, label) in [(1, "1 day"), (14, "2 weeks"), (91, "3 months"), (1826, "5 years")] {
        let back = age_days(age_position(Some(d)));
        assert_eq!(back, Some(d), "{label}");
        assert_eq!(age_label(back), format!("Pictures up to {label} old"));
    }
}

struct AgeScene {
    days: Option<u32>,
    changes: u32,
}

fn age_scene(ui: &mut Ui, t: &Tokens, s: &mut AgeScene) {
    row(ui, Rect::from_min_size(pos2(40.0, 40.0), vec2(200.0, 20.0)), |ui| {
        if age_slider(ui, t, "slider:age", &mut s.days, 100.0, "Show only recent photos").changed() {
            s.changes += 1;
        }
    });
}

#[test]
fn a_drag_and_the_arrow_keys_set_the_age() {
    let mut rig = Rig::new((300, 120), AgeScene { days: None, changes: 0 }, age_scene);
    let r = rig.rect("slider:age");
    assert_eq!(r.size(), vec2(100.0, 20.0));
    assert_eq!(rig.info("slider:age").label, "Show only recent photos");
    assert!(!rig.info("slider:age").active);
    // Dragged all the way right: one day.
    rig.drag(pos2(r.min.x + 2.0, r.center().y), pos2(r.max.x + 30.0, r.center().y));
    assert_eq!(rig.state.days, Some(1));
    assert_eq!(rig.info("slider:age").label, "Pictures up to 1 day old");
    assert!(rig.info("slider:age").active);
    // Back to the left end: everything.
    rig.drag(pos2(r.max.x - 2.0, r.center().y), pos2(r.min.x - 30.0, r.center().y));
    assert_eq!(rig.state.days, None);
    // A click in the middle: an age between, already on its unit.
    rig.click_at(r.center());
    rig.settle();
    let mid = rig.state.days.unwrap();
    assert!(mid > 14 && mid < 730, "{mid}");
    assert_eq!(age_snapped(mid), mid);
    // → with the focus moves right (newer only), ← back.
    rig.ctx.memory_mut(|m| m.request_focus(egui::Id::new("slider:age")));
    rig.settle();
    let changes = rig.state.changes;
    rig.key(Key::ArrowRight);
    let newer = rig.state.days.unwrap();
    assert!(newer < mid, "{newer} after {mid}");
    rig.key(Key::ArrowLeft);
    rig.key(Key::ArrowLeft);
    assert!(rig.state.days.unwrap() > mid);
    assert_eq!(rig.state.changes, changes + 3);
    // The thumb is drawn at the age's place: a light pixel there, the track elsewhere.
    rig.state.days = Some(91);
    rig.hover_at(pos2(5.0, 5.0));
    let inset = rig.t.metrics.zoom_thumb.x / 2.0 + 1.0;
    let x = r.min.x + inset + age_position(Some(91)) * (r.width() - 2.0 * inset);
    let px = rig.pixel(pos2(x.round(), r.center().y + 4.0));
    assert!(px[0] > 180, "no thumb at {x}: {px:?}");
    check_ids("age", &rig.widgets, &registry::unregistered(&rig.ctx));
}

// ---- The drop target and the insertion marker ----

#[test]
fn the_drop_target_and_the_marker_paint_their_tokens() {
    let t = Tokens::default();
    let img = dalica_ui_kit::snapshot(200, 100, 1.0, &t, |ui| {
        let p = ui.painter();
        drop_target(p, &t, Rect::from_min_size(pos2(10.0, 10.0), vec2(120.0, 24.0)));
        insertion_marker(p, &t, pos2(160.0, 10.0), 60.0);
        // Degenerate input paints nothing and never panics.
        drop_target(p, &t, Rect::NOTHING);
        drop_target(p, &t, Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(5.0, 5.0)));
        insertion_marker(p, &t, pos2(f32::NAN, 0.0), 10.0);
        insertion_marker(p, &t, pos2(190.0, 0.0), -5.0);
        insertion_marker(p, &t, pos2(190.0, 0.0), f32::INFINITY);
    })
    .unwrap();
    let px = |x, y| img.pixel(x, y).unwrap();
    // The outline is accent; the inside is the chrome tinted by the translucent fill.
    let edge = px(60, 10);
    assert!(edge[2] > 150 && edge[0] < 60, "outline {edge:?}");
    let inside = px(60, 22);
    let chrome = t.chrome.to_array();
    assert!(inside[2] > chrome[2] && inside[0] < chrome[0] + 2, "fill {inside:?} over {chrome:?}");
    // The marker: the bar and both caps in accent, nothing beside the bar between them.
    let a = t.accent.to_array();
    for (x, y) in [(160, 40), (157, 10), (163, 10), (157, 69), (163, 69)] {
        assert_eq!(px(x, y)[..3], a[..3], "({x}, {y})");
    }
    assert_eq!(px(165, 40), chrome, "beside the bar");
    assert_eq!(px(190, 2), chrome, "a bad marker painted");
}

// ---- The sheet ----

/// Every state of the age slider, the banner at three widths, the pill per operation, the
/// highlight and the marker, in one frame.
fn sheet(ui: &mut Ui, t: &Tokens, _: &mut ()) {
    let p = ui.painter().clone();
    let label = |y: f32, s: &str| {
        p.text(pos2(12.0, y), Align2::LEFT_TOP, s, fonts::semibold(t.small), t.text_label);
    };
    label(8.0, "age_slider: everything · 3 months · 1 day · disabled");
    for (i, days) in [None, Some(91), Some(1)].into_iter().enumerate() {
        let mut d = days;
        row(ui, Rect::from_min_size(pos2(12.0 + 120.0 * i as f32, 26.0), vec2(110.0, 20.0)), |ui| {
            age_slider(ui, t, &format!("slider:sheet.age{i}"), &mut d, 100.0, "Show only recent photos");
        });
    }
    let mut d = Some(91);
    row(ui, Rect::from_min_size(pos2(372.0, 26.0), vec2(110.0, 20.0)), |ui| {
        ui.add_enabled_ui(false, |ui| age_slider(ui, t, "slider:sheet.ageOff", &mut d, 100.0, "Show only recent photos"));
    });
    for (i, days) in [Some(1), Some(14), Some(91), Some(1826)].into_iter().enumerate() {
        p.text(pos2(12.0 + 150.0 * i as f32, 52.0), Align2::LEFT_TOP, age_label(days), fonts::regular(t.small), t.text_dim);
    }
    label(76.0, "filter_banner: 600 · 380 · 140");
    for (i, w) in [600.0, 380.0, 140.0].into_iter().enumerate() {
        let y = 94.0 + 40.0 * i as f32;
        area(ui, Rect::from_min_size(pos2(12.0, y), vec2(w, BANNER_H)), |ui| {
            filter_banner(ui, t, &format!("banner:sheet{i}"), &format!("button:sheet.showAll{i}"), SHORT);
        });
    }
    label(218.0, "drop_target · insertion_marker");
    drop_target(&p, t, Rect::from_min_size(pos2(12.0, 236.0), vec2(220.0, 24.0)));
    paint_album(&p, Rect::from_center_size(pos2(32.0, 248.0), vec2(16.0, 16.0)), AlbumIcon::Starred, t.text);
    p.text(pos2(48.0, 248.0), Align2::LEFT_CENTER, "Starred Photos", fonts::regular(t.body), t.text);
    p.rect_filled(Rect::from_min_size(pos2(260.0, 232.0), vec2(48.0, 48.0)), 0, Color32::from_rgb(70, 110, 150));
    p.rect_filled(Rect::from_min_size(pos2(320.0, 232.0), vec2(48.0, 48.0)), 0, Color32::from_rgb(150, 120, 70));
    insertion_marker(&p, t, pos2(314.0, 228.0), 56.0);
}

/// The pills of the sheet: one per operation, stacked at the sheet's right.
fn sheet_pills(ui: &mut Ui, t: &Tokens, _: &mut ()) {
    sheet(ui, t, &mut ());
    let ops =
        [(DragOp::Move, Some("Garden")), (DragOp::Copy, Some("Garden")), (DragOp::AddToAlbum, None), (DragOp::Hold, None), (DragOp::None, None)];
    for (i, (op, target)) in ops.into_iter().enumerate() {
        let pointer = pos2(620.0, 70.0 + 44.0 * i as f32);
        drag_pill(ui.ctx(), t, &format!("drag:sheet{i}"), pointer, &DragPill { op, count: 3, target, thumb: None });
    }
}

#[test]
fn the_m9_widget_sheet_matches_its_fingerprint() {
    let mut failures = vec![];
    let mut a = Rig::new((900, 300), (), sheet_pills);
    let mut b = Rig::new((900, 300), (), sheet_pills);
    let (one, two) = (a.image(), b.image());
    assert!(one.rgba == two.rgba, "two renders of the sheet differ");
    check_ids("sheet", &a.widgets, &registry::unregistered(&a.ctx));
    for i in 0..5 {
        let r = a.rect(&format!("drag:sheet{i}"));
        assert!(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 300.0)).contains_rect(r), "pill {i}: {r:?}");
    }
    check("m9-widgets", &one, &mut failures);
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}

// ---- The gallery: M9's widgets in a window ----

/// A small procedural photo for the pill: a sky over a sea.
fn photo(ctx: &Context) -> egui::TextureHandle {
    let (w, h) = (48usize, 32usize);
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let c = if y < h / 2 { Color32::from_rgb(140, 170, 205 - x as u8) } else { Color32::from_rgb(40, 80 + y as u8, 120) };
            pixels.push(c);
        }
    }
    let image = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
    ctx.load_texture("gallery-photo", image, egui::TextureOptions::LINEAR)
}

struct Gallery {
    days: Option<u32>,
    pointer: Pos2,
    op: DragOp,
    target: &'static str,
    photo: Option<egui::TextureHandle>,
}

fn gallery(ui: &mut Ui, t: &Tokens, s: &mut Gallery) {
    let screen = ui.ctx().content_rect();
    let (w, h) = (screen.width(), screen.height());
    let p = ui.painter().clone();
    let toolbar = Rect::from_min_size(Pos2::ZERO, vec2(w, t.metrics.toolbar_h));
    p.rect_filled(Rect::from_min_size(pos2(0.0, toolbar.max.y - 1.0), vec2(w, 1.0)), 0, t.divider);
    // The toolbar's age slider with its label in the caption above it.
    let (left, width) = light_table(t, w);
    let age_x = left + 140.0;
    p.text(pos2(age_x, 4.0), Align2::LEFT_TOP, age_label(s.days), fonts::regular(t.small), t.text_dim);
    for (i, icon) in [Icon::Star, Icon::Faces, Icon::Videos, Icon::Geotagged].into_iter().enumerate() {
        icons::paint(&p, Rect::from_center_size(pos2(left + 16.0 + 28.0 * i as f32, 27.0), vec2(16.0, 16.0)), icon, t.icon);
    }
    row(ui, Rect::from_min_size(pos2(age_x, 17.0), vec2(100.0, 20.0)), |ui| {
        age_slider(ui, t, "slider:age", &mut s.days, 100.0, "Show only recent photos");
    });
    // The tree: the Albums group with the automatic albums, then folders; Holidays a drop target.
    let tree_top = toolbar.max.y;
    let m = &t.metrics;
    p.text(pos2(12.0, tree_top + m.group_header_h / 2.0), Align2::LEFT_CENTER, "Albums (4)", fonts::semibold(t.body), t.text_label);
    enum Glyph {
        Auto(AlbumIcon),
        Shell(Icon),
    }
    let rows = [
        (Glyph::Auto(AlbumIcon::Search), "Search results for \u{201c}lisbon\u{201d}", "33"),
        (Glyph::Auto(AlbumIcon::Starred), "Starred Photos", "128"),
        (Glyph::Auto(AlbumIcon::Recent), "Recently Updated", "412"),
        (Glyph::Shell(Icon::Album), "Holidays", "56"),
        (Glyph::Shell(Icon::Folder), "Folders (2)", ""),
        (Glyph::Shell(Icon::Folder), "Lisbon", "48"),
        (Glyph::Shell(Icon::Folder), s.target, "17"),
    ];
    for (i, (glyph, name, count)) in rows.into_iter().enumerate() {
        let y = tree_top + m.group_header_h + m.tree_row_h * i as f32;
        let row_rect = Rect::from_min_size(pos2(4.0, y), vec2(m.tree_w - 8.0, m.tree_row_h));
        if count.is_empty() {
            p.text(pos2(12.0, row_rect.center().y), Align2::LEFT_CENTER, name, fonts::semibold(t.body), t.text_label);
            continue;
        }
        let target = name == s.target && s.op != DragOp::AddToAlbum || name == "Holidays" && s.op == DragOp::AddToAlbum;
        if target {
            drop_target(&p, t, row_rect);
        }
        let icon_rect = Rect::from_center_size(pos2(row_rect.min.x + 18.0, row_rect.center().y), vec2(16.0, 16.0));
        match glyph {
            Glyph::Auto(icon) => paint_album(&p, icon_rect, icon, t.icon),
            Glyph::Shell(icon) => icons::paint(&p, icon_rect, icon, t.icon),
        }
        let color = if target { t.text } else { t.text_label };
        p.text(pos2(row_rect.min.x + 34.0, row_rect.center().y), Align2::LEFT_CENTER, name, fonts::regular(t.body), color);
        p.text(pos2(row_rect.max.x - 6.0, row_rect.center().y), Align2::RIGHT_CENTER, count, fonts::regular(t.count), t.text_dim);
    }
    // The light table: the banner across its top, then a row of cells with the insertion marker.
    let table = Rect::from_min_max(pos2(left, tree_top), pos2(left + width, h));
    p.rect_filled(table, 0, t.grid_bg);
    area(ui, Rect::from_min_size(table.min, vec2(width, BANNER_H)), |ui| {
        filter_banner(ui, t, "banner:search", "button:search.showAll", SHORT);
    });
    let cell = 144.0;
    let colours = [(70, 110, 150), (150, 120, 70), (90, 140, 90), (160, 90, 110), (110, 100, 160)];
    let mut x = table.min.x + m.grid_margin;
    let y = table.min.y + BANNER_H + 22.0;
    for (i, (r, g, b)) in colours.into_iter().enumerate() {
        if x + cell > table.max.x - m.grid_margin {
            break;
        }
        let photo_rect = Rect::from_min_size(pos2(x, y + 24.0), vec2(cell, 96.0));
        p.rect_filled(photo_rect, 0, Color32::from_rgb(r, g, b));
        if i == 2 {
            insertion_marker(&p, t, pos2(x - 6.0, y), cell);
        }
        x += cell + 12.0;
    }
    // The pill, with the first photo's thumbnail.
    let photo = s.photo.get_or_insert_with(|| photo(ui.ctx()));
    let thumb = PillThumb { texture: photo.id(), size: vec2(48.0, 32.0) };
    let target = if s.op == DragOp::AddToAlbum { "Holidays" } else { s.target };
    drag_pill(ui.ctx(), t, "drag:pill", s.pointer, &DragPill { op: s.op, count: 3, target: Some(target), thumb: Some(thumb) });
}

fn gallery_window(size: (u32, u32)) -> Rig<Gallery> {
    let t = Tokens::default();
    let m = t.metrics;
    let tree_row = |i: f32| t.metrics.toolbar_h + m.group_header_h + m.tree_row_h * i + m.tree_row_h / 2.0;
    let state = if size.0 >= 1400 {
        // Adding three photos to Holidays, the pointer on its row.
        Gallery { days: Some(91), pointer: pos2(150.0, tree_row(3.0)), op: DragOp::AddToAlbum, target: "Garden", photo: None }
    } else {
        // Copying onto Garden with Ctrl, the pointer near the window's bottom-right corner.
        Gallery { days: Some(1826), pointer: pos2(size.0 as f32 - 6.0, size.1 as f32 - 6.0), op: DragOp::Copy, target: "Garden", photo: None }
    };
    Rig::new(size, state, gallery)
}

#[test]
fn the_m9_gallery_at_both_window_sizes() {
    let mut failures = vec![];
    for size in WINDOWS {
        let mut one = gallery_window(size);
        check_ids(&format!("gallery {size:?}"), &one.widgets, &registry::unregistered(&one.ctx));
        let window = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        assert!(window.shrink(PILL_MARGIN).expand(0.01).contains_rect(one.rect("drag:pill")), "{size:?}");
        assert_eq!(one.info("slider:age").label, age_label(one.state.days));
        // One fingerprint per state: the large window's (the small one has to fit, above).
        if size == (1400, 900) {
            let mut two = gallery_window(size);
            assert!(one.image().rgba == two.image().rgba, "two renders of the {size:?} window differ");
            check(&format!("gallery-m9-{}x{}", size.0, size.1), &one.image(), &mut failures);
        }
    }
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
