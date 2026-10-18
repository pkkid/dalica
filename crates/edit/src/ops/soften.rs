//! Soften with its paint mask (`editing.md` §8.4): `soften(softness,fade;stroke;…)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Softness 0–100 (50)
//!   and Fade 0–100 (50), then the brush strokes, each after a `;`, in the order they were drawn:
//!   `paint,r,x,y,x,y,…` or `erase,r,x,y,…` — the mode, the brush radius `r` relative to the
//!   stored photo's shorter side ([`MIN_RADIUS`]..[`MAX_RADIUS`], as Retouch's), and the stroke's
//!   points (at least one, at most [`MAX_POINTS`]) normalised in the stored frame (`data-model.md`
//!   §3.3) and brought into −1..2 as Retouch's. Without strokes (`soften(50,50)`) Soften applies
//!   to the whole photo; from the first stroke it applies where paint strokes cover minus where
//!   later erase strokes do. More than [`MAX_STROKES`] strokes, a stroke without a mode, a radius
//!   and whole points, `NaN`, or a missing or extra field is refused (the entry is kept and
//!   skipped); numbers out of range are clamped.
//! - **Rendering:** a stub (`VERSION` 0, `prepare` returns `None`) until M12.11, on M12.3's brush
//!   dabs and blur, the strokes mapped through [`super::Prepare::geometry`].

use std::fmt;

use super::{Adjust, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, num, num_point};
use crate::frame::Point;

/// The range of Softness.
pub const SOFTNESS: (f64, f64) = (0.0, 100.0);
/// The most strokes one `soften` entry holds (as Retouch, `security.md` §8).
pub const MAX_STROKES: usize = 1000;
/// The most points one stroke holds.
pub const MAX_POINTS: usize = 4096;
/// The smallest brush radius, relative to the shorter side.
pub const MIN_RADIUS: f64 = 0.001;
/// The largest brush radius, relative to the shorter side.
pub const MAX_RADIUS: f64 = 0.25;
/// The range of a stroke's points (as Retouch's: a brush may reach past the edge).
pub const POINT: (f64, f64) = (-1.0, 2.0);
/// The range of a stroke's radius.
pub const RADIUS: (f64, f64) = (MIN_RADIUS, MAX_RADIUS);

/// Whether a stroke paints where Soften applies or erases it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Paint,
    Erase,
}

impl Mode {
    /// The word an entry writes.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Paint => "paint",
            Mode::Erase => "erase",
        }
    }
}

/// The modes by their words.
pub const MODES: [(&str, Mode); 2] = [("paint", Mode::Paint), ("erase", Mode::Erase)];

/// One brush stroke in the stored frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stroke {
    pub mode: Mode,
    /// Relative to the shorter side.
    pub radius: f64,
    pub points: Vec<Point>,
}

impl Stroke {
    /// The stroke with its radius and points brought into range (`NaN` to the low end).
    pub fn clamped(self) -> Stroke {
        let c = |p: Point| Point::new(clamp(p.x, POINT), clamp(p.y, POINT));
        Stroke { mode: self.mode, radius: clamp(self.radius, RADIUS), points: self.points.into_iter().map(c).collect() }
    }
}

/// `soften(softness,fade;stroke;…)`: a soft glow blur, everywhere or where painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Soften {
    pub softness: f64,
    pub fade: f64,
    pub strokes: Vec<Stroke>,
}

impl Default for Soften {
    fn default() -> Soften {
        Soften { softness: 50.0, fade: 50.0, strokes: Vec::new() }
    }
}

impl fmt::Display for Soften {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.softness), num(self.fade))?;
        for s in &self.strokes {
            write!(f, ";{},{}", s.mode.word(), num(s.radius))?;
            for p in &s.points {
                write!(f, ",{}", num_point(*p))?;
            }
        }
        Ok(())
    }
}

impl Operation for Soften {
    const NAME: &'static str = "soften";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Soften";

    fn parse(params: &str) -> Result<Soften, ParamError> {
        let refuse = |why: &str| ParamError::new(Self::NAME, params, why);
        let mut parts = params.split(';');
        let mut r = Reader::new(Self::NAME, parts.next().unwrap_or(""));
        let softness = r.number("the softness", SOFTNESS)?;
        let fade = r.number("fade", FADE)?;
        r.end()?;
        let mut strokes = Vec::new();
        for text in parts {
            if strokes.len() >= MAX_STROKES {
                return Err(refuse("holds more strokes than one entry may"));
            }
            let mut r = Reader::new(Self::NAME, text);
            let mode = r.choice("the brush mode", &MODES)?;
            let radius = r.number("the brush radius", RADIUS)?;
            // The points: pairs to the end of the stroke.
            let fields = text.split(',').count().saturating_sub(2);
            if fields == 0 || fields % 2 != 0 || fields / 2 > MAX_POINTS {
                return Err(refuse("each stroke takes a mode, a radius and whole points"));
            }
            let mut points = Vec::with_capacity(fields / 2);
            for _ in 0..fields / 2 {
                let x = r.number("a point", POINT)?;
                let y = r.number("a point", POINT)?;
                points.push(Point::new(x, y));
            }
            r.end()?;
            strokes.push(Stroke { mode, radius, points });
        }
        Ok(Soften { softness, fade, strokes })
    }

    fn clamped(self) -> Soften {
        Soften {
            softness: clamp(self.softness, SOFTNESS),
            fade: clamp(self.fade, FADE),
            strokes: self.strokes.into_iter().map(Stroke::clamped).collect(),
        }
    }
}

// A stub until M12.11: it changes nothing.

impl Adjust for Soften {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
