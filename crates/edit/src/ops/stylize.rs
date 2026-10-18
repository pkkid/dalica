//! The stylize effects of tab 5 (`editing.md` §8.4): `vignette(size,strength,color,fade)`,
//! `pixelate(size,mode,fade)` and `focal_zoom(x,y,size,amount,hardness,fade)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - Vignette: Size 0–50 (35), how far the darkening reaches in from the edges in percent of
//!     the half-diagonal; Strength 1–2 (1.4); the colour the edges go toward (`#000000`; the panel
//!     can pick it from the photo); Fade 0–100 (0). Repeatable.
//!   - Pixelate: Pixel Size 2–150 (20), a cell's side in thousandths of the picture's shorter side,
//!     cells anchored in the stored frame; the blend mode by name ([`BLENDS`]: `normal`,
//!     `multiply`, `screen`, `overlay`, `soft_light`; `normal` by default); Fade 0–100 (0).
//!   - Focal Zoom: the crosshair's centre `x,y`, normalised in the stored frame (`data-model.md`
//!     §3.3), (0.5, 0.5) by default; Focal Size 0–1 (0.3), the sharp circle's radius as a fraction
//!     of the shorter side; Zoom Amount 1–100 (50); Edge Hardness 0–100 (50); Fade 0–100 (0).
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field, a colour that is not
//!   `#rrggbb` or a blend mode that is not one of the words is refused (the entry is kept and
//!   skipped).
//! - **Rendering** (M12.12, each `VERSION` 1; Fade 100, Vignette's Size 0 and Pixelate's cells
//!   of one pixel in Normal mode change nothing):
//!   - **Vignette** (from LightCraft `crates/pipeline/src/finish.rs` (`vignette`) at 0da998f2: a
//!     smooth ramp of the distance from the centre, applied in linear light): the distance of a
//!     pixel from the output picture's centre (after the crop), as a fraction of its
//!     half-diagonal, ramps smoothly from 0 at `1 − Size/100` to 1 at the corners; the pixel moves
//!     toward the colour, in linear light, by that ramp times `Strength / 2` (so the corners go
//!     70 % of the way at the default 1.4, all the way at 2). A second Vignette compounds.
//!   - **Pixelate** (from PhotoCraft `crates/algo/src/stylize.rs` (`mosaic`) at e4b2868a: the mean
//!     of each cell, clipped at the picture's edges): square cells of `Pixel Size / 1000` of the
//!     stored photo's shorter side, square to the output and anchored at the stored frame's
//!     top-left corner, so the cells sit on the same content in a thumbnail, a zoomed region
//!     and a Save, and stay put when the crop changes; each cell's mean (in the working values,
//!     exact integer sums of the 8-bit pixels) is blended onto the picture by the mode, then
//!     faded. Cells are at least one output pixel.
//!   - **Focal Zoom** (from PhotoCraft `crates/algo/src/blur.rs` (`radial`, the zoom method) and
//!     `crates/algo/src/blur/radial.rs` at e4b2868a: samples along the ray toward the centre, up
//!     to `Zoom Amount / 200` of the way, bilinear): the centre is a stored-frame point, so it
//!     lands on the same content under any EXIF orientation, crop and straighten. The circle of
//!     Focal Size is untouched; outside it the zoom blur comes in over `(1 − Edge Hardness/100) ×
//!     0.5` of the shorter side ([`FOCAL_RAMP`]). The taps are spaced evenly in scale (`k =
//!     q^m`), so the blur is made in two passes, a fine one over the whole window and a coarse
//!     one per pixel, whose product is every one of `n₁ × n₂` taps (at most [`MAX_TAPS`]),
//!     about one per output pixel of the longest path: a 12 MP picture needs `2 × √N` taps a
//!     pixel rather than `N`.
//!
//!   The blend modes are M12.3's [`Blend`], Fade its [`fx::fade`]; lengths and points go through
//!   [`Space`] (`NOTICE`).

use std::fmt;
use std::sync::{Mutex, OnceLock};

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reach, Reader, UNIT, View, clamp, clamp_point, hex, num, num_point};
use crate::frame::Point;
use crate::fx::{self, Blend, Circle, Plane, Space};

