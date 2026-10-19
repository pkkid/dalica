//! Create Collage and Save Draft (`creating.md` §3, `roadmap.md` M14 → *Collage*): the page
//! rendered on a worker as a [`crate::jobs::JobKind::Collage`] job, the JPEG and its
//! `.dalica-collage` recipe written into the projects directory's `Collages` folder. A stub until
//! M14.17 fills it.

use crate::jobs::JobId;
use crate::session::Session;

/// Create Collage's and Save Draft's state.
#[derive(Debug, Default)]
pub struct CollageOutput {
    job: Option<JobId>,
}

impl CollageOutput {
    /// The `collage` job running or last run, if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }
}

impl Session {
    /// Create Collage's and Save Draft's state.
    pub fn collage_output(&self) -> &CollageOutput {
        &self.collage_output
    }
}
