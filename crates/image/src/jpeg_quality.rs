//! The settings a JPEG was saved with, read back from its own header, so a Save can write it again
//! the way it came (`data-model.md` §6): the quality from its quantisation tables and the chroma
//! subsampling from its frame header.
//!
//! Adapted from PhotoCraft (MIT OR Apache-2.0), `crates/codecs/src/source_settings.rs`
//! (`jpeg_quality`, `scaled`, `table_error`, `jpeg_tables`, `read_dqt`) at bb17dd55. Changes: the
//! tables compared are the ones the frame header's components select (not slots 0 and 1); the
//! header must be complete up to the first scan; a table holding a zero (illegal in T.81) gives no
//! estimate; the segment walk is capped; the chroma subsampling is read as well; no WebP.
//!
//! Most encoders scale the example tables of ITU-T T.81 Annex K (K.1 luminance, K.2 chrominance)
//! by the Independent JPEG Group's quality curve, so the quality whose scaled tables match the
//! file's best is the one it was saved at (exact for such tables; the closest fit for a camera's
//! own tables). Every input is hostile: every index is checked and a malformed or truncated header
//! gives `None`.

use crate::encode::Subsampling;

/// ITU-T T.81 Annex K.1, the example luminance quantisation table, in natural (row) order.
const ANNEX_K_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, //
    12, 12, 14, 19, 26, 58, 60, 55, //
    14, 13, 16, 24, 40, 57, 69, 56, //
    14, 17, 22, 29, 51, 87, 80, 62, //
    18, 22, 37, 56, 68, 109, 103, 77, //
    24, 35, 55, 64, 81, 104, 113, 92, //
    49, 64, 78, 87, 103, 121, 120, 101, //
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// ITU-T T.81 Annex K.2, the example chrominance quantisation table, in natural order.
const ANNEX_K_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, //
    18, 21, 26, 66, 99, 99, 99, 99, //
    24, 26, 56, 99, 99, 99, 99, 99, //
    47, 66, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99, //
    99, 99, 99, 99, 99, 99, 99, 99,
];

/// The natural-order index of the k-th coefficient in zig-zag order (T.81 Figure A.6): DQT
/// stores its 64 values in this order.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Most segments looked at before the first scan (as `Limits::max_segments`).
const MAX_SEGMENTS: usize = 4096;
/// Most components a frame header may declare (T.81 B.2.2 allows 255; Dalica reads up to 4).
const MAX_COMPONENTS: usize = 4;

/// How a JPEG's chroma is sampled, from its frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// One component.
    Grey,
    /// Every component at full resolution (also CMYK files sampled that way).
    S444,
    /// Chroma halved across.
    S422,
    /// Chroma halved across and down.
    S420,
    /// Chroma halved down.
    S440,
    /// Chroma quartered across.
    S411,
    /// Any other layout (unequal chroma components, or factors that do not divide).
    Other,
}

impl Sampling {
    /// The subsampling Dalica's encoder writes for a file sampled this way: 4:2:0 stays 4:2:0;
    /// everything else is written as 4:4:4, never with less chroma than the source had.
    pub fn for_jpeg(self) -> Subsampling {
        match self {
            Sampling::S420 => Subsampling::S420,
            _ => Subsampling::S444,
        }
    }
}

/// What a JPEG's header says about how it was saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The quality (1–100, the Independent JPEG Group's scale that Dalica's encoder uses).
    pub quality: u8,
    pub sampling: Sampling,
}

/// One quantisation table from a DQT segment, in natural order, with its precision.
#[derive(Debug, Clone, Copy)]
struct Table {
    values: [u16; 64],
    /// 16-bit entries (Pq = 1) rather than 8-bit.
    wide: bool,
}

/// One component of the frame header: sampling factors and quantisation table slot.
#[derive(Debug, Clone, Copy)]
struct Component {
    h: u8,
    v: u8,
    tq: u8,
}

