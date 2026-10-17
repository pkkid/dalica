//! Painting one layer of a slideshow frame (`viewing.md` §1.3): M11.4's geometry gives each photo
//! its rectangle (the photo stretched to it), its opacity and its clip (a rectangle, and for
//! Circle a circle too); here that becomes one textured mesh. The area a layer may paint is a convex
//! polygon (its rectangle within its clip rectangle, cut by the circle when there is one, drawn as a
//! [`CIRCLE_SEGMENTS`]-gon), fanned into triangles whose texture coordinates place the photo at its
//! rectangle. So every transition, Pan and Zoom's crop included, paints the same way, and a circle
//! needs no clip egui does not have.

use dalica_create::slideshow::{Layer, Point, Rect as SlideRect};
use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Painter, Pos2, TextureId, pos2};

/// The sides of the polygon that draws Circle's circle.
pub const CIRCLE_SEGMENTS: usize = 96;

/// Keeps the part of the convex polygon `poly` on the side of one edge where `inside` holds;
/// `cross` is where the segment between two points meets that edge.
fn clip_edge(poly: &[Point], inside: impl Fn(Point) -> bool, cross: impl Fn(Point, Point) -> Point) -> Vec<Point> {
    let mut out = Vec::with_capacity(poly.len() + 4);
    let Some(&last) = poly.last() else { return out };
    let mut prev = last;
    for &p in poly {
        match (inside(prev), inside(p)) {
            (true, true) => out.push(p),
            (true, false) => out.push(cross(prev, p)),
            (false, true) => {
                out.push(cross(prev, p));
                out.push(p);
            }
            (false, false) => {}
        }
        prev = p;
    }
    out
}

/// Where the segment `a`–`b` crosses `x = at` (or `y = at` with `vertical` false).
fn cross_at(a: Point, b: Point, at: f32, x_axis: bool) -> Point {
    let (da, db) = if x_axis { (a.x, b.x) } else { (a.y, b.y) };
    let span = db - da;
    let t = if span.abs() > f32::EPSILON { ((at - da) / span).clamp(0.0, 1.0) } else { 0.0 };
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// The convex polygon `poly` cut to the rectangle `r`.
fn clip_to_rect(poly: &[Point], r: &SlideRect) -> Vec<Point> {
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let p = clip_edge(poly, |p| p.x >= x0, |a, b| cross_at(a, b, x0, true));
    let p = clip_edge(&p, |p| p.x <= x1, |a, b| cross_at(a, b, x1, true));
    let p = clip_edge(&p, |p| p.y >= y0, |a, b| cross_at(a, b, y0, false));
    clip_edge(&p, |p| p.y <= y1, |a, b| cross_at(a, b, y1, false))
}

/// The corners of a rectangle, clockwise from the top left.
fn corners(r: &SlideRect) -> Vec<Point> {
    vec![Point::new(r.x, r.y), Point::new(r.right(), r.y), Point::new(r.right(), r.bottom()), Point::new(r.x, r.bottom())]
}

/// The area `layer` paints, in the screen's units: its rectangle within its clip rectangle, cut to
/// its circle when it has one. Empty when it paints nothing.
pub fn polygon(layer: &Layer) -> Vec<Point> {
    let area = layer.rect.intersect(&layer.clip.rect);
    if area.is_empty() {
        return Vec::new();
    }
    match layer.clip.circle {
        None => corners(&area),
        Some(c) if c.radius.is_finite() && c.radius > 0.0 => {
            let ring: Vec<Point> = (0..CIRCLE_SEGMENTS)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / CIRCLE_SEGMENTS as f32;
                    Point::new(c.centre.x + c.radius * a.cos(), c.centre.y + c.radius * a.sin())
                })
                .collect();
            let cut = clip_to_rect(&ring, &area);
            if cut.len() < 3 { Vec::new() } else { cut }
        }
        Some(_) => Vec::new(),
    }
}

