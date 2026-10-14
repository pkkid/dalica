//! Lossless JPEG rotation and flips (`data-model.md` §6): one of the eight dihedral transforms
//! applied to a JPEG's quantised DCT coefficients, so a rotate-only Save never decodes and
//! re-encodes the pixels.
//!
//! Provenance: our own code, written from ITU-T Recommendation T.81 (ISO/IEC 10918-1): the marker
//! and segment syntax (§B.1, §B.2: frame header B.2.2, scan header B.2.3, DQT B.2.4.1, DHT
//! B.2.4.2, DRI B.2.4.4, the process each SOF marker names in Table B.1), the sample and block
//! layout of components (§A.1.1, §A.2: MCUs of interleaved and non-interleaved scans), the
//! baseline Huffman coding (Annex C, §F.1.2, §F.2.2) and optimal Huffman tables (Annex K.2). The
//! DCT-domain identities follow from the DCT's definition (§A.3.3): mirroring an 8 × 8 block left
//! to right negates its coefficients of odd horizontal frequency, top to bottom those of odd
//! vertical frequency, and transposing a block transposes its coefficients.
//!
//! - [`transform`]: the file turned as [`orient::apply`](crate::orient::apply) turns its pixels.
//!   Baseline and extended sequential Huffman files with 8-bit samples and any sampling factors
//!   are transformed. The quantisation tables are the file's own, never requantised: copied, or
//!   transposed entry by entry when the blocks are (a coefficient keeps its own quantiser). Every
//!   segment other than the frame header, tables, restart interval and scan is copied byte for
//!   byte (an EXIF orientation tag or thumbnail included: the caller updates them), and new
//!   Huffman tables are built for the new data. Everything else answers [`NotLossless`], and the caller falls back to the orientation
//!   tag: progressive, arithmetic-coded, 12-bit, lossless and hierarchical files; a transform that
//!   would move a partial MCU from the right or bottom edge to the leading one (the decoder would
//!   show its padding); and files with data after the end of the image (secondary images whose
//!   offsets and orientation would go stale).
//! - [`read`]: the quantised coefficients themselves ([`Coefficients`]);
//!   [`Coefficients::transformed`] the same transform in memory; [`encode`] coefficients back to
//!   a baseline file.
//!
//! The output is always one interleaved scan (or one scan of the only component) under an SOF0
//! frame header: slot 0 tables for the first component, slot 1 tables for the others.

mod huffman;
mod scan;

use std::ops::Range;

use huffman::Decoder;
use scan::Geometry;

use crate::limits::{Budget, Work};
use crate::{Error, Limits, Orientation, Result};

/// Why a file cannot be transformed losslessly; the caller falls back to the orientation tag.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotLossless {
    /// The transform would move the partial MCU column or row at the right or bottom edge to the
    /// left or top edge.
    #[error("the image size is not a whole number of blocks on the edge this transform moves")]
    PartialMcu,
    #[error("progressive JPEG")]
    Progressive,
    #[error("arithmetic-coded JPEG")]
    Arithmetic,
    #[error("{0}-bit JPEG")]
    Precision(u8),
    #[error("lossless-coded JPEG")]
    LosslessCoding,
    #[error("hierarchical JPEG")]
    Hierarchical,
    #[error("data after the end of the image")]
    TrailingData,
    /// A legal layout the transform does not handle (more than four components, a height defined
    /// after the scan, tables redefined between scans, a scan without its Huffman tables).
    #[error("{0}")]
    Layout(&'static str),
}

/// Why [`transform`], [`read`] or [`encode`] gave no result.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("not lossless here: {0}")]
    NotLossless(NotLossless),
    /// The file is damaged or too large to read (the decode limits).
    #[error(transparent)]
    Damaged(#[from] Error),
}

impl From<NotLossless> for Refusal {
    fn from(n: NotLossless) -> Refusal {
        Refusal::NotLossless(n)
    }
}

/// The quantised DCT coefficients of a JPEG frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coefficients {
    pub width: u32,
    pub height: u32,
    pub components: Vec<Component>,
}

