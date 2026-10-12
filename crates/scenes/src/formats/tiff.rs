//! Our own baseline TIFF writer, from the public TIFF 6.0 specification: one image per IFD, chunky
//! samples in strips, either byte order. IFD0 is the full image; with a thumbnail, IFD1 (chained
//! after it, `NewSubfileType` 1) holds a small uncompressed 8-bit RGB copy stored the same way.
//! Tags are in ascending order; values longer than four bytes follow their IFD at even offsets.

use crate::exif::ByteOrder;
use crate::formats::{Colour, Depth, Error, Result, SampleData, Samples, pack, palette, samples};
use crate::orient;
use crate::render::Scene;

/// How the strips are compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compression {
    None,
    /// LZW (5).
    Lzw,
    /// zlib (8, "Adobe Deflate").
    Deflate,
    /// PackBits (32773), each row packed on its own.
    PackBits,
}

impl Compression {
    pub const ALL: [Compression; 4] = [Compression::None, Compression::Lzw, Compression::Deflate, Compression::PackBits];

    /// The value of the Compression tag.
    pub fn tag(self) -> u16 {
        match self {
            Compression::None => 1,
            Compression::Lzw => 5,
            Compression::Deflate => 8,
            Compression::PackBits => 32_773,
        }
    }
}

/// What the TIFF holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tiff {
    pub colour: Colour,
    pub depth: Depth,
    pub compression: Compression,
    /// The Orientation tag, 1 to 8: the pixels are stored so that it displays them upright.
    pub orientation: u8,
    /// An embedded thumbnail IFD this many pixels on its longer side.
    pub thumbnail: Option<u32>,
    /// The ICC profile tag (34675).
    pub icc: Option<Vec<u8>>,
    pub byte_order: ByteOrder,
    /// Rows per strip; `None`: strips of about 8 KiB.
    pub rows_per_strip: Option<u32>,
}

impl Default for Tiff {
    fn default() -> Tiff {
        Tiff {
            colour: Colour::Rgb,
            depth: Depth::Eight,
            compression: Compression::None,
            orientation: 1,
            thumbnail: None,
            icc: None,
            byte_order: ByteOrder::Little,
            rows_per_strip: None,
        }
    }
}

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;
const UNDEFINED: u16 = 7;

pub const TAG_NEW_SUBFILE_TYPE: u16 = 254;
pub const TAG_IMAGE_WIDTH: u16 = 256;
pub const TAG_IMAGE_LENGTH: u16 = 257;
pub const TAG_BITS_PER_SAMPLE: u16 = 258;
pub const TAG_COMPRESSION: u16 = 259;
pub const TAG_PHOTOMETRIC: u16 = 262;
pub const TAG_STRIP_OFFSETS: u16 = 273;
pub const TAG_ORIENTATION: u16 = 274;
pub const TAG_SAMPLES_PER_PIXEL: u16 = 277;
pub const TAG_ROWS_PER_STRIP: u16 = 278;
pub const TAG_STRIP_BYTE_COUNTS: u16 = 279;
pub const TAG_X_RESOLUTION: u16 = 282;
pub const TAG_Y_RESOLUTION: u16 = 283;
pub const TAG_PLANAR_CONFIGURATION: u16 = 284;
pub const TAG_RESOLUTION_UNIT: u16 = 296;
pub const TAG_SOFTWARE: u16 = 305;
pub const TAG_COLOR_MAP: u16 = 320;
pub const TAG_INK_SET: u16 = 332;
pub const TAG_SAMPLE_FORMAT: u16 = 339;
pub const TAG_ICC_PROFILE: u16 = 34_675;

/// Target strip size when `rows_per_strip` is `None`.
const STRIP_BYTES: usize = 8 * 1024;

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

#[derive(Clone, Copy)]
struct W(ByteOrder);

impl W {
    fn u16(self, v: u16) -> [u8; 2] {
        match self.0 {
            ByteOrder::Little => v.to_le_bytes(),
            ByteOrder::Big => v.to_be_bytes(),
        }
    }

    fn u32(self, v: u32) -> [u8; 4] {
        match self.0 {
            ByteOrder::Little => v.to_le_bytes(),
            ByteOrder::Big => v.to_be_bytes(),
        }
    }

    fn shorts(self, tag: u16, v: &[u16]) -> Entry {
        Entry { tag, kind: SHORT, count: v.len() as u32, value: v.iter().flat_map(|s| self.u16(*s)).collect() }
    }

    fn longs(self, tag: u16, v: &[u32]) -> Entry {
        Entry { tag, kind: LONG, count: v.len() as u32, value: v.iter().flat_map(|s| self.u32(*s)).collect() }
    }

    fn rational(self, tag: u16, n: u32, d: u32) -> Entry {
        let mut value = self.u32(n).to_vec();
        value.extend(self.u32(d));
        Entry { tag, kind: RATIONAL, count: 1, value }
    }

