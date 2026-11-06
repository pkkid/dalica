//! Intra prediction (RFC 6386 §12) into a macroblock's work area: the 16 × 16 luma block or an
//! 8 × 8 chroma block, with the row above (and, for luma, 4 more to the right) and the column to
//! the left around it. Every index is a constant offset into the fixed-size work area.
//!
//! Adapted from `image-webp` 0.2.4 `src/vp8.rs` (github.com/image-rs/image-webp, MIT OR
//! Apache-2.0; `NOTICE`): `create_border_luma`, `create_border_chroma`, `add_residue` and the
//! `predict_*` functions, unchanged but for the residue's saturating add and visibility.

#![allow(clippy::manual_clamp)]

/// The luma work area: 1 + 16 + 4 columns, 1 + 16 rows.
pub(super) const LUMA_STRIDE: usize = 1 + 16 + 4;
pub(super) const LUMA_AREA: usize = (1 + 16) * LUMA_STRIDE;
/// The chroma work area: 1 + 8 columns, 1 + 8 rows.
pub(super) const CHROMA_STRIDE: usize = 1 + 8;
pub(super) const CHROMA_AREA: usize = (1 + 8) * CHROMA_STRIDE;

/// A subblock's prediction mode (§12.3), in the coded order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum IntraMode {
    #[default]
    DC = 0,
    TM = 1,
    VE = 2,
    HE = 3,
    LD = 4,
    RD = 5,
    VR = 6,
    VL = 7,
    HD = 8,
    HU = 9,
}

impl IntraMode {
    pub(super) const fn from_i8(v: i8) -> Option<IntraMode> {
        Some(match v {
            0 => IntraMode::DC,
            1 => IntraMode::TM,
            2 => IntraMode::VE,
            3 => IntraMode::HE,
            4 => IntraMode::LD,
            5 => IntraMode::RD,
            6 => IntraMode::VR,
            7 => IntraMode::VL,
            8 => IntraMode::HD,
            9 => IntraMode::HU,
            _ => return None,
        })
    }
}

/// The luma work area of macroblock `mbx`, `mby` of a frame `mbw` macroblocks wide, from the
/// unfiltered row above (`top`, the frame's width plus slack) and column to the left (`left`,
/// the corner first).
pub(super) fn border_luma(mbx: usize, mby: usize, mbw: usize, top: &[u8], left: &[u8; 17]) -> [u8; LUMA_AREA] {
    let stride = LUMA_STRIDE;
    let mut ws = [0u8; LUMA_AREA];
    {
        let above = &mut ws[1..stride];
        if mby == 0 {
            above.fill(127);
        } else {
            let from = top.get(mbx * 16..).unwrap_or_default();
            for (a, &t) in above[..16].iter_mut().zip(from) {
                *a = t;
            }
            if mbx + 1 == mbw {
                let last = from.get(15).copied().unwrap_or(127);
                above[16..].fill(last);
            } else {
                for (a, &t) in above[16..].iter_mut().zip(from.get(16..).unwrap_or_default()) {
                    *a = t;
                }
            }
        }
    }
    // The 4 pixels above and to the right serve the right-hand subblocks of every subblock row.
    for i in 17usize..stride {
        ws[4 * stride + i] = ws[i];
        ws[8 * stride + i] = ws[i];
        ws[12 * stride + i] = ws[i];
    }
    if mbx == 0 {
        for i in 0usize..16 {
            ws[(i + 1) * stride] = 129;
        }
    } else {
        for (i, &l) in left[1..].iter().enumerate() {
            ws[(i + 1) * stride] = l;
        }
    }
    ws[0] = if mby == 0 {
        127
    } else if mbx == 0 {
        129
    } else {
        left[0]
    };
    ws
}

