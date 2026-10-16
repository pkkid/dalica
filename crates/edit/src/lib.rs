//! dalica-edit: The edit recipe: operation syntax parse/print (unknown operations preserved), pipeline order, the CPU renderer for every tool and effect (the oracle), auto-fix analysis.
//!
//! Layer L1; see `docs/architecture.md` §2. M6 brought the recipe and its geometric operations
//! (`data-model.md` §3.1, `editing.md` §10); M7 the renderer and the operations' modules.
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
//! - [`ops`]: one module per operation family, each with its typed parameters, version, history
//!   label and rendering ([`Typed`], [`ops::Warp`], [`ops::Adjust`]).
//! - [`render`]: [`render()`] and [`render_region()`], the CPU renderer for thumbnails, the edit
//!   view, Save and Export, and the recipe's [`Geometry`].
//! - [`version`]: a recipe's [`RenderKey`] for caches (the printed recipe and the versions of the
//!   operations it uses).
//! - [`catalog`]: every operation the parser knows, with its parameters, ranges and defaults, for
//!   agents (M10.9).
//! - [`analysis`]: the fixed-size proxy that analysing operations share; [`suggest`]: suggested
//!   crops (M7.12).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod analysis;
pub mod catalog;
pub mod crop_geom;
pub mod frame;
pub mod geometry;
pub mod guides;
pub mod ops;
pub mod recipe;
pub mod render;
pub mod suggest;
pub mod syntax;
pub mod version;

pub use frame::{Point, Rect};
pub use geometry::Dihedral;
pub use ops::{ParamError, Typed};
pub use recipe::{MAX_OPERATIONS, Recipe};
pub use render::{Geometry, Region, Source, Target, render, render_region};
pub use syntax::{LISTED, Op, Stage, Turn};
pub use version::RenderKey;

/// Why a recipe or an image was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// More operations than `security.md` §8 allows in one recipe.
    #[error("a recipe holds at most {MAX_OPERATIONS} operations")]
    TooManyOperations,
    /// The pixel buffer does not hold `width × height` RGBA pixels.
    #[error("{len} bytes for a {width} × {height} RGBA image")]
    BadImage { width: u32, height: u32, len: usize },
    /// A render whose output, or the window it works on, is over `render::MAX_PIXELS` or
    /// `render::MAX_EDGE`.
    #[error("a {width} × {height} render is too large")]
    TooLarge { width: u64, height: u64 },
    /// A render target or region that asks for nothing (an empty box, a scale that is not finite
    /// and positive or is over `render::MAX_SCALE`, a window outside the picture).
    #[error("bad render target: {0}")]
    BadTarget(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