    /// Sample bytes in this byte order.
    fn sample_bytes(self, data: &SampleData) -> Vec<u8> {
        match data {
            SampleData::U8(v) => v.clone(),
            SampleData::U16(v) => v.iter().flat_map(|s| self.u16(*s)).collect(),
            SampleData::F32(v) => v.iter().flat_map(|s| self.u32(s.to_bits())).collect(),
        }
    }
}

fn ascii(tag: u16, s: &str) -> Entry {
    let mut value: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    value.push(0);
    Entry { tag, kind: ASCII, count: value.len() as u32, value }
}

/// One IFD and its strips.
struct Page {
    entries: Vec<Entry>,
    strips: Vec<Vec<u8>>,
}

impl Page {
    /// Bytes the IFD takes with its out-of-line values.
    fn ifd_len(&self) -> usize {
        2 + 12 * self.entries.len() + 4 + self.entries.iter().filter(|e| e.value.len() > 4).map(|e| e.value.len().next_multiple_of(2)).sum::<usize>()
    }

    fn strips_len(&self) -> usize {
        self.strips.iter().map(|s| s.len().next_multiple_of(2)).sum()
    }
}

fn too_large() -> Error {
    Error::Encode { format: "TIFF", message: "the file would pass the 4 GiB offset limit".into() }
}

fn offset(v: usize) -> Result<u32> {
    u32::try_from(v).map_err(|_| too_large())
}

/// The IFD of `s` with its strips (offsets filled in later).
fn page(w: W, s: &Samples, compression: Compression, rows_per_strip: Option<u32>, subfile: u32, orientation: u8) -> Result<Page> {
    let spp = s.colour.channels();
    let row_bytes = (s.width as usize).saturating_mul(spp).saturating_mul(s.depth.bytes()).max(1);
    let rows = rows_per_strip.unwrap_or((STRIP_BYTES / row_bytes).max(1) as u32).clamp(1, s.height.max(1));
    let bytes = w.sample_bytes(&s.data);
    let strips: Vec<Vec<u8>> = bytes
        .chunks(row_bytes.saturating_mul(rows as usize))
        .map(|strip| match compression {
            Compression::None => strip.to_vec(),
            Compression::Lzw => pack::lzw(strip),
            Compression::Deflate => pack::deflate(strip),
            Compression::PackBits => strip.chunks(row_bytes).flat_map(pack::packbits).collect(),
        })
        .collect();
    let bits = (s.depth.bytes() * 8) as u16;
    let photometric = match s.colour {
        Colour::Grey => 1,
        Colour::Rgb => 2,
        Colour::Palette => 3,
        Colour::Cmyk => 5,
    };
    let format = if s.depth == Depth::Float { 3 } else { 1 };
    let counts = strips.iter().map(|s| offset(s.len())).collect::<Result<Vec<u32>>>()?;
    let mut entries = vec![
        w.longs(TAG_NEW_SUBFILE_TYPE, &[subfile]),
        w.longs(TAG_IMAGE_WIDTH, &[s.width]),
        w.longs(TAG_IMAGE_LENGTH, &[s.height]),
        w.shorts(TAG_BITS_PER_SAMPLE, &vec![bits; spp]),
        w.shorts(TAG_COMPRESSION, &[compression.tag()]),
        w.shorts(TAG_PHOTOMETRIC, &[photometric]),
        w.longs(TAG_STRIP_OFFSETS, &vec![0; strips.len()]),
        w.shorts(TAG_ORIENTATION, &[u16::from(orientation)]),
        w.shorts(TAG_SAMPLES_PER_PIXEL, &[spp as u16]),
        w.longs(TAG_ROWS_PER_STRIP, &[rows]),
        w.longs(TAG_STRIP_BYTE_COUNTS, &counts),
        w.rational(TAG_X_RESOLUTION, 72, 1),
        w.rational(TAG_Y_RESOLUTION, 72, 1),
        w.shorts(TAG_PLANAR_CONFIGURATION, &[1]),
        w.shorts(TAG_RESOLUTION_UNIT, &[2]),
        ascii(TAG_SOFTWARE, "Dalica scenes"),
        w.shorts(TAG_SAMPLE_FORMAT, &vec![format; spp]),
    ];
    match s.colour {
        Colour::Palette => {
            let p = palette();
            let map: Vec<u16> = (0..3).flat_map(|c| p.iter().map(move |rgb| u16::from(rgb[c]) * 257)).collect();
            entries.push(w.shorts(TAG_COLOR_MAP, &map));
        }
        Colour::Cmyk => entries.push(w.shorts(TAG_INK_SET, &[1])),
        Colour::Grey | Colour::Rgb => {}
    }
    Ok(Page { entries, strips })
}

