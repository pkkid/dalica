//! Rendering a collage (M14.10 fills it in; `creating.md` §3): the page drawn at its pixel size
//! from the document and its placements, each picture asked of the caller one at a time (rendered
//! through the one output pipeline with its edits, at the size its place needs), and encoded as a
//! JPEG. This stub fixes the signature the engine's output job (M14.17) calls.

use dalica_image::Rgba8;

use super::doc::Collage;
use super::layout::Placement;

/// Why a collage was not rendered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("collages are not rendered yet")]
    NotBuilt,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// The page's JPEG at `size` pixels. `picture(placement, (width, height))` answers the picture of
/// one placement rendered at about that size (or `None` when it cannot be read: its place is left
/// empty); `cancelled` is asked between pictures; `progress(done, total)` after each.
pub fn render(
    collage: &Collage,
    placements: &[Placement],
    picture: &mut dyn FnMut(&Placement, (u32, u32)) -> Option<Rgba8>,
    size: (u32, u32),
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<u8>, RenderError> {
    let _ = (collage, placements, picture, size, progress);
    if cancelled() {
        return Err(RenderError::Cancelled);
    }
    Err(RenderError::NotBuilt)
}
