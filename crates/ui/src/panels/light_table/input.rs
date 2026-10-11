//! The light table's pointer input on cells and empty space (`ui-layout.md` §5.4–§5.5), on the
//! model of [`crate::selection`]: a click selects a photo, Ctrl-click toggles it (in another
//! section it replaces the selection), Shift-click selects the range from the anchor, a right-click
//! on an unselected photo selects it first, a click on empty space selects nothing, and a rubber
//! band dragged from empty space selects the photos it touches in the section it started in (with
//! Ctrl, added to the selection there). Pressing on a thumbnail and dragging never starts a band.
//! Every change goes through `select.set`, which makes the section's folder the tree's folder.
//!
//! **Boundary.** Two calls a frame and one after it. [`begin`] runs before any cell is laid out
//! (an interaction it creates on the empty space sits under the cells, so cells win their clicks);
//! [`finish`] runs after the last cell with the cells' [`CellOut`]s and what [`begin`] returned, and
//! collects what to do as [`Pending`]; the parent passes that to [`apply`] once the library is no
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

/// What the input keeps between frames.
#[derive(Debug, Default)]
pub struct Input {
    band: Option<Band>,
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

/// What this frame's input asks for, in order.
#[derive(Debug, Default)]
pub struct Pending {
    pub actions: Vec<Action>,
}

impl Pending {
    fn select(&mut self, ids: &[PhotoId], mode: &str) {
        let ids: Vec<u64> = ids.iter().map(|id| id.get()).collect();
        self.actions.push(Action { command: "select.set".into(), params: json!({ "ids": ids, "mode": mode }) });
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
        pending.select(&[c.photo], mode);
    } else if let Some(c) = cells.iter().find(|c| c.response.secondary_clicked() && !sel.contains(c.photo)) {
        pending.select(&[c.photo], "replace");
    }
    if let Some(bg) = background {
        band(ui, f, input, &bg, cells, command, &mut pending);
        if bg.clicked() && !command && !shift && !sel.ids.is_empty() {
            pending.actions.push(Action { command: "select.none".into(), params: json!({}) });
        }
    }
    pending
}

/// The rubber band: started by a drag from empty space, it selects the photos it touches in its
/// section each frame they change, and is painted over the cells.
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
        pending.select(&ids, "replace");
        b.hits = Some(hits);
    }
    let start = pos2(f.area.min.x + b.start.0, f.screen_y(b.start.1));
    let rect = Rect::from_two_pos(start, pos);
    let t = f.t;
    ui.painter().rect(rect, 0, t.accent.gamma_multiply(0.18), Stroke::new(1.0, t.accent), StrokeKind::Inside);
}

/// Runs what [`finish`] collected.
pub fn apply(app: &mut App, pending: Pending) {
    for a in pending.actions {
        let _ = app.run(&a.command, &a.params);
    }
}