/// The quality and subsampling a JPEG was saved with; `None` when it is not a JPEG, its header is
/// cut before the first scan, or its frame selects no usable table.
pub fn read(bytes: &[u8]) -> Option<Settings> {
    let (tables, frame) = walk(bytes)?;
    let table = |c: Option<&Component>| tables.get(usize::from(c?.tq)).copied().flatten();
    let luma = table(frame.first())?;
    // The chroma table: the second component's, when the frame has colour.
    let chroma = match frame.len() {
        1 => None,
        _ => Some(table(frame.get(1))?),
    };
    let mut best: Option<(u64, u8)> = None;
    for q in 1..=100u8 {
        let mut err = table_error(&luma, &ANNEX_K_LUMA, q);
        if let Some(c) = &chroma {
            err = err.saturating_add(table_error(c, &ANNEX_K_CHROMA, q));
        }
        if best.is_none_or(|(e, _)| err < e) {
            best = Some((err, q));
        }
    }
    Some(Settings { quality: best?.1, sampling: sampling(&frame) })
}

/// The quality (1–100) a JPEG was most likely saved at (see [`read`]).
pub fn quality(bytes: &[u8]) -> Option<u8> {
    read(bytes).map(|s| s.quality)
}

/// The chroma subsampling of a JPEG's frame (see [`read`]).
pub fn subsampling(bytes: &[u8]) -> Option<Sampling> {
    read(bytes).map(|s| s.sampling)
}

/// The Independent JPEG Group's quality scaling of `base` (libjpeg's `jpeg_quality_scaling` and
/// `jpeg_add_quant_table`, as published with its source): a percentage scale of 5000 / q below
/// quality 50 and 200 − 2q from it, rounded, and kept within 1..=255 (8-bit) or 1..=32767.
fn scaled(base: u16, q: u8, wide: bool) -> u32 {
    let q = u32::from(q.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    let v = (u32::from(base) * scale + 50) / 100;
    v.clamp(1, if wide { 32767 } else { 255 })
}

/// How far `t` is from `base` scaled to quality `q`: the summed absolute difference.
fn table_error(t: &Table, base: &[u16; 64], q: u8) -> u64 {
    t.values.iter().zip(base).map(|(&v, &b)| u64::from(u32::from(v).abs_diff(scaled(b, q, t.wide)))).sum()
}

/// The sampling layout of a frame's components.
fn sampling(frame: &[Component]) -> Sampling {
    let [y, rest @ ..] = frame else { return Sampling::Other };
    if rest.is_empty() {
        return Sampling::Grey;
    }
    if rest.iter().all(|c| (c.h, c.v) == (y.h, y.v)) {
        return Sampling::S444;
    }
    let [cb, cr] = rest else { return Sampling::Other };
    if (cb.h, cb.v) != (cr.h, cr.v) || cb.h == 0 || cb.v == 0 || y.h % cb.h != 0 || y.v % cb.v != 0 {
        return Sampling::Other;
    }
    match (y.h / cb.h, y.v / cb.v) {
        (2, 1) => Sampling::S422,
        (2, 2) => Sampling::S420,
        (1, 2) => Sampling::S440,
        (4, 1) => Sampling::S411,
        _ => Sampling::Other,
    }
}

/// Is `marker` a start-of-frame (SOF0–SOF15, not DHT, JPG or DAC)?
fn is_sof(marker: u8) -> bool {
    (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC)
}

/// The quantisation tables 0..=3 and the frame's components defined before the first scan (a
/// later definition of the same slot replaces an earlier one); `None` unless the walk reaches the
/// first SOS with a frame header read.
fn walk(b: &[u8]) -> Option<([Option<Table>; 4], Vec<Component>)> {
    let mut tables = [None; 4];
    let mut frame: Option<Vec<Component>> = None;
    if b.get(..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2usize;
    for _ in 0..MAX_SEGMENTS {
        let (Some(&0xFF), Some(&marker)) = (b.get(i), b.get(i.saturating_add(1))) else { return None };
        if marker == 0xFF {
            i = i.saturating_add(1);
            continue;
        }
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i = i.saturating_add(2);
            continue;
        }
        if marker == 0xDA {
            return Some((tables, frame?));
        }
        if marker == 0xD9 {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]));
        let seg = b.get(i + 4..(i + 2).checked_add(len)?)?;
        if len < 2 {
            return None;
        }
        if marker == 0xDB {
            read_dqt(seg, &mut tables);
        } else if is_sof(marker) && frame.is_none() {
            frame = Some(read_sof(seg)?);
        }
        i = i.saturating_add(2).saturating_add(len);
    }
    None
}