/// The chroma work area of macroblock `mbx`, `mby`, as [`border_luma`].
pub(super) fn border_chroma(mbx: usize, mby: usize, top: &[u8], left: &[u8; 9]) -> [u8; CHROMA_AREA] {
    let stride = CHROMA_STRIDE;
    let mut block = [0u8; CHROMA_AREA];
    {
        let above = &mut block[1..stride];
        if mby == 0 {
            above.fill(127);
        } else {
            for (a, &t) in above.iter_mut().zip(top.get(mbx * 8..).unwrap_or_default()) {
                *a = t;
            }
        }
    }
    if mbx == 0 {
        for y in 0usize..8 {
            block[(y + 1) * stride] = 129;
        }
    } else {
        for (y, &l) in left[1..].iter().enumerate() {
            block[(y + 1) * stride] = l;
        }
    }
    block[0] = if mby == 0 {
        127
    } else if mbx == 0 {
        129
    } else {
        left[0]
    };
    block
}

/// Adds a 4 × 4 residue at `y0`, `x0`, clamped to 0–255.
pub(super) fn add_residue(pblock: &mut [u8], rblock: &[i32; 16], y0: usize, x0: usize, stride: usize) {
    let mut pos = y0 * stride + x0;
    for row in rblock.as_chunks::<4>().0 {
        if let Some(px) = pblock.get_mut(pos..pos + 4) {
            for (p, &a) in px.iter_mut().zip(row) {
                *p = a.saturating_add(i32::from(*p)).clamp(0, 255) as u8;
            }
        }
        pos += stride;
    }
}

/// The 16 subblocks of a B_PRED macroblock, each predicted from the reconstructed pixels around
/// it and its residue added before the next.
pub(super) fn predict_4x4(ws: &mut [u8; LUMA_AREA], modes: &[IntraMode; 16], resdata: &[i32; 384]) {
    let stride = LUMA_STRIDE;
    for sby in 0usize..4 {
        for sbx in 0usize..4 {
            let i = sbx + sby * 4;
            let y0 = sby * 4 + 1;
            let x0 = sbx * 4 + 1;
            match modes[i] {
                IntraMode::TM => predict_tmpred(ws, 4, x0, y0, stride),
                IntraMode::VE => predict_bvepred(ws, x0, y0, stride),
                IntraMode::HE => predict_bhepred(ws, x0, y0, stride),
                IntraMode::DC => predict_bdcpred(ws, x0, y0, stride),
                IntraMode::LD => predict_bldpred(ws, x0, y0, stride),
                IntraMode::RD => predict_brdpred(ws, x0, y0, stride),
                IntraMode::VR => predict_bvrpred(ws, x0, y0, stride),
                IntraMode::VL => predict_bvlpred(ws, x0, y0, stride),
                IntraMode::HD => predict_bhdpred(ws, x0, y0, stride),
                IntraMode::HU => predict_bhupred(ws, x0, y0, stride),
            }
            add_residue(ws, &block(resdata, i), y0, x0, stride);
        }
    }
}

/// Residue block `i` (16 coefficients) of a macroblock's 24.
pub(super) fn block(resdata: &[i32; 384], i: usize) -> [i32; 16] {
    let mut out = [0i32; 16];
    if let Some(b) = resdata.get(i * 16..i * 16 + 16) {
        out.copy_from_slice(b);
    }
    out
}

fn avg3(left: u8, this: u8, right: u8) -> u8 {
    ((u16::from(left) + 2 * u16::from(this) + u16::from(right) + 2) >> 2) as u8
}

fn avg2(this: u8, right: u8) -> u8 {
    ((u16::from(this) + u16::from(right) + 1) >> 1) as u8
}

pub(super) fn predict_vpred(a: &mut [u8], size: usize, x0: usize, y0: usize, stride: usize) {
    // This pass copies the top row to the rows below it.
    let (above, curr) = a.split_at_mut(stride * y0);
    let above_slice = &above[x0..];

    for curr_chunk in curr.chunks_exact_mut(stride).take(size) {
        for (curr, &above) in curr_chunk[1..].iter_mut().zip(above_slice) {
            *curr = above;
        }
    }
}

pub(super) fn predict_hpred(a: &mut [u8], size: usize, x0: usize, y0: usize, stride: usize) {
    // This pass copies the first value of a row to the values right of it.
    for chunk in a.chunks_exact_mut(stride).skip(y0).take(size) {
        let left = chunk[x0 - 1];
        chunk[x0..].iter_mut().for_each(|a| *a = left);
    }
}

