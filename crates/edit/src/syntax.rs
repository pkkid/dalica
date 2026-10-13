//! The operation syntax (`data-model.md` §3.1): each edit operation is one string, `name(params)`,
//! the same text in the sidecar's `"edits"` and `"redo"` lists and in the recipe API.
//!
//! [`Op::parse`] never fails and [`Op`]'s `Display` prints back exactly the text it was parsed
//! from, for every string: an operation is only typed when its text is the one Dalica would print
//! for it, and everything else is kept verbatim. Three kinds of verbatim text exist:
//!
//! - an operation of §3.1 other than the rotations and flips ([`Op::Listed`]): its name and the
//!   text between the parentheses, typed on demand by the module that renders it
//!   ([`Op::typed`], `crate::ops`), so a parameter a module refuses never loses the entry;
//! - an entry converted from the legacy organizer's sidecar that has no Dalica equivalent
//!   ([`Op::Legacy`], `legacy:<name>(…)`, `data-model.md` §9);
//! - anything else ([`Op::Unknown`]): an operation from a newer Dalica, a typed operation with a
//!   parameter out of range (`rotate(45)`), or damaged text.
//!
//! Legacy and unknown operations are skipped by rendering and never drop the operations after them.

use std::fmt;

use crate::Dihedral;
use crate::ops::orient::Rotate;
use crate::ops::{self, Operation, ParamError, Typed};

/// The prefix of operations kept from the legacy organizer's sidecar.
pub const LEGACY_PREFIX: &str = "legacy:";

/// The operations of `data-model.md` §3.1 whose parameters stay text in M6, so their modules (M7)
/// type them. `rotate`, `flip_h` and `flip_v` are typed here and are not in this list. An effect
/// `editing.md` describes but §3.1 does not yet name joins the list with its module.
pub const LISTED: [&str; 21] = [
    "crop",
    "straighten",
    "retouch",
    "autofix",
    "autocontrast",
    "autocolor",
    "fill_light",
    "tuning",
    "trim",
    "sharpen",
    "sepia",
    "bw",
    "warmify",
    "grain",
    "tint",
    "saturation",
    "soft_focus",
    "glow",
    "filtered_bw",
    "focal_bw",
    "graduated_tint",
];

/// Listed operations that change the picture's geometry; they come first in the pipeline.
const LISTED_GEOMETRY: [&str; 2] = ["crop", "straighten"];

/// A clockwise rotation by a quarter, half or three-quarter turn: `rotate(90)`, `rotate(180)` and
/// `rotate(270)`. Ctrl+R stores `rotate(90)` and Ctrl+Shift+R `rotate(270)` (`editing.md` §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Turn {
    Quarter,
    Half,
    ThreeQuarters,
}

impl Turn {
    /// The parameter of `rotate(…)`.
    pub fn degrees(self) -> u16 {
        u16::from(self.quarters()) * 90
    }

    /// `90`, `180` or `270`; anything else is not a turn.
    pub fn from_degrees(degrees: u16) -> Option<Turn> {
        match degrees {
            90 => Some(Turn::Quarter),
            180 => Some(Turn::Half),
            270 => Some(Turn::ThreeQuarters),
            _ => None,
        }
    }

    /// Clockwise quarter turns, 1–3.
    pub fn quarters(self) -> u8 {
        match self {
            Turn::Quarter => 1,
            Turn::Half => 2,
            Turn::ThreeQuarters => 3,
        }
    }

    /// The turn of `quarters` clockwise quarter turns (taken modulo 4); `None` for a whole turn.
    pub fn from_quarters(quarters: u8) -> Option<Turn> {
        match quarters % 4 {
            1 => Some(Turn::Quarter),
            2 => Some(Turn::Half),
            3 => Some(Turn::ThreeQuarters),
            _ => None,
        }
    }
}

/// Where an operation runs in the pipeline (`data-model.md` §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// Rotations, flips, crop and straighten: first, in the order they stand.
    Geometry,
    /// Every other operation Dalica knows, in the order applied.
    Adjust,
    /// Legacy and unknown operations: kept in the recipe, never rendered.
    Skipped,
}

/// One edit operation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// `rotate(90)`, `rotate(180)`, `rotate(270)`: clockwise.
    Rotate(Turn),
    /// `flip_h()`: mirror left–right.
    FlipH,
    /// `flip_v()`: mirror top–bottom.
    FlipV,
    /// An operation of [`LISTED`]: its name and its parameter text, verbatim.
    Listed { name: &'static str, params: String },
    /// `legacy:<name>(…)`, verbatim.
    Legacy(String),
    /// Anything else, verbatim.
    Unknown(String),
}

impl Op {
    /// The operation `text` stands for. Never fails; `op.to_string() == text` always holds.
    pub fn parse(text: &str) -> Op {
        if text.starts_with(LEGACY_PREFIX) {
            return Op::Legacy(text.to_owned());
        }
        let unknown = || Op::Unknown(text.to_owned());
        let Some((name, params)) = split(text) else {
            return unknown();
        };
        match name {
            // Only the text Dalica prints is typed: `rotate(090)` stays verbatim.
            Rotate::NAME => Rotate::parse(params).map_or_else(|_| unknown(), |r| Op::Rotate(r.0)),
            "flip_h" if params.is_empty() => Op::FlipH,
            "flip_v" if params.is_empty() => Op::FlipV,
            _ => match LISTED.iter().find(|&&listed| listed == name) {
                Some(&listed) => Op::Listed { name: listed, params: params.to_owned() },
                None => unknown(),
            },
        }
    }

