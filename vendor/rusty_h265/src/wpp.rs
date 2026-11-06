//! Wavefront decoding on several threads. Added for Dalica (not in rusty_h265 0.6.2 as
//! published): see `README.dalica.md` at the crate root.
//!
//! A slice segment that covers the whole picture with `entropy_coding_sync_enabled_flag` carries
//! one substream per CTB row, each at its own entry point, and a row's contexts start from the
//! ones the row above stored after its second CTB (§9.3.1, §9.3.2.4). So once the row above is
//! two CTBs ahead, a row can decode on its own: everything a CTB reads of the picture being
//! decoded lies to its left, in the row above up to the CTB above-right, or in the reference
//! pictures, which are finished.
//!
//! Each thread decodes every `threads`-th row into a private picture and private per-4x4 maps of
//! the full size (so the CTU code indexes them as it always does), receives from the thread above
//! the last sample row and the last row of 4x4 maps of every CTB the row above finished (an
//! [`Edge`]) and the contexts after its second CTB, sends its own to the thread below, and copies
//! each finished row into its own band of the real picture and maps (disjoint `chunks_mut`, so no
//! `unsafe`). Nothing is shared mutably: rows talk through channels.
//!
//! The output is the serial decoder's, bit for bit. A row that fails, or a slice that ends early,
//! stops every row below it, and the CTBs after the first such point in raster order are put back
//! as never decoded, which is what the serial loop leaves there.

use std::sync::mpsc::{channel, Receiver, Sender};

use crate::cabac::Contexts;
use crate::ctu::{Ablate, CtxStore, RowEnd, ScalingFactors, SliceDecoder, SliceInputs};
use crate::decoder::RefLists;
use crate::error::{Error, Result};
use crate::frame::Picture;
use crate::nal::Rbsp;
use crate::pic::{CtbFilterParams, Motion, PicState, SaoParams, SeqTables, PRED_NONE};
use crate::ps::{Pps, Sps, TileLayout};
use crate::slice::SliceHeader;

/// The largest CTB HEVC allows.
const MAX_CTB: usize = 64;
const MAX_CTB4: usize = MAX_CTB / 4;

/// Whether a slice segment can be decoded row by row on `threads` threads.
pub(crate) fn eligible(threads: usize, sps: &Sps, pps: &Pps, sh: &SliceHeader, st: &PicState) -> bool {
    threads > 1
        && pps.entropy_coding_sync_enabled
        && !pps.tiles_enabled
        && !pps.dependent_slice_segments_enabled
        && !sh.dependent_slice_segment
        && sh.segment_address == 0
        && sps.chroma_format_idc == 1
        && st.log2_ctb >= 4
        && (1usize << st.log2_ctb) <= MAX_CTB
        && st.ctb_w >= 2
        && st.ctb_h >= 2
        && sh.entry_point_offsets.len() + 1 == st.ctb_h
}

/// The slice-wide inputs every row shares.
pub(crate) struct Shared<'a> {
    pub sps: &'a Sps,
    pub pps: &'a Pps,
    pub sh: &'a SliceHeader,
    pub tiles: &'a TileLayout,
    pub rbsp: &'a Rbsp,
    pub scaling: Option<&'a ScalingFactors>,
    pub refs: &'a RefLists,
    pub poc: i32,
    pub ablate: Ablate,
    pub slice_addr_rs: i32,
    pub slice_idx: u16,
    pub tables: &'a SeqTables,
}

/// What the row below needs of one CTB: its last sample row, its last row of 4x4 maps and its
/// own per-CTB entries.
struct Edge {
    rx: usize,
    luma: [u16; MAX_CTB],
    cb: [u16; MAX_CTB / 2],
    cr: [u16; MAX_CTB / 2],
    pred_mode: [u8; MAX_CTB4],
    intra_mode: [u8; MAX_CTB4],
    qp_y: [i8; MAX_CTB4],
    ct_depth: [u8; MAX_CTB4],
    edges: [u8; MAX_CTB4],
    nz: [u8; MAX_CTB4],
    filter_bypass: [u8; MAX_CTB4],
    motion: [Motion; MAX_CTB4],
    slice_addr: i32,
    ctb_slice: u16,
    ctb_done: bool,
    sao: [SaoParams; 3],
    ctb_filter: CtbFilterParams,
}

