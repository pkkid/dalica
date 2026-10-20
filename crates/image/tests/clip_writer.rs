//! Clips made for the tests and the fuzz seeds (`crates/image/fuzz`, which compiles this file in
//! with `#[path]`): an H.264 writer of our own that writes every macroblock as I_PCM (ITU-T H.264
//! §7.3.5), an MP4 writer and a Matroska writer, with silent AAC-LC packets for the sound.

#![allow(dead_code)]
// ---------------------------------------------------------------------------------------------
// An H.264 writer: every macroblock I_PCM (raw samples), CAVLC, Baseline, POC type 2.

#[derive(Default)]
pub struct Bits {
    pub out: Vec<u8>,
    pub acc: u64,
    pub n: u32,
}

impl Bits {
    pub fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
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
    pub fn ue(&mut self, v: u64) -> &mut Bits {
        let v1 = v + 1;
        let len = 64 - v1.leading_zeros();
        self.u(len - 1, 0).u(len, v1)
    }
    pub fn se(&mut self, v: i64) -> &mut Bits {
        self.ue(if v > 0 { (2 * v - 1) as u64 } else { (-2 * v) as u64 })
    }
    pub fn align_zero(&mut self) -> &mut Bits {
        while self.n != 0 {
            self.u(1, 0);
        }
        self
    }
    pub fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        self.align_zero();
        self.out
    }
}

/// A NAL unit with its header and emulation prevention.
pub fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
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

pub fn sps(mbs_w: u64, mbs_h: u64) -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 30).ue(0); // Baseline, level 3, SPS 0
    b.ue(0).ue(2).ue(1).u(1, 0); // log2_max_frame_num 4, POC type 2, one reference frame, no gaps
    b.ue(mbs_w - 1).ue(mbs_h - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0); // size, frames only, 8x8 inference, no crop, no VUI
    nal(3, 7, &b.trailing())
}

pub fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0); // CAVLC, one slice group
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0); // QP, deblocking control present
    nal(3, 8, &b.trailing())
}

/// One picture as an I slice of I_PCM macroblocks; `luma(x, y)` gives each sample.
pub fn slice(idr: bool, frame_num: u64, idr_id: u64, mbs_w: usize, mbs_h: usize, luma: &dyn Fn(usize, usize) -> u8) -> Vec<u8> {
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

pub fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

pub fn full_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0, 0, 0, 0];
    p.extend_from_slice(payload);
    bx(kind, &p)
}

pub fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

pub const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];
pub const QUARTER: [i32; 9] = [0, 0x1_0000, 0, -0x1_0000, 0, 0, 0, 0, 0x4000_0000];

pub struct TrackSpec {
    pub handler: [u8; 4],
    pub entry: Vec<u8>,
    pub timescale: u32,
    /// (size, duration) per sample, in decode order; sync flags (`None`: all).
    pub samples: Vec<(u32, u32)>,
    pub syncs: Option<Vec<u32>>,
    pub matrix: [i32; 9],
    pub size: (u32, u32),
}

pub fn trak(t: &TrackSpec, offsets: &[u32]) -> Vec<u8> {
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
pub fn mp4(tracks: &[(TrackSpec, Vec<Vec<u8>>)]) -> Vec<u8> {
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

pub fn avc1(code: &[u8; 4], w: u16, h: u16, sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(pps);
    visual_entry(code, w, h, &bx(b"avcC", &avcc))
}

/// A visual sample entry of `w` × `h` carrying `config` (an `avcC` or `hvcC` box).
pub fn visual_entry(code: &[u8; 4], w: u16, h: u16, config: &[u8]) -> Vec<u8> {
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
    p.extend(config);
    bx(code, &p)
}

/// An `mp4a` entry, AAC-LC mono at 48 kHz (`AudioSpecificConfig` 0x11 0x88), or another code.
pub fn sound_entry(code: &[u8; 4]) -> Vec<u8> {
    sound_entry_with(code, &[0x11, 0x88])
}

/// A sound sample entry whose `esds` carries the `AudioSpecificConfig` `asc` (at most 100 bytes).
pub fn sound_entry_with(code: &[u8; 4], asc: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be32(48_000 << 16));
    let n = asc.len() as u8;
    let mut esds = vec![3, 3 + 2 + 15 + n + 3, 0, 1, 0, 4, 15 + n, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, n];
    esds.extend(asc);
    esds.extend([6, 1, 2]);
    p.extend(full_box(b"esds", &esds));
    bx(code, &p)
}

/// A silent AAC-LC packet (ISO/IEC 14496-3 §4.4.2): one single-channel element with no
/// scale-factor bands (global gain 0, long window, max_sfb 0, no pulse, TNS or gain control),
/// then the end element.
pub const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

/// Frames a second, and the clip's frame size in macroblocks (160 × 96).
pub const FPS: u32 = 25;
pub const MBS: (usize, usize) = (10, 6);
/// Frames before the picture lights up.
pub const BLACK: usize = 5;

/// The luma of frame `k` (one grey per frame), and a white top-left macroblock in every frame.
pub fn grey(k: usize) -> u8 {
    if k < BLACK { 16 } else { 40 + 3 * k as u8 }
}

/// A clip of `frames` frames at 25 fps (an IDR every 10 frames, I slices between), with
/// `matrix`, and sound for as long when `sound` names its sample entry code.
pub fn clip(frames: usize, matrix: [i32; 9], sound: Option<&[u8; 4]>, video_code: &[u8; 4]) -> Vec<u8> {
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

pub fn ebml_id(id: u32) -> Vec<u8> {
    id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect()
}

pub fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = ebml_id(id);
    v.push(0x01); // an 8-byte size
    v.extend((data.len() as u64).to_be_bytes().iter().skip(1));
    v.extend_from_slice(data);
    v
}

pub fn el_uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// A clip of `frames` frames at 25 fps and, with `sound`, silent AAC laced `lacing` (0 none, 1 Xiph,
/// 2 fixed, 3 EBML) four packets to a block. Clusters of unknown size when `unknown`.
pub fn mkv(frames: usize, sound: bool, lacing: u8, unknown: bool) -> Vec<u8> {
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
