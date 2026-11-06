//! A lossy WebP's VP8 key frame (RFC 6386), decoded macroblock row by macroblock row and handed
//! out as RGB rows the moment they are final, so the memory it holds is a few macroblock rows of
//! the frame's width, never the frame.
//!
//! Adapted from `image-webp` 0.2.4 `src/vp8.rs` and `src/yuv.rs` (github.com/image-rs/image-webp,
//! MIT OR Apache-2.0; `NOTICE`): the frame header, the macroblock headers, the coefficient tokens,
//! intra prediction, the loop filter and the "fancy" chroma upsampling are its code, and the
//! pixels come out identical to its full decode (a unit test compares them). What changed:
//!
//! - **Rows, not a frame.** It decodes every macroblock into whole-frame Y, U and V planes and
//!   then filters the whole frame. Here each plane is a ring of [`Y_RING`] / [`C_RING`] rows: a
//!   macroblock row is predicted into it, filtered at once (the filter reads 4 rows above it and
//!   changes 3; prediction reads only the unfiltered borders kept apart, as there, so filtering a
//!   row early changes nothing), and every row no later filter or upsampling step can change is
//!   converted to RGB and handed to the caller.
//! - **Bounded reads.** Partitions are slices of the file, every bool read past their end is
//!   noticed ([`BoolDecoder::past_end`]) and ends the decode with [`Error::Truncated`]; every
//!   buffer is charged to the [`Budget`] and taken fallibly; nothing indexes by a value read from
//!   the file without `get`.

mod bool_decoder;
mod filter;
mod predict;
mod tables;

use bool_decoder::BoolDecoder;
use predict::{CHROMA_STRIDE, IntraMode, LUMA_STRIDE, block};
use tables::*;

use crate::limits::Budget;
use crate::{Error, Result};

