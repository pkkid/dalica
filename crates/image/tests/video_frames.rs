//! Video frames and the clip's sound (reserved decision 2, `docs/video-decoders.md`): H.264 clips
//! made here (an encoder of our own that writes every macroblock as I_PCM, ITU-T H.264 §7.3.5,
//! muxed into MP4 with silent AAC-LC packets), and the corpus job's phone clips (`corpus/video/`,
//! skipped and named when absent) for H.264 and, with the `hevc` feature, HEVC. Posters, seeking,
//! stepping, rotation, sound, the codecs Dalica cannot play, and damaged, cut and oversized
//! streams.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::common::full;

use dalica_image::formats::video::frames::{FrameError, FrameOptions, FrameSource, MAX_FRAME_SIDE, POSTER_LUMA};
use dalica_image::formats::video::sound::SoundSource;
use dalica_image::formats::video::{self, SoundPlay, VideoCodec, Why};
use dalica_image::{Error, Limits, Rgba8};

use crate::clip_writer::*;

fn open(bytes: Vec<u8>) -> Result<FrameSource, FrameError> {
    FrameSource::from_bytes(bytes, &Limits::default(), FrameOptions::default())
}

/// The mean grey of the frame away from its marker (the bottom-right quarter).
fn grey_of(img: &Rgba8) -> u8 {
    let (w, h) = (img.width as usize, img.height as usize);
    let at = ((h * 3 / 4) * w + w * 3 / 4) * 4;
    img.data[at]
}

/// The frame number a grey belongs to.
fn frame_of(img: &Rgba8) -> Option<usize> {
    let g = grey_of(img);
    (0..200).find(|&k| {
        let y = f64::from(grey(k));
        let expected = ((y - 16.0) / 219.0 * 255.0).round();
        (f64::from(g) - expected).abs() <= 2.0 && (k >= BLACK || g < 3)
    })
}

#[test]
fn a_clip_that_starts_black_gives_its_first_lit_frame_as_poster() {
    let mut src = open(clip(50, IDENTITY, None, b"avc1")).unwrap();
    assert_eq!(src.frame_count(), 50);
    let poster = src.poster().unwrap();
    assert_eq!(poster.index, BLACK, "the first frame with a mean luma of {POSTER_LUMA} or more");
    assert_eq!(poster.time_ms, BLACK as u64 * 40);
    assert_eq!(frame_of(&poster.image), Some(BLACK));
    assert_eq!((poster.image.width, poster.image.height), (160, 96));
    // A clip black throughout: its first frame.
    let mut dark = open(clip(BLACK, IDENTITY, None, b"avc1")).unwrap();
    assert_eq!(dark.poster().unwrap().index, 0);
}

#[test]
fn the_frame_at_a_time_and_stepping_both_ways() {
    let mut src = open(clip(50, IDENTITY, None, b"avc1")).unwrap();
    // 1.5 s into a 2 s clip at 25 fps: frame 37 (shown from 1,480 ms).
    let f = src.frame_at(1500).unwrap();
    assert_eq!((f.index, f.time_ms), (37, 1480));
    assert_eq!(frame_of(&f.image), Some(37));
    // Forward three, then back three: the same frames.
    let mut forward = vec![f.clone()];
    for _ in 0..3 {
        forward.push(src.next_frame().unwrap().unwrap());
    }
    assert_eq!(forward.iter().map(|f| f.index).collect::<Vec<_>>(), [37, 38, 39, 40]);
    let mut back = vec![forward[3].clone()];
    for _ in 0..3 {
        back.push(src.previous_frame().unwrap().unwrap());
    }
    back.reverse();
    assert_eq!(back, forward);
    // Further back than the frames kept: decoded again from the sync sample, identical.
    let again = src.frame(12).unwrap();
    let mut fresh = open(clip(50, IDENTITY, None, b"avc1")).unwrap();
    assert_eq!(fresh.frame(12).unwrap(), again);
    assert_eq!(frame_of(&again.image), Some(12));
    // The key frames (every tenth) at or after a frame.
    assert_eq!([0, 1, 10, 41].map(|i| src.key_after(i)), [Some(0), Some(10), Some(10), None]);
    // The ends.
    assert_eq!(src.frame_at(0).unwrap().index, 0);
    assert!(src.previous_frame().unwrap().is_none());
    assert_eq!(src.frame_at(60_000).unwrap().index, 49);
    assert!(src.next_frame().unwrap().is_none());
    assert!(matches!(src.frame(50), Err(FrameError::Image(Error::Malformed(_)))));
}

