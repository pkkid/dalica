//! The colour picker's maths and meshes: the saturation / value field at a hue and the hue strip,
//! both ways (a point to its colour, a colour to its point), and the vertex-coloured grid that
//! paints them.
//!
//! Ported from PhotoCraft `crates/ui-egui/src/color_picker_ui.rs` (`field_color`, `locate`,
//! `grid_mesh`) at e4b2868a, MIT OR Apache-2.0: only the hue mode (the field is saturation across
//! and value down, the strip is the hue), every input clamped, and the colour maths from
//! `dalica_core::color`.

use dalica_core::color::{Hsv, Rgb, hsv_to_rgb, to_u8};
use egui::{Color32, Mesh, Rect, pos2};

/// A normalised coordinate kept in 0 … 1: NaN is 0.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// The colour at (`x`, `y`) of the field (each 0 … 1, `y` down) for `hue` (degrees): saturation
/// grows to the right, value falls downwards (white at the top-left, the pure hue at the
/// top-right, black along the bottom).
pub fn field_color(hue: f32, x: f32, y: f32) -> Rgb {
    hsv_to_rgb(hue, unit(x), 1.0 - unit(y))
}

/// Where `hsv` sits on the field: its saturation across, its value down from the top (the inverse
/// of [`field_color`] for that hue).
pub fn field_point(hsv: Hsv) -> (f32, f32) {
    (unit(hsv[1]), 1.0 - unit(hsv[2]))
}

/// The hue of the strip at `y` (0 at the top … 1 at the bottom): 360° at the top running down to
/// 0° at the bottom.
pub fn hue_at(y: f32) -> f32 {
    (1.0 - unit(y)) * 360.0
}

/// The strip's colour at `y`: its hue at full saturation and value.
pub fn hue_color(y: f32) -> Rgb {
    hsv_to_rgb(hue_at(y), 1.0, 1.0)
}

/// Where `hue` (degrees, any value: it wraps; not finite reads as 0) sits on the strip, 0 … 1 down.
pub fn hue_point(hue: f32) -> f32 {
    let h = if hue.is_finite() { hue.rem_euclid(360.0) } else { 0.0 };
    1.0 - h / 360.0
}

/// An sRGB colour as an opaque egui colour.
pub(super) fn c32(c: Rgb) -> Color32 {
    Color32::from_rgb(to_u8(c[0]), to_u8(c[1]), to_u8(c[2]))
}

/// A mesh over `rect` of `n` × `n` cells whose corners take `color(x, y)` (x and y 0 … 1), blended
/// across each cell.
pub(super) fn grid_mesh(rect: Rect, n: u32, color: impl Fn(f32, f32) -> Rgb) -> Mesh {
    let n = n.clamp(1, 64);
    let mut m = Mesh::default();
    let step = 1.0 / n as f32;
    for j in 0..=n {
        for i in 0..=n {
            let (x, y) = (i as f32 * step, j as f32 * step);
            m.colored_vertex(pos2(rect.left() + x * rect.width(), rect.top() + y * rect.height()), c32(color(x, y)));
        }
    }
    let w = n + 1;
    for j in 0..n {
        for i in 0..n {
            let a = j * w + i;
            m.add_triangle(a, a + 1, a + w + 1);
            m.add_triangle(a, a + w + 1, a + w);
        }
    }
    m
}
