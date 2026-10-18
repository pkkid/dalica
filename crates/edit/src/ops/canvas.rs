//! The effects that change the picture's size, part one (`editing.md` §8.3–§8.4):
//! `widescreen(letterbox)` and `border(outer_color,outer,inner_color,inner,radius,caption)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - Widescreen Film: Letterbox (`true`): the 15 % black bars top and bottom. The picture is
//!     cropped to 1.7:1 either way.
//!   - Border: the outer colour (`#000000`) and thickness 0–100 (20), the inner colour
//!     (`#ffffff`) and thickness 0–100 (5), thicknesses in thousandths of the picture's shorter
//!     side; Corner radius 0–100 (0), in percent of the largest, where 100 rounds the frame into
//!     an oval; Caption height 0–200 (0), a band below the photo in thousandths of the shorter
//!     side.
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field, a colour that is not
//!   `#rrggbb` or a checkbox other than `true` / `false` is refused (the entry is kept and
//!   skipped).
//! - **Rendering** (M12.13, `VERSION` 1): both are frames ([`Reframe`], the renderer's third kind
//!   of operation): each puts the picture as it stands at its place among the effects on a canvas of
//!   its own size, in whole pixels, and paints it.
//!   - Widescreen Film crops the picture about its centre to 1.7:1 ([`RATIO`]; within a pixel),
//!     mutes its colour the way film stock does (less saturation, a gentle S-curve with lifted
//!     blacks and softened whites, cool shadows and warm highlights), adds a fine monochrome grain
//!     placed in the stored frame ([`crate::fx::Noise`], seeded by the effect's name, so Letterbox
//!     changes only the bars) and, with Letterbox, paints black bars over the top and bottom
//!     [`BAR`] (15 %) of the crop, their inner edges anti-aliased where they fall inside a row.
//!   - Border grows the canvas by the inner and then the outer band on every side (each band's
//!     thickness in thousandths of the shorter side of the picture it frames; the two together,
//!     and the caption, rounded to whole pixels, so the photo lands on whole pixels; the line
//!     between the bands anti-aliased where it falls inside a pixel) and by the caption height
//!     below, in the outer colour; the photo's pixels are copied unchanged. A corner radius
//!     rounds the photo's opening with elliptic corners whose radii are that percentage of half
//!     the photo's width and height (100: an oval), anti-aliased by 4 × 4 samples on the curve;
//!     the inner band follows the opening, the outer band fills the rest of the rectangular
//!     canvas.
//! - **Speed** (M13.42, the slider's 100 ms final): Widescreen Film works a row at a time: a
//!   row under a bar is filled black without its colour or grain, the colour runs over the row's
//!   flat slice, the grain is drawn a run of [`FILM_RUN`] pixels at a time ([`Noise::row`], within
//!   10⁻⁴ of [`Noise::at`]; the runs anchored at multiples of [`FILM_RUN`] in the whole canvas, so
//!   a region draws exactly the whole render's values) and only the row an inner bar edge crosses
//!   is darkened by its coverage.
//!
//! Our own design from `editing.md` §8.3–§8.4.

use std::fmt;

use super::{Band, Operation, ParamError, Pass, Reader, clamp, flag, hex, num};
use crate::fx::noise::hash01;
use crate::fx::{Noise, Space, luma, noise};
use crate::render::{Layout, Placed, Reframe, pixels};

/// The range of a Border band's thickness, thousandths of the shorter side.
pub const THICKNESS: (f64, f64) = (0.0, 100.0);
/// The range of Border's Corner radius, percent of the largest (100: an oval).
pub const CORNER: (f64, f64) = (0.0, 100.0);
/// The range of Border's Caption height, thousandths of the shorter side.
pub const CAPTION: (f64, f64) = (0.0, 200.0);
/// Widescreen Film's width to height.
pub const RATIO: f64 = 1.7;
/// Widescreen Film's letterbox bars, each a fraction of the crop's height.
pub const BAR: f64 = 0.15;
/// Widescreen Film's grain: its strength (in 0..1 working values, one standard deviation) and its
/// cell, relative to the stored photo's shorter side.
const GRAIN: f32 = 0.03;
const GRAIN_CELL: f64 = 0.0012;
/// Widescreen Film draws its grain in runs of this many pixels along a row, anchored at its
/// multiples in the whole canvas ([`Noise::row`]).
pub const FILM_RUN: u32 = 256;

