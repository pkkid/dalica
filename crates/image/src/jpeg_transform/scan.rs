//! Baseline scans read into and written from quantised coefficients, written from ITU-T T.81:
//! the MCU and block order of interleaved and non-interleaved scans (§A.2.2, §A.2.3), restart
//! intervals (§B.2.4.4, §F.1.2.4), and the Huffman coding of DC differences and AC run-lengths
//! (§F.1.2.1, §F.1.2.2 for encoding; §F.2.2.1, §F.2.2.2 for decoding). Coefficients are stored
//! in natural (row-major) order; the zig-zag order is Figure A.6.

use super::huffman::{BitReader, BitWriter, Decoder, Encoder, category, extend};
use super::{Coefficients, Refusal};
use crate::{Error, Result};

/// The natural-order index of each zig-zag position (Figure A.6).
pub(super) const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Largest DC difference category of an 8-bit frame (§F.1.2.1.1).
const MAX_DC_CATEGORY: u32 = 11;
/// Largest AC magnitude category of an 8-bit frame (§F.1.2.2.1).
const MAX_AC_CATEGORY: u32 = 10;

/// One component of a scan: its index in the frame and its sampling inside an MCU.
#[derive(Clone, Copy, Debug)]
pub(super) struct ScanComponent {
    pub(super) index: usize,
    pub(super) h: u32,
    pub(super) v: u32,
    /// Blocks per row of the component's plane.
    pub(super) stride: u32,
}

/// The order a scan visits blocks in.
#[derive(Clone, Debug)]
pub(super) struct Geometry {
    pub(super) components: Vec<ScanComponent>,
    /// MCUs per row and per column.
    pub(super) units: (u32, u32),
    /// MCUs per restart interval; 0 for none.
    pub(super) restart: u16,
}

impl Geometry {
    /// The geometry of a scan over `scan` (indices into the frame's components) of `frame`
    /// (§A.2): one component is non-interleaved (one block per MCU, over the blocks its samples
    /// cover), several are interleaved over the frame's MCU grid.
    pub(super) fn new(frame: &Coefficients, scan: &[usize], restart: u16) -> Result<Geometry> {
        let (hmax, vmax) = frame.max_sampling();
        let mut components = Vec::with_capacity(scan.len());
        let single = scan.len() == 1;
        let mut units = (0, 0);
        for &index in scan {
            let c = frame.components.get(index).ok_or_else(|| Error::Malformed("scan component outside the frame".into()))?;
            if single {
                // The component's own samples, rounded up to whole blocks (§A.1.1).
                let cw = (u64::from(frame.width) * u64::from(c.h)).div_ceil(u64::from(hmax));
                let ch = (u64::from(frame.height) * u64::from(c.v)).div_ceil(u64::from(vmax));
                units = (cw.div_ceil(8) as u32, ch.div_ceil(8) as u32);
                components.push(ScanComponent { index, h: 1, v: 1, stride: c.blocks_wide });
            } else {
                units = (frame.width.div_ceil(8 * u32::from(hmax)), frame.height.div_ceil(8 * u32::from(vmax)));
                components.push(ScanComponent { index, h: u32::from(c.h), v: u32::from(c.v), stride: c.blocks_wide });
            }
        }
        Ok(Geometry { components, units, restart })
    }

