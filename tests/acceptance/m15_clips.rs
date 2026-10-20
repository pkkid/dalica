//! The clips M15's acceptance plays (`m15.rs`): H.264 made here, every macroblock I_PCM (raw
//! samples, ITU-T H.264 §7.3.5; CAVLC, Baseline, POC type 2), muxed by hand into MP4 and MOV
//! (ISO/IEC 14496-12 boxes; QuickTime's `qt  ` brand) with silent AAC-LC sound, and into Matroska
//! (RFC 9559). The writer is `dalica-image`'s `tests/video_frames.rs` one, copied (a test file of
//! another crate cannot be called from here; each crate's tests keep a copy of their own too).
//!
//! Every clip is 2 s of 160 × 96 at 25 frames a second, an IDR every 10 frames: frame `k` is one
//! grey ([`luma`]) with a white top-left macroblock, black (luma 16) for its first [`BLACK`]
//! frames, so its poster is frame [`BLACK`] and a saved frame tells which frame it was.

/// Frames a second, frames in a clip, the clip's size in macroblocks (160 × 96).
pub const FPS: usize = 25;
pub const FRAMES: usize = 50;
pub const MBS: (usize, usize) = (10, 6);
pub const SIZE: (u32, u32) = ((MBS.0 * 16) as u32, (MBS.1 * 16) as u32);
/// A clip's length in milliseconds.
pub const DURATION_MS: u64 = (FRAMES * 1000 / FPS) as u64;
/// Frames before the picture lights up: the poster is the first frame after them.
pub const BLACK: usize = 5;
/// Recorded 2026-10-19 12:00:00 UTC (noon, so the day is the same in every time zone a test runs
/// in), in seconds since 1904 (ISO/IEC 14496-12 `mvhd`).
const RECORDED: u32 = 3_875_256_000;

/// The luma of frame `k` of a clip whose lit frames start at `base` (at most 88, so the last frame
/// stays under 235).
pub fn luma(base: u8, k: usize) -> u8 {
    if k < BLACK { 16 } else { base + 3 * k as u8 }
}

/// The 8-bit grey a decoder shows for `luma` (studio range, BT.601: `(y - 16) × 255 / 219`).
pub fn shown(luma: u8) -> u8 {
    ((f64::from(luma) - 16.0) / 219.0 * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The frame of a clip of `base` a decoded grey belongs to (within 2 of what it shows).
pub fn frame_of(base: u8, grey: u8) -> Option<usize> {
    (BLACK..FRAMES).find(|&k| (i32::from(shown(luma(base, k))) - i32::from(grey)).abs() <= 2)
}

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

fn sps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 30).ue(0); // Baseline, level 3, SPS 0
    b.ue(0).ue(2).ue(1).u(1, 0); // log2_max_frame_num 4, POC type 2, one reference frame, no gaps
    b.ue(MBS.0 as u64 - 1).ue(MBS.1 as u64 - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0); // size, frames only, 8x8 inference, no crop, no VUI
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0); // CAVLC, one slice group
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0); // QP, deblocking control present
    nal(3, 8, &b.trailing())
}

/// Frame `k` as an I slice of I_PCM macroblocks, length-prefixed (4 bytes) as MP4 and Matroska
/// carry it.
fn frame(base: u8, k: usize) -> Vec<u8> {
    let idr = k.is_multiple_of(10);
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, (k % 10) as u64); // first MB, I slice, PPS 0, frame_num
    if idr {
        b.ue((k / 10 % 2) as u64).u(1, 0).u(1, 0); // idr_pic_id, no_output_of_prior_pics, long_term_reference
    } else {
        b.u(1, 0); // adaptive_ref_pic_marking_mode_flag
    }
    b.se(0).ue(1); // slice_qp_delta, deblocking off
    for my in 0..MBS.1 {
        for mx in 0..MBS.0 {
            b.ue(25).align_zero(); // I_PCM
            for _y in 0..16 {
                for _x in 0..16 {
                    b.u(8, u64::from(if mx == 0 && my == 0 { 235 } else { luma(base, k) }));
                }
            }
            for _ in 0..128 {
                b.u(8, 128);
            }
        }
    }
    let s = nal(if idr { 3 } else { 2 }, if idr { 5 } else { 1 }, &b.trailing());
    let mut sample = (s.len() as u32).to_be_bytes().to_vec();
    sample.extend(s);
    sample
}