/// The range of Vignette's Size, percent of the half-diagonal.
pub const VIGNETTE_SIZE: (f64, f64) = (0.0, 50.0);
/// The range of Vignette's Strength.
pub const VIGNETTE_STRENGTH: (f64, f64) = (1.0, 2.0);
/// The range of Pixelate's Pixel Size, thousandths of the shorter side.
pub const PIXEL_SIZE: (f64, f64) = (2.0, 150.0);
/// The range of Focal Zoom's Focal Size: the sharp circle's radius as a fraction of the shorter
/// side.
pub const FOCAL_SIZE: (f64, f64) = UNIT;
/// The range of Focal Zoom's Zoom Amount.
pub const ZOOM_AMOUNT: (f64, f64) = (1.0, 100.0);
/// The range of Focal Zoom's Edge Hardness.
pub const HARDNESS: (f64, f64) = (0.0, 100.0);

/// Pixelate's blend modes by their words, in the order the panel lists them ([`Blend`], M12.3).
pub const BLENDS: [(&str, Blend); 5] = [
    ("normal", Blend::Normal),
    ("multiply", Blend::Multiply),
    ("screen", Blend::Screen),
    ("overlay", Blend::Overlay),
    ("soft_light", Blend::SoftLight),
];

/// The word an entry writes for blend mode `b`.
pub fn blend_word(b: Blend) -> &'static str {
    match b {
        Blend::Normal => "normal",
        Blend::Multiply => "multiply",
        Blend::Screen => "screen",
        Blend::Overlay => "overlay",
        Blend::SoftLight => "soft_light",
    }
}

/// `vignette(size,strength,color,fade)`: edges darkened, or taken toward a colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vignette {
    pub size: f64,
    pub strength: f64,
    pub color: [u8; 3],
    pub fade: f64,
}

impl Default for Vignette {
    fn default() -> Vignette {
        Vignette { size: 35.0, strength: 1.4, color: [0, 0, 0], fade: 0.0 }
    }
}

/// `pixelate(size,mode,fade)`: a mosaic of square cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pixelate {
    /// Thousandths of the shorter side.
    pub size: f64,
    pub mode: Blend,
    pub fade: f64,
}

impl Default for Pixelate {
    fn default() -> Pixelate {
        Pixelate { size: 20.0, mode: Blend::Normal, fade: 0.0 }
    }
}

/// `focal_zoom(x,y,size,amount,hardness,fade)`: a radial zoom blur outside a sharp circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocalZoom {
    pub centre: Point,
    pub size: f64,
    pub amount: f64,
    pub hardness: f64,
    pub fade: f64,
}

impl Default for FocalZoom {
    fn default() -> FocalZoom {
        FocalZoom { centre: Point { x: 0.5, y: 0.5 }, size: 0.3, amount: 50.0, hardness: 50.0, fade: 0.0 }
    }
}

impl fmt::Display for Vignette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{}", num(self.size), num(self.strength), hex(self.color), num(self.fade))
    }
}

impl Operation for Vignette {
    const NAME: &'static str = "vignette";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Vignette";

    fn parse(params: &str) -> Result<Vignette, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let v = Vignette {
            size: r.number("the size", VIGNETTE_SIZE)?,
            strength: r.number("the strength", VIGNETTE_STRENGTH)?,
            color: r.colour("the colour")?,
            fade: r.number("fade", FADE)?,
        };
        r.end()?;
        Ok(v)
    }

    fn clamped(self) -> Vignette {
        Vignette {
            size: clamp(self.size, VIGNETTE_SIZE),
            strength: clamp(self.strength, VIGNETTE_STRENGTH),
            color: self.color,
            fade: clamp(self.fade, FADE),
        }
    }
}

impl fmt::Display for Pixelate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.size), blend_word(self.mode), num(self.fade))
    }
}

impl Operation for Pixelate {
    const NAME: &'static str = "pixelate";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Pixelate";

    fn parse(params: &str) -> Result<Pixelate, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let p = Pixelate { size: r.number("the pixel size", PIXEL_SIZE)?, mode: r.choice("the blend mode", &BLENDS)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(p)
    }

    fn clamped(self) -> Pixelate {
        Pixelate { size: clamp(self.size, PIXEL_SIZE), mode: self.mode, fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for FocalZoom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{}", num_point(self.centre), num(self.size), num(self.amount), num(self.hardness), num(self.fade))
    }
}

impl Operation for FocalZoom {
    const NAME: &'static str = "focal_zoom";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Focal Zoom";

