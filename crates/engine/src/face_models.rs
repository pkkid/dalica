//! Face models (`faces.md` §3): the models folder (`<config>/models` or `$DALICA_FACE_MODELS`)
//! shared by the app, the CLI and MCP; the installed, built-in and `catalog.json` models; the
//! download running; the recogniser in use (the newest installed unless
//! `people.model` selects another, `crate::settings::PeopleSettings::model`). A stub until M13.16
//! fills it.

use crate::jobs::JobId;
use crate::session::Session;

/// The face models' state.
#[derive(Debug, Default)]
pub struct FaceModels {
    download: Option<JobId>,
}

impl FaceModels {
    /// The download running or last run ([`crate::jobs::JobKind::Network`]), if any.
    pub fn download(&self) -> Option<JobId> {
        self.download
    }
}

impl Session {
    /// The face models' state (`faces.md` §3).
    pub fn face_models(&self) -> &FaceModels {
        &self.face_models
    }
}
