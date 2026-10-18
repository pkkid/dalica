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
//!     70 % of the way at the default 1.4, all the way at 2). A second Vignette compounds. Made
//!     by rows (M13.41): the untouched middle of a row is skipped, the distance is in `f32` and
//!     linear light goes through M13.34's [`fx::linear_light`] tables.
//!   - **Pixelate** (from PhotoCraft `crates/algo/src/stylize.rs` (`mosaic`) at e4b2868a: the mean
//!     of each cell, clipped at the picture's edges): square cells of `Pixel Size / 1000` of the
//!     stored photo's shorter side, square to the output and anchored at the stored frame's
//!     top-left corner, so the cells sit on the same content in a thumbnail, a zoomed region
//!     and a Save, and stay put when the crop changes; each cell's mean (in the working values,
//!     exact integer sums of the 8-bit pixels) is blended onto the picture by the mode, then
//!     faded. Cells are at least one output pixel. Each column's cell comes from a table made
//!     once per render, and in Normal mode at Fade 0 a row's run of one cell is filled with its
//!     mean (M13.41).
//!   - **Focal Zoom** (from PhotoCraft `crates/algo/src/blur.rs` (`radial`, the zoom method) and
//!     `crates/algo/src/blur/radial.rs` at e4b2868a: samples along the ray toward the centre, up
//!     to `Zoom Amount / 200` of the way): the centre is a stored-frame point, so it lands on the
//!     same content under any EXIF orientation, crop and straighten. The circle of Focal Size is
//!     untouched; outside it the zoom blur comes in over `(1 − Edge Hardness/100) × 0.5` of the
//!     shorter side ([`FOCAL_RAMP`]). The blur is a box along each ray from the centre, made once
//!     per render at a constant cost per pixel (M13.41; M12 averaged up to 1024 bilinear taps per
//!     pixel in two passes): around the centre, four sectors where one axis of a pixel's offset
//!     dominates (right, left, down, up) each send out rays at most a cell apart, sampled once per
//!     row or column of cells along that axis between the two nearest cells across, and a cell's
//!     box over its ray reaches `Zoom Amount / 200` of its distance back toward the centre, each
//!     sample weighted by one over its distance as taps evenly spaced in scale were. The running
//!     sums are exact integers, so a box costs the same however long it is and a zoomed region
//!     equals the whole render. Every cell then reads the two rays on either side of it. A render
//!     of more than [`COARSE_PIXELS`] blurs on a grid of 2 × 2-pixel cells read back bilinearly:
//!     the same along the rays, about a pixel softer across them; a smaller one blurs every pixel.
//!
//!   The blend modes are M12.3's [`Blend`], Fade its [`fx::fade`]; lengths and points go through
//!   [`Space`] (`NOTICE`).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reach, Reader, UNIT, View, clamp, clamp_point, hex, num, num_point};
use crate::frame::Point;
use crate::fx::{self, Blend, LinearLight, Space};

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
/// The steps between two cells in which Focal Zoom's rays sample: a sample is an exact integer,
/// and its boxes keep a level of an 8-bit channel as this many units.
const LEVEL: u32 = 256;
/// The largest value Focal Zoom's boxes keep: level 255.
const SAMPLE_MAX: f64 = (255 * LEVEL) as f64;
/// Focal Zoom's weight of a sample one cell from the centre along its ray, in fixed point: a
/// sample's weight is this over its distance along the ray (half a cell at least), so the samples
/// of a box weigh what taps evenly spaced in scale would.
const WEIGHT_ONE: f64 = (1u64 << 24) as f64;

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
            alpha: (1.0 - v.fade / 100.0) as f32,
            light: fx::linear_light(),
        }))
    }
}

/// A prepared Vignette: the output picture's centre and half-diagonal in pixels, where the ramp
/// starts (a fraction of the half-diagonal), how far the corners go, the colour in linear light,
/// what is left of the effect after Fade, and the shared linear-light tables.
struct VignettePass {
    centre: (f64, f64),
    half: f64,
    inner: f64,
    amount: f32,
    colour: [f32; 3],
    alpha: f32,
    light: &'static LinearLight,
}

impl VignettePass {
    /// The columns `[a, b)` of the output row whose centre lies `dy` pixels from the picture's
    /// centre that lie a pixel or more inside the ramp's start: untouched, so skipped (the
    /// per-pixel test decides the rest).
    fn untouched(&self, dy: f64) -> (i64, i64) {
        let r = self.inner * self.half;
        let s = (r * r - dy * dy).sqrt() - 1.0;
        if s.is_nan() || s <= 0.0 {
            return (0, 0);
        }
        // Column `x` has `dx = x + 0.5 − cx`; `|dx| < s` holds for `x` in `(cx − 0.5 − s, cx − 0.5 + s)`.
        let (a, b) = ((self.centre.0 - 0.5 - s).ceil(), (self.centre.0 - 0.5 + s).floor() + 1.0);
        // `as` saturates; the picture is far inside `i64`.
        (a as i64, b as i64)
    }

    /// The pixel `dx` from the centre along a row `dy2 = dy²` from it, vignetted.
    fn shade(&self, px: &mut [f32; 3], dx: f32, dy2: f32, inv_half: f32, inner: f32, ramp: f32) {
        let d = (dx * dx + dy2).sqrt() * inv_half;
        let t = (d - inner) / ramp;
        if t.is_nan() || t <= 0.0 {
            return;
        }
        let t = t.min(1.0);
        let a = (t * t * (3.0 - 2.0 * t) * self.amount).min(1.0);
        for (v, &c) in px.iter_mut().zip(&self.colour) {
            let lin = self.light.to_linear(*v);
            let out = self.light.to_srgb(lin + (c - lin) * a);
            *v += (out - *v) * self.alpha;
        }
    }
}

