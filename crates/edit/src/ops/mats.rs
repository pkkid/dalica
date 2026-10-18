//! The effects that change the picture's size, part two (`editing.md` §8.4):
//! `drop_shadow(color,distance,angle,blur,fade,background)` and
//! `museum_matte(outer_color,outer,inner_color,inner)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - Drop Shadow: the shadow colour (`#000000`); Distance 0–30 (4) and Blur 0–100 (10), in
//!     thousandths of the picture's shorter side; Angle 0–360 (90), the direction the shadow
//!     falls in degrees clockwise from pointing up (90: to the right); Fade 0–100 (30); the background
//!     colour (`#ffffff`). The canvas grows to hold the shadow.
//!   - Museum Matte: the outer mat's colour (`#1a0e03`) and thickness 0–100 (25), the inner mat's
//!     colour (`#f0eae4`) and thickness 0–100 (40), thicknesses in thousandths of the shorter
//!     side. Repeatable.
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that is not
//!   `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`, the size unchanged) until M12.14,
//!   on M12.13's size-changing kind; Drop Shadow from PhotoCraft `crates/compose/src/effects.rs`
//!   (`shadow_map`, `tent_kernel`, `offset`) at e4b2868a (`NOTICE`).

use std::fmt;

use super::{Adjust, DEGREES, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, hex, num};

/// The range of Drop Shadow's Distance, thousandths of the shorter side.
pub const DISTANCE: (f64, f64) = (0.0, 30.0);
/// The range of Drop Shadow's Angle, degrees clockwise from pointing up.
pub const ANGLE: (f64, f64) = DEGREES;
/// The range of Drop Shadow's Blur, thousandths of the shorter side.
pub const BLUR: (f64, f64) = (0.0, 100.0);
/// The range of a Museum Matte mat's thickness, thousandths of the shorter side.
pub const THICKNESS: (f64, f64) = (0.0, 100.0);

/// `drop_shadow(color,distance,angle,blur,fade,background)`: the photo floating over a background
/// with its shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropShadow {
    pub color: [u8; 3],
    pub distance: f64,
    pub angle: f64,
    pub blur: f64,
    pub fade: f64,
    pub background: [u8; 3],
}

impl Default for DropShadow {
    fn default() -> DropShadow {
        DropShadow { color: [0, 0, 0], distance: 4.0, angle: 90.0, blur: 10.0, fade: 30.0, background: [0xff, 0xff, 0xff] }
    }
}

/// `museum_matte(outer_color,outer,inner_color,inner)`: a double mat with an inner shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MuseumMatte {
    pub outer_color: [u8; 3],
    pub outer: f64,
    pub inner_color: [u8; 3],
    pub inner: f64,
}

impl Default for MuseumMatte {
    fn default() -> MuseumMatte {
        MuseumMatte { outer_color: [0x1a, 0x0e, 0x03], outer: 25.0, inner_color: [0xf0, 0xea, 0xe4], inner: 40.0 }
    }
}

impl fmt::Display for DropShadow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{},{}", hex(self.color), num(self.distance), num(self.angle), num(self.blur), num(self.fade), hex(self.background))
    }
}

impl Operation for DropShadow {
    const NAME: &'static str = "drop_shadow";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Drop Shadow";

    fn parse(params: &str) -> Result<DropShadow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d = DropShadow {
            color: r.colour("the shadow colour")?,
            distance: r.number("the distance", DISTANCE)?,
            angle: r.number("the angle", ANGLE)?,
            blur: r.number("the blur", BLUR)?,
            fade: r.number("fade", FADE)?,
            background: r.colour("the background colour")?,
        };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> DropShadow {
        DropShadow {
            distance: clamp(self.distance, DISTANCE),
            angle: clamp(self.angle, ANGLE),
            blur: clamp(self.blur, BLUR),
            fade: clamp(self.fade, FADE),
            ..self
        }
    }
}

impl fmt::Display for MuseumMatte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{}", hex(self.outer_color), num(self.outer), hex(self.inner_color), num(self.inner))
    }
}

impl Operation for MuseumMatte {
    const NAME: &'static str = "museum_matte";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Museum Matte";

    fn parse(params: &str) -> Result<MuseumMatte, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let m = MuseumMatte {
            outer_color: r.colour("the outer colour")?,
            outer: r.number("the outer thickness", THICKNESS)?,
            inner_color: r.colour("the inner colour")?,
            inner: r.number("the inner thickness", THICKNESS)?,
        };
        r.end()?;
        Ok(m)
    }

    fn clamped(self) -> MuseumMatte {
        MuseumMatte { outer: clamp(self.outer, THICKNESS), inner: clamp(self.inner, THICKNESS), ..self }
    }
}

// Stubs until M12.14: each changes nothing, the picture's size included.

impl Adjust for DropShadow {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for MuseumMatte {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
