//! The crosshair effects of tab 3 (`editing.md` §8.2): `soft_focus(x,y,size,amount)`,
//! `focal_bw(x,y,size,sharpness)` and `graduated_tint(x,y,angle,feather,shade,color)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): the crosshair's point
//!   `x,y`, normalised in the stored frame (`data-model.md` §3.3) and brought into 0..1, the
//!   centre (0.5, 0.5) by default; mapped into the output through [`super::Prepare::geometry`].
//!   - Soft Focus: Size 0–1 (0.3), the sharp circle's radius as a fraction of the picture's shorter
//!     side; Amount 0–1 (0.5), how strongly the outside blurs.
//!   - Focal B&W: Size 0–1 (0.3), as Soft Focus; Sharpness 0–1 (0.5), the circle's edge from soft
//!     (0) to hard (1).
//!   - Graduated Tint: the gradient's start `x,y`; Angle 0–360 (0), the direction from the start
//!     toward the tinted side in degrees clockwise from pointing up (0: a sky; as
//!     [`crate::fx::Gradient`] takes it);
//!     Feather 0–1 (0.25), the ramp from the start to the full tint as a fraction of the shorter
//!     side (0 a hard edge); Shade 0–1 (0.25), how deep the colour; the colour (`#9cc8ff`, a light
//!     blue). Pixels behind the start are unchanged and whites stay white.
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that is not
//!   `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.8, on M12.3's masks and
//!   blur.

use std::fmt;

use super::{Adjust, DEGREES, Operation, ParamError, Pass, Prepare, Reader, UNIT, clamp, clamp_point, hex, num, num_point};
use crate::frame::Point;

/// The range of a focal circle's Size: its radius as a fraction of the shorter side.
pub const SIZE: (f64, f64) = UNIT;
/// The range of Soft Focus's Amount.
pub const AMOUNT: (f64, f64) = UNIT;
/// The range of Focal B&W's Sharpness.
pub const SHARPNESS: (f64, f64) = UNIT;
/// The range of Graduated Tint's Angle, degrees clockwise from pointing up.
pub const ANGLE: (f64, f64) = DEGREES;
/// The range of Graduated Tint's Feather, a fraction of the shorter side.
pub const FEATHER: (f64, f64) = UNIT;
/// The range of Graduated Tint's Shade.
pub const SHADE: (f64, f64) = UNIT;

/// The picture's centre, where a crosshair starts.
const CENTRE: Point = Point { x: 0.5, y: 0.5 };

/// `soft_focus(x,y,size,amount)`: the picture blurred outside a sharp circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftFocus {
    pub centre: Point,
    pub size: f64,
    pub amount: f64,
}

impl Default for SoftFocus {
    fn default() -> SoftFocus {
        SoftFocus { centre: CENTRE, size: 0.3, amount: 0.5 }
    }
}

/// `focal_bw(x,y,size,sharpness)`: colour inside a circle, B&W outside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocalBw {
    pub centre: Point,
    pub size: f64,
    pub sharpness: f64,
}

impl Default for FocalBw {
    fn default() -> FocalBw {
        FocalBw { centre: CENTRE, size: 0.3, sharpness: 0.5 }
    }
}

/// `graduated_tint(x,y,angle,feather,shade,color)`: a coloured gradient from its start along its
/// angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraduatedTint {
    pub start: Point,
    pub angle: f64,
    pub feather: f64,
    pub shade: f64,
    pub color: [u8; 3],
}

impl Default for GraduatedTint {
    fn default() -> GraduatedTint {
        GraduatedTint { start: CENTRE, angle: 0.0, feather: 0.25, shade: 0.25, color: [0x9c, 0xc8, 0xff] }
    }
}

impl fmt::Display for SoftFocus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num_point(self.centre), num(self.size), num(self.amount))
    }
}

impl Operation for SoftFocus {
    const NAME: &'static str = "soft_focus";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Soft Focus";

    fn parse(params: &str) -> Result<SoftFocus, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = SoftFocus { centre: r.point("the centre")?, size: r.number("the size", SIZE)?, amount: r.number("the amount", AMOUNT)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> SoftFocus {
        SoftFocus { centre: clamp_point(self.centre), size: clamp(self.size, SIZE), amount: clamp(self.amount, AMOUNT) }
    }
}

impl fmt::Display for FocalBw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num_point(self.centre), num(self.size), num(self.sharpness))
    }
}

impl Operation for FocalBw {
    const NAME: &'static str = "focal_bw";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Focal B&W";

    fn parse(params: &str) -> Result<FocalBw, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = FocalBw { centre: r.point("the centre")?, size: r.number("the size", SIZE)?, sharpness: r.number("the sharpness", SHARPNESS)? };
        r.end()?;
        Ok(b)
    }

    fn clamped(self) -> FocalBw {
        FocalBw { centre: clamp_point(self.centre), size: clamp(self.size, SIZE), sharpness: clamp(self.sharpness, SHARPNESS) }
    }
}

impl fmt::Display for GraduatedTint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{}", num_point(self.start), num(self.angle), num(self.feather), num(self.shade), hex(self.color))
    }
}

impl Operation for GraduatedTint {
    const NAME: &'static str = "graduated_tint";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Graduated Tint";

    fn parse(params: &str) -> Result<GraduatedTint, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = GraduatedTint {
            start: r.point("the start")?,
            angle: r.number("the angle", ANGLE)?,
            feather: r.number("the feather", FEATHER)?,
            shade: r.number("the shade", SHADE)?,
            color: r.colour("the colour")?,
        };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> GraduatedTint {
        GraduatedTint {
            start: clamp_point(self.start),
            angle: clamp(self.angle, ANGLE),
            feather: clamp(self.feather, FEATHER),
            shade: clamp(self.shade, SHADE),
            color: self.color,
        }
    }
}

// Stubs until M12.8: each changes nothing.

impl Adjust for SoftFocus {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for FocalBw {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for GraduatedTint {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