/// Luma rows kept: the macroblock row being decoded (16), the 4 above it the filter reads, and
/// the rows the upsampling still waits on.
const Y_RING: usize = 32;
/// Chroma rows kept, likewise.
const C_RING: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum LumaMode {
    #[default]
    DC,
    V,
    H,
    TM,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum ChromaMode {
    #[default]
    DC,
    V,
    H,
    TM,
}

/// What a macroblock's neighbours need of it: modes and the non-zero flags of its blocks'
/// edges, and what the loop filter needs.
#[derive(Clone, Copy, Default)]
struct Macroblock {
    bpred: [IntraMode; 16],
    complexity: [u8; 9],
    luma: LumaMode,
    chroma: ChromaMode,
    segment: u8,
    skipped: bool,
    non_zero_dct: bool,
}

#[derive(Clone, Copy, Default)]
struct Segment {
    ydc: i16,
    yac: i16,
    y2dc: i16,
    y2ac: i16,
    uvdc: i16,
    uvac: i16,
    delta_values: bool,
    quantizer_level: i8,
    loopfilter_level: i8,
}

fn malformed(what: &str) -> Error {
    Error::Malformed(format!("WebP: {what}"))
}

/// A frame's width and height from its first 10 bytes.
pub(crate) fn frame_size(data: &[u8]) -> Result<(u32, u32)> {
    let Some(&[tag, _, _, s0, s1, s2, w0, w1, h0, h1]) = data.get(..10) else {
        return Err(Error::Truncated);
    };
    if tag & 1 != 0 {
        return Err(malformed("the VP8 frame is not a key frame"));
    }
    if [s0, s1, s2] != [0x9d, 0x01, 0x2a] {
        return Err(malformed("no VP8 start code"));
    }
    let w = u32::from(u16::from_le_bytes([w0, w1]) & 0x3fff);
    let h = u32::from(u16::from_le_bytes([h0, h1]) & 0x3fff);
    if w == 0 || h == 0 {
        return Err(malformed("an empty VP8 frame"));
    }
    Ok((w, h))
}

/// Decodes the key frame in `data` (a `VP8 ` chunk's payload), calling `row(y, rgb)` for each
/// row of the picture in order, top to bottom, `rgb` holding its `width × 3` bytes.
pub(crate) fn decode_rows(data: &[u8], budget: &mut Budget, row: &mut dyn FnMut(usize, &[u8]) -> Result<()>) -> Result<()> {
    let (width, height) = frame_size(data)?;
    let mut d = Decoder::new(data, width as usize, height as usize, budget)?;
    d.read_frame_header()?;
    d.decode(row)
}

struct Decoder<'a> {
    width: usize,
    height: usize,
    mbw: usize,
    mbh: usize,
    /// The first partition: frame header and macroblock headers.
    b: BoolDecoder<'a>,
    /// The rest of the data, split into partitions by `read_frame_header`.
    rest: &'a [u8],
    partitions: Vec<BoolDecoder<'a>>,

    segments_enabled: bool,
    segments_update_map: bool,
    segment: [Segment; 4],
    segment_probs: [Prob; 3],
    filter_simple: bool,
    filter_level: u8,
    sharpness: u8,
    lf_adjustments: bool,
    ref_delta: [i32; 4],
    mode_delta: [i32; 4],
    token_probs: Box<TokenProbs>,
    prob_skip_false: Option<Prob>,

    top: Vec<Macroblock>,
    left: Macroblock,
    /// This row's macroblocks, for the loop filter.
    row_mbs: Vec<Macroblock>,
    // The unfiltered borders prediction reads (§12); the left ones start with the corner pixel.
    top_y: Vec<u8>,
    top_u: Vec<u8>,
    top_v: Vec<u8>,
    left_y: [u8; 17],
    left_u: [u8; 9],
    left_v: [u8; 9],

    /// The planes' rings: row `r` of the frame is ring row `r % Y_RING` (or `C_RING`).
    ring_y: Vec<u8>,
    ring_u: Vec<u8>,
    ring_v: Vec<u8>,
    /// Ring strides: whole macroblocks.
    stride_y: usize,
    stride_c: usize,
    rgb: Vec<u8>,
    /// One row's chroma, blended down the rows ([`Decoder::convert_row`]).
    blend_u: Vec<u16>,
    blend_v: Vec<u16>,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8], width: usize, height: usize, budget: &mut Budget) -> Result<Decoder<'a>> {
        let Some(&[t0, t1, t2]) = data.get(..3) else {
            return Err(Error::Truncated);
        };
        let first_size = u32::from_le_bytes([t0, t1, t2, 0]) >> 5;
        let first_end = 10usize.checked_add(first_size as usize).ok_or(Error::OutOfRange)?;
        let first = data.get(10..first_end).ok_or(Error::Truncated)?;
        let rest = data.get(first_end..).unwrap_or_default();
        let (mbw, mbh) = (width.div_ceil(16), height.div_ceil(16));
        let stride_y = mbw * 16;
        let stride_c = mbw * 8;
        Ok(Decoder {
            width,
            height,
            mbw,
            mbh,
            b: BoolDecoder::new(first),
            rest,
            partitions: Vec::new(),
            segments_enabled: false,
            segments_update_map: false,
            segment: [Segment::default(); 4],
            segment_probs: [255; 3],
            filter_simple: false,
            filter_level: 0,
            sharpness: 0,
            lf_adjustments: false,
            ref_delta: [0; 4],
            mode_delta: [0; 4],
            token_probs: Box::new(COEFF_PROBS),
            prob_skip_false: None,
            top: budget.vec(mbw, Macroblock::default())?,
            left: Macroblock::default(),
            row_mbs: budget.vec(mbw, Macroblock::default())?,
            top_y: budget.vec(stride_y + 20, 127u8)?,
            top_u: budget.vec(stride_c, 127u8)?,
            top_v: budget.vec(stride_c, 127u8)?,
            left_y: [129; 17],
            left_u: [129; 9],
            left_v: [129; 9],
            ring_y: budget.vec(stride_y * Y_RING, 0u8)?,
            ring_u: budget.vec(stride_c * C_RING, 0u8)?,
            ring_v: budget.vec(stride_c * C_RING, 0u8)?,
            stride_y,
            stride_c,
            rgb: budget.vec(width * 3, 0u8)?,
            blend_u: budget.vec(width.div_ceil(2), 0u16)?,
            blend_v: budget.vec(width.div_ceil(2), 0u16)?,
        })
    }

    /// Fails with [`Error::Truncated`] once any partition has been read past its end.
    fn check(&self) -> Result<()> {
        if self.b.past_end() || self.partitions.iter().any(BoolDecoder::past_end) {
            return Err(Error::Truncated);
        }
        Ok(())
    }

    // ---- Frame header (§9) -------------------------------------------------------------------

    fn read_frame_header(&mut self) -> Result<()> {
        let color_space = self.b.read_literal(1);
        let _pixel_type = self.b.read_literal(1);
        if color_space != 0 {
            return Err(malformed("a VP8 colour space other than 0"));
        }
        self.segments_enabled = self.b.read_flag();
        if self.segments_enabled {
            self.read_segment_updates();
        }
        self.filter_simple = self.b.read_flag();
        self.filter_level = self.b.read_literal(6);
        self.sharpness = self.b.read_literal(3);
        self.lf_adjustments = self.b.read_flag();
        if self.lf_adjustments && self.b.read_flag() {
            for i in 0..4 {
                self.ref_delta[i] = self.b.read_optional_signed(6);
            }
            for i in 0..4 {
                self.mode_delta[i] = self.b.read_optional_signed(6);
            }
        }
        let n = 1usize << self.b.read_literal(2);
        self.check()?;
        self.init_partitions(n)?;
        self.read_quantization();
        // The "refresh entropy probabilities" flag means nothing in a single key frame.
        let _ = self.b.read_literal(1);
        self.update_token_probabilities();
        self.prob_skip_false = if self.b.read_literal(1) == 1 { Some(self.b.read_literal(8)) } else { None };
        self.check()
    }

    fn read_segment_updates(&mut self) {
        self.segments_update_map = self.b.read_flag();
        if self.b.read_flag() {
            let absolute = self.b.read_flag();
            for s in &mut self.segment {
                s.delta_values = !absolute;
            }
            for i in 0..4 {
                self.segment[i].quantizer_level = self.b.read_optional_signed(7) as i8;
            }
            for i in 0..4 {
                self.segment[i].loopfilter_level = self.b.read_optional_signed(6) as i8;
            }
        }
        if self.segments_update_map {
            for i in 0..3 {
                self.segment_probs[i] = if self.b.read_flag() { self.b.read_literal(8) } else { 255 };
            }
        }
    }

    /// The DCT partitions after the first: `n - 1` 3-byte sizes, the partitions, the last taking
    /// the rest (§9.5).
    fn init_partitions(&mut self, n: usize) -> Result<()> {
        let sizes_len = 3 * (n - 1);
        let sizes = self.rest.get(..sizes_len).ok_or(Error::Truncated)?;
        let mut pos = sizes_len;
        for &[a, b, c] in sizes.as_chunks::<3>().0 {
            let size = usize::from(a) | usize::from(b) << 8 | usize::from(c) << 16;
            let end = pos.checked_add(size).ok_or(Error::OutOfRange)?;
            self.partitions.push(BoolDecoder::new(self.rest.get(pos..end).ok_or(Error::Truncated)?));
            pos = end;
        }
        self.partitions.push(BoolDecoder::new(self.rest.get(pos..).unwrap_or_default()));
        Ok(())
    }

    fn read_quantization(&mut self) {
        let dc = |i: i32| DC_QUANT[i.clamp(0, 127) as usize];
        let ac = |i: i32| AC_QUANT[i.clamp(0, 127) as usize];
        let yac_abs = i32::from(self.b.read_literal(7));
        let ydc_delta = self.b.read_optional_signed(4);
        let y2dc_delta = self.b.read_optional_signed(4);
        let y2ac_delta = self.b.read_optional_signed(4);
        let uvdc_delta = self.b.read_optional_signed(4);
        let uvac_delta = self.b.read_optional_signed(4);
        let n = if self.segments_enabled { 4 } else { 1 };
        for s in self.segment.iter_mut().take(n) {
            let base = if !self.segments_enabled {
                yac_abs
            } else if s.delta_values {
                i32::from(s.quantizer_level) + yac_abs
            } else {
                i32::from(s.quantizer_level)
            };
            s.ydc = dc(base + ydc_delta);
            s.yac = ac(base);
            s.y2dc = dc(base + y2dc_delta) * 2;
            s.y2ac = ((i32::from(ac(base + y2ac_delta)) * 155 / 100) as i16).max(8);
            s.uvdc = dc(base + uvdc_delta).min(132);
            s.uvac = ac(base + uvac_delta);
        }
    }

    fn update_token_probabilities(&mut self) {
        for (i, is) in COEFF_UPDATE_PROBS.iter().enumerate() {
            for (j, js) in is.iter().enumerate() {
                for (k, ks) in js.iter().enumerate() {
                    for (t, &prob) in ks.iter().enumerate() {
                        if self.b.read_bool(prob) {
                            self.token_probs[i][j][k][t] = self.b.read_literal(8);
                        }
                    }
                }
            }
        }
    }

    // ---- Macroblocks (§10, §11, §13) --------------------------------------------------------

    fn read_macroblock_header(&mut self, mbx: usize) -> Result<Macroblock> {
        let mut mb = Macroblock::default();
        if self.segments_enabled && self.segments_update_map {
            mb.segment = self.b.read_tree(&SEGMENT_TREE, &self.segment_probs, 0) as u8;
        }
        mb.skipped = self.prob_skip_false.is_some_and(|p| self.b.read_bool(p));
        mb.luma = match self.b.read_tree(&YMODE_TREE, &YMODE_PROBS, 0) {
            0 => LumaMode::DC,
            1 => LumaMode::V,
            2 => LumaMode::H,
            3 => LumaMode::TM,
            _ => LumaMode::B,
        };
        let top = self.top.get_mut(mbx).ok_or(Error::OutOfRange)?;
        match mb.luma {
            LumaMode::B => {
                for y in 0usize..4 {
                    for x in 0usize..4 {
                        let probs = &KEYFRAME_BPRED_MODE_PROBS[top.bpred[12 + x] as usize][self.left.bpred[y] as usize];
                        let v = self.b.read_tree(&BMODE_TREE, probs, 0);
                        let mode = IntraMode::from_i8(v).ok_or_else(|| malformed("a subblock mode out of range"))?;
                        mb.bpred[x + y * 4] = mode;
                        top.bpred[12 + x] = mode;
                        self.left.bpred[y] = mode;
                    }
                }
            }
            luma => {
                let mode = match luma {
                    LumaMode::V => IntraMode::VE,
                    LumaMode::H => IntraMode::HE,
                    LumaMode::TM => IntraMode::TM,
                    _ => IntraMode::DC,
                };
                for i in 0usize..4 {
                    mb.bpred[12 + i] = mode;
                    self.left.bpred[i] = mode;
                }
            }
        }
        mb.chroma = match self.b.read_tree(&UV_MODE_TREE, &UV_MODE_PROBS, 0) {
            0 => ChromaMode::DC,
            1 => ChromaMode::V,
            2 => ChromaMode::H,
            _ => ChromaMode::TM,
        };
        top.chroma = mb.chroma;
        top.luma = mb.luma;
        top.bpred = mb.bpred;
        Ok(mb)
    }

    /// One block's tokens (§13.2), dequantised into `block` in natural order. Returns whether any
    /// token other than the end of block was read.
    fn read_coefficients(&mut self, block: &mut [i32; 16], p: usize, plane: usize, ctx: usize, dcq: i16, acq: i16) -> Result<bool> {
        let first = usize::from(plane == 0);
        let probs = self.token_probs.get(plane).ok_or(Error::OutOfRange)?;
        let d = self.partitions.get_mut(p).ok_or(Error::OutOfRange)?;
        let mut ctx = ctx.min(2);
        let mut has = false;
        let mut skip = false;
        for i in first..16usize {
            let tree_probs = &probs[usize::from(COEFF_BANDS[i])][ctx];
            let token = d.read_tree(&TOKEN_TREE, tree_probs, if skip { 2 } else { 0 });
            let mut v = match token {
                11 => break,
                0 => {
                    skip = true;
                    has = true;
                    ctx = 0;
                    continue;
                }
                1..=4 => i32::from(token),
                5..=10 => {
                    let cat = (token - 5) as usize;
                    let mut extra = 0i32;
                    for &t in PROB_DCT_CAT[cat].iter().take_while(|&&t| t != 0) {
                        extra = 2 * extra + i32::from(d.read_bool(t));
                    }
                    i32::from(DCT_CAT_BASE[cat]) + extra
                }
                _ => return Err(malformed("a coefficient token out of range")),
            };
            skip = false;
            ctx = if v == 1 { 1 } else { 2 };
            if d.read_flag() {
                v = -v;
            }
            let zz = usize::from(ZIGZAG[i]);
            block[zz] = v * i32::from(if zz > 0 { acq } else { dcq });
            has = true;
        }
        if d.past_end() {
            return Err(Error::Truncated);
        }
        Ok(has)
    }

    /// A macroblock's 25 blocks of residue (§13, §14): Y2 if the luma mode has one, 16 Y, 4 U,
    /// 4 V, each inverse-transformed, in 24 blocks of 16.
    fn read_residual_data(&mut self, mb: &mut Macroblock, mbx: usize, p: usize) -> Result<[i32; 384]> {
        let s = *self.segment.get(usize::from(mb.segment)).ok_or(Error::OutOfRange)?;
        let mut blocks = [0i32; 384];
        let mut plane = if mb.luma == LumaMode::B { 3 } else { 1 };
        let top = |d: &Self, i: usize| d.top.get(mbx).map_or(0, |t| t.complexity[i]);
        let set_top = |d: &mut Self, i: usize, v: u8| {
            if let Some(t) = d.top.get_mut(mbx) {
                t.complexity[i] = v;
            }
        };
        if plane == 1 {
            let ctx = usize::from(top(self, 0) + self.left.complexity[0]);
            let mut block = [0i32; 16];
            let n = self.read_coefficients(&mut block, p, plane, ctx, s.y2dc, s.y2ac)?;
            self.left.complexity[0] = u8::from(n);
            set_top(self, 0, u8::from(n));
            predict::iwht4x4(&mut block);
            for (k, &v) in block.iter().enumerate() {
                blocks[16 * k] = v;
            }
            plane = 0;
        }
        for y in 0usize..4 {
            let mut left = self.left.complexity[y + 1];
            for x in 0usize..4 {
                let i = x + y * 4;
                let mut b = block(&blocks, i);
                let ctx = usize::from(top(self, x + 1) + left);
                let n = self.read_coefficients(&mut b, p, plane, ctx, s.ydc, s.yac)?;
                if b[0] != 0 || n {
                    mb.non_zero_dct = true;
                    predict::idct4x4(&mut b);
                }
                blocks[i * 16..i * 16 + 16].copy_from_slice(&b);
                left = u8::from(n);
                set_top(self, x + 1, u8::from(n));
            }
            self.left.complexity[y + 1] = left;
        }
        for j in [5usize, 7] {
            for y in 0usize..2 {
                let mut left = self.left.complexity[y + j];
                for x in 0usize..2 {
                    let i = x + y * 2 + if j == 5 { 16 } else { 20 };
                    let mut b = [0i32; 16];
                    let ctx = usize::from(top(self, x + j) + left);
                    let n = self.read_coefficients(&mut b, p, 2, ctx, s.uvdc, s.uvac)?;
                    if b[0] != 0 || n {
                        mb.non_zero_dct = true;
                        predict::idct4x4(&mut b);
                    }
                    blocks[i * 16..i * 16 + 16].copy_from_slice(&b);
                    left = u8::from(n);
                    set_top(self, x + j, u8::from(n));
                }
                self.left.complexity[y + j] = left;
            }
        }
        Ok(blocks)
    }

    fn predict_luma(&mut self, mbx: usize, mby: usize, mb: &Macroblock, res: &[i32; 384]) {
        let stride = LUMA_STRIDE;
        let mut ws = predict::border_luma(mbx, mby, self.mbw, &self.top_y, &self.left_y);
        match mb.luma {
            LumaMode::V => predict::predict_vpred(&mut ws, 16, 1, 1, stride),
            LumaMode::H => predict::predict_hpred(&mut ws, 16, 1, 1, stride),
            LumaMode::TM => predict::predict_tmpred(&mut ws, 16, 1, 1, stride),
            LumaMode::DC => predict::predict_dcpred(&mut ws, 16, stride, mby != 0, mbx != 0),
            LumaMode::B => predict::predict_4x4(&mut ws, &mb.bpred, res),
        }
        if mb.luma != LumaMode::B {
            for y in 0usize..4 {
                for x in 0usize..4 {
                    predict::add_residue(&mut ws, &block(res, x + y * 4), 1 + y * 4, 1 + x * 4, stride);
                }
            }
        }
        self.left_y[0] = ws[16];
        for (i, l) in self.left_y[1..].iter_mut().enumerate() {
            *l = ws[(i + 1) * stride + 16];
        }
        if let Some(top) = self.top_y.get_mut(mbx * 16..mbx * 16 + 16) {
            top.copy_from_slice(&ws[16 * stride + 1..16 * stride + 17]);
        }
        let base = (mby * 16) % Y_RING;
        for y in 0usize..16 {
            let at = (base + y) * self.stride_y + mbx * 16;
            if let Some(dst) = self.ring_y.get_mut(at..at + 16) {
                dst.copy_from_slice(&ws[(1 + y) * stride + 1..(1 + y) * stride + 17]);
            }
        }
    }

    fn predict_chroma(&mut self, mbx: usize, mby: usize, mb: &Macroblock, res: &[i32; 384]) {
        let stride = CHROMA_STRIDE;
        let base = (mby * 8) % C_RING;
        for (first, top, left, ring) in
            [(16usize, &mut self.top_u, &mut self.left_u, &mut self.ring_u), (20, &mut self.top_v, &mut self.left_v, &mut self.ring_v)]
        {
            let mut ws = predict::border_chroma(mbx, mby, top, left);
            match mb.chroma {
                ChromaMode::DC => predict::predict_dcpred(&mut ws, 8, stride, mby != 0, mbx != 0),
                ChromaMode::V => predict::predict_vpred(&mut ws, 8, 1, 1, stride),
                ChromaMode::H => predict::predict_hpred(&mut ws, 8, 1, 1, stride),
                ChromaMode::TM => predict::predict_tmpred(&mut ws, 8, 1, 1, stride),
            }
            for y in 0usize..2 {
                for x in 0usize..2 {
                    predict::add_residue(&mut ws, &block(res, first + x + y * 2), 1 + y * 4, 1 + x * 4, stride);
                }
            }
            left[0] = ws[8];
            for (i, l) in left[1..].iter_mut().enumerate() {
                *l = ws[(i + 1) * stride + 8];
            }
            if let Some(t) = top.get_mut(mbx * 8..mbx * 8 + 8) {
                t.copy_from_slice(&ws[8 * stride + 1..8 * stride + 9]);
            }
            for y in 0usize..8 {
                let at = (base + y) * self.stride_c + mbx * 8;
                if let Some(dst) = ring.get_mut(at..at + 8) {
                    dst.copy_from_slice(&ws[(1 + y) * stride + 1..(1 + y) * stride + 9]);
                }
            }
        }
    }

    // ---- Loop filter (§15) -------------------------------------------------------------------

    /// The filter level, interior limit and high edge variance threshold of `mb` (§15.1, §15.2).
    fn filter_parameters(&self, mb: &Macroblock) -> (u8, u8, u8) {
        let mut level = i32::from(self.filter_level);
        if level == 0 {
            return (0, 0, 0);
        }
        if self.segments_enabled {
            let s = self.segment.get(usize::from(mb.segment)).copied().unwrap_or_default();
            if s.delta_values {
                level += i32::from(s.loopfilter_level);
            } else {
                level = i32::from(s.loopfilter_level);
            }
        }
        level = level.clamp(0, 63);
        if self.lf_adjustments {
            level += self.ref_delta[0];
            if mb.luma == LumaMode::B {
                level += self.mode_delta[0];
            }
        }
        let level = level.clamp(0, 63) as u8;
        let mut interior = level;
        if self.sharpness > 0 {
            interior >>= if self.sharpness > 4 { 2 } else { 1 };
            interior = interior.min(9u8.saturating_sub(self.sharpness));
        }
        let interior = interior.max(1);
        let hev = if level >= 40 {
            2
        } else if level >= 15 {
            1
        } else {
            0
        };
        (level, interior, hev)
    }

    /// Filters macroblock `mbx` of row `mby`, in place in the rings, in the order of §15.
    fn loop_filter(&mut self, mbx: usize, mby: usize, mb: &Macroblock) {
        let (level, interior, hev) = self.filter_parameters(mb);
        if level == 0 {
            return;
        }
        let mbedge = (level + 2) * 2 + interior;
        let subedge = level * 2 + interior;
        let inner = mb.luma == LumaMode::B || (!mb.skipped && mb.non_zero_dct);
        let (sy, sc) = (self.stride_y, self.stride_c);
        let (y0, c0) = (mby * 16, mby * 8);
        let (x0, cx0) = (mbx * 16, mbx * 8);
        let simple = self.filter_simple;
        let mb_f = move |px: &mut [u8; 8]| {
            if simple { filter::simple(mbedge, px) } else { filter::macroblock(hev, interior, mbedge, px) }
        };
        let sub_f = move |px: &mut [u8; 8]| {
            if simple { filter::simple(subedge, px) } else { filter::subblock(hev, interior, subedge, px) }
        };
        // Across the left edge, then the inner vertical edges.
        if mbx > 0 {
            across_columns(&mut self.ring_y, sy, Y_RING, y0, 16, x0, mb_f);
            if !simple {
                across_columns(&mut self.ring_u, sc, C_RING, c0, 8, cx0, mb_f);
                across_columns(&mut self.ring_v, sc, C_RING, c0, 8, cx0, mb_f);
            }
        }
        if inner {
            for x in [4usize, 8, 12] {
                across_columns(&mut self.ring_y, sy, Y_RING, y0, 16, x0 + x, sub_f);
            }
            if !simple {
                across_columns(&mut self.ring_u, sc, C_RING, c0, 8, cx0 + 4, sub_f);
                across_columns(&mut self.ring_v, sc, C_RING, c0, 8, cx0 + 4, sub_f);
            }
        }
        // Across the top edge, then the inner horizontal edges.
        if mby > 0 {
            across_rows(&mut self.ring_y, sy, Y_RING, y0, x0, 16, mb_f);
            if !simple {
                across_rows(&mut self.ring_u, sc, C_RING, c0, cx0, 8, mb_f);
                across_rows(&mut self.ring_v, sc, C_RING, c0, cx0, 8, mb_f);
            }
        }
        if inner {
            for y in [4usize, 8, 12] {
                across_rows(&mut self.ring_y, sy, Y_RING, y0 + y, x0, 16, sub_f);
            }
            if !simple {
                across_rows(&mut self.ring_u, sc, C_RING, c0 + 4, cx0, 8, sub_f);
                across_rows(&mut self.ring_v, sc, C_RING, c0 + 4, cx0, 8, sub_f);
            }
        }
    }

    // ---- The frame --------------------------------------------------------------------------

    fn decode(&mut self, row: &mut dyn FnMut(usize, &[u8]) -> Result<()>) -> Result<()> {
        let n = self.partitions.len().max(1);
        let chroma_height = self.height.div_ceil(2);
        let mut next_row = 0usize;
        for mby in 0..self.mbh {
            let p = mby % n;
            self.left = Macroblock::default();
            self.left_y = [129; 17];
            self.left_u = [129; 9];
            self.left_v = [129; 9];
            for mbx in 0..self.mbw {
                let mut mb = self.read_macroblock_header(mbx)?;
                let blocks = if mb.skipped {
                    if mb.luma != LumaMode::B {
                        self.left.complexity[0] = 0;
                        if let Some(t) = self.top.get_mut(mbx) {
                            t.complexity[0] = 0;
                        }
                    }
                    for i in 1usize..9 {
                        self.left.complexity[i] = 0;
                        if let Some(t) = self.top.get_mut(mbx) {
                            t.complexity[i] = 0;
                        }
                    }
                    [0i32; 384]
                } else {
                    self.read_residual_data(&mut mb, mbx, p)?
                };
                self.check()?;
                self.predict_luma(mbx, mby, &mb, &blocks);
                self.predict_chroma(mbx, mby, &mb, &blocks);
                if let Some(slot) = self.row_mbs.get_mut(mbx) {
                    *slot = mb;
                }
            }
            for mbx in 0..self.mbw {
                let mb = self.row_mbs.get(mbx).copied().unwrap_or_default();
                self.loop_filter(mbx, mby, &mb);
            }
            // What no later filter can change: the next row's top edge changes the last 3 rows
            // of this one (luma and chroma).
            let last = mby + 1 == self.mbh;
            let (y_final, c_final) =
                if last { (self.height, chroma_height) } else { ((mby * 16 + 13).min(self.height), (mby * 8 + 5).min(chroma_height)) };
            while next_row < y_final && chroma_needed(next_row, chroma_height) < c_final {
                self.convert_row(next_row, chroma_height);
                row(next_row, &self.rgb)?;
                next_row += 1;
            }
        }
        Ok(())
    }

    /// Luma row `y` and the chroma rows around it to RGB in `self.rgb`, with the bilinear
    /// ("fancy") chroma upsampling of libwebp: each chroma sample weighs 9, its neighbours across
    /// and along 3, the diagonal 1; the edges repeat.
    fn convert_row(&mut self, y: usize, chroma_height: usize) {
        let (main, other) = chroma_rows(y, chroma_height);
        let w = self.width;
        let cw = w.div_ceil(2);
        let ry = (y % Y_RING) * self.stride_y;
        let (rm, ro) = ((main % C_RING) * self.stride_c, (other % C_RING) * self.stride_c);
        let luma = self.ring_y.get(ry..ry + w).unwrap_or_default();
        let um = self.ring_u.get(rm..rm + cw).unwrap_or_default();
        let uo = self.ring_u.get(ro..ro + cw).unwrap_or_default();
        let vm = self.ring_v.get(rm..rm + cw).unwrap_or_default();
        let vo = self.ring_v.get(ro..ro + cw).unwrap_or_default();
        // Each chroma column blended down the rows first (3 × nearest + the other, × 4), so a
        // pixel is (3 × its column + the neighbouring column + 8) / 16.
        for (t, (m, o)) in self.blend_u.iter_mut().zip(um.iter().zip(uo)) {
            *t = 3 * u16::from(*m) + u16::from(*o);
        }
        for (t, (m, o)) in self.blend_v.iter_mut().zip(vm.iter().zip(vo)) {
            *t = 3 * u16::from(*m) + u16::from(*o);
        }
        let (bu, bv) = (&self.blend_u[..cw.min(self.blend_u.len())], &self.blend_v[..cw.min(self.blend_v.len())]);
        let mix = |near: u16, far: u16| ((3 * near + far + 8) / 16) as u8;
        let (Some((first, rest)), Some((&y0, ys)), Some(&u0), Some(&v0)) =
            (self.rgb.split_first_chunk_mut::<3>(), luma.split_first(), bu.first(), bv.first())
        else {
            return;
        };
        *first = yuv_to_rgb(y0, mix(u0, u0), mix(v0, v0));
        // From x = 1 on, pixels pair up between chroma columns c and c + 1: the first nearer c,
        // the second nearer c + 1.
        let (outs, out_rest) = rest.as_chunks_mut::<6>();
        let (lumas, luma_rest) = ys.as_chunks::<2>();
        for (((out, &[y1, y2]), u), v) in outs.iter_mut().zip(lumas).zip(bu.windows(2)).zip(bv.windows(2)) {
            let (a, b) = out.split_at_mut(3);
            a.copy_from_slice(&yuv_to_rgb(y1, mix(u[0], u[1]), mix(v[0], v[1])));
            b.copy_from_slice(&yuv_to_rgb(y2, mix(u[1], u[0]), mix(v[1], v[0])));
        }
        // An even width ends on one pixel past the last pair, on the last chroma column.
        if let (Some(out), [yv], Some(&u), Some(&v)) = (out_rest.first_chunk_mut::<3>(), luma_rest, bu.last(), bv.last()) {
            *out = yuv_to_rgb(*yv, mix(u, u), mix(v, v));
        }
    }
}

