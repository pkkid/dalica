//! The navigator (`editing.md` §3, a Dalica addition): while the photo is zoomed larger than the
//! photo area, a thumbnail of it at most [`MAX`] points on its longer side sits at the bottom right
//! of the area, above the zoom read-out, on a `toast_bg` plate. The part shown in the area is
//! outlined in `photo_focus` and the rest dimmed. A click or a drag on it moves the view there
//! (the point under the pointer to the area's centre). Registered as `navigator:edit`.

use dalica_ui_kit::Tokens;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Color32, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, pos2, vec2};

use super::canvas::View;
use super::zoom;
use crate::tl;

pub const ID: &str = "navigator:edit";
/// The thumbnail's longer side, in points.
pub const MAX: f32 = 120.0;
/// Between the navigator and the read-out under it.
const GAP: f32 = 8.0;
/// The plate around the thumbnail.
const PLATE: f32 = 3.0;

/// Where the navigator's thumbnail goes for `v`: its aspect, at most [`MAX`] on its longer side,
/// bottom-right of the area above the read-out.
pub fn rect(v: &View) -> Option<Rect> {
    let (w, h) = v.size;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let k = MAX / w.max(h);
    let size = vec2((w * k).max(8.0).round(), (h * k).max(8.0).round());
    let bottom = v.area.max.y - zoom::INSET - zoom::HEIGHT - GAP;
    let r = Rect::from_min_size(pos2(v.area.max.x - zoom::INSET - size.x, bottom - size.y), size);
    v.area.contains_rect(r.expand(PLATE)).then_some(r)
}

/// The part of the photo shown in the area, as fractions of the photo (0 … 1).
pub fn visible(v: &View) -> Rect {
    let seen = v.image.intersect(v.area);
    let (w, h) = (v.image.width(), v.image.height());
    if !(seen.is_positive() && w > 0.0 && h > 0.0) {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let f = |p: Pos2| pos2(((p.x - v.image.min.x) / w).clamp(0.0, 1.0), ((p.y - v.image.min.y) / h).clamp(0.0, 1.0));
    Rect::from_min_max(f(seen.min), f(seen.max))
}

/// Draws the navigator for `v` with the preview's texture `tex`; the image point (0 … 1) a click
/// or drag on it asks to centre, if any.
pub fn show(ui: &mut Ui, t: &Tokens, v: &View, tex: Option<TextureId>) -> Option<Pos2> {
    let thumb = rect(v)?;
    let painter = ui.painter_at(v.area);
    painter.rect_filled(thumb.expand(PLATE), 4, t.toast_bg);
    match tex {
        Some(id) => {
            painter.image(id, thumb, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(thumb, 0, t.placeholder);
        }
    }
    let part = visible(v);
    let to = |p: Pos2| thumb.min + vec2(p.x * thumb.width(), p.y * thumb.height());
    let shown = Rect::from_min_max(to(part.min), to(part.max));
    let dim = Color32::from_black_alpha(110);
    for r in [
        Rect::from_min_max(thumb.min, pos2(thumb.max.x, shown.min.y)),
        Rect::from_min_max(pos2(thumb.min.x, shown.max.y), thumb.max),
        Rect::from_min_max(pos2(thumb.min.x, shown.min.y), pos2(shown.min.x, shown.max.y)),
        Rect::from_min_max(pos2(shown.max.x, shown.min.y), pos2(thumb.max.x, shown.max.y)),
    ] {
        if r.is_positive() {
            painter.add(Shape::rect_filled(r, 0, dim));
        }
    }
    painter.rect_stroke(shown, 0, Stroke::new(1.5, t.photo_focus), StrokeKind::Inside);
    let r = ui.interact(thumb, Id::new(ID), Sense::click_and_drag());
    widgets::register(ui.ctx(), ID, Role::Other, &tl!("Navigator"), Target::Widget(&r), false);
    if r.clicked() || r.dragged() {
        let p = r.interact_pointer_pos()?;
        return Some(pos2(((p.x - thumb.min.x) / thumb.width()).clamp(0.0, 1.0), ((p.y - thumb.min.y) / thumb.height()).clamp(0.0, 1.0)));
    }
    None
}