/// `widescreen(letterbox)`: crop to 1.7:1, muted filmic colour, grain, and with `letterbox` black
/// bars top and bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widescreen {
    pub letterbox: bool,
}

impl Default for Widescreen {
    fn default() -> Widescreen {
        Widescreen { letterbox: true }
    }
}

/// `border(outer_color,outer,inner_color,inner,radius,caption)`: a two-band frame around the
/// photo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub outer_color: [u8; 3],
    /// Thousandths of the shorter side.
    pub outer: f64,
    pub inner_color: [u8; 3],
    /// Thousandths of the shorter side.
    pub inner: f64,
    /// Percent of the largest corner radius (100: an oval).
    pub radius: f64,
    /// Thousandths of the shorter side.
    pub caption: f64,
}

impl Default for Border {
    fn default() -> Border {
        Border { outer_color: [0, 0, 0], outer: 20.0, inner_color: [0xff, 0xff, 0xff], inner: 5.0, radius: 0.0, caption: 0.0 }
    }
}

impl fmt::Display for Widescreen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(flag(self.letterbox))
    }
}

impl Operation for Widescreen {
    const NAME: &'static str = "widescreen";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Widescreen Film";

    fn parse(params: &str) -> Result<Widescreen, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let w = Widescreen { letterbox: r.flag("letterbox")? };
        r.end()?;
        Ok(w)
    }
}

impl fmt::Display for Border {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            hex(self.outer_color),
            num(self.outer),
            hex(self.inner_color),
            num(self.inner),
            num(self.radius),
            num(self.caption)
        )
    }
}

impl Operation for Border {
    const NAME: &'static str = "border";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Border";

    fn parse(params: &str) -> Result<Border, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = Border {
            outer_color: r.colour("the outer colour")?,
            outer: r.number("the outer thickness", THICKNESS)?,
            inner_color: r.colour("the inner colour")?,
            inner: r.number("the inner thickness", THICKNESS)?,
            radius: r.number("the corner radius", CORNER)?,
            caption: r.number("the caption height", CAPTION)?,
        };
        r.end()?;
        Ok(b)
    }

    fn clamped(self) -> Border {
        Border {
            outer: clamp(self.outer, THICKNESS),
            inner: clamp(self.inner, THICKNESS),
            radius: clamp(self.radius, CORNER),
            caption: clamp(self.caption, CAPTION),
            ..self
        }
    }
}

impl Widescreen {
    /// The crop of a `width × height` picture: its size (1.7:1 within a pixel, at least one pixel).
    fn crop(width: u32, height: u32) -> Option<(u32, u32)> {
        let (w, h) = (f64::from(width), f64::from(height));
        if w > h * RATIO {
            Some((pixels(h * RATIO)?.clamp(1, width.max(1)), height))
        } else {
            Some((width, pixels(w / RATIO)?.clamp(1, height.max(1))))
        }
    }
}

impl Reframe for Widescreen {
    fn layout(&self, width: u32, height: u32) -> Option<Layout> {
        let (w, h) = Widescreen::crop(width, height)?;
        // Centred: the input's first pixel lands left of and above the canvas.
        let at = (-(i64::from(width.saturating_sub(w)) / 2), -(i64::from(height.saturating_sub(h)) / 2));
        Some(Layout { size: (w, h), at })
    }

    fn prepare(&self, placed: &Placed<'_>) -> Option<Box<dyn Pass>> {
        let (_, h) = placed.layout.size;
        let bar = if self.letterbox { f64::from(h) * BAR } else { 0.0 };
        // Seeded by the name alone, so Letterbox changes the bars and nothing else.
        let seed = noise::seed(Self::NAME);
        let grain = Space::new(placed.ctx).map(|space| FilmGrain { space, noise: Noise::new(seed, GRAIN_CELL), seed });
        Some(Box::new(Film { height: h, bar, grain }))
    }
}

