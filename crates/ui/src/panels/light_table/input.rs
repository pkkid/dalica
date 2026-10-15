//! The light table's pointer input on cells and empty space (`ui-layout.md` §5.4–§5.5), on the
//! model of [`crate::selection`]: a click selects a photo, Ctrl-click toggles it (in another
//! section it replaces the selection), Shift-click selects the range from the anchor, a right-click
//! on an unselected photo selects it first, a double-click opens the photo in the edit view
//! (`view.edit`, `editing.md` §1), a click on empty space selects nothing, and a rubber
//! band dragged from empty space selects the photos it touches in the section it started in (with
//! Ctrl, added to the selection there). Pressing on a thumbnail and dragging never starts a band.
//! A band held at the view's top or bottom edge scrolls it ([`autoscroll_speed`]: faster the
//! farther past the edge) and keeps selecting the rows that come in, never beyond its section's
//! ends. Every change goes through `select.set`, which makes the section's folder the tree's folder.
//!
//! A right-click on a photo opens the photo context menu ([`super::context_menu`]) for the
//! selection, after selecting the photo when it was not. A press on a thumbnail dragged past
//! egui's threshold starts a drag of photos (`crate::drag`): the selection when the photo is
//! selected, else that photo alone; `crate::drag` ends it when the button is released. While a
//! drag of photos is over a user album's section, the gap nearest the pointer between two of its
//! cells (or before the first, after the last of a row) is offered as a drop target ([`gap`]) and
//! shows the insertion marker; only the cells on screen are looked at.
//!
//! **Boundary.** Two calls a frame and one after it. [`begin`] runs before any cell is laid out
//! (an interaction it creates on the empty space sits under the cells, so cells win their clicks);
//! [`finish`] runs after the last cell with the cells' [`CellOut`]s and what [`begin`] returned, and
//! collects what to do as [`Pending`] (the commands, and how far to scroll the view); the parent
//! moves the view by [`Pending::scroll`] and passes the rest to [`apply`] once the library is no
//! longer borrowed. What input keeps between frames (a rubber band) lives in [`Input`].

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2};
use serde_json::{Value, json};

use super::Frame;
use super::cell::CellOut;
use crate::app::App;
use crate::selection;
use crate::state;

/// The automation id of the light table's empty space (`control-protocol.md` §7).
pub const EMPTY_SPACE_ID: &str = "lightTable";

/// How far inside the view's top and bottom edges a held band starts scrolling it (points), so it
/// scrolls in a full-screen window whose edges the pointer cannot pass.
pub const AUTOSCROLL_EDGE: f32 = 8.0;
/// The band's scroll speed at that line (points per second), how much it grows per point the
/// pointer is farther out, and its cap.
pub const AUTOSCROLL_BASE: f32 = 120.0;
pub const AUTOSCROLL_GAIN: f32 = 12.0;
pub const AUTOSCROLL_MAX: f32 = 3000.0;

/// How fast a held rubber band scrolls the view (points per second; positive = down, towards the
/// end) with the pointer at `y` and the view spanning `top..bottom`: none inside, from
/// [`AUTOSCROLL_EDGE`] in from an edge [`AUTOSCROLL_BASE`] growing by [`AUTOSCROLL_GAIN`] per point
/// farther out, at most [`AUTOSCROLL_MAX`]. 0 for numbers that are not finite.
pub fn autoscroll_speed(y: f32, top: f32, bottom: f32) -> f32 {
    if !(y.is_finite() && top.is_finite() && bottom.is_finite()) {
        return 0.0;
    }
    let speed = |past: f32| (AUTOSCROLL_BASE + AUTOSCROLL_GAIN * past).min(AUTOSCROLL_MAX);
    let (below, above) = (y - (bottom - AUTOSCROLL_EDGE), (top + AUTOSCROLL_EDGE) - y);
    if below > 0.0 && below >= above {
        speed(below)
    } else if above > 0.0 {
        -speed(above)
    } else {
        0.0
    }
}

/// What the input keeps between frames.
#[derive(Debug, Default)]
pub struct Input {
    band: Option<Band>,
    /// A drag of photos started on a thumbnail is in progress.
    dragging: bool,
}

impl Input {
    /// Whether a rubber band is being dragged.
    pub fn banding(&self) -> bool {
        self.band.is_some()
    }
}

/// A rubber band being dragged.
#[derive(Debug)]
struct Band {
    /// The section it started in: it selects only there.
    section: state::Section,
    /// Where it started: x from the light table's left edge, y in content coordinates (so it stays
    /// put on the photos when the view scrolls).
    start: (f32, f64),
    /// The selection it adds to (Ctrl), else empty.
    base: Vec<PhotoId>,
    /// The photos (indices in the section) it touched last frame.
    hits: Option<Vec<usize>>,
}

