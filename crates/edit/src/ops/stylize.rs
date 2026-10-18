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
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.12: Vignette from
//!   LightCraft `crates/pipeline/src/finish.rs` (`vignette`) at 0da998f2, Pixelate from PhotoCraft
//!   `crates/algo/src/stylize.rs` (`mosaic`) and Focal Zoom from PhotoCraft
//!   `crates/algo/src/blur.rs` (`radial`) and `crates/algo/src/blur/radial.rs` at e4b2868a
//!   (`NOTICE`). The blend modes are M12.3's [`Blend`].

use std::fmt;

use super::{Adjust, FADE, Operation, ParamError, Pass, Prepare, Reader, UNIT, clamp, clamp_point, hex, num, num_point};
use crate::frame::Point;
use crate::fx::Blend;

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
    const VERSION: u32 = 0;
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
    const VERSION: u32 = 0;
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
    const VERSION: u32 = 0;
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

// Stubs until M12.12: each changes nothing.

impl Adjust for Vignette {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Pixelate {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for FocalZoom {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
