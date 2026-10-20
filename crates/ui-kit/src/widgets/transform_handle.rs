//! The selected picture's outline and its turn-and-scale wheel (`creating.md` §3, the Picture
//! Pile): the `accent` outline of the picture at its angle, and a wheel handle on its top-right
//! corner. Dragging the wheel turns and scales the picture together round its centre: the angle
//! follows the pointer's angle round the centre, the scale its distance from it; Shift keeps the
//! scale and only turns.
//!
//! It works on the screen: the caller gives the picture's rect as shown (unturned, at its current
//! scale) and its angle, and gets back the new angle and scale; the placement's own coordinates
//! stay the caller's (the engine's collage document).

use egui::{Id, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use super::{Target, access, focus_ring, register};
use crate::Tokens;
use crate::registry::Role;
use crate::text::{self, Text};

/// The radius of the wheel handle, in points (its grab square is twice as wide plus 8).
pub const TRANSFORM_WHEEL: f32 = 8.0;
/// The smallest scale a drag gives.
pub const TRANSFORM_MIN_SCALE: f32 = 0.05;
/// The largest scale a drag gives.
pub const TRANSFORM_MAX_SCALE: f32 = 20.0;
/// The width of the outline, in points.
pub const TRANSFORM_OUTLINE: f32 = 2.0;

/// The selected picture as shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformHandle {
    /// The picture's rect on the screen, unturned, at its current scale.
    pub rect: Rect,
    /// Its angle in degrees, clockwise on the screen (y grows downwards).
    pub angle: f32,
    /// Its scale (1 the layout's size).
    pub scale: f32,
}

/// What a [`transform_handle`] did this pass.
#[derive(Debug, Clone)]
pub struct TransformOut {
    /// The new angle in degrees (-180 … 180), while the wheel is dragged.
    pub angle: Option<f32>,
    /// The new scale, while the wheel is dragged.
    pub scale: Option<f32>,
    /// A drag began: open an undo step (one per drag).
    pub drag_started: bool,
    /// It ended.
    pub drag_stopped: bool,
    /// The wheel's response.
    pub response: Response,
}

/// An angle in degrees brought into -180 … 180; a non-finite one is 0.
fn wrapped(deg: f32) -> f32 {
    if !deg.is_finite() {
        return 0.0;
    }
    let d = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if d == -180.0 { 180.0 } else { d }
}

/// A scale kept in [`TRANSFORM_MIN_SCALE`] … [`TRANSFORM_MAX_SCALE`]; a non-finite one is 1.
fn kept(scale: f32) -> f32 {
    if scale.is_finite() { scale.clamp(TRANSFORM_MIN_SCALE, TRANSFORM_MAX_SCALE) } else { 1.0 }
}

/// The corners of `rect` turned `angle` degrees clockwise round its centre: top-left, top-right,
/// bottom-right, bottom-left as they were before the turn. A non-finite angle is 0.
pub fn transform_corners(rect: Rect, angle: f32) -> [Pos2; 4] {
    let (s, c) = wrapped(angle).to_radians().sin_cos();
    let centre = rect.center();
    [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].map(|p| {
        let d = p - centre;
        centre + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
    })
}

/// Where the wheel handle sits: the turned picture's top-right corner.
pub fn transform_handle_pos(rect: Rect, angle: f32) -> Pos2 {
    transform_corners(rect, angle)[1]
}

