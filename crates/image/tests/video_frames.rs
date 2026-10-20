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

// ---------------------------------------------------------------------------------------------
// An H.264 writer: every macroblock I_PCM (raw samples), CAVLC, Baseline, POC type 2.

#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                (self.acc, self.n) = (0, 0);
            }
        }
        self
    }
    fn ue(&mut self, v: u64) -> &mut Bits {
        let v1 = v + 1;
        let len = 64 - v1.leading_zeros();
        self.u(len - 1, 0).u(len, v1)
    }
    fn se(&mut self, v: i64) -> &mut Bits {
        self.ue(if v > 0 { (2 * v - 1) as u64 } else { (-2 * v) as u64 })
    }
    fn align_zero(&mut self) -> &mut Bits {
        while self.n != 0 {
            self.u(1, 0);
        }
        self
    }
    fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        self.align_zero();
        self.out
    }
}

/// A NAL unit with its header and emulation prevention.
fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![(ref_idc << 5) | kind];
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    out
}

fn sps(mbs_w: u64, mbs_h: u64) -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 30).ue(0); // Baseline, level 3, SPS 0
    b.ue(0).ue(2).ue(1).u(1, 0); // log2_max_frame_num 4, POC type 2, one reference frame, no gaps
    b.ue(mbs_w - 1).ue(mbs_h - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0); // size, frames only, 8x8 inference, no crop, no VUI
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0); // CAVLC, one slice group
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0); // QP, deblocking control present
    nal(3, 8, &b.trailing())
}

/// One picture as an I slice of I_PCM macroblocks; `luma(x, y)` gives each sample.
fn slice(idr: bool, frame_num: u64, idr_id: u64, mbs_w: usize, mbs_h: usize, luma: &dyn Fn(usize, usize) -> u8) -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, frame_num); // first MB, I slice, PPS 0, frame_num
    if idr {
        b.ue(idr_id).u(1, 0).u(1, 0); // idr_pic_id, no_output_of_prior_pics, long_term_reference
    } else {
        b.u(1, 0); // adaptive_ref_pic_marking_mode_flag
    }
    b.se(0).ue(1); // slice_qp_delta, deblocking off
    for my in 0..mbs_h {
        for mx in 0..mbs_w {
            b.ue(25).align_zero(); // I_PCM
            for y in 0..16 {
                for x in 0..16 {
                    b.u(8, u64::from(luma(mx * 16 + x, my * 16 + y)));
                }
            }
            for _ in 0..128 {
                b.u(8, 128);
            }
        }
    }
    nal(if idr { 3 } else { 2 }, if idr { 5 } else { 1 }, &b.trailing())
}

// ---------------------------------------------------------------------------------------------
// An MP4 writer.

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0, 0, 0, 0];
    p.extend_from_slice(payload);
    bx(kind, &p)
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];
const QUARTER: [i32; 9] = [0, 0x1_0000, 0, -0x1_0000, 0, 0, 0, 0, 0x4000_0000];

struct TrackSpec {
    handler: [u8; 4],
    entry: Vec<u8>,
    timescale: u32,
    /// (size, duration) per sample, in decode order; sync flags (`None`: all).
    samples: Vec<(u32, u32)>,
    syncs: Option<Vec<u32>>,
    matrix: [i32; 9],
    size: (u32, u32),
}

fn trak(t: &TrackSpec, offsets: &[u32]) -> Vec<u8> {
    let mut tkhd = vec![0u8; 20];
    tkhd.extend([0u8; 16]);
    tkhd.extend(t.matrix.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be32(t.size.0 << 16));
    tkhd.extend(be32(t.size.1 << 16));
    let duration: u32 = t.samples.iter().map(|s| s.1).sum();
    let mut mdhd = vec![0u8; 8];
    mdhd.extend(be32(t.timescale));
    mdhd.extend(be32(duration));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be32(1).to_vec();
    stsd.extend(&t.entry);
    let mut stts = be32(t.samples.len() as u32).to_vec();
    for s in &t.samples {
        stts.extend(be32(1));
        stts.extend(be32(s.1));
    }
    let mut stsz = be32(0).to_vec();
    stsz.extend(be32(t.samples.len() as u32));
    for s in &t.samples {
        stsz.extend(be32(s.0));
    }
    let mut stsc = be32(1).to_vec();
    stsc.extend(be32(1));
    stsc.extend(be32(1));
    stsc.extend(be32(1));
    let mut stco = be32(offsets.len() as u32).to_vec();
    for &o in offsets {
        stco.extend(be32(o));
    }
    let mut stbl =
        [full_box(b"stsd", &stsd), full_box(b"stts", &stts), full_box(b"stsc", &stsc), full_box(b"stsz", &stsz), full_box(b"stco", &stco)].concat();
    if let Some(syncs) = &t.syncs {
        let mut stss = be32(syncs.len() as u32).to_vec();
        for &s in syncs {
            stss.extend(be32(s));
        }
        stbl.extend(full_box(b"stss", &stss));
    }
    let minf = bx(b"minf", &bx(b"stbl", &stbl));
    let mdia = bx(b"mdia", &[full_box(b"mdhd", &mdhd), full_box(b"hdlr", &hdlr), minf].concat());
    bx(b"trak", &[full_box(b"tkhd", &tkhd), mdia].concat())
}

