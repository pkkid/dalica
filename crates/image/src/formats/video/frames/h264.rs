//! H.264 frames through `altc264` (=0.1.1; `docs/video-decoders.md` §2): the one crate of the
//! record's that decoded every phone file and conformance stream exactly.
//!
//! Dalica's own part is the gate in front of it: every sequence parameter set (from the `avcC`
//! record and in band) is parsed with the crate's own parser before the decoder sees it, and one
//! whose picture size or bit depth is past the frame limits ([`super::Gate`]) refuses the clip
//! with its size, so the decoder never allocates for it. MVC's second view (prefix, subset SPS
//! and extension NAL units) is dropped: Dalica shows the base view. Every call into the crate
//! runs inside `catch_unwind`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use altc264::decoder::{Decoder, Frame};

use super::Gate;
use super::yuv::{Colour, Picture, Plane, Samples};
use crate::sniff::{Format, VideoKind};
use crate::{Error, Result};

/// NAL unit types: SPS, and the MVC ones Dalica drops.
const SPS: u8 = 7;
const MVC: [u8; 3] = [14, 15, 20];

pub struct H264 {
    dec: Decoder,
    length_size: usize,
    threads: usize,
    params: Vec<Vec<u8>>,
    kind: VideoKind,
    rbsp: Vec<u8>,
}

impl H264 {
    /// A decoder for a track whose samples are NAL units prefixed by `length_size` bytes, primed
    /// with the configuration record's parameter sets (each SPS checked by `gate`).
    pub fn new(length_size: usize, params: &[Vec<u8>], threads: usize, kind: VideoKind, gate: &mut Gate) -> Result<H264> {
        let mut h = H264 { dec: decoder(threads), length_size, threads, params: Vec::new(), kind, rbsp: Vec::new() };
        for p in params {
            if h.admit(p, gate)? {
                h.params.push(p.clone());
            }
        }
        h.prime()?;
        Ok(h)
    }

    /// Whether `nal` goes to the decoder; an SPS past the limits is an error.
    fn admit(&mut self, nal: &[u8], gate: &mut Gate) -> Result<bool> {
        let Some(&b0) = nal.first() else { return Ok(false) };
        let kind = b0 & 0x1f;
        if MVC.contains(&kind) {
            return Ok(false);
        }
        if kind == SPS {
            self.rbsp.clear();
            altc264::nal::unescape_into(nal.get(1..).unwrap_or_default(), &mut self.rbsp);
            let sps = guard(self.kind, || altc264::ps::Sps::parse(&self.rbsp))?
                .map_err(|e| Error::Malformed(format!("an H.264 sequence parameter set: {e}")))?;
            let (w, h) = (sps.pic_width_in_mbs.saturating_mul(16), sps.frame_height_in_mbs().saturating_mul(16));
            let [luma, chroma] = sps.bit_depths();
            let depth = luma.max(chroma);
            // 4:0:0, 4:2:0, 4:2:2, 4:4:4: the chroma planes add 0, ½, 1 or 2 luma planes.
            let chroma_halves = match sps.picture_chroma_type() {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            gate.admit(w, h, depth, chroma_halves, self.threads)?;
        }
        Ok(true)
    }

    fn prime(&mut self) -> Result<()> {
        let params = std::mem::take(&mut self.params);
        for p in &params {
            let dec = &mut self.dec;
            guard(self.kind, || dec.decode_nal(p))?;
        }
        self.params = params;
        Ok(())
    }

    /// Feeds one sample (an access unit), shown as `pts`.
    pub fn push(&mut self, sample: &[u8], pts: i64, gate: &mut Gate) -> Result<()> {
        self.dec.set_timestamp(Some(pts));
        let mut rest = sample;
        while rest.len() > self.length_size {
            let (head, tail) = rest.split_at(self.length_size);
            let n = head.iter().fold(0usize, |v, &b| (v << 8) | usize::from(b));
            let Some(nal) = tail.get(..n) else {
                return Err(Error::Malformed("an H.264 NAL unit runs past its sample".into()));
            };
            rest = tail.get(n..).unwrap_or_default();
            if !self.admit(nal, gate)? {
                continue;
            }
            let dec = &mut self.dec;
            guard(self.kind, || dec.decode_nal(nal))?;
        }
        Ok(())
    }

    /// The next frame in display order, if one is ready.
    pub fn pull(&mut self) -> Result<Option<Frame>> {
        let dec = &mut self.dec;
        guard(self.kind, || dec.next_frame())
    }

    /// The end of the stream: every frame still held becomes ready.
    pub fn flush(&mut self) -> Result<()> {
        let dec = &mut self.dec;
        guard(self.kind, || dec.flush())
    }

    /// Drops every picture for a seek; the parameter sets are kept.
    pub fn reset(&mut self) -> Result<()> {
        let dec = &mut self.dec;
        guard(self.kind, || dec.discard())?;
        self.prime()
    }

    /// The most recent error the decoder reported, as text.
    pub fn last_error(&self) -> Option<String> {
        let dec = &self.dec;
        guard(self.kind, || dec.last_error()).ok().flatten().map(|e| e.to_string())
    }
}

fn decoder(threads: usize) -> Decoder {
    let mut d = Decoder::with_threads(threads);
    // The base view only, as a 2D decoder.
    d.decode_all_views = false;
    d
}

/// Runs a call into the crate, its panic caught as [`Error::Panic`].
fn guard<T>(kind: VideoKind, f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| Error::Panic(Format::Video(kind)))
}

/// The planes of a decoded frame (already cropped by the crate).
pub fn picture(f: &Frame) -> Option<Picture<'_>> {
    let plane = |i: usize| {
        let p = f.plane(i)?;
        let samples = if p.data16.is_empty() { Samples::U8(p.data) } else { Samples::U16(p.data16) };
        Some(Plane { samples, stride: p.stride, x0: 0, y0: 0, width: p.width, height: p.height })
    };
    let luma = plane(0)?;
    let chroma = match (f.chroma_width, plane(1), plane(2)) {
        (0, _, _) => None,
        (_, Some(cb), Some(cr)) => Some((cb, cr)),
        _ => None,
    };
    let colour = match f.colour {
        Some((primaries, transfer, matrix)) => Colour { primaries, transfer, matrix, full_range: f.full_range },
        None => Colour { full_range: f.full_range, ..Colour::UNSPECIFIED },
    };
    let bit_depth = u8::try_from(f.bit_depth[0].max(f.bit_depth[1])).unwrap_or(16);
    Some(Picture { luma, chroma, bit_depth, colour })
}
