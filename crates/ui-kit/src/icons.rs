//! Icons drawn in code on a 20 × 20 design grid (`design-system.md` §5): our own drawings, tinted
//! by the caller, stroke `max(1.5·k, 1)` where `k = side / 20`. No icon fonts, no bitmaps.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A filled five-pointed star: the photo is starred.
    Star,
    /// The same star as an outline: not starred.
    StarOutline,
    /// A plus sign: add something.
    Plus,
}

/// The five outer and five inner points of the star on the 20 × 20 grid, mapped into `rect`.
fn star_points(rect: Rect) -> Vec<Pos2> {
    let k = rect.width().min(rect.height()) / 20.0;
    let c = rect.center() + egui::vec2(0.0, 0.6 * k);
    let (outer, inner) = (8.6 * k, 3.7 * k);
    (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { outer } else { inner };
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// The stroke width of an icon drawn in a square of `side` points.
pub fn stroke_width(side: f32) -> f32 {
    (1.5 * side / 20.0).max(1.0)
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    if !(side.is_finite() && side > 0.0) {
        return;
    }
    let rect = Rect::from_center_size(rect.center(), egui::vec2(side, side));
    let k = side / 20.0;
    match icon {
        Icon::Star => {
            let points = star_points(rect);
            // The star is concave: fill it as a fan around its centre, then stroke the outline thinly
            // so its edges are anti-aliased like egui's own shapes.
            let mut mesh = Mesh::default();
            mesh.colored_vertex(rect.center() + egui::vec2(0.0, 0.6 * k), color);
            for &p in &points {
                mesh.colored_vertex(p, color);
            }
            for i in 0..10u32 {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::closed_line(points, Stroke::new((0.6 * k).max(0.5), color)));
        }
        Icon::StarOutline => {
            painter.add(Shape::closed_line(star_points(rect.shrink(0.5 * stroke_width(side))), Stroke::new(stroke_width(side), color)));
        }
        Icon::Plus => {
            let (c, r) = (rect.center(), 6.0 * k);
            let stroke = Stroke::new(stroke_width(side), color);
            painter.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_star_fits_its_square_and_scales() {
        for side in [12.0, 20.0, 28.0] {
            let rect = Rect::from_min_size(pos2(5.0, 7.0), egui::vec2(side, side));
            let points = star_points(rect);
            assert_eq!(points.len(), 10);
            assert!(points.iter().all(|p| rect.expand(0.01).contains(*p)), "{side}: {points:?}");
        }
        assert_eq!(stroke_width(12.0), 1.0);
        assert_eq!(stroke_width(20.0), 1.5);
    }
}
