//! The guides the Crop and Straighten tools draw over the photo (`editing.md` §6.1–§6.2), as
//! polylines in the unit square of the rectangle they cover (`[0, 0]` its top-left corner,
//! `[1, 1]` its bottom-right corner): the rule-of-thirds lines inside a crop while it is dragged,
//! and the grid over the photo while it is straightened. The UI maps them onto the screen and dashes
//! them; nothing here depends on the window's size except the grid's spacing.
//!
//! Ported from LightCraft `crates/ui-egui/src/panels/crop_overlay.rs` (`paths`: the thirds) at
//! revision 76b53778 and PhotoCraft `crates/ui-egui/src/crop_overlay.rs` (`guides`: the thirds and
//! the bounded grid) at revision 32c918aa (MIT OR Apache-2.0; `NOTICE`). Dalica's changes: only the
//! two guides the tools draw, no overlay choice or orientation.

/// A guide polyline in the unit square of the rectangle it covers.
pub type Polyline = Vec<[f32; 2]>;

/// The grid's target spacing on screen, in points.
pub const GRID_PX: f32 = 32.0;
/// The most lines the grid draws along each axis, however large the photo is on screen.
pub const GRID_MAX: usize = 96;

fn segment(a: [f32; 2], b: [f32; 2]) -> Polyline {
    vec![a, b]
}

/// A vertical and a horizontal line at each fraction.
fn cross(fractions: &[f32]) -> Vec<Polyline> {
    fractions.iter().flat_map(|&f| [segment([f, 0.0], [f, 1.0]), segment([0.0, f], [1.0, f])]).collect()
}

/// The rule-of-thirds lines: two vertical and two horizontal.
pub fn thirds() -> Vec<Polyline> {
    cross(&[1.0 / 3.0, 2.0 / 3.0])
}

/// The straighten grid over a photo shown `width × height` points on screen: square cells of about
/// [`GRID_PX`], at least one and fewer than [`GRID_MAX`] lines along each axis. A size that is not
/// finite or under one point gives no lines.
pub fn grid(width: f32, height: f32) -> Vec<Polyline> {
    if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
        return Vec::new();
    }
    let axis = |len: f32, vertical: bool| -> Vec<Polyline> {
        // The float-to-int cast saturates, so a huge size is bounded by the clamp.
        let n = ((len / GRID_PX) as usize).clamp(2, GRID_MAX);
        (1..n)
            .map(|i| {
                let f = i as f32 / n as f32;
                if vertical { segment([f, 0.0], [f, 1.0]) } else { segment([0.0, f], [1.0, f]) }
            })
            .collect()
    };
    let mut lines = axis(width, true);
    lines.extend(axis(height, false));
    lines
}
