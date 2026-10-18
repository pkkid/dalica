//! The crosshair of the focal effects (`editing.md` §8.1: Soft Focus, Focal B&W, Graduated Tint,
//! Focal Zoom): drawn over the photo, a draggable centre with an optional circle of a given
//! radius (the sharp area) and an optional second handle (Graduated Tint's start and angle).
//!
//! It works in its own normalised coordinates: (0, 0) is the top-left of the rect it is given
//! (the photo as shown), (1, 1) its bottom-right, and a radius is a share of the rect's shorter
//! side, so the caller maps them to the stored frame (reserved decision 11) and nothing here
//! depends on the zoom. Both points stay on the rect: the crosshair cannot be put on a frame.

use egui::{Id, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::{Target, register};
use crate::Tokens;
use crate::registry::Role;

/// How far each arm of the cross reaches from the centre, in points.
pub const CROSSHAIR_ARM: f32 = 10.0;
/// The side of the square round the centre that takes a drag, in points.
pub const CROSSHAIR_GRAB: f32 = 24.0;
/// The radius of the second handle's disc, in points (its grab square is twice as wide plus 8).
pub const CROSSHAIR_HANDLE: f32 = 5.0;

/// The small ring at the centre, inside the arms.
const RING: f32 = 3.5;

/// Where the crosshair is, in its own normalised coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crosshair {
    /// The centre, (0, 0) the rect's top-left and (1, 1) its bottom-right.
    pub centre: [f32; 2],
    /// A circle round the centre, as a share of the rect's shorter side; `None` for none.
    pub radius: Option<f32>,
    /// The second handle (a start and an angle: the line from the centre to it); `None` for none.
    pub handle: Option<[f32; 2]>,
}

/// What a [`crosshair`] did this pass.
#[derive(Debug, Clone)]
pub struct CrosshairOut {
    /// The centre's new place, when it was dragged.
    pub centre: Option<[f32; 2]>,
    /// The handle's new place, when it was dragged.
    pub handle: Option<[f32; 2]>,
    /// A drag began: open an undo step (one per drag).
    pub drag_started: bool,
    /// It ended.
    pub drag_stopped: bool,
    /// The centre's response.
    pub response: Response,
}

/// The automation id of the second handle of crosshair `id`: `<id>/handle`.
pub fn crosshair_handle_id(id: &str) -> String {
    format!("{id}/handle")
}

/// The angle of the line from `centre` to `handle` as it shows on `rect`, in degrees: 0 points
/// right, 90 down (y grows downwards), in -180 … 180. 0 when the two points meet or a value is
/// not finite.
pub fn handle_angle(rect: Rect, centre: [f32; 2], handle: [f32; 2]) -> f32 {
    let d = vec2((handle[0] - centre[0]) * rect.width(), (handle[1] - centre[1]) * rect.height());
    if !(d.x.is_finite() && d.y.is_finite()) || d.length() <= 0.0 { 0.0 } else { d.y.atan2(d.x).to_degrees() }
}

/// A normalised coordinate kept on the rect: NaN is the middle.
fn on_rect(v: f32) -> f32 {
    if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) }
}

fn point(p: [f32; 2]) -> [f32; 2] {
    p.map(on_rect)
}

/// Follows a drag of the point `at` (normalised) taken by `r`: the offset between the pointer and
/// the point is kept from the drag's start, so the point never jumps under the pointer. Gives the
/// point's new place when it moved, and notes the drag's start and end.
fn follow(
    ui: &Ui,
    r: &Response,
    at: [f32; 2],
    started: &mut bool,
    stopped: &mut bool,
    to_screen: impl Fn([f32; 2]) -> Pos2,
    to_norm: impl Fn(Pos2) -> [f32; 2],
) -> Option<[f32; 2]> {
    let grab = r.id.with("grab");
    if r.drag_started() {
        *started = true;
        if let Some(p) = r.interact_pointer_pos() {
            ui.data_mut(|d| d.insert_temp(grab, to_screen(at) - p));
        }
    }
    if r.drag_stopped() {
        *stopped = true;
        ui.data_mut(|d| d.remove::<Vec2>(grab));
    }
    if !(r.dragged() || r.drag_started()) {
        return None;
    }
    let offset = ui.data(|d| d.get_temp::<Vec2>(grab)).unwrap_or(Vec2::ZERO);
    let moved = to_norm(r.interact_pointer_pos()? + offset);
    (moved != at).then_some(moved)
}