    /// Visits the scan's blocks in coding order (§A.2.3): a [`Step::Block`] per block, and a
    /// [`Step::Restart`] before each restart interval after the first.
    pub(super) fn walk(&self, mut f: impl FnMut(Step) -> Result<(), Refusal>) -> Result<(), Refusal> {
        let interval = u64::from(self.restart);
        let mut mcu: u64 = 0;
        let mut rst: u8 = 0;
        for my in 0..self.units.1 {
            for mx in 0..self.units.0 {
                if interval > 0 && mcu > 0 && mcu.is_multiple_of(interval) {
                    f(Step::Restart(rst))?;
                    rst = rst.wrapping_add(1) & 7;
                }
                for (slot, c) in self.components.iter().enumerate() {
                    for y in 0..c.v {
                        for x in 0..c.h {
                            let bx = u64::from(mx) * u64::from(c.h) + u64::from(x);
                            let by = u64::from(my) * u64::from(c.v) + u64::from(y);
                            let at = usize::try_from(by * u64::from(c.stride) + bx).map_err(|_| Error::OutOfRange)?;
                            f(Step::Block { slot, comp: c.index, at })?;
                        }
                    }
                }
                mcu += 1;
            }
        }
        Ok(())
    }
}

/// One step of a scan's walk.
#[derive(Clone, Copy, Debug)]
pub(super) enum Step {
    /// Restart marker RST`n` comes next.
    Restart(u8),
    /// The next block: the scan component's slot, its index in the frame and the block's index in
    /// that component's plane.
    Block { slot: usize, comp: usize, at: usize },
}

/// Decodes one scan's entropy-coded data starting at `pos` into `frame`'s coefficient planes, the
/// tables per scan component given as (DC, AC). Returns the position after the last byte read.
pub(super) fn decode(
    data: &[u8],
    pos: usize,
    frame: &mut Coefficients,
    geometry: &Geometry,
    tables: &[(&Decoder, &Decoder)],
) -> Result<usize, Refusal> {
    let mut reader = BitReader::new(data, pos);
    let mut pred = vec![0i32; geometry.components.len()];
    geometry.walk(|step| {
        match step {
            Step::Restart(n) => {
                reader.restart(n)?;
                pred.iter_mut().for_each(|p| *p = 0);
            }
            Step::Block { slot, comp, at } => {
                let (dc, ac) = tables.get(slot).ok_or(Error::OutOfRange)?;
                let pred = pred.get_mut(slot).ok_or(Error::OutOfRange)?;
                let block = frame.components.get_mut(comp).and_then(|c| c.blocks.get_mut(at)).ok_or(Error::OutOfRange)?;
                decode_block(&mut reader, dc, ac, pred, block)?;
            }
        }
        Ok(())
    })?;
    Ok(reader.pos())
}

fn decode_block(r: &mut BitReader, dc: &Decoder, ac: &Decoder, pred: &mut i32, block: &mut [i16; 64]) -> Result<()> {
    let t = u32::from(r.decode(dc)?);
    if t > MAX_DC_CATEGORY {
        return Err(Error::Malformed(format!("DC difference category {t}")));
    }
    let diff = extend(r.receive(t)?, t);
    *pred += diff;
    block[0] = i16::try_from(*pred).map_err(|_| Error::Malformed("DC coefficient out of range".into()))?;
    let mut k = 1usize;
    while k < 64 {
        let rs = r.decode(ac)?;
        let (run, size) = (usize::from(rs >> 4), u32::from(rs & 15));
        if size == 0 {
            if run == 15 {
                k += 16;
                continue;
            }
            break;
        }
        if size > MAX_AC_CATEGORY {
            return Err(Error::Malformed(format!("AC coefficient category {size}")));
        }
        k += run;
        let zz = *ZIGZAG.get(k).ok_or_else(|| Error::Malformed("AC coefficients past the end of a block".into()))?;
        block[usize::from(zz)] = extend(r.receive(size)?, size) as i16;
        k += 1;
    }
    Ok(())
}