impl Pass for VignettePass {
    fn run(&self, band: &mut Band<'_>) {
        let width = band.width as usize;
        if width == 0 {
            return;
        }
        let (inv_half, inner) = ((1.0 / self.half) as f32, self.inner as f32);
        let ramp = (1.0 - self.inner) as f32;
        let x0 = i64::from(band.x0);
        let dx0 = f64::from(band.x0) + 0.5 - self.centre.0;
        for (j, row) in band.pixels.chunks_mut(width).enumerate() {
            let dy = f64::from(band.y0) + j as f64 + 0.5 - self.centre.1;
            let dy2 = (dy * dy) as f32;
            // The untouched middle of the row `[from, to)`, in the band's columns, is skipped.
            let (a, b) = self.untouched(dy);
            let from = a.saturating_sub(x0).clamp(0, width as i64) as usize;
            let to = b.saturating_sub(x0).clamp(from as i64, width as i64) as usize;
            let Some((left, rest)) = row.split_at_mut_checked(from) else { continue };
            for (i, px) in left.iter_mut().enumerate() {
                self.shade(px, (dx0 + i as f64) as f32, dy2, inv_half, inner, ramp);
            }
            let right = rest.get_mut(to - from..).unwrap_or(&mut []);
            for (i, px) in right.iter_mut().enumerate() {
                self.shade(px, (dx0 + (to + i) as f64) as f32, dy2, inv_half, inner, ramp);
            }
        }
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

/// The mean of every cell that meets a window, row-major from the cell row `j0`, and the window's
/// columns by cell.
struct Cells {
    j0: i64,
    nx: usize,
    means: Vec<[f32; 3]>,
    /// The window's first column in the whole output.
    x0: u32,
    /// Each column of the window: its cell's place in a row of `means`.
    cols: Vec<usize>,
    /// The window's columns in runs of one cell: `(first, end, place in a row of means)`.
    runs: Vec<(usize, usize, usize)>,
}

impl Cells {
    /// The means of the cell row `j`, or `None` outside the window.
    fn row(&self, j: i64) -> Option<&[[f32; 3]]> {
        let j = usize::try_from(j.checked_sub(self.j0)?).ok()?;
        let at = j.checked_mul(self.nx)?;
        self.means.get(at..at.checked_add(self.nx)?)
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
        // The cells grow with the column, so each cell's columns are one run.
        let cols: Vec<usize> = cols.iter().map(|&c| c.checked_sub(i0).and_then(|i| usize::try_from(i).ok()).unwrap_or(0)).collect();
        let mut runs: Vec<(usize, usize, usize)> = Vec::new();
        for (x, &c) in cols.iter().enumerate() {
            match runs.last_mut() {
                Some(run) if run.2 == c => run.1 = x + 1,
                _ => runs.push((x, x + 1, c)),
            }
        }
        let mut means = alloc(nx.checked_mul(ny)?, [0.0f32; 3])?;
        let data = view.data;
        par_rows(&mut means, nx, |j, out| {
            let cj = j0.saturating_add(i64::try_from(j).unwrap_or(i64::MAX));
            let (start, end) = (rows.partition_point(|&r| r < cj), rows.partition_point(|&r| r <= cj));
            let mut sums = vec![[0u64; 4]; out.len()];
            for ly in start..end {
                let Some(row) = ly.checked_mul(row_bytes).and_then(|at| data.get(at..at.checked_add(row_bytes)?)) else { continue };
                for &(first, end, c) in &runs {
                    let (Some(pixels), Some(s)) = (row.get(first * 4..end * 4), sums.get_mut(c)) else { continue };
                    // A run is at most a row of the window: its sums fit in `u32`.
                    let mut run = [0u32; 3];
                    for px in pixels.as_chunks::<4>().0 {
                        run[0] += u32::from(px[0]);
                        run[1] += u32::from(px[1]);
                        run[2] += u32::from(px[2]);
                    }
                    s[0] += u64::from(run[0]);
                    s[1] += u64::from(run[1]);
                    s[2] += u64::from(run[2]);
                    s[3] += (end - first) as u64;
                }
            }
            for (o, s) in out.iter_mut().zip(&sums) {
                if s[3] > 0 {
                    let n = s[3] as f64 * 255.0;
                    *o = [(s[0] as f64 / n) as f32, (s[1] as f64 / n) as f32, (s[2] as f64 / n) as f32];
                }
            }
        });
        Some(Cells { j0, nx, means, x0: view.x0, cols, runs })
    }
}

impl Pass for PixelatePass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.reach)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(cells) = self.cells.get_or_init(|| self.cells_of(view)).as_ref() else { return };
        let width = band.width as usize;
        let Some(skip) = band.x0.checked_sub(cells.x0).map(|s| s as usize) else { return };
        if width == 0 {
            return;
        }
        // Normal at Fade 0 is each cell's mean: a run of one cell is filled with it.
        let copy = self.mode == Blend::Normal && self.fade <= 0.0 && skip == 0 && width == cells.cols.len();
        for (j, row) in band.pixels.chunks_mut(width).enumerate() {
            let y = band.y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            let Some(means) = cells.row(self.cell(y, self.origin.1)) else { continue };
            if copy {
                for &(first, end, c) in &cells.runs {
                    if let (Some(run), Some(mean)) = (row.get_mut(first..end), means.get(c)) {
                        run.fill(*mean);
                    }
                }
                continue;
            }
            let cols = cells.cols.get(skip..).unwrap_or(&[]);
            for (px, &c) in row.iter_mut().zip(cols) {
                if let Some(&top) = means.get(c) {
                    *px = fx::fade(*px, self.mode.apply(*px, top), self.fade);
                }
            }
        }
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
        let centre = space.to_output(z.centre);
        // The longest path any pixel's box takes, in output pixels: toward the centre from the
        // farthest corner of the whole output.
        let span = z.amount / 200.0;
        let (w, h) = (f64::from(ctx.width), f64::from(ctx.height));
        let far = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].iter().map(|&(x, y)| (x - centre.0).hypot(y - centre.1)).fold(0.0, f64::max);
        let path = span * far;
        if !(path.is_finite() && centre.0.is_finite() && centre.1.is_finite()) || span <= 0.0 {
            return None;
        }
        let cell = if u64::from(ctx.width) * u64::from(ctx.height) > COARSE_PIXELS { 2 } else { 1 };
        let (inside, outside) = (space.length(z.size), space.length(outer));
        // The box, a cell across between two rows or columns of cells, a ray at most a cell away
        // and the cells read back around a pixel.
        let reach = u32::try_from(path.ceil() as u64).unwrap_or(u32::MAX).saturating_add(4 + 4 * cell);
        Some(Box::new(FocalZoomPass {
            centre,
            full: (ctx.width, ctx.height),
            cell,
            span,
            inside,
            inside2: inside * inside,
            outside2: outside * outside,
            ramp: if outside - inside > 1e-6 { (1.0 / (outside - inside)) as f32 } else { 0.0 },
            keep: (1.0 - z.fade / 100.0) as f32,
            reach,
            blur: OnceLock::new(),
        }))
    }
}

/// The pixels of a render above which Focal Zoom blurs on a grid of 2 × 2-pixel cells and reads
/// it back bilinearly, so the slider's final render stays within its cap (M13.41): a
/// 1920 × 1080 render and anything smaller blurs every pixel.
pub const COARSE_PIXELS: u64 = 1 << 21;

/// A prepared Focal Zoom: the centre in output pixels and the whole output's size, the side of the
/// grid's cells in pixels, how far the box reaches toward the centre (a fraction of the
/// distance), the untouched radius in output pixels and the squared radii of the circle's ramp
/// (its weight is where the picture stays) with one over its width, what is left after Fade, and
/// the blur on the grid, made once per render.
struct FocalZoomPass {
    centre: (f64, f64),
    full: (u32, u32),
    cell: u32,
    span: f64,
    inside: f64,
    inside2: f64,
    outside2: f64,
    ramp: f32,
    keep: f32,
    reach: u32,
    blur: OnceLock<Option<Blur>>,
}

