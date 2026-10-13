//! The Crop tool's maths on top of `dalica_edit::crop_geom` (M7.7) and the coordinate frame
//! (M7.1): the photo as the tool shows it (its whole picture, uncropped, in the displayed frame),
//! the ratio a size row locks, drawing a new rectangle from a press, finding the handle under the
//! pointer, and the round trip between the draft's stored `[x, y, w, h]` and the displayed frame.
//!
//! Everything here is pure and total: hostile numbers give a usable answer, never a panic.

use dalica_edit::crop_geom::{self, CropBox, Handle, MIN_CROP_FRACTION};
use dalica_edit::{Dihedral, Point, Rect};

/// The photo as the Crop tool shows it: its whole picture after the recipe's rotations and flips
/// (and straightened, which keeps the size), in pixels, and the turn from the stored frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub width: f64,
    pub height: f64,
    pub turn: Dihedral,
}

impl Shown {
    /// A photo of `width × height` pixels as shown, turned by `turn` from the stored frame; a
    /// missing or hostile size counts as 1 × 1 so the ratios stay finite.
    pub fn new(width: f64, height: f64, turn: Dihedral) -> Shown {
        let side = |v: f64| if v.is_finite() && v >= 1.0 { v } else { 1.0 };
        Shown { width: side(width), height: side(height), turn }
    }

    /// The photo's own ratio, width / height.
    pub fn aspect(&self) -> f64 {
        self.width / self.height
    }

    /// The displayed rectangle of a stored draft rectangle `[x, y, w, h]`.
    pub fn to_display(&self, rect: [f32; 4]) -> Rect {
        let [x, y, w, h] = rect.map(f64::from);
        Rect::new(x, y, x + w, y + h).to_display(self.turn)
    }

    /// The stored draft rectangle `[x, y, w, h]` of a displayed rectangle, inside the photo (an
    /// unusable rectangle gives the whole photo).
    pub fn to_stored(&self, rect: Rect) -> [f32; 4] {
        let r = unit(rect).to_stored(self.turn);
        let r = unit(r);
        let (x, y) = (r.x0 as f32, r.y0 as f32);
        // In f32, so that x + w and y + h stay inside the photo after the narrowing.
        let w = ((r.x1 as f32) - x).min(1.0 - x);
        let h = ((r.y1 as f32) - y).min(1.0 - y);
        if w > 0.0 && h > 0.0 { [x, y, w, h] } else { [0.0, 0.0, 1.0, 1.0] }
    }

    /// The size in pixels of a displayed rectangle (at least 1 × 1).
    pub fn pixels(&self, rect: Rect) -> (u32, u32) {
        let px = |v: f64| if v.is_finite() { v.round().clamp(1.0, f64::from(u32::MAX)) as u32 } else { 1 };
        (px((rect.x1 - rect.x0) * self.width), px((rect.y1 - rect.y0) * self.height))
    }

    /// Whether a displayed rectangle is wider than tall, in pixels.
    pub fn landscape(&self, rect: Rect) -> bool {
        (rect.x1 - rect.x0) * self.width >= (rect.y1 - rect.y0) * self.height
    }
}

/// `rect` ordered, clamped into the unit square; the whole square for non-finite corners.
pub fn unit(rect: Rect) -> Rect {
    let r = rect.ordered();
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
        return Rect::new(0.0, 0.0, 1.0, 1.0);
    }
    Rect::new(r.x0.clamp(0.0, 1.0), r.y0.clamp(0.0, 1.0), r.x1.clamp(0.0, 1.0), r.y1.clamp(0.0, 1.0))
}

/// The sides of a `"W:H"` ratio text, both finite and above 0.
pub fn sides(ratio: &str) -> Option<(f64, f64)> {
    let side = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0);
    let (w, h) = ratio.split_once(':')?;
    Some((side(w)?, side(h)?))
}

/// The ratio (width / height in pixels) that size row `ratio` locks on `shown`, `None` for Manual
/// (and for a text that is not a ratio). Sizes are orientation-free: the ratio is turned to the
/// rectangle drawn (`rect`, displayed), else to the photo, so "4 × 6" is 6:4 on a landscape photo
/// and Rotate turns it; "Current ratio" is the photo's own.
pub fn locked(ratio: &str, shown: &Shown, rect: Option<Rect>) -> Option<f64> {
    let base = match ratio {
        "manual" => return None,
        "current" => shown.aspect(),
        text => {
            let (w, h) = sides(text)?;
            w / h
        }
    };
    if !(base.is_finite() && base > 0.0) {
        return None;
    }
    let wide = rect.map_or(shown.width >= shown.height, |r| shown.landscape(r));
    let base = base.clamp(1.0 / crop_geom::MAX_RATIO, crop_geom::MAX_RATIO);
    Some(if (base >= 1.0) == wide || base == 1.0 { base } else { 1.0 / base })
}

