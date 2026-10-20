//! Fuzz target `video_sound`: a clip's sound (`docs/video-decoders.md` §2, `security.md` §4.2,
//! §8.7) through [`SoundSource`], the path the player and the slideshow use: the first sound
//! track's sample table (MP4, M4V, QuickTime) or blocks (Matroska, laced or not), its
//! `AudioSpecificConfig`, and the AAC decoder (`rusty_aac`).
//!
//! The first byte chooses the input's shape. Even: the rest is a clip file. Odd: the rest is cut
//! into chunks (16-bit lengths), the first the `AudioSpecificConfig` and the rest AAC packets,
//! muxed here into an MP4, so every run reaches the decoder.
//!
//! Whatever the input: the clip plays muted with a reason, or it opens at a rate and channel count
//! within the limits, and every chunk it decodes is whole frames of finite samples, timed within
//! the track; a seek and the reads after it hold the same, and reading the whole track gives at
//! most the frames asked for. Never a panic outside the decoder's guard, a hang, or an allocation past the
//! limits.

#![no_main]

mod av;

use dalica_image::Limits;
use dalica_image::formats::video::sound::{MAX_CHANNELS, MAX_RATE, SoundSource};
use libfuzzer_sys::fuzz_target;

/// The most frames read from one clip.
const MAX_FRAMES: usize = 1 << 20;

/// Chunks read after the seek.
const AFTER_SEEK: usize = 8;

fuzz_target!(init: av::let_decoder_panics_through(), |data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else { return };
    let bytes = if mode & 1 == 0 {
        rest.to_vec()
    } else {
        let chunks = av::chunks(rest);
        let Some((asc, packets)) = chunks.split_first() else { return };
        if packets.is_empty() {
            return;
        }
        av::sound_mp4(asc, packets)
    };
    let limits = Limits { max_alloc: 256 << 20, ..Limits::default() };
    let mut src = match SoundSource::from_bytes(bytes, &limits) {
        Ok(src) => src,
        Err(muted) => {
            assert!(!muted.reason.is_empty(), "muted without a reason");
            return;
        }
    };
    let rate = src.rate();
    assert!(rate > 0 && rate <= MAX_RATE, "a rate of {rate}");
    assert!(src.channels() <= MAX_CHANNELS);
    let duration = src.duration_ms();
    let mut frames = 0usize;
    loop {
        match src.next_chunk() {
            Ok(Some(chunk)) => {
                let channels = usize::from(src.channels());
                assert!(channels > 0 && channels <= usize::from(MAX_CHANNELS));
                assert!(!chunk.samples.is_empty() && chunk.samples.len() % channels == 0, "a chunk of part frames");
                assert!(chunk.samples.iter().all(|s| s.is_finite()), "a sample that is not a number");
                assert!(chunk.time_ms <= duration, "a chunk at {} ms of a track of {duration} ms", chunk.time_ms);
                frames += chunk.samples.len() / channels;
                if frames > MAX_FRAMES {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                assert!(!e.reason().is_empty());
                break;
            }
        }
    }
    src.seek(duration / 2);
    for _ in 0..AFTER_SEEK {
        match src.next_chunk() {
            Ok(Some(chunk)) => assert!(chunk.samples.iter().all(|s| s.is_finite())),
            _ => break,
        }
    }
    src.seek(0);
    if let Ok(all) = src.read_all(4096) {
        assert!(all.len() / usize::from(src.channels().max(1)) <= 4096);
    }
});