enum Msg {
    Edge(Box<Edge>),
    Ctx(Box<Contexts>),
    /// The row above stopped: so does this one.
    Stop,
}

/// Every per-4x4 map of [`PicState`].
macro_rules! maps4 {
    ($m:ident) => {
        $m!(pred_mode);
        $m!(intra_mode);
        $m!(qp_y);
        $m!(ct_depth);
        $m!(edges);
        $m!(nz);
        $m!(filter_bypass);
        $m!(motion);
    };
}

/// Every per-CTB map of [`PicState`] a slice writes.
macro_rules! maps_ctb {
    ($m:ident) => {
        $m!(slice_addr);
        $m!(ctb_slice);
        $m!(ctb_done);
        $m!(sao);
        $m!(ctb_filter);
    };
}

/// Where the last sample row of CTB row `ry` lies in a plane `1 << shift` times smaller, and
/// the columns of CTB `rx`: (row, x0, x1), or `None` outside the plane.
fn bottom(plane_w: usize, plane_h: usize, ctb: usize, shift: usize, rx: usize, ry: usize) -> Option<(usize, usize, usize)> {
    let y = (((ry + 1) * ctb) >> shift).min(plane_h).checked_sub(1)?;
    let x0 = (rx * ctb) >> shift;
    let x1 = (((rx + 1) * ctb) >> shift).min(plane_w);
    (x0 < x1).then_some((y, x0, x1))
}

/// The edge of CTB (`rx`, `ry`) of a private picture.
fn take(st: &PicState, pic: &Picture, rx: usize, ry: usize) -> Box<Edge> {
    let ctb = 1usize << st.log2_ctb;
    let mut e = Box::new(Edge {
        rx,
        luma: [0; MAX_CTB],
        cb: [0; MAX_CTB / 2],
        cr: [0; MAX_CTB / 2],
        pred_mode: [PRED_NONE; MAX_CTB4],
        intra_mode: [0; MAX_CTB4],
        qp_y: [0; MAX_CTB4],
        ct_depth: [0; MAX_CTB4],
        edges: [0; MAX_CTB4],
        nz: [0; MAX_CTB4],
        filter_bypass: [0; MAX_CTB4],
        motion: [Motion::default(); MAX_CTB4],
        slice_addr: -1,
        ctb_slice: 0,
        ctb_done: false,
        sao: [SaoParams::default(); 3],
        ctb_filter: CtbFilterParams::default(),
    });
    for (c, out) in [(0usize, &mut e.luma[..]), (1, &mut e.cb[..]), (2, &mut e.cr[..])] {
        let p = &pic.planes[c];
        let shift = usize::from(c > 0);
        if let Some((y, x0, x1)) = bottom(p.width, p.height, ctb, shift, rx, ry) {
            let a = y * p.stride;
            out[..x1 - x0].copy_from_slice(&p.data[a + x0..a + x1]);
        }
    }
    if let Some((y4, x0, x1)) = bottom(st.w4, st.h4, ctb, 2, rx, ry) {
        let a = y4 * st.w4;
        macro_rules! out {
            ($f:ident) => {
                e.$f[..x1 - x0].copy_from_slice(&st.$f[a + x0..a + x1]);
            };
        }
        maps4!(out);
    }
    let rs = ry * st.ctb_w + rx;
    macro_rules! out_ctb {
        ($f:ident) => {
            e.$f = st.$f[rs];
        };
    }
    maps_ctb!(out_ctb);
    e
}

