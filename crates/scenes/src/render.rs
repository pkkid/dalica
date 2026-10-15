//! Seeded procedural pictures. Coordinates are normalised (the same scene at two sizes is the same
//! picture), and only `+ − × ÷ √` are used, so results are identical on every platform.

use crate::{Error, MAX_PIXELS, MAX_SIDE, Result};

/// 8-bit RGB pixels, rows top to bottom, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgb8 {
    /// A black image; `Err` for sizes outside the scene limits.
    pub fn new(width: u32, height: u32) -> Result<Rgb8> {
        check_size(width, height)?;
        Ok(Rgb8 { width, height, data: vec![0; width as usize * height as usize * 3] })
    }

    /// The pixel at `(x, y)`, or `None` outside the image.
    pub fn get(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        Some([*self.data.get(i)?, *self.data.get(i + 1)?, *self.data.get(i + 2)?])
    }

    pub(crate) fn set(&mut self, x: u32, y: u32, px: [u8; 3]) {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        if let Some(slot) = self.data.get_mut(i..i + 3) {
            slot.copy_from_slice(&px);
        }
    }

    /// A box-filtered copy that fits in `long_side` pixels on its longer side (never enlarged).
    pub fn downscale(&self, long_side: u32) -> Result<Rgb8> {
        let long = self.width.max(self.height).max(1);
        let scale = |v: u32| ((u64::from(v) * u64::from(long_side.min(long))) / u64::from(long)).max(1) as u32;
        let (w, h) = (scale(self.width), scale(self.height));
        let mut out = Rgb8::new(w, h)?;
        for y in 0..h {
            let (y0, y1) = span(y, h, self.height);
            for x in 0..w {
                let (x0, x1) = span(x, w, self.width);
                let mut sum = [0u64; 3];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        if let Some(p) = self.get(sx, sy) {
                            for (s, c) in sum.iter_mut().zip(p) {
                                *s += u64::from(c);
                            }
                        }
                    }
                }
                let n = u64::from((x1 - x0) * (y1 - y0)).max(1);
                out.set(x, y, sum.map(|s| ((s + n / 2) / n) as u8));
            }
        }
        Ok(out)
    }
}

/// Source pixels `[start, end)` that output pixel `i` of `out` covers in a `src`-pixel axis.
fn span(i: u32, out: u32, src: u32) -> (u32, u32) {
    let start = (u64::from(i) * u64::from(src) / u64::from(out)) as u32;
    let end = ((u64::from(i) + 1) * u64::from(src)).div_ceil(u64::from(out)) as u32;
    (start, end.max(start + 1).min(src))
}

pub(crate) fn check_size(width: u32, height: u32) -> Result<()> {
    let ok = (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height) && u64::from(width) * u64::from(height) <= MAX_PIXELS;
    if ok { Ok(()) } else { Err(Error::Size(width, height)) }
}

/// What a scene shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A two-colour gradient at an angle.
    Gradient,
    /// Sky over ground, a sloping horizon and a sun.
    Horizon,
    /// Soft coloured blobs over a gradient.
    Blobs,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Gradient, Kind::Horizon, Kind::Blobs];
}

/// One procedural picture: the same scene always renders the same pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scene {
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub kind: Kind,
}

impl Scene {
    /// A scene whose kind is chosen by the seed.
    pub fn new(seed: u64, width: u32, height: u32) -> Scene {
        let kind = Kind::ALL.get((Rng::new(seed ^ 0x5eed).next() % 3) as usize).copied().unwrap_or(Kind::Gradient);
        Scene { seed, width, height, kind }
    }

    pub fn with_kind(self, kind: Kind) -> Scene {
        Scene { kind, ..self }
    }

    /// The upright picture. A small light square near the top-left corner marks "up and left", so
    /// every EXIF orientation of it is a different image.
    pub fn render(&self) -> Result<Rgb8> {
        let mut img = Rgb8::new(self.width, self.height)?;
        let mut rng = Rng::new(self.seed);
        let paint = Painter::new(self.kind, &mut rng, self.width, self.height);
        let (w, h) = (self.width as f32, self.height as f32);
        let marker = (self.width.min(self.height) / 16).max(1);
        for y in 0..self.height {
            let v = (y as f32 + 0.5) / h;
            for x in 0..self.width {
                let u = (x as f32 + 0.5) / w;
                let mut c = paint.at(u, v);
                if (marker..2 * marker).contains(&x) && (marker..2 * marker).contains(&y) {
                    c = mix(c, [0.97, 0.97, 0.92], 0.85);
                }
                img.set(x, y, c.map(to_u8));
            }
        }
        Ok(img)
    }
}

type Rgb = [f32; 3];

fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Blob {
    x: f32,
    y: f32,
    r: f32,
    color: Rgb,
    alpha: f32,
}

impl Blob {
    /// Coverage at `(u, v)` in 0..1: a smooth disc that fades out at its radius.
    fn cover(&self, u: f32, v: f32, aspect: f32) -> f32 {
        let (dx, dy) = ((u - self.x) * aspect, v - self.y);
        let d = (dx * dx + dy * dy).sqrt();
        smooth(1.0 - d / self.r) * self.alpha
    }
}

/// The per-scene parameters, drawn from the seed once.
struct Painter {
    kind: Kind,
    aspect: f32,
    a: Rgb,
    b: Rgb,
    c: Rgb,
    dir: (f32, f32),
    horizon: f32,
    slope: f32,
    blobs: Vec<Blob>,
}

impl Painter {
    fn new(kind: Kind, rng: &mut Rng, width: u32, height: u32) -> Painter {
        let aspect = width as f32 / height as f32;
        let (dx, dy) = (rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let mut p = Painter {
            kind,
            aspect,
            a: rng.color(),
            b: rng.color(),
            c: rng.color(),
            dir: (dx / len, dy / len),
            horizon: rng.range(0.35, 0.65),
            slope: rng.range(-0.15, 0.15),
            blobs: Vec::new(),
        };
        p.blobs = match kind {
            Kind::Gradient => Vec::new(),
            // The sun: above the horizon.
            Kind::Horizon => {
                vec![Blob { x: rng.range(0.15, 0.85), y: rng.range(0.08, 0.25), r: rng.range(0.05, 0.12), color: [1.0, 0.93, 0.7], alpha: 0.95 }]
            }
            Kind::Blobs => {
                let n = 3 + rng.next() % 5;
                (0..n)
                    .map(|_| Blob { x: rng.unit(), y: rng.unit(), r: rng.range(0.08, 0.35), color: rng.color(), alpha: rng.range(0.5, 0.9) })
                    .collect()
            }
        };
        p
    }

    fn at(&self, u: f32, v: f32) -> Rgb {
        let mut c = match self.kind {
            Kind::Gradient | Kind::Blobs => {
                let t = ((u - 0.5) * self.dir.0 + (v - 0.5) * self.dir.1) * 1.2 + 0.5;
                mix(self.a, self.b, smooth(t))
            }
            Kind::Horizon => {
                let line = self.horizon + self.slope * (u - 0.5);
                if v < line {
                    // Sky: darker at the top, a pale band near the horizon.
                    mix(darken(self.a, 0.55), lighten(self.a, 0.35), smooth(v / line))
                } else {
                    // Ground: a near colour at the bottom, hazy towards the horizon.
                    mix(lighten(self.c, 0.2), darken(self.c, 0.6), smooth((v - line) / (1.0 - line).max(1e-3)))
                }
            }
        };
        for blob in &self.blobs {
            c = mix(c, blob.color, blob.cover(u, v, self.aspect));
        }
        c
    }
}

fn darken(c: Rgb, by: f32) -> Rgb {
    c.map(|v| v * (1.0 - by))
}

fn lighten(c: Rgb, by: f32) -> Rgb {
    c.map(|v| v + (1.0 - v) * by)
}

/// SplitMix64: small, fast, fully deterministic.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`, from the top 24 bits.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// A colour that is neither too dark nor washed out.
    fn color(&mut self) -> Rgb {
        [self.range(0.12, 0.92), self.range(0.12, 0.92), self.range(0.12, 0.92)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_outside_the_limits_are_refused() {
        for (w, h) in [(0, 10), (10, 0), (MAX_SIDE + 1, 1), (16_384, 16_384)] {
            assert!(matches!(Scene::new(1, w, h).render(), Err(Error::Size(..))), "{w}x{h}");
        }
        assert!(Scene::new(1, 1, 1).render().is_ok());
    }

    #[test]
    fn the_corner_marker_is_light() {
        let img = Scene::new(3, 160, 120).render().unwrap();
        let [r, g, b] = img.get(12, 12).unwrap();
        assert!(r > 180 && g > 180 && b > 170, "{r} {g} {b}");
    }

    #[test]
    fn downscale_fits_the_long_side_and_never_enlarges() {
        let img = Scene::new(1, 300, 200).render().unwrap();
        let t = img.downscale(160).unwrap();
        assert_eq!((t.width, t.height), (160, 106));
        let same = img.downscale(1000).unwrap();
        assert_eq!(same, img);
        let flat = Rgb8 { width: 2, height: 1, data: vec![10, 20, 30, 30, 40, 50] };
        assert_eq!(flat.downscale(1).unwrap().data, [20, 30, 40]);
    }
}