#[test]
fn a_turned_clip_comes_out_upright_and_scaled() {
    let mut src = open(clip(12, QUARTER, None, b"avc1")).unwrap();
    assert_eq!(src.info().rotation, 1);
    let f = src.frame(6).unwrap();
    // Stored 160 × 96 with a white top-left corner; a quarter turn clockwise puts it top-right.
    assert_eq!((f.image.width, f.image.height), (96, 160));
    let px = |x: usize, y: usize| f.image.data[(y * 96 + x) * 4];
    assert!(px(90, 5) > 250 && px(5, 5) < 200, "{} {}", px(90, 5), px(5, 5));
    // Scaled at decode to a long side of 40.
    let mut small =
        FrameSource::from_bytes(clip(12, QUARTER, None, b"avc1"), &Limits::default(), FrameOptions { max_side: Some(40), threads: 0 }).unwrap();
    let s = small.frame(6).unwrap();
    assert_eq!((s.image.width, s.image.height), (24, 40));
    assert_eq!(frame_of(&s.image), Some(6));
}

#[test]
fn threads_give_the_same_frames() {
    // Sources move to the workers that decode them.
    fn send<T: Send>() {}
    send::<FrameSource>();
    send::<SoundSource>();
    let mut one = open(clip(30, IDENTITY, None, b"avc1")).unwrap();
    let mut four =
        FrameSource::from_bytes(clip(30, IDENTITY, None, b"avc1"), &Limits::default(), FrameOptions { max_side: None, threads: 4 }).unwrap();
    for ms in [0, 700, 1100, 300] {
        assert_eq!(one.frame_at(ms).unwrap(), four.frame_at(ms).unwrap(), "{ms}");
    }
}

#[test]
fn the_sound_decodes_to_the_clips_length() {
    let bytes = clip(50, IDENTITY, Some(b"mp4a"), b"avc1");
    let info = video::probe(&bytes, &Limits::default()).unwrap();
    let p = video::playability(&info);
    assert_eq!((p.video, p.sound), (Ok(VideoCodec::H264), SoundPlay::Plays));
    let mut sound = SoundSource::from_bytes(bytes, &Limits::default()).unwrap();
    assert_eq!((sound.rate(), sound.channels()), (48_000, 1));
    let samples = sound.read_all(10 * 48_000).unwrap();
    // 2 s of sound, to the packet (1,024 samples).
    assert!(samples.len().abs_diff(96_000) < 1024, "{}", samples.len());
    assert!(samples.iter().all(|&s| s == 0.0));
    assert_eq!(sound.damaged(), 0);
    // From the packet playing at 1 s on: half of it, to the packet.
    sound.seek(1000);
    let rest = sound.read_all(10 * 48_000).unwrap();
    assert!(rest.len() >= 48_000 && rest.len() < 48_000 + 2 * 1024, "{}", rest.len());
    // Past the cap: refused, not grown.
    sound.seek(0);
    assert_eq!(sound.read_all(10_000), Err(Error::TooMany("sound samples in a clip")));
}

