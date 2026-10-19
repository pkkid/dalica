//! Drawing on the collage's page: a turned rectangle visited pixel by pixel in its own frame,
//! pictures sampled bilinearly, the shadow in closed form, and the two blends (over, and the equal
//! weights of Multiple Exposure).
//!
//! The shadow is the closed form `edit`'s Drop Shadow renders (M12.14, `edit::ops::mats`, after
//! PhotoCraft's `shadow_map` and `tent_kernel` @ e4b2868a): a rectangle blurred by a tent, which is
//! separable, so its coverage at a point is the product of two one-dimensional profiles. Here the
//! profiles run along the turned picture's own axes, so a turned picture's shadow turns with it.

use dalica_image::Rgba8;

use super::compose::{PxRect, Rect, px_rect};

/// A rectangle in a frame's own coordinates: `u` to the right and `v` down, in page pixels, about
/// the frame's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}

impl Local {
    pub fn grown(self, by: f64) -> Local {
        Local { u0: self.u0 - by, v0: self.v0 - by, u1: self.u1 + by, v1: self.v1 + by }
    }

    pub fn width(&self) -> f64 {
        self.u1 - self.u0
    }

    pub fn height(&self) -> f64 {
        self.v1 - self.v0
    }

    fn is_finite(&self) -> bool {
        self.u0.is_finite() && self.v0.is_finite() && self.u1.is_finite() && self.v1.is_finite()
    }
}

/// A picture's frame on the page: its centre in page pixels and its turn (clockwise, `y` down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub cx: f64,
    pub cy: f64,
    cos: f64,
    sin: f64,
}

impl Frame {
    pub fn new(cx: f64, cy: f64, degrees: f64) -> Frame {
        let degrees = if degrees.is_finite() { degrees } else { 0.0 };
        let (sin, cos) = degrees.to_radians().sin_cos();
        Frame { cx, cy, cos, sin }
    }

    /// The same frame moved by `(dx, dy)` page pixels.
    pub fn moved(self, dx: f64, dy: f64) -> Frame {
        Frame { cx: self.cx + dx, cy: self.cy + dy, ..self }
    }

    /// The page point of the local point `(u, v)`.
    fn to_page(self, u: f64, v: f64) -> (f64, f64) {
        (self.cx + self.cos * u - self.sin * v, self.cy + self.sin * u + self.cos * v)
    }

    /// The page pixels `rect` touches, clipped to a `width × height` page.
    fn bounds(self, rect: Local, width: u32, height: u32) -> Option<PxRect> {
        if !rect.is_finite() {
            return None;
        }
        let corners = [(rect.u0, rect.v0), (rect.u1, rect.v0), (rect.u0, rect.v1), (rect.u1, rect.v1)].map(|(u, v)| self.to_page(u, v));
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (x, y) in corners {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        // One pixel more each way for the anti-aliased edge.
        px_rect(&Rect { x: x0 - 1.0, y: y0 - 1.0, w: x1 - x0 + 2.0, h: y1 - y0 + 2.0 }, width, height)
    }
}

/// How a painted colour joins the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Premultiplied colour over the opaque page.
    Over,
    /// Multiple Exposure: each picture weighs the same at each pixel. The page's alpha counts the
    /// pictures that pixel has had so far (saturating at 255: past that, each further picture
    /// weighs 1/255).
    Mean,
}

/// Paints `rect` of `frame` onto `page`: `colour(u, v)` answers the premultiplied colour (0–255)
/// at each local point inside it; with `edge`, the rectangle's edge is anti-aliased over one pixel.
pub fn paint(page: &mut Rgba8, frame: Frame, rect: Local, edge: bool, blend: Blend, mut colour: impl FnMut(f64, f64) -> [f32; 4]) {
    let Some(b) = frame.bounds(rect, page.width, page.height) else { return };
    let width = page.width as usize;
    for y in b.y..b.y + b.h {
        let dy = f64::from(y) + 0.5 - frame.cy;
        let x0 = f64::from(b.x) + 0.5 - frame.cx;
        let (mut u, mut v) = (frame.cos * x0 + frame.sin * dy, -frame.sin * x0 + frame.cos * dy);
        let row = y as usize * width;
        for x in b.x..b.x + b.w {
            let (pu, pv) = (u, v);
            u += frame.cos;
            v -= frame.sin;
            let cover = if edge {
                let cu = ((pu - rect.u0).min(rect.u1 - pu) + 0.5).clamp(0.0, 1.0);
                let cv = ((pv - rect.v0).min(rect.v1 - pv) + 0.5).clamp(0.0, 1.0);
                cu * cv
            } else if pu >= rect.u0 && pu < rect.u1 && pv >= rect.v0 && pv < rect.v1 {
                1.0
            } else {
                0.0
            };
            if cover <= 0.0 {
                continue;
            }
            let c = colour(pu, pv);
            let k = cover as f32;
            let i = (row + x as usize) * 4;
            let Some(px): Option<&mut [u8; 4]> = page.data.get_mut(i..i + 4).and_then(|s| s.try_into().ok()) else { continue };
            match blend {
                Blend::Over => {
                    let a = (c[3] * k / 255.0).clamp(0.0, 1.0);
                    if a <= 0.0 && c[..3].iter().all(|&v| v <= 0.0) {
                        continue;
                    }
                    for (d, s) in px.iter_mut().zip(c).take(3) {
                        *d = (s * k + f32::from(*d) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                    }
                }
                Blend::Mean => {
                    let a = (c[3] * k / 255.0).clamp(0.0, 1.0);
                    if a < 0.5 {
                        // An edge or a transparent part: no vote here.
                        continue;
                    }
                    let n = f32::from(px[3].saturating_add(1));
                    for (d, s) in px.iter_mut().zip(c).take(3) {
                        let straight = s / a.max(1e-6);
                        let old = f32::from(*d);
                        *d = (old + (straight - old) / n).round().clamp(0.0, 255.0) as u8;
                    }
                    px[3] = px[3].saturating_add(1);
                }
            }
        }
    }
}

/// A picture sampled bilinearly: the part `x0..x0 + sw × y0..y0 + sh` of the image (in its pixels)
/// stretched over a local rectangle.
pub struct Sampler<'a> {
    img: &'a Rgba8,
    rect: Local,
    x0: f64,
    y0: f64,
    /// Image pixels per local pixel.
    kx: f64,
    ky: f64,
}

