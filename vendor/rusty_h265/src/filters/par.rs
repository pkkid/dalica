//! The in-loop filters on several threads. Added for Dalica (not in rusty_h265 0.6.2 as
//! published): see `README.dalica.md` at the crate root.
//!
//! The same filters as the serial pass in `filters.rs`, sample for sample, split into bands of
//! CTB rows that threads take in turn. Each band is a disjoint `chunks_mut` / `split_at_mut` of
//! the planes, so nothing is shared mutably and no `unsafe` is needed:
//!
//! 1. Boundary strengths, then the vertical edges, band by band: a vertical edge filters along a
//!    sample row, so a band of rows is closed under it.
//! 2. The horizontal edges, once every vertical edge is done (§8.7.2's order). A horizontal edge
//!    on the 8-sample luma grid reads four rows on each side and writes three, so luma bands
//!    start four rows above a CTB row (on the 4-mod-8 rows no edge reaches across); chroma edges
//!    lie on the 8-sample chroma grid and reach two rows, so chroma bands start four chroma rows
//!    above. Each band, once filtered, is copied for SAO to read.
//! 3. SAO, CTB row by CTB row, reading the deblocked copy and writing its own rows.

use super::{boundary_strength, edge_allowed, sao_neighbour_usable, scalar_sao, EDGE_CATEGORY};
use crate::accel;
use crate::frame::Plane;
use crate::pic::PicState;
use crate::ps::{Pps, Sps};
use crate::tables::{BETA_TABLE, CHROMA_QP_420, TC_TABLE};

/// Runs `jobs` on `n` threads, dealt in turn (the calling thread takes the first share).
fn run<J: Send>(jobs: Vec<J>, n: usize, f: impl Fn(J) + Sync) {
    let n = n.max(1);
    let mut per: Vec<Vec<J>> = (0..n).map(|_| Vec::new()).collect();
    for (i, j) in jobs.into_iter().enumerate() {
        per[i % n].push(j);
    }
    let f = &f;
    std::thread::scope(|s| {
        let mut it = per.into_iter();
        let first = it.next();
        for share in it.filter(|v| !v.is_empty()) {
            s.spawn(move || share.into_iter().for_each(f));
        }
        if let Some(share) = first {
            share.into_iter().for_each(f);
        }
    });
}

/// `data` cut at the element offsets `cuts` (ascending, each within `data`).
fn split_at<'a>(mut data: &'a mut [u16], cuts: &[usize]) -> Vec<&'a mut [u16]> {
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut at = 0;
    for &c in cuts {
        let mid = c.clamp(at, at + data.len()) - at;
        let (a, b) = std::mem::take(&mut data).split_at_mut(mid);
        out.push(a);
        data = b;
        at = c.max(at);
    }
    out.push(data);
    out
}

/// `v`'s first `len` elements cut at `cuts`, or as many `None`s when `on` is false.
fn take<'a>(v: &'a mut [u16], on: bool, cuts: &[usize], len: usize) -> Vec<Option<&'a mut [u16]>> {
    match v.get_mut(..len) {
        Some(v) if on => split_at(v, cuts).into_iter().map(Some).collect(),
        _ => (0..=cuts.len()).map(|_| None).collect(),
    }
}

/// What the filters need of the picture besides its planes.
pub(super) struct Inputs<'a> {
    pub st: &'a PicState,
    pub sps: &'a Sps,
    pub pps: &'a Pps,
    pub deblock: bool,
    /// Per component: SAO applies somewhere in it.
    pub sao: [bool; 3],
    pub has_bypass: bool,
}

