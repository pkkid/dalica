//! Geotags (`organizing.md` §7, `roadmap.md` M13 → *Places*): the place Copy Location kept for
//! Paste Location, and the geotags of the photos shown. A stub until M13.14 fills it.

use crate::session::Session;

/// The places' state.
#[derive(Debug, Default)]
pub struct Places {
    copied: Option<[f64; 2]>,
}

impl Places {
    /// The place Copy Location kept, `[latitude, longitude]` in degrees, if any.
    pub fn copied(&self) -> Option<[f64; 2]> {
        self.copied
    }
}

impl Session {
    /// The places' state (`organizing.md` §7).
    pub fn places(&self) -> &Places {
        &self.places
    }
}
