//! The RGB histogram of the edit panel (`design-system.md` §6, `editing.md` §5.2): red, green and
//! blue as translucent areas, additive where they overlap, normalised to the mean so tall spikes
//! clip. The caller gives the bins (any count up to [`MAX_BINS`]); the widget only draws them.
//!
//! The mesh painting is ported from PhotoCraft `crates/ui-egui/src/rgb_histogram.rs` at revision
//! 10fb9233 (MIT OR Apache-2.0; `NOTICE`): the display smoothing of quantisation combs, and the
//! spans split where two curves cross so every overlap has one exact colour whatever the paint
//! order. Dalica's changes: any number of bins instead of 256, the scale set by the mean (capped at
//! the peak) instead of the interior peak, Dalica's channel colours added together for overlaps
//! (`theme::histogram_fill`), the `canvas` plate, and an automation id.

use egui::{Color32, Mesh, Painter, Rect, Shape, Stroke, Ui, pos2};

use super::{Target, register};
use crate::Tokens;
use crate::registry::Role;
use crate::text::{self, Text};
use crate::theme::{HISTOGRAM, histogram_fill};

/// The most bins drawn per channel; more are ignored (a hostile caller cannot make the mesh huge).
pub const MAX_BINS: usize = 4096;

/// The plot's top is this many times the mean height of the bins that hold anything (never above
/// the tallest bin, plus a little room): a few tall spikes clip instead of flattening the rest.
pub const HISTOGRAM_HEADROOM: f32 = 4.0;

/// The heights to draw, 0 to 1, per channel: `bins` smoothed for display, on one scale for the
/// three channels. Channels of different lengths are padded with empty bins to the longest; at
/// most [`MAX_BINS`] bins each. Empty input gives empty curves.
pub fn histogram_curves(bins: [&[u64]; 3]) -> [Vec<f32>; 3] {
    let n = bins.iter().map(|b| b.len()).max().unwrap_or(0).min(MAX_BINS);
    let raw = |c: &[u64], i: usize| c.get(i).copied().unwrap_or(0) as f32;
    let mut curves: [Vec<f32>; 3] = bins.map(|c| {
        (0..n)
            .map(|i| {
                // A five-bin binomial kernel softens quantisation combs but cannot bridge a wide
                // empty range. The edge (clipping) bins are kept as they are and never smoothed
                // into their neighbours.
                if n < 5 || i == 0 || i + 1 == n {
                    return raw(c, i);
                }
                let sample = |j: usize| raw(c, j.clamp(1, n - 2));
                (sample(i.saturating_sub(2)) + 4.0 * sample(i - 1) + 6.0 * raw(c, i) + 4.0 * sample(i + 1) + sample(i + 2)) / 16.0
            })
            .collect()
    });
    // One scale for the three channels, from the interior bins (the edge spikes of clipped
    // shadows and highlights must not set it); a photo that is all black or all white uses its
    // edges.
    let interior = |c: &Vec<f32>| -> Vec<f32> { if n >= 3 { c.get(1..n - 1).map(<[f32]>::to_vec).unwrap_or_default() } else { c.clone() } };
    let mut values: Vec<f32> = curves.iter().flat_map(interior).filter(|v| v.is_finite() && *v > 0.0).collect();
    if values.is_empty() {
        values = curves.iter().flatten().copied().filter(|v| v.is_finite() && *v > 0.0).collect();
    }
    let peak = values.iter().copied().fold(0.0f32, f32::max);
    let mean = if values.is_empty() { 0.0 } else { values.iter().map(|&v| f64::from(v)).sum::<f64>() as f32 / values.len() as f32 };
    let scale = (peak * 1.1).min(HISTOGRAM_HEADROOM * mean);
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    for v in curves.iter_mut().flatten() {
        *v = if v.is_finite() { (*v / scale).clamp(0.0, 1.0) } else { 0.0 };
    }
    curves
}

fn quad(mesh: &mut Mesh, rect: Rect, x: [f32; 2], bottom: [f32; 2], top: [f32; 2], color: Color32) {
    // Bounded: at most MAX_BINS + 1 spans × 4 cuts × 3 bands × 4 vertices.
    let Ok(vertex) = u32::try_from(mesh.vertices.len()) else { return };
    for (x, height) in [(x[0], bottom[0]), (x[0], top[0]), (x[1], top[1]), (x[1], bottom[1])] {
        mesh.colored_vertex(pos2(x, rect.bottom() - height * rect.height()), color);
    }
    mesh.add_triangle(vertex, vertex + 1, vertex + 2);
    mesh.add_triangle(vertex, vertex + 2, vertex + 3);
}