/// Widescreen Film's colour, grain and bars over the cropped canvas.
struct Film {
    height: u32,
    /// The bars' height in rows (0: none).
    bar: f64,
    grain: Option<FilmGrain>,
}

/// Widescreen Film's grain: the noise field in this render's space.
struct FilmGrain {
    space: Space,
    noise: Noise,
    /// The noise's seed.
    seed: u32,
}

impl Film {
    /// The muted filmic colour of one pixel.
    fn colour(p: [f32; 3]) -> [f32; 3] {
        let l = luma(p);
        let tone = (l - 0.5) * 0.06;
        let shift = [tone, 0.0, -tone];
        let mut out = [0.0; 3];
        for c in 0..3 {
            let v = (l + (p[c] - l) * 0.72).clamp(0.0, 1.0);
            let s = v * v * (3.0 - 2.0 * v);
            let v = v + 0.25 * (s - v);
            out[c] = 0.03 + 0.93 * v + shift[c];
        }
        out
    }
}

impl Film {
    /// How much of the row `y` the bars cover: 0 between them, 1 under them, a fraction on the
    /// row an inner edge crosses.
    fn covered(&self, y: u32) -> f32 {
        let (top, bottom) = (f64::from(y), f64::from(self.height) - f64::from(y) - 1.0);
        (self.bar - top).clamp(0.0, 1.0).max((self.bar - bottom).clamp(0.0, 1.0)) as f32
    }
}

impl Pass for Film {
    fn run(&self, band: &mut Band<'_>) {
        let width = band.width as usize;
        if width == 0 {
            return;
        }
        let mut noise = [0.0f32; FILM_RUN as usize];
        let mut corners = Corners::default();
        let x1 = band.x0.saturating_add(band.width);
        for (j, row) in band.pixels.chunks_exact_mut(width).enumerate() {
            let y = band.y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            let covered = self.covered(y);
            if covered >= 1.0 {
                // Under a bar: black, without the colour or the grain.
                row.fill([0.0; 3]);
                continue;
            }
            for p in row.iter_mut() {
                *p = Film::colour(*p);
            }
            if let Some(grain) = &self.grain {
                // The row in runs anchored at multiples of `FILM_RUN` in the whole canvas, so a
                // region's pixels draw exactly the whole render's values.
                let mut start = band.x0 - band.x0 % FILM_RUN;
                while start < x1 {
                    let end = start.saturating_add(FILM_RUN).min(x1);
                    let Some(run) = noise.get_mut(..(end - start) as usize) else { break };
                    grain.run(start, y, run, &mut corners);
                    let skip = band.x0.saturating_sub(start) as usize;
                    let at = (start.max(band.x0) - band.x0) as usize;
                    let Some(pixels) = row.get_mut(at..(end - band.x0) as usize) else { break };
                    for (p, n) in pixels.iter_mut().zip(run.iter().skip(skip)) {
                        let g = n * GRAIN;
                        *p = p.map(|c| c + g);
                    }
                    start = end;
                }
            }
            if covered > 0.0 {
                // The row a bar's anti-aliased inner edge crosses.
                for p in row.iter_mut() {
                    *p = p.map(|c| c * (1.0 - covered));
                }
            }
        }
    }
}

/// The lattice corners [`FilmGrain::run_values`] hashes along one run at most: two lattice cells a
/// pixel, and the corner past the last.
const CORNERS: usize = 2 * FILM_RUN as usize + 3;

/// [`FilmGrain::run_values`]'s scratch: the hashed corners of two lattice rows and, per cell, its
/// top-left corner, the step to the top-right one and the same below.
struct Corners {
    hashes: [[f32; CORNERS]; 2],
    cells: [[f32; 4]; CORNERS],
}

impl Default for Corners {
    fn default() -> Corners {
        Corners { hashes: [[0.0; CORNERS]; 2], cells: [[0.0; 4]; CORNERS] }
    }
}