    /// The operation's name, for operations Dalica knows.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Op::Rotate(_) => Some("rotate"),
            Op::FlipH => Some("flip_h"),
            Op::FlipV => Some("flip_v"),
            Op::Listed { name, .. } => Some(name),
            Op::Legacy(_) | Op::Unknown(_) => None,
        }
    }

    /// Whether Dalica knows this operation (it is neither legacy nor unknown).
    pub fn is_known(&self) -> bool {
        self.name().is_some()
    }

    /// Where the operation runs in the pipeline.
    pub fn stage(&self) -> Stage {
        match self {
            Op::Rotate(_) | Op::FlipH | Op::FlipV => Stage::Geometry,
            Op::Listed { name, .. } if LISTED_GEOMETRY.contains(name) => Stage::Geometry,
            Op::Listed { .. } => Stage::Adjust,
            Op::Legacy(_) | Op::Unknown(_) => Stage::Skipped,
        }
    }

    /// The operation's typed parameters, read by its module (`ops`): `None` for legacy and unknown
    /// operations and for known ones without a module yet (the effects); `Some(Err)` when the
    /// module refuses the parameter text (the operation is then kept and skipped by rendering).
    pub fn typed(&self) -> Option<Result<Typed, ParamError>> {
        match self {
            Op::Rotate(turn) => Some(Ok(Typed::Rotate(Rotate(*turn)))),
            Op::FlipH => Some(Ok(Typed::FlipH(ops::orient::FlipH))),
            Op::FlipV => Some(Ok(Typed::FlipV(ops::orient::FlipV))),
            Op::Listed { name, params } => Typed::parse(name, params),
            Op::Legacy(_) | Op::Unknown(_) => None,
        }
    }

    /// The history label ("Crop", "Tuning", "Rotate"; `editing.md` §5.1) of a known operation.
    pub fn label(&self) -> Option<&'static str> {
        ops::label(self.name()?)
    }

    /// The version of a known operation's renderer (0 while it renders nothing).
    pub fn version(&self) -> Option<u32> {
        ops::version(self.name()?)
    }

    /// The rotation or mirroring this operation applies to the picture, for `rotate`, `flip_h` and
    /// `flip_v`.
    pub fn orientation(&self) -> Option<Dihedral> {
        match self {
            Op::Rotate(turn) => Some(Dihedral::new(turn.quarters(), false)),
            Op::FlipH => Some(Dihedral::FLIP_H),
            Op::FlipV => Some(Dihedral::FLIP_V),
            Op::Listed { .. } | Op::Legacy(_) | Op::Unknown(_) => None,
        }
    }
}

impl From<Typed> for Op {
    /// The recipe entry for typed parameters, as Dalica prints it.
    fn from(t: Typed) -> Op {
        match t {
            Typed::Rotate(r) => Op::Rotate(r.0),
            Typed::FlipH(_) => Op::FlipH,
            Typed::FlipV(_) => Op::FlipV,
            other => {
                let name = other.name();
                let params = other.params();
                // Every typed name is listed; the lookup gives the `'static` name `Op` keeps.
                match LISTED.iter().find(|&&listed| listed == name) {
                    Some(&listed) => Op::Listed { name: listed, params },
                    None => Op::Unknown(format!("{name}({params})")),
                }
            }
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Rotate(turn) => write!(f, "rotate({})", turn.degrees()),
            Op::FlipH => f.write_str("flip_h()"),
            Op::FlipV => f.write_str("flip_v()"),
            Op::Listed { name, params } => write!(f, "{name}({params})"),
            Op::Legacy(text) | Op::Unknown(text) => f.write_str(text),
        }
    }
}

/// `name(params)` split into its name and parameter text: the name is a lowercase ASCII letter
/// followed by lowercase letters, digits and `_`; the parameters hold no parenthesis and no
/// control character.
fn split(text: &str) -> Option<(&str, &str)> {
    let inner = text.strip_suffix(')')?;
    let (name, params) = inner.split_once('(')?;
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let name_ok = first_ok && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let params_ok = !params.chars().any(|c| c == '(' || c == ')' || c.is_control());
    (name_ok && params_ok).then_some((name, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_round_trip_through_degrees_and_quarters() {
        for turn in [Turn::Quarter, Turn::Half, Turn::ThreeQuarters] {
            assert_eq!(Turn::from_degrees(turn.degrees()), Some(turn));
            assert_eq!(Turn::from_quarters(turn.quarters()), Some(turn));
            assert_eq!(Turn::from_quarters(turn.quarters() + 4), Some(turn));
        }
        assert_eq!(Turn::from_quarters(0), None);
        assert_eq!(Turn::from_quarters(8), None);
        assert_eq!(Turn::from_degrees(0), None);
        assert_eq!(Turn::from_degrees(360), None);
    }

    #[test]
    fn the_name_and_parameters_are_split_by_shape() {
        assert_eq!(split("crop(0.1,0.2,0.9,0.8)"), Some(("crop", "0.1,0.2,0.9,0.8")));
        assert_eq!(split("autofix()"), Some(("autofix", "")));
        assert_eq!(split("x2_y(a;b)"), Some(("x2_y", "a;b")));
        for bad in ["", "()", "crop", "crop(", "Crop()", "2crop()", "crop(()", "crop())", "cr op()", "crop(\n)", "crop()x"] {
            assert_eq!(split(bad), None, "{bad:?}");
        }
    }
}