/// One span between two bin centres, split where two curves cross, so every part has a stable
/// channel order: exact overlap colours without alpha stacking and without cracks between bars.
fn span(mesh: &mut Mesh, rect: Rect, x: [f32; 2], a: [f32; 3], b: [f32; 3]) {
    let mut cuts = [0.0f32, 1.0, 1.0, 1.0, 1.0];
    let mut n = 2;
    for (i, (ai, bi)) in a.iter().zip(&b).enumerate() {
        for (aj, bj) in a.iter().zip(&b).skip(i + 1) {
            let denominator = (bi - ai) - (bj - aj);
            if denominator.abs() > f32::EPSILON {
                let crossing = (aj - ai) / denominator;
                if crossing > 0.0
                    && crossing < 1.0
                    && let Some(cut) = cuts.get_mut(n)
                {
                    *cut = crossing;
                    n += 1;
                }
            }
        }
    }
    let Some(cuts) = cuts.get_mut(..n) else { return };
    cuts.sort_unstable_by(f32::total_cmp);
    for cut in cuts.windows(2) {
        let (&start, &end) = match cut {
            [s, e] => (s, e),
            _ => continue,
        };
        if end <= start {
            continue;
        }
        let left: [f32; 3] = std::array::from_fn(|c| a[c] + (b[c] - a[c]) * start);
        let right: [f32; 3] = std::array::from_fn(|c| a[c] + (b[c] - a[c]) * end);
        let mut levels = [(0usize, 1u8), (1, 2), (2, 4)];
        levels.sort_unstable_by(|(i, _), (j, _)| (left[*i] + right[*i]).total_cmp(&(left[*j] + right[*j])));
        let x = [x[0] + (x[1] - x[0]) * start, x[0] + (x[1] - x[0]) * end];
        let mut floor = [0.0; 2];
        let mut mask = 7;
        for (channel, bit) in levels {
            let top = [left[channel], right[channel]];
            if top[0] > floor[0] || top[1] > floor[1] {
                quad(mesh, rect, x, floor, top, histogram_fill(mask));
            }
            floor = top;
            mask &= !bit;
        }
    }
}

fn usable(rect: Rect) -> bool {
    rect.is_finite() && rect.is_positive() && rect.size().is_finite()
}

/// The filled areas of `curves` in `rect` (what [`histogram`] paints under the outlines).
pub fn histogram_mesh(rect: Rect, curves: &[Vec<f32>; 3]) -> Mesh {
    let mut mesh = Mesh::default();
    let n = curves.iter().map(Vec::len).min().unwrap_or(0);
    if !usable(rect) || n == 0 {
        return mesh;
    }
    let at = |c: usize, i: usize| curves.get(c).and_then(|v| v.get(i)).copied().unwrap_or(0.0);
    let mut previous_x = rect.left();
    let mut previous = [at(0, 0), at(1, 0), at(2, 0)];
    for i in 0..n {
        let x = rect.left() + (i as f32 + 0.5) / n as f32 * rect.width();
        let next = [at(0, i), at(1, i), at(2, i)];
        span(&mut mesh, rect, [previous_x, x], previous, next);
        previous_x = x;
        previous = next;
    }
    span(&mut mesh, rect, [previous_x, rect.right()], previous, previous);
    mesh
}

/// Paints the histogram of `bins` (red, green, blue) into `rect` on `painter`: the `canvas` plate,
/// the overlapping translucent areas, and a 1 pt outline per channel in its [`HISTOGRAM`] colour.
pub fn paint_histogram(painter: &Painter, t: &Tokens, rect: Rect, bins: [&[u64]; 3]) {
    if !usable(rect) {
        return;
    }
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(rect));
    painter.rect_filled(rect, 0, t.canvas);
    let curves = histogram_curves(bins);
    painter.add(histogram_mesh(rect, &curves));
    for (values, color) in curves.iter().zip(HISTOGRAM) {
        let (Some(&first), Some(&last)) = (values.first(), values.last()) else { continue };
        if !values.iter().any(|&v| v > 0.0) {
            continue;
        }
        let n = values.len() as f32;
        let mut points = Vec::with_capacity(values.len() + 2);
        points.push(pos2(rect.left(), rect.bottom() - first * rect.height()));
        for (i, v) in values.iter().enumerate() {
            points.push(pos2(rect.left() + (i as f32 + 0.5) / n * rect.width(), rect.bottom() - v * rect.height()));
        }
        points.push(pos2(rect.right(), rect.bottom() - last * rect.height()));
        painter.add(Shape::line(points, Stroke::new(1.0, color)));
    }
}