/// The rectangle a size row draws: the largest of its ratio centred on the photo (Manual keeps
/// `rect`, or the whole photo when there is none).
pub fn chosen(ratio: &str, shown: &Shown, rect: Option<Rect>) -> Rect {
    match locked(ratio, shown, rect) {
        Some(a) => crop_geom::ratio_rect(shown.width, shown.height, a),
        None => rect.map_or(Rect::new(0.0, 0.0, 1.0, 1.0), unit),
    }
}

/// The rectangle drawn by a press at `anchor` dragged to `pointer` (both displayed, normalised):
/// from the anchor towards the pointer, in the locked ratio when there is one (as wide as the
/// pointer reaches either way), inside the photo, never smaller than the tools' minimum.
pub fn draw(anchor: Point, pointer: Point, shown: &Shown, aspect: Option<f64>) -> Rect {
    let (w, h) = (shown.width, shown.height);
    let fin = |v: f64, d: f64| if v.is_finite() { v } else { d };
    let (ax, ay) = (fin(anchor.x, 0.5).clamp(0.0, 1.0) * w, fin(anchor.y, 0.5).clamp(0.0, 1.0) * h);
    let (dx, dy) = (fin(pointer.x, 0.5).clamp(-1.0, 2.0) * w - ax, fin(pointer.y, 0.5).clamp(-1.0, 2.0) * h - ay);
    let (right, down) = (dx >= 0.0, dy >= 0.0);
    let room_x = if right { w - ax } else { ax };
    let room_y = if down { h - ay } else { ay };
    let min = MIN_CROP_FRACTION * w.min(h);
    let aspect = aspect.filter(|a| a.is_finite() && *a > 0.0).map(|a| a.clamp(1.0 / crop_geom::MAX_RATIO, crop_geom::MAX_RATIO));
    let (mut bw, mut bh) = match aspect {
        None => (dx.abs().min(room_x), dy.abs().min(room_y)),
        Some(a) => {
            let bw = dx.abs().max(dy.abs() * a).min(room_x).min(room_y * a);
            (bw, bw / a)
        }
    };
    if bw < min || bh < min {
        match aspect {
            None => (bw, bh) = (bw.max(min), bh.max(min)),
            // The shorter side at the minimum, the other in the ratio.
            Some(a) if a >= 1.0 => (bw, bh) = (min * a, min),
            Some(a) => (bw, bh) = (min, min / a),
        }
    }
    // A minimum frame can reach past the photo near an edge: it is pushed back inside (and a
    // locked one too large for the photo is shrunk to fit).
    if bw > w || bh > h {
        let k = (w / bw).min(h / bh);
        (bw, bh) = (bw * k, bh * k);
    }
    let x0 = if right { ax } else { ax - bw }.clamp(0.0, (w - bw).max(0.0));
    let y0 = if down { ay } else { ay - bh }.clamp(0.0, (h - bh).max(0.0));
    unit(Rect::new(x0 / w, y0 / h, (x0 + bw) / w, (y0 + bh) / h))
}

/// Drags `handle` of `start` (displayed) from `grab` to `pointer`: `crop_geom::drag` on the photo
/// as shown.
pub fn drag(start: Rect, handle: Handle, grab: Point, pointer: Point, shown: &Shown, aspect: Option<f64>) -> Rect {
    unit(crop_geom::drag(CropBox::new(start, 0.0), handle, grab, pointer, shown.width, shown.height, aspect).rect)
}

/// Rotate: `rect` (displayed) turned between portrait and landscape about its centre, kept inside.
pub fn rotate(rect: Rect, shown: &Shown) -> Rect {
    unit(crop_geom::rotate(CropBox::new(rect, 0.0), shown.width, shown.height).rect)
}

/// How near a corner (in points) grabs it.
pub const CORNER_GRAB: f32 = 10.0;
/// How near an edge (in points) grabs it.
pub const EDGE_GRAB: f32 = 6.0;