/// Writes the edge of CTB (`e.rx`, `ry`) into a private picture.
fn put(e: &Edge, st: &mut PicState, pic: &mut Picture, ry: usize) {
    let ctb = 1usize << st.log2_ctb;
    let rx = e.rx;
    for (c, src) in [(0usize, &e.luma[..]), (1, &e.cb[..]), (2, &e.cr[..])] {
        let p = &mut pic.planes[c];
        let shift = usize::from(c > 0);
        if let Some((y, x0, x1)) = bottom(p.width, p.height, ctb, shift, rx, ry) {
            let a = y * p.stride;
            p.data[a + x0..a + x1].copy_from_slice(&src[..x1 - x0]);
        }
    }
    if let Some((y4, x0, x1)) = bottom(st.w4, st.h4, ctb, 2, rx, ry) {
        let a = y4 * st.w4;
        macro_rules! inp {
            ($f:ident) => {
                st.$f[a + x0..a + x1].copy_from_slice(&e.$f[..x1 - x0]);
            };
        }
        maps4!(inp);
    }
    let rs = ry * st.ctb_w + rx;
    if rs < st.slice_addr.len() {
        macro_rules! inp_ctb {
            ($f:ident) => {
                st.$f[rs] = e.$f;
            };
        }
        maps_ctb!(inp_ctb);
    }
}

/// One thread's bands of the real picture and maps: band `k` is CTB row `t + k * threads`.
struct Bands<'a> {
    planes: [Vec<&'a mut [u16]>; 3],
    pred_mode: Vec<&'a mut [u8]>,
    intra_mode: Vec<&'a mut [u8]>,
    qp_y: Vec<&'a mut [i8]>,
    ct_depth: Vec<&'a mut [u8]>,
    edges: Vec<&'a mut [u8]>,
    nz: Vec<&'a mut [u8]>,
    filter_bypass: Vec<&'a mut [u8]>,
    motion: Vec<&'a mut [Motion]>,
    slice_addr: Vec<&'a mut [i32]>,
    ctb_slice: Vec<&'a mut [u16]>,
    ctb_done: Vec<&'a mut [bool]>,
    sao: Vec<&'a mut [[SaoParams; 3]]>,
    ctb_filter: Vec<&'a mut [CtbFilterParams]>,
}

/// Deals `v`'s bands of `band` elements to `n` threads in turn.
fn deal<T>(v: &mut [T], band: usize, n: usize) -> Vec<Vec<&mut [T]>> {
    let mut out: Vec<Vec<&mut [T]>> = (0..n).map(|_| Vec::new()).collect();
    for (i, c) in v.chunks_mut(band.max(1)).enumerate() {
        out[i % n].push(c);
    }
    out
}

/// Band lengths: the planes', a per-4x4 map's, a per-CTB map's.
fn band_lens(st: &PicState, pic: &Picture) -> ([usize; 3], usize, usize) {
    let ctb = 1usize << st.log2_ctb;
    let planes = [0usize, 1, 2].map(|c| {
        let p = &pic.planes[c];
        (ctb >> usize::from(c > 0)) * p.stride
    });
    (planes, (ctb >> 2) * st.w4, st.ctb_w)
}

fn bands<'a>(pic: &'a mut Picture, st: &'a mut PicState, n: usize) -> Vec<Bands<'a>> {
    let (lp, l4, lc) = band_lens(st, pic);
    let [p0, p1, p2] = &mut pic.planes;
    let mut planes = [deal(&mut p0.data, lp[0], n), deal(&mut p1.data, lp[1], n), deal(&mut p2.data, lp[2], n)];
    macro_rules! dealt {
        ($f:ident, $len:expr) => {
            let mut $f = deal(&mut st.$f, $len, n);
        };
    }
    dealt!(pred_mode, l4);
    dealt!(intra_mode, l4);
    dealt!(qp_y, l4);
    dealt!(ct_depth, l4);
    dealt!(edges, l4);
    dealt!(nz, l4);
    dealt!(filter_bypass, l4);
    dealt!(motion, l4);
    dealt!(slice_addr, lc);
    dealt!(ctb_slice, lc);
    dealt!(ctb_done, lc);
    dealt!(sao, lc);
    dealt!(ctb_filter, lc);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        // Popped from the back, so reversed below.
        out.push(Bands {
            planes: [planes[0].pop().unwrap_or_default(), planes[1].pop().unwrap_or_default(), planes[2].pop().unwrap_or_default()],
            pred_mode: pred_mode.pop().unwrap_or_default(),
            intra_mode: intra_mode.pop().unwrap_or_default(),
            qp_y: qp_y.pop().unwrap_or_default(),
            ct_depth: ct_depth.pop().unwrap_or_default(),
            edges: edges.pop().unwrap_or_default(),
            nz: nz.pop().unwrap_or_default(),
            filter_bypass: filter_bypass.pop().unwrap_or_default(),
            motion: motion.pop().unwrap_or_default(),
            slice_addr: slice_addr.pop().unwrap_or_default(),
            ctb_slice: ctb_slice.pop().unwrap_or_default(),
            ctb_done: ctb_done.pop().unwrap_or_default(),
            sao: sao.pop().unwrap_or_default(),
            ctb_filter: ctb_filter.pop().unwrap_or_default(),
        });
    }
    out.reverse();
    out
}

