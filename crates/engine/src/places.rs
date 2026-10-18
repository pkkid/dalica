//! Geotags (`organizing.md` §7, `data-model.md` §5): the place Copy Location kept for Paste
//! Location, and the geotags of the photos shown (the commands are `cmd/places.rs`).
//!
//! - **The effective geotag** of a photo is the model's word on it ([`dalica_library::Geo`]): a
//!   point, or the location the user removed. A photo the model has no word on shows its file's
//!   GPS: the first scan that reads the file brings that GPS into the model as the photo's point
//!   (`scanning.rs`, `dalica-scan` `meta`), so after a scan every readable file's GPS is a point
//!   here, and a later change made to it in another application follows the file.
//! - **Copy Location** keeps one point for this session (nothing is written); Paste Location gives
//!   it to photos.

use dalica_core::PhotoId;
use dalica_core::geo::LatLon;

use crate::session::Session;

/// The places' state.
#[derive(Debug, Default)]
pub struct Places {
    copied: Option<LatLon>,
}

impl Places {
    /// The place Copy Location kept, `[latitude, longitude]` in degrees, if any.
    pub fn copied(&self) -> Option<[f64; 2]> {
        self.copied.map(|p| [p.lat(), p.lon()])
    }

    /// The place Copy Location kept, if any.
    pub(crate) fn copied_point(&self) -> Option<LatLon> {
        self.copied
    }

    /// Keeps `point` for Paste Location (at six decimals, as a geotag is stored).
    pub(crate) fn copy(&mut self, point: LatLon) {
        self.copied = Some(point.round6());
    }
}

impl Session {
    /// The places' state (`organizing.md` §7).
    pub fn places(&self) -> &Places {
        &self.places
    }

    /// Photo `id`'s effective geotag: the model's point, `None` when the user removed the location
    /// or the model has no point for it (its file's GPS, if any, not read yet).
    pub fn geotag(&self, id: PhotoId) -> Option<LatLon> {
        self.library.geotag(id)
    }
}
