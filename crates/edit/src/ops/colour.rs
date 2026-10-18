//! The colour effects of tab 3 (`editing.md` §8.2): `sepia()`, `bw()`, `warmify()`,
//! `tint(color,fade)`, `saturation(a)` and `filtered_bw(color)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Sepia, B&W and Warmify
//!   take none (one click); Tint's colour (`#80cfff`) and Fade 0–100 (0); Saturation's Amount
//!   −1…+1 (0.16; −1 is B&W, 0 the photo); Filtered B&W's filter colour (`#ff8000`, an orange
//!   filter). Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that
//!   is not `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.6 renders them as pixel
//!   passes, with PhotoCraft `crates/compose/src/adjust.rs` at e4b2868a as the source (`NOTICE`).

use std::fmt;

use super::{Adjust, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, hex, no_params, num};

/// The range of Saturation's Amount.
pub const SATURATION: (f64, f64) = (-1.0, 1.0);

/// `sepia()`: warm brown monochrome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sepia;

/// `bw()`: luminance greyscale (Ctrl+Shift+B).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bw;

/// `warmify()`: warmer colours, golden skin tones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Warmify;

/// `tint(color,fade)`: desaturate, then colourise toward `color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub color: [u8; 3],
    /// 0–100: 0 is the effect, 100 the input.
    pub fade: f64,
}

impl Default for Tint {
    fn default() -> Tint {
        Tint { color: [0x80, 0xcf, 0xff], fade: 0.0 }
    }
}

/// `saturation(a)`: toward B&W (−1) or vivid (+1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saturation {
    pub amount: f64,
}

impl Default for Saturation {
    fn default() -> Saturation {
        Saturation { amount: 0.16 }
    }
}

/// `filtered_bw(color)`: B&W through a filter of `color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilteredBw {
    pub color: [u8; 3],
}

impl Default for FilteredBw {
    fn default() -> FilteredBw {
        FilteredBw { color: [0xff, 0x80, 0x00] }
    }
}

macro_rules! one_click {
    ($t:ident, $name:literal, $label:literal) => {
        impl fmt::Display for $t {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        impl Operation for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = 0;
            const LABEL: &'static str = $label;

            fn parse(params: &str) -> Result<$t, ParamError> {
                no_params(params, $t)
            }
        }
    };
}

one_click!(Sepia, "sepia", "Sepia");
one_click!(Bw, "bw", "B&W");
one_click!(Warmify, "warmify", "Warmify");

impl fmt::Display for Tint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", hex(self.color), num(self.fade))
    }
}

impl Operation for Tint {
    const NAME: &'static str = "tint";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Tint";

    fn parse(params: &str) -> Result<Tint, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let t = Tint { color: r.colour("the colour")?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(t)
    }

    fn clamped(self) -> Tint {
        Tint { fade: clamp(self.fade, FADE), ..self }
    }
}

impl fmt::Display for Saturation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.amount))
    }
}

impl Operation for Saturation {
    const NAME: &'static str = "saturation";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Saturation";

    fn parse(params: &str) -> Result<Saturation, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = Saturation { amount: r.number("the amount", SATURATION)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> Saturation {
        Saturation { amount: clamp(self.amount, SATURATION) }
    }
}

impl fmt::Display for FilteredBw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(self.color))
    }
}

impl Operation for FilteredBw {
    const NAME: &'static str = "filtered_bw";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Filtered B&W";

    fn parse(params: &str) -> Result<FilteredBw, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = FilteredBw { color: r.colour("the filter colour")? };
        r.end()?;
        Ok(b)
    }
}

// Stubs until M12.6: each changes nothing.

impl Adjust for Sepia {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Bw {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Warmify {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Tint {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Saturation {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for FilteredBw {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