/// One component's coefficients: blocks in rows, top to bottom, each block's 64 coefficients in
/// natural order (row by row: index `8 × v + u`, `u` the horizontal frequency).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    /// Component identifier, sampling factors and quantisation table, as in the frame header.
    pub id: u8,
    pub h: u8,
    pub v: u8,
    pub tq: u8,
    /// The plane's size in blocks: whole MCUs when the frame has several components (the
    /// padding blocks included), the blocks the samples cover when it has one (§A.2).
    pub blocks_wide: u32,
    pub blocks_high: u32,
    pub blocks: Vec<[i16; 64]>,
}

impl Coefficients {
    /// The largest horizontal and vertical sampling factors (at least 1).
    pub fn max_sampling(&self) -> (u8, u8) {
        let h = self.components.iter().map(|c| c.h).max().unwrap_or(1).max(1);
        let v = self.components.iter().map(|c| c.v).max().unwrap_or(1).max(1);
        (h, v)
    }

    /// The pixel size of the unit transforms move whole: the MCU, or one block when the frame
    /// has one component.
    pub fn unit(&self) -> (u32, u32) {
        if self.components.len() == 1 {
            return (8, 8);
        }
        let (h, v) = self.max_sampling();
        (8 * u32::from(h), 8 * u32::from(v))
    }

    /// Whether transform `o` keeps every partial unit at the right and bottom edges.
    pub fn allows(&self, o: Orientation) -> bool {
        let (uw, uh) = self.unit();
        let (whole_w, whole_h) = (self.width.is_multiple_of(uw), self.height.is_multiple_of(uh));
        match o.get() {
            2 | 8 => whole_w,
            4 | 6 => whole_h,
            3 | 7 => whole_w && whole_h,
            _ => true,
        }
    }

    /// The coefficients turned as [`orient::apply`](crate::orient::apply) turns pixels, block
    /// positions mapped within the padded planes. Partial units moved to a leading edge are not
    /// checked here (see [`Coefficients::allows`]).
    pub fn transformed(&self, o: Orientation) -> Result<Coefficients> {
        let mut budget = Budget::new(&Limits::default(), self.width, self.height);
        self.transformed_within(o, &mut budget)
    }

    fn transformed_within(&self, o: Orientation, budget: &mut Budget) -> Result<Coefficients> {
        let (width, height) = o.display_size(self.width, self.height);
        let mut components = Vec::with_capacity(self.components.len());
        for c in &self.components {
            let (bw, bh) = o.display_size(c.blocks_wide, c.blocks_high);
            let (h, v) = if o.swaps() { (c.v, c.h) } else { (c.h, c.v) };
            let len = plane_len(bw, bh)?;
            if c.blocks.len() != len {
                return Err(Error::Malformed(format!("{} blocks for a {} × {} plane", c.blocks.len(), c.blocks_wide, c.blocks_high)));
            }
            let mut blocks = budget.vec(len, [0i16; 64])?;
            for oy in 0..bh {
                for ox in 0..bw {
                    let (sx, sy) = stored_at(o.get(), ox, oy, bw, bh);
                    let from = c.blocks.get(sy as usize * c.blocks_wide as usize + sx as usize).ok_or(Error::OutOfRange)?;
                    if let Some(to) = blocks.get_mut(oy as usize * bw as usize + ox as usize) {
                        *to = transform_block(from, o);
                    }
                }
            }
            components.push(Component { id: c.id, h, v, tq: c.tq, blocks_wide: bw, blocks_high: bh, blocks });
        }
        Ok(Coefficients { width, height, components })
    }

    /// Checks that the planes have the sizes the frame implies.
    fn validate(&self) -> Result<(), Refusal> {
        if self.width == 0 || self.height == 0 || self.width > 65_535 || self.height > 65_535 {
            return Err(Error::Malformed(format!("frame size {} × {}", self.width, self.height)).into());
        }
        if self.components.is_empty() {
            return Err(Error::Malformed("a frame without components".into()).into());
        }
        if self.components.len() > 4 {
            return Err(NotLossless::Layout("more than four components").into());
        }
        let multi = self.components.len() > 1;
        if multi && self.components.iter().map(|c| u32::from(c.h) * u32::from(c.v)).sum::<u32>() > 10 {
            return Err(NotLossless::Layout("too many blocks in an MCU to interleave").into());
        }
        let (hmax, vmax) = self.max_sampling();
        for c in &self.components {
            if !(1..=4).contains(&c.h) || !(1..=4).contains(&c.v) || c.tq > 3 {
                return Err(Error::Malformed(format!("component {} sampling {} × {}, table {}", c.id, c.h, c.v, c.tq)).into());
            }
            let expected = plane_size(self.width, self.height, multi, (c.h, c.v), (hmax, vmax));
            if (c.blocks_wide, c.blocks_high) != expected || c.blocks.len() != plane_len(c.blocks_wide, c.blocks_high)? {
                return Err(Error::Malformed(format!(
                    "component {} plane {} × {} with {} blocks",
                    c.id,
                    c.blocks_wide,
                    c.blocks_high,
                    c.blocks.len()
                ))
                .into());
            }
        }
        Ok(())
    }
}