/// Focal Zoom on a window's grid of cells: cell `(x, y)` (whole-output cells) at
/// `data[(y − y0) · width + x − x0]`, its three channels 16 bits each (a level is [`LEVEL`]),
/// [`NONE`] where it holds none (well inside the untouched circle). Filled by the rays' blocks
/// from several threads, each cell by the one block whose rays it lies between.
struct Blur {
    x0: i64,
    y0: i64,
    width: usize,
    data: Vec<AtomicU64>,
}

/// A [`Blur`] cell that holds nothing.
const NONE: u64 = u64::MAX;

impl Blur {
    /// The cells of row `y`.
    fn row(&self, y: i64) -> Option<&[AtomicU64]> {
        let at = usize::try_from(y.checked_sub(self.y0)?).ok()?.checked_mul(self.width)?;
        self.data.get(at..at.checked_add(self.width)?)
    }
}

/// Focal Zoom's geometry on a grid: the centre in cells (a cell's centre at `+ 0.5`), the whole
/// output's size in cells, the untouched radius in cells and how far a box reaches back.
struct Geom {
    centre: (f64, f64),
    full: (f64, f64),
    inside: f64,
    span: f64,
}

/// One of the four sectors about the centre, where one axis of a pixel's offset from it
/// dominates: its rays are sampled once per row or column of cells along that axis (`main`),
/// between the two nearest cells across it.
#[derive(Clone, Copy, Debug)]
struct Sector {
    /// The main axis is `y` (the rays run up or down), else `x`.
    vertical: bool,
    /// The sector lies ahead of the centre along its main axis (`1`) or behind it (`-1`).
    sign: f64,
}

/// Right, left, down, up.
const SECTORS: [Sector; 4] = [
    Sector { vertical: false, sign: 1.0 },
    Sector { vertical: false, sign: -1.0 },
    Sector { vertical: true, sign: 1.0 },
    Sector { vertical: true, sign: -1.0 },
];

/// How many neighbouring rays Focal Zoom makes together and reads cells back from.
const BLOCK: usize = 16;

/// What a sector's rays through one window share: the sector, the centre along its main axis
/// (`cm`) and across it (`cc`), the rays' scale, the window's cell centres from the centre along
/// the main axis (`a_lo ..= a_hi`, only those ahead of the centre count) and across it
/// (`b0 ..= b1`), the window's rows or columns along the main axis (`m0 ..= m1`), and the rows or
/// columns the rays sample.
struct Wedges {
    sector: Sector,
    cm: f64,
    cc: f64,
    scale: f64,
    a_lo: f64,
    a_hi: f64,
    b0: f64,
    b1: f64,
    m0: i64,
    m1: i64,
    along: Along,
}

/// The rows or columns of cells of a sector's main axis its rays through one window sample, from
/// a little behind the centre outward (entry `i` is the row or column `m0 + step · i`), with what
/// every ray shares there: its sample's weight (fixed point, an exact integer) and the box of a
/// cell there.
struct Along {
    m0: i64,
    step: i64,
    w: Vec<f64>,
    boxes: Vec<BoxShape>,
}

/// The box `[a − d, a]` of a cell `a` ahead of the centre over the samples of its ray, each
/// standing for `[a' − ½, a' + ½]`: its own sample counts by `own` (half, or all of a box shorter
/// than that), the `full` before it wholly, the one before those by `part`; `inv` is one over the
/// box's weight (0 where the samples do not reach back far enough).
#[derive(Clone, Copy, Debug, Default)]
struct BoxShape {
    own: f64,
    full: usize,
    part: f64,
    inv: f64,
}

impl Along {
    /// The rows or columns from `a_from` ahead of the centre (behind it when negative) out to
    /// the row or column `last`, for boxes reaching `span` of the way back.
    fn new(sector: Sector, cm: f64, a_from: f64, last: i64, span: f64) -> Option<Along> {
        let at = cm - 0.5;
        let (m0, step) = if sector.sign > 0.0 { ((at + a_from).floor(), 1) } else { ((at - a_from).ceil(), -1) };
        // `as` saturates; the row or column is near the window.
        let m0 = m0 as i64;
        let n = usize::try_from(last.checked_sub(m0)?.checked_mul(step)?.checked_add(1)?).ok()?;
        let along = |i: usize| sector.sign * (m0.saturating_add(step.saturating_mul(i as i64)) as f64 + 0.5 - cm);
        let mut w = alloc(n, 0.0)?;
        let mut sums = alloc(n.checked_add(1)?, 0.0)?;
        let mut sum = 0.0;
        for (i, (wi, s)) in w.iter_mut().zip(sums.iter_mut().skip(1)).enumerate() {
            *wi = (WEIGHT_ONE / along(i).max(0.5)).round();
            sum += *wi;
            *s = sum;
        }
        let mut boxes = alloc(n, BoxShape::default())?;
        for (i, (b, &wi)) in boxes.iter_mut().zip(&w).enumerate() {
            let d = span * along(i);
            let shape = if d < 0.5 {
                Some(BoxShape { own: 1.0, full: 0, part: 0.0, inv: 1.0 / wi })
            } else {
                let full = (d - 0.5).floor();
                let part = d - 0.5 - full;
                // `as` saturates; `d` is finite.
                let full = full as usize;
                i.checked_sub(full.saturating_add(1)).and_then(|far| {
                    let (whole, before) = (sums.get(i)? - sums.get(far + 1)?, *w.get(far)?);
                    Some(BoxShape { own: 0.5, full, part, inv: 1.0 / (0.5 * wi + whole + part * before) })
                })
            };
            *b = shape.unwrap_or_default();
        }
        Some(Along { m0, step, w, boxes })
    }

    /// The entry of the row or column `m`.
    fn index(&self, m: i64) -> Option<usize> {
        usize::try_from(m.checked_sub(self.m0)?.checked_mul(self.step)?).ok().filter(|&i| i < self.w.len())
    }
}

/// Narrows `[lo, hi]` to the `a` with `k·a ≥ c`.
fn at_least(lo: &mut f64, hi: &mut f64, k: f64, c: f64) {
    if k > 0.0 {
        *lo = lo.max(c / k);
    } else if k < 0.0 {
        *hi = hi.min(c / k);
    } else if c > 0.0 {
        *hi = f64::NEG_INFINITY;
    }
}

/// Narrows `[lo, hi]` to the `a` with `k·a ≤ c`.
fn at_most(lo: &mut f64, hi: &mut f64, k: f64, c: f64) {
    if k > 0.0 {
        *hi = hi.min(c / k);
    } else if k < 0.0 {
        *lo = lo.max(c / k);
    } else if c < 0.0 {
        *hi = f64::NEG_INFINITY;
    }
}

/// The largest whole number at most `v` (`as` saturates, `NaN` gives 0), without the library call
/// `floor` is on the baseline x86-64 target.
fn floor_i64(v: f64) -> i64 {
    let t = v as i64;
    if t as f64 > v { t - 1 } else { t }
}