#[test]
fn sound_dalica_cannot_decode_plays_muted_with_the_reason() {
    let bytes = clip(10, IDENTITY, Some(b"ac-3"), b"avc1");
    let info = video::probe(&bytes, &Limits::default()).unwrap();
    let muted = "This clip plays without sound (AC-3 sound can't be played)";
    assert_eq!(video::playability(&info).sound, SoundPlay::Muted(muted.into()));
    assert_eq!(SoundSource::from_bytes(bytes.clone(), &Limits::default()).err().map(|m| m.reason), Some(muted.into()));
    // Its frames still play.
    assert!(open(bytes).unwrap().frame(3).is_ok());
    let silent = clip(10, IDENTITY, None, b"avc1");
    assert_eq!(video::playability(&video::probe(&silent, &Limits::default()).unwrap()).sound, SoundPlay::Silent);
    assert_eq!(SoundSource::from_bytes(silent, &Limits::default()).err().map(|m| m.reason), Some("This clip has no sound".into()));
}

#[test]
fn a_codec_dalica_cannot_play_says_so_with_its_name() {
    for (code, name) in [(b"mp4v", "MPEG-4 Part 2"), (b"apcn", "ProRes"), (b"av01", "AV1"), (b"zzzz", "zzzz")] {
        let bytes = clip(3, IDENTITY, None, code);
        let info = video::probe(&bytes, &Limits::default()).unwrap();
        assert!(!video::playable(&info), "{name}");
        let reason = format!("This video can't be played ({name})");
        let err = open(bytes).err().unwrap();
        assert_eq!(err.reason(), reason);
        assert!(matches!(err, FrameError::Unplayable(ref u) if u.why == Why::NoDecoder));
    }
    // HEVC without the `hevc` feature: Dalica has the decoder, this build left it out.
    let info = video::VideoInfo {
        video_codec: Some("hvc1".into()),
        size: Some((1920, 1080)),
        ..video::probe(&clip(1, IDENTITY, None, b"avc1"), &Limits::default()).unwrap()
    };
    let p = video::playability(&info);
    if cfg!(feature = "hevc") {
        assert_eq!(p.video, Ok(VideoCodec::Hevc));
    } else {
        let u = p.video.unwrap_err();
        assert_eq!(u.reason(), "This video can't be played (HEVC support isn't included in this build)");
    }
}

/// Every outcome of reading every frame is a frame or an error with its reason, within `limit`.
fn read_everything(src: &mut FrameSource, limit: Duration) -> (usize, Vec<String>) {
    let start = Instant::now();
    let (mut ok, mut reasons) = (0, Vec::new());
    for i in 0..src.frame_count() {
        match src.frame(i) {
            Ok(_) => ok += 1,
            Err(e) => reasons.push(e.reason()),
        }
        assert!(start.elapsed() < limit, "a damaged stream hung");
    }
    let _ = src.poster();
    (ok, reasons)
}