/// The crosshair over `rect` (the photo as shown): a cross (four arms to [`CROSSHAIR_ARM`] round a
/// small ring) at the centre, the circle when `c.radius` is set, and the handle (a
/// [`CROSSHAIR_HANDLE`] disc joined to the centre by a line) when `c.handle` is set. Each stroke is
/// `crosshair` over a wider `crosshair_outer` edge, so it reads on any photo; the part under the
/// pointer or dragged turns `accent`. Dragging the centre or the handle moves it with the pointer
/// (kept on the rect) and reports its new place in normalised coordinates. Registers `id`
/// (`crosshair:effect`, role `other`, active while dragged) on the centre and
/// [`crosshair_handle_id`] on the handle.
pub fn crosshair(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, c: &Crosshair) -> CrosshairOut {
    let enabled = ui.is_enabled();
    let size = vec2(rect.width().max(1.0), rect.height().max(1.0));
    let to_screen = |p: [f32; 2]| pos2(rect.min.x + p[0] * rect.width(), rect.min.y + p[1] * rect.height());
    let to_norm = |p: Pos2| point([(p.x - rect.min.x) / size.x, (p.y - rect.min.y) / size.y]);
    let centre = point(c.centre);
    let handle = c.handle.map(point);
    let sense = if enabled { Sense::drag() } else { Sense::hover() };

    // The centre first, then the handle: the later one is on top where they meet.
    let centre_id = Id::new(id);
    let cr = ui.interact(Rect::from_center_size(to_screen(centre), Vec2::splat(CROSSHAIR_GRAB)), centre_id, sense);
    let hid = crosshair_handle_id(id);
    let hr = handle.map(|h| ui.interact(Rect::from_center_size(to_screen(h), Vec2::splat(2.0 * CROSSHAIR_HANDLE + 8.0)), Id::new(&hid), sense));

    let mut out = CrosshairOut { centre: None, handle: None, drag_started: false, drag_stopped: false, response: cr.clone() };
    if enabled {
        if let (Some(r), Some(h)) = (&hr, handle) {
            out.handle = follow(ui, r, h, &mut out.drag_started, &mut out.drag_stopped, to_screen, to_norm);
        }
        out.centre = follow(ui, &cr, centre, &mut out.drag_started, &mut out.drag_stopped, to_screen, to_norm);
    }
    let centre = out.centre.unwrap_or(centre);
    let handle = out.handle.or(handle);

    // Paint: the dark edges first, then the light strokes over them.
    let hot = |r: &Response| enabled && (r.hovered() || r.dragged());
    let p = ui.painter_at(rect.expand(CROSSHAIR_GRAB));
    let at = to_screen(centre);
    let arms = [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)];
    let radius = c.radius.map(|r| if r.is_finite() { r.max(0.0) } else { 0.0 } * rect.width().min(rect.height())).filter(|r| *r > 0.0);
    for edge in [true, false] {
        // The edge pass draws everything `crosshair_outer` and wider; the light pass `crosshair`,
        // or `accent` for the part under the pointer.
        let ink = |r: Option<&Response>| {
            if edge {
                t.crosshair_outer
            } else if r.is_some_and(hot) {
                t.accent
            } else {
                t.crosshair
            }
        };
        let (thin, thick) = if edge { (3.0, 3.5) } else { (1.0, 1.5) };
        if let Some(radius) = radius {
            p.circle_stroke(at, radius, Stroke::new(thin, ink(None)));
        }
        if let Some(h) = handle {
            p.line_segment([at, to_screen(h)], Stroke::new(thin, ink(hr.as_ref())));
        }
        for d in arms {
            p.line_segment([at + d * RING, at + d * CROSSHAIR_ARM], Stroke::new(thick, ink(Some(&cr))));
        }
        p.circle_stroke(at, RING, Stroke::new(thick, ink(Some(&cr))));
        if let Some(h) = handle {
            p.circle_filled(to_screen(h), if edge { CROSSHAIR_HANDLE + 1.0 } else { CROSSHAIR_HANDLE - 0.5 }, ink(hr.as_ref()));
        }
    }
    if hot(&cr) || hr.as_ref().is_some_and(hot) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    register(ui.ctx(), id, Role::Other, "Crosshair", Target::Widget(&cr), cr.dragged());
    cr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, enabled, "Crosshair"));
    if let Some(r) = &hr {
        register(ui.ctx(), &hid, Role::Other, "Angle", Target::Widget(r), r.dragged());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, enabled, "Angle"));
    }
    out
}