/// Deblocking then SAO on `n` threads.
pub(super) fn filters(planes: &mut [Plane; 3], inp: &Inputs, bs_v: &mut Vec<u8>, bs_h: &mut Vec<u8>, copies: &mut [Vec<u16>; 3], n: usize) {
    let st = inp.st;
    let (w4, h4) = (st.w4, st.h4);
    let r4 = 1usize << (st.log2_ctb - 2);
    let nb = h4.div_ceil(r4);
    if inp.deblock {
        crate::prof_scope!(crate::prof::Stage::Deblock);
        bs_v.clear();
        bs_h.clear();
        bs_v.resize(w4 * h4, 0);
        bs_h.resize(w4 * h4, 0);
        let [p0, p1, p2] = &mut *planes;
        let (s0, s1, s2) = (p0.stride, p1.stride, p2.stride);
        let jobs: Vec<_> = bs_v
            .chunks_mut(r4 * w4)
            .zip(bs_h.chunks_mut(r4 * w4))
            .zip(p0.data.chunks_mut(r4 * 4 * s0))
            .zip(p1.data.chunks_mut(r4 * 2 * s1))
            .zip(p2.data.chunks_mut(r4 * 2 * s2))
            .enumerate()
            .map(|(k, ((((bv, bh), l), cb), cr))| (k, bv, bh, [l, cb, cr]))
            .collect();
        run(jobs, n, |(k, bv, bh, mut band)| {
            let a = k * r4;
            let b = (a + r4).min(h4);
            strengths(st, inp.pps, a, b, bv, bh);
            let rel = [4 * a, 2 * a, 2 * a];
            edges(&mut band, [s0, s1, s2], rel, inp, a, b, 0, &bv[..]);
        });
    }
    let any_sao = inp.sao.iter().any(|&s| s);
    if inp.deblock || any_sao {
        // Horizontal edges, and the copy SAO reads.
        let strides = [planes[0].stride, planes[1].stride, planes[2].stride];
        let heights = [planes[0].height, planes[1].height, planes[2].height];
        let lens = [planes[0].data.len(), planes[1].data.len(), planes[2].data.len()];
        // Band k: luma rows from 4·(k·r4) − 4, chroma rows from 2·(k·r4) − 4.
        let cuts = |c: usize| -> Vec<usize> {
            let unit = if c == 0 { 4 } else { 2 };
            (1..nb).map(|k| ((unit * k * r4).saturating_sub(4)).min(heights[c]) * strides[c]).collect()
        };
        let starts = |c: usize| -> Vec<usize> {
            let unit = if c == 0 { 4 } else { 2 };
            (0..nb).map(|k| if k == 0 { 0 } else { ((unit * k * r4).saturating_sub(4)).min(heights[c]) }).collect()
        };
        let (c0, c1, c2) = (cuts(0), cuts(1), cuts(2));
        let (st0, st1, st2) = (starts(0), starts(1), starts(2));
        for (c, cp) in copies.iter_mut().enumerate() {
            let need = if inp.sao[c] { lens[c] } else { 0 };
            if cp.len() < need {
                cp.resize(need, 0);
            }
        }
        let [q0, q1, q2] = &mut *copies;
        // Borrow the copies before the planes are moved into the band list.
        let mut k0 = take(q0, inp.sao[0], &c0, lens[0]).into_iter();
        let mut k1 = take(q1, inp.sao[1], &c1, lens[1]).into_iter();
        let mut k2 = take(q2, inp.sao[2], &c2, lens[2]).into_iter();
        let [x0, x1, x2] = &mut *planes;
        let mut l = split_at(&mut x0.data, &c0).into_iter();
        let mut u = split_at(&mut x1.data, &c1).into_iter();
        let mut v = split_at(&mut x2.data, &c2).into_iter();
        let mut jobs = Vec::with_capacity(nb);
        for k in 0..nb {
            let (Some(a), Some(b), Some(c)) = (l.next(), u.next(), v.next()) else { break };
            let copy = [k0.next().flatten(), k1.next().flatten(), k2.next().flatten()];
            jobs.push((k, [a, b, c], copy));
        }
        let bs_h = &bs_h[..];
        run(jobs, n, |(k, mut band, copy)| {
            let a = k * r4;
            let b = (a + r4).min(h4);
            if inp.deblock {
                edges(&mut band, strides, [st0[k], st1[k], st2[k]], inp, a, b, 1, bs_h);
            }
            for (dst, src) in copy.into_iter().zip(band.iter()) {
                if let Some(dst) = dst {
                    dst.copy_from_slice(src);
                }
            }
        });
    }
    if any_sao && std::env::var_os("RH265_NO_SAO").is_none() {
        crate::prof_scope!(crate::prof::Stage::Sao);
        let ctb = 1usize << st.log2_ctb;
        let [p0, p1, p2] = &mut *planes;
        let strides = [p0.stride, p1.stride, p2.stride];
        let dims = [(p0.width, p0.height), (p1.width, p1.height), (p2.width, p2.height)];
        let jobs: Vec<_> = p0
            .data
            .chunks_mut(ctb * strides[0])
            .zip(p1.data.chunks_mut((ctb >> 1) * strides[1]))
            .zip(p2.data.chunks_mut((ctb >> 1) * strides[2]))
            .enumerate()
            .map(|(ry, ((a, b), c))| (ry, [a, b, c]))
            .collect();
        let copies = &*copies;
        let use_kernel = !inp.has_bypass && !scalar_sao();
        run(jobs, n, |(ry, bands)| {
            for (c, dst) in bands.into_iter().enumerate() {
                if !inp.sao[c] {
                    continue;
                }
                let geo = Geo { c, stride: strides[c], w: dims[c].0, h: dims[c].1, by0: ry * (ctb >> usize::from(c > 0)) };
                for rx in 0..st.ctb_w {
                    sao_ctb(dst, &copies[c], &geo, inp, rx, ry, use_kernel);
                }
            }
        });
    }
}