impl FilmGrain {
    /// [`Noise::row`] for the pixels `(start, y) … (start + out.len() − 1, y)` (at most
    /// [`FILM_RUN`]), bit for bit, into `out`.
    fn run(&self, start: u32, y: u32, out: &mut [f32], scratch: &mut Corners) {
        self.run_values(start, y, out, scratch);
        let k = self.noise.attenuation(&self.space);
        if k != 1.0 {
            for v in out.iter_mut() {
                *v *= k;
            }
        }
    }

    /// [`Noise::row_values`] for the pixels `(start, y) … (start + out.len() − 1, y)` (at most
    /// [`FILM_RUN`]), bit for bit, into `out`; `scratch` holds the run's corners.
    ///
    /// Where the run stays on one row of lattice cells and steps forward by at most two cells a
    /// pixel (a render that is not turned or mirrored, its shorter side at least about 420 pixels:
    /// the edit view's final and a Save), each lattice corner along it is hashed once, in a loop
    /// that vectorises, rather than four per cell the row enters; the stored-frame point, the cell
    /// and the smoothstep blend are those `row_values` computes. Elsewhere it is `row_values`
    /// itself. Answers whether the run took the fast path. (Film Grain's sampler of M13.36, for
    /// this effect's own noise; `fx::noise` is shared by tasks running beside this one.)
    fn run_values(&self, start: u32, y: u32, out: &mut [f32], scratch: &mut Corners) -> bool {
        // `row_values`'s affine step along the row.
        const STRIDE: u32 = 1024;
        let cell = self.noise.cell();
        let (u0, v0) = self.space.pixel_units(start, y);
        let (u1, v1) = self.space.pixel_units(start.saturating_add(STRIDE), y);
        let span = f64::from(start.saturating_add(STRIDE) - start).max(1.0);
        let (x, v) = (u0 / cell, v0 / cell);
        let (dx, dv) = ((u1 - u0) / span / cell, (v1 - v0) / span / cell);
        let last = out.len().saturating_sub(1) as f64;
        let (x_end, v_end) = (x + dx * last, v + dv * last);
        let inside = |t: f64| t.is_finite() && t.abs() < 1e9;
        let row = lattice_floor(v);
        let fast = !out.is_empty()
            && out.len() <= FILM_RUN as usize
            && [x, v, x_end, v_end].into_iter().all(inside)
            && dx > 0.0
            && dx <= 2.0
            && lattice_floor(v_end) == row;
        if !fast {
            self.noise.row_values(&self.space, start, y, 0, out);
            return false;
        }
        let Corners { hashes, cells } = scratch;
        // The two lattice rows' corners from the first pixel's cell to past the last's, hashed
        // once; then per cell its top-left corner and the step to the top-right one, and the same
        // below (the differences the blend takes, made once per cell).
        let first = lattice_floor(x);
        let count = usize::try_from(lattice_floor(x_end) - first + 2).unwrap_or(0).min(CORNERS);
        let [top, bottom] = hashes;
        let (top, bottom) = (top.get_mut(..count).unwrap_or(&mut []), bottom.get_mut(..count).unwrap_or(&mut []));
        let (j0, j1) = (row as i32, (row + 1) as i32);
        for (k, (t, b)) in top.iter_mut().zip(bottom.iter_mut()).enumerate() {
            let i = (first as i32).wrapping_add(k as i32);
            *t = 2.0 * hash01(i, j0, 0, self.seed) - 1.0;
            *b = 2.0 * hash01(i, j1, 0, self.seed) - 1.0;
        }
        for (c, ((t, t1), (b, b1))) in cells.iter_mut().zip(top.iter().zip(top.iter().skip(1)).zip(bottom.iter().zip(bottom.iter().skip(1)))) {
            *c = [*t, *t1 - *t, *b, *b1 - *b];
        }
        let spread = noise::SPREAD as f32;
        let (first, row) = (first as f64, row as f64);
        let smooth = |f: f32| f * f * (3.0 - 2.0 * f);
        // The pixels step forward along the run, so each one's cell is found by stepping past
        // the lattice lines it crossed (exact integers in `f64`: `⌊px⌋` itself).
        let (mut at, mut next) = (0usize, first + 1.0);
        let mut blend = |o: &mut f32, px: f64, sv: f32| {
            while px >= next {
                next += 1.0;
                at += 1;
            }
            let su = smooth((px - (next - 1.0)) as f32);
            let [t0, dt, b0, db] = cells.get(at).copied().unwrap_or([0.0; 4]);
            let (t, b) = (t0 + dt * su, b0 + db * su);
            *o = (t + (b - t) * sv) / spread;
        };
        if dv == 0.0 {
            // On a row of the stored frame the run's place between the lattice rows is the same
            // at every pixel.
            let sv = smooth((v - row) as f32);
            for (k, o) in out.iter_mut().enumerate() {
                blend(o, x + dx * k as f64, sv);
            }
        } else {
            for (k, o) in out.iter_mut().enumerate() {
                let k = k as f64;
                blend(o, x + dx * k, smooth((v + dv * k - row) as f32));
            }
        }
        true
    }
}