/// The size in blocks of a component's plane (§A.2): whole MCUs of the frame when it has several
/// components, the blocks the component's samples cover when it has one.
fn plane_size(width: u32, height: u32, multi: bool, (h, v): (u8, u8), (hmax, vmax): (u8, u8)) -> (u32, u32) {
    if multi {
        let mcux = width.div_ceil(8 * u32::from(hmax));
        let mcuy = height.div_ceil(8 * u32::from(vmax));
        (mcux * u32::from(h), mcuy * u32::from(v))
    } else {
        (width.div_ceil(8), height.div_ceil(8))
    }
}

fn plane_len(bw: u32, bh: u32) -> Result<usize> {
    usize::try_from(u64::from(bw) * u64::from(bh)).map_err(|_| Error::TooLarge { width: bw, height: bh })
}

/// The source position shown at `(x, y)` of a `w × h` result of orientation `o` (the mapping of
/// `orient::apply`, on blocks or on samples).
fn stored_at(o: u8, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
    match o {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, w - 1 - x),
        7 => (h - 1 - y, w - 1 - x),
        8 => (h - 1 - y, x),
        _ => (x, y),
    }
}

/// One block's coefficients under orientation `o`: transposed when `o` swaps the axes, then the
/// odd horizontal frequencies negated for a left–right mirror and the odd vertical ones for a
/// top–bottom mirror (the same mapping as [`stored_at`] on the block's samples).
fn transform_block(block: &[i16; 64], o: Orientation) -> [i16; 64] {
    let (swap, flip_u, flip_v) = match o.get() {
        2 => (false, true, false),
        3 => (false, true, true),
        4 => (false, false, true),
        5 => (true, false, false),
        6 => (true, true, false),
        7 => (true, true, true),
        8 => (true, false, true),
        _ => (false, false, false),
    };
    let mut out = [0i16; 64];
    for v in 0..8 {
        for u in 0..8 {
            let from = if swap { block[u * 8 + v] } else { block[v * 8 + u] };
            let negate = (flip_u && u % 2 == 1) != (flip_v && v % 2 == 1);
            // −(−32768) cannot occur in a baseline file (|AC| ≤ 1023), and saturates if it does.
            out[v * 8 + u] = if negate { from.saturating_neg() } else { from };
        }
    }
    out
}

/// A segment of the source file, from its marker.
#[derive(Clone, Debug)]
enum Piece {
    /// Copied byte for byte.
    Copy(Range<usize>),
    /// Quantisation tables: copied byte for byte, or with every table transposed when the
    /// transform transposes the blocks (a coefficient keeps its own quantiser).
    Tables(Range<usize>),
    /// The frame header, written anew.
    Frame,
}

/// A source file read for transforming.
struct Parsed {
    coefficients: Coefficients,
    /// The segments before the first scan, in file order.
    head: Vec<Piece>,
    /// The segments after the first scan (between scans or before EOI), in file order.
    tail: Vec<Range<usize>>,
    /// The restart interval in effect at the first scan.
    restart: u16,
    budget: Budget,
}

/// The frame process an SOF marker names (Table B.1), when it is not baseline or extended
/// sequential Huffman.
fn unsupported_process(marker: u8) -> Option<NotLossless> {
    match marker {
        0xc0 | 0xc1 => None,
        0xc2 => Some(NotLossless::Progressive),
        0xc3 => Some(NotLossless::LosslessCoding),
        0xc5..=0xc7 | 0xcd..=0xcf => Some(NotLossless::Hierarchical),
        _ => Some(NotLossless::Arithmetic),
    }
}

fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    match b.get(at..at.saturating_add(2)) {
        Some(&[hi, lo]) => Ok(u16::from_be_bytes([hi, lo])),
        _ => Err(Error::Truncated),
    }
}

/// The position of the next marker at or after `pos` (skipping stuffed bytes, restart markers
/// and fill bytes), or the end of the data.
fn next_marker(b: &[u8], mut pos: usize) -> usize {
    while let (Some(&x), Some(&y)) = (b.get(pos), b.get(pos + 1)) {
        if x == 0xff && !matches!(y, 0x00 | 0xd0..=0xd7 | 0xff) {
            return pos;
        }
        pos += 1;
    }
    b.len()
}

/// Reads the frame header (§B.2.2) and allocates the coefficient planes.
fn frame(data: &[u8], limits: &Limits) -> Result<(Coefficients, Budget), Refusal> {
    let precision = *data.first().ok_or(Error::Truncated)?;
    let height = u16_at(data, 1)?;
    let width = u16_at(data, 3)?;
    let count = usize::from(*data.get(5).ok_or(Error::Truncated)?);
    if precision != 8 {
        return Err(NotLossless::Precision(precision).into());
    }
    if height == 0 {
        return Err(NotLossless::Layout("the height is defined after the scan").into());
    }
    if width == 0 || count == 0 {
        return Err(Error::Malformed(format!("frame {width} × {height} with {count} components")).into());
    }
    if count > 4 {
        return Err(NotLossless::Layout("more than four components").into());
    }
    if data.len() != 6 + 3 * count {
        return Err(Error::Malformed("frame header length".into()).into());
    }
    let (width, height) = (u32::from(width), u32::from(height));
    limits.check_size(width, height, 1)?;
    let mut components = Vec::with_capacity(count);
    for i in 0..count {
        let at = 6 + 3 * i;
        let (id, hv, tq) = match data.get(at..at + 3) {
            Some(&[id, hv, tq]) => (id, hv, tq),
            _ => return Err(Error::Truncated.into()),
        };
        let (h, v) = (hv >> 4, hv & 15);
        if !(1..=4).contains(&h) || !(1..=4).contains(&v) || tq > 3 {
            return Err(Error::Malformed(format!("component {id} sampling {h} × {v}, table {tq}")).into());
        }
        if components.iter().any(|c: &Component| c.id == id) {
            return Err(Error::Malformed(format!("component {id} repeats")).into());
        }
        components.push(Component { id, h, v, tq, blocks_wide: 0, blocks_high: 0, blocks: Vec::new() });
    }
    let mut c = Coefficients { width, height, components };
    let (hmax, vmax) = c.max_sampling();
    let multi = count > 1;
    let mut budget = Budget::new(limits, width, height);
    for comp in &mut c.components {
        let (bw, bh) = plane_size(width, height, multi, (comp.h, comp.v), (hmax, vmax));
        comp.blocks = budget.vec(plane_len(bw, bh)?, [0i16; 64])?;
        (comp.blocks_wide, comp.blocks_high) = (bw, bh);
    }
    Ok((c, budget))
}

