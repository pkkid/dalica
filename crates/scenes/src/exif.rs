//! A minimal EXIF block, written from the public TIFF 6.0 / EXIF 2.32 structure: a TIFF header,
//! IFD0 (Make, Model, Orientation, the EXIF IFD pointer), the EXIF IFD (ExifVersion,
//! DateTimeOriginal, PixelXDimension, PixelYDimension) and, with a thumbnail, IFD1 (Compression = 6,
//! JPEGInterchangeFormat, JPEGInterchangeFormatLength) followed by the thumbnail's JPEG bytes.
//! Tags are in ascending order; values longer than four bytes follow their IFD at even offsets.

use dalica_core::DateTime;

use crate::{Error, Result};

/// The TIFF byte order: `II` (little-endian, most cameras) or `MM` (big-endian).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ByteOrder {
    #[default]
    Little,
    Big,
}

/// What goes into the EXIF block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exif {
    /// 1 to 8 (`orient` module).
    pub orientation: u8,
    pub date_time_original: Option<DateTime>,
    pub make: Option<String>,
    pub model: Option<String>,
    /// Embed a JPEG thumbnail this many pixels on its longer side (`None`: no IFD1).
    pub thumbnail: Option<u32>,
    pub byte_order: ByteOrder,
}

impl Default for Exif {
    fn default() -> Exif {
        Exif { orientation: 1, date_time_original: None, make: None, model: None, thumbnail: None, byte_order: ByteOrder::Little }
    }
}

const SHORT: u16 = 3;
const LONG: u16 = 4;
const ASCII: u16 = 2;
const UNDEFINED: u16 = 7;

pub const TAG_MAKE: u16 = 0x010f;
pub const TAG_MODEL: u16 = 0x0110;
pub const TAG_ORIENTATION: u16 = 0x0112;
pub const TAG_EXIF_IFD: u16 = 0x8769;
pub const TAG_EXIF_VERSION: u16 = 0x9000;
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
pub const TAG_PIXEL_X: u16 = 0xa002;
pub const TAG_PIXEL_Y: u16 = 0xa003;
pub const TAG_COMPRESSION: u16 = 0x0103;
pub const TAG_THUMB_OFFSET: u16 = 0x0201;
pub const TAG_THUMB_LENGTH: u16 = 0x0202;

/// One IFD entry: tag, type, count and the value's bytes (already in the block's byte order).
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

struct Writer {
    order: ByteOrder,
}

impl Writer {
    fn u16(&self, v: u16) -> [u8; 2] {
        match self.order {
            ByteOrder::Little => v.to_le_bytes(),
            ByteOrder::Big => v.to_be_bytes(),
        }
    }

    fn u32(&self, v: u32) -> [u8; 4] {
        match self.order {
            ByteOrder::Little => v.to_le_bytes(),
            ByteOrder::Big => v.to_be_bytes(),
        }
    }

    fn short(&self, tag: u16, v: u16) -> Entry {
        Entry { tag, kind: SHORT, count: 1, value: self.u16(v).to_vec() }
    }

    fn long(&self, tag: u16, v: u32) -> Entry {
        Entry { tag, kind: LONG, count: 1, value: self.u32(v).to_vec() }
    }

    fn ascii(tag: u16, s: &str) -> Entry {
        let mut value: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
        value.push(0);
        Entry { tag, kind: ASCII, count: value.len() as u32, value }
    }
}

/// Bytes an IFD takes with its out-of-line values.
fn ifd_len(entries: &[Entry]) -> usize {
    2 + 12 * entries.len() + 4 + entries.iter().filter(|e| e.value.len() > 4).map(|e| e.value.len().next_multiple_of(2)).sum::<usize>()
}

/// Appends an IFD at `out.len()` (which must be its offset) pointing to `next`.
fn write_ifd(w: &Writer, out: &mut Vec<u8>, mut entries: Vec<Entry>, next: u32) {
    entries.sort_by_key(|e| e.tag);
    let start = out.len();
    let mut data_at = start + 2 + 12 * entries.len() + 4;
    let mut data = Vec::new();
    out.extend(w.u16(entries.len() as u16));
    for e in &entries {
        out.extend(w.u16(e.tag));
        out.extend(w.u16(e.kind));
        out.extend(w.u32(e.count));
        if e.value.len() <= 4 {
            let mut inline = e.value.clone();
            inline.resize(4, 0);
            out.extend(inline);
        } else {
            out.extend(w.u32(data_at as u32));
            data.extend(&e.value);
            if e.value.len() % 2 == 1 {
                data.push(0);
            }
            data_at += e.value.len().next_multiple_of(2);
        }
    }
    out.extend(w.u32(next));
    out.extend(data);
}