/// The histogram widget: [`paint_histogram`] into `rect`, registered as `id` (role `histogram`, a
/// painted part with no input of its own).
pub fn histogram(ui: &Ui, t: &Tokens, id: &str, rect: Rect, bins: [&[u64]; 3]) {
    paint_histogram(ui.painter(), t, rect, bins);
    if usable(rect) {
        register(ui.ctx(), id, Role::Histogram, &text::get(ui.ctx(), Text::Histogram), Target::Painted { rect, clip: ui.clip_rect() }, false);
    }
}

#[cfg(test)]
mod tests {
    //! Ported from PhotoCraft `crates/ui-egui/src/rgb_histogram.rs` at 10fb9233, with Dalica's
    //! colours and scale.
    use super::*;

    const BINS: usize = 256;

    fn color_at(mesh: &Mesh, point: egui::Pos2) -> Option<Color32> {
        mesh.indices.as_chunks::<3>().0.iter().find_map(|tri| {
            let a = mesh.vertices[tri[0] as usize].pos;
            let b = mesh.vertices[tri[1] as usize].pos;
            let c = mesh.vertices[tri[2] as usize].pos;
            let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
            if cross(b - a, c - a).abs() < 0.00001 {
                return None;
            }
            let sides = [cross(b - a, point - a), cross(c - b, point - b), cross(a - c, point - c)];
            (sides.iter().all(|&s| s >= -0.00001) || sides.iter().all(|&s| s <= 0.00001)).then_some(mesh.vertices[tri[0] as usize].color)
        })
    }

    fn mesh(rect: Rect, bins: &[Vec<u64>; 3]) -> Mesh {
        histogram_mesh(rect, &histogram_curves([&bins[0], &bins[1], &bins[2]]))
    }

