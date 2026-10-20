//! The starting inputs of the clip and music targets, written procedurally into
//! `corpus/<target>/` (gitignored; `security.md` §7.4: media files are never committed). Run from
//! `crates/image/fuzz` before fuzzing them: `cargo +nightly run --release --example seeds`.
//!
//! - `video_frames`: H.264 clips of the test suite's writer (MP4 turned and with sound, Matroska),
//!   and streams for the target's muxer: the writer's SPS, PPS and I_PCM slices, and an HEVC VPS,
//!   SPS, PPS and an IDR slice header followed by bytes for its CABAC data, written here.
//! - `video_sound`: those clips with silent AAC (MP4, Matroska in every lacing), and AAC-LC
//!   configurations with silent packets for the muxer.
//! - `music`: WAV, FLAC, MP3 and Ogg Vorbis files of the test suite's writers.

#[path = "../fuzz_targets/av.rs"]
mod av;
#[path = "../../tests/music_writer.rs"]
mod music_writer;

use std::path::Path;

use av::clip_writer::{Bits, IDENTITY, QUARTER, SILENT_AAC, clip, mkv, nal, pps, slice, sps};

fn write(target: &str, name: &str, bytes: &[u8]) {
    let dir = Path::new("corpus").join(target);
    std::fs::create_dir_all(&dir).expect("the corpus folder");
    std::fs::write(dir.join(name), bytes).expect("a seed");
}

/// `mode` then `bytes`.
fn with(mode: u8, bytes: &[u8]) -> Vec<u8> {
    let mut v = vec![mode];
    v.extend(bytes);
    v
}

// --- HEVC parameter sets and an IDR slice (ITU-T H.265 §7.3), Main profile, 64 × 64 ---------------

fn hevc_nal(kind: u8, rbsp: Vec<u8>) -> Vec<u8> {
    let mut v = nal(0, 0, &rbsp);
    v[0] = kind << 1;
    v.insert(1, 1);
    v
}

fn ptl(b: &mut Bits) {
    b.u(2, 0).u(1, 0).u(5, 1).u(32, 0x6000_0000); // Main, compatible with Main and Main 10
    b.u(1, 1).u(1, 0).u(1, 0).u(1, 1).u(43, 0).u(1, 0); // progressive, frames only
    b.u(8, 90); // level 3
}

fn hevc_vps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(4, 0).u(1, 1).u(1, 1).u(6, 0).u(3, 0).u(1, 1).u(16, 0xffff);
    ptl(&mut b);
    b.u(1, 1).ue(1).ue(0).ue(0); // two pictures, no reordering
    b.u(6, 0).ue(0).u(1, 0).u(1, 0);
    hevc_nal(32, b.trailing())
}

fn hevc_sps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(4, 0).u(3, 0).u(1, 1);
    ptl(&mut b);
    b.ue(0).ue(1).ue(64).ue(64).u(1, 0); // SPS 0, 4:2:0, 64 × 64, no cropping
    b.ue(0).ue(0).ue(4).u(1, 1).ue(1).ue(0).ue(0); // 8 bits, POC LSB of 8 bits, two pictures
    b.ue(0).ue(1).ue(0).ue(2).ue(1).ue(1); // CB 8–16, TB 4–16, one split level
    b.u(1, 0).u(1, 0).u(1, 0).u(1, 0); // no scaling lists, AMP, SAO or PCM
    b.ue(0).u(1, 0).u(1, 0).u(1, 0).u(1, 0).u(1, 0); // no RPS, long-term refs, TMVP, smoothing, VUI, extensions
    hevc_nal(33, b.trailing())
}

fn hevc_pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).u(3, 0).u(1, 0).u(1, 0).ue(0).ue(0).se(0); // QP 26
    b.u(1, 0).u(1, 0).u(1, 0).se(0).se(0).u(1, 0).u(1, 0).u(1, 0).u(1, 0).u(1, 0).u(1, 0).u(1, 0);
    b.u(1, 1).u(1, 0).u(1, 1); // deblocking off
    b.u(1, 0).u(1, 0).ue(0).u(1, 0).u(1, 0);
    hevc_nal(34, b.trailing())
}

/// An IDR (`IDR_W_RADL`) I slice, its CABAC data `data`.
fn hevc_idr(data: &[u8]) -> Vec<u8> {
    let mut b = Bits::default();
    b.u(1, 1).u(1, 0).ue(0).ue(2).se(0); // first slice, PPS 0, I, QP delta 0
    b.u(1, 1).align_zero(); // byte_alignment()
    let mut rbsp = b.out;
    rbsp.extend(data);
    rbsp.push(0x80);
    hevc_nal(19, rbsp)
}