/// The highest chroma row luma row `y` reads.
fn chroma_needed(y: usize, chroma_height: usize) -> usize {
    let (main, other) = chroma_rows(y, chroma_height);
    main.max(other)
}

/// The chroma row luma row `y` sits nearest, and the one beside it on its side (the same row at
/// the top and bottom edges).
fn chroma_rows(y: usize, chroma_height: usize) -> (usize, usize) {
    let last = chroma_height.saturating_sub(1);
    if y == 0 {
        (0, 0)
    } else if !y.is_multiple_of(2) {
        let main = (y - 1) / 2;
        let other = y.div_ceil(2);
        if other > last { (main, main) } else { (main, other) }
    } else {
        (y / 2, y / 2 - 1)
    }
}

/// Filters across the vertical edge at column `x` for `n` rows from frame row `y0`: each row's
/// 8 pixels around the edge.
fn across_columns(ring: &mut [u8], stride: usize, rows: usize, y0: usize, n: usize, x: usize, f: impl Fn(&mut [u8; 8])) {
    let Some(start) = x.checked_sub(4) else { return };
    for y in y0..y0 + n {
        let at = (y % rows) * stride + start;
        if let Some(px) = ring.get_mut(at..at + 8).and_then(|s| <&mut [u8; 8]>::try_from(s).ok()) {
            f(px);
        }
    }
}