/// What one coded block emits: a symbol of the DC or AC table and the extra bits after it.
fn block_symbols(block: &[i16; 64], pred: &mut i32, mut emit: impl FnMut(bool, u8, u32, u32)) -> Result<()> {
    let dc = i32::from(block[0]);
    let diff = dc - *pred;
    *pred = dc;
    let t = category(diff);
    if t > MAX_DC_CATEGORY {
        return Err(Error::Malformed(format!("a DC difference of {diff} does not fit a baseline JPEG")));
    }
    emit(false, t as u8, extra_bits(diff, t), t);
    let mut run = 0u8;
    for &zz in ZIGZAG.iter().skip(1) {
        let v = i32::from(block[usize::from(zz)]);
        if v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            emit(true, 0xf0, 0, 0);
            run -= 16;
        }
        let s = category(v);
        if s > MAX_AC_CATEGORY {
            return Err(Error::Malformed(format!("an AC coefficient of {v} does not fit a baseline JPEG")));
        }
        emit(true, (run << 4) | s as u8, extra_bits(v, s), s);
        run = 0;
    }
    if run > 0 {
        emit(true, 0x00, 0, 0);
    }
    Ok(())
}

/// The `size` low bits that code `v` (§F.1.2.1.1): `v` itself, or `v − 1` when negative.
fn extra_bits(v: i32, size: u32) -> u32 {
    let bits = if v < 0 { v - 1 } else { v } as u32;
    if size == 0 { 0 } else { bits & ((1u32 << size) - 1) }
}

/// The optimal (DC, AC) tables for each table slot: slot 0 for the first component, slot 1 for
/// every other one.
pub(super) fn optimal_tables(frame: &Coefficients, geometry: &Geometry) -> Result<Vec<(Encoder, Encoder)>, Refusal> {
    let slots = if geometry.components.len() > 1 { 2 } else { 1 };
    let mut freq = vec![([0u64; 256], [0u64; 256]); slots];
    let mut pred = vec![0i32; geometry.components.len()];
    geometry.walk(|step| {
        match step {
            Step::Restart(_) => pred.iter_mut().for_each(|p| *p = 0),
            Step::Block { slot, comp, at } => {
                let block = frame.components.get(comp).and_then(|c| c.blocks.get(at)).ok_or(Error::OutOfRange)?;
                let pred = pred.get_mut(slot).ok_or(Error::OutOfRange)?;
                let (dc, ac) = freq.get_mut(slot.min(1)).ok_or(Error::OutOfRange)?;
                block_symbols(block, pred, |is_ac, symbol, _, _| {
                    let f = if is_ac { &mut ac[usize::from(symbol)] } else { &mut dc[usize::from(symbol)] };
                    *f = f.saturating_add(1);
                })?;
            }
        }
        Ok(())
    })?;
    Ok(freq.iter().map(|(dc, ac)| (Encoder::optimal(dc), Encoder::optimal(ac))).collect())
}

/// Writes one scan's entropy-coded data (with restart markers) for `geometry` over `frame`.
pub(super) fn encode(out: &mut Vec<u8>, frame: &Coefficients, geometry: &Geometry, tables: &[(Encoder, Encoder)]) -> Result<(), Refusal> {
    let mut pred = vec![0i32; geometry.components.len()];
    let mut w = BitWriter::new(out);
    geometry.walk(|step| {
        match step {
            Step::Restart(n) => {
                w.marker(0xd0 + n);
                pred.iter_mut().for_each(|p| *p = 0);
            }
            Step::Block { slot, comp, at } => {
                let block = frame.components.get(comp).and_then(|c| c.blocks.get(at)).ok_or(Error::OutOfRange)?;
                let pred = pred.get_mut(slot).ok_or(Error::OutOfRange)?;
                let (dc, ac) = tables.get(slot.min(1)).ok_or(Error::OutOfRange)?;
                let mut missing = false;
                block_symbols(block, pred, |is_ac, symbol, bits, len| {
                    let (code, size) = if is_ac { ac.code(symbol) } else { dc.code(symbol) };
                    missing |= size == 0;
                    w.put(u32::from(code), u32::from(size));
                    w.put(bits, len);
                })?;
                if missing {
                    return Err(Error::Malformed("a symbol without a Huffman code".into()).into());
                }
            }
        }
        Ok(())
    })?;
    w.align();
    Ok(())
}