/// Boundary strengths of the 4x4 rows `a..b` into the band's rows of the two maps (the serial
/// pass's first loop, `filters.rs`).
fn strengths(st: &PicState, pps: &Pps, a: usize, b: usize, bv_band: &mut [u8], bh_band: &mut [u8]) {
    let w4 = st.w4;
    for y4 in a..b {
        let row = y4 * w4;
        let local = (y4 - a) * w4;
        let edges = &st.edges[row..row + w4];
        let modes = &st.pred_mode[row..row + w4];
        let above = if y4 > 0 { &st.pred_mode[row - w4..row] } else { modes };
        let bv = &mut bv_band[local..local + w4];
        let bh = &mut bh_band[local..local + w4];
        for x4 in 0..w4 {
            let e = edges[x4];
            if modes[x4] == 0 {
                continue;
            }
            let ve = x4 > 0 && x4 % 2 == 0 && e & 0b0101 != 0 && modes[x4 - 1] != 0;
            let he = y4 > 0 && y4 % 2 == 0 && e & 0b1010 != 0 && above[x4] != 0;
            if !(ve || he) {
                continue;
            }
            let cq = st.ctb_of(x4 * 4, y4 * 4);
            let fq = &st.ctb_filter[cq];
            if fq.deblock_disabled {
                continue;
            }
            let lfs = fq.lf_across_slices;
            if ve && edge_allowed(st, pps, st.ctb_of(x4 * 4 - 1, y4 * 4), cq, lfs) {
                bv[x4] = boundary_strength(st, row + x4 - 1, row + x4, e & 1 != 0);
            }
            if he && edge_allowed(st, pps, st.ctb_of(x4 * 4, y4 * 4 - 1), cq, lfs) {
                bh[x4] = boundary_strength(st, row + x4 - w4, row + x4, e & 2 != 0);
            }
        }
    }
}

/// The edges of direction `dir` in 4x4 rows `a..b` (the serial pass's second loop), on bands
/// whose first rows are `rel` (luma, Cb, Cr). `bs` is the strength map: the band's own rows for
/// the vertical edges (`dir` 0), the whole map for the horizontal ones.
#[allow(clippy::too_many_arguments)]
fn edges(band: &mut [&mut [u16]; 3], strides: [usize; 3], rel: [usize; 3], inp: &Inputs, a: usize, b: usize, dir: usize, bs: &[u8]) {
    let st = inp.st;
    let w4 = st.w4;
    let bd_y = inp.sps.bit_depth_luma;
    let bd_c = inp.sps.bit_depth_chroma;
    let (ystep, xs, xstep) = if dir == 0 { (1, 2, 2) } else { (2, 0, 1) };
    let mut y4 = if dir == 0 { a } else { a.max(2).next_multiple_of(2) };
    let base = if dir == 0 { a * w4 } else { 0 };
    while y4 < b {
        let mut x4 = xs;
        while x4 < w4 {
            let i = y4 * w4 + x4;
            let bsv = bs[i - base];
            if bsv == 0 {
                x4 += xstep;
                continue;
            }
            let p = if dir == 0 { i - 1 } else { i - w4 };
            let f = &st.ctb_filter[st.ctb_of(x4 * 4, y4 * 4)];
            let qp = (st.qp_y[p] as i32 + st.qp_y[i] as i32 + 1) >> 1;
            let no_p = st.filter_bypass[p] != 0;
            let no_q = st.filter_bypass[i] != 0;
            let qb = (qp + ((f.beta_offset_div2 as i32) << 1)).clamp(0, 51);
            let beta = (BETA_TABLE[qb as usize] as i32) << (bd_y - 8);
            let qt = (qp + 2 * (bsv as i32 - 1) + ((f.tc_offset_div2 as i32) << 1)).clamp(0, 53);
            let tc = (TC_TABLE[qt as usize] as i32) << (bd_y - 8);
            let mx = (1i32 << bd_y) - 1;
            accel::deblock::luma_edge(band[0], strides[0], x4 * 4, y4 * 4 - rel[0], dir, beta, tc, no_p, no_q, mx);
            let on_chroma_grid = if dir == 0 { x4 % 4 == 0 } else { y4 % 4 == 0 };
            if bsv == 2 && on_chroma_grid {
                for c in 1..3usize {
                    let off = if c == 1 { inp.pps.cb_qp_offset } else { inp.pps.cr_qp_offset };
                    let qpi = qp + off;
                    let qpc = if qpi < 0 {
                        qpi
                    } else if qpi > 57 {
                        qpi - 6
                    } else {
                        CHROMA_QP_420[qpi as usize] as i32
                    };
                    let qt = (qpc + 2 + ((f.tc_offset_div2 as i32) << 1)).clamp(0, 53);
                    let tc = (TC_TABLE[qt as usize] as i32) << (bd_c - 8);
                    chroma_edge(band[c], strides[c], x4 * 2, y4 * 2 - rel[c], dir, tc, no_p, no_q, bd_c);
                }
            }
            x4 += xstep;
        }
        y4 += ystep;
    }
}