    fn parse(params: &str) -> Result<FocalZoom, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let z = FocalZoom {
            centre: r.point("the centre")?,
            size: r.number("the focal size", FOCAL_SIZE)?,
            amount: r.number("the zoom amount", ZOOM_AMOUNT)?,
            hardness: r.number("the edge hardness", HARDNESS)?,
            fade: r.number("fade", FADE)?,
        };
        r.end()?;
        Ok(z)
    }

    fn clamped(self) -> FocalZoom {
        FocalZoom {
            centre: clamp_point(self.centre),
            size: clamp(self.size, FOCAL_SIZE),
            amount: clamp(self.amount, ZOOM_AMOUNT),
            hardness: clamp(self.hardness, HARDNESS),
            fade: clamp(self.fade, FADE),
        }
    }
}

/// How far outside Focal Zoom's circle the blur comes in fully at Edge Hardness 0, relative to the
/// stored photo's shorter side (at 100 it starts at the circle's edge).
pub const FOCAL_RAMP: f64 = 0.5;
/// The most zoom taps a Focal Zoom pixel averages: two passes of at most 32.
pub const MAX_TAPS: usize = 1024;

// --- Vignette ---------------------------------------------------------------------------------

impl Adjust for Vignette {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let v = self.clamped();
        if v.size <= 0.0 || v.fade >= 100.0 || ctx.width == 0 || ctx.height == 0 {
            return None;
        }
        let (w, h) = (f64::from(ctx.width), f64::from(ctx.height));
        Some(Box::new(VignettePass {
            centre: (w / 2.0, h / 2.0),
            half: w.hypot(h) / 2.0,
            inner: 1.0 - v.size / 100.0,
            amount: (v.strength / 2.0) as f32,
            colour: v.color.map(|c| fx::to_linear(f32::from(c) / 255.0)),
            fade: v.fade,
        }))
    }
}

/// A prepared Vignette: the output picture's centre and half-diagonal in pixels, where the ramp
/// starts (a fraction of the half-diagonal), how far the corners go and the colour in linear light.
struct VignettePass {
    centre: (f64, f64),
    half: f64,
    inner: f64,
    amount: f32,
    colour: [f32; 3],
    fade: f64,
}

impl Pass for VignettePass {
    fn run(&self, band: &mut Band<'_>) {
        let (x0, y0) = (band.x0, band.y0);
        for_each_pixel(band.pixels, band.width, x0, y0, |x, y, px| {
            let d = (f64::from(x) + 0.5 - self.centre.0).hypot(f64::from(y) + 0.5 - self.centre.1) / self.half;
            let t = fx::smoothstep(self.inner, 1.0, d) as f32;
            if t <= 0.0 {
                return;
            }
            let a = (t * self.amount).clamp(0.0, 1.0);
            let lin = px.map(fx::to_linear);
            let out = std::array::from_fn(|c| fx::to_srgb(lin[c] + (self.colour[c] - lin[c]) * a).clamp(0.0, 1.0));
            *px = fx::fade(*px, out, self.fade);
        });
    }
}

// --- Pixelate ---------------------------------------------------------------------------------

impl Adjust for Pixelate {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let p = self.clamped();
        if p.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let side = space.length(p.size / 1000.0).max(1.0);
        // Cells of one pixel hold one pixel each: the mean is the pixel.
        if side <= 1.0 && p.mode == Blend::Normal {
            return None;
        }
        let origin = space.to_output(Point::new(0.0, 0.0));
        if !(origin.0.is_finite() && origin.1.is_finite() && side.is_finite()) {
            return None;
        }
        // A cell reaches at most its side from any of its pixels.
        let reach = u32::try_from(side.ceil() as u64).unwrap_or(u32::MAX).saturating_add(1);
        Some(Box::new(PixelatePass { origin, side, mode: p.mode, fade: p.fade, reach, cells: OnceLock::new() }))
    }
}

/// A prepared Pixelate: the cell lattice in output pixels (its origin, the stored frame's
/// top-left corner, and the side) and each cell's mean, made once per render.
struct PixelatePass {
    origin: (f64, f64),
    side: f64,
    mode: Blend,
    fade: f64,
    reach: u32,
    cells: OnceLock<Option<Cells>>,
}

/// The mean of every cell that meets a window, row-major from cell `(i0, j0)`.
struct Cells {
    i0: i64,
    j0: i64,
    nx: usize,
    means: Vec<[f32; 3]>,
}

