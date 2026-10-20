//! The light table's speed-control scroll bar, in the 16-wide strip right of the grid
//! (`ui-layout.md` §5.6). Top to bottom: **⇑** (`scroll:prevSection`), **▲** (`scroll:up`), the
//! track (`scroll:track`) with the speed thumb (`scroll:thumb`) and the position marker, **▼**
//! (`scroll:down`), **⇓** (`scroll:nextSection`).
//!
//! - The buttons and the track act on press and auto-repeat while held ([`REPEAT_DELAY`], then
//!   every [`REPEAT_EVERY`]): ▲/▼ move one thumbnail row, the track a page of one row + 90 towards
//!   the press, ⇑/⇓ bring the previous/next section's header to the top (⇑ first snaps to the
//!   current section's start when mid-section). Every step is animated by the light table, and
//!   steps taken while one is still easing add up: each starts from the anchor the last one asked
//!   for ([`target`]), not from where the view is on its way.
//! - The thumb ([`THUMB_H`] high) rests at the centre of the track whatever the position. Dragged,
//!   it sets the scroll **speed**: [`speed`] of its distance from the centre (nothing inside the
//!   [`DEAD_ZONE`], then `top · u^exponent` screen heights per second, `u` the share of the way to
//!   the track's end), the exponent and top speed read from the library's settings
//!   ([`dalica_engine::settings::ThumbSpeed`]). On release the view stops where it is and the thumb
//!   springs back to the centre over [`SPRING_SECONDS`] (ease-out).
//! - The position is shown by the faint marker ([`MARKER_H`] high, `text_dim` at 40 %) at the
//!   proportional place in the track ([`marker`]); it takes no input.
//!
//! **Boundary.** Two calls a frame. [`input`] runs before the content is painted, with the frame at
//! the offset the view has so far, and returns a [`ScrollRequest`] (jump, or animate to an anchor)
//! that the parent applies at once, so the content is painted where the bar put it; [`paint`] runs
//! after the content with the final offset, paints the bar and registers its six ids. The scroll
//! commands (`commands/scroll.rs`) reuse [`target`] so the keys and the buttons move alike.

use dalica_engine::settings::ThumbSpeed;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{CornerRadius, Id, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::Frame;
use super::layout::Layout;
use crate::state::ScrollAnchor;

/// The height of each of the four buttons (`design-system.md` §4: 16 × 24).
pub const BUTTON_H: f32 = 24.0;
/// The speed thumb's height (16 × 64).
pub const THUMB_H: f32 = 64.0;
/// The position marker's height (16 × 3).
pub const MARKER_H: f32 = 3.0;
/// How far from the centre the thumb moves before anything scrolls (points).
pub const DEAD_ZONE: f32 = 4.0;
/// How long the thumb takes to spring back to the centre after a release (seconds).
pub const SPRING_SECONDS: f64 = 0.15;
/// A held button or track waits this long before it repeats (seconds)…
pub const REPEAT_DELAY: f64 = 0.4;
/// …then repeats this often (seconds).
pub const REPEAT_EVERY: f64 = 0.06;
/// What a track press adds to one thumbnail row (points).
pub const PAGE_EXTRA: f64 = 90.0;
/// The longest frame the speed thumb integrates over (seconds): a stalled frame never jumps far.
const MAX_DT: f64 = 0.1;
/// Within this of a section's top, the view counts as at its start (⇑ goes to the one before).
const AT_TOP: f64 = 0.5;

/// What the scroll bar asks the light table to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollRequest {
    /// Put the view's top at this content offset now.
    To(f64),
    /// Ease the view to this anchor.
    Animate(ScrollAnchor),
}

/// One move of the view, as the buttons, the track and the scroll commands ask for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Down by this many points (up when negative).
    By(f64),
    /// The previous section's header to the top; first the current section's, when mid-section.
    PrevSection,
    /// The next section's header to the top (the end of the library after the last).
    NextSection,
    /// The top of the library.
    Top,
    /// The bottom of the library.
    Bottom,
}

/// The parts of the strip, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    PrevSection,
    Up,
    Track,
    Down,
    NextSection,
}

impl Part {
    /// Every part that takes presses, the buttons first.
    const PRESSABLE: [Part; 5] = [Part::PrevSection, Part::Up, Part::Down, Part::NextSection, Part::Track];