/// `filters.rs`'s `filter_chroma_edge` on a band.
#[allow(clippy::too_many_arguments)]
fn chroma_edge(data: &mut [u16], stride: usize, x: usize, y: usize, dir: usize, tc: i32, no_p: bool, no_q: bool, bd: u8) {
    let max = (1i32 << bd) - 1;
    let origin = y * stride + x;
    #[inline(always)]
    fn line(p1: i32, p0: i32, q0: i32, q1: i32, tc: i32) -> i32 {
        ((((q0 - p0) << 2) + p1 - q1 + 4) >> 3).clamp(-tc, tc)
    }
    if dir == 0 {
        let lo = origin - 2;
        let w = &mut data[lo..lo + stride + 4];
        for k in 0..2 {
            let b = k * stride;
            let (p1, p0, q0, q1) = (w[b] as i32, w[b + 1] as i32, w[b + 2] as i32, w[b + 3] as i32);
            let d = line(p1, p0, q0, q1, tc);
            if !no_p {
                w[b + 1] = (p0 + d).clamp(0, max) as u16;
            }
            if !no_q {
                w[b + 2] = (q0 - d).clamp(0, max) as u16;
            }
        }
    } else {
        let lo = origin - 2 * stride;
        let w = &mut data[lo..lo + 3 * stride + 2];
        for k in 0..2 {
            let (p1, p0, q0, q1) = (w[k] as i32, w[stride + k] as i32, w[2 * stride + k] as i32, w[3 * stride + k] as i32);
            let d = line(p1, p0, q0, q1, tc);
            if !no_p {
                w[stride + k] = (p0 + d).clamp(0, max) as u16;
            }
            if !no_q {
                w[2 * stride + k] = (q0 - d).clamp(0, max) as u16;
            }
        }
    }
}

/// A component's plane, and where its band starts.
struct Geo {
    c: usize,
    stride: usize,
    w: usize,
    h: usize,
    /// First row of the band `dst` holds.
    by0: usize,
}

