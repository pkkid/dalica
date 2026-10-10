//! The small marks some widgets draw as part of themselves (a dropdown's chevron, a search field's
//! magnifier and clear cross), on the 20 × 20 grid of `design-system.md` §5 with the icons' stroke.
//! They are parts of widgets, not icons a caller picks: the shell's icons are `crate::icons`.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

use crate::icons::stroke_width;

/// Which way a chevron points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chevron {
    Down,
    Right,
}

/// The square `rect` is centred in, its 20 × 20 grid scale, and the map from grid to points.
fn grid(rect: Rect) -> Option<(f32, impl Fn(f32, f32) -> Pos2)> {
    let (w, h) = (rect.width(), rect.height());
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 && rect.center().is_finite()) {
        return None;
    }
    let side = w.min(h);
    let k = side / 20.0;
    let origin = rect.center() - egui::vec2(side, side) / 2.0;
    Some((side, move |x: f32, y: f32| pos2(origin.x + x * k, origin.y + y * k)))
}

pub(crate) fn chevron(painter: &Painter, rect: Rect, dir: Chevron, color: Color32) {
    let Some((side, at)) = grid(rect) else { return };
    let points = match dir {
        Chevron::Down => vec![at(5.5, 7.5), at(10.0, 12.5), at(14.5, 7.5)],
        Chevron::Right => vec![at(7.5, 5.5), at(12.5, 10.0), at(7.5, 14.5)],
    };
    painter.add(Shape::line(points, Stroke::new(stroke_width(side), color)));
}

pub(crate) fn cross(painter: &Painter, rect: Rect, color: Color32) {
    let Some((side, at)) = grid(rect) else { return };
    let stroke = Stroke::new(stroke_width(side), color);
    painter.line_segment([at(6.0, 6.0), at(14.0, 14.0)], stroke);
    painter.line_segment([at(14.0, 6.0), at(6.0, 14.0)], stroke);
}

pub(crate) fn magnifier(painter: &Painter, rect: Rect, color: Color32) {
    let Some((side, at)) = grid(rect) else { return };
    let stroke = Stroke::new(stroke_width(side), color);
    let k = side / 20.0;
    painter.circle_stroke(at(8.5, 8.5), 5.0 * k, stroke);
    painter.line_segment([at(12.3, 12.3), at(16.0, 16.0)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(paint: impl Fn(&Painter)) -> usize {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui.painter()));
        out.textures_delta.clear();
        out.shapes.len()
    }

    #[test]
    fn marks_paint_inside_their_square_and_ignore_empty_rects() {
        let none = shapes(|_| {});
        let empty = shapes(|p| {
            for r in [Rect::NOTHING, Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(f32::NAN, 4.0))] {
                chevron(p, r, Chevron::Down, Color32::WHITE);
                cross(p, r, Color32::WHITE);
                magnifier(p, r, Color32::WHITE);
            }
        });
        assert_eq!(empty, none, "nothing painted for an empty rect");
        let r = Rect::from_min_size(pos2(10.0, 10.0), egui::vec2(20.0, 12.0));
        let all = shapes(|p| {
            chevron(p, r, Chevron::Right, Color32::WHITE);
            cross(p, r, Color32::WHITE);
            magnifier(p, r, Color32::WHITE);
        });
        assert_eq!(all - none, 5, "a polyline, two segments, a circle and a segment");
        let Some((side, at)) = grid(r) else { panic!("a square fits") };
        assert_eq!(side, 12.0);
        assert_eq!(at(0.0, 0.0), pos2(14.0, 10.0), "the square is centred in the rect");
        assert_eq!(at(20.0, 20.0), pos2(26.0, 22.0));
    }
}
