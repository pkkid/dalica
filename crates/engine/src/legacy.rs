//! The previous organizer's data in the session (`data-model.md` §9, `roadmap.md` M14 → *Legacy
//! sidecars*): what the scans found in legacy sidecars, applied as a scan's imports are, and the
//! first-run notice. A stub until M14.13 fills it; the settings are
//! [`crate::settings::LegacySettings`].

use crate::session::Session;

/// The legacy data's state.
#[derive(Debug, Default)]
pub struct Legacy {
    notice: Option<usize>,
}

impl Legacy {
    /// The number of folders with legacy data the first-run notice names, while it waits to be
    /// shown.
    pub fn notice(&self) -> Option<usize> {
        self.notice
    }
}

impl Session {
    /// The legacy data's state (`data-model.md` §9).
    pub fn legacy(&self) -> &Legacy {
        &self.legacy
    }
}