impl FocalZoomPass {
    /// The zoom blur over `view`'s window, on its grid of cells.
    fn blur_of(&self, view: &View<'_>) -> Option<Blur> {
        let grid = Grid::of(view, self.cell)?;
        let c = f64::from(self.cell);
        let geom = Geom {
            centre: (self.centre.0 / c, self.centre.1 / c),
            full: ((f64::from(self.full.0) / c).ceil(), (f64::from(self.full.1) / c).ceil()),
            inside: self.inside / c,
            span: self.span,
        };
        // Every cell of the window outside the untouched circle, from the rays of its sector.
        let width = usize::try_from(grid.w).ok()?;
        let n = width.checked_mul(usize::try_from(grid.h).ok()?)?;
        let mut data = Vec::new();
        data.try_reserve_exact(n).ok()?;
        data.extend((0..n).map(|_| AtomicU64::new(NONE)));
        let blur = Blur { x0: grid.x0, y0: grid.y0, width, data };
        for s in SECTORS {
            fan(s, &geom, &grid, &blur)?;
        }
        Some(blur)
    }

    /// The band pixel `px` with the blur `v` there (in [`LEVEL`] steps) mixed in by the circle's
    /// weight `w`, then faded: `fade(px, mix(px, blurred, w), fade)`.
    fn mix_in(&self, px: &mut [f32; 3], v: [f32; 3], w: f32) {
        let k = w * self.keep;
        if k >= 1.0 {
            *px = v.map(|v| v * (1.0 / SAMPLE_MAX as f32));
            return;
        }
        for (o, v) in px.iter_mut().zip(v) {
            *o += (v * (1.0 / SAMPLE_MAX as f32) - *o) * k;
        }
    }