impl Cells {
    fn get(&self, i: i64, j: i64) -> Option<[f32; 3]> {
        let (i, j) = (usize::try_from(i.checked_sub(self.i0)?).ok()?, usize::try_from(j.checked_sub(self.j0)?).ok()?);
        if i >= self.nx {
            return None;
        }
        self.means.get(j.checked_mul(self.nx)?.checked_add(i)?).copied()
    }
}

impl PixelatePass {
    /// The cell along one axis of the output pixel at `v` (its centre at `v + 0.5`).
    fn cell(&self, v: u32, origin: f64) -> i64 {
        ((f64::from(v) + 0.5 - origin) / self.side).floor() as i64
    }

    /// The mean of every cell that meets `view`'s window, over the cell's pixels inside the
    /// picture: exact integer sums, so a region's cells equal the whole render's.
    fn cells_of(&self, view: &View<'_>) -> Option<Cells> {
        let (w, h) = (view.width as usize, view.height as usize);
        let row_bytes = w.checked_mul(4)?;
        if w == 0 || h == 0 || view.data.len() < row_bytes.checked_mul(h)? {
            return None;
        }
        let cols: Vec<i64> = (0..view.width).map(|lx| self.cell(view.x0.saturating_add(lx), self.origin.0)).collect();
        let rows: Vec<i64> = (0..view.height).map(|ly| self.cell(view.y0.saturating_add(ly), self.origin.1)).collect();
        let (i0, i1, j0, j1) = (*cols.first()?, *cols.last()?, *rows.first()?, *rows.last()?);
        let nx = usize::try_from(i1.checked_sub(i0)?.checked_add(1)?).ok()?;
        let ny = usize::try_from(j1.checked_sub(j0)?.checked_add(1)?).ok()?;
        let mut means = alloc(nx.checked_mul(ny)?, [0.0f32; 3])?;
        let data = view.data;
        par_rows(&mut means, nx, |j, out| {
            let cj = j0.saturating_add(i64::try_from(j).unwrap_or(i64::MAX));
            let (start, end) = (rows.partition_point(|&r| r < cj), rows.partition_point(|&r| r <= cj));
            let mut sums = vec![[0u64; 4]; out.len()];
            for ly in start..end {
                let Some(row) = ly.checked_mul(row_bytes).and_then(|at| data.get(at..at.checked_add(row_bytes)?)) else { continue };
                for (px, ci) in row.as_chunks::<4>().0.iter().zip(&cols) {
                    let Some(s) = ci.checked_sub(i0).and_then(|i| usize::try_from(i).ok()).and_then(|i| sums.get_mut(i)) else { continue };
                    s[0] += u64::from(px[0]);
                    s[1] += u64::from(px[1]);
                    s[2] += u64::from(px[2]);
                    s[3] += 1;
                }
            }
            for (o, s) in out.iter_mut().zip(&sums) {
                if s[3] > 0 {
                    let n = s[3] as f64 * 255.0;
                    *o = [(s[0] as f64 / n) as f32, (s[1] as f64 / n) as f32, (s[2] as f64 / n) as f32];
                }
            }
        });
        Some(Cells { i0, j0, nx, means })
    }
}

impl Pass for PixelatePass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.reach)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(cells) = self.cells.get_or_init(|| self.cells_of(view)).as_ref() else { return };
        let (x0, y0) = (band.x0, band.y0);
        for_each_pixel(band.pixels, band.width, x0, y0, |x, y, px| {
            if let Some(top) = cells.get(self.cell(x, self.origin.0), self.cell(y, self.origin.1)) {
                *px = fx::fade(*px, self.mode.apply(*px, top), self.fade);
            }
        });
    }
}

// --- Focal Zoom -------------------------------------------------------------------------------

