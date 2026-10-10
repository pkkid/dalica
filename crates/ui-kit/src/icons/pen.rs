//! A pen on the 20 × 20 icon grid: every drawing call takes grid coordinates, the pen maps them
//! into the icon's square and strokes with the icon stroke width.
//!
//! Straight axis-aligned strokes go through egui's line segments and rectangles, which the
//! tessellator snaps to pixel centres, so they stay crisp at every size; other open strokes get
//! round caps (a dot of half the stroke width at each end) when the colour is opaque.

use egui::epaint::RectShape;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};

#[derive(Clone, Copy)]
pub(super) struct Pen<'a> {
    painter: &'a Painter,
    /// The icon's square.
    rect: Rect,
    /// Points per grid unit.
    k: f32,
    stroke: Stroke,
    /// Mirror the drawing left to right (the clockwise twin of a counterclockwise glyph).
    mirror: bool,
}

impl<'a> Pen<'a> {
    pub(super) fn new(painter: &'a Painter, rect: Rect, color: Color32) -> Self {
        let side = rect.width().min(rect.height());
        Pen { painter, rect, k: side / 20.0, stroke: Stroke::new(super::stroke_width(side), color), mirror: false }
    }

    /// The same pen drawing left to right mirrored.
    pub(super) fn mirrored(&self) -> Self {
        Pen { mirror: !self.mirror, ..*self }
    }

    /// A grid point in screen coordinates.
    pub(super) fn pt(&self, x: f32, y: f32) -> Pos2 {
        let x = if self.mirror { 20.0 - x } else { x };
        pos2(self.rect.min.x + x * self.k, self.rect.min.y + y * self.k)
    }

    fn pts(&self, pts: &[(f32, f32)]) -> Vec<Pos2> {
        pts.iter().map(|&(x, y)| self.pt(x, y)).collect()
    }

    pub(super) fn color(&self) -> Color32 {
        self.stroke.color
    }

    /// The same pen in another colour.
    pub(super) fn tinted(&self, color: Color32) -> Self {
        Pen { stroke: Stroke::new(self.stroke.width, color), ..*self }
    }

    /// A round cap. Skipped for translucent colours, where the overlap with the stroke would show.
    fn cap(&self, p: Pos2) {
        if self.color().is_opaque() {
            self.painter.circle_filled(p, 0.5 * self.stroke.width, self.color());
        }
    }

    /// A straight stroke from `a` to `b`; round caps unless it is horizontal or vertical.
    pub(super) fn seg(&self, a: (f32, f32), b: (f32, f32)) {
        let (pa, pb) = (self.pt(a.0, a.1), self.pt(b.0, b.1));
        self.painter.line_segment([pa, pb], self.stroke);
        if a.0 != b.0 && a.1 != b.1 {
            self.cap(pa);
            self.cap(pb);
        }
    }

    /// A polyline of horizontal and vertical strokes, each snapped to pixels like [`Pen::seg`],
    /// with a square of the stroke width filling every inner corner (snapped the same way), so the
    /// corners stay square instead of notched.
    pub(super) fn elbow(&self, pts: &[(f32, f32)]) {
        for w in pts.windows(2) {
            if let &[a, b] = w {
                self.seg(a, b);
            }
        }
        let ppp = self.painter.pixels_per_point();
        for &(x, y) in pts.iter().skip(1).take(pts.len().saturating_sub(2)) {
            let mut c = self.pt(x, y);
            self.stroke.round_center_to_pixel(ppp, &mut c.x);
            self.stroke.round_center_to_pixel(ppp, &mut c.y);
            let corner = Rect::from_center_size(c, egui::vec2(self.stroke.width, self.stroke.width));
            self.painter.add(RectShape::filled(corner, 0.0, self.color()).with_round_to_pixels(false));
        }
    }

    /// An open polyline with round caps.
    pub(super) fn line(&self, pts: &[(f32, f32)]) {
        let v = self.pts(pts);
        if let (Some(&first), Some(&last)) = (v.first(), v.last()) {
            self.cap(first);
            self.cap(last);
        }
        self.painter.add(Shape::line(v, self.stroke));
    }

    /// A closed outline.
    pub(super) fn closed(&self, pts: &[(f32, f32)]) {
        self.painter.add(Shape::closed_line(self.pts(pts), self.stroke));
    }

    /// A filled convex polygon.
    pub(super) fn fill(&self, pts: &[(f32, f32)]) {
        self.painter.add(Shape::convex_polygon(self.pts(pts), self.color(), Stroke::NONE));
    }

    /// A circle outline of grid radius `r`.
    pub(super) fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_stroke(self.pt(x, y), r * self.k, self.stroke);
    }

    /// A filled disc of grid radius `r`.
    pub(super) fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_filled(self.pt(x, y), r * self.k, self.color());
    }

    /// A rectangle outline centred on its edges, corner radius `round` in grid units.
    pub(super) fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, round: f32) {
        let r = Rect::from_two_pos(self.pt(x0, y0), self.pt(x1, y1));
        self.painter.rect_stroke(r, round * self.k, self.stroke, StrokeKind::Middle);
    }

    /// An open arc around (`cx`, `cy`) from `a0` to `a1` degrees (0° = right, 90° = down), with
    /// round caps.
    pub(super) fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
        self.line(&arc_points(cx, cy, r, a0, a1));
    }
}

/// Points along an arc in grid units, one every 10° or less (at least two).
pub(super) fn arc_points(cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    let n = ((a1 - a0).abs() / 10.0).ceil();
    let n = if n.is_finite() { n.clamp(1.0, 48.0) as usize } else { 1 };
    (0..=n)
        .map(|i| {
            let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}