/// The angle and scale a drag of the wheel from `grab` to `now` gives round `centre`, from the
/// picture's `angle` and `scale` when the drag began: the angle turns by the angle the pointer
/// swept round the centre, the scale grows by the ratio of its distances from it (kept when
/// `keep_scale`, Shift). A pointer on the centre, or a non-finite value, changes nothing.
pub fn turn_and_scale(centre: Pos2, grab: Pos2, now: Pos2, angle: f32, scale: f32, keep_scale: bool) -> (f32, f32) {
    let (a, b) = (grab - centre, now - centre);
    let usable = |v: Vec2| v.x.is_finite() && v.y.is_finite() && v.length() > 1e-3;
    if !(usable(a) && usable(b)) {
        return (wrapped(angle), kept(scale));
    }
    let swept = (b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees();
    let scale = if keep_scale { kept(scale) } else { kept(kept(scale) * b.length() / a.length()) };
    (wrapped(angle + swept), scale)
}

/// The outline of the selected picture `h` at its angle, [`TRANSFORM_OUTLINE`] `accent`
/// (`text_disabled` when disabled), and the wheel handle on its top-right corner: a
/// [`TRANSFORM_WHEEL`] disc in `crosshair_outer` with a `crosshair` rim and four spokes (`accent`
/// hovered or dragged; no wheel when disabled). Dragging the wheel reports the new angle and scale
/// ([`turn_and_scale`] from the drag's start, round the picture's centre); Shift keeps the scale.
/// Registers `id` on the wheel (role `other`, active while dragged).
pub fn transform_handle(ui: &mut Ui, t: &Tokens, id: &str, h: &TransformHandle) -> TransformOut {
    let enabled = ui.is_enabled();
    let rect = if h.rect.is_finite() && h.rect.is_positive() { h.rect } else { Rect::from_center_size(Pos2::ZERO, Vec2::ZERO) };
    let (angle, scale) = (wrapped(h.angle), kept(h.scale));
    let wheel_at = transform_handle_pos(rect, angle);
    let grab_rect = Rect::from_center_size(wheel_at, Vec2::splat(2.0 * TRANSFORM_WHEEL + 8.0));
    let r = ui.interact(grab_rect, Id::new(id), if enabled { Sense::drag() } else { Sense::hover() });
    let mut out = TransformOut { angle: None, scale: None, drag_started: false, drag_stopped: false, response: r.clone() };

    // The drag's start: the centre, where the pointer took the wheel, the angle and the scale.
    let start = r.id.with("start");
    if enabled {
        if r.drag_started() {
            out.drag_started = true;
            let grab = r.interact_pointer_pos().unwrap_or(wheel_at);
            let c = rect.center();
            ui.data_mut(|d| d.insert_temp(start, ([c.x, c.y], [grab.x, grab.y], angle, scale)));
        }
        if (r.dragged() || r.drag_started())
            && let Some(now) = r.interact_pointer_pos()
            && let Some((c, g, a0, s0)) = ui.data(|d| d.get_temp::<([f32; 2], [f32; 2], f32, f32)>(start))
        {
            let shift = ui.input(|i| i.modifiers.shift);
            let (a, s) = turn_and_scale(pos2(c[0], c[1]), pos2(g[0], g[1]), now, a0, s0, shift);
            if a != angle || s != scale {
                out.angle = Some(a);
                out.scale = Some(s);
            }
        }
        if r.drag_stopped() {
            out.drag_stopped = true;
            ui.data_mut(|d| d.remove::<([f32; 2], [f32; 2], f32, f32)>(start));
        }
    }

    // Paint at what was reported, so the outline follows the pointer in the same frame: the new
    // angle turns it, the new scale grows it round its centre.
    let shown_angle = out.angle.unwrap_or(angle);
    let grow = out.scale.map_or(1.0, |s| s / scale);
    let shown = Rect::from_center_size(rect.center(), rect.size() * grow);
    let p = ui.painter();
    let ink = if enabled { t.accent } else { t.text_disabled };
    p.add(Shape::closed_line(transform_corners(shown, shown_angle).to_vec(), Stroke::new(TRANSFORM_OUTLINE, ink)));
    if enabled {
        let at = transform_handle_pos(shown, shown_angle);
        let hot = r.hovered() || r.dragged();
        let rim = if hot { t.accent } else { t.crosshair };
        p.circle_filled(at, TRANSFORM_WHEEL + 1.0, t.crosshair_outer);
        p.circle_stroke(at, TRANSFORM_WHEEL - 1.0, Stroke::new(1.5, rim));
        let (s, c) = shown_angle.to_radians().sin_cos();
        for k in 0..4 {
            let (dx, dy) = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][k];
            let d = vec2(dx * c - dy * s, dx * s + dy * c);
            p.line_segment([at + d * 1.5, at + d * (TRANSFORM_WHEEL - 2.0)], Stroke::new(1.5, rim));
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Alias);
        }
        focus_ring(p, t, &r, Rect::from_center_size(at, Vec2::splat(2.0 * TRANSFORM_WHEEL)), TRANSFORM_WHEEL as u8);
    }
    // The angle and the scale as values: "Turned 15°, scaled 120 %".
    let texts = text::texts(ui.ctx());
    let name = texts.get(Text::TurnAndScale);
    let shown_scale = out.scale.unwrap_or(scale);
    let value =
        text::fill(texts.get(Text::TurnedScaled), &[("angle", &format!("{shown_angle:.0}")), ("scale", &format!("{:.0}", shown_scale * 100.0))]);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, enabled, name));
    access(&r, |node| node.set_value(value));
    register(ui.ctx(), id, Role::Other, name, Target::Widget(&r), r.dragged());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_wrap_and_scales_are_kept_in_range() {
        assert_eq!(wrapped(270.0), -90.0);
        assert_eq!(wrapped(-180.0), 180.0);
        assert_eq!(wrapped(f32::NAN), 0.0);
        assert_eq!(kept(0.0), TRANSFORM_MIN_SCALE);
        assert_eq!(kept(1e9), TRANSFORM_MAX_SCALE);
        assert_eq!(kept(f32::INFINITY), 1.0);
        let c = pos2(0.0, 0.0);
        assert_eq!(turn_and_scale(c, c, pos2(5.0, 5.0), 10.0, 2.0, false), (10.0, 2.0), "a grab on the centre changes nothing");
    }
}