impl Adjust for FocalZoom {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let z = self.clamped();
        if z.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        // Untouched inside Focal Size; the blur comes in over the ramp outside it.
        let ramp = (1.0 - z.hardness / 100.0) * FOCAL_RAMP;
        let outer = z.size + ramp;
        let circle = Circle::new(&space, z.centre, outer, if outer > 0.0 { ramp / outer } else { 0.0 });
        let centre = space.to_output(z.centre);
        // The longest path any pixel's taps take, in output pixels: toward the centre from the
        // farthest corner of the whole output.
        let span = z.amount / 200.0;
        let (w, h) = (f64::from(ctx.width), f64::from(ctx.height));
        let far = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].iter().map(|&(x, y)| (x - centre.0).hypot(y - centre.1)).fold(0.0, f64::max);
        let path = span * far;
        if !(path.is_finite() && centre.0.is_finite() && centre.1.is_finite()) || span <= 0.0 {
            return None;
        }
        // About a tap per pixel of the longest path, as `fine × coarse` taps evenly spaced in scale.
        let total = (path.ceil() + 1.0).clamp(2.0, MAX_TAPS as f64) as usize;
        let fine = ((total as f64).sqrt().ceil() as usize).max(1);
        let coarse = total.div_ceil(fine).max(1);
        let steps = fine.checked_mul(coarse)?.saturating_sub(1).max(1);
        let q = (1.0 - span).powf(1.0 / steps as f64);
        let fine_k: Vec<f64> = (0..fine).map(|j| q.powi(i32::try_from(j).unwrap_or(i32::MAX))).collect();
        let coarse_k: Vec<f64> = (0..coarse).map(|i| q.powi(i32::try_from(i * fine).unwrap_or(i32::MAX))).collect();
        let reach = u32::try_from(path.ceil() as u64).unwrap_or(u32::MAX).saturating_add(2);
        Some(Box::new(FocalZoomPass { centre, circle, fine: fine_k, coarse: coarse_k, fade: z.fade, reach, plane: OnceLock::new() }))
    }
}

/// A prepared Focal Zoom: the centre in output pixels, the circle it leaves alone (its weight is
/// where the picture stays), the scale factors of the two passes' taps, and the window after the
/// fine pass, made once per render.
struct FocalZoomPass {
    centre: (f64, f64),
    circle: Circle,
    fine: Vec<f64>,
    coarse: Vec<f64>,
    fade: f64,
    reach: u32,
    plane: OnceLock<Option<Plane>>,
}

impl FocalZoomPass {
    /// The mean of `src` over the points `centre + ((x, y) − centre) × k` for every `k` of `ks`
    /// (bilinear, pixel centres at `+ 0.5`).
    fn zoom(&self, ks: &[f64], x: u32, y: u32, src: &impl Taps) -> [f32; 3] {
        let (dx, dy) = (f64::from(x) + 0.5 - self.centre.0, f64::from(y) + 0.5 - self.centre.1);
        let mut acc = [0.0f32; 3];
        for &k in ks {
            let p = bilinear(src, self.centre.0 + dx * k, self.centre.1 + dy * k);
            for c in 0..3 {
                acc[c] += p[c];
            }
        }
        let n = ks.len().max(1) as f32;
        acc.map(|v| v / n)
    }

    /// The fine pass over `view`'s whole window.
    fn fine_pass(&self, view: &View<'_>) -> Option<Plane> {
        let (w, h) = (view.width as usize, view.height as usize);
        let mut data = alloc(w.checked_mul(h).filter(|n| *n > 0)?, [0.0f32; 3])?;
        par_rows(&mut data, w, |j, row| {
            let y = view.y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            for (i, px) in row.iter_mut().enumerate() {
                let x = view.x0.saturating_add(u32::try_from(i).unwrap_or(u32::MAX));
                *px = self.zoom(&self.fine, x, y, view);
            }
        });
        Some(Plane { x0: view.x0, y0: view.y0, width: view.width, height: view.height, data })
    }
}

impl Pass for FocalZoomPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.reach)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(plane) = self.plane.get_or_init(|| self.fine_pass(view)).as_ref() else { return };
        let (x0, y0) = (band.x0, band.y0);
        for_each_pixel(band.pixels, band.width, x0, y0, |x, y, px| {
            let w = 1.0 - self.circle.at(x, y);
            if w <= 0.0 {
                return;
            }
            let blurred = self.zoom(&self.coarse, x, y, plane);
            *px = fx::fade(*px, fx::mix(*px, blurred, w), self.fade);
        });
    }
}

// --- Shared -----------------------------------------------------------------------------------

/// `f(x, y, pixel)` for every pixel of a band's `pixels` (`width` wide, its top-left pixel at
/// `(x0, y0)` of the whole output).
fn for_each_pixel(pixels: &mut [[f32; 3]], width: u32, x0: u32, y0: u32, mut f: impl FnMut(u32, u32, &mut [f32; 3])) {
    if width == 0 {
        return;
    }
    for (j, row) in pixels.chunks_mut(width as usize).enumerate() {
        let y = y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
        for (i, px) in row.iter_mut().enumerate() {
            f(x0.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)), y, px);
        }
    }
}

