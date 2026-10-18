//! The in-memory face index (`faces.md` §5–§6, `roadmap.md` M13 → *Grouping*): the embeddings of
//! the model in use, loaded from its cache file beside the index, and the groups of unnamed faces
//! recomputed from them (never user data). A stub until M13.18 fills it.

use crate::session::Session;

/// The face index's state.
#[derive(Debug, Default)]
pub struct FacesIndex {
    model: Option<String>,
    generation: u64,
}

impl FacesIndex {
    /// The id of the model whose embeddings are loaded, if any.
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Rises each time the faces or their groups change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Session {
    /// The in-memory face index (`faces.md` §5).
    pub fn faces_index(&self) -> &FacesIndex {
        &self.faces_index
    }
}
