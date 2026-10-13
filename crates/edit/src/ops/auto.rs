//! The one-click fixes (`editing.md` §6.3): `autofix()`, `autocontrast()` and `autocolor()`. They
//! store no parameters and analyse the picture at their place in the pipeline at the fixed
//! analysis size ([`crate::analysis`]), so a thumbnail and the full size agree and cropping first
//! changes the outcome.
//!
//! - `autocolor()`: Auto Color's per-channel gains ([`analysis::balance`]).
//! - `autocontrast()`: one set of levels for all three channels ([`analysis::levels`]), through a
//!   table.
//! - `autofix()`: the colour first, then the contrast measured on the balanced picture, then a
//!   little Fill Light ([`analysis::fill_amount`], measured on the balanced and levelled picture)
//!   through Tuning's own pass ([`super::tone`]).
//!
//! Each is one pixel-only pass; a picture the analysis finds nothing to change in is left alone.
//! Without a proxy (which the renderer always gives an analysing operation) they change nothing.

use std::fmt;

use super::tone::Tuning;
use super::{Adjust, Band, Operation, ParamError, Pass, Prepare};
use crate::analysis::{self, Levels};

/// `autofix()`: auto contrast, auto colour and a little fill light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoFix;

/// `autocontrast()`: black and white points and midtones, linked across the channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoContrast;

/// `autocolor()`: removes a colour cast.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoColor;

fn no_params<T: Operation>(params: &str, value: T) -> Result<T, ParamError> {
    if params.is_empty() { Ok(value) } else { Err(ParamError::new(T::NAME, params, "takes no parameters")) }
}

/// Which parts of Auto Fix an operation runs.
#[derive(Clone, Copy)]
struct Parts {
    color: bool,
    contrast: bool,
    fill: bool,
}

macro_rules! one_click {
    ($t:ident, $name:literal, $label:literal, $parts:expr) => {
        impl fmt::Display for $t {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        impl Operation for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = 1;
            const LABEL: &'static str = $label;

            fn parse(params: &str) -> Result<$t, ParamError> {
                no_params(params, $t)
            }
        }

        impl Adjust for $t {
            const ANALYSES: bool = true;

            fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
                fix(ctx, $parts)
            }
        }
    };
}

one_click!(AutoFix, "autofix", "Auto Fix", Parts { color: true, contrast: true, fill: true });
one_click!(AutoContrast, "autocontrast", "Auto Contrast", Parts { color: false, contrast: true, fill: false });
one_click!(AutoColor, "autocolor", "Auto Color", Parts { color: true, contrast: false, fill: false });

/// Analyses the proxy and prepares the pass of `parts`; `None` when it would change nothing.
fn fix(ctx: &Prepare<'_>, parts: Parts) -> Option<Box<dyn Pass>> {
    let mut px = analysis::pixels(ctx.proxy?);
    let gains = if parts.color { analysis::balance(&px) } else { [1.0; 3] };
    let levels = if parts.contrast {
        analysis::apply_gains(&mut px, gains);
        analysis::levels(&px)
    } else {
        Levels::IDENTITY
    };
    let fill = if parts.fill {
        analysis::apply_levels(&mut px, &levels);
        let amount = analysis::fill_amount(&px);
        Tuning { fill: amount, ..Tuning::default() }.prepare(ctx)
    } else {
        None
    };
    let curve = (!levels.is_identity()).then(|| Lut::new(&levels));
    if gains == [1.0; 3] && curve.is_none() && fill.is_none() {
        return None;
    }
    Some(Box::new(AutoPass { gains, curve, fill }))
}

/// Intervals of the levels table.
const STEPS: usize = 4096;

/// [`Levels`] tabulated over 0..1, read with linear interpolation.
struct Lut {
    table: Vec<f32>,
}

impl Lut {
    fn new(levels: &Levels) -> Lut {
        Lut { table: (0..=STEPS).map(|i| levels.apply(i as f32 / STEPS as f32)).collect() }
    }

    fn at(&self, v: f32) -> f32 {
        // NaN reads as black.
        let x = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) * STEPS as f32 };
        let i = (x.floor() as usize).min(STEPS - 1);
        let frac = x - i as f32;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * frac,
            _ => v,
        }
    }
}

/// The prepared fix: channel gains, then the levels, then the fill light.
struct AutoPass {
    gains: [f32; 3],
    curve: Option<Lut>,
    fill: Option<Box<dyn Pass>>,
}

impl Pass for AutoPass {
    fn run(&self, band: &mut Band<'_>) {
        for p in band.pixels.iter_mut() {
            let mut c = [p[0] * self.gains[0], p[1] * self.gains[1], p[2] * self.gains[2]].map(|v| v.clamp(0.0, 1.0));
            if let Some(curve) = &self.curve {
                c = c.map(|v| curve.at(v));
            }
            *p = c;
        }
        if let Some(fill) = &self.fill {
            fill.run(band);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_follows_the_levels() {
        let levels = Levels { black: 0.1, white: 0.8, gamma: 0.9 };
        let lut = Lut::new(&levels);
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            assert!((lut.at(v) - levels.apply(v)).abs() < 1e-3, "{v}");
        }
        assert_eq!(lut.at(f32::NAN), lut.at(0.0));
    }
}