/// A command to run.
#[derive(Debug)]
pub struct Action {
    pub command: String,
    pub params: Value,
}

/// A band held at an edge asks the view to move by `step` content points (positive = down), but
/// never past `limit` (its section's end going down, its top going up) and never back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Autoscroll {
    pub step: f64,
    pub limit: f64,
}

impl Autoscroll {
    /// Where a view at `offset` goes.
    pub fn apply(&self, offset: f64) -> f64 {
        if !(self.step.is_finite() && self.limit.is_finite() && offset.is_finite()) {
            return offset;
        }
        if self.step > 0.0 { (offset + self.step).min(self.limit.max(offset)) } else { (offset + self.step).max(self.limit.min(offset)) }
    }
}

/// What a drag of photos did this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum DragEvent {
    /// Started on thumbnail `from` (`thumb:12`) carrying `ids`.
    Start { from: String, ids: Vec<PhotoId> },
}

/// What this frame's input asks for: commands, in order, and a scroll.
#[derive(Debug, Default)]
pub struct Pending {
    pub actions: Vec<Action>,
    /// A band held at an edge: the parent moves the view as the user's own scroll, so the anchor
    /// follows.
    pub scroll: Option<Autoscroll>,
    /// The thumbnail whose photo menu was asked for this frame or is open: drawn on it once the
    /// actions ran ([`apply`]).
    pub menu: Option<Response>,
    /// A drag of photos started or ended.
    pub drag: Option<DragEvent>,
}

impl Pending {
    /// Selects `ids` in `section`: a click on a photo's copy in an album section selects it in the
    /// album (`select.set {section}`), on its folder's copy in the folder.
    fn select(&mut self, ids: &[PhotoId], mode: &str, section: state::Section) {
        let ids: Vec<u64> = ids.iter().map(|id| id.get()).collect();
        let params = if section.is_album() { json!({ "ids": ids, "mode": mode, "section": section }) } else { json!({ "ids": ids, "mode": mode }) };
        self.actions.push(Action { command: "select.set".into(), params });
    }
}

/// Before the cells: the empty space's interaction (`lightTable`), under the cells.
pub fn begin(ui: &mut Ui, f: &Frame<'_>, _input: &mut Input) -> Option<Response> {
    if !f.area.is_positive() {
        return None;
    }
    let response = ui.interact(f.area, Id::new("lightTable.emptySpace"), Sense::click_and_drag());
    widgets::register(ui.ctx(), EMPTY_SPACE_ID, Role::Other, "Light table", Target::Widget(&response), false);
    Some(response)
}