/// `⌊v⌋` of a finite `v` inside ±10⁹, as the noise's row sampler takes it.
fn lattice_floor(v: f64) -> i64 {
    let t = v as i64;
    if (t as f64) > v { t - 1 } else { t }
}

impl Border {
    /// The bands of a frame around a `width × height` picture, in pixels: outer, inner, caption.
    fn bands(&self, width: u32, height: u32) -> (f64, f64, f64) {
        let b = self.clamped();
        let short = f64::from(width.min(height));
        (b.outer / 1000.0 * short, b.inner / 1000.0 * short, b.caption / 1000.0 * short)
    }
}

impl Reframe for Border {
    fn layout(&self, width: u32, height: u32) -> Option<Layout> {
        let (outer, inner, caption) = self.bands(width, height);
        let (band, caption) = (pixels(outer + inner)?, pixels(caption)?);
        let both = band.checked_mul(2)?;
        let size = (width.checked_add(both)?, height.checked_add(both)?.checked_add(caption)?);
        Some(Layout { size, at: (i64::from(band), i64::from(band)) })
    }

    fn prepare(&self, placed: &Placed<'_>) -> Option<Box<dyn Pass>> {
        let (_, inner, _) = self.bands(placed.input.0, placed.input.1);
        let (l, t) = (placed.layout.at.0 as f64, placed.layout.at.1 as f64);
        let (w, h) = (f64::from(placed.input.0), f64::from(placed.input.1));
        let k = self.clamped().radius / 100.0;
        let photo = Shape { l, t, r: l + w, b: t + h, rx: k * w / 2.0, ry: k * h / 2.0 };
        let i = inner;
        let grow = |r: f64| if r > 0.0 { r + i } else { 0.0 };
        let band = Shape { l: l - i, t: t - i, r: l + w + i, b: t + h + i, rx: grow(photo.rx), ry: grow(photo.ry) };
        let rgb = |c: [u8; 3]| c.map(|v| f32::from(v) / 255.0);
        Some(Box::new(Frame { photo, band, outer: rgb(self.outer_color), inner: rgb(self.inner_color) }))
    }
}

/// A rectangle in canvas pixels with elliptic corners of radii `rx`, `ry` (none when either is 0).
#[derive(Clone, Copy, Debug)]
struct Shape {
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    rx: f64,
    ry: f64,
}

impl Shape {
    fn rounded(&self) -> bool {
        self.rx > 0.0 && self.ry > 0.0
    }

    /// Whether the point `(x, y)` is inside.
    fn contains(&self, x: f64, y: f64) -> bool {
        if x < self.l || x >= self.r || y < self.t || y >= self.b {
            return false;
        }
        if !self.rounded() {
            return true;
        }
        let dx = (self.l + self.rx - x).max(x - (self.r - self.rx)).max(0.0);
        let dy = (self.t + self.ry - y).max(y - (self.b - self.ry)).max(0.0);
        dx == 0.0 || dy == 0.0 || (dx / self.rx).powi(2) + (dy / self.ry).powi(2) <= 1.0
    }