/// Copies CTB row `ry` (band `k` of this thread) from the private picture into the real one.
fn copy_out(b: &mut Bands, k: usize, ry: usize, st: &PicState, pic: &Picture) {
    let (lp, l4, lc) = band_lens(st, pic);
    for c in 0..3 {
        if let Some(dst) = b.planes[c].get_mut(k) {
            let a = ry * lp[c];
            if let Some(src) = pic.planes[c].data.get(a..a + dst.len()) {
                dst.copy_from_slice(src);
            }
        }
    }
    macro_rules! out {
        ($f:ident, $len:expr) => {
            if let Some(dst) = b.$f.get_mut(k) {
                let a = ry * $len;
                if let Some(src) = st.$f.get(a..a + dst.len()) {
                    dst.copy_from_slice(src);
                }
            }
        };
    }
    out!(pred_mode, l4);
    out!(intra_mode, l4);
    out!(qp_y, l4);
    out!(ct_depth, l4);
    out!(edges, l4);
    out!(nz, l4);
    out!(filter_bypass, l4);
    out!(motion, l4);
    out!(slice_addr, lc);
    out!(ctb_slice, lc);
    out!(ctb_done, lc);
    out!(sao, lc);
    out!(ctb_filter, lc);
}

/// Receives edges of the row above (`ry - 1`) until `need` of them and, with `want_ctx`, its
/// stored contexts have come; false when the row above stopped.
struct Inbox {
    rx: Receiver<Msg>,
    have: usize,
    ctx: Option<Contexts>,
}

/// How long a row spins for the row above before it sleeps: a CTB decodes in a few
/// microseconds, about what waking a sleeping thread takes, so a short wait is cheaper spent awake.
const SPIN: std::time::Duration = std::time::Duration::from_micros(50);