fn avcc() -> Vec<u8> {
    let (sps, pps) = (sps(), pps());
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    avcc
}

/// A silent AAC-LC packet (ISO/IEC 14496-3 §4.4.2): one single-channel element with no
/// scale-factor bands, then the end element.
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

fn sound_packets() -> usize {
    (FRAMES as u64 * 48_000 / FPS as u64).div_ceil(1024) as usize
}

// --- MP4 and MOV ------------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0, 0, 0, 0];
    p.extend_from_slice(payload);
    bx(kind, &p)
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

struct Track {
    id: u32,
    handler: [u8; 4],
    entry: Vec<u8>,
    timescale: u32,
    /// Each sample's duration.
    duration: u32,
    syncs: Option<Vec<u32>>,
    size: (u32, u32),
    samples: Vec<Vec<u8>>,
}

fn trak(t: &Track, at: &mut u32) -> Vec<u8> {
    let total = t.duration * t.samples.len() as u32;
    let mut tkhd = [be(RECORDED), be(RECORDED), be(t.id), be(0), be(total * 1000 / t.timescale)].concat();
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(t.size.0 << 16));
    tkhd.extend(be(t.size.1 << 16));
    let mut mdhd = [be(RECORDED), be(RECORDED), be(t.timescale), be(total)].concat();
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(&t.entry);
    let stts = [be(1), be(t.samples.len() as u32), be(t.duration)].concat();
    let mut stsz = [be(0), be(t.samples.len() as u32)].concat();
    let mut stco = be(t.samples.len() as u32).to_vec();
    for s in &t.samples {
        stsz.extend(be(s.len() as u32));
        stco.extend(be(*at));
        *at += s.len() as u32;
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let mut stbl = [full(b"stsd", &stsd), full(b"stts", &stts), full(b"stsc", &stsc), full(b"stsz", &stsz), full(b"stco", &stco)].concat();
    if let Some(syncs) = &t.syncs {
        let mut stss = be(syncs.len() as u32).to_vec();
        for &s in syncs {
            stss.extend(be(s));
        }
        stbl.extend(full(b"stss", &stss));
    }
    let minf = bx(b"minf", &bx(b"stbl", &stbl));
    let mdia = bx(b"mdia", &[full(b"mdhd", &mdhd), full(b"hdlr", &hdlr), minf].concat());
    bx(b"trak", &[full(b"tkhd", &tkhd), mdia].concat())
}

/// A visual sample entry of `code` holding the H.264 configuration (`avc1`; `mp4v` for a codec
/// Dalica has no decoder for, MPEG-4 Part 2, with the same body).
fn avc1(code: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend((SIZE.0 as u16).to_be_bytes());
    p.extend((SIZE.1 as u16).to_be_bytes());
    p.extend(be(0x48_0000));
    p.extend(be(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    p.extend(bx(b"avcC", &avcc()));
    bx(code, &p)
}

/// An `mp4a` entry, AAC-LC mono at 48 kHz (`AudioSpecificConfig` 0x11 0x88).
fn mp4a() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(b"mp4a", &p)
}

/// An ISO BMFF clip of brand `brand` (`isom` for MP4, `qt  ` for MOV) whose video is `code`
/// (`avc1`, or `mp4v` for one Dalica cannot play), lit from `base`, with silent sound.
fn bmff(brand: &[u8; 4], code: &[u8; 4], base: u8) -> Vec<u8> {
    let tracks = [
        Track {
            id: 1,
            handler: *b"vide",
            entry: avc1(code),
            timescale: 1000,
            duration: (1000 / FPS) as u32,
            syncs: Some((0..FRAMES as u32).filter(|k| k.is_multiple_of(10)).map(|k| k + 1).collect()),
            size: SIZE,
            samples: (0..FRAMES).map(|k| frame(base, k)).collect(),
        },
        Track {
            id: 2,
            handler: *b"soun",
            entry: mp4a(),
            timescale: 48_000,
            duration: 1024,
            syncs: None,
            size: (0, 0),
            samples: vec![SILENT_AAC.to_vec(); sound_packets()],
        },
    ];
    let compatible: &[u8] = if brand == b"qt  " { b"qt  " } else { b"isomavc1mp41" };
    let mut out = bx(b"ftyp", &[&brand[..], &be(0x200), compatible].concat());
    let data: Vec<u8> = tracks.iter().flat_map(|t| t.samples.concat()).collect();
    let mut at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &data));
    let traks: Vec<u8> = tracks.iter().flat_map(|t| trak(t, &mut at)).collect();
    let mut mvhd = [be(RECORDED), be(RECORDED), be(1000), be(DURATION_MS as u32), be(0x1_0000)].concat();
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    out.extend(bx(b"moov", &[full(b"mvhd", &mvhd), traks].concat()));
    out
}