impl<'a> Sampler<'a> {
    /// `shown` is the part of `img` that covers `rect`, normalised to the image (left, top, right,
    /// bottom).
    pub fn new(img: &'a Rgba8, shown: [f64; 4], rect: Local) -> Option<Sampler<'a>> {
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let [l, t, r, b] = shown;
        let (rw, rh) = (rect.width(), rect.height());
        if img.width == 0 || img.height == 0 || !(rw > 0.0 && rh > 0.0) {
            return None;
        }
        let (kx, ky) = ((r - l) * w / rw, (b - t) * h / rh);
        (kx.is_finite() && ky.is_finite()).then_some(Sampler { img, rect, x0: l * w, y0: t * h, kx, ky })
    }

    /// The premultiplied colour (0–255) at the local point `(u, v)`.
    pub fn at(&self, u: f64, v: f64) -> [f32; 4] {
        let fx = self.x0 + (u - self.rect.u0) * self.kx - 0.5;
        let fy = self.y0 + (v - self.rect.v0) * self.ky - 0.5;
        let (maxx, maxy) = (f64::from(self.img.width - 1), f64::from(self.img.height - 1));
        let (fx, fy) = (fx.clamp(0.0, maxx), fy.clamp(0.0, maxy));
        let (ix, iy) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - ix) as f32, (fy - iy) as f32);
        let (ix, iy) = (ix as usize, iy as usize);
        let (jx, jy) = ((ix + 1).min(maxx as usize), (iy + 1).min(maxy as usize));
        let w = self.img.width as usize;
        let tap = |x: usize, y: usize| -> [f32; 4] {
            let i = (y * w + x) * 4;
            match self.img.data.get(i..i + 4) {
                Some(&[r, g, b, a]) => {
                    let k = f32::from(a) / 255.0;
                    [f32::from(r) * k, f32::from(g) * k, f32::from(b) * k, f32::from(a)]
                }
                _ => [0.0; 4],
            }
        };
        let (p00, p10, p01, p11) = (tap(ix, iy), tap(jx, iy), tap(ix, jy), tap(jx, jy));
        let mut out = [0.0f32; 4];
        for (c, o) in out.iter_mut().enumerate() {
            let top = p00[c] + (p10[c] - p00[c]) * tx;
            let bottom = p01[c] + (p11[c] - p01[c]) * tx;
            *o = top + (bottom - top) * ty;
        }
        out
    }
}

/// The share of a unit step at 0 blurred by a tent of half-width `w` that lies at or left of `t`
/// (as `edit`'s Drop Shadow; a hard step when `w` is nearly 0).
fn tent_cdf(t: f64, w: f64) -> f64 {
    if w <= 1e-9 {
        return if t >= 0.0 { 1.0 } else { 0.0 };
    }
    let u = (t / w).clamp(-1.0, 1.0);
    if u < 0.0 { 0.5 * (1.0 + u) * (1.0 + u) } else { 1.0 - 0.5 * (1.0 - u) * (1.0 - u) }
}

/// The coverage of the interval `lo..hi` blurred by a tent of half-width `w`, at `t`.
fn blurred(t: f64, lo: f64, hi: f64, w: f64) -> f64 {
    (tent_cdf(t - lo, w) - tent_cdf(t - hi, w)).clamp(0.0, 1.0)
}

/// The shadow of `card` (in `frame`): the card moved by `(dx, dy)` page pixels and blurred by a
/// tent of half-width `blur`, black at `opacity` at its densest.
pub fn shadow(page: &mut Rgba8, frame: Frame, card: Local, (dx, dy): (f64, f64), blur: f64, opacity: f32) {
    let at = frame.moved(dx, dy);
    paint(page, at, card.grown(blur + 1.0), false, Blend::Over, |u, v| {
        let a = blurred(u, card.u0, card.u1, blur) * blurred(v, card.v0, card.v1, blur);
        [0.0, 0.0, 0.0, 255.0 * opacity * a as f32]
    });
}

/// A blank page: `colour` everywhere (opaque), or for Multiple Exposure all zeros (no picture yet).
pub fn page(width: u32, height: u32, colour: Option<[u8; 3]>) -> Option<Rgba8> {
    let n = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    let mut data = Vec::new();
    data.try_reserve_exact(n).ok()?;
    match colour {
        Some([r, g, b]) => {
            data.extend_from_slice(&[r, g, b, 255]);
            // Doubling copies: a few dozen memcpys for any page.
            while data.len() < n {
                let more = (n - data.len()).min(data.len());
                data.extend_from_within(..more);
            }
        }
        None => data.resize(n, 0),
    }
    Some(Rgba8 { width, height, data })
}

/// The end of Multiple Exposure: pixels no picture reached become black, and the page opaque.
pub fn settle(page: &mut Rgba8) {
    for px in page.data.as_chunks_mut::<4>().0 {
        if px[3] == 0 {
            px[..3].fill(0);
        }
        px[3] = 255;
    }
}