/// Walks the file from SOI to EOI (§B.1, §B.2), reading every scan into coefficients. With
/// `plan`, a transform that the frame does not allow is refused right after the frame header,
/// before any scan is decoded.
fn parse(b: &[u8], limits: &Limits, plan: Option<Orientation>) -> Result<Parsed, Refusal> {
    if !b.starts_with(&[0xff, 0xd8]) {
        return Err(if b.len() < 2 { Error::Truncated } else { Error::Unknown }.into());
    }
    let mut pos = 2;
    let mut work = Work::new(limits.max_segments, "JPEG segments");
    let mut tables: [[Option<Decoder>; 4]; 2] = Default::default();
    let mut restart: u16 = 0;
    let mut first_restart: u16 = 0;
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut frame_seen: Option<(Coefficients, Budget)> = None;
    let mut scanned: Vec<bool> = Vec::new();
    let mut scans = 0usize;
    loop {
        work.step()?;
        if pos >= b.len() {
            // A missing EOI after complete scans is accepted; anything less is truncated.
            if scans > 0 && scanned.iter().all(|&s| s) {
                break;
            }
            return Err(Error::Truncated.into());
        }
        if b.get(pos) != Some(&0xff) {
            return Err(Error::Malformed(format!("no marker at byte {pos}")).into());
        }
        while b.get(pos + 1) == Some(&0xff) {
            pos += 1;
        }
        let start = pos;
        let marker = *b.get(pos + 1).ok_or(Error::Truncated)?;
        pos += 2;
        match marker {
            0x01 | 0xd0..=0xd7 => continue,
            0xd8 => return Err(Error::Malformed("a second start of image".into()).into()),
            0xd9 => {
                if scans == 0 || !scanned.iter().all(|&s| s) {
                    return Err(Error::Malformed("end of image before every component was coded".into()).into());
                }
                if b.len() > pos {
                    return Err(NotLossless::TrailingData.into());
                }
                break;
            }
            _ => {}
        }
        let len = usize::from(u16_at(b, pos)?);
        if len < 2 {
            return Err(Error::Malformed(format!("segment length {len}")).into());
        }
        let data = b.get(pos + 2..pos + len).ok_or(Error::Truncated)?;
        let end = pos + len;
        match marker {
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                if let Some(reason) = unsupported_process(marker) {
                    return Err(reason.into());
                }
                if frame_seen.is_some() {
                    return Err(Error::Malformed("a second frame header".into()).into());
                }
                let (c, budget) = frame(data, limits)?;
                if let Some(o) = plan
                    && !c.allows(o)
                {
                    return Err(NotLossless::PartialMcu.into());
                }
                scanned = vec![false; c.components.len()];
                frame_seen = Some((c, budget));
                head.push(Piece::Frame);
            }
            0xc4 => read_tables(data, &mut tables)?,
            0xcc => return Err(NotLossless::Arithmetic.into()),
            0xdd => restart = u16_at(data, 0)?,
            0xdc => return Err(NotLossless::Layout("the height is defined after the scan").into()),
            0xde | 0xdf => return Err(NotLossless::Hierarchical.into()),
            0xdb if scans > 0 => return Err(NotLossless::Layout("quantisation tables redefined between scans").into()),
            0xdb => {
                transposed_tables(data)?;
                head.push(Piece::Tables(start..end));
            }
            0xda => {
                let (c, _) = frame_seen.as_mut().ok_or_else(|| Error::Malformed("scan data before a frame header".into()))?;
                let scan = scan_header(data, c, &mut scanned)?;
                let mut decoders = Vec::with_capacity(scan.len());
                for &(_, td, ta) in &scan {
                    match (tables[0].get(td).and_then(Option::as_ref), tables[1].get(ta).and_then(Option::as_ref)) {
                        (Some(dc), Some(ac)) => decoders.push((dc, ac)),
                        _ => return Err(NotLossless::Layout("a scan uses Huffman tables the file does not define").into()),
                    }
                }
                let indices: Vec<usize> = scan.iter().map(|&(i, _, _)| i).collect();
                let geometry = Geometry::new(c, &indices, restart)?;
                let after = scan::decode(b, end, c, &geometry, &decoders)?;
                if scans == 0 {
                    first_restart = restart;
                }
                scans += 1;
                pos = next_marker(b, after);
                continue;
            }
            _ => {
                if scans == 0 {
                    head.push(Piece::Copy(start..end));
                } else {
                    tail.push(start..end);
                }
            }
        }
        pos = end;
    }
    let (coefficients, budget) = frame_seen.ok_or(Error::Truncated)?;
    Ok(Parsed { coefficients, head, tail, restart: first_restart, budget })
}

/// A DQT segment's body (§B.2.4.1) with every table transposed: the entry for horizontal
/// frequency `u` and vertical frequency `v` moves to `(v, u)`, in zig-zag order as stored. 8- and
/// 16-bit entries; a malformed segment is `Err`.
fn transposed_tables(data: &[u8]) -> Result<Vec<u8>> {
    let mut natural_to_zigzag = [0usize; 64];
    for (k, &n) in scan::ZIGZAG.iter().enumerate() {
        natural_to_zigzag[usize::from(n)] = k;
    }
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    while at < data.len() {
        let pqtq = *data.get(at).ok_or(Error::Truncated)?;
        let (precision, id) = (pqtq >> 4, pqtq & 15);
        if precision > 1 || id > 3 {
            return Err(Error::Malformed(format!("quantisation table precision {precision}, slot {id}")));
        }
        let size = if precision == 0 { 1 } else { 2 };
        let table = data.get(at + 1..at + 1 + 64 * size).ok_or(Error::Truncated)?;
        out.push(pqtq);
        for &n in &scan::ZIGZAG {
            // The entry stored at natural index n = 8v + u comes from (u, v) transposed.
            let n = usize::from(n);
            let k = natural_to_zigzag[(n % 8) * 8 + n / 8];
            out.extend_from_slice(table.get(k * size..(k + 1) * size).ok_or(Error::Truncated)?);
        }
        at += 1 + 64 * size;
    }
    Ok(out)
}

