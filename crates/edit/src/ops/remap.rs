//! The colour remaps of tabs 4 and 5 (`editing.md` §8.3–§8.4): `invert()`, `infrared(fade)`,
//! `duotone(first,second,brightness,contrast,fade)` and `boost(strength)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Invert Colors takes
//!   none (one click); Infrared Film's Fade 0–100 (0); Duo-Tone's first colour (`#004488`, the
//!   shadows), second colour (`#ffff00`, the highlights), Brightness −95…95 (0), Contrast 0–100
//!   (20) and Fade 0–100 (0); Boost's Strength 0–100 (50). Numbers out of range are clamped;
//!   `NaN`, a missing or extra field or a colour that is not `#rrggbb` is refused (the entry is
//!   kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.9: pixel passes, with
//!   PhotoCraft `crates/compose/src/effects.rs` (`sample_stops`) and `crates/compose/src/adjust.rs`
//!   (`vibrance_px`, `modern_contrast`) at e4b2868a as the sources (`NOTICE`).

use std::fmt;

use super::{Adjust, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, hex, no_params, num};

/// The range of Duo-Tone's Brightness.
pub const BRIGHTNESS: (f64, f64) = (-95.0, 95.0);
/// The range of Duo-Tone's Contrast.
pub const CONTRAST: (f64, f64) = (0.0, 100.0);
/// The range of Boost's Strength.
pub const STRENGTH: (f64, f64) = (0.0, 100.0);

/// `invert()`: the negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invert;

/// `infrared(fade)`: B&W infrared, foliage bright and skies dark.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Infrared {
    pub fade: f64,
}

/// `duotone(first,second,brightness,contrast,fade)`: the tones mapped onto the gradient from the
/// first colour (black) to the second (white).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duotone {
    pub first: [u8; 3],
    pub second: [u8; 3],
    pub brightness: f64,
    pub contrast: f64,
    pub fade: f64,
}

impl Default for Duotone {
    fn default() -> Duotone {
        Duotone { first: [0x00, 0x44, 0x88], second: [0xff, 0xff, 0x00], brightness: 0.0, contrast: 20.0, fade: 0.0 }
    }
}

/// `boost(strength)`: stronger colour and contrast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boost {
    pub strength: f64,
}

impl Default for Boost {
    fn default() -> Boost {
        Boost { strength: 50.0 }
    }
}

impl fmt::Display for Invert {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl Operation for Invert {
    const NAME: &'static str = "invert";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Invert Colors";

    fn parse(params: &str) -> Result<Invert, ParamError> {
        no_params(params, Invert)
    }
}

impl fmt::Display for Infrared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.fade))
    }
}

impl Operation for Infrared {
    const NAME: &'static str = "infrared";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Infrared Film";

    fn parse(params: &str) -> Result<Infrared, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let i = Infrared { fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(i)
    }

    fn clamped(self) -> Infrared {
        Infrared { fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for Duotone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{}", hex(self.first), hex(self.second), num(self.brightness), num(self.contrast), num(self.fade))
    }
}

impl Operation for Duotone {
    const NAME: &'static str = "duotone";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Duo-Tone";

    fn parse(params: &str) -> Result<Duotone, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d = Duotone {
            first: r.colour("the first colour")?,
            second: r.colour("the second colour")?,
            brightness: r.number("the brightness", BRIGHTNESS)?,
            contrast: r.number("the contrast", CONTRAST)?,
            fade: r.number("fade", FADE)?,
        };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> Duotone {
        Duotone { brightness: clamp(self.brightness, BRIGHTNESS), contrast: clamp(self.contrast, CONTRAST), fade: clamp(self.fade, FADE), ..self }
    }
}

impl fmt::Display for Boost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.strength))
    }
}

impl Operation for Boost {
    const NAME: &'static str = "boost";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Boost";

    fn parse(params: &str) -> Result<Boost, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = Boost { strength: r.number("the strength", STRENGTH)? };
        r.end()?;
        Ok(b)
    }

    fn clamped(self) -> Boost {
        Boost { strength: clamp(self.strength, STRENGTH) }
    }
}

// Stubs until M12.9: each changes nothing.

impl Adjust for Invert {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Infrared {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Duotone {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Boost {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