pub(super) fn predict_dcpred(a: &mut [u8], size: usize, stride: usize, above: bool, left: bool) {
    let mut sum = 0;
    let mut shf = if size == 8 { 2 } else { 3 };

    if left {
        for y in 0usize..size {
            sum += u32::from(a[(y + 1) * stride]);
        }

        shf += 1;
    }

    if above {
        sum += a[1..=size].iter().fold(0, |acc, &x| acc + u32::from(x));

        shf += 1;
    }

    let dcval = if !left && !above { 128 } else { (sum + (1 << (shf - 1))) >> shf };

    for y in 0usize..size {
        a[1 + stride * (y + 1)..][..size].iter_mut().for_each(|a| *a = dcval as u8);
    }
}

pub(super) fn predict_tmpred(a: &mut [u8], size: usize, x0: usize, y0: usize, stride: usize) {
    // The formula for tmpred is:
    // X_ij = L_i + A_j - P (i, j=0, 1, 2, 3)
    //
    // |-----|-----|-----|-----|-----|
    // | P   | A0  | A1  | A2  | A3  |
    // |-----|-----|-----|-----|-----|
    // | L0  | X00 | X01 | X02 | X03 |
    // |-----|-----|-----|-----|-----|
    // | L1  | X10 | X11 | X12 | X13 |
    // |-----|-----|-----|-----|-----|
    // | L2  | X20 | X21 | X22 | X23 |
    // |-----|-----|-----|-----|-----|
    // | L3  | X30 | X31 | X32 | X33 |
    // |-----|-----|-----|-----|-----|
    // Diagram from p. 52 of RFC 6386

    // Split at L0
    let (above, x_block) = a.split_at_mut(y0 * stride + (x0 - 1));
    let p = i32::from(above[(y0 - 1) * stride + x0 - 1]);
    let above_slice = &above[(y0 - 1) * stride + x0..];

    for y in 0usize..size {
        let left_minus_p = i32::from(x_block[y * stride]) - p;

        // Add 1 to skip over L0 byte
        x_block[y * stride + 1..][..size]
            .iter_mut()
            .zip(above_slice)
            .for_each(|(cur, &abv)| *cur = (left_minus_p + i32::from(abv)).max(0).min(255) as u8);
    }
}

fn predict_bdcpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let mut v = 4;

    a[(y0 - 1) * stride + x0..][..4].iter().for_each(|&a| v += u32::from(a));

    for i in 0usize..4 {
        v += u32::from(a[(y0 + i) * stride + x0 - 1]);
    }

    v >>= 3;
    for chunk in a.chunks_exact_mut(stride).skip(y0).take(4) {
        for ch in &mut chunk[x0..][..4] {
            *ch = v as u8;
        }
    }
}

fn topleft_pixel(a: &[u8], x0: usize, y0: usize, stride: usize) -> u8 {
    a[(y0 - 1) * stride + x0 - 1]
}

fn top_pixels(a: &[u8], x0: usize, y0: usize, stride: usize) -> (u8, u8, u8, u8, u8, u8, u8, u8) {
    let pos = (y0 - 1) * stride + x0;
    let a_slice = &a[pos..pos + 8];
    let a0 = a_slice[0];
    let a1 = a_slice[1];
    let a2 = a_slice[2];
    let a3 = a_slice[3];
    let a4 = a_slice[4];
    let a5 = a_slice[5];
    let a6 = a_slice[6];
    let a7 = a_slice[7];

    (a0, a1, a2, a3, a4, a5, a6, a7)
}

fn left_pixels(a: &[u8], x0: usize, y0: usize, stride: usize) -> (u8, u8, u8, u8) {
    let l0 = a[y0 * stride + x0 - 1];
    let l1 = a[(y0 + 1) * stride + x0 - 1];
    let l2 = a[(y0 + 2) * stride + x0 - 1];
    let l3 = a[(y0 + 3) * stride + x0 - 1];

    (l0, l1, l2, l3)
}