    /// The circle's weight at `d2` squared from the centre: 0 inside the untouched radius, rising
    /// smoothly over the ramp to 1.
    fn weight(&self, d2: f64) -> f32 {
        if d2 < self.inside2 {
            0.0
        } else if d2 >= self.outside2 || self.ramp <= 0.0 {
            1.0
        } else {
            let t = (((d2 as f32).sqrt() - self.inside as f32) * self.ramp).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
    }
}

/// The rays of sector `s` through every cell of `grid`'s window, sampled and box-filtered, in
/// blocks of [`BLOCK`] neighbouring rays, each filling the cells of `blur` between its rays.
fn fan(s: Sector, g: &Geom, grid: &Grid, blur: &Blur) -> Option<()> {
    let Some((wedges, j0, j1)) = wedges(s, g, grid) else { return Some(()) };
    let mut blocks = alloc(usize::try_from(j1 - j0 + 1).ok()?.div_ceil(BLOCK), ())?;
    par_split(&mut blocks, 1, |first, blocks| {
        let mut scratch = Scratch::default();
        for k in first..first + blocks.len() {
            if let Some(j) = i64::try_from(k * BLOCK).ok().and_then(|o| j0.checked_add(o)) {
                ray_block(&wedges, g.inside, (j, j1), grid, &mut scratch, blur);
            }
        }
    });
    Some(())
}

/// What sector `s`'s rays through `grid`'s window share, and the first and last of those rays;
/// `None` when no cell of the window lies in the sector.
fn wedges(s: Sector, g: &Geom, grid: &Grid) -> Option<(Wedges, i64, i64)> {
    let (cm, cc) = if s.vertical { (g.centre.1, g.centre.0) } else { g.centre };
    let n_main = if s.vertical { g.full.1 } else { g.full.0 };
    let scale = (if s.sign > 0.0 { n_main - 0.5 - cm } else { cm - 0.5 }).max(1.0);
    let (m0, m1, q0, q1) = if s.vertical {
        (grid.y0, grid.y0 + grid.h - 1, grid.x0, grid.x0 + grid.w - 1)
    } else {
        (grid.x0, grid.x0 + grid.w - 1, grid.y0, grid.y0 + grid.h - 1)
    };
    let (e0, e1) = (s.sign * (m0 as f64 + 0.5 - cm), s.sign * (m1 as f64 + 0.5 - cm));
    let (a_lo, a_hi) = (e0.min(e1).max(1e-9), e0.max(e1));
    if !(a_hi > 0.0 && scale.is_finite()) {
        return None;
    }
    // The rays sample from a little behind the farthest any box of the window reaches back.
    let a_from = (a_lo * (1.0 - g.span) - 3.0).max(-2.0);
    let along = Along::new(s, cm, a_from, if s.sign > 0.0 { m1 } else { m0 }, g.span)?;
    let wedges = Wedges { sector: s, cm, cc, scale, a_lo, a_hi, b0: q0 as f64 + 0.5 - cc, b1: q1 as f64 + 0.5 - cc, m0, m1, along };
    // The rays through the window: the steepest and the shallowest cell offsets lie at its
    // corners (inside the sector, at most one cell across per cell along).
    let ratios = [wedges.b0 / a_lo, wedges.b0 / a_hi, wedges.b1 / a_lo, wedges.b1 / a_hi];
    let lo = ratios.iter().copied().fold(1.0, f64::min).max(-1.0);
    let hi = ratios.iter().copied().fold(-1.0, f64::max).min(1.0);
    let (j0, j1) = ((lo * scale).floor() - 1.0, (hi * scale).floor() + 1.0);
    if !(j0.is_finite() && j1.is_finite() && j0 <= j1) {
        return None;
    }
    // `as` saturates; both are within the sector's `±scale`.
    let (j0, j1) = (j0 as i64, j1 as i64);
    Some((wedges, j0, j1))
}

/// A thread's room for a block of rays: the running sums of [`sweep`] and the rays' boxes
/// ([`SIDE`] values a row or column of the main axis).
#[derive(Default)]
struct Scratch {
    prefix: Vec<f64>,
    rays: Vec<[u16; 3]>,
}

/// The rays `j ..= j + BLOCK` of `f` (none past `last`), sampled and box-filtered, and the cells of
/// `blur` whose rays lie from `j` up to `j + BLOCK`, each read back from the two on either side of
/// it.
fn ray_block(f: &Wedges, inside: f64, (j, last): (i64, i64), grid: &Grid, scratch: &mut Scratch, blur: &Blur) -> Option<()> {
    let Scratch { prefix, rays } = scratch;
    let spans: [Option<(i64, i64)>; SIDE] = std::array::from_fn(|r| {
        let ray = j.checked_add(i64::try_from(r).ok()?)?;
        if ray <= last { outputs(f, inside, ray) } else { None }
    });
    let m0 = spans.iter().flatten().map(|s| s.0).min()?;
    let m1 = spans.iter().flatten().map(|s| s.1).max()?;
    let rows = usize::try_from(m1.checked_sub(m0)?.checked_add(1)?).ok()?;
    rays.clear();
    rays.try_reserve(rows.checked_mul(SIDE)?).ok()?;
    rays.resize(rows * SIDE, [u16::MAX; 3]);
    sweep(f, j, (m0, m1), grid, prefix, rays)?;
    // A ray without cells to give holds nothing.
    for (r, span) in spans.iter().enumerate() {
        if span.is_none() {
            for v in rays.iter_mut().skip(r).step_by(SIDE) {
                *v = [u16::MAX; 3];
            }
        }
    }
    // The cells whose rays `ρ = across · scale / along` lie from `j` to `j + BLOCK`, each between
    // the two rays on either side of it.
    let skip2 = (inside - 2.0).max(0.0).powi(2);
    let (sign, vertical) = (f.sector.sign, f.sector.vertical);
    let (q0, q1) = if vertical { (grid.x0, grid.x0 + grid.w - 1) } else { (grid.y0, grid.y0 + grid.h - 1) };
    let width = i64::try_from(blur.width).ok()?;
    for (m, row) in (m0..=m1).zip(rays.as_chunks::<SIDE>().0) {
        let a = sign * (m as f64 + 0.5 - f.cm);
        if a.is_nan() || a <= 0.0 {
            continue;
        }
        let per = f.scale / a;
        let (b_lo, b_hi) = (j as f64 / per, (j + BLOCK as i64) as f64 / per);
        // `as` saturates.
        let (first, end) = (((f.cc - 0.5 + b_lo).floor() as i64 - 1).max(q0), ((f.cc - 0.5 + b_hi).ceil() as i64 + 1).min(q1));
        // Cell `q` across at `base + q · stride` of the blur.
        let (base, stride) = if vertical { ((m - blur.y0) * width - blur.x0, 1) } else { (m - blur.x0 - blur.y0 * width, width) };
        for q in first..=end {
            let b = q as f64 + 0.5 - f.cc;
            // The diagonals belong to the side sectors; the untouched circle is left out.
            let inside_sector = if vertical { b.abs() < a } else { b.abs() <= a };
            if !inside_sector || a * a + b * b < skip2 {
                continue;
            }
            let rho = b * per;
            let k = floor_i64(rho);
            let Some(r) = usize::try_from(k - j).ok().filter(|&r| r < BLOCK) else { continue };
            // `as` saturates; under 256.
            let t = ((rho - k as f64) * 256.0) as u32;
            let (Some(&p), Some(&n)) = (row.get(r), row.get(r + 1)) else { continue };
            let v: [u32; 3] = match (p[0] != u16::MAX, n[0] != u16::MAX) {
                (true, true) => std::array::from_fn(|c| (u32::from(p[c]) * (256 - t) + u32::from(n[c]) * t + 128) >> 8),
                (true, false) => p.map(u32::from),
                (false, true) => n.map(u32::from),
                (false, false) => continue,
            };
            let at = usize::try_from(base + q * stride).ok();
            if let Some(cell) = at.and_then(|i| blur.data.get(i)) {
                cell.store(u64::from(v[0]) | u64::from(v[1]) << 16 | u64::from(v[2]) << 32, Ordering::Relaxed);
            }
        }
    }
    Some(())
}

/// The rows or columns of the main axis where ray `j` of `f` holds values: those of the window's
/// cells between rays `j − 1` and `j + 1` (with a cell of slack each way) that lie ahead of the
/// centre and outside the untouched circle of radius `inside`.
fn outputs(f: &Wedges, inside: f64, j: i64) -> Option<(i64, i64)> {
    let jf = j as f64;
    let (mut lo, mut hi) = (f.a_lo, f.a_hi);
    at_least(&mut lo, &mut hi, jf + 1.0, (f.b0 - 1.0) * f.scale);
    at_most(&mut lo, &mut hi, jf - 1.0, (f.b1 + 1.0) * f.scale);
    let slope = (jf.abs() + 1.0) / f.scale;
    lo = lo.max((inside - 3.0) / (1.0 + slope * slope).sqrt());
    if lo.is_nan() || hi.is_nan() || lo > hi {
        return None;
    }
    let (sign, at) = (f.sector.sign, f.cm - 0.5);
    let (ma, mb) = (at + sign * lo, at + sign * hi);
    let (mut m_lo, mut m_hi) = ((ma.min(mb).floor() - 1.0).max(f.m0 as f64), (ma.max(mb).ceil() + 1.0).min(f.m1 as f64));
    if sign > 0.0 {
        m_lo = m_lo.max(at.floor() + 1.0);
    } else {
        m_hi = m_hi.min(at.ceil() - 1.0);
    }
    // `as` saturates; both lie in the window.
    (m_lo <= m_hi).then_some((m_lo as i64, m_hi as i64))
}

/// Ray `j`'s slope in fixed point: 2⁻²⁴ cells across a cell along.
fn slope(f: &Wedges, j: i64) -> i64 {
    // `as` saturates; within `±2²⁴` for the rays of a sector.
    (j as f64 / f.scale * f64::from(1u32 << 24)).round() as i64
}

/// Where ray `j` of `f` crosses the row or column `m` of the main axis, in fixed point (2⁻³² cells,
/// a cell's centre at `+ ½`): the centre across and the rows or columns along in 1/256 cells, so
/// the next row or column lies exactly `256 ·` [`slope`] further.
fn position(f: &Wedges, j: i64, m: i64) -> i64 {
    // `as` saturates; the window is far inside `i64`.
    let along = (f.sector.sign * ((m as f64 + 0.5) * 256.0 - (f.cm * 256.0).round())) as i64;
    let centre = ((f.cc - 0.5) * (1u64 << 32) as f64).round() as i64;
    centre.saturating_add(slope(f, j).saturating_mul(along))
}

/// The rays a block makes together: [`BLOCK`] and the next block's first, so a block reads back
/// every cell up to it.
const SIDE: usize = BLOCK + 1;
/// The running sums a block keeps per row or column: three channels of each ray.
const LANES: usize = SIDE * 3;

/// Rays `j ..= j + BLOCK` of `f`, swept together from behind their nearest output outward, one
/// sample per row or column of cells along the main axis each, and their boxes on the rows or
/// columns `m0 ..= m1`, written to `rays[(m − m0) · SIDE + ray]`. `prefix` is scratch room for the
/// samples' running sums.
///
/// The box of a cell `a` ahead of the centre reaches `span · a` back toward it, as the taps from
/// M12's scale `1` to `1 − span` did: each sample stands for its row or column's cell of the ray
/// and counts by the part of it inside the box ([`BoxShape`], the same for every ray on a row or
/// column); each weighs `1 / a` (taps evenly spaced in scale lie that densely). The samples and
/// weights are whole numbers and their running sums stay far below 2⁵³, so they are exact: a box
/// is the same whichever sample the sums start at, and a region gets the whole render's values.
fn sweep(f: &Wedges, j: i64, (m0, m1): (i64, i64), grid: &Grid, prefix: &mut Vec<f64>, rays: &mut [[u16; 3]]) -> Option<()> {
    let t = &f.along;
    let (sign, step) = (f.sector.sign, t.step);
    let (near_m, far_m) = if sign > 0.0 { (m0, m1) } else { (m1, m0) };
    let (near, far) = (t.index(near_m)?, t.index(far_m)?);
    // The samples start as far back from the nearest output as its box reaches, and one more.
    let first = near.saturating_sub(t.boxes.get(near)?.full.saturating_add(2));
    let w = t.w.get(first..=far)?;
    prefix.clear();
    prefix.try_reserve(w.len().checked_add(1)?.checked_mul(LANES)?).ok()?;
    prefix.resize(LANES, 0.0);
    // Each ray's position across in fixed point (2⁻³² cells), stepped exactly: the centre across
    // and the rows or columns along in 1/256 cells, the rays' slopes in 2⁻²⁴.
    let mut m = t.m0.checked_add(step.checked_mul(i64::try_from(first).ok()?)?)?;
    let slopes: [i64; SIDE] = std::array::from_fn(|r| slope(f, j.saturating_add(r as i64)));
    let mut pos: [i64; SIDE] = std::array::from_fn(|r| position(f, j.saturating_add(r as i64), m));
    let mut sums = [0.0f64; LANES];
    let mut v = [0u32; LANES];
    for &wi in w {
        grid.samples(f.sector.vertical, m, &pos, &mut v);
        for i in 0..LANES {
            sums[i] += wi * f64::from(v[i]);
        }
        prefix.extend_from_slice(&sums);
        m = m.saturating_add(step);
        for (p, s) in pos.iter_mut().zip(&slopes) {
            *p = p.saturating_add(s.saturating_mul(256));
        }
    }
    // The boxes from the nearest output outward, every ray of a row or column at once.
    let at = |k: usize| -> Option<&[f64; LANES]> { prefix.get(k.checked_mul(LANES)?..k.checked_mul(LANES)?.checked_add(LANES)?)?.try_into().ok() };
    let mut m = near_m;
    let mut totals = [0.0f64; LANES];
    for (n, b) in t.boxes.get(near..=far)?.iter().enumerate() {
        let k = near - first + n;
        let row = usize::try_from(m - m0).ok().and_then(|row| rays.get_mut(row.checked_mul(SIDE)?..row.checked_mul(SIDE)?.checked_add(SIDE)?));
        m = m.saturating_add(step);
        let (Some(row), Some(end), Some(pixel), Some(mid), Some(back)) =
            (row, at(k + 1), at(k), k.checked_sub(b.full).and_then(at), k.checked_sub(b.full + 1).and_then(at))
        else {
            continue;
        };
        if b.inv <= 0.0 {
            continue;
        }
        // Over 4, rounded.
        let scale = b.inv * 0.25;
        for i in 0..LANES {
            totals[i] = (b.own * (end[i] - pixel[i]) + (pixel[i] - mid[i]) + b.part * (mid[i] - back[i])) * scale + 0.5;
        }
        for (out, total) in row.iter_mut().zip(totals.as_chunks::<3>().0) {
            // `as` saturates at 0 and at `u16::MAX`, above any sample.
            *out = total.map(|v| v as u16);
        }
    }
    Some(())
}

impl Pass for FocalZoomPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.reach)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(blur) = self.blur.get_or_init(|| self.blur_of(view)).as_ref() else { return };
        let width = band.width as usize;
        if width == 0 {
            return;
        }
        let cell = |v: &AtomicU64| {
            let v = v.load(Ordering::Relaxed);
            // The three channels, 16 bits each.
            (v != NONE).then_some([(v & 0xffff) as f32, (v >> 16 & 0xffff) as f32, (v >> 32 & 0xffff) as f32])
        };
        let x0 = i64::from(band.x0);
        // The columns `[from, to)` of a row `dy` from the centre well inside the untouched circle
        // are skipped (a pixel of slack: each pixel is weighed again).
        let middle = |dy: f64| {
            let r = (self.inside2 - dy * dy).max(0.0).sqrt() - 1.0;
            let (a, b) = ((self.centre.0 - 0.5 - r - f64::from(band.x0)).ceil(), (self.centre.0 - 0.5 + r - f64::from(band.x0)).floor() + 1.0);
            // `as` saturates.
            let from = (a.max(0.0) as usize).min(width);
            (from, (b.max(0.0) as usize).clamp(from, width))
        };
        let dxs: Vec<f64> = (0..width).map(|i| f64::from(band.x0) + i as f64 + 0.5 - self.centre.0).collect();
        // For cells of 2 × 2: the two rows of cells around a band row (`rows_at` the first), and
        // the two mixed.
        let (mut above, mut below, mut row) = (Vec::<[f32; 3]>::new(), Vec::<[f32; 3]>::new(), Vec::<[f32; 3]>::new());
        let mut rows_at: Option<i64> = None;
        for (j, line) in band.pixels.chunks_mut(width).enumerate() {
            let y = i64::from(band.y0) + j as i64;
            let dy = y as f64 + 0.5 - self.centre.1;
            let (from, to) = middle(dy);
            if self.cell == 1 {
                let Some(cells) = blur.row(y).and_then(|r| r.get(usize::try_from(x0 - blur.x0).ok()?..)) else { continue };
                for (first, end) in [(0, from), (to, width)] {
                    let (Some(pixels), Some(dx), Some(values)) = (line.get_mut(first..end), dxs.get(first..end), cells.get(first..end)) else {
                        continue;
                    };
                    for ((px, &dx), v) in pixels.iter_mut().zip(dx).zip(values) {
                        let w = self.weight(dx * dx + dy * dy);
                        if let (true, Some(v)) = (w > 0.0, cell(v)) {
                            self.mix_in(px, v, w);
                        }
                    }
                }
                continue;
            }
            // A pixel's centre lies a quarter of a cell from its cell's centre, toward the cells it
            // reads with weights ¾ and ¼: the two rows of cells around the pixel row (shared by an
            // odd row and the even one after it) mixed first, then the two cells around the pixel
            // (pixel `x` reads cells `(x − 1) div 2` and the next, ¾ of the way when `x` is even).
            let (r0, ty) = ((y - 1).div_euclid(2), if y % 2 == 0 { 0.75f32 } else { 0.25 });
            let Ok(k0) = usize::try_from((x0 - 1).div_euclid(2) - blur.x0) else { continue };
            if rows_at != Some(r0) {
                let cells = k0..k0 + width / 2 + 2;
                let unpack = |r: i64, out: &mut Vec<[f32; 3]>| {
                    out.clear();
                    let cells = blur.row(r).and_then(|r| r.get(cells.start.min(r.len())..cells.end.min(r.len()))).unwrap_or(&[]);
                    // A cell holding nothing is `NaN`.
                    out.extend(cells.iter().map(|v| cell(v).unwrap_or([f32::NAN; 3])));
                };
                // The row below the last pair is the row above this one.
                if rows_at == Some(r0 - 1) {
                    std::mem::swap(&mut above, &mut below);
                } else {
                    unpack(r0, &mut above);
                }
                unpack(r0 + 1, &mut below);
                rows_at = Some(r0);
            }
            row.clear();
            row.extend(above.iter().zip(&below).map(|(p, q)| std::array::from_fn(|c| p[c] + (q[c] - p[c]) * ty)));
            let (par, even) = (usize::from((x0 - 1) % 2 != 0), x0 % 2 == 0);
            for (first, end) in [(0, from), (to, width)] {
                let (Some(pixels), Some(dx)) = (line.get_mut(first..end), dxs.get(first..end)) else { continue };
                for (i, (px, &dx)) in (first..end).zip(pixels.iter_mut().zip(dx)) {
                    let w = self.weight(dx * dx + dy * dy);
                    let c = (i + par) / 2;
                    let (Some(p), Some(q)) = (row.get(c), row.get(c + 1)) else { continue };
                    if w <= 0.0 || p[0].is_nan() || q[0].is_nan() {
                        continue;
                    }
                    let tx = if (i % 2 == 0) == even { 0.75f32 } else { 0.25 };
                    self.mix_in(px, std::array::from_fn(|k| p[k] + (q[k] - p[k]) * tx), w);
                }
            }
        }
    }
}