/// Appends `page`'s IFD at `out.len()` pointing to `next`, with its strips at `strips_at`.
fn write_page(w: W, out: &mut Vec<u8>, mut page: Page, strips_at: usize, next: u32) -> Result<()> {
    let mut at = strips_at;
    let mut offsets = Vec::with_capacity(page.strips.len());
    for s in &page.strips {
        offsets.push(offset(at)?);
        at += s.len().next_multiple_of(2);
    }
    for e in &mut page.entries {
        if e.tag == TAG_STRIP_OFFSETS {
            e.value = offsets.iter().flat_map(|o| w.u32(*o)).collect();
        }
    }
    page.entries.sort_by_key(|e| e.tag);
    let start = out.len();
    let mut data_at = start + 2 + 12 * page.entries.len() + 4;
    let mut data = Vec::new();
    out.extend(w.u16(page.entries.len() as u16));
    for e in &page.entries {
        out.extend(w.u16(e.tag));
        out.extend(w.u16(e.kind));
        out.extend(w.u32(e.count));
        if e.value.len() <= 4 {
            let mut inline = e.value.clone();
            inline.resize(4, 0);
            out.extend(inline);
        } else {
            out.extend(w.u32(offset(data_at)?));
            data.extend(&e.value);
            if e.value.len() % 2 == 1 {
                data.push(0);
            }
            data_at += e.value.len().next_multiple_of(2);
        }
    }
    out.extend(w.u32(next));
    out.extend(data);
    for s in &page.strips {
        out.extend(s);
        if s.len() % 2 == 1 {
            out.push(0);
        }
    }
    Ok(())
}

/// A TIFF of `scene` as `opts` describes.
pub fn tiff(scene: &Scene, opts: &Tiff) -> Result<Vec<u8>> {
    let w = W(opts.byte_order);
    let stored = orient::store(&scene.render()?, opts.orientation)?;
    let main = samples(&stored, opts.colour, opts.depth)?;
    let mut ifd0 = page(w, &main, opts.compression, opts.rows_per_strip, 0, opts.orientation)?;
    if let Some(icc) = &opts.icc {
        ifd0.entries.push(Entry { tag: TAG_ICC_PROFILE, kind: UNDEFINED, count: offset(icc.len())?, value: icc.clone() });
    }
    let ifd1 = match opts.thumbnail {
        Some(long_side) => {
            let thumb = samples(&stored.downscale(long_side)?, Colour::Rgb, Depth::Eight)?;
            Some(page(w, &thumb, Compression::None, None, 1, opts.orientation)?)
        }
        None => None,
    };
    // Layout: header, IFD0, its strips, IFD1, its strips.
    let ifd0_at = 8;
    let strips0_at = ifd0_at + ifd0.ifd_len();
    let ifd1_at = strips0_at + ifd0.strips_len();
    let total = ifd1_at + ifd1.as_ref().map_or(0, |p| p.ifd_len() + p.strips_len());
    offset(total)?;
    let mut out = Vec::with_capacity(total);
    out.extend(match opts.byte_order {
        ByteOrder::Little => *b"II",
        ByteOrder::Big => *b"MM",
    });
    out.extend(w.u16(42));
    out.extend(w.u32(ifd0_at as u32));
    write_page(w, &mut out, ifd0, strips0_at, if ifd1.is_some() { offset(ifd1_at)? } else { 0 })?;
    if let Some(ifd1) = ifd1 {
        let strips1_at = ifd1_at + ifd1.ifd_len();
        write_page(w, &mut out, ifd1, strips1_at, 0)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_follow_their_ifd_and_offsets_point_at_them() {
        let opts = Tiff { rows_per_strip: Some(5), ..Tiff::default() };
        let t = tiff(&Scene::new(1, 12, 11), &opts).unwrap();
        assert_eq!(&t[..8], b"II*\0\x08\0\0\0");
        let n = u16::from_le_bytes([t[8], t[9]]) as usize;
        let entry = |tag: u16| (0..n).map(|k| 10 + 12 * k).find(|&e| u16::from_le_bytes([t[e], t[e + 1]]) == tag).unwrap();
        let e = entry(TAG_STRIP_OFFSETS);
        assert_eq!(u32::from_le_bytes([t[e + 4], t[e + 5], t[e + 6], t[e + 7]]), 3, "11 rows in strips of 5");
        let at = u32::from_le_bytes([t[e + 8], t[e + 9], t[e + 10], t[e + 11]]) as usize;
        let first = u32::from_le_bytes([t[at], t[at + 1], t[at + 2], t[at + 3]]) as usize;
        let upright = Scene::new(1, 12, 11).render().unwrap();
        assert_eq!(&t[first..first + 12 * 3], &upright.data[..12 * 3]);
        let tags: Vec<u16> = (0..n).map(|k| 10 + 12 * k).map(|e| u16::from_le_bytes([t[e], t[e + 1]])).collect();
        assert!(tags.windows(2).all(|p| p[0] < p[1]), "ascending tags: {tags:?}");
    }

    #[test]
    fn unsupported_layouts_and_orientations_are_refused() {
        let s = Scene::new(1, 8, 8);
        assert!(tiff(&s, &Tiff { colour: Colour::Palette, depth: Depth::Sixteen, ..Tiff::default() }).is_err());
        assert!(tiff(&s, &Tiff { colour: Colour::Cmyk, depth: Depth::Float, ..Tiff::default() }).is_err());
        assert!(tiff(&s, &Tiff { orientation: 9, ..Tiff::default() }).is_err());
        assert!(tiff(&Scene::new(1, 0, 8), &Tiff::default()).is_err());
    }
}
