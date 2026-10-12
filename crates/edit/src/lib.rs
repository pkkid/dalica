//! dalica-edit: The edit recipe: operation syntax parse/print (unknown operations preserved), pipeline order, the CPU renderer for every tool and effect (the oracle), auto-fix analysis.
//!
//! Layer L1; see `docs/architecture.md` §2. M6 brings the recipe and its geometric operations
//! (`data-model.md` §3.1, `editing.md` §10); the renderer and the other operations follow in M7.
//!
//! - [`syntax`]: one operation string parsed into an [`Op`] and printed back to the same text. Parsing
//!   never fails: anything Dalica does not understand (an operation from a newer version, damaged
//!   text, `legacy:` entries) is kept verbatim and skipped by rendering.
//! - [`recipe`]: the ordered list of operations, the rules for appending rotations and flips, the
//!   pipeline order, and the net orientation a recipe gives.
//! - [`geometry`]: the eight rotations and mirrorings of a picture ([`Dihedral`]), their composition
//!   with the EXIF orientation, the displayed size, and applying one to an RGBA image (the CPU
//!   oracle Save uses).
//! - [`frame`]: the coordinate frame of stored rectangles and points (the photo after its EXIF
//!   orientation, before the recipe's rotations and flips; `data-model.md` §3.3), and mapping
//!   normalised [`Point`]s and [`Rect`]s between it and the displayed photo.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod frame;
pub mod geometry;
pub mod recipe;
pub mod syntax;

pub use frame::{Point, Rect};
pub use geometry::Dihedral;
pub use recipe::{MAX_OPERATIONS, Recipe};
pub use syntax::{LISTED, Op, Stage, Turn};

/// Why a recipe or an image was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// More operations than `security.md` §8 allows in one recipe.
    #[error("a recipe holds at most {MAX_OPERATIONS} operations")]
    TooManyOperations,
    /// The pixel buffer does not hold `width × height` RGBA pixels.
    #[error("{len} bytes for a {width} × {height} RGBA image")]
    BadImage { width: u32, height: u32, len: usize },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