#[test]
fn cut_damaged_and_oversized_streams_are_refused_with_their_reasons() {
    let good = clip(30, IDENTITY, Some(b"mp4a"), b"avc1");
    // Cut: the movie box (at the end) is gone.
    for at in [good.len() / 2, good.len() - 5, 40, 9] {
        let err = open(good[..at].to_vec()).err().unwrap();
        assert!(matches!(err, FrameError::Image(Error::Truncated)), "{at}: {err:?}");
        assert_eq!(err.reason(), "Damaged file");
    }
    // A sample whose NAL unit runs past its end (the stream cut inside the sample).
    let mut cut = good.clone();
    let mdat = good.windows(4).position(|w| w == b"mdat").unwrap() + 4;
    let first_len = u32::from_be_bytes(cut[mdat..mdat + 4].try_into().unwrap());
    cut[mdat..mdat + 4].copy_from_slice(&(first_len + 1000).to_be_bytes());
    let mut src = open(cut).unwrap();
    let err = src.frame(0).unwrap_err();
    assert_eq!(err.reason(), "Damaged file");
    // Bit flips through the coded frames: concealed or refused, never a panic or a hang.
    let mut seed = 0x9e37_79b9_u32;
    for round in 0..if full() { 40 } else { 6 } {
        let mut bad = good.clone();
        for _ in 0..30 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = 60 + (seed as usize % (good.len() / 2));
            bad[at] ^= 1 << (seed % 8);
        }
        match open(bad.clone()) {
            Ok(mut src) => {
                let (_, reasons) = read_everything(&mut src, Duration::from_secs(60));
                assert!(reasons.iter().all(|r| !r.is_empty()), "{round}");
            }
            Err(e) => assert!(!e.reason().is_empty()),
        }
        if let Ok(mut s) = SoundSource::from_bytes(bad, &Limits::default()) {
            let _ = s.read_all(1 << 20);
        }
    }
    // A frame size past the limits: refused before the decoder sees it.
    let wide = (MAX_FRAME_SIDE as u64 / 16) + 1;
    // A configuration whose SPS declares 8,208 × 32 over frames of 160 × 96.
    let s = slice(true, 0, 0, MBS.0, MBS.1, &|_, _| 100);
    let mut sample = be32(s.len() as u32).to_vec();
    sample.extend(s);
    let spec = TrackSpec {
        handler: *b"vide",
        entry: avc1(b"avc1", 160, 96, &sps(wide, 2), &pps()),
        timescale: 1000,
        samples: vec![(sample.len() as u32, 40)],
        syncs: None,
        matrix: IDENTITY,
        size: (160, 96),
    };
    let huge = mp4(&[(spec, vec![sample])]);
    let err = open(huge).err().unwrap();
    assert_eq!(err, FrameError::Image(Error::TooLarge { width: wide as u32 * 16, height: 32 }));
    assert!(err.reason().starts_with("Too large to display"));
    // The caller's limits bite too: no frame above 10,000 pixels.
    let small = Limits { max_pixels: 10_000, ..Limits::default() };
    let err = FrameSource::from_bytes(clip(2, IDENTITY, None, b"avc1"), &small, FrameOptions::default()).err().unwrap();
    assert_eq!(err, FrameError::Image(Error::TooLarge { width: 160, height: 96 }));
    // And the memory a decoder may hold: 20 pictures and the kept frames must fit `max_alloc`.
    let tight = Limits { max_alloc: 160 * 96 * 20, ..Limits::default() };
    assert!(matches!(
        FrameSource::from_bytes(clip(2, IDENTITY, None, b"avc1"), &tight, FrameOptions::default()),
        Err(FrameError::Image(Error::TooLarge { .. }))
    ));
}

#[test]
fn matroska_clips_give_the_same_frames_and_sound() {
    for (lacing, unknown) in [(0, false), (1, true), (2, false), (3, true)] {
        let bytes = mkv(30, true, lacing, unknown);
        let info = video::probe(&bytes, &Limits::default()).unwrap();
        assert_eq!(video::playability(&info).video, Ok(VideoCodec::H264));
        let mut src = open(bytes.clone()).unwrap();
        assert_eq!(src.frame_count(), 30, "{lacing}");
        assert_eq!(src.poster().unwrap().index, BLACK);
        let f = src.frame_at(1000).unwrap();
        assert_eq!((f.index, f.time_ms), (25, 1000));
        assert_eq!(f, open(clip(30, IDENTITY, None, b"avc1")).unwrap().frame(25).unwrap());
        let mut sound = SoundSource::from_bytes(bytes, &Limits::default()).unwrap();
        let samples = sound.read_all(48_000 * 10).unwrap();
        assert_eq!(samples.len(), 57 * 1024, "lacing {lacing}");
    }
}

