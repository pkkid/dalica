//! Lossless Save of a JPEG whose recipe only rotates and flips (`data-model.md` §6,
//! `formats.md` §1): the DCT-domain transform of `image::jpeg_transform` (M8.8), or the EXIF
//! orientation flag when it answers "not lossless here", through the same journaled steps as a
//! rendered Save. Built by M8.14; until then every Save renders ([`save`] answers `None`).

use dalica_edit::{Op, Recipe};
use dalica_meta::Changes;

use super::{Options, Photo, Result, Saved};

/// Whether `recipe` holds only rotations and flips (and at least one): the recipes a JPEG can
/// save without re-encoding.
pub fn applies(recipe: &Recipe) -> bool {
    !recipe.is_empty() && recipe.ops().iter().all(|op| matches!(op, Op::Rotate(_) | Op::FlipH | Op::FlipV))
}

/// Saves `photo` losslessly when it can (M8.14); `None` sends it to the rendered Save.
pub(super) fn save(photo: &Photo, recipe: &Recipe, changes: &Changes, options: &Options) -> Result<Option<Saved>> {
    let _ = (photo, recipe, changes, options);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_rotations_and_flips_apply() {
        let parse = |ops: &[&str]| Recipe::parse(ops).unwrap_or_default();
        assert!(applies(&parse(&["rotate(90)", "flip_h()"])));
        assert!(!applies(&parse(&[])));
        assert!(!applies(&parse(&["rotate(90)", "autofix()"])));
    }
}
