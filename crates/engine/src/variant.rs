//! A│A's variant (`editing.md` §4, `roadmap.md` M11): the right side of comparing a photo with
//! itself, a scratch history of that photo held in the session only. It starts as a copy of the
//! photo's recipe, has its own undo, and reaches no sidecar until it is kept. A stub until M11.10
//! fills it.

use dalica_core::PhotoId;

/// The variant open in this session, if any.
#[derive(Debug, Default)]
pub struct Variant {
    photo: Option<PhotoId>,
}

impl Variant {
    /// The photo whose variant is open.
    pub fn photo(&self) -> Option<PhotoId> {
        self.photo
    }
}
