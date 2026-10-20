//! Fuzz target `video_frames`: a clip's frames (`docs/video-decoders.md`, `security.md` §4.2,
//! §8.7) through [`FrameSource`], the path the player, the slideshow and the poster tiles use: the
//! container's sample table (MP4, M4V, QuickTime) or clusters (Matroska), the gate on every
//! sequence parameter set, the H.264 decoder (`altc264`) and the HEVC one (`rusty_h265`), and the
//! conversion to upright sRGB.
//!
//! The first byte chooses the input's shape. Even: the rest is a clip file. Odd: the rest is cut
//! into chunks (16-bit lengths), the first one to four (bits 2–3) parameter sets and the rest
//! samples, muxed here into an MP4 (bit 1: HEVC, else H.264; bit 4: only the first sample is a
//! sync sample; bit 5: each chunk is a whole sample with its own length fields, else one NAL
//! unit), so every run reaches a decoder. Bit 6 decodes with two threads.
//!
//! Whatever the input: the clip is refused with a reason, or it opens with at least one frame,
//! and every frame it gives (the poster, stepping forward and back, the frame at a time) is at
//! most the size asked for, whole, numbered within the clip and timed as the clip says. Never a
//! panic outside a decoder's guard, a hang, or an allocation past the limits.

#![no_main]

mod av;

use dalica_image::Limits;
use dalica_image::formats::video::frames::{Frame, FrameError, FrameOptions, FrameSource};
use libfuzzer_sys::fuzz_target;

/// The longest side of the frames asked for.
const SIDE: u32 = 64;

/// Frames stepped through after the poster.
const STEPS: usize = 6;

fn limits() -> Limits {
    // Small enough that a hostile size is refused before the decoder allocates for it, and one
    // frame decodes quickly.
    Limits { max_side: 1024, max_pixels: 1 << 18, max_alloc: 256 << 20, ..Limits::default() }
}

fn check(src: &FrameSource, got: Result<Frame, FrameError>) -> Option<usize> {
    let f = match got {
        Ok(f) => f,
        Err(e) => {
            assert!(!e.reason().is_empty(), "an error without a reason");
            return None;
        }
    };
    let (w, h) = (f.image.width, f.image.height);
    assert!(w > 0 && h > 0 && w.max(h) <= SIDE, "a frame of {w} × {h}");
    assert_eq!(f.image.data.len(), w as usize * h as usize * 4);
    assert!(f.index < src.frame_count(), "frame {} of {}", f.index, src.frame_count());
    assert_eq!(src.time_of(f.index), Some(f.time_ms));
    Some(f.index)
}

fuzz_target!(init: av::let_decoder_panics_through(), |data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode & 1 == 0 {
        rest.to_vec()
    } else {
        let chunks = av::chunks(rest);
        let params = (usize::from(mode >> 2) & 3) + 1;
        if chunks.len() <= params {
            return;
        }
        let (params, samples) = chunks.split_at(params);
        av::video_mp4(mode & 2 != 0, params, samples, mode & 0x20 != 0, mode & 0x10 != 0)
    };
    let options = FrameOptions { max_side: Some(SIDE), threads: if mode & 0x40 != 0 { 2 } else { 0 } };
    let Ok(mut src) = FrameSource::from_bytes(bytes, &limits(), options) else { return };
    let n = src.frame_count();
    assert!(n > 0, "a clip without frames opened");
    let poster = src.poster();
    check(&src, poster);
    for _ in 0..STEPS {
        match src.next_frame() {
            Ok(None) => break,
            Ok(Some(f)) => {
                check(&src, Ok(f));
            }
            Err(e) => {
                check(&src, Err(e));
                break;
            }
        }
    }
    let back = src.previous_frame();
    if let Ok(Some(f)) = back {
        check(&src, Ok(f));
    }
    let last = src.time_of(n - 1).unwrap_or(0);
    let middle = src.frame_at(last / 2);
    check(&src, middle);
    let end = src.frame(n - 1);
    check(&src, end);
    assert!(src.frame(n).is_err(), "a frame past the end");
});