impl Inbox {
    fn next(&self) -> std::result::Result<Msg, std::sync::mpsc::RecvError> {
        let start = std::time::Instant::now();
        loop {
            match self.rx.try_recv() {
                Ok(m) => return Ok(m),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err(std::sync::mpsc::RecvError),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            if start.elapsed() >= SPIN {
                return self.rx.recv();
            }
            std::hint::spin_loop();
        }
    }

    fn wait(&mut self, need: usize, want_ctx: bool, st: &mut PicState, pic: &mut Picture, above: usize) -> bool {
        while self.have < need || (want_ctx && self.ctx.is_none()) {
            match self.next() {
                Ok(Msg::Edge(e)) => {
                    put(&e, st, pic, above);
                    self.have = e.rx + 1;
                }
                Ok(Msg::Ctx(c)) => self.ctx = Some(*c),
                Ok(Msg::Stop) | Err(_) => return false,
            }
        }
        true
    }
}

type RowResult = (usize, usize, Result<RowEnd>);

/// Thread `t` of `n`: rows `t`, `t + n`, … .
fn rows(t: usize, n: usize, sh: &Shared, own: &mut (Picture, PicState), inbox: Receiver<Msg>, tx: Sender<Msg>, mut out: Bands) -> Vec<RowResult> {
    let (pic, st) = own;
    // The sequence's tables; the maps are reset row by row below (`reset_row`), the rows
    // this thread decodes being the only ones it copies out.
    st.width = sh.sps.width as usize;
    st.height = sh.sps.height as usize;
    st.log2_ctb = sh.sps.log2_ctb_size as usize;
    st.zs = std::sync::Arc::clone(&sh.tables.zs);
    st.tile_id = std::sync::Arc::clone(&sh.tables.tile_id);
    let ctb_h = st.ctb_h;
    let ctb_w = st.ctb_w;
    let mut store = CtxStore::default();
    let dec = SliceDecoder::new(SliceInputs {
        sps: sh.sps,
        pps: sh.pps,
        sh: sh.sh,
        tiles: sh.tiles,
        pic,
        st,
        store: &mut store,
        rbsp: sh.rbsp,
        scaling: sh.scaling,
        refs: sh.refs,
        poc: sh.poc,
        ablate: sh.ablate,
        slice_addr_rs: sh.slice_addr_rs,
        slice_idx: sh.slice_idx,
    });
    let mut dec = match dec {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(Msg::Stop);
            return vec![(t, 0, Err(e))];
        }
    };
    let mut inbox = Inbox { rx: inbox, have: 0, ctx: None };
    let mut results = Vec::new();
    let mut ry = t;
    let mut k = 0;
    while ry < ctb_h {
        inbox.have = 0;
        inbox.ctx = None;
        {
            let (st, _) = dec.target();
            reset_row(st, ry);
        }
        let above = ry.wrapping_sub(1);
        let first = ry == 0;
        let ok = first || {
            let (st, pic) = dec.target();
            inbox.wait(2.min(ctb_w), true, st, pic, above)
        };
        let (n_done, end) = if ok {
            let sync = inbox.ctx.take();
            let mut ready = |rx: usize, st: &mut PicState, pic: &mut Picture| first || inbox.wait((rx + 2).min(ctb_w), false, st, pic, above);
            let mut done = |rx: usize, st: &PicState, pic: &Picture, ctx: Option<&Contexts>| {
                let _ = tx.send(Msg::Edge(take(st, pic, rx, ry)));
                if let Some(c) = ctx {
                    let _ = tx.send(Msg::Ctx(Box::new(c.clone())));
                }
            };
            dec.decode_row(ry, sync, &mut ready, &mut done)
        } else {
            (0, Ok(RowEnd::Stopped))
        };
        if n_done > 0 {
            let (st, pic) = dec.target();
            copy_out(&mut out, k, ry, st, pic);
        }
        let go_on = matches!(end, Ok(RowEnd::Done));
        results.push((ry, n_done, end));
        if !go_on {
            let _ = tx.send(Msg::Stop);
            break;
        }
        ry += n;
        k += 1;
    }
    results
}

