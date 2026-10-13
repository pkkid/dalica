//! The edit view's preview renders (`editing.md` §3, `architecture.md` §5–§6): the open photo's
//! screen preview, a tool's draft, the refined window while zoomed, the neighbours prefetched and
//! the histograms. Empty until M7.11 fills it; [`Session::poll`] already calls
//! [`Session::poll_preview`] each frame, so that task changes only this module.

use crate::session::Session;

/// The preview renders' state (`Session::preview`); nothing yet.
#[derive(Debug, Default)]
pub(crate) struct Preview {}

impl Session {
    /// Applies the preview renders that finished; returns how many. Never blocks and never touches
    /// the disk.
    pub(crate) fn poll_preview(&mut self) -> usize {
        let Preview {} = &self.preview;
        0
    }
}