/// `ftyp`, `mdat` (every track's samples, one chunk each), then `moov`.
fn mp4(tracks: &[(TrackSpec, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut out = bx(b"ftyp", &[&b"isom"[..], &be32(0x200), b"isom", b"avc1", b"mp41"].concat());
    let data: Vec<u8> = tracks.iter().flat_map(|(_, s)| s.concat()).collect();
    let mdat_at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &data));
    let mut at = mdat_at;
    let mut traks = Vec::new();
    for (spec, samples) in tracks {
        let offsets: Vec<u32> = samples
            .iter()
            .map(|s| {
                let o = at;
                at += s.len() as u32;
                o
            })
            .collect();
        traks.push(trak(spec, &offsets));
    }
    let longest =
        tracks.iter().map(|(t, _)| u64::from(t.samples.iter().map(|s| s.1).sum::<u32>()) * 1000 / u64::from(t.timescale)).max().unwrap_or(0);
    let mut mvhd = vec![0u8; 8];
    mvhd.extend(be32(1000));
    mvhd.extend(be32(longest as u32));
    mvhd.extend(be32(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be32(3));
    out.extend(bx(b"moov", &[full_box(b"mvhd", &mvhd), traks.concat()].concat()));
    out
}

fn avc1(code: &[u8; 4], w: u16, h: u16, sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend(w.to_be_bytes());
    p.extend(h.to_be_bytes());
    p.extend(be32(0x48_0000));
    p.extend(be32(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(pps);
    p.extend(bx(b"avcC", &avcc));
    bx(code, &p)
}

/// An `mp4a` entry, AAC-LC mono at 48 kHz (`AudioSpecificConfig` 0x11 0x88), or another code.
fn sound_entry(code: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be32(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full_box(b"esds", &esds));
    bx(code, &p)
}

/// A silent AAC-LC packet (ISO/IEC 14496-3 §4.4.2): one single-channel element with no
/// scale-factor bands (global gain 0, long window, max_sfb 0, no pulse, TNS or gain control),
/// then the end element.
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

/// Frames a second, and the clip's frame size in macroblocks (160 × 96).
const FPS: u32 = 25;
const MBS: (usize, usize) = (10, 6);
/// Frames before the picture lights up.
const BLACK: usize = 5;

/// The luma of frame `k` (one grey per frame), and a white top-left macroblock in every frame.
fn grey(k: usize) -> u8 {
    if k < BLACK { 16 } else { 40 + 3 * k as u8 }
}

/// A clip of `frames` frames at 25 fps (an IDR every 10 frames, I slices between), with
/// `matrix`, and sound for as long when `sound` names its sample entry code.
fn clip(frames: usize, matrix: [i32; 9], sound: Option<&[u8; 4]>, video_code: &[u8; 4]) -> Vec<u8> {
    let (sps, pps) = (sps(MBS.0 as u64, MBS.1 as u64), pps());
    let mut samples = Vec::new();
    for k in 0..frames {
        let idr = k % 10 == 0;
        let luma = move |x: usize, y: usize| if x < 16 && y < 16 { 235 } else { grey(k) };
        let s = slice(idr, (k % 10) as u64, (k / 10 % 2) as u64, MBS.0, MBS.1, &luma);
        let mut sample = be32(s.len() as u32).to_vec();
        sample.extend(s);
        samples.push(sample);
    }
    let (w, h) = ((MBS.0 * 16) as u16, (MBS.1 * 16) as u16);
    let video = TrackSpec {
        handler: *b"vide",
        entry: avc1(video_code, w, h, &sps, &pps),
        timescale: 1000,
        samples: samples.iter().map(|s| (s.len() as u32, 1000 / FPS)).collect(),
        syncs: Some((0..frames as u32).filter(|k| k % 10 == 0).map(|k| k + 1).collect()),
        matrix,
        size: (u32::from(w), u32::from(h)),
    };
    let mut tracks = vec![(video, samples)];
    if let Some(code) = sound {
        let packets = (frames as u64 * 48_000 / u64::from(FPS)).div_ceil(1024) as usize;
        let s = vec![SILENT_AAC.to_vec(); packets];
        let spec = TrackSpec {
            handler: *b"soun",
            entry: sound_entry(code),
            timescale: 48_000,
            samples: vec![(4, 1024); packets],
            syncs: None,
            matrix: IDENTITY,
            size: (0, 0),
        };
        tracks.push((spec, s));
    }
    mp4(&tracks)
}

// ---------------------------------------------------------------------------------------------
// A Matroska writer (RFC 9559): the same frames and packets in clusters of one second.

fn ebml_id(id: u32) -> Vec<u8> {
    id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect()
}

fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = ebml_id(id);
    v.push(0x01); // an 8-byte size
    v.extend((data.len() as u64).to_be_bytes().iter().skip(1));
    v.extend_from_slice(data);
    v
}

fn el_uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// A clip of `frames` frames at 25 fps and, with `sound`, silent AAC laced `lacing` (0 none, 1 Xiph,
/// 2 fixed, 3 EBML) four packets to a block. Clusters of unknown size when `unknown`.
fn mkv(frames: usize, sound: bool, lacing: u8, unknown: bool) -> Vec<u8> {
    let (sps, pps) = (sps(MBS.0 as u64, MBS.1 as u64), pps());
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    let video = [el_uint(0xb0, 160), el_uint(0xba, 96)].concat();
    let mut tracks = [el_uint(0xd7, 1), el_uint(0x83, 1), el(0x86, b"V_MPEG4/ISO/AVC"), el(0x63a2, &avcc), el(0xe0, &video)].concat();
    tracks = el(0xae, &tracks);
    if sound {
        tracks.extend(el(0xae, &[el_uint(0xd7, 2), el_uint(0x83, 2), el(0x86, b"A_AAC"), el(0x63a2, &[0x11, 0x88])].concat()));
    }
    let mut segment = [el(0x1549_a966, &el_uint(0x2a_d7b1, 1_000_000)), el(0x1654_ae6b, &tracks)].concat();
    let packets = (frames as u64 * 48_000 / u64::from(FPS)).div_ceil(1024) as usize;
    let block = |track: u8, rel: i16, key: bool, lace: u8, frames: &[Vec<u8>]| {
        let mut b = vec![0x80 | track];
        b.extend(rel.to_be_bytes());
        b.push(if key { 0x80 } else { 0 } | (lace << 1));
        if lace != 0 {
            b.push(frames.len() as u8 - 1);
            let sizes: Vec<usize> = frames.iter().map(Vec::len).collect();
            match lace {
                1 => {
                    for &n in &sizes[..sizes.len() - 1] {
                        b.extend(std::iter::repeat_n(255u8, n / 255));
                        b.push((n % 255) as u8);
                    }
                }
                3 => {
                    b.extend([0x40 | (sizes[0] >> 8) as u8, sizes[0] as u8]);
                    for w in sizes[..sizes.len() - 1].windows(2) {
                        let diff = w[1] as i64 - w[0] as i64 + 63;
                        b.push(0x80 | diff as u8);
                    }
                }
                _ => {}
            }
        }
        for f in frames {
            b.extend(f);
        }
        b
    };
    let mut packet = 0;
    for second in 0..frames.div_ceil(FPS as usize) {
        let mut cluster = el_uint(0xe7, second as u64 * 1000);
        for k in second * FPS as usize..((second + 1) * FPS as usize).min(frames) {
            let luma = move |x: usize, y: usize| if x < 16 && y < 16 { 235 } else { grey(k) };
            let s = slice(k % 10 == 0, (k % 10) as u64, (k / 10 % 2) as u64, MBS.0, MBS.1, &luma);
            let mut sample = be32(s.len() as u32).to_vec();
            sample.extend(s);
            let rel = (k - second * FPS as usize) as i16 * 40;
            cluster.extend(el(0xa3, &block(1, rel, k % 10 == 0, 0, &[sample])));
            if sound {
                while packet < packets && (packet as u64 * 1024 * 1000 / 48_000) < ((k + 1) as u64 * 40) {
                    let n = if lacing == 0 { 1 } else { 4.min(packets - packet) };
                    let t = (packet as u64 * 1024 * 1000 / 48_000) as i64 - second as i64 * 1000;
                    let lace = if n == 1 { 0 } else { lacing };
                    cluster.extend(el(0xa3, &block(2, t as i16, true, lace, &vec![SILENT_AAC.to_vec(); n])));
                    packet += n;
                }
            }
        }
        if unknown {
            let mut c = ebml_id(0x1f43_b675);
            c.push(0xff);
            c.extend(cluster);
            segment.extend(c);
        } else {
            segment.extend(el(0x1f43_b675, &cluster));
        }
    }
    let header = el(0x1a45_dfa3, &[el(0x4282, b"matroska"), el_uint(0x4287, 4)].concat());
    [header, el(0x1853_8067, &segment)].concat()
}

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