// --- Shared -----------------------------------------------------------------------------------

/// A pass's `before` window on a grid of `cell × cell`-pixel cells anchored at the whole output's
/// top-left corner, its pixels read as [`View::get`] reads them (clamped into the picture, then
/// into the window), each cell their sum scaled to four times a pixel's level (`0 ..= 1020`). The
/// grid holds a cell more each way when a cell has more than one pixel: a pixel at the window's
/// edge reads the cells on both sides of it.
struct Grid {
    x0: i64,
    y0: i64,
    w: i64,
    h: i64,
    /// The whole output's size in cells.
    full: (i64, i64),
    data: Vec<[u16; 3]>,
}

impl Grid {
    fn of(view: &View<'_>, cell: u32) -> Option<Grid> {
        let c = i64::from(cell.clamp(1, 2));
        let (vx0, vy0, vw, vh) = (i64::from(view.x0), i64::from(view.y0), i64::from(view.width), i64::from(view.height));
        let row_bytes = usize::try_from(vw).ok()?.checked_mul(4)?;
        if vw <= 0 || vh <= 0 || view.data.len() < row_bytes.checked_mul(usize::try_from(vh).ok()?)? {
            return None;
        }
        let extra = i64::from(c > 1);
        let (x0, y0) = ((vx0 - extra).div_euclid(c), (vy0 - extra).div_euclid(c));
        let (w, h) = ((vx0 + vw - 1 + extra).div_euclid(c) - x0 + 1, (vy0 + vh - 1 + extra).div_euclid(c) - y0 + 1);
        let (fw, fh) = (i64::from(view.full_width.max(1)), i64::from(view.full_height.max(1)));
        // Each pixel column of the cells: its byte in a row of the window, clamped as `View::get`.
        let columns: Vec<usize> =
            (0..w * c).map(|k| usize::try_from(((x0 * c + k).clamp(0, fw - 1) - vx0).clamp(0, vw - 1)).unwrap_or(0) * 4).collect();
        let line = |y: i64| {
            let ly = usize::try_from(((y).clamp(0, fh - 1) - vy0).clamp(0, vh - 1)).unwrap_or(0);
            view.data.get(ly * row_bytes..(ly + 1) * row_bytes).unwrap_or(&[])
        };
        let mut data = alloc(usize::try_from(w.checked_mul(h)?).ok()?, [0u16; 3])?;
        par_rows(&mut data, usize::try_from(w).ok()?, |j, out| {
            let y = (y0 + i64::try_from(j).unwrap_or(i64::MAX)).saturating_mul(c);
            if c == 1 {
                let row = line(y);
                for (o, &at) in out.iter_mut().zip(&columns) {
                    if let Some(&[r, g, b]) = row.get(at..at + 3) {
                        *o = [u16::from(r) * 4, u16::from(g) * 4, u16::from(b) * 4];
                    }
                }
            } else {
                let (top, bottom) = (line(y), line(y + 1));
                for (o, &[a, b]) in out.iter_mut().zip(columns.as_chunks::<2>().0) {
                    let px = |row: &[u8], at: usize| match row.get(at..at + 3) {
                        Some(&[r, g, b]) => [u16::from(r), u16::from(g), u16::from(b)],
                        _ => [0; 3],
                    };
                    let (p, q, r, t) = (px(top, a), px(top, b), px(bottom, a), px(bottom, b));
                    *o = std::array::from_fn(|c| p[c] + q[c] + r[c] + t[c]);
                }
            }
        });
        Some(Grid { x0, y0, w, h, full: ((fw + c - 1) / c, (fh + c - 1) / c), data })
    }

