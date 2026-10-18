//! The detail effects of tab 3 (`editing.md` §8.2): `sharpen(a)`, `grain(grain,lighten)` and `glow(i,r)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Sharpen's Amount 0–3
//!   (0.6); Film Grain's Grain 0–50 (10) and Lighten (`false`); Glow's Intensity 0–1 (0.65) and
//!   Radius 0.3–30 (3, on a logarithmic slider), the blur's radius in percent of the picture's
//!   shorter side. Numbers out of range are clamped; `NaN`, a missing or extra field or a
//!   checkbox other than `true` / `false` is refused (the entry is kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.7: Sharpen from
//!   PhotoCraft `crates/algo/src/sharpen.rs` (`unsharp`), Film Grain from M12.3's seeded noise and
//!   PhotoCraft `crates/algo/src/noise.rs` (`add`), both at e4b2868a (`NOTICE`).

use std::fmt;

use super::{Adjust, Operation, ParamError, Pass, Prepare, Reader, UNIT, clamp, flag, num};

/// The range of Sharpen's Amount.
pub const SHARPEN: (f64, f64) = (0.0, 3.0);
/// The range of Film Grain's Grain.
pub const GRAIN: (f64, f64) = (0.0, 50.0);
/// The range of Glow's Intensity.
pub const GLOW_INTENSITY: (f64, f64) = UNIT;
/// The range of Glow's Radius, percent of the shorter side (a logarithmic slider).
pub const GLOW_RADIUS: (f64, f64) = (0.3, 30.0);

/// `sharpen(a)`: an unsharp mask, accurate at 1:1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sharpen {
    pub amount: f64,
}

impl Default for Sharpen {
    fn default() -> Sharpen {
        Sharpen { amount: 0.6 }
    }
}

/// `grain(grain,lighten)`: monochrome film grain; with `lighten`, only light grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    pub grain: f64,
    pub lighten: bool,
}

impl Default for Grain {
    fn default() -> Grain {
        Grain { grain: 10.0, lighten: false }
    }
}

/// `glow(i,r)`: the highlights bloom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub intensity: f64,
    /// Percent of the shorter side.
    pub radius: f64,
}

impl Default for Glow {
    fn default() -> Glow {
        Glow { intensity: 0.65, radius: 3.0 }
    }
}

impl fmt::Display for Sharpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.amount))
    }
}

impl Operation for Sharpen {
    const NAME: &'static str = "sharpen";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Sharpen";

    fn parse(params: &str) -> Result<Sharpen, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = Sharpen { amount: r.number("the amount", SHARPEN)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> Sharpen {
        Sharpen { amount: clamp(self.amount, SHARPEN) }
    }
}

impl fmt::Display for Grain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.grain), flag(self.lighten))
    }
}

impl Operation for Grain {
    const NAME: &'static str = "grain";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Film Grain";

    fn parse(params: &str) -> Result<Grain, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = Grain { grain: r.number("the grain", GRAIN)?, lighten: r.flag("lighten")? };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> Grain {
        Grain { grain: clamp(self.grain, GRAIN), ..self }
    }
}

impl fmt::Display for Glow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.intensity), num(self.radius))
    }
}

impl Operation for Glow {
    const NAME: &'static str = "glow";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Glow";

    fn parse(params: &str) -> Result<Glow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = Glow { intensity: r.number("the intensity", GLOW_INTENSITY)?, radius: r.number("the radius", GLOW_RADIUS)? };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> Glow {
        Glow { intensity: clamp(self.intensity, GLOW_INTENSITY), radius: clamp(self.radius, GLOW_RADIUS) }
    }
}

// Stubs until M12.7: each changes nothing.

impl Adjust for Sharpen {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Grain {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for Glow {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