/// The components of a frame header (T.81 B.2.2); `None` when it is malformed.
fn read_sof(seg: &[u8]) -> Option<Vec<Component>> {
    let n = usize::from(*seg.get(5)?);
    if n == 0 || n > MAX_COMPONENTS {
        return None;
    }
    let specs = seg.get(6..6 + 3 * n)?;
    specs
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[_, hv, tq]| {
            let (h, v) = (hv >> 4, hv & 0x0F);
            ((1..=4).contains(&h) && (1..=4).contains(&v) && tq <= 3).then_some(Component { h, v, tq })
        })
        .collect()
}

/// The tables of one DQT segment; a truncated or malformed table ends the segment, and a table
/// holding a zero is not usable (T.81 B.2.4.1: entries are 1 or more).
fn read_dqt(mut seg: &[u8], tables: &mut [Option<Table>; 4]) {
    while let Some((&pq_tq, rest)) = seg.split_first() {
        let wide = pq_tq >> 4 == 1;
        if pq_tq >> 4 > 1 {
            return;
        }
        let slot = usize::from(pq_tq & 0x0F);
        let n = if wide { 128 } else { 64 };
        let Some(data) = rest.get(..n) else { return };
        let mut values = [0u16; 64];
        for (k, &natural) in ZIGZAG.iter().enumerate() {
            let v = if wide {
                data.get(2 * k..2 * k + 2).map_or(0, |p| u16::from_be_bytes([p[0], p[1]]))
            } else {
                data.get(k).map_or(0, |&p| u16::from(p))
            };
            if let Some(dst) = values.get_mut(natural) {
                *dst = v;
            }
        }
        let table = values.iter().all(|&v| v > 0).then_some(Table { values, wide });
        if let Some(dst) = tables.get_mut(slot) {
            *dst = table;
        }
        seg = rest.get(n..).unwrap_or(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_matches_the_published_curve() {
        // Quality 50 is the Annex K table itself; 100 is all ones; 75 halves it.
        assert_eq!(scaled(16, 50, false), 16);
        assert_eq!(scaled(121, 100, false), 1);
        assert_eq!(scaled(16, 75, false), 8);
        assert_eq!(scaled(121, 1, false), 255);
        assert_eq!(scaled(121, 1, true), 6050);
    }

    #[test]
    fn zigzag_is_a_permutation() {
        let mut seen = [false; 64];
        for &n in &ZIGZAG {
            seen[n] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn our_encoder_uses_the_same_tables() {
        use crate::encode::jpeg as ours;
        assert_eq!(ours::Q_LUMA, ANNEX_K_LUMA);
        assert_eq!(ours::Q_CHROMA, ANNEX_K_CHROMA);
        assert_eq!(ours::ZIGZAG, ZIGZAG);
    }

    #[test]
    fn sampling_layouts() {
        let c = |h, v| Component { h, v, tq: 0 };
        assert_eq!(sampling(&[c(1, 1)]), Sampling::Grey);
        assert_eq!(sampling(&[c(1, 1), c(1, 1), c(1, 1)]), Sampling::S444);
        assert_eq!(sampling(&[c(2, 1), c(1, 1), c(1, 1)]), Sampling::S422);
        assert_eq!(sampling(&[c(2, 2), c(1, 1), c(1, 1)]), Sampling::S420);
        assert_eq!(sampling(&[c(1, 2), c(1, 1), c(1, 1)]), Sampling::S440);
        assert_eq!(sampling(&[c(4, 1), c(1, 1), c(1, 1)]), Sampling::S411);
        assert_eq!(sampling(&[c(2, 2), c(1, 1), c(2, 1)]), Sampling::Other);
        assert_eq!(sampling(&[c(3, 2), c(2, 1), c(2, 1)]), Sampling::Other);
        assert_eq!(sampling(&[c(1, 1), c(1, 1), c(1, 1), c(1, 1)]), Sampling::S444);
        assert_eq!(sampling(&[c(2, 2), c(1, 1), c(1, 1), c(2, 2)]), Sampling::Other);
        assert_eq!(sampling(&[]), Sampling::Other);
    }
}