    #[test]
    fn one_scale_and_exact_overlaps() {
        let h = [vec![3; BINS], vec![2; BINS], vec![1; BINS]];
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(256.0, 110.0));
        let m = mesh(rect, &h);
        for (y, mask) in [(100.0, 7), (60.0, 3), (20.0, 1)] {
            assert_eq!(color_at(&m, pos2(128.7, y)), Some(histogram_fill(mask)), "{y}");
        }
        assert!(m.vertices.iter().all(|v| rect.contains(v.pos)));
    }

    #[test]
    fn every_overlap_and_crossing_has_an_order_independent_colour() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(256.0, 100.0));
        let mut h = [vec![0u64; BINS], vec![0; BINS], vec![0; BINS]];
        for mask in 1..=7usize {
            for (c, bins) in h.iter_mut().enumerate() {
                bins[mask * 30..mask * 30 + 10].fill(u64::from(mask & (1 << c) != 0));
            }
        }
        let m = mesh(rect, &h);
        for mask in 1..=7u8 {
            assert_eq!(color_at(&m, pos2(f32::from(mask) * 30.0 + 5.5, 95.0)), Some(histogram_fill(mask)), "{mask}");
        }
        let mut m = Mesh::default();
        span(&mut m, rect, [0.0, 256.0], [1.0, 0.25, 0.75], [0.25, 1.0, 0.75]);
        for (x, y, mask) in [(25.6, 90.0, 7), (25.6, 50.0, 5), (25.6, 10.0, 1), (230.4, 50.0, 6), (230.4, 10.0, 2)] {
            assert_eq!(color_at(&m, pos2(x, y)), Some(histogram_fill(mask)));
        }
        // Triangles never stack: the covered area is the tallest curve's.
        let area: f32 = m
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|i| {
                let (a, b, c) = (m.vertices[i[0] as usize].pos, m.vertices[i[1] as usize].pos, m.vertices[i[2] as usize].pos);
                ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() * 0.5
            })
            .sum();
        assert!((area - 256.0 * 100.0 * (5.0 / 6.0)).abs() < 0.01, "{area}");
    }

    #[test]
    fn edge_spikes_neither_set_the_scale_nor_bleed_inwards() {
        let mut h = [vec![10u64; BINS], vec![10; BINS], vec![10; BINS]];
        h[0][0] = 1_000_000;
        h[1][255] = 1_000_000;
        let p = histogram_curves([&h[0], &h[1], &h[2]]);
        assert_eq!((p[0][0], p[1][255]), (1.0, 1.0));
        assert!((p[0][1] - 1.0 / 1.1).abs() < 0.0001, "{}", p[0][1]);
        assert!((p[1][254] - 1.0 / 1.1).abs() < 0.0001);
        let mut h = [vec![0u64; BINS], vec![0; BINS], vec![0; BINS]];
        h[0][255] = 100;
        let p = histogram_curves([&h[0], &h[1], &h[2]]);
        assert!(p[0][255] > 0.9, "an all-white photo uses its edge");
        assert_eq!(p[0][254], 0.0);
    }

    #[test]
    fn an_interior_spike_clips_at_the_mean_headroom() {
        // A flat histogram with one tall spike: the rest keeps its height instead of flattening.
        let mut h = [vec![100u64; BINS], vec![100; BINS], vec![100; BINS]];
        for c in &mut h {
            c[128] = 20_000;
        }
        let p = histogram_curves([&h[0], &h[1], &h[2]]);
        assert_eq!(p[0][128], 1.0, "the spike clips");
        // Scaled to the peak, the flat part would sit at 100 / (1.1 × 7 600) ≈ 0.012.
        assert!(p[0][40] > 0.1, "{}", p[0][40]);
        // A flat histogram is not affected: its peak sets the scale.
        let flat = [vec![50u64; BINS], vec![50; BINS], vec![50; BINS]];
        let p = histogram_curves([&flat[0], &flat[1], &flat[2]]);
        assert!((p[2][100] - 1.0 / 1.1).abs() < 1e-5);
    }

    #[test]
    fn quantisation_combs_are_smoothed_and_gaps_do_not_cut_bars() {
        let mut h = [vec![0u64; BINS], vec![0; BINS], vec![0; BINS]];
        for c in &mut h {
            for i in (30..220).step_by(3) {
                c[i] = 100;
            }
        }
        let p = histogram_curves([&h[0], &h[1], &h[2]]);
        let low = p[0][40..210].iter().copied().fold(f32::INFINITY, f32::min);
        let high = p[0][40..210].iter().copied().fold(0.0f32, f32::max);
        assert!(high - low < 0.2, "the comb still shows: {}", high - low);
        assert_eq!(p[0][240], 0.0, "a wide empty range stays empty");
        let mut h = [vec![0u64; BINS], vec![0; BINS], vec![0; BINS]];
        for c in &mut h {
            c[119] = 100;
            c[121] = 100;
        }
        let m = mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(256.0, 100.0)), &h);
        assert!(color_at(&m, pos2(120.5, 85.0)).is_some(), "one empty bin is no stripe");
        assert!(color_at(&m, pos2(180.5, 85.0)).is_none(), "a real empty range stays empty");
    }

    #[test]
    fn hostile_input_draws_nothing_or_stays_bounded() {
        let empty: [&[u64]; 3] = [&[], &[], &[]];
        assert!(histogram_curves(empty).iter().all(Vec::is_empty));
        assert!(histogram_mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)), &histogram_curves(empty)).is_empty());
        let zero = [vec![0u64; BINS], vec![0; BINS], vec![0; BINS]];
        assert!(mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(256.0, 100.0)), &zero).is_empty());
        let one = [vec![5u64; BINS], vec![5; BINS], vec![5; BINS]];
        assert!(mesh(Rect::NOTHING, &one).is_empty());
        assert!(mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::NAN, 10.0)), &one).is_empty());
        // Huge counts, uneven channel lengths, tiny bin counts and too many bins.
        let p = histogram_curves([&[u64::MAX, 1, u64::MAX], &[7], &[]]);
        assert!(p.iter().all(|c| c.len() == 3 && c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))));
        let many = vec![1u64; MAX_BINS * 4];
        assert!(histogram_curves([&many, &many, &many]).iter().all(|c| c.len() == MAX_BINS));
    }
}
