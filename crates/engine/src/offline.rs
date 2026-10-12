//! Offline volumes (`data-model.md` §7, `security.md` §5.9). A stub today: [`Session::poll`] calls
//! [`Session::poll_offline`] every frame, and it does nothing yet.
//!
//! Seam: M5.13 fills it with a worker-filled cache of which roots are online (checked on a
//! schedule and after each scan, never on the owner thread), the `offline` flag on the folders
//! under a root that went away (their photos, rows and cached thumbnails stay), and the queued
//! sidecar writes that wait for the root to come back. Its state lives in [`Offline`], a field of
//! the session, so that work never touches `session.rs`.

use crate::session::Session;

/// Which roots are reachable. Empty until M5.13 (every root counts as online).
#[derive(Debug, Default)]
pub(crate) struct Offline {}

impl Session {
    /// Takes in what the online checks found since the last frame (nothing yet). Returns how many
    /// roots changed state. Never blocks and never touches the disk.
    pub(crate) fn poll_offline(&mut self) -> usize {
        let Offline {} = &self.offline;
        0
    }
}