/// The mesh that paints `layer` with `texture` (the whole photo) on a screen whose top-left corner
/// is `origin`; `None` when it paints nothing.
pub fn mesh(layer: &Layer, origin: Pos2, texture: TextureId) -> Option<Mesh> {
    let poly = polygon(layer);
    let opacity = if layer.opacity.is_finite() { layer.opacity.clamp(0.0, 1.0) } else { 0.0 };
    if poly.len() < 3 || opacity <= 0.0 || layer.rect.width <= 0.0 || layer.rect.height <= 0.0 {
        return None;
    }
    // Premultiplied white at the layer's opacity tints the photo's texels to that opacity.
    let color = Color32::WHITE.gamma_multiply(opacity);
    let mut mesh = Mesh::with_texture(texture);
    for p in &poly {
        let uv = pos2((p.x - layer.rect.x) / layer.rect.width, (p.y - layer.rect.y) / layer.rect.height);
        mesh.vertices.push(Vertex { pos: pos2(origin.x + p.x, origin.y + p.y), uv, color });
    }
    let n = u32::try_from(poly.len()).ok()?;
    for k in 1..n.saturating_sub(1) {
        mesh.add_triangle(0, k, k + 1);
    }
    Some(mesh)
}

/// Paints `layer` with `texture` on the screen whose top-left corner is `origin`.
pub fn paint(painter: &Painter, origin: Pos2, layer: &Layer, texture: TextureId) {
    if let Some(m) = mesh(layer, origin, texture) {
        painter.add(m);
    }
}

#[cfg(test)]
mod tests {
    use dalica_create::slideshow::transition::{Circle, Clip};

    use super::*;

    fn layer(rect: SlideRect, clip: SlideRect, circle: Option<Circle>) -> Layer {
        Layer { rect, opacity: 0.5, clip: Clip { rect: clip, circle } }
    }

    /// A layer paints its rectangle within its clip, the photo's texture placed at its rectangle
    /// (Pan and Zoom's crop, Push's half-way photo); a circle clip paints a polygon inside both the
    /// circle and the clip; a layer with no area paints nothing.
    #[test]
    fn layers_paint_their_rectangle_within_their_clip() {
        let l = layer(SlideRect::new(-100.0, 0.0, 400.0, 200.0), SlideRect::new(0.0, 0.0, 200.0, 200.0), None);
        let m = mesh(&l, pos2(10.0, 20.0), TextureId::default()).unwrap();
        let pos: Vec<(f32, f32)> = m.vertices.iter().map(|v| (v.pos.x, v.pos.y)).collect();
        assert_eq!(pos, [(10.0, 20.0), (210.0, 20.0), (210.0, 220.0), (10.0, 220.0)]);
        let uv: Vec<(f32, f32)> = m.vertices.iter().map(|v| (v.uv.x, v.uv.y)).collect();
        assert_eq!(uv, [(0.25, 0.0), (0.75, 0.0), (0.75, 1.0), (0.25, 1.0)], "the visible half of the photo");
        assert_eq!(m.indices.len(), 6);
        assert_eq!(m.vertices[0].color, Color32::WHITE.gamma_multiply(0.5));

        let screen = SlideRect::new(0.0, 0.0, 400.0, 300.0);
        let circle = Circle { centre: Point::new(200.0, 150.0), radius: 180.0 };
        let poly = polygon(&layer(screen, screen, Some(circle)));
        assert!(poly.len() > CIRCLE_SEGMENTS / 2);
        for p in &poly {
            assert!(screen.contains_rect(&SlideRect::new(p.x, p.y, 0.0, 0.0), 1e-3), "{p:?} inside the screen");
            assert!((p.x - 200.0).hypot(p.y - 150.0) <= 180.0 + 1e-2, "{p:?} inside the circle");
        }
        assert!(poly.iter().any(|p| p.y <= 1e-3) && poly.iter().any(|p| p.y >= 300.0 - 1e-3), "cut by the screen's edges");

        let none = layer(SlideRect::new(500.0, 0.0, 10.0, 10.0), screen, None);
        assert!(mesh(&none, Pos2::ZERO, TextureId::default()).is_none());
        let point = layer(screen, screen, Some(Circle { centre: Point::new(0.0, 0.0), radius: 0.0 }));
        assert!(polygon(&point).is_empty());
    }
}