fn edge_pixels(a: &[u8], x0: usize, y0: usize, stride: usize) -> (u8, u8, u8, u8, u8, u8, u8, u8, u8) {
    let pos = (y0 - 1) * stride + x0 - 1;
    let a_slice = &a[pos..=pos + 4];
    let e0 = a[pos + 4 * stride];
    let e1 = a[pos + 3 * stride];
    let e2 = a[pos + 2 * stride];
    let e3 = a[pos + stride];
    let e4 = a_slice[0];
    let e5 = a_slice[1];
    let e6 = a_slice[2];
    let e7 = a_slice[3];
    let e8 = a_slice[4];

    (e0, e1, e2, e3, e4, e5, e6, e7, e8)
}

fn predict_bvepred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let p = topleft_pixel(a, x0, y0, stride);
    let (a0, a1, a2, a3, a4, ..) = top_pixels(a, x0, y0, stride);
    let avg_1 = avg3(p, a0, a1);
    let avg_2 = avg3(a0, a1, a2);
    let avg_3 = avg3(a1, a2, a3);
    let avg_4 = avg3(a2, a3, a4);

    let avg = [avg_1, avg_2, avg_3, avg_4];

    let mut pos = y0 * stride + x0;
    for _ in 0..4 {
        a[pos..=pos + 3].copy_from_slice(&avg);
        pos += stride;
    }
}

fn predict_bhepred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let p = topleft_pixel(a, x0, y0, stride);
    let (l0, l1, l2, l3) = left_pixels(a, x0, y0, stride);

    let avgs = [avg3(p, l0, l1), avg3(l0, l1, l2), avg3(l1, l2, l3), avg3(l2, l3, l3)];

    let mut pos = y0 * stride + x0;
    for avg in avgs {
        for a_p in &mut a[pos..=pos + 3] {
            *a_p = avg;
        }
        pos += stride;
    }
}

fn predict_bldpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (a0, a1, a2, a3, a4, a5, a6, a7) = top_pixels(a, x0, y0, stride);

    let avgs = [avg3(a0, a1, a2), avg3(a1, a2, a3), avg3(a2, a3, a4), avg3(a3, a4, a5), avg3(a4, a5, a6), avg3(a5, a6, a7), avg3(a6, a7, a7)];

    let mut pos = y0 * stride + x0;

    for i in 0..4 {
        a[pos..=pos + 3].copy_from_slice(&avgs[i..=i + 3]);
        pos += stride;
    }
}

fn predict_brdpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (e0, e1, e2, e3, e4, e5, e6, e7, e8) = edge_pixels(a, x0, y0, stride);

    let avgs = [avg3(e0, e1, e2), avg3(e1, e2, e3), avg3(e2, e3, e4), avg3(e3, e4, e5), avg3(e4, e5, e6), avg3(e5, e6, e7), avg3(e6, e7, e8)];
    let mut pos = y0 * stride + x0;

    for i in 0..4 {
        a[pos..=pos + 3].copy_from_slice(&avgs[3 - i..7 - i]);
        pos += stride;
    }
}

fn predict_bvrpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (_, e1, e2, e3, e4, e5, e6, e7, e8) = edge_pixels(a, x0, y0, stride);

    a[(y0 + 3) * stride + x0] = avg3(e1, e2, e3);
    a[(y0 + 2) * stride + x0] = avg3(e2, e3, e4);
    a[(y0 + 3) * stride + x0 + 1] = avg3(e3, e4, e5);
    a[(y0 + 1) * stride + x0] = avg3(e3, e4, e5);
    a[(y0 + 2) * stride + x0 + 1] = avg2(e4, e5);
    a[y0 * stride + x0] = avg2(e4, e5);
    a[(y0 + 3) * stride + x0 + 2] = avg3(e4, e5, e6);
    a[(y0 + 1) * stride + x0 + 1] = avg3(e4, e5, e6);
    a[(y0 + 2) * stride + x0 + 2] = avg2(e5, e6);
    a[y0 * stride + x0 + 1] = avg2(e5, e6);
    a[(y0 + 3) * stride + x0 + 3] = avg3(e5, e6, e7);
    a[(y0 + 1) * stride + x0 + 2] = avg3(e5, e6, e7);
    a[(y0 + 2) * stride + x0 + 3] = avg2(e6, e7);
    a[y0 * stride + x0 + 2] = avg2(e6, e7);
    a[(y0 + 1) * stride + x0 + 3] = avg3(e6, e7, e8);
    a[y0 * stride + x0 + 3] = avg2(e7, e8);
}

