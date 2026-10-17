//! Importing (`organizing.md` §3 items 4–6, `roadmap.md` M11): the import job that copies,
//! verifies, names and enters the photos, and what the last finished import from a card copied
//! (for "Delete copied files from the card"). A stub until M11.8 fills it.

use crate::jobs::JobId;

/// The import running or last finished in this session.
#[derive(Debug, Default)]
pub struct ImportRun {
    job: Option<JobId>,
}

impl ImportRun {
    /// The import job running or last run ([`crate::jobs::JobKind::Import`]), if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }
}