/// Reads a DHT segment's tables (§B.2.4.2) into their slots.
fn read_tables(data: &[u8], tables: &mut [[Option<Decoder>; 4]; 2]) -> Result<()> {
    let mut at = 0;
    while at < data.len() {
        let tcth = *data.get(at).ok_or(Error::Truncated)?;
        let (class, id) = (usize::from(tcth >> 4), usize::from(tcth & 15));
        let mut bits = [0u8; 16];
        bits.copy_from_slice(data.get(at + 1..at + 17).ok_or(Error::Truncated)?);
        let n: usize = bits.iter().map(|&x| usize::from(x)).sum();
        let values = data.get(at + 17..at + 17 + n).ok_or(Error::Truncated)?;
        let slot =
            tables.get_mut(class).and_then(|c| c.get_mut(id)).ok_or_else(|| Error::Malformed(format!("Huffman table class {class}, slot {id}")))?;
        *slot = Some(Decoder::new(&bits, values)?);
        at += 17 + n;
    }
    Ok(())
}

/// Reads a scan header (§B.2.3): per scan component its frame index and (DC, AC) table slots.
fn scan_header(data: &[u8], frame: &Coefficients, scanned: &mut [bool]) -> Result<Vec<(usize, usize, usize)>> {
    let n = usize::from(*data.first().ok_or(Error::Truncated)?);
    if !(1..=4).contains(&n) || data.len() != 4 + 2 * n {
        return Err(Error::Malformed(format!("scan header with {n} components")));
    }
    let mut scan = Vec::with_capacity(n);
    let mut blocks = 0u32;
    for i in 0..n {
        let (id, tdta) = match data.get(1 + 2 * i..3 + 2 * i) {
            Some(&[id, t]) => (id, t),
            _ => return Err(Error::Truncated),
        };
        let index =
            frame.components.iter().position(|c| c.id == id).ok_or_else(|| Error::Malformed(format!("scan component {id} not in the frame")))?;
        let done = scanned.get_mut(index).ok_or(Error::OutOfRange)?;
        if *done {
            return Err(Error::Malformed(format!("component {id} coded twice")));
        }
        *done = true;
        if let Some(c) = frame.components.get(index) {
            blocks += u32::from(c.h) * u32::from(c.v);
        }
        scan.push((index, usize::from(tdta >> 4), usize::from(tdta & 15)));
    }
    if n > 1 && blocks > 10 {
        return Err(Error::Malformed(format!("{blocks} blocks in an MCU")));
    }
    let spectral = data.get(1 + 2 * n..4 + 2 * n);
    if spectral != Some(&[0, 63, 0][..]) {
        return Err(Error::Malformed(format!("sequential scan with spectral selection {spectral:?}")));
    }
    Ok(scan)
}

/// Reads a JPEG's quantised coefficients under `limits`.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<Coefficients, Refusal> {
    Ok(parse(bytes, limits, None)?.coefficients)
}

