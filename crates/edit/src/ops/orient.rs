//! `rotate(90|180|270)`, `flip_h()` and `flip_v()` (`data-model.md` §3.1, `editing.md` §10): the
//! rotations and mirrorings of [`crate::geometry`] (M6.3) behind the same interface as every other
//! operation. The renderer adds them up to one [`Dihedral`] with [`crate::Recipe::orientation`] and
//! applies it last, moving pixels without resampling.

use std::fmt;

use super::{Affine, Operation, ParamError, Warp};
use crate::{Dihedral, Turn};

/// `rotate(deg)`: a clockwise quarter, half or three-quarter turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rotate(pub Turn);

/// `flip_h()`: mirror left–right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlipH;

/// `flip_v()`: mirror top–bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlipV;

impl Default for Rotate {
    fn default() -> Rotate {
        Rotate(Turn::Quarter)
    }
}

impl fmt::Display for Rotate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.degrees())
    }
}

impl fmt::Display for FlipH {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl fmt::Display for FlipV {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl Operation for Rotate {
    const NAME: &'static str = "rotate";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Rotate";

    /// Only the text Dalica prints is read: `90`, `180`, `270` (not `090` or ` 90`).
    fn parse(params: &str) -> Result<Rotate, ParamError> {
        match params.parse::<u16>().ok().and_then(Turn::from_degrees) {
            Some(turn) if turn.degrees().to_string() == params => Ok(Rotate(turn)),
            _ => Err(ParamError::new(Self::NAME, params, "takes 90, 180 or 270")),
        }
    }
}

fn no_params<T: Operation>(params: &str, value: T) -> Result<T, ParamError> {
    if params.is_empty() { Ok(value) } else { Err(ParamError::new(T::NAME, params, "takes no parameters")) }
}

impl Operation for FlipH {
    const NAME: &'static str = "flip_h";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Flip Horizontal";

    fn parse(params: &str) -> Result<FlipH, ParamError> {
        no_params(params, FlipH)
    }
}

impl Operation for FlipV {
    const NAME: &'static str = "flip_v";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Flip Vertical";

    fn parse(params: &str) -> Result<FlipV, ParamError> {
        no_params(params, FlipV)
    }
}

impl Rotate {
    pub fn dihedral(self) -> Dihedral {
        Dihedral::new(self.0.quarters(), false)
    }
}

/// The map from the result's pixel coordinates to the input's for `d` on a `width × height`
/// picture: the continuous counterpart of [`Dihedral::map_pixel`], inverted.
pub fn dihedral_to_input(d: Dihedral, width: f64, height: f64) -> Affine {
    // Forward: mirror (x ↦ w − x), then the quarter turns on the mirrored picture.
    let mirror = if d.mirrored() { Affine { a: -1.0, b: 0.0, c: width, d: 0.0, e: 1.0, f: 0.0 } } else { Affine::IDENTITY };
    let turn = match d.quarters() {
        1 => Affine { a: 0.0, b: -1.0, c: height, d: 1.0, e: 0.0, f: 0.0 },
        2 => Affine { a: -1.0, b: 0.0, c: width, d: 0.0, e: -1.0, f: height },
        3 => Affine { a: 0.0, b: 1.0, c: 0.0, d: -1.0, e: 0.0, f: width },
        _ => Affine::IDENTITY,
    };
    // Every dihedral map is its own kind of permutation; its inverse always exists.
    turn.then_after(mirror).inverse().unwrap_or_default()
}

macro_rules! dihedral_warp {
    ($t:ty, $d:expr) => {
        impl Warp for $t {
            fn output_size(&self, width: f64, height: f64) -> (f64, f64) {
                let d: Dihedral = $d(self);
                if d.swaps() { (height, width) } else { (width, height) }
            }

            fn to_input(&self, width: f64, height: f64) -> Affine {
                dihedral_to_input($d(self), width, height)
            }
        }
    };
}

dihedral_warp!(Rotate, |r: &Rotate| r.dihedral());
dihedral_warp!(FlipH, |_: &FlipH| Dihedral::FLIP_H);
dihedral_warp!(FlipV, |_: &FlipV| Dihedral::FLIP_V);
