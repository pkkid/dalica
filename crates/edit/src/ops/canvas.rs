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
//!
//! Our own design from `editing.md` §8.3–§8.4.

use std::fmt;

use super::{Band, Operation, ParamError, Pass, Reader, clamp, flag, hex, num};
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
        let grain = Space::new(placed.ctx).map(|space| (space, Noise::new(noise::seed(Self::NAME), GRAIN_CELL)));
        Some(Box::new(Film { height: h, bar, grain }))
    }
}

/// Widescreen Film's colour, grain and bars over the cropped canvas.
struct Film {
    height: u32,
    /// The bars' height in rows (0: none).
    bar: f64,
    grain: Option<(Space, Noise)>,
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

impl Pass for Film {
    fn run(&self, band: &mut Band<'_>) {
        let width = band.width as usize;
        if width == 0 {
            return;
        }
        let (x0, y0) = (band.x0, band.y0);
        for (i, p) in band.pixels.iter_mut().enumerate() {
            let x = x0.saturating_add((i % width) as u32);
            let y = y0.saturating_add((i / width) as u32);
            // How much of the row the bars cover.
            let (top, bottom) = (f64::from(y), f64::from(self.height) - f64::from(y) - 1.0);
            let covered = (self.bar - top).clamp(0.0, 1.0).max((self.bar - bottom).clamp(0.0, 1.0)) as f32;
            if covered >= 1.0 {
                *p = [0.0; 3];
                continue;
            }
            let mut v = Film::colour(*p);
            if let Some((space, noise)) = &self.grain {
                let g = noise.at(space, x, y, 0) * GRAIN;
                v = v.map(|c| c + g);
            }
            *p = v.map(|c| c * (1.0 - covered));
        }
    }
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
