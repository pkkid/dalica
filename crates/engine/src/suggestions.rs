//! Suggestions (`faces.md` §5): "Name?" for an unnamed face whose best person scores at or above
//! the Suggestion confidence's threshold and ahead of the runner-up, kept current as faces are
//! embedded and named; the pending count per person. A stub until M13.26 fills it.

use crate::session::Session;

/// The suggestions' state.
#[derive(Debug, Default)]
pub struct Suggestions {
    generation: u64,
}

impl Suggestions {
    /// Rises each time the suggestions change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Session {
    /// The suggestions' state (`faces.md` §5).
    pub fn suggestions(&self) -> &Suggestions {
        &self.suggestions
    }
}