    /// The grid's cells that lie in the whole output, `(first, last)` across and down: read
    /// straight, every other cell through [`Grid::at`].
    fn inner(&self) -> ((i64, i64), (i64, i64)) {
        ((self.x0.max(0), (self.x0 + self.w).min(self.full.0) - 1), (self.y0.max(0), (self.y0 + self.h).min(self.full.1) - 1))
    }

    /// The cells `p0` and `p0 + 1` across row (`vertical`) or column `m`.
    fn pair(&self, vertical: bool, m: i64, p0: i64) -> ([u16; 3], [u16; 3]) {
        let p1 = p0.saturating_add(1);
        if vertical { (self.at(p0, m), self.at(p1, m)) } else { (self.at(m, p0), self.at(m, p1)) }
    }

    /// The samples of [`SIDE`] rays at the positions `pos` across (fixed point, 2⁻³² cells) on row
    /// (`vertical`) or column `m`, each between the two nearest cells in 1/256 steps:
    /// `v[ray · 3 + channel]`.
    fn samples(&self, vertical: bool, m: i64, pos: &[i64; SIDE], v: &mut [u32; LANES]) {
        // Straight from the grid when every ray's two cells lie in it and in the output (the rays
        // are in order across, either way round).
        let (lo, hi) = (pos[0].min(pos[SIDE - 1]) >> 32, pos[0].max(pos[SIDE - 1]) >> 32);
        let (xs, ys) = self.inner();
        let ((m_lo, m_hi), (q_lo, q_hi)) = if vertical { (ys, xs) } else { (xs, ys) };
        if m >= m_lo && m <= m_hi && lo >= q_lo && hi < q_hi {
            // Cell `k` across at `base + k · stride`.
            let (base, stride) = if vertical { ((m - self.y0) * self.w - self.x0, 1) } else { (m - self.x0 - self.y0 * self.w, self.w) };
            for (&p, v) in pos.iter().zip(v.as_chunks_mut::<3>().0) {
                let i = usize::try_from(base + (p >> 32) * stride).unwrap_or(usize::MAX);
                let (Some(a), Some(b)) = (self.data.get(i), self.data.get(i.wrapping_add(stride as usize))) else { continue };
                let frac = ((p >> 24) & 0xff) as u32;
                for ((v, &a), &b) in v.iter_mut().zip(a).zip(b) {
                    *v = u32::from(a) * (LEVEL - frac) + u32::from(b) * frac;
                }
            }
            return;
        }
        for (&p, v) in pos.iter().zip(v.as_chunks_mut::<3>().0) {
            let (a, b) = self.pair(vertical, m, p >> 32);
            let frac = ((p >> 24) & 0xff) as u32;
            for ((v, &a), &b) in v.iter_mut().zip(&a).zip(&b) {
                *v = u32::from(a) * (LEVEL - frac) + u32::from(b) * frac;
            }
        }
    }