/// Decodes a whole-picture WPP slice segment ([`eligible`]) on `threads` threads into `pic` and
/// `st`, with `pool` holding each thread's private picture and maps between pictures.
pub(crate) fn decode(sh: &Shared, pic: &mut Picture, st: &mut PicState, pool: &mut Vec<(Picture, PicState)>, threads: usize) -> Result<()> {
    let (ctb_w, ctb_h) = (st.ctb_w, st.ctb_h);
    let n = threads.min(ctb_h).max(1);
    let geometry = |p: &Picture| (p.chroma_format_idc, [0, 1, 2].map(|c| (p.planes[c].width, p.planes[c].height)));
    pool.retain(|(p, s)| geometry(p) == geometry(pic) && (s.w4, s.h4, s.ctb_w, s.ctb_h) == (st.w4, st.h4, st.ctb_w, st.ctb_h));
    while pool.len() < n {
        let p = Picture::new(pic.planes[0].width, pic.planes[0].height, pic.chroma_format_idc, pic.bit_depth_luma, pic.bit_depth_chroma);
        pool.push((p, PicState::with_tables(sh.sps, sh.tables)));
    }
    for (p, _) in pool.iter_mut() {
        p.bit_depth_luma = pic.bit_depth_luma;
        p.bit_depth_chroma = pic.bit_depth_chroma;
    }
    let mut txs = Vec::with_capacity(n);
    let mut rxs = Vec::with_capacity(n);
    for _ in 0..n {
        let (tx, rx) = channel();
        txs.push(tx);
        rxs.push(rx);
    }
    // Thread t sends to thread t + 1, which decodes the row below.
    txs.rotate_left(1);
    let outs = bands(pic, st, n);
    let mut results: Vec<RowResult> = Vec::new();
    std::thread::scope(|s| {
        let mut work = pool.iter_mut().zip(rxs).zip(txs).zip(outs).enumerate();
        let first = work.next();
        let handles: Vec<_> = work.map(|(t, (((own, rx), tx), out))| s.spawn(move || rows(t, n, sh, own, rx, tx, out))).collect();
        if let Some((t, (((own, rx), tx), out))) = first {
            results.extend(rows(t, n, sh, own, rx, tx, out));
        }
        for h in handles {
            match h.join() {
                Ok(r) => results.extend(r),
                Err(p) => std::panic::resume_unwind(p),
            }
        }
    });
    // The first point, in raster order, where the serial loop would have stopped.
    let total = ctb_w * ctb_h;
    let mut cut = total;
    let mut first_err: Option<(usize, Error)> = None;
    for (ry, done, end) in results {
        let at = ry * ctb_w + done;
        match end {
            Ok(RowEnd::Done) => {}
            Ok(_) => cut = cut.min(at),
            Err(e) => {
                cut = cut.min(at);
                if first_err.as_ref().is_none_or(|(p, _)| at < *p) {
                    first_err = Some((at, e));
                }
            }
        }
    }
    for rs in cut..total {
        never_decoded(st, rs);
    }
    match first_err {
        Some((at, e)) if at == cut => Err(e),
        _ => Ok(()),
    }
}

/// Puts CTB `rs` back as the serial loop leaves a CTB it never reached.
fn never_decoded(st: &mut PicState, rs: usize) {
    let ctb = 1usize << st.log2_ctb;
    let (rx, ry) = (rs % st.ctb_w, rs / st.ctb_w);
    st.slice_addr[rs] = -1;
    st.ctb_slice[rs] = 0;
    st.ctb_done[rs] = false;
    st.sao[rs] = [SaoParams::default(); 3];
    st.ctb_filter[rs] = CtbFilterParams::default();
    let c4 = ctb >> 2;
    let (x0, x1) = (rx * c4, ((rx + 1) * c4).min(st.w4));
    for y4 in ry * c4..((ry + 1) * c4).min(st.h4) {
        let a = y4 * st.w4;
        st.pred_mode[a + x0..a + x1].fill(PRED_NONE);
        st.edges[a + x0..a + x1].fill(0);
        st.nz[a + x0..a + x1].fill(0);
        st.motion[a + x0..a + x1].fill(Motion::default());
    }
}

/// Resets CTB row `ry` of a private state as `PicState::reuse` resets a whole one: the maps a
/// CTB does not always write over.
fn reset_row(st: &mut PicState, ry: usize) {
    let w = st.ctb_w;
    let r4 = 1usize << (st.log2_ctb - 2);
    for rs in ry * w..((ry + 1) * w).min(st.slice_addr.len()) {
        st.slice_addr[rs] = -1;
        st.ctb_slice[rs] = 0;
        st.ctb_done[rs] = false;
        st.sao[rs] = [SaoParams::default(); 3];
        st.ctb_filter[rs] = CtbFilterParams::default();
    }
    let a = (ry * r4 * st.w4).min(st.pred_mode.len());
    let b = ((ry + 1) * r4 * st.w4).min(st.pred_mode.len());
    st.pred_mode[a..b].fill(PRED_NONE);
    st.edges[a..b].fill(0);
    st.nz[a..b].fill(0);
    st.motion[a..b].fill(Motion::default());
}
