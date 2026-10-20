//! Fuzz target `music`: any bytes as a slideshow music file (`formats::audio`,
//! `docs/video-decoders.md` §7.3, `security.md` §4.2, §8.7) through [`Music`], the reader the
//! slideshow plays: sniffed as MP3 (`nanomp3`), Ogg Vorbis (`lewton`), FLAC (`claxon`) or WAV
//! (`hound`), or refused.
//!
//! Whatever the input: the file is refused with a reason, or it opens as the kind it sniffs as, at
//! a rate in [`RATES`] with 1 to [`MAX_CHANNELS`] channels, and every block it decodes is whole
//! frames (at most [`MAX_CHUNK_FRAMES`]) of samples within −1 ..= 1 (never NaN); a decoder that
//! fails partway says why once and the file then ends. Never a panic outside the
//! decoders' guard, a hang, or an allocation past the limits.

#![no_main]

mod av;

use dalica_image::formats::audio::{MAX_CHANNELS, MAX_CHUNK_FRAMES, Music, RATES, sniff};
use libfuzzer_sys::fuzz_target;

/// The most frames read from one file.
const MAX_FRAMES: usize = 1 << 20;

fuzz_target!(init: av::let_decoder_panics_through(), |data: &[u8]| {
    let kind = sniff(data);
    let mut music = match Music::from_bytes(data.to_vec()) {
        Ok(m) => m,
        Err(e) => {
            assert!(!e.reason.is_empty(), "refused without a reason");
            return;
        }
    };
    assert_eq!(Some(music.kind()), kind, "opened as another kind than it sniffs as");
    assert!(RATES.contains(&music.rate()), "a rate of {}", music.rate());
    let channels = usize::from(music.channels());
    assert!(channels > 0 && channels <= usize::from(MAX_CHANNELS));
    let mut frames = 0usize;
    loop {
        match music.next_chunk() {
            Ok(Some(block)) => {
                assert!(!block.is_empty() && block.len() % channels == 0, "a block of part frames");
                assert!(block.len() / channels <= MAX_CHUNK_FRAMES);
                assert!(block.iter().all(|s| (-1.0..=1.0).contains(s)), "a sample past full scale or not a number");
                frames += block.len() / channels;
                if frames > MAX_FRAMES {
                    return;
                }
            }
            Ok(None) => break,
            Err(e) => {
                assert!(!e.reason.is_empty());
                assert_eq!(music.failed().map(|f| f.reason.clone()), Some(e.reason));
                break;
            }
        }
    }
    // The file has ended, and stays ended.
    assert!(matches!(music.next_chunk(), Ok(None)));
    assert!(music.read_frames(16).is_ok_and(|v| v.is_empty()));
});