/// After the cells: what the clicks and the rubber band ask for.
pub fn finish(ui: &mut Ui, f: &Frame<'_>, input: &mut Input, background: Option<Response>, cells: &[CellOut]) -> Pending {
    let mut pending = Pending::default();
    let (command, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
    let sel = &f.state.selection;
    if let Some(c) = cells.iter().find(|c| c.response.clicked()) {
        let here = sel.ids.is_empty() || sel.section == Some(c.section);
        let mode = if shift {
            "range"
        } else if command && here {
            "toggle"
        } else {
            "replace"
        };
        pending.select(&[c.photo], mode, c.section);
    } else if let Some(c) =
        cells.iter().find(|c| c.response.secondary_clicked() && !(sel.contains(c.photo) && super::cell::is_primary(c.section, sel)))
    {
        pending.select(&[c.photo], "replace", c.section);
    }
    // The photo menu: asked for by this right-click, or open from an earlier one.
    pending.menu = cells.iter().find(|c| c.response.secondary_clicked() || c.response.context_menu_opened()).map(|c| c.response.clone());
    // The double-click's first click selected the photo; the second opens it.
    if let Some(c) = cells.iter().find(|c| c.response.double_clicked()) {
        pending.actions.push(Action { command: "view.edit".into(), params: json!({ "id": c.photo.get() }) });
    }
    gap(ui, f, cells);
    if let Some(bg) = background {
        pending.drag = thumb_drag(ui, f, input, &bg, cells);
        band(ui, f, input, &bg, cells, command, &mut pending);
        if bg.clicked() && !command && !shift && !sel.ids.is_empty() {
            pending.actions.push(Action { command: "select.none".into(), params: json!({}) });
        }
    }
    pending
}

/// How far above and below a cell the pointer still finds its gaps (points): the row gap between
/// two rows of cells belongs to the row above it and the row below it.
pub const GAP_REACH: f32 = 8.0;

/// Where a drop between two cells of a user album's section goes, for a pointer at `pos`: the cell
/// of such a section nearest the pointer in its row (the pointer within [`GAP_REACH`] of the
/// row's height and within half a cell of the cell), and whether the gap is after it (the pointer
/// right of its centre). Looks only at `cells`, the cells on screen.
pub fn gap_at(pos: Pos2, cells: &[CellOut]) -> Option<(&CellOut, bool)> {
    let near = |c: &&CellOut| {
        c.section.user_album().is_some() && pos.y >= c.rect.min.y - GAP_REACH && pos.y <= c.rect.max.y + GAP_REACH && c.rect.is_positive()
    };
    let distance = |c: &CellOut| (pos.x - pos.x.clamp(c.rect.min.x, c.rect.max.x)).abs();
    let c = cells.iter().filter(near).min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    (distance(c) <= c.rect.width() / 2.0).then(|| (c, pos.x > c.rect.center().x))
}

/// The x of the insertion marker for a gap after (or before) cell `c`: half-way to its neighbour in
/// the row, else [`GAP_REACH`] / 2 past its edge.
fn marker_x(c: &CellOut, after: bool, cells: &[CellOut]) -> f32 {
    let row = cells.iter().filter(|n| n.section == c.section && (n.rect.min.y - c.rect.min.y).abs() < 1.0);
    if after {
        let next = row.filter(|n| n.rect.min.x > c.rect.max.x).map(|n| n.rect.min.x).min_by(f32::total_cmp);
        next.map_or(c.rect.max.x + GAP_REACH / 2.0, |x| (c.rect.max.x + x) / 2.0)
    } else {
        let prev = row.filter(|n| n.rect.max.x < c.rect.min.x).map(|n| n.rect.max.x).max_by(f32::total_cmp);
        prev.map_or(c.rect.min.x - GAP_REACH / 2.0, |x| (x + c.rect.min.x) / 2.0)
    }
}

/// During a drag of photos: offers the gap under the pointer in a user album's section
/// ([`gap_at`]) and, when a drop there does something, paints the insertion marker in it.
fn gap(ui: &Ui, f: &Frame<'_>, cells: &[CellOut]) {
    let Some(pos) = crate::drag::pointer(ui.ctx(), f.state).filter(|p| f.area.contains(*p)) else { return };
    let Some((c, after)) = gap_at(pos, cells) else { return };
    let Some(album) = (match c.section {
        state::Section::Album(state::AlbumSection::User(k)) => Some(k),
        _ => None,
    }) else {
        return;
    };
    let at = state::DropAt::Gap { album, anchor: c.photo, after };
    if crate::drag::offer(ui.ctx(), f.session, f.state, at, &super::cell::thumb_id(c.section, c.photo)).is_some() {
        let x = marker_x(c, after, cells);
        widgets::insertion_marker(ui.painter(), f.t, pos2(x, c.rect.min.y), c.rect.height());
    }
}

/// A drag of photos: a press on a thumbnail moved past the drag threshold (the cells sense clicks
/// only, so the light table's own space senses the drag) carries the selection when the photo is
/// selected, else that photo; `crate::drag` ends it when the button is released.
fn thumb_drag(ui: &Ui, f: &Frame<'_>, input: &mut Input, bg: &Response, cells: &[CellOut]) -> Option<DragEvent> {
    if bg.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        let cell = origin.and_then(|o| cells.iter().find(|c| c.rect.contains(o)));
        if let Some(c) = cell {
            let sel = &f.state.selection;
            let ids = if sel.contains(c.photo) { sel.ids.clone() } else { vec![c.photo] };
            input.dragging = true;
            return Some(DragEvent::Start { from: super::cell::thumb_id(c.section, c.photo), ids });
        }
    }
    if input.dragging && (!bg.dragged() || bg.drag_stopped()) {
        input.dragging = false;
    }
    None
}