    /// The automation id (`control-protocol.md` §7).
    pub fn id(self) -> &'static str {
        match self {
            Part::PrevSection => "scroll:prevSection",
            Part::Up => "scroll:up",
            Part::Track => "scroll:track",
            Part::Down => "scroll:down",
            Part::NextSection => "scroll:nextSection",
        }
    }

    fn label(self) -> String {
        match self {
            Part::PrevSection => crate::tl!("Previous section"),
            Part::Up => crate::tl!("Scroll up"),
            Part::Track => crate::tl!("Scroll bar"),
            Part::Down => crate::tl!("Scroll down"),
            Part::NextSection => crate::tl!("Next section"),
        }
    }
}

/// The rects of the strip's parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    pub prev_section: Rect,
    pub up: Rect,
    pub track: Rect,
    pub down: Rect,
    pub next_section: Rect,
}

impl Parts {
    pub fn get(&self, part: Part) -> Rect {
        match part {
            Part::PrevSection => self.prev_section,
            Part::Up => self.up,
            Part::Track => self.track,
            Part::Down => self.down,
            Part::NextSection => self.next_section,
        }
    }
}

/// The strip `bar` cut into its parts: four [`BUTTON_H`] buttons (less when the strip is shorter
/// than five of them), the track between.
pub fn parts(bar: Rect) -> Parts {
    let h = if bar.height().is_finite() { bar.height().max(0.0) } else { 0.0 };
    let b = BUTTON_H.min(h / 5.0);
    let row = |top: f32, bottom: f32| Rect::from_x_y_ranges(bar.x_range(), top..=bottom.max(top));
    let (top, bottom) = (bar.min.y, bar.min.y + h);
    Parts {
        prev_section: row(top, top + b),
        up: row(top + b, top + 2.0 * b),
        track: row(top + 2.0 * b, bottom - 2.0 * b),
        down: row(bottom - 2.0 * b, bottom - b),
        next_section: row(bottom - b, bottom),
    }
}

/// How far the thumb's centre can move from the track's centre either way.
pub fn reach(track: Rect) -> f32 {
    let h = if track.height().is_finite() { track.height().max(0.0) } else { 0.0 };
    ((h - THUMB_H.min(h)) / 2.0).max(0.0)
}

/// The thumb in `track`, `displacement` points from its centre (kept inside the track).
pub fn thumb(track: Rect, displacement: f32) -> Rect {
    let r = reach(track);
    let d = if displacement.is_finite() { displacement.clamp(-r, r) } else { 0.0 };
    let h = THUMB_H.min(track.height().max(0.0));
    Rect::from_center_size(pos2(track.center().x, track.center().y + d), vec2(track.width(), h))
}

