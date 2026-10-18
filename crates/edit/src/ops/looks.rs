//! The blur-based looks of tab 4 (`editing.md` §8.3): `hdr(radius,strength,fade)`,
//! `dream_glow(bloom,brightness,fade)` and `plastic_lens(blur,grain,fade)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - HDR Look: Radius 1.3–80 (20), the blur's radius in thousandths of the picture's shorter
//!     side; Strength 1–7 (3), the `C` of `out = in + C·(in − blur(r))`; Fade 0–100 (0).
//!   - Dream Glow: Bloom 0–50 (25), the blurred copy's radius in thousandths of the shorter side;
//!     Brightness 0–100 (50), the midtone lift; Fade 0–100 (0).
//!   - Plastic Lens: Blur Edges 0–100 (70), Grain 0–100 (30), Fade 0–100 (0).
//!
//!   Numbers out of range are clamped; `NaN` or a missing or extra field is refused (the entry is
//!   kept and skipped).
//! - **Rendering:** stubs (`VERSION` 0, `prepare` returns `None`) until M12.10, on M12.3's blur,
//!   noise and blends.

use std::fmt;

use super::{Adjust, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, num};

/// The range of HDR Look's Radius, thousandths of the shorter side.
pub const HDR_RADIUS: (f64, f64) = (1.3, 80.0);
/// The range of HDR Look's Strength.
pub const HDR_STRENGTH: (f64, f64) = (1.0, 7.0);
/// The range of Dream Glow's Bloom, thousandths of the shorter side.
pub const BLOOM: (f64, f64) = (0.0, 50.0);
/// The range of Dream Glow's Brightness.
pub const BRIGHTNESS: (f64, f64) = (0.0, 100.0);
/// The range of Plastic Lens's Blur Edges.
pub const BLUR_EDGES: (f64, f64) = (0.0, 100.0);
/// The range of Plastic Lens's Grain.
pub const GRAIN: (f64, f64) = (0.0, 100.0);

/// `hdr(radius,strength,fade)`: large-radius local contrast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hdr {
    /// Thousandths of the shorter side.
    pub radius: f64,
    pub strength: f64,
    pub fade: f64,
}

impl Default for Hdr {
    fn default() -> Hdr {
        Hdr { radius: 20.0, strength: 3.0, fade: 0.0 }
    }
}

/// `dream_glow(bloom,brightness,fade)`: a blurred copy blended in overlay and a midtone lift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DreamGlow {
    /// Thousandths of the shorter side.
    pub bloom: f64,
    pub brightness: f64,
    pub fade: f64,
}

impl Default for DreamGlow {
    fn default() -> DreamGlow {
        DreamGlow { bloom: 25.0, brightness: 50.0, fade: 0.0 }
    }
}

/// `plastic_lens(blur,grain,fade)`: soft edges, grain and a vignette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlasticLens {
    pub blur: f64,
    pub grain: f64,
    pub fade: f64,
}

impl Default for PlasticLens {
    fn default() -> PlasticLens {
        PlasticLens { blur: 70.0, grain: 30.0, fade: 0.0 }
    }
}

impl fmt::Display for Hdr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.radius), num(self.strength), num(self.fade))
    }
}

impl Operation for Hdr {
    const NAME: &'static str = "hdr";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "HDR Look";

    fn parse(params: &str) -> Result<Hdr, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let h = Hdr { radius: r.number("the radius", HDR_RADIUS)?, strength: r.number("the strength", HDR_STRENGTH)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(h)
    }

    fn clamped(self) -> Hdr {
        Hdr { radius: clamp(self.radius, HDR_RADIUS), strength: clamp(self.strength, HDR_STRENGTH), fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for DreamGlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.bloom), num(self.brightness), num(self.fade))
    }
}

impl Operation for DreamGlow {
    const NAME: &'static str = "dream_glow";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Dream Glow";

    fn parse(params: &str) -> Result<DreamGlow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d =
            DreamGlow { bloom: r.number("the bloom", BLOOM)?, brightness: r.number("the brightness", BRIGHTNESS)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> DreamGlow {
        DreamGlow { bloom: clamp(self.bloom, BLOOM), brightness: clamp(self.brightness, BRIGHTNESS), fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for PlasticLens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.blur), num(self.grain), num(self.fade))
    }
}

impl Operation for PlasticLens {
    const NAME: &'static str = "plastic_lens";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Plastic Lens";

    fn parse(params: &str) -> Result<PlasticLens, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let p = PlasticLens { blur: r.number("blur edges", BLUR_EDGES)?, grain: r.number("the grain", GRAIN)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(p)
    }

    fn clamped(self) -> PlasticLens {
        PlasticLens { blur: clamp(self.blur, BLUR_EDGES), grain: clamp(self.grain, GRAIN), fade: clamp(self.fade, FADE) }
    }
}

// Stubs until M12.10: each changes nothing.

impl Adjust for Hdr {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for DreamGlow {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for PlasticLens {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