/// How a seed opens, so a run of this example shows that each one reaches its decoder.
fn outcome(target: &str, input: &[u8]) -> String {
    use dalica_image::Limits;
    use dalica_image::formats::audio::Music;
    use dalica_image::formats::video::frames::{FrameOptions, FrameSource};
    use dalica_image::formats::video::sound::SoundSource;
    let unmux = |hevc: bool| {
        let (mode, rest) = input.split_first().expect("a seed");
        if mode & 1 == 0 {
            return rest.to_vec();
        }
        let chunks = av::chunks(rest);
        if !hevc {
            let (asc, packets) = chunks.split_first().expect("a configuration");
            return av::sound_mp4(asc, packets);
        }
        let (params, samples) = chunks.split_at((usize::from(mode >> 2) & 3) + 1);
        av::video_mp4(mode & 2 != 0, params, samples, mode & 0x20 != 0, mode & 0x10 != 0)
    };
    match target {
        "video_frames" => match FrameSource::from_bytes(unmux(true), &Limits::default(), FrameOptions::default()) {
            Ok(mut src) => match src.poster() {
                Ok(f) => format!("{} frames, poster {} × {}", src.frame_count(), f.image.width, f.image.height),
                Err(e) => format!("{} frames, poster: {}", src.frame_count(), e.reason()),
            },
            Err(e) => e.reason(),
        },
        "video_sound" => match SoundSource::from_bytes(unmux(false), &Limits::default()) {
            Ok(mut src) => {
                let n = src.read_all(1 << 20).map_or(0, |s| s.len());
                format!("{} Hz, {} channels, {n} samples, {} damaged", src.rate(), src.channels(), src.damaged())
            }
            Err(m) => m.reason,
        },
        _ => match Music::from_bytes(input.to_vec()) {
            Ok(mut m) => {
                let n = m.read_frames(1 << 20).map_or(0, |s| s.len());
                format!("{} at {} Hz, {} channels, {n} samples", m.kind().name(), m.rate(), m.channels())
            }
            Err(e) => e.reason,
        },
    }
}

fn main() {
    // video_frames: files (H.264, 160 × 96).
    let f = "video_frames";
    write(f, "file-mp4", &with(0, &clip(2, IDENTITY, None, b"avc1")));
    write(f, "file-mp4-turned", &with(0, &clip(2, QUARTER, None, b"avc1")));
    write(f, "file-mkv", &with(0, &mkv(2, false, 0, false)));
    write(f, "file-mkv-unknown-size", &with(0, &mkv(2, false, 0, true)));
    write(f, "file-mp4-threads", &with(0x40, &clip(2, IDENTITY, None, b"avc1")));
    // video_frames: H.264 streams for the muxer, 2 × 2 macroblocks, two parameter sets (bits 2–3 = 1).
    let (s, p) = (sps(2, 2), pps());
    let slices: Vec<Vec<u8>> =
        (0..4).map(|k| slice(k % 2 == 0, k as u64 % 2, 0, 2, 2, &move |x, y| ((x * 3 + y * 5 + k * 40) % 220 + 16) as u8)).collect();
    let mut parts: Vec<&[u8]> = vec![&s, &p];
    parts.extend(slices.iter().map(Vec::as_slice));
    write(f, "h264-stream", &with(0x01 | (1 << 2), &av::join(&parts)));
    write(f, "h264-stream-first-sync", &with(0x01 | (1 << 2) | 0x10, &av::join(&parts)));
    // video_frames: an HEVC stream (bit 1), three parameter sets (bits 2–3 = 2).
    let (v, s, p) = (hevc_vps(), hevc_sps(), hevc_pps());
    let data: Vec<u8> = (0..96u32).map(|i| (i.wrapping_mul(97) ^ 0x5a) as u8).collect();
    let (i1, i2) = (hevc_idr(&data), hevc_idr(&[0x80; 8]));
    write(f, "hevc-stream", &with(0x01 | 0x02 | (2 << 2), &av::join(&[&v, &s, &p, &i1, &i2])));

    // video_sound: files (Matroska with one video frame, in every lacing), and packets for the
    // muxer: AAC-LC mono 48 kHz, stereo 44.1 kHz, HE-AAC (SBR signalled) stereo.
    let f = "video_sound";
    for lacing in 0..4 {
        write(f, &format!("file-mkv-lacing-{lacing}"), &with(0, &mkv(1, true, lacing, false)));
    }
    let silent: Vec<&[u8]> = vec![&SILENT_AAC[..]; 6];
    for (name, asc) in [("aac-lc-mono", &[0x11u8, 0x88][..]), ("aac-lc-stereo", &[0x12, 0x10][..]), ("he-aac", &[0x2b, 0x92, 0x08, 0x00][..])] {
        let mut parts = vec![asc];
        parts.extend(silent.iter().copied());
        write(f, name, &with(0x01, &av::join(&parts)));
    }

    // music.
    let f = "music";
    write(f, "wav", &music_writer::wav(8000, 2, 400));
    write(f, "flac", &music_writer::flac(8000, 2, &[192, 256]));
    write(f, "mp3", &music_writer::mp3(3));
    write(f, "vorbis", &music_writer::vorbis(2048, 2048));

    for target in ["video_frames", "video_sound", "music"] {
        let mut names: Vec<_> = std::fs::read_dir(Path::new("corpus").join(target)).expect("the seeds").flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names {
            let bytes = std::fs::read(&path).expect("a seed");
            println!("{}: {} bytes: {}", path.display(), bytes.len(), outcome(target, &bytes));
        }
    }
}