/// The position marker in `track` for a view `view_h` high over `content` points scrolled to
/// `offset`: at the top for the top of the content, at the bottom for its end. Hostile numbers stay
/// inside the track.
pub fn marker(track: Rect, view_h: f32, content: f64, offset: f64) -> Rect {
    let max = if content.is_finite() { content - f64::from(view_h.max(0.0)) } else { 0.0 };
    let frac = if max > 0.0 && offset.is_finite() { (offset / max).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let h = MARKER_H.min(track.height().max(0.0));
    let top = track.min.y + frac * (track.height().max(0.0) - h);
    Rect::from_x_y_ranges(track.x_range(), top..=top + h)
}

/// The scroll speed, in screen heights per second (negative = up), of a thumb `distance` points
/// from the centre of a track it can move `reach` points either way: nothing inside the
/// [`DEAD_ZONE`], then `top_speed · u^exponent` with `u` the share of the way from the dead zone to
/// the track's end. Hostile numbers give 0 or stay within the top speed.
pub fn speed(distance: f32, reach: f32, curve: &ThumbSpeed) -> f64 {
    let room = reach - DEAD_ZONE;
    if !(distance.is_finite() && room.is_finite() && room > 0.0 && distance.abs() > DEAD_ZONE) {
        return 0.0;
    }
    let u = f64::from(((distance.abs() - DEAD_ZONE) / room).clamp(0.0, 1.0));
    let exponent = if curve.exponent.is_finite() && curve.exponent > 0.0 { f64::from(curve.exponent) } else { 2.0 };
    let top = if curve.top_speed.is_finite() { f64::from(curve.top_speed.max(0.0)) } else { 0.0 };
    (top * u.powf(exponent)).copysign(f64::from(distance))
}

/// Where `step` takes the view, as an anchor to ease to. It starts from `from` (the anchor last
/// asked for, so steps taken while one is still easing add up), or from `current` when `from`'s
/// section is not laid out; both are taken inside the content (`0 ..= max_offset`).
pub fn target(layout: &Layout, from: &ScrollAnchor, current: f64, view_h: f32, step: Step) -> ScrollAnchor {
    let y = layout.clamp(layout.offset_of(from).unwrap_or(current), view_h);
    let header = |i: usize| layout.section(i).map(|s| ScrollAnchor { section: Some(s), offset: 0.0 });
    let bottom = || layout.anchor_at(layout.max_offset(view_h));
    match step {
        Step::By(d) => layout.anchor_at(layout.clamp(y + if d.is_finite() { d } else { 0.0 }, view_h)),
        Step::Top => ScrollAnchor::default(),
        Step::Bottom => bottom(),
        Step::NextSection => match layout.index_at(y) {
            Some(i) => header(i + 1).unwrap_or_else(bottom),
            None => ScrollAnchor::default(),
        },
        Step::PrevSection => {
            let Some(i) = layout.index_at(y) else { return ScrollAnchor::default() };
            let top = layout.top(i).unwrap_or(0.0);
            let back = if y - top > AT_TOP { Some(i) } else { i.checked_sub(1) };
            // The first section's top is the top of the library.
            back.filter(|&k| layout.top(k).is_some_and(|t| t > 0.0)).and_then(header).unwrap_or_default()
        }
    }
}

/// The step of one thumbnail row (▲/▼).
pub fn row_step(layout: &Layout) -> f64 {
    layout.row_pitch()
}

/// The step of one track press: one thumbnail row + [`PAGE_EXTRA`].
pub fn page_step(layout: &Layout) -> f64 {
    layout.row_pitch() + PAGE_EXTRA
}

/// A part held down, and when it next repeats.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Held {
    part: Part,
    next: f64,
}

/// What the scroll bar keeps between frames.
#[derive(Debug, Default)]
pub struct Scrollbar {
    /// The thumb's distance from the track's centre (points, positive = down).
    displacement: f32,
    /// While the thumb is held: its displacement when it was pressed.
    grab: Option<f32>,
    /// The spring back to the centre: when it started and from where.
    spring: Option<(f64, f32)>,
    /// The clock of the last [`input`], to integrate the thumb's speed.
    last: Option<f64>,
    /// The button or track held down.
    held: Option<Held>,
    /// The speed the thumb asks for now (points per second).
    speed: f64,
    /// Whether the content is taller than the view (the parts take input).
    scrollable: bool,
    /// This frame's interaction with each part, and with the thumb.
    responses: Vec<(Part, Response)>,
    thumb: Option<Response>,
    /// The thumb and the marker as last painted.
    painted: Option<(Rect, Rect)>,
}

impl Scrollbar {
    /// The thumb's distance from the track's centre (points, positive = down).
    pub fn displacement(&self) -> f32 {
        self.displacement
    }

    /// The speed the thumb sets now, in points per second (negative = up); 0 when not held.
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// The thumb's rect as last painted.
    pub fn thumb_rect(&self) -> Option<Rect> {
        self.painted.map(|(t, _)| t)
    }

    /// The position marker's rect as last painted.
    pub fn marker_rect(&self) -> Option<Rect> {
        self.painted.map(|(_, m)| m)
    }
}

