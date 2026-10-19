//! Fuzz target `video_probe`: Video recognition and probing (MP4, M4V, QuickTime, Matroska): any bytes give facts or an error, never a panic or a hang.

#![no_main]

use dalica_image::formats::video;
use dalica_image::sniff::VideoKind;
use dalica_image::{Limits, sniff};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = Limits::default();
    let _ = sniff(data);
    let _ = video::probe(data, &limits);
    // Every reader on the same bytes, whatever they sniff as.
    for kind in [VideoKind::Mp4, VideoKind::Matroska] {
        let _ = video::probe_as(kind, data, &limits);
    }
});
