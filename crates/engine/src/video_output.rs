//! Save Frame as Picture and Export Clip (`editing.md` §11, `creating.md`; `roadmap.md` M15 →
//! *Video*): the frame written as a JPEG into the Video Frames project, the trimmed clip written
//! where the user chose, each on a worker as a job. A stub until M15.25 fills it.

use crate::jobs::JobId;
use crate::session::Session;

/// Save Frame's and Export Clip's state.
#[derive(Debug, Default)]
pub struct VideoOutput {
    job: Option<JobId>,
}

impl VideoOutput {
    /// The job running or last run, if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }
}

impl Session {
    /// Save Frame's and Export Clip's state.
    pub fn video_output(&self) -> &VideoOutput {
        &self.video_output
    }
}
