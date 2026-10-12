//! The recipe: a photo's edits in the order the user applied them (`data-model.md` §3.1).
//!
//! - [`Recipe::parse`] reads the sidecar's list of operation strings and [`Recipe::print`] writes
//!   the same strings back; legacy and unknown operations keep their text and their place.
//! - [`Recipe::push`] appends an operation with the rules for rotations and flips: a rotation right
//!   after a rotation merges into it (a net whole turn removes both), and a flip right after the
//!   same flip cancels it. Only the last operation is ever merged, so an operation between two
//!   rotations keeps both.
//! - [`Recipe::pipeline`] is the order rendering applies them in: geometry first (rotations,
//!   flips, crop and straighten, in the order they stand), then every other known operation in the
//!   order applied; legacy and unknown operations are skipped.
//! - [`Recipe::orientation`] is what the rotations and flips add up to, and
//!   [`Recipe::display`] composes it after the EXIF orientation.

use dalica_image::Orientation;

use crate::{Dihedral, Error, Op, Result, Stage};

/// The most operations one recipe holds (`security.md` §8, the sidecar limits).
pub const MAX_OPERATIONS: usize = 10_000;

/// A photo's edit operations, in the order applied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Recipe {
    ops: Vec<Op>,
}

impl Recipe {
    /// An empty recipe: the photo as its file shows it.
    pub fn new() -> Recipe {
        Recipe::default()
    }

    /// The recipe the operation strings stand for, kept as they are (no merging). Fails only when
    /// there are more than [`MAX_OPERATIONS`].
    pub fn parse<I, S>(texts: I) -> Result<Recipe>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut ops = Vec::new();
        for text in texts {
            if ops.len() >= MAX_OPERATIONS {
                return Err(Error::TooManyOperations);
            }
            ops.push(Op::parse(text.as_ref()));
        }
        Ok(Recipe { ops })
    }

    /// The operation strings, exactly as parsed and appended.
    pub fn print(&self) -> Vec<String> {
        self.ops.iter().map(Op::to_string).collect()
    }

    /// The operations in the order applied.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Appends `op` with the rules for rotations and flips (see the module). Fails only when the
    /// recipe already holds [`MAX_OPERATIONS`] and `op` would be a new entry.
    pub fn push(&mut self, op: Op) -> Result<()> {
        match (self.ops.last(), &op) {
            (Some(Op::Rotate(before)), Op::Rotate(turn)) => {
                let merged = crate::Turn::from_quarters(before.quarters() + turn.quarters());
                self.ops.pop();
                if let Some(merged) = merged {
                    self.ops.push(Op::Rotate(merged));
                }
                Ok(())
            }
            (Some(Op::FlipH), Op::FlipH) | (Some(Op::FlipV), Op::FlipV) => {
                self.ops.pop();
                Ok(())
            }
            _ if self.ops.len() >= MAX_OPERATIONS => Err(Error::TooManyOperations),
            _ => {
                self.ops.push(op);
                Ok(())
            }
        }
    }

    /// The operations rendering applies, in pipeline order (see the module).
    pub fn pipeline(&self) -> impl Iterator<Item = &Op> {
        let stage = |wanted: Stage| move |op: &&Op| op.stage() == wanted;
        self.ops.iter().filter(stage(Stage::Geometry)).chain(self.ops.iter().filter(stage(Stage::Adjust)))
    }

    /// What the recipe's rotations and flips add up to, applied to the upright picture.
    pub fn orientation(&self) -> Dihedral {
        self.ops.iter().filter_map(Op::orientation).fold(Dihedral::IDENTITY, Dihedral::then)
    }

    /// The transform from the stored pixels of a file with EXIF orientation `exif` to the picture
    /// the user sees: the EXIF orientation, then the recipe's.
    pub fn display(&self, exif: Orientation) -> Dihedral {
        Dihedral::from_exif(exif).then(self.orientation())
    }

    /// The displayed size of a file whose stored pixels are `width × height` with EXIF
    /// orientation `exif`.
    pub fn display_size(&self, exif: Orientation, width: u32, height: u32) -> (u32, u32) {
        self.display(exif).display_size(width, height)
    }
}