/// Before the content: the thumb's speed, the buttons and the track.
pub fn input(ui: &mut Ui, f: &Frame<'_>, bar: &mut Scrollbar, rect: Rect) -> Option<ScrollRequest> {
    if !rect.is_positive() {
        *bar = Scrollbar::default();
        return None;
    }
    let view_h = f.view_h();
    let scrollable = f.layout.max_offset(view_h) > 0.0;
    bar.scrollable = scrollable;
    let p = parts(rect);
    let dt = bar.last.map_or(0.0, |l| if f.now.is_finite() && f.now > l { (f.now - l).min(MAX_DT) } else { 0.0 });
    bar.last = f.now.is_finite().then_some(f.now);
    // Drags too: a press held past egui's longest click would otherwise stop counting as held.
    let sense = if scrollable { Sense::click_and_drag() } else { Sense::hover() };
    bar.responses = Part::PRESSABLE.iter().map(|&part| (part, ui.interact(p.get(part), Id::new(part.id()), sense))).collect();
    let reach = reach(p.track);
    let mut request = None;

    // The speed thumb. First where it is this frame: held, it is as far from the centre as the
    // pointer travelled since the press; released, it springs back.
    let (primary_down, origin, pos) = ui.input(|i| (i.pointer.primary_down(), i.pointer.press_origin(), i.pointer.latest_pos()));
    if let (Some(grab), true, Some(origin), Some(pos)) = (bar.grab, primary_down, origin, pos) {
        let d = grab + pos.y - origin.y;
        bar.displacement = if d.is_finite() { d.clamp(-reach, reach) } else { 0.0 };
    } else if let Some((start, from)) = bar.spring {
        let t = if f.now.is_finite() && start.is_finite() { ((f.now - start) / SPRING_SECONDS).clamp(0.0, 1.0) } else { 1.0 };
        if t >= 1.0 - 1e-6 {
            bar.displacement = 0.0;
            bar.spring = None;
        } else {
            bar.displacement = from * (1.0 - t as f32).powi(3);
        }
    }
    // Then its input, on top of the track's, so a press on it is the thumb's.
    let r = ui.interact(thumb(p.track, bar.displacement), Id::new("scroll:thumb"), if scrollable { Sense::drag() } else { Sense::hover() });
    if scrollable && r.is_pointer_button_down_on() {
        bar.spring = None;
        bar.grab.get_or_insert(bar.displacement);
        let curve = f.session.settings().thumb_speed();
        bar.speed = speed(bar.displacement, reach, &curve) * f64::from(view_h);
        if bar.speed != 0.0 {
            request = Some(ScrollRequest::To(f.offset + bar.speed * dt));
            ui.ctx().request_repaint();
        }
    } else {
        bar.speed = 0.0;
        if bar.grab.take().is_some() && bar.displacement != 0.0 {
            bar.spring = Some((f.now, bar.displacement));
        }
        if !scrollable {
            bar.spring = None;
            bar.displacement = 0.0;
        }
        if bar.spring.is_some() {
            ui.ctx().request_repaint();
        }
    }
    bar.thumb = Some(r);

    // The buttons and the track: act on press, repeat while held.
    let down = bar.responses.iter().find(|(_, r)| r.is_pointer_button_down_on()).map(|(part, _)| *part);
    match down {
        Some(part) if scrollable && request.is_none() => {
            let fire = match &mut bar.held {
                Some(h) if h.part == part => {
                    let due = f.now >= h.next;
                    if due {
                        h.next = (h.next + REPEAT_EVERY).max(f.now);
                    }
                    due
                }
                _ => {
                    bar.held = Some(Held { part, next: f.now + REPEAT_DELAY });
                    true
                }
            };
            let step = match part {
                Part::PrevSection => Some(Step::PrevSection),
                Part::NextSection => Some(Step::NextSection),
                Part::Up => Some(Step::By(-row_step(f.layout))),
                Part::Down => Some(Step::By(row_step(f.layout))),
                Part::Track => ui.input(|i| i.pointer.latest_pos()).and_then(|pos| {
                    let t = thumb(p.track, bar.displacement);
                    if pos.y < t.min.y {
                        Some(Step::By(-page_step(f.layout)))
                    } else if pos.y > t.max.y {
                        Some(Step::By(page_step(f.layout)))
                    } else {
                        None
                    }
                }),
            };
            if fire && let Some(step) = step {
                request = Some(ScrollRequest::Animate(target(f.layout, &f.state.scroll, f.offset, view_h, step)));
            }
            ui.ctx().request_repaint();
        }
        _ => bar.held = None,
    }
    request
}

