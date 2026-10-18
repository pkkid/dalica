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
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`, the size unchanged) until M12.13
//!   brings the third kind of operation beside `Warp` and `Adjust`: an output size of its own and
//!   the pixels outside the photo.

use std::fmt;

use super::{Adjust, Operation, ParamError, Pass, Prepare, Reader, clamp, flag, hex, num};

/// The range of a Border band's thickness, thousandths of the shorter side.
pub const THICKNESS: (f64, f64) = (0.0, 100.0);
/// The range of Border's Corner radius, percent of the largest (100: an oval).
pub const CORNER: (f64, f64) = (0.0, 100.0);
/// The range of Border's Caption height, thousandths of the shorter side.
pub const CAPTION: (f64, f64) = (0.0, 200.0);

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
    const VERSION: u32 = 0;
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
    const VERSION: u32 = 0;
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

// Stubs until M12.13: each changes nothing, the picture's size included.

impl Adjust for Widescreen {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Border {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