    /// How much of the pixel `(x, y)` is inside: the area it shares with the rectangle (exactly 0
    /// or 1 along sides that are whole pixels), or 4 × 4 samples where a rounded corner crosses it.
    fn coverage(&self, x: u32, y: u32) -> f32 {
        let (x, y) = (f64::from(x), f64::from(y));
        let straight = (x >= self.l + self.rx && x + 1.0 <= self.r - self.rx) || (y >= self.t + self.ry && y + 1.0 <= self.b - self.ry);
        if !self.rounded() || straight {
            let ox = ((x + 1.0).min(self.r) - x.max(self.l)).clamp(0.0, 1.0);
            let oy = ((y + 1.0).min(self.b) - y.max(self.t)).clamp(0.0, 1.0);
            return (ox * oy) as f32;
        }
        if x + 1.0 <= self.l || x >= self.r || y + 1.0 <= self.t || y >= self.b {
            return 0.0;
        }
        let mut n = 0u32;
        for j in 0..4 {
            for i in 0..4 {
                n += u32::from(self.contains(x + (f64::from(i) + 0.5) / 4.0, y + (f64::from(j) + 0.5) / 4.0));
            }
        }
        n as f32 / 16.0
    }
}

/// Border's bands over the canvas: the photo inside its opening, the inner band around it, the
/// outer band (and the caption) elsewhere.
struct Frame {
    photo: Shape,
    band: Shape,
    outer: [f32; 3],
    inner: [f32; 3],
}

impl Pass for Frame {
    fn run(&self, band: &mut Band<'_>) {
        let width = band.width as usize;
        if width == 0 {
            return;
        }
        let (x0, y0) = (band.x0, band.y0);
        for (i, p) in band.pixels.iter_mut().enumerate() {
            let x = x0.saturating_add((i % width) as u32);
            let y = y0.saturating_add((i / width) as u32);
            let photo = self.photo.coverage(x, y);
            if photo >= 1.0 {
                continue;
            }
            let inner = self.band.coverage(x, y).max(photo);
            for ((v, i), o) in p.iter_mut().zip(self.inner).zip(self.outer) {
                *v = *v * photo + i * (inner - photo) + o * (1.0 - inner);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Recipe, render::Geometry};
    use dalica_image::Orientation;

    /// A small xorshift for the tests' places.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u32) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % u64::from(n.max(1))) as u32
        }
    }

    /// Widescreen Film's grain runs give bit for bit `Noise::row`: the fast path where a run stays
    /// on one row of lattice cells (taken on an upright render at 1:1 and at half size), `row_values`
    /// itself on a straightened, turned or small render (attenuated there).
    #[test]
    fn grain_runs_equal_the_noise_rows() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let cases: [(&[&str], u8, u32, u32, f64); 5] = [
            (&[], 1, 2000, 1500, 1.0),
            (&[], 1, 4000, 3000, 0.5),
            (&["straighten(3)"], 1, 2000, 1500, 0.8),
            (&["crop(0.1,0.2,0.9,0.8)"], 6, 2000, 1500, 0.5),
            (&[], 1, 2000, 1500, 0.1),
        ];
        let mut fast = 0usize;
        for (ops, o, w, h, scale) in cases {
            let g = Geometry::new(&Recipe::parse(ops).unwrap(), Orientation::new(o).unwrap(), w, h);
            let (ow, oh) = g.output_size(scale).unwrap();
            let space = Space::from_geometry(&g, ow, oh).unwrap();
            let grain = FilmGrain { space, noise: Noise::new(77, GRAIN_CELL), seed: 77 };
            let mut corners = Corners::default();
            for _ in 0..200 {
                let (start, y) = (rng.below(ow), rng.below(oh));
                let n = (1 + rng.below(FILM_RUN)).min(ow - start) as usize;
                let (mut got, mut want, mut values) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
                fast += usize::from(grain.run_values(start, y, &mut values, &mut corners));
                grain.run(start, y, &mut got, &mut corners);
                grain.noise.row(&grain.space, start, y, 0, &mut want);
                let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&got), bits(&want), "{ops:?} at {scale}: ({start}, {y}) × {n}");
            }
        }
        assert!(fast >= 300, "the fast path ran {fast} times");
    }
}