/// SAO of CTB (`rx`, `ry`) of one component (`filters.rs`'s per-CTB body): `src` is the whole
/// deblocked copy, `dst` the band of the CTB row.
fn sao_ctb(dst: &mut [u16], src: &[u16], g: &Geo, inp: &Inputs, rx: usize, ry: usize, use_kernel: bool) {
    let st = inp.st;
    let rs = ry * st.ctb_w + rx;
    if st.slice_addr[rs] < 0 {
        return;
    }
    let c = g.c;
    let ss = usize::from(c > 0);
    let bd = if c == 0 { inp.sps.bit_depth_luma } else { inp.sps.bit_depth_chroma };
    let max = (1i32 << bd) - 1;
    let has_bypass = inp.has_bypass;
    let (pw, ph, pstride) = (g.w, g.h, g.stride);
    let prm = st.sao[rs][c];
    if prm.type_idx == 0 {
        return;
    }
    let cs = (1usize << st.log2_ctb) >> ss;
    let x0 = rx * cs;
    let y0 = ry * cs;
    let x1 = (x0 + cs).min(pw);
    let y1 = (y0 + cs).min(ph);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let by0 = g.by0;
    // `src` from the band's first row: the kernels index it as they index `dst`.
    let src_band = &src[by0 * pstride..];
    let fq = &st.ctb_filter[rs];
    if prm.type_idx == 1 {
        let mut band = [0i16; 32];
        for k in 0..4usize {
            band[(k + prm.aux as usize) & 31] = prm.offset[k];
        }
        let shift = bd - 5;
        if use_kernel {
            accel::sao::sao_band(dst, src_band, pstride, x0, y0 - by0, x1 - x0, y1 - y0, shift as u32, prm.aux, &band, max);
            return;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                if has_bypass && st.filter_bypass[st.idx4(x << ss, y << ss)] != 0 {
                    continue;
                }
                let v = src[y * pstride + x] as i32;
                let off = band[(v >> shift) as usize] as i32;
                if off != 0 {
                    dst[(y - by0) * pstride + x] = (v + off).clamp(0, max) as u16;
                }
            }
        }
        return;
    }
    let (da, db) = match prm.aux {
        0 => ((-1i32, 0i32), (1i32, 0i32)),
        1 => ((0, -1), (0, 1)),
        2 => ((-1, -1), (1, 1)),
        _ => ((1, -1), (-1, 1)),
    };
    let offs = prm.offset;
    let ix0 = x0 + 1;
    let iy0 = y0 + 1;
    let ix1 = x1.saturating_sub(1);
    let iy1 = y1.saturating_sub(1);
    let interior_ok = ix0 < ix1 && iy0 < iy1;
    if interior_ok && use_kernel {
        accel::sao::sao_edge(dst, src_band, pstride, ix0, iy0 - by0, ix1 - ix0, iy1 - iy0, da, db, &offs, max);
    } else if interior_ok {
        for y in iy0..iy1 {
            let row = y * pstride;
            let arow = (y as i32 + da.1) as usize * pstride;
            let brow = (y as i32 + db.1) as usize * pstride;
            for x in ix0..ix1 {
                if has_bypass && st.filter_bypass[st.idx4(x << ss, y << ss)] != 0 {
                    continue;
                }
                let v = src[row + x] as i32;
                let a = src[(arow as i32 + x as i32 + da.0) as usize] as i32;
                let b = src[(brow as i32 + x as i32 + db.0) as usize] as i32;
                let e = 2 + (v - a).signum() + (v - b).signum();
                let k = EDGE_CATEGORY[e as usize];
                if k != 0 {
                    let off = offs[k as usize - 1] as i32;
                    dst[(y - by0) * pstride + x] = (v + off).clamp(0, max) as u16;
                }
            }
        }
    }
    for y in y0..y1 {
        let interior_row = interior_ok && y >= iy0 && y < iy1;
        let mut x = x0;
        while x < x1 {
            if interior_row && x == ix0 {
                x = ix1;
                continue;
            }
            if !(has_bypass && st.filter_bypass[st.idx4(x << ss, y << ss)] != 0) {
                let (ax, ay) = (x as i32 + da.0, y as i32 + da.1);
                let (bx, by) = (x as i32 + db.0, y as i32 + db.1);
                let inside = ax >= 0 && ay >= 0 && bx >= 0 && by >= 0 && ax < pw as i32 && ay < ph as i32 && bx < pw as i32 && by < ph as i32;
                if inside
                    && sao_neighbour_usable(st, inp.pps, fq.lf_across_slices, x << ss, y << ss, (ax as usize) << ss, (ay as usize) << ss)
                    && sao_neighbour_usable(st, inp.pps, fq.lf_across_slices, x << ss, y << ss, (bx as usize) << ss, (by as usize) << ss)
                {
                    let v = src[y * pstride + x] as i32;
                    let a = src[ay as usize * pstride + ax as usize] as i32;
                    let b = src[by as usize * pstride + bx as usize] as i32;
                    let e = 2 + (v - a).signum() + (v - b).signum();
                    let k = EDGE_CATEGORY[e as usize];
                    if k != 0 {
                        let off = offs[k as usize - 1] as i32;
                        dst[(y - by0) * pstride + x] = (v + off).clamp(0, max) as u16;
                    }
                }
            }
            x += 1;
        }
    }
}