/// Filters across the horizontal edge above frame row `y` for `n` (at most 16) columns from
/// `x0`: the 8 rows around the edge are copied out, filtered column by column and copied back.
fn across_rows(ring: &mut [u8], stride: usize, rows: usize, y: usize, x0: usize, n: usize, f: impl Fn(&mut [u8; 8])) {
    let Some(top) = y.checked_sub(4) else { return };
    let n = n.min(16);
    let starts: [usize; 8] = std::array::from_fn(|k| ((top + k) % rows) * stride + x0);
    let mut block = [[0u8; 16]; 8];
    for (b, &s) in block.iter_mut().zip(&starts) {
        let Some(src) = ring.get(s..s + n) else { return };
        b[..n].copy_from_slice(src);
    }
    for x in 0..n {
        let mut px: [u8; 8] = std::array::from_fn(|k| block[k][x]);
        f(&mut px);
        for (b, p) in block.iter_mut().zip(px) {
            b[x] = p;
        }
    }
    for (b, &s) in block.iter().zip(&starts) {
        if let Some(dst) = ring.get_mut(s..s + n) {
            dst.copy_from_slice(&b[..n]);
        }
    }
}

/// libwebp's integer BT.601 conversion (`src/dsp/yuv.h`), as `image-webp` 0.2.4 `src/yuv.rs`.
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let mulhi = |v: u8, coeff: u16| ((u32::from(v) * u32::from(coeff)) >> 8) as i32;
    let clip = |v: i32| (v >> 6).clamp(0, 255) as u8;
    let yy = mulhi(y, 19077);
    [clip(yy + mulhi(v, 26149) - 14234), clip(yy - mulhi(u, 6419) - mulhi(v, 13320) + 8708), clip(yy + mulhi(u, 33050) - 17685)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    #[test]
    fn chroma_rows_follow_the_upsampling() {
        // 5 rows: chroma rows 0, 1, 2.
        let rows: Vec<_> = (0..5).map(|y| chroma_rows(y, 3)).collect();
        assert_eq!(rows, [(0, 0), (0, 1), (1, 0), (1, 2), (2, 1)]);
        // 4 rows: the last one has no chroma row below.
        let rows: Vec<_> = (0..4).map(|y| chroma_rows(y, 2)).collect();
        assert_eq!(rows, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn a_frame_without_its_partitions_is_cut_short() {
        // A key frame header declaring a 1 KiB first partition, with 10 bytes of it.
        let mut data = vec![0x10, 0x80, 0, 0x9d, 0x01, 0x2a, 64, 0, 48, 0];
        data.extend([0u8; 10]);
        let mut budget = Budget::new(&Limits::default(), 64, 48);
        assert_eq!(decode_rows(&data, &mut budget, &mut |_, _| Ok(())), Err(Error::Truncated));
    }
}
