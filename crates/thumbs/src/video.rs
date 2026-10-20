//! Videos' poster frames (reserved decision 2, `docs/video-decoders.md`; `architecture.md` §6.5):
//! a clip's thumbnails at every ladder size, and the edit view's preview of it, are its poster
//! frame, the first frame whose mean luma clears `dalica_image`'s `POSTER_LUMA` within its first
//! ten seconds, else its first frame (`FrameSource::poster`, M15.15), upright and in 8-bit sRGB.
//!
//! - **Which clips** ([`plays`]): those whose video codec this build decodes, by the container's
//!   code alone (H.264 always, HEVC with `dalica-image`'s `hevc` feature), so a clip Dalica
//!   cannot play is never opened: its cell keeps the video tile. A clip whose codec passes but
//!   whose stream the decoder refuses (a profile it does not take) answers
//!   [`ladder::Error::Unplayable`] with its reason; a damaged one, its decode error.
//! - **[`scan`]** and **[`fetch`]** are the ladder's two jobs for a clip ([`ladder::scan`],
//!   [`ladder::fetch`]): the same entries, keys and sketch, drawn from one poster decode (at 256
//!   for the scan, which fits it to 128 too; at the size asked for a fetch). A clip takes no
//!   edits into its pixels (its rotation is the cell's, its trim is playback's), so its key is
//!   [`Source::key_for`]'s, made again when the file changes (its size or modification time).
//! - **Cost:** opening a clip reads its sample table (bounded by `dalica_image`'s caps), and the
//!   poster decodes from the first sync sample to the first lit frame on the calling worker (no
//!   decoder threads: the pool's workers are the parallelism), each frame scaled to the size
//!   asked before it is kept. The file is read sample by sample, never held whole.
//!
//! Runs on pool workers only, never on the thread that owns the session.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use dalica_image::formats::video::frames::{FrameError, FrameOptions, FrameSource};
use dalica_image::formats::video::{self as clip, VideoInfo};
use dalica_image::sniff::VideoKind;

use crate::Rgba8;
use crate::ladder::{self, Error, Fetched, Render, Scanned, Source};
use crate::pack::{Cache, Size};

/// Whether this build decodes the frames of a clip whose video codec the container names `code`
/// (a sample entry code such as `avc1` or `hvc1`, or a Matroska `CodecID`); `false` for an empty
/// or unknown one.
pub fn plays(code: &str) -> bool {
    why_not(code).is_none()
}

/// Why this build does not decode a clip whose video codec is `code` ("This video can't be played
/// (…)"), or `None` when it does.
pub fn why_not(code: &str) -> Option<String> {
    if code.is_empty() {
        return Some(clip::REASON.to_string());
    }
    let info = VideoInfo {
        kind: VideoKind::Mp4,
        duration_ms: None,
        size: Some((1, 1)),
        rotation: 0,
        created: None,
        video_codec: Some(code.to_string()),
        sound_codec: None,
        frame_rate: None,
        has_sound: false,
        location: None,
    };
    clip::playability(&info).video.err().map(|u| u.reason())
}

/// The poster frame of the clip at `path`, upright 8-bit sRGB, fitted to `long_side` pixels
/// (`None`: the clip's own size; never enlarged).
pub fn poster(path: &Path, long_side: Option<u32>, limits: &dalica_image::Limits) -> Result<Rgba8, Error> {
    let options = FrameOptions { max_side: long_side.map(|s| s.max(1)), threads: 0 };
    let frame = FrameSource::open(path, limits, options).and_then(|mut src| src.poster()).map_err(error)?;
    Ok(frame.image)
}

/// A frame source's error as the ladder's.
fn error(e: FrameError) -> Error {
    match e {
        FrameError::Unplayable(u) => Error::Unplayable(u.reason()),
        FrameError::Image(e) => Error::Photo(crate::Error::Image(e)),
    }
}

/// The scan job for the clip at `source` ([`ladder::scan_with`] for a clip): its 128 and 256
/// entries from one poster decode unless the cache has both, and its colour sketch when
/// `want_sketch`.
pub fn scan(
    cache: &Cache,
    source: &Source,
    render: &Render,
    want_sketch: bool,
    limits: &dalica_image::Limits,
    cancel: &AtomicBool,
) -> Result<Scanned, Error> {
    let key = source.key_for(render);
    ladder::scan_by(cache, key, want_sketch, limits, cancel, |long_side| {
        checked(cancel)?;
        poster(&source.path, Some(long_side), limits)
    })
}

/// The fetch job for the clip at `source` ([`ladder::fetch_with`] for a clip): `size` from the
/// cache, else its poster decoded to fit `size` and stored.
pub fn fetch(
    cache: &Cache,
    source: &Source,
    render: &Render,
    size: Size,
    screen: u32,
    limits: &dalica_image::Limits,
    cancel: &AtomicBool,
) -> Result<Fetched, Error> {
    let key = source.key_for(render);
    ladder::fetch_by(cache, key, size, screen, limits, cancel, |long_side| {
        checked(cancel)?;
        poster(&source.path, Some(long_side), limits)
    })
}

fn checked(cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) { Err(Error::Cancelled) } else { Ok(()) }
}