/// After the content: the bar at the final offset, and its ids.
pub fn paint(ui: &mut Ui, f: &Frame<'_>, bar: &mut Scrollbar, rect: Rect) {
    let Some(thumb_r) = bar.thumb.clone() else { return };
    let t = f.t;
    let p = parts(rect);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(p.track, 0, t.inset);
    let enabled = bar.scrollable;
    for (part, r) in &bar.responses {
        if *part == Part::Track {
            widgets::register(ui.ctx(), part.id(), Role::ScrollTrack, &part.label(), Target::Widget(r), r.is_pointer_button_down_on());
            continue;
        }
        let area = p.get(*part);
        let pressed = enabled && r.is_pointer_button_down_on();
        let hovered = enabled && r.hovered();
        if pressed || hovered {
            painter.rect_filled(area, 0, if pressed { t.pressed } else { t.hover });
        }
        let color = if !enabled {
            t.text_disabled
        } else if hovered || pressed {
            t.text
        } else {
            t.icon
        };
        paint_arrow(&painter, area, *part, color);
        widgets::register(ui.ctx(), part.id(), Role::IconButton, &part.label(), Target::Widget(r), pressed);
    }
    let th = thumb(p.track, bar.displacement);
    let active = enabled && (thumb_r.is_pointer_button_down_on() || thumb_r.hovered());
    let m = marker(p.track, f.view_h(), f.layout.height(), f.offset);
    // With nothing to scroll the track stays empty: no thumb to pull, no position to show.
    let pill = th.shrink2(vec2(3.0, 2.0));
    if enabled && pill.is_positive() {
        painter.rect_filled(pill, CornerRadius::same(4), if active { t.thumb } else { t.track });
    }
    if enabled {
        painter.rect_filled(m, 0, t.text_dim.gamma_multiply(0.4));
    }
    bar.painted = Some((th, m));
    widgets::register(
        ui.ctx(),
        "scroll:thumb",
        Role::ScrollThumb,
        &crate::tl!("Scroll speed"),
        Target::Widget(&thumb_r),
        thumb_r.is_pointer_button_down_on(),
    );
}