/// A window of the whole output to sample, read with its nearest edge pixel outside it (the
/// picture's edge, or in a region render a margin no tap of a kept pixel reaches past).
trait Taps {
    /// The window's top-left pixel in the whole output and its size.
    fn window(&self) -> (i64, i64, i64, i64);
    /// The pixel at index `i` of the window, row-major.
    fn at(&self, i: usize) -> [f32; 3];
}

impl Taps for View<'_> {
    fn window(&self) -> (i64, i64, i64, i64) {
        (i64::from(self.x0), i64::from(self.y0), i64::from(self.width), i64::from(self.height))
    }

    fn at(&self, i: usize) -> [f32; 3] {
        match i.checked_mul(4).and_then(|at| self.data.get(at..at.checked_add(3)?)) {
            Some(&[r, g, b]) => [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0],
            _ => [0.0; 3],
        }
    }
}

impl Taps for Plane {
    fn window(&self) -> (i64, i64, i64, i64) {
        (i64::from(self.x0), i64::from(self.y0), i64::from(self.width), i64::from(self.height))
    }

    fn at(&self, i: usize) -> [f32; 3] {
        self.data.get(i).copied().unwrap_or([0.0; 3])
    }
}

/// The bilinear sample of `src` at the continuous whole-output point `(x, y)`, pixel centres at
/// `+ 0.5`.
fn bilinear(src: &impl Taps, x: f64, y: f64) -> [f32; 3] {
    let (wx, wy, w, h) = src.window();
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (fx0, fy0) = (fx.floor(), fy.floor());
    let (tx, ty) = ((fx - fx0) as f32, (fy - fy0) as f32);
    // `as` saturates (`NaN` to 0); the window is far inside `i64`.
    let (lx, ly) = ((fx0 as i64).saturating_sub(wx), (fy0 as i64).saturating_sub(wy));
    let index = |x: i64, y: i64| usize::try_from(y.clamp(0, (h - 1).max(0)) * w + x.clamp(0, (w - 1).max(0))).unwrap_or(0);
    let [a, b, c, d] = if lx >= 0 && ly >= 0 && lx + 1 < w && ly + 1 < h {
        // Inside: four neighbours.
        let i = usize::try_from(ly * w + lx).unwrap_or(0);
        let below = i.saturating_add(usize::try_from(w).unwrap_or(0));
        [src.at(i), src.at(i.saturating_add(1)), src.at(below), src.at(below.saturating_add(1))]
    } else {
        let (x1, y1) = (lx.saturating_add(1), ly.saturating_add(1));
        [src.at(index(lx, ly)), src.at(index(x1, ly)), src.at(index(lx, y1)), src.at(index(x1, y1))]
    };
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
        top + (bottom - top) * ty
    })
}

/// A share of [`par_rows`]'s rows: the first row's index and the rows, until someone runs them.
type Slot<'a, T> = Mutex<Option<(usize, &'a mut [T])>>;

/// `f(row index, row)` over the `row_len`-long rows of `data`, split across threads.
///
/// Made from plain scoped threads, not `rayon`: it runs inside a pass's once-per-render
/// initialisation, which other bands wait on; a `rayon` job there could steal one of those bands
/// and wait on itself.
fn par_rows<T: Send>(data: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    if row_len == 0 || data.is_empty() {
        return;
    }
    let rows = data.len().div_ceil(row_len);
    let threads = rayon::current_num_threads().clamp(1, 64).min(rows);
    let per = rows.div_ceil(threads).max(1);
    let slots: Vec<Slot<'_, T>> =
        data.chunks_mut(per.saturating_mul(row_len)).enumerate().map(|(t, chunk)| Mutex::new(Some((t * per, chunk)))).collect();
    // Whoever takes a slot first runs it.
    let run = |slot: &Slot<'_, T>| {
        if let Some((first, chunk)) = slot.lock().ok().and_then(|mut s| s.take()) {
            for (k, row) in chunk.chunks_mut(row_len).enumerate() {
                f(first + k, row);
            }
        }
    };
    std::thread::scope(|s| {
        for slot in slots.iter().skip(1) {
            let run = &run;
            // A thread that cannot be had leaves its slot to the loop below.
            let _spawned = std::thread::Builder::new().spawn_scoped(s, move || run(slot));
        }
        for slot in &slots {
            run(slot);
        }
    });
}

fn alloc<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}
