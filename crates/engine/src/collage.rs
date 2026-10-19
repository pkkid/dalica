//! The open collage (`creating.md` §3, `roadmap.md` M14 → *Collage*): at most one Collage room at a
//! time, its document and the room's own history (Ctrl+Z and Ctrl+Y in the room; never the
//! library's undo stack). A stub until M14.16 fills it; the room's last style and settings are
//! [`crate::settings::CollageSettings`].

use crate::session::Session;

/// The Collage room's state.
#[derive(Debug, Default)]
pub struct Collage {
    open: bool,
    undo_steps: usize,
    redo_steps: usize,
}

impl Collage {
    /// Whether a collage is open in the room.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The steps of the room's history that can be undone and redone.
    pub fn history_depth(&self) -> (usize, usize) {
        (self.undo_steps, self.redo_steps)
    }
}

impl Session {
    /// The Collage room's state (`creating.md` §3).
    pub fn collage(&self) -> &Collage {
        &self.collage
    }
}
