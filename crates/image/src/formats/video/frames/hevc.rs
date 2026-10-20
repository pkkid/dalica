//! HEVC frames through `rusty_h265` (=0.6.2 with `simd`; `docs/video-decoders.md` §2), only in
//! builds with the `hevc` feature.
//!
//! As for H.264 (`h264.rs`), Dalica's own part is the gate: every SPS (from the `hvcC` record and
//! in band) is parsed with the crate's own parser first, and one past the frame limits refuses
//! the clip before the decoder allocates for it; the colours the SPS signals (its VUI) describe the
//! frames, which the crate does not carry. Layers above the base (Dolby Vision's enhancement,
//! MV-HEVC) are left out by the crate. Every call runs inside `catch_unwind`. A seek starts a new
//! decoder primed with the parameter sets seen so far.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rusty_h265::{Decoder, Frame};

use super::Gate;
use super::yuv::{Colour, Picture, Plane, Samples};
use crate::sniff::{Format, VideoKind};
use crate::{Error, Result};

/// NAL unit types: the parameter sets.
const VPS: u8 = 32;
const SPS: u8 = 33;
const PPS: u8 = 34;

pub struct Hevc {
    dec: Decoder,
    length_size: usize,
    /// The parameter sets seen so far, by type and id order, to prime a new decoder.
    params: Vec<Vec<u8>>,
    colour: Colour,
    kind: VideoKind,
}

impl Hevc {
    pub fn new(length_size: usize, params: &[Vec<u8>], kind: VideoKind, gate: &mut Gate) -> Result<Hevc, Refusal> {
        let mut h = Hevc { dec: Decoder::new(), length_size, params: Vec::new(), colour: Colour::UNSPECIFIED, kind };
        for p in params {
            h.feed(p, None, gate)?;
        }
        Ok(h)
    }

    /// Feeds one NAL unit: a parameter set is remembered (an SPS checked first).
    fn feed(&mut self, nal: &[u8], pts: Option<i64>, gate: &mut Gate) -> Result<(), Refusal> {
        let Some(&b0) = nal.first() else { return Ok(()) };
        let kind = (b0 >> 1) & 0x3f;
        let layer = ((b0 & 1) << 5) | (nal.get(1).copied().unwrap_or(0) >> 3);
        if layer == 0 && kind == SPS {
            let rbsp = guard(self.kind, || rusty_h265::nal::unescape(nal.get(2..).unwrap_or_default()))?;
            let sps = match guard(self.kind, || rusty_h265::ps::parse_sps(&rbsp.data))? {
                Ok(sps) => sps,
                Err(rusty_h265::Error::Unsupported(what)) => return Err(Refusal::Unsupported(what)),
                Err(e) => return Err(Refusal::Error(Error::Malformed(format!("an HEVC sequence parameter set: {e}")))),
            };
            let chroma_halves = match sps.chroma_format_idc {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            gate.admit(sps.width, sps.height, u32::from(sps.bit_depth_luma.max(sps.bit_depth_chroma)), chroma_halves, 0)?;
            if let Some(v) = &sps.vui {
                self.colour = Colour {
                    primaries: v.colour_primaries,
                    transfer: v.transfer_characteristics,
                    matrix: v.matrix_coeffs,
                    full_range: v.video_full_range_flag,
                };
            } else {
                self.colour = Colour::UNSPECIFIED;
            }
        }
        if layer == 0 && matches!(kind, VPS | SPS | PPS) && !self.params.iter().any(|p| p == nal) {
            if self.params.len() >= 64 {
                self.params.remove(0);
            }
            self.params.push(nal.to_vec());
        }
        let dec = &mut self.dec;
        match guard(self.kind, || dec.push_nal(nal, pts))? {
            Ok(()) => Ok(()),
            Err(rusty_h265::Error::Unsupported(what)) => Err(Refusal::Unsupported(what)),
            // A damaged NAL unit: the decoder stays usable and conceals what it lost.
            Err(_) => Ok(()),
        }
    }

    /// Feeds one sample (an access unit), shown as `pts`.
    pub fn push(&mut self, sample: &[u8], pts: i64, gate: &mut Gate) -> Result<(), Refusal> {
        let mut rest = sample;
        let mut first = true;
        while rest.len() > self.length_size {
            let (head, tail) = rest.split_at(self.length_size);
            let n = head.iter().fold(0usize, |v, &b| (v << 8) | usize::from(b));
            let Some(nal) = tail.get(..n) else {
                return Err(Refusal::Error(Error::Malformed("an HEVC NAL unit runs past its sample".into())));
            };
            rest = tail.get(n..).unwrap_or_default();
            self.feed(nal, first.then_some(pts), gate)?;
            first = false;
        }
        Ok(())
    }

    pub fn pull(&mut self) -> Result<Option<Frame>> {
        let dec = &mut self.dec;
        guard(self.kind, || dec.next_frame().ok())
    }

    pub fn flush(&mut self) -> Result<()> {
        let dec = &mut self.dec;
        guard(self.kind, || dec.flush())
    }

    /// A new decoder for a seek, primed with the parameter sets seen so far.
    pub fn reset(&mut self, gate: &mut Gate) -> Result<(), Refusal> {
        self.dec = Decoder::new();
        for p in std::mem::take(&mut self.params) {
            self.feed(&p, None, gate)?;
        }
        Ok(())
    }

    pub fn colour(&self) -> Colour {
        self.colour
    }
}

/// Why a feed failed: a stream the crate does not decode, or an error.
pub enum Refusal {
    Unsupported(String),
    Error(Error),
}

impl From<Error> for Refusal {
    fn from(e: Error) -> Refusal {
        Refusal::Error(e)
    }
}

fn guard<T>(kind: VideoKind, f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| Error::Panic(Format::Video(kind)))
}

/// The cropped planes of a decoded frame.
pub fn picture(f: &Frame, colour: Colour) -> Option<Picture<'_>> {
    let p = &f.picture;
    let (cx, cy, cw, ch) = p.crop;
    let (sw, sh) = match p.chroma_format_idc {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let plane = |i: usize, sub_w: usize, sub_h: usize| {
        let q = p.planes.get(i)?;
        Some(Plane {
            samples: Samples::U16(&q.data),
            stride: q.stride,
            x0: cx / sub_w,
            y0: cy / sub_h,
            width: cw.div_ceil(sub_w),
            height: ch.div_ceil(sub_h),
        })
    };
    let luma = plane(0, 1, 1)?;
    let chroma = if p.chroma_format_idc == 0 { None } else { Some((plane(1, sw, sh)?, plane(2, sw, sh)?)) };
    Some(Picture { luma, chroma, bit_depth: p.bit_depth_luma.max(p.bit_depth_chroma), colour })
}