fn predict_bvlpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (a0, a1, a2, a3, a4, a5, a6, a7) = top_pixels(a, x0, y0, stride);

    a[y0 * stride + x0] = avg2(a0, a1);
    a[(y0 + 1) * stride + x0] = avg3(a0, a1, a2);
    a[(y0 + 2) * stride + x0] = avg2(a1, a2);
    a[y0 * stride + x0 + 1] = avg2(a1, a2);
    a[(y0 + 1) * stride + x0 + 1] = avg3(a1, a2, a3);
    a[(y0 + 3) * stride + x0] = avg3(a1, a2, a3);
    a[(y0 + 2) * stride + x0 + 1] = avg2(a2, a3);
    a[y0 * stride + x0 + 2] = avg2(a2, a3);
    a[(y0 + 3) * stride + x0 + 1] = avg3(a2, a3, a4);
    a[(y0 + 1) * stride + x0 + 2] = avg3(a2, a3, a4);
    a[(y0 + 2) * stride + x0 + 2] = avg2(a3, a4);
    a[y0 * stride + x0 + 3] = avg2(a3, a4);
    a[(y0 + 3) * stride + x0 + 2] = avg3(a3, a4, a5);
    a[(y0 + 1) * stride + x0 + 3] = avg3(a3, a4, a5);
    a[(y0 + 2) * stride + x0 + 3] = avg3(a4, a5, a6);
    a[(y0 + 3) * stride + x0 + 3] = avg3(a5, a6, a7);
}

fn predict_bhdpred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (e0, e1, e2, e3, e4, e5, e6, e7, _) = edge_pixels(a, x0, y0, stride);

    a[(y0 + 3) * stride + x0] = avg2(e0, e1);
    a[(y0 + 3) * stride + x0 + 1] = avg3(e0, e1, e2);
    a[(y0 + 2) * stride + x0] = avg2(e1, e2);
    a[(y0 + 3) * stride + x0 + 2] = avg2(e1, e2);
    a[(y0 + 2) * stride + x0 + 1] = avg3(e1, e2, e3);
    a[(y0 + 3) * stride + x0 + 3] = avg3(e1, e2, e3);
    a[(y0 + 2) * stride + x0 + 2] = avg2(e2, e3);
    a[(y0 + 1) * stride + x0] = avg2(e2, e3);
    a[(y0 + 2) * stride + x0 + 3] = avg3(e2, e3, e4);
    a[(y0 + 1) * stride + x0 + 1] = avg3(e2, e3, e4);
    a[(y0 + 1) * stride + x0 + 2] = avg2(e3, e4);
    a[y0 * stride + x0] = avg2(e3, e4);
    a[(y0 + 1) * stride + x0 + 3] = avg3(e3, e4, e5);
    a[y0 * stride + x0 + 1] = avg3(e3, e4, e5);
    a[y0 * stride + x0 + 2] = avg3(e4, e5, e6);
    a[y0 * stride + x0 + 3] = avg3(e5, e6, e7);
}