/// What a press at `p` takes hold of on the rectangle `r` (in screen points): a corner within
/// [`CORNER_GRAB`], an edge within [`EDGE_GRAB`], the inside (move); `None` outside (a new
/// rectangle is drawn).
pub fn hit(r: egui::Rect, p: egui::Pos2) -> Option<Handle> {
    if !(r.is_finite() && p.is_finite() && r.width() >= 0.0 && r.height() >= 0.0) {
        return None;
    }
    let corners = [
        (r.left_top(), Handle::TopLeft),
        (r.right_top(), Handle::TopRight),
        (r.right_bottom(), Handle::BottomRight),
        (r.left_bottom(), Handle::BottomLeft),
    ];
    if let Some((_, h)) = corners.iter().filter(|(c, _)| c.distance(p) <= CORNER_GRAB).min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p))) {
        return Some(*h);
    }
    let within_x = p.x >= r.min.x - EDGE_GRAB && p.x <= r.max.x + EDGE_GRAB;
    let within_y = p.y >= r.min.y - EDGE_GRAB && p.y <= r.max.y + EDGE_GRAB;
    let edges = [
        ((p.y - r.min.y).abs(), within_x, Handle::Top),
        ((p.x - r.max.x).abs(), within_y, Handle::Right),
        ((p.y - r.max.y).abs(), within_x, Handle::Bottom),
        ((p.x - r.min.x).abs(), within_y, Handle::Left),
    ];
    if let Some((_, _, h)) = edges.iter().filter(|(d, along, _)| *along && *d <= EDGE_GRAB).min_by(|a, b| a.0.total_cmp(&b.0)) {
        return Some(*h);
    }
    r.contains(p).then_some(Handle::Move)
}