/// An MP4 of H.264 and AAC lit from `base`.
pub fn mp4(base: u8) -> Vec<u8> {
    bmff(b"isom", b"avc1", base)
}

/// A QuickTime movie of H.264 and AAC lit from `base`.
pub fn mov(base: u8) -> Vec<u8> {
    bmff(b"qt  ", b"avc1", base)
}

/// A QuickTime movie whose video is MPEG-4 Part 2, which Dalica has no decoder for.
pub fn mpeg4_part2() -> Vec<u8> {
    bmff(b"qt  ", b"mp4v", 40)
}

/// An MP4 whose video is HEVC (`hvc1`, the H.264 body under its code): a codec Dalica decodes only
/// in a build with the `hevc` feature, which the default build leaves out.
pub fn hevc() -> Vec<u8> {
    bmff(b"isom", b"hvc1", 40)
}

/// A hostile container: an MP4 whose `moov` claims far more than the file holds, cut off.
pub fn hostile() -> Vec<u8> {
    let mut out = mp4(40);
    let moov = out.windows(4).rposition(|w| w == b"moov").unwrap() - 4;
    out[moov..moov + 4].copy_from_slice(&be(0x7fff_fff0));
    out.truncate(moov + 64);
    out
}

// --- Matroska ---------------------------------------------------------------------------------

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

fn el_float(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// A Matroska clip of H.264 lit from `base`, and silent AAC, in clusters of one second.
pub fn mkv(base: u8) -> Vec<u8> {
    let video = [el_uint(0xb0, u64::from(SIZE.0)), el_uint(0xba, u64::from(SIZE.1))].concat();
    let mut tracks = el(0xae, &[el_uint(0xd7, 1), el_uint(0x83, 1), el(0x86, b"V_MPEG4/ISO/AVC"), el(0x63a2, &avcc()), el(0xe0, &video)].concat());
    tracks.extend(el(0xae, &[el_uint(0xd7, 2), el_uint(0x83, 2), el(0x86, b"A_AAC"), el(0x63a2, &[0x11, 0x88])].concat()));
    // Info: TimecodeScale 1 ms, Duration in those units.
    let info = [el_uint(0x2a_d7b1, 1_000_000), el_float(0x4489, DURATION_MS as f64)].concat();
    let mut segment = [el(0x1549_a966, &info), el(0x1654_ae6b, &tracks)].concat();
    let packets = sound_packets();
    let block = |track: u8, rel: i16, key: bool, data: &[u8]| {
        let mut b = vec![0x80 | track];
        b.extend(rel.to_be_bytes());
        b.push(if key { 0x80 } else { 0 });
        b.extend(data);
        b
    };
    let mut packet = 0;
    for second in 0..FRAMES.div_ceil(FPS) {
        let mut cluster = el_uint(0xe7, second as u64 * 1000);
        for k in second * FPS..((second + 1) * FPS).min(FRAMES) {
            let rel = ((k - second * FPS) * 1000 / FPS) as i16;
            cluster.extend(el(0xa3, &block(1, rel, k.is_multiple_of(10), &frame(base, k))));
            while packet < packets && (packet as u64 * 1024 * 1000 / 48_000) < ((k + 1) * 1000 / FPS) as u64 {
                let t = (packet as u64 * 1024 * 1000 / 48_000) as i64 - second as i64 * 1000;
                cluster.extend(el(0xa3, &block(2, t as i16, true, &SILENT_AAC)));
                packet += 1;
            }
        }
        segment.extend(el(0x1f43_b675, &cluster));
    }
    let header = el(0x1a45_dfa3, &[el(0x4282, b"matroska"), el_uint(0x4287, 4)].concat());
    [header, el(0x1853_8067, &segment)].concat()
}