fn predict_bhupred(a: &mut [u8], x0: usize, y0: usize, stride: usize) {
    let (l0, l1, l2, l3) = left_pixels(a, x0, y0, stride);

    a[y0 * stride + x0] = avg2(l0, l1);
    a[y0 * stride + x0 + 1] = avg3(l0, l1, l2);
    a[y0 * stride + x0 + 2] = avg2(l1, l2);
    a[(y0 + 1) * stride + x0] = avg2(l1, l2);
    a[y0 * stride + x0 + 3] = avg3(l1, l2, l3);
    a[(y0 + 1) * stride + x0 + 1] = avg3(l1, l2, l3);
    a[(y0 + 1) * stride + x0 + 2] = avg2(l2, l3);
    a[(y0 + 2) * stride + x0] = avg2(l2, l3);
    a[(y0 + 1) * stride + x0 + 3] = avg3(l2, l3, l3);
    a[(y0 + 2) * stride + x0 + 1] = avg3(l2, l3, l3);
    a[(y0 + 2) * stride + x0 + 2] = l3;
    a[(y0 + 2) * stride + x0 + 3] = l3;
    a[(y0 + 3) * stride + x0] = l3;
    a[(y0 + 3) * stride + x0 + 1] = l3;
    a[(y0 + 3) * stride + x0 + 2] = l3;
    a[(y0 + 3) * stride + x0 + 3] = l3;
}

/// The inverse DCT of one 4 × 4 block (§14.3), in place.
pub(super) fn idct4x4(block: &mut [i32; 16]) {
    const C1: i64 = 20091;
    const C2: i64 = 35468;
    let f = |b: &[i32; 16], i: usize| i64::from(b[i]);
    for i in 0usize..4 {
        let a1 = f(block, i) + f(block, 8 + i);
        let b1 = f(block, i) - f(block, 8 + i);
        let c1 = ((f(block, 4 + i) * C2) >> 16) - (f(block, 12 + i) + ((f(block, 12 + i) * C1) >> 16));
        let d1 = (f(block, 4 + i) + ((f(block, 4 + i) * C1) >> 16)) + ((f(block, 12 + i) * C2) >> 16);
        block[i] = (a1 + d1) as i32;
        block[4 + i] = (b1 + c1) as i32;
        block[12 + i] = (a1 - d1) as i32;
        block[8 + i] = (b1 - c1) as i32;
    }
    for i in 0usize..4 {
        let a1 = f(block, 4 * i) + f(block, 4 * i + 2);
        let b1 = f(block, 4 * i) - f(block, 4 * i + 2);
        let c1 = ((f(block, 4 * i + 1) * C2) >> 16) - (f(block, 4 * i + 3) + ((f(block, 4 * i + 3) * C1) >> 16));
        let d1 = (f(block, 4 * i + 1) + ((f(block, 4 * i + 1) * C1) >> 16)) + ((f(block, 4 * i + 3) * C2) >> 16);
        block[4 * i] = ((a1 + d1 + 4) >> 3) as i32;
        block[4 * i + 3] = ((a1 - d1 + 4) >> 3) as i32;
        block[4 * i + 1] = ((b1 + c1 + 4) >> 3) as i32;
        block[4 * i + 2] = ((b1 - c1 + 4) >> 3) as i32;
    }
}

/// The inverse Walsh-Hadamard transform of the Y2 block (§14.3), in place.
pub(super) fn iwht4x4(block: &mut [i32; 16]) {
    for i in 0usize..4 {
        let a1 = block[i].wrapping_add(block[12 + i]);
        let b1 = block[4 + i].wrapping_add(block[8 + i]);
        let c1 = block[4 + i].wrapping_sub(block[8 + i]);
        let d1 = block[i].wrapping_sub(block[12 + i]);
        block[i] = a1.wrapping_add(b1);
        block[4 + i] = c1.wrapping_add(d1);
        block[8 + i] = a1.wrapping_sub(b1);
        block[12 + i] = d1.wrapping_sub(c1);
    }
    for row in block.as_chunks_mut::<4>().0 {
        let a1 = row[0].wrapping_add(row[3]);
        let b1 = row[1].wrapping_add(row[2]);
        let c1 = row[1].wrapping_sub(row[2]);
        let d1 = row[0].wrapping_sub(row[3]);
        row[0] = a1.wrapping_add(b1).wrapping_add(3) >> 3;
        row[1] = c1.wrapping_add(d1).wrapping_add(3) >> 3;
        row[2] = a1.wrapping_sub(b1).wrapping_add(3) >> 3;
        row[3] = d1.wrapping_sub(c1).wrapping_add(3) >> 3;
    }
}
