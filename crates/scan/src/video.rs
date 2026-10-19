//! Videos on scan (M15.11, `formats.md` §1): with Options → File Types' video type on (the
//! default), a scan lists MP4, MOV, M4V and MKV files as videos, with what `dalica-image`'s probe
//! reads from the container (no frame decoded) as their facts.
//!
//! - **Size:** the frame size as shown, the container's rotation applied (a phone clip filmed
//!   upright is taller than wide), so the cell is the right shape; 0 × 0 when no video track says.
//! - **Date:** the container's creation date; without one the photo has none, and it sorts by
//!   its file's modification time like any photo without a date.
//! - **Facts** ([`VideoFacts`]): duration, stored frame size, rotation, codecs, sound and frame
//!   rate, derived data the diff sets (`Op::SetVideos`). The codec names are made safe as camera
//!   names are ([`crate::facts::camera`]: control characters dropped, cut at [`MAX_CODEC`] bytes).
//! - **What the file says** ([`FileValues`]): the date and the location, read with the header when
//!   the scan reads metadata, so a location comes in as a geotag the way a photo's GPS does
//!   (`xmp-interop.md` §5; [`crate::meta`]); the search facts say it is a video, and whether it
//!   holds a location.
//!
//! A container the probe refuses (cut, damaged, hostile past its bounds) is still listed, with the
//! reason as a scan failure and the empty record (a video with nothing known); a probe never
//! panics, and the scan's per-file guard holds it to the work budget like a header read. Nothing
//! here writes to a video file.

use std::path::Path;

use dalica_core::DateTime;
use dalica_image::formats::video::{self, VideoInfo};
use dalica_library::sync::Values;
use dalica_library::{Facts, MAX_CODEC, VideoFacts};

use crate::meta::FileValues;

/// What a scan learns of one video from its container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probed {
    /// The frame size as shown (after the rotation); 0 × 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// The container's creation date.
    pub date: Option<DateTime>,
    pub facts: VideoFacts,
    /// What the file says about the fields Dalica owns (its date and location) and its search
    /// facts, for a scan that reads metadata.
    pub file: FileValues,
}

/// Probes the video at `path`, reading only the boxes or elements the facts need.
pub fn read(path: &Path, limits: &dalica_image::Limits) -> dalica_image::Result<Probed> {
    video::probe_file(path, limits).map(|info| probed(&info))
}

/// What a scan takes from a probe's `info`.
pub fn probed(info: &VideoInfo) -> Probed {
    let (width, height) = info.display_size().unwrap_or((0, 0));
    let place = info.location.map(|p| (p.lat(), p.lon()));
    let values = Values { place, date: info.created, ..Values::default() };
    let search = Facts { video: true, geotagged: place.is_some(), ..Facts::default() };
    Probed { width, height, date: info.created, facts: facts(info), file: FileValues(values, search, Vec::new()) }
}

/// The library's facts of a probe's `info`.
pub fn facts(info: &VideoInfo) -> VideoFacts {
    let (width, height) = info.size.unwrap_or((0, 0));
    VideoFacts {
        duration_ms: info.duration_ms.unwrap_or(0),
        width,
        height,
        rotation: info.rotation % 4,
        video_codec: codec(info.video_codec.as_deref()),
        sound_codec: codec(info.sound_codec.as_deref()),
        sound: info.has_sound,
        frame_rate_milli: info.frame_rate.map_or(0, frame_rate_milli),
    }
}

/// A codec name as the facts keep it: control characters dropped, trimmed, at most [`MAX_CODEC`]
/// bytes (cut on a character boundary); empty for none.
fn codec(text: Option<&str>) -> String {
    text.and_then(|t| dalica_meta::fields::clean_tag(t, MAX_CODEC)).unwrap_or_default()
}

/// Frames per 1 000 seconds, rounded; 0 for a rate that is not a positive finite number or does
/// not fit.
fn frame_rate_milli(fps: f64) -> u32 {
    let milli = (fps * 1000.0).round();
    if milli.is_finite() && milli >= 1.0 && milli <= f64::from(u32::MAX) {
        // In range and whole, so the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let m = milli as u32;
        m
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_core::geo::LatLon;
    use dalica_image::sniff::VideoKind;

    /// A probe result built by hand: an upright phone clip.
    fn info() -> VideoInfo {
        VideoInfo {
            kind: VideoKind::QuickTime,
            duration_ms: Some(4_321),
            size: Some((1920, 1080)),
            rotation: 1,
            created: None,
            video_codec: Some("hvc1".into()),
            sound_codec: Some("lp\u{7}cm".into()),
            frame_rate: Some(29.970_029),
            has_sound: true,
            location: LatLon::new(38.7139, -9.1394).ok(),
        }
    }

    #[test]
    fn a_probe_becomes_the_facts_the_cell_and_the_file_values() {
        let p = probed(&info());
        assert_eq!((p.width, p.height), (1080, 1920), "the rotation applied to the cell");
        assert_eq!(
            p.facts,
            VideoFacts {
                duration_ms: 4_321,
                width: 1920,
                height: 1080,
                rotation: 1,
                video_codec: "hvc1".into(),
                sound_codec: "lpcm".into(),
                sound: true,
                frame_rate_milli: 29_970,
            }
        );
        assert_eq!(p.file.0.place, Some((38.7139, -9.1394)));
        assert_eq!(p.file.1, Facts { video: true, geotagged: true, ..Facts::default() });
    }

    #[test]
    fn hostile_numbers_and_text_are_bounded() {
        for (fps, milli) in [(f64::NAN, 0), (f64::INFINITY, 0), (-30.0, 0), (0.0, 0), (1e12, 0), (0.0004, 0), (0.001, 1), (60.0, 60_000)] {
            assert_eq!(frame_rate_milli(fps), milli, "{fps}");
        }
        let long = "é".repeat(MAX_CODEC);
        assert!(codec(Some(&long)).len() <= MAX_CODEC);
        assert_eq!(codec(Some("\u{0}\u{1b}")), "");
        let mut i = info();
        i.rotation = 7;
        i.video_codec = Some(long);
        let f = facts(&i);
        assert!(f.rotation <= 3 && f.video_codec.len() <= MAX_CODEC);
    }
}