/// The cursor a handle shows.
pub fn cursor(handle: Option<Handle>) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match handle {
        Some(Handle::TopLeft | Handle::BottomRight) => C::ResizeNwSe,
        Some(Handle::TopRight | Handle::BottomLeft) => C::ResizeNeSw,
        Some(Handle::Top | Handle::Bottom) => C::ResizeVertical,
        Some(Handle::Left | Handle::Right) => C::ResizeHorizontal,
        Some(Handle::Move) => C::Move,
        None => C::Crosshair,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANDSCAPE: Shown = Shown { width: 3000.0, height: 2000.0, turn: Dihedral::IDENTITY };

    fn px_ratio(r: Rect, s: &Shown) -> f64 {
        (r.x1 - r.x0) * s.width / ((r.y1 - r.y0) * s.height)
    }

    fn inside(r: Rect) -> bool {
        r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 1.0 && r.y1 <= 1.0 && r.x1 > r.x0 && r.y1 > r.y0
    }

    #[test]
    fn sizes_turn_to_the_photo_and_to_the_rectangle() {
        let portrait = Shown::new(2000.0, 3000.0, Dihedral::IDENTITY);
        assert_eq!(locked("manual", &LANDSCAPE, None), None);
        assert_eq!(locked("4:6", &LANDSCAPE, None), Some(1.5));
        assert_eq!(locked("4:6", &portrait, None), Some(4.0 / 6.0));
        assert_eq!(locked("16:9", &portrait, None), Some(9.0 / 16.0));
        assert_eq!(locked("1:1", &portrait, None), Some(1.0));
        assert_eq!(locked("current", &LANDSCAPE, None), Some(1.5));
        // A portrait rectangle on a landscape photo (after Rotate) keeps the ratio portrait.
        let tall = Rect::new(0.4, 0.1, 0.6, 0.9);
        assert_eq!(locked("4:3", &LANDSCAPE, Some(tall)), Some(0.75));
        for bad in ["", "x", "4:", ":3", "0:3", "-1:2", "inf:1", "NaN:1"] {
            assert_eq!(locked(bad, &LANDSCAPE, None), None, "{bad}");
        }
    }

    #[test]
    fn a_ratio_draws_its_largest_centred_rectangle() {
        let r = chosen("4:3", &LANDSCAPE, None);
        assert!((px_ratio(r, &LANDSCAPE) - 4.0 / 3.0).abs() < 1e-9 && inside(r));
        assert!((r.y1 - r.y0 - 1.0).abs() < 1e-9, "as tall as the photo");
        let kept = Rect::new(0.1, 0.2, 0.3, 0.4);
        assert_eq!(chosen("manual", &LANDSCAPE, Some(kept)), kept);
        assert_eq!(chosen("manual", &LANDSCAPE, None), Rect::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn drawing_follows_the_pointer_holds_the_ratio_and_stays_inside() {
        let a = Point::new(0.2, 0.2);
        let free = draw(a, Point::new(0.6, 0.9), &LANDSCAPE, None);
        assert!((free.x0 - 0.2).abs() < 1e-12 && (free.x1 - 0.6).abs() < 1e-12 && (free.y1 - 0.9).abs() < 1e-12);
        let up_left = draw(Point::new(0.8, 0.8), Point::new(0.5, 0.3), &LANDSCAPE, None);
        assert!((up_left.x0 - 0.5).abs() < 1e-12 && (up_left.y0 - 0.3).abs() < 1e-12 && (up_left.x1 - 0.8).abs() < 1e-12);
        for (p, q) in
            [((0.2, 0.2), (0.6, 0.9)), ((0.9, 0.1), (-3.0, 5.0)), ((0.5, 0.5), (0.5, 0.5)), ((1.0, 1.0), (2.0, 2.0)), ((0.0, 0.3), (0.0, 0.31))]
        {
            for aspect in [None, Some(4.0 / 3.0), Some(0.05), Some(20.0)] {
                let r = draw(Point::new(p.0, p.1), Point::new(q.0, q.1), &LANDSCAPE, aspect);
                assert!(inside(r), "{p:?} {q:?} {aspect:?}: {r:?}");
                let min = MIN_CROP_FRACTION * 2000.0;
                assert!((r.x1 - r.x0) * 3000.0 >= min - 1e-6 || (r.y1 - r.y0) * 2000.0 >= min - 1e-6, "{r:?}");
                if let Some(a) = aspect {
                    assert!((px_ratio(r, &LANDSCAPE) - a).abs() < 1e-6 * a.max(1.0), "{p:?} {q:?} {a}: {}", px_ratio(r, &LANDSCAPE));
                }
            }
        }
        for v in [f64::NAN, f64::INFINITY, -f64::INFINITY] {
            assert!(inside(draw(Point::new(v, v), Point::new(v, 0.3), &LANDSCAPE, Some(1.0))));
        }
    }

    #[test]
    fn stored_and_displayed_round_trip_on_a_turned_photo() {
        let turned = Shown::new(2000.0, 3000.0, Dihedral::ROTATE_90);
        let stored = [0.1, 0.2, 0.5, 0.25];
        let shown = turned.to_display(stored);
        let back = turned.to_stored(shown);
        for (a, b) in stored.iter().zip(back) {
            assert!((a - b).abs() < 1e-6, "{stored:?} {back:?}");
        }
        // A quarter turn clockwise: the stored top-left corner goes to the top right.
        assert!((shown.x1 - 0.8).abs() < 1e-6 && (shown.y0 - 0.1).abs() < 1e-6, "{shown:?}");
        assert_eq!(turned.to_stored(Rect::new(f64::NAN, 0.0, 1.0, 1.0)), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(turned.to_stored(Rect::new(0.5, 0.5, 0.5, 0.5)), [0.0, 0.0, 1.0, 1.0]);
        let edge = LANDSCAPE.to_stored(Rect::new(0.3, 0.3, 1.0, 1.0));
        assert!(edge[0] + edge[2] <= 1.0 && edge[1] + edge[3] <= 1.0);
    }

    #[test]
    fn a_press_finds_corners_then_edges_then_the_inside() {
        let r = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0));
        assert_eq!(hit(r, egui::pos2(103.0, 98.0)), Some(Handle::TopLeft));
        assert_eq!(hit(r, egui::pos2(300.0, 200.0)), Some(Handle::BottomRight));
        assert_eq!(hit(r, egui::pos2(200.0, 104.0)), Some(Handle::Top));
        assert_eq!(hit(r, egui::pos2(296.0, 150.0)), Some(Handle::Right));
        assert_eq!(hit(r, egui::pos2(150.0, 150.0)), Some(Handle::Move));
        assert_eq!(hit(r, egui::pos2(50.0, 150.0)), None);
        assert_eq!(hit(r, egui::pos2(f32::NAN, 150.0)), None);
    }

    #[test]
    fn rotate_and_pixels() {
        let r = crop_geom::ratio_rect(3000.0, 2000.0, 1.5);
        let turned = rotate(Rect::new(0.25, 0.25, 0.75, 0.75), &LANDSCAPE);
        assert!(!LANDSCAPE.landscape(turned) && inside(turned));
        assert_eq!(LANDSCAPE.pixels(r), (3000, 2000));
        assert_eq!(LANDSCAPE.pixels(Rect::new(0.0, 0.0, 0.0, 0.0)), (1, 1));
    }
}
