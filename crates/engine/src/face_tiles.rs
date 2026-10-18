//! Face tiles (`roadmap.md` M13 → *Face tiles*): the square face pictures of the Unnamed grid, the
//! tree and the People pane, cut on a worker from the thumbnail ladder at the smallest cached size
//! covering twice the face as shown, kept in memory only, never in the thumbnail cache. A stub
//! until M13.15 fills it.

use crate::session::Session;

/// The face tiles' state.
#[derive(Debug, Default)]
pub struct FaceTiles {
    pending: usize,
}

impl FaceTiles {
    /// The tiles asked for and not yet handed over.
    pub fn pending(&self) -> usize {
        self.pending
    }
}

impl Session {
    /// The face tiles' state.
    pub fn face_tiles(&self) -> &FaceTiles {
        &self.face_tiles
    }
}
