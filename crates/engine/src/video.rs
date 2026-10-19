//! The open video and its playback (`editing.md` §11, `viewing.md` §4; `roadmap.md` M15 →
//! *Video*): at most one video open in the edit view, whether it plays, where it is, and its trim.
//! A stub until M15.16 (`video.inspect`) and M15.25 (playback, driven by the [`crate::sound`]
//! clock) fill it; the volume is [`crate::settings::VideoSettings`].

use dalica_core::PhotoId;

use crate::session::Session;

/// The edit view's video state.
#[derive(Debug, Default)]
pub struct Video {
    open: Option<PhotoId>,
    playing: bool,
    position_ms: u64,
}

impl Video {
    /// The video open in the edit view, if any.
    pub fn open(&self) -> Option<PhotoId> {
        self.open
    }

    /// Whether it is playing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Where it is, in milliseconds from its start.
    pub fn position_ms(&self) -> u64 {
        self.position_ms
    }
}

impl Session {
    /// The edit view's video state.
    pub fn video(&self) -> &Video {
        &self.video
    }
}
