//! Watching the Scan Always roots for external changes (`data-model.md` §7). A stub today:
//! [`Session::poll`] calls [`Session::poll_watching`] every frame, and it does nothing yet.
//!
//! Seam: M5.17 fills it with M5.11's `Watcher` over the Scan Always roots: events are hints that
//! mark folders dirty, dirty folders are rescanned as scan jobs (through
//! [`crate::scanning`]'s `start_scan`), and a full verification pass runs on the session clock.
//! Its state lives in [`Watching`], a field of the session, so that work never touches
//! `session.rs`.

use crate::session::Session;

/// What the session knows of the watched roots' changes. Empty until M5.17.
#[derive(Debug, Default)]
pub(crate) struct Watching {}

impl Session {
    /// Applies what the watcher reported since the last frame (nothing yet). Returns how many
    /// changes it handled. Never blocks and never touches the disk.
    pub(crate) fn poll_watching(&mut self) -> usize {
        let Watching {} = &self.watching;
        0
    }
}