    /// The cell `(x, y)` of the whole output's grid, clamped into the output, then into the grid.
    fn at(&self, x: i64, y: i64) -> [u16; 3] {
        let lx = (x.clamp(0, self.full.0 - 1) - self.x0).clamp(0, self.w - 1);
        let ly = (y.clamp(0, self.full.1 - 1) - self.y0).clamp(0, self.h - 1);
        self.data.get((ly * self.w + lx) as usize).copied().unwrap_or([0; 3])
    }
}

/// A share of [`par_split`]'s data: its first unit's index and its units, until someone runs them.
type Slot<'a, T> = Mutex<Option<(usize, &'a mut [T])>>;

/// `f(first unit's index, units)` over pieces of `data` made of whole `unit`-long units (the last
/// may be shorter), split across threads, a few pieces a thread, each run by whoever is free.
///
/// Made from plain scoped threads, not `rayon`: it runs inside a pass's once-per-render
/// initialisation, which other bands wait on; a `rayon` job there could steal one of those bands
/// and wait on itself.
fn par_split<T: Send>(data: &mut [T], unit: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    if unit == 0 || data.is_empty() {
        return;
    }
    let units = data.len().div_ceil(unit);
    let threads = rayon::current_num_threads().clamp(1, 64).min(units);
    let per = units.div_ceil(threads.saturating_mul(4).min(units)).max(1);
    let slots: Vec<Slot<'_, T>> =
        data.chunks_mut(per.saturating_mul(unit)).enumerate().map(|(t, chunk)| Mutex::new(Some((t * per, chunk)))).collect();
    let run = || {
        for slot in &slots {
            if let Some((first, chunk)) = slot.lock().ok().and_then(|mut s| s.take()) {
                f(first, chunk);
            }
        }
    };
    std::thread::scope(|s| {
        for _ in 1..threads {
            let run = &run;
            // A thread that cannot be had leaves its pieces to the others.
            let _spawned = std::thread::Builder::new().spawn_scoped(s, run);
        }
        run();
    });
}

/// `f(row index, row)` over the `row_len`-long rows of `data`, split across threads
/// ([`par_split`]).
fn par_rows<T: Send>(data: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    par_split(data, row_len, |first, rows| {
        for (k, row) in rows.chunks_mut(row_len).enumerate() {
            f(first + k, row);
        }
    });
}

fn alloc<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small xorshift for the tests' pictures and places.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// A `w × h` RGBA picture of noise.
    fn noise(w: u32, h: u32, seed: u64) -> Vec<u8> {
        let mut rng = Rng(seed);
        (0..w * h * 4).map(|_| rng.next() as u8).collect()
    }

    fn view(data: &[u8], (x0, y0, w, h): (u32, u32, u32, u32), full: (u32, u32)) -> View<'_> {
        View { data, x0, y0, width: w, height: h, full_width: full.0, full_height: full.1 }
    }

    #[test]
    fn a_grid_read_straight_equals_the_clamped_read() {
        let data = noise(40, 30, 7);
        for cell in [1, 2] {
            for window in [(0, 0, 40, 30), (0, 0, 13, 9)] {
                let v = view(&data, window, (40, 30));
                let grid = Grid::of(&v, cell).unwrap();
                let mut rng = Rng(11);
                for _ in 0..2000 {
                    let vertical = rng.next().is_multiple_of(2);
                    let m = (rng.next() % 50) as i64 - 5;
                    let start = ((rng.next() % 60) as i64 - 10) << 32 | (rng.next() as i64 & 0xffff_ffff);
                    let step = (rng.next() % (1 << 32)) as i64 / 2;
                    let pos: [i64; SIDE] = std::array::from_fn(|r| start + step * r as i64);
                    let mut fast = [0u32; LANES];
                    grid.samples(vertical, m, &pos, &mut fast);
                    for (r, &p) in pos.iter().enumerate() {
                        let (a, b) = grid.pair(vertical, m, p >> 32);
                        let frac = ((p >> 24) & 0xff) as u32;
                        for c in 0..3 {
                            assert_eq!(fast[r * 3 + c], u32::from(a[c]) * (LEVEL - frac) + u32::from(b[c]) * frac, "cell {cell} {window:?} ray {r}");
                        }
                    }
                }
            }
        }
    }

    /// Each ray's box is the weighted mean of the samples it covers, every sample standing for a
    /// cell of the ray and counting by the length of it inside `[a − span · a, a]`, weighted by
    /// one over its distance from the centre: summed directly, sample by sample.
    #[test]
    fn a_box_is_the_weighted_mean_of_the_samples_it_covers() {
        let data = noise(64, 48, 3);
        let v = view(&data, (0, 0, 64, 48), (64, 48));
        let grid = Grid::of(&v, 1).unwrap();
        for (span, centre) in [(0.25, (20.3, 22.7)), (0.5, (3.5, 40.0)), (0.005, (31.0, 24.0)), (0.1, (-8.25, 10.5))] {
            let g = Geom { centre, full: (64.0, 48.0), inside: 0.0, span };
            let s = SECTORS[0];
            let (f, j0, j1) = wedges(s, &g, &grid).unwrap();
            let (mut prefix, mut rays) = (Vec::new(), Vec::new());
            for j in (j0..=j1).step_by(5) {
                let (m0, m1) = (f.m0.max(f.cm.floor() as i64 + 1), f.m1);
                rays.clear();
                rays.resize(usize::try_from(m1 - m0 + 1).unwrap() * SIDE, [u16::MAX; 3]);
                sweep(&f, j, (m0, m1), &grid, &mut prefix, &mut rays).unwrap();
                for m in m0..=m1 {
                    let a = m as f64 + 0.5 - f.cm;
                    let d = span * a;
                    for r in [0, 7, BLOCK] {
                        let ray = j + r as i64;
                        let (mut total, mut weight) = ([0.0f64; 3], 0.0);
                        for back in 0..=(d as i64 + 2) {
                            let am = a - back as f64;
                            let inside = (am + 0.5).min(a) - (am - 0.5).max(a - d.max(0.0));
                            let cover = if d < 0.5 { if back == 0 { 1.0 } else { 0.0 } } else { inside.max(0.0) };
                            if cover <= 0.0 {
                                continue;
                            }
                            let w = (WEIGHT_ONE / am.max(0.5)).round() * cover;
                            let p = position(&f, ray, m - back);
                            let (pa, pb) = grid.pair(false, m - back, p >> 32);
                            let frac = ((p >> 24) & 0xff) as u32;
                            for c in 0..3 {
                                total[c] += w * f64::from(u32::from(pa[c]) * (LEVEL - frac) + u32::from(pb[c]) * frac);
                            }
                            weight += w;
                        }
                        let got = rays[usize::try_from(m - m0).unwrap() * SIDE + r];
                        for c in 0..3 {
                            let want = total[c] / weight / 4.0;
                            assert!((f64::from(got[c]) - want).abs() <= 0.51, "span {span} ray {ray} at {m}: {} for {want}", got[c]);
                        }
                    }
                }
            }
        }
    }
}