/// The TIFF bytes of the EXIF block (without the `Exif\0\0` APP1 prefix) for an image whose
/// stored pixels are `width × height`.
pub fn tiff(exif: &Exif, width: u32, height: u32, thumbnail_jpeg: Option<&[u8]>) -> Result<Vec<u8>> {
    if !(1..=8).contains(&exif.orientation) {
        return Err(Error::Orientation(exif.orientation));
    }
    let w = Writer { order: exif.byte_order };
    let mut ifd0 = vec![w.short(TAG_ORIENTATION, u16::from(exif.orientation)), w.long(TAG_EXIF_IFD, 0)];
    if let Some(make) = &exif.make {
        ifd0.push(Writer::ascii(TAG_MAKE, make));
    }
    if let Some(model) = &exif.model {
        ifd0.push(Writer::ascii(TAG_MODEL, model));
    }
    let mut exif_ifd = vec![
        Entry { tag: TAG_EXIF_VERSION, kind: UNDEFINED, count: 4, value: b"0232".to_vec() },
        w.long(TAG_PIXEL_X, width),
        w.long(TAG_PIXEL_Y, height),
    ];
    if let Some(dt) = exif.date_time_original {
        let text =
            format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", dt.date().year(), dt.date().month(), dt.date().day(), dt.hour(), dt.minute(), dt.second());
        exif_ifd.push(Writer::ascii(TAG_DATE_TIME_ORIGINAL, &text));
    }
    let ifd1 = thumbnail_jpeg.map(|t| vec![w.short(TAG_COMPRESSION, 6), w.long(TAG_THUMB_OFFSET, 0), w.long(TAG_THUMB_LENGTH, t.len() as u32)]);

    // Layout: header, IFD0, EXIF IFD, IFD1, thumbnail.
    let ifd0_at = 8;
    let exif_at = ifd0_at + ifd_len(&ifd0);
    let ifd1_at = exif_at + ifd_len(&exif_ifd);
    let thumb_at = ifd1_at + ifd1.as_ref().map_or(0, |e| ifd_len(e));
    for e in &mut ifd0 {
        if e.tag == TAG_EXIF_IFD {
            e.value = w.u32(exif_at as u32).to_vec();
        }
    }
    let mut out = Vec::with_capacity(thumb_at + thumbnail_jpeg.map_or(0, <[u8]>::len));
    out.extend(match exif.byte_order {
        ByteOrder::Little => *b"II",
        ByteOrder::Big => *b"MM",
    });
    out.extend(w.u16(42));
    out.extend(w.u32(ifd0_at as u32));
    write_ifd(&w, &mut out, ifd0, if ifd1.is_some() { ifd1_at as u32 } else { 0 });
    write_ifd(&w, &mut out, exif_ifd, 0);
    if let (Some(mut ifd1), Some(thumb)) = (ifd1, thumbnail_jpeg) {
        for e in &mut ifd1 {
            if e.tag == TAG_THUMB_OFFSET {
                e.value = w.u32(thumb_at as u32).to_vec();
            }
        }
        write_ifd(&w, &mut out, ifd1, 0);
        out.extend(thumb);
    }
    // A JPEG APP1 segment holds 65 535 − 2 (length) − 6 (`Exif\0\0`) bytes.
    if out.len() > 65_527 {
        return Err(Error::ExifTooLarge(out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (tag, type, count, value-or-offset bytes) of one IFD entry.
    type RawEntry = (u16, u16, u32, [u8; 4]);

    /// A tiny reader for the tests: the entries of the IFD at `at`, and the next IFD's offset.
    fn read_ifd(t: &[u8], at: usize) -> (Vec<RawEntry>, u32) {
        let le = &t[..2] == b"II";
        let u16_at = |i: usize| if le { u16::from_le_bytes([t[i], t[i + 1]]) } else { u16::from_be_bytes([t[i], t[i + 1]]) };
        let u32_at = |i: usize| {
            let b = [t[i], t[i + 1], t[i + 2], t[i + 3]];
            if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
        };
        let n = u16_at(at) as usize;
        let entries =
            (0..n).map(|k| at + 2 + 12 * k).map(|e| (u16_at(e), u16_at(e + 2), u32_at(e + 4), [t[e + 8], t[e + 9], t[e + 10], t[e + 11]])).collect();
        (entries, u32_at(at + 2 + 12 * n))
    }

    #[test]
    fn both_byte_orders_carry_the_same_fields() {
        let dt: DateTime = "2026-06-04T19:42:10".parse().unwrap();
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let exif = Exif {
                orientation: 6,
                date_time_original: Some(dt),
                make: Some("Dalica".into()),
                model: Some("Scenes 1".into()),
                thumbnail: None,
                byte_order: order,
            };
            let t = tiff(&exif, 480, 640, Some(b"\xff\xd8thumb\xff\xd9")).unwrap();
            let le = order == ByteOrder::Little;
            assert_eq!(&t[..4], if le { b"II*\0" } else { b"MM\0*" });
            let (ifd0, next) = read_ifd(&t, 8);
            assert_eq!(ifd0.iter().map(|e| e.0).collect::<Vec<_>>(), [TAG_MAKE, TAG_MODEL, TAG_ORIENTATION, TAG_EXIF_IFD]);
            let orientation = ifd0[2].3;
            assert_eq!(if le { orientation[0] } else { orientation[1] }, 6);
            let u32v = |b: [u8; 4]| if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) };
            let (exif_ifd, _) = read_ifd(&t, u32v(ifd0[3].3) as usize);
            let date = exif_ifd.iter().find(|e| e.0 == TAG_DATE_TIME_ORIGINAL).unwrap();
            let at = u32v(date.3) as usize;
            assert_eq!(&t[at..at + 20], b"2026:06:04 19:42:10\0");
            let (ifd1, _) = read_ifd(&t, next as usize);
            let off = u32v(ifd1.iter().find(|e| e.0 == TAG_THUMB_OFFSET).unwrap().3) as usize;
            let len = u32v(ifd1.iter().find(|e| e.0 == TAG_THUMB_LENGTH).unwrap().3) as usize;
            assert_eq!(&t[off..off + len], b"\xff\xd8thumb\xff\xd9");
        }
    }

    #[test]
    fn bad_orientations_and_oversized_blocks_are_refused() {
        assert!(matches!(tiff(&Exif { orientation: 0, ..Exif::default() }, 1, 1, None), Err(Error::Orientation(0))));
        assert!(matches!(tiff(&Exif::default(), 1, 1, Some(&vec![0u8; 70_000])), Err(Error::ExifTooLarge(_))));
        let t = tiff(&Exif::default(), 1, 1, None).unwrap();
        assert_eq!(read_ifd(&t, 8).1, 0, "no IFD1 without a thumbnail");
    }
}