#[test]
fn a_file_with_no_video_track_is_unplayable() {
    let packets = 10;
    let spec = TrackSpec {
        handler: *b"soun",
        entry: sound_entry(b"mp4a"),
        timescale: 48_000,
        samples: vec![(4, 1024); packets],
        syncs: None,
        matrix: IDENTITY,
        size: (0, 0),
    };
    let bytes = mp4(&[(spec, vec![SILENT_AAC.to_vec(); packets])]);
    let err = open(bytes).err().unwrap();
    assert!(matches!(err, FrameError::Unplayable(ref u) if u.why == Why::NoVideo), "{err:?}");
    assert_eq!(err.reason(), "This video can't be played (it has no video track)");
}

// ---------------------------------------------------------------------------------------------
// The corpus job's phone clips (`cargo xtask corpus video`; `docs/video-decoders.md` §9).

fn corpus(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/video").join(name);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("skipped: corpus/video/{name} is not fetched (cargo xtask corpus video)");
        None
    }
}

/// The checks every passing codec's phone clip meets: the frame at 1.5 s, stepping forward and
/// back, upright, the poster within the first 10 s, and the sound's length.
fn phone_clip(name: &str, codec: VideoCodec, portrait: bool) {
    let Some(path) = corpus(name) else { return };
    let limits = Limits::default();
    let info = video::probe_file(&path, &limits).unwrap();
    assert_eq!(video::playability(&info).video, Ok(codec), "{name}");
    let mut src = FrameSource::open(&path, &limits, FrameOptions { max_side: Some(480), threads: 0 }).unwrap();
    let frame_ms = 1000.0 / info.frame_rate.unwrap_or(30.0);
    let f = src.frame_at(1500).unwrap();
    assert!((f.time_ms as f64 - 1500.0).abs() <= frame_ms + 1.0 && f.time_ms <= 1500, "{name}: {}", f.time_ms);
    let next = src.next_frame().unwrap().unwrap();
    assert_eq!(next.index, f.index + 1);
    assert_eq!(src.previous_frame().unwrap().unwrap(), f, "{name}");
    // Decoded again from its sync sample by a fresh source: the same pixels.
    let mut fresh = FrameSource::open(&path, &limits, FrameOptions { max_side: Some(480), threads: 0 }).unwrap();
    assert_eq!(fresh.frame(f.index).unwrap(), f, "{name}");
    assert_eq!(f.image.height > f.image.width, portrait, "{name}: upright");
    let poster = src.poster().unwrap();
    assert!(poster.time_ms <= 10_000);
    if info.has_sound {
        let mut sound = SoundSource::open(&path, &limits).unwrap();
        let rate = f64::from(sound.rate());
        let channels = usize::from(sound.channels());
        let (mut first, mut count) = (None, 0usize);
        while let Some(chunk) = sound.next_chunk().unwrap() {
            first.get_or_insert(chunk.time_ms);
            count += chunk.samples.len();
        }
        // From where the track starts (a phone may start it a little after the picture) to its end.
        let end = first.unwrap_or(0) as f64 / 1000.0 + count as f64 / channels as f64 / rate;
        let clip_s = info.duration_ms.unwrap_or(0) as f64 / 1000.0;
        assert!((end - clip_s).abs() < 0.1, "{name}: sound to {end} s in a {clip_s} s clip");
        assert_eq!(sound.damaged(), 0);
    }
}

#[test]
fn phone_clips_h264() {
    phone_clip("IMG_6851.MOV", VideoCodec::H264, true);
    if full() {
        phone_clip("20260404_201346.mp4", VideoCodec::H264, true);
        phone_clip("VID_20261017_160853309.mp4", VideoCodec::H264, false);
    }
}

#[cfg(feature = "hevc")]
#[test]
fn phone_clips_hevc() {
    phone_clip("IMG_0445.MOV", VideoCodec::Hevc, false);
    phone_clip("IMG_0477.MOV", VideoCodec::Hevc, true);
    if full() {
        // Main 10, HLG, Dolby Vision 8.4: tone-mapped to SDR.
        phone_clip("M-6_Fun_Fact.MOV", VideoCodec::Hevc, true);
    }
}
