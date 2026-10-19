//! Pixel rectangles and scaled sizes on the collage's page.
//!
//! Ported from LightCraft `crates/engine/src/print/compose.rs` (`place`, `px_rect`) @ 20ccedf5,
//! MIT OR Apache-2.0 (`NOTICE`). Changed from the original: no quarter turn (a collage turns its
//! pictures by any angle as it draws them); sizes in `u32`; the rectangle is given in page pixels
//! (the original takes inches and a resolution), so `px_rect` only rounds and clips it.

/// A rectangle in page pixels, before rounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }
}

/// A rectangle of whole page pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The whole pixels `r` touches on a `pw × ph` page, clipped to the page; `None` when nothing of it
/// is on the page. Edges are rounded outwards, so every pixel a picture's edge passes through is
/// visited.
pub fn px_rect(r: &Rect, pw: u32, ph: u32) -> Option<PxRect> {
    if !r.is_finite() {
        return None;
    }
    let lo = |v: f64, max: u32| v.floor().clamp(0.0, f64::from(max)) as u32;
    let hi = |v: f64, max: u32| v.ceil().clamp(0.0, f64::from(max)) as u32;
    let (x0, y0, x1, y1) = (lo(r.x, pw), lo(r.y, ph), hi(r.x + r.w, pw), hi(r.y + r.h, ph));
    (x1 > x0 && y1 > y0).then_some(PxRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// The size a picture of `iw × ih` is scaled to for an `aw × ah` area: at least the area when
/// `fill` (the part outside is cropped), at most the area otherwise. `None` for an empty area or a
/// picture without a size.
pub fn place(iw: f64, ih: f64, aw: u32, ah: u32, fill: bool) -> Option<(u32, u32)> {
    if !(iw.is_finite() && ih.is_finite() && iw > 0.0 && ih > 0.0) || aw == 0 || ah == 0 {
        return None;
    }
    let (af, bf) = (f64::from(aw), f64::from(ah));
    let s = if fill { (af / iw).max(bf / ih) } else { (af / iw).min(bf / ih) };
    let (w, h) = (iw * s, ih * s);
    let clamp = |v: f64| v.clamp(1.0, f64::from(u32::MAX)) as u32;
    Some(if fill {
        (clamp((w - 1e-6).ceil()).max(aw), clamp((h - 1e-6).ceil()).max(ah))
    } else {
        (clamp(w.round()).min(aw), clamp(h.round()).min(ah))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px_rect_rounds_outwards_and_clips() {
        let r = Rect { x: -3.5, y: 2.25, w: 10.0, h: 4.5 };
        assert_eq!(px_rect(&r, 5, 100), Some(PxRect { x: 0, y: 2, w: 5, h: 5 }));
        assert_eq!(px_rect(&Rect { x: 10.0, y: 0.0, w: 2.0, h: 2.0 }, 5, 5), None);
        assert_eq!(px_rect(&Rect { x: f64::NAN, y: 0.0, w: 2.0, h: 2.0 }, 5, 5), None);
    }

    #[test]
    fn place_fits_and_fills() {
        assert_eq!(place(400.0, 300.0, 200, 200, false), Some((200, 150)));
        assert_eq!(place(400.0, 300.0, 200, 200, true), Some((267, 200)));
        assert_eq!(place(0.0, 300.0, 200, 200, true), None);
        assert_eq!(place(1.0, 1.0, 0, 200, true), None);
    }
}