/// The JPEG in `bytes` turned as [`orient::apply`](crate::orient::apply) turns its pixels with
/// `o`, without decoding them: the decoded result is the source's decoded pixels transformed, up
/// to the rounding of the decoder's inverse DCT and upsampling.
pub fn transform(bytes: &[u8], o: Orientation, limits: &Limits) -> Result<Vec<u8>, Refusal> {
    let Parsed { coefficients, head, tail, restart, mut budget } = parse(bytes, limits, Some(o))?;
    let turned = coefficients.transformed_within(o, &mut budget)?;
    drop(coefficients);
    let mut out = Vec::new();
    out.try_reserve(bytes.len().saturating_add(1024)).map_err(|_| Error::TooLarge { width: turned.width, height: turned.height })?;
    out.extend_from_slice(&[0xff, 0xd8]);
    let mut wrote_frame = false;
    for piece in &head {
        match piece {
            Piece::Copy(r) => out.extend_from_slice(bytes.get(r.clone()).ok_or(Error::OutOfRange)?),
            Piece::Tables(r) => {
                let seg = bytes.get(r.clone()).ok_or(Error::OutOfRange)?;
                if o.swaps() {
                    let body = transposed_tables(seg.get(4..).ok_or(Error::Truncated)?)?;
                    segment(&mut out, 0xdb, &body)?;
                } else {
                    out.extend_from_slice(seg);
                }
            }
            Piece::Frame => {
                write_frame(&mut out, &turned)?;
                wrote_frame = true;
            }
        }
    }
    if !wrote_frame {
        write_frame(&mut out, &turned)?;
    }
    write_scan(&mut out, &turned, restart)?;
    for r in &tail {
        out.extend_from_slice(bytes.get(r.clone()).ok_or(Error::OutOfRange)?);
    }
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

/// A baseline JPEG of `coefficients`: SOI, then `segments` (whole marker segments, such as the
/// DQT tables the coefficients were quantised with and any APP segments) byte for byte, the frame
/// header, optimal Huffman tables, one scan with restart markers every `restart_interval` MCUs (0
/// for none), EOI.
pub fn encode(coefficients: &Coefficients, segments: &[u8], restart_interval: u16) -> Result<Vec<u8>, Refusal> {
    let mut out = vec![0xff, 0xd8];
    out.extend_from_slice(segments);
    write_frame(&mut out, coefficients)?;
    write_scan(&mut out, coefficients, restart_interval)?;
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

fn segment(out: &mut Vec<u8>, marker: u8, body: &[u8]) -> Result<()> {
    let len = u16::try_from(body.len() + 2).map_err(|_| Error::Malformed("segment too long".into()))?;
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    Ok(())
}

/// An SOF0 frame header (§B.2.2) for `c`.
fn write_frame(out: &mut Vec<u8>, c: &Coefficients) -> Result<(), Refusal> {
    c.validate()?;
    let mut body = vec![8];
    body.extend_from_slice(&(c.height as u16).to_be_bytes());
    body.extend_from_slice(&(c.width as u16).to_be_bytes());
    body.push(c.components.len() as u8);
    for comp in &c.components {
        body.extend_from_slice(&[comp.id, (comp.h << 4) | comp.v, comp.tq]);
    }
    segment(out, 0xc0, &body)?;
    Ok(())
}

/// DRI (when restarting), DHT and SOS, then the entropy-coded data of one scan over every
/// component of `c`.
fn write_scan(out: &mut Vec<u8>, c: &Coefficients, restart: u16) -> Result<(), Refusal> {
    c.validate()?;
    let all: Vec<usize> = (0..c.components.len()).collect();
    let geometry = Geometry::new(c, &all, restart)?;
    let tables = scan::optimal_tables(c, &geometry)?;
    if restart > 0 {
        segment(out, 0xdd, &restart.to_be_bytes())?;
    }
    let mut dht = Vec::new();
    for class in 0..2u8 {
        for (slot, (dc, ac)) in tables.iter().enumerate() {
            let table = if class == 0 { dc } else { ac };
            dht.push((class << 4) | slot as u8);
            dht.extend_from_slice(&table.bits);
            dht.extend_from_slice(&table.values);
        }
    }
    segment(out, 0xc4, &dht)?;
    let mut sos = vec![c.components.len() as u8];
    for (i, comp) in c.components.iter().enumerate() {
        let slot = i.min(1) as u8;
        sos.extend_from_slice(&[comp.id, (slot << 4) | slot]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    segment(out, 0xda, &sos)?;
    scan::encode(out, c, &geometry, &tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(seed: i16) -> [i16; 64] {
        std::array::from_fn(|i| (i as i16 * 7 + seed) % 23 - 11)
    }

    #[test]
    fn block_transforms_compose_like_orientations() {
        let b = block(3);
        let o = |v| Orientation::new(v).unwrap();
        // Four quarter turns, two half turns, and every mirror twice are the identity.
        let quarter = (0..4).fold(b, |acc, _| transform_block(&acc, o(6)));
        assert_eq!(quarter, b);
        for v in [2, 3, 4, 5, 7] {
            assert_eq!(transform_block(&transform_block(&b, o(v)), o(v)), b, "{v}");
        }
        // 6 then 8 undo each other; 5 is 2 after 6.
        assert_eq!(transform_block(&transform_block(&b, o(6)), o(8)), b);
        assert_eq!(transform_block(&transform_block(&b, o(6)), o(2)), transform_block(&b, o(5)));
    }

    /// A 4:2:0 frame coded as three non-interleaved scans (one per component, each with its own
    /// tables) reads back exactly and transforms like any other.
    #[test]
    fn non_interleaved_scans_are_read() {
        let plane = |id: u8, h: u8, v: u8, bw: u32, bh: u32| Component {
            id,
            h,
            v,
            tq: u8::from(id > 1),
            blocks_wide: bw,
            blocks_high: bh,
            blocks: (0..bw * bh).map(|i| std::array::from_fn(|k| if k < 12 { ((i as usize * 7 + k * 3) % 9) as i16 - 4 } else { 0 })).collect(),
        };
        let c = Coefficients { width: 32, height: 32, components: vec![plane(1, 2, 2, 4, 4), plane(2, 1, 1, 2, 2), plane(3, 1, 1, 2, 2)] };
        let mut file = vec![0xff, 0xd8];
        let mut dqt = vec![0u8];
        dqt.extend([1u8; 64]);
        dqt.push(1);
        dqt.extend([2u8; 64]);
        segment(&mut file, 0xdb, &dqt).unwrap();
        write_frame(&mut file, &c).unwrap();
        for (i, comp) in c.components.iter().enumerate() {
            let g = Geometry::new(&c, &[i], 0).unwrap();
            let tables = scan::optimal_tables(&c, &g).unwrap();
            let (dc, ac) = &tables[0];
            let mut dht = vec![0x00];
            dht.extend_from_slice(&dc.bits);
            dht.extend_from_slice(&dc.values);
            dht.push(0x10);
            dht.extend_from_slice(&ac.bits);
            dht.extend_from_slice(&ac.values);
            segment(&mut file, 0xc4, &dht).unwrap();
            segment(&mut file, 0xda, &[1, comp.id, 0x00, 0, 63, 0]).unwrap();
            scan::encode(&mut file, &c, &g, &tables).unwrap();
        }
        file.extend_from_slice(&[0xff, 0xd9]);
        assert_eq!(read(&file, &Limits::default()), Ok(c.clone()));
        let o6 = Orientation::new(6).unwrap();
        let out = transform(&file, o6, &Limits::default()).unwrap();
        assert_eq!(read(&out, &Limits::default()), Ok(c.transformed(o6).unwrap()));
        // A component coded twice is damaged.
        let at = file.windows(2).rposition(|w| w == [0xff, 0xda]).unwrap();
        let mut twice = file.clone();
        twice[at + 5] = 1;
        assert!(matches!(read(&twice, &Limits::default()), Err(Refusal::Damaged(_))));
    }

    #[test]
    fn edges_that_move_must_be_whole() {
        let c = |w, h, n| Coefficients {
            width: w,
            height: h,
            components: (0..n)
                .map(|i| Component {
                    id: i,
                    h: if i == 0 { 2 } else { 1 },
                    v: if i == 0 { 2 } else { 1 },
                    tq: 0,
                    blocks_wide: 0,
                    blocks_high: 0,
                    blocks: vec![],
                })
                .collect(),
        };
        let o = |v| Orientation::new(v).unwrap();
        let ragged_w = c(40, 32, 3); // 4:2:0, MCU 16 × 16
        assert_eq!((1..=8).filter(|&v| ragged_w.allows(o(v))).collect::<Vec<_>>(), vec![1, 4, 5, 6]);
        let ragged_h = c(32, 40, 3);
        assert_eq!((1..=8).filter(|&v| ragged_h.allows(o(v))).collect::<Vec<_>>(), vec![1, 2, 5, 8]);
        // One component: blocks of 8 whatever its sampling factors say.
        assert!((1..=8).all(|v| c(40, 24, 1).allows(o(v))));
    }
}
