//! Cameras and phones as import sources (`organizing.md` §3; `roadmap.md` M15 → *Cameras*): the
//! devices connected over MTP/PTP, listed beside cards and drives in Import from, read through
//! `dalica-scan`'s PTP reader on the import worker. A stub until M15.19 fills it.

use crate::session::Session;

/// A camera connected, as Import from lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraSource {
    /// The device's name ("Canon EOS R6").
    pub name: String,
    /// The storage's name ("SD1"), when the device has several.
    pub storage: Option<String>,
}

/// The camera sources.
#[derive(Debug, Default)]
pub struct Cameras {
    sources: Vec<CameraSource>,
}

impl Cameras {
    /// The cameras connected, as last listed.
    pub fn sources(&self) -> &[CameraSource] {
        &self.sources
    }
}

impl Session {
    /// The camera sources.
    pub fn cameras(&self) -> &Cameras {
        &self.cameras
    }
}