/// ▲ / ▼ as a chevron; ⇑ / ⇓ as a chevron under (over) a bar.
fn paint_arrow(painter: &egui::Painter, area: Rect, part: Part, color: egui::Color32) {
    let side = 12.0f32.min(area.width()).min(area.height());
    if side.is_nan() || side <= 0.0 {
        return;
    }
    let glyph = Rect::from_center_size(area.center(), vec2(side, side));
    let stroke = Stroke::new(icons::stroke_width(side), color);
    let bar = |y: f32| painter.line_segment([pos2(glyph.min.x + side * 0.2, y), pos2(glyph.max.x - side * 0.2, y)], stroke);
    match part {
        Part::Up => icons::paint(painter, glyph, Icon::ChevronUp, color),
        Part::Down => icons::paint(painter, glyph, Icon::ChevronDown, color),
        Part::PrevSection => {
            icons::paint(painter, glyph.translate(vec2(0.0, side * 0.15)), Icon::ChevronUp, color);
            bar(glyph.min.y + side * 0.15);
        }
        Part::NextSection => {
            icons::paint(painter, glyph.translate(vec2(0.0, -side * 0.15)), Icon::ChevronDown, color);
            bar(glyph.max.y - side * 0.15);
        }
        Part::Track => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::light_table::layout::Spacing;
    use crate::state::Section;
    use dalica_core::FolderId;

    fn folder(n: u64) -> Section {
        Section::Folder(FolderId::new(n))
    }

    #[test]
    fn the_strip_is_cut_into_four_buttons_and_the_track() {
        let bar = Rect::from_min_size(pos2(990.0, 64.0), vec2(16.0, 600.0));
        let p = parts(bar);
        assert_eq!((p.prev_section.min.y, p.prev_section.height()), (64.0, 24.0));
        assert_eq!((p.up.min.y, p.up.height()), (88.0, 24.0));
        assert_eq!((p.track.min.y, p.track.max.y), (112.0, 616.0));
        assert_eq!((p.down.min.y, p.next_section.max.y), (616.0, 664.0));
        assert_eq!(reach(p.track), (504.0 - 64.0) / 2.0);
        // At rest the thumb is centred; it never leaves the track; tiny and hostile strips are safe.
        assert_eq!(thumb(p.track, 0.0).center().y, p.track.center().y);
        assert_eq!(thumb(p.track, 1e9).max.y, p.track.max.y);
        assert_eq!(thumb(p.track, f32::NAN).center().y, p.track.center().y);
        for h in [0.0, 10.0, 90.0, f32::NAN] {
            let p = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(16.0, h)));
            assert!(p.track.height() >= 0.0 && reach(p.track) >= 0.0 && thumb(p.track, 30.0).height() >= 0.0, "{h}");
        }
    }

    #[test]
    fn the_marker_follows_the_offset_and_never_leaves_its_track() {
        let track = Rect::from_min_size(pos2(490.0, 100.0), vec2(16.0, 403.0));
        assert_eq!(marker(track, 400.0, 1600.0, 0.0).min.y, 100.0);
        assert_eq!(marker(track, 400.0, 1600.0, 1200.0).max.y, 503.0);
        assert_eq!(marker(track, 400.0, 1600.0, 600.0).min.y, 300.0);
        for (content, offset) in [(f64::NAN, 0.0), (0.0, 0.0), (1600.0, f64::NAN), (1600.0, -50.0), (1600.0, 1e9), (f64::INFINITY, 5.0)] {
            let r = marker(track, 400.0, content, offset);
            assert!(r.min.y >= 100.0 && r.max.y <= 503.0 && r.height() == MARKER_H, "{content} {offset}: {r:?}");
        }
    }

    #[test]
    fn hostile_speeds_are_zero_or_bounded() {
        let c = ThumbSpeed::default();
        for (d, r) in [(f32::NAN, 100.0), (50.0, f32::NAN), (50.0, 3.0), (f32::INFINITY, 100.0), (-1e30, 100.0), (50.0, f32::INFINITY)] {
            let s = speed(d, r, &c);
            assert!(s.is_finite() && s.abs() <= 40.0, "{d} {r}: {s}");
        }
        let odd = ThumbSpeed { exponent: f32::NAN, top_speed: f32::INFINITY };
        assert_eq!(speed(50.0, 100.0, &odd), 0.0);
    }

    #[test]
    fn steps_land_on_headers_and_add_up() {
        // Three sections of 20, 3 and 40 photos at 1100 × 144 (6 columns), a view 400 high.
        let l = Layout::new([(folder(1), 20), (folder(2), 3), (folder(3), 40)], 1, 1100.0, 144.0, false, Spacing::default());
        let (t1, t2) = (l.top(1).unwrap(), l.top(2).unwrap());
        let max = l.max_offset(400.0);
        let near = |a: &ScrollAnchor, y: f64| l.offset_of(a).is_some_and(|o| (o - y).abs() < 1e-3);
        let top = ScrollAnchor::default();
        let at = |s: u64, o: f32| ScrollAnchor { section: Some(folder(s)), offset: o };
        assert_eq!(target(&l, &top, 0.0, 400.0, Step::NextSection), at(2, 0.0));
        assert_eq!(target(&l, &at(2, 0.0), 0.0, 400.0, Step::NextSection), at(3, 0.0), "from the anchor asked for");
        assert!(near(&target(&l, &at(3, 0.0), 0.0, 400.0, Step::NextSection), max), "after the last: the end");
        // ⇑ mid-section snaps to its start first, then goes to the one before.
        assert_eq!(target(&l, &at(2, 30.0), 0.0, 400.0, Step::PrevSection), at(2, 0.0));
        assert_eq!(target(&l, &at(2, 0.0), 0.0, 400.0, Step::PrevSection), top);
        assert_eq!(target(&l, &at(3, 0.0), 0.0, 400.0, Step::PrevSection), at(2, 0.0));
        assert_eq!(target(&l, &at(1, 10.0), 0.0, 400.0, Step::PrevSection), top);
        assert_eq!(target(&l, &top, 0.0, 400.0, Step::PrevSection), top);
        // Steps by points, clamped to the content.
        assert!(near(&target(&l, &at(2, 0.0), 0.0, 400.0, Step::By(100.0)), t1 + 100.0));
        assert_eq!(target(&l, &top, 0.0, 400.0, Step::By(-100.0)), top);
        assert!(near(&target(&l, &top, 0.0, 400.0, Step::By(1e12)), max));
        assert_eq!(target(&l, &at(2, 0.0), 0.0, 400.0, Step::By(f64::NAN)), at(2, 0.0));
        assert_eq!(target(&l, &at(2, 5.0), 0.0, 400.0, Step::Top), top);
        assert!(near(&target(&l, &top, 0.0, 400.0, Step::Bottom), max));
        // An anchor whose section is gone starts from the current offset.
        assert_eq!(target(&l, &at(9, 0.0), t2 + 1.0, 400.0, Step::PrevSection), at(3, 0.0));
        // An empty layout goes nowhere.
        let empty = Layout::new([], 1, 1100.0, 144.0, false, Spacing::default());
        for step in [Step::By(10.0), Step::NextSection, Step::PrevSection, Step::Top, Step::Bottom] {
            assert_eq!(target(&empty, &top, 0.0, 400.0, step), top, "{step:?}");
        }
    }
}