/// The rubber band: started by a drag from empty space, it selects the photos it touches in its
/// section each frame they change, is painted over the cells, and held at an edge asks for a scroll.
fn band(ui: &Ui, f: &Frame<'_>, input: &mut Input, bg: &Response, cells: &[CellOut], command: bool, pending: &mut Pending) {
    let content_y = |p: Pos2| f.offset + f64::from(p.y - f.area.min.y);
    if bg.drag_started() {
        input.band = None;
        let origin = ui.input(|i| i.pointer.press_origin());
        if let Some(o) = origin.filter(|o| !cells.iter().any(|c| c.rect.contains(*o)))
            && let Some(section) = f.layout.index_at(content_y(o)).and_then(|i| f.layout.section(i))
        {
            let sel = &f.state.selection;
            let base = if command && sel.section == Some(section) { sel.ids.clone() } else { Vec::new() };
            input.band = Some(Band { section, start: (o.x - f.area.min.x, content_y(o)), base, hits: None });
        }
    }
    if !bg.dragged() || bg.drag_stopped() {
        input.band = None;
        return;
    }
    let (Some(b), Some(pos)) = (input.band.as_mut(), ui.input(|i| i.pointer.interact_pos())) else { return };
    let (Some(index), Some(section)) = (f.layout.index_of(b.section), f.sections.get(b.section)) else {
        input.band = None;
        return;
    };
    let hits = selection::band(f.layout, index, (b.start.0, pos.x - f.area.min.x), (b.start.1, content_y(pos)));
    if b.hits.as_ref() != Some(&hits) {
        let mut seen: HashSet<PhotoId> = b.base.iter().copied().collect();
        let mut ids = b.base.clone();
        ids.extend(hits.iter().filter_map(|&k| section.photos.get(k).copied()).filter(|id| seen.insert(*id)));
        pending.select(&ids, "replace", b.section);
        b.hits = Some(hits);
    }
    let start = pos2(f.area.min.x + b.start.0, f.screen_y(b.start.1));
    let rect = Rect::from_two_pos(start, pos);
    let t = f.t;
    ui.painter().rect(rect, 0, t.accent.gamma_multiply(0.18), Stroke::new(1.0, t.accent), StrokeKind::Inside);
    let dt = ui.input(|i| i.stable_dt);
    pending.scroll = autoscroll(f, index, section.photos.len(), pos.y, dt);
}

/// How a band held at `y` in section `index` (of `count` photos) scrolls the view this frame (`dt`
/// seconds long): never past the section's ends, so it stays inside the one section it selects in.
fn autoscroll(f: &Frame<'_>, index: usize, count: usize, y: f32, dt: f32) -> Option<Autoscroll> {
    let speed = autoscroll_speed(y, f.area.min.y, f.area.max.y);
    let top = f.layout.top(index)?;
    if speed == 0.0 || !dt.is_finite() {
        return None;
    }
    let step = f64::from(speed * dt.clamp(1.0 / 120.0, 0.1));
    let limit = if step > 0.0 { top + f.layout.section_height(count) - f64::from(f.view_h()) } else { top };
    Some(Autoscroll { step, limit })
}

/// Runs what [`finish`] collected: the commands, the start of a drag, then the photo menu (built
/// for the selection the commands left).
pub fn apply(app: &mut App, pending: Pending) {
    for a in pending.actions {
        let _ = app.run(&a.command, &a.params);
    }
    if let Some(DragEvent::Start { from, ids }) = pending.drag {
        crate::drag::start(app, from, ids);
    }
    if let Some(r) = pending.menu {
        super::context_menu::show(&r, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_scrolls_only_near_an_edge_and_faster_farther_out() {
        let (top, bottom) = (100.0, 500.0);
        assert_eq!(autoscroll_speed(300.0, top, bottom), 0.0);
        assert_eq!(autoscroll_speed(bottom - AUTOSCROLL_EDGE - 1.0, top, bottom), 0.0, "short of the edge line");
        let (near, far) = (autoscroll_speed(bottom + 1.0, top, bottom), autoscroll_speed(bottom + 60.0, top, bottom));
        assert!(near > 0.0 && far > near, "{near} {far}");
        assert_eq!(autoscroll_speed(bottom + 10_000.0, top, bottom), AUTOSCROLL_MAX, "capped");
        assert_eq!(autoscroll_speed(top - 60.0, top, bottom), -far, "the same going up");
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(autoscroll_speed(bad, top, bottom), 0.0);
            assert_eq!(autoscroll_speed(300.0, bad, bottom), 0.0);
        }
        // A view shorter than its edge lines still scrolls one way only.
        assert!(autoscroll_speed(105.0, 100.0, 106.0).is_finite());
    }

    #[test]
    fn an_autoscroll_stops_at_its_limit_and_never_goes_back() {
        let down = Autoscroll { step: 10.0, limit: 100.0 };
        assert_eq!(down.apply(50.0), 60.0);
        assert_eq!(down.apply(95.0), 100.0);
        assert_eq!(down.apply(120.0), 120.0, "already past the limit: stays");
        let up = Autoscroll { step: -10.0, limit: 40.0 };
        assert_eq!(up.apply(80.0), 70.0);
        assert_eq!(up.apply(45.0), 40.0);
        assert_eq!(up.apply(20.0), 20.0);
        assert_eq!(Autoscroll { step: f64::NAN, limit: 0.0 }.apply(5.0), 5.0);
        assert_eq!(Autoscroll { step: 1.0, limit: f64::INFINITY }.apply(5.0), 5.0);
    }
}
