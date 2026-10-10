//! Reading the EXIF fields Dalica needs from a TIFF structure (an EXIF APP1 payload, a PNG `eXIf`
//! chunk, or a TIFF file): orientation, dates, camera make and model, and where the embedded JPEG
//! thumbnail (IFD1) lies. Strict: a loop, an offset outside the block or a count past the limits
//! anywhere on the way is an error.

use std::ops::Range;

use dalica_core::DateTime;

use crate::ifd::{Tiff, Visited};
use crate::limits::Limits;
use crate::{Error, Result};

pub const TAG_MAKE: u16 = 0x010f;
pub const TAG_MODEL: u16 = 0x0110;
pub const TAG_ORIENTATION: u16 = 0x0112;
pub const TAG_EXIF_IFD: u16 = 0x8769;
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
pub const TAG_DATE_TIME_DIGITIZED: u16 = 0x9004;
pub const TAG_THUMB_OFFSET: u16 = 0x0201;
pub const TAG_THUMB_LENGTH: u16 = 0x0202;

/// The EXIF fields of one image. Absent or unreadable fields are `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exif {
    /// 1–8; values outside that range are dropped.
    pub orientation: Option<u8>,
    pub date_time_original: Option<DateTime>,
    pub date_time_digitized: Option<DateTime>,
    pub make: Option<String>,
    pub model: Option<String>,
    /// The embedded JPEG thumbnail, as a byte range of the TIFF block.
    pub thumbnail: Option<Range<usize>>,
}

/// Reads [`Exif`] from a TIFF block.
pub fn read(block: &[u8], limits: &Limits) -> Result<Exif> {
    let tiff = Tiff::new(block)?;
    let mut visited = Visited::default();
    let ifd0 = tiff.ifd(tiff.first_ifd()?, &mut visited, limits)?;
    let mut out = Exif {
        orientation: ifd0.get(TAG_ORIENTATION).and_then(|e| tiff.uint(e)).and_then(|o| u8::try_from(o).ok()).filter(|o| (1..=8).contains(o)),
        make: ifd0.get(TAG_MAKE).and_then(|e| tiff.ascii(e, limits.max_string)).filter(|s| !s.trim().is_empty()),
        model: ifd0.get(TAG_MODEL).and_then(|e| tiff.ascii(e, limits.max_string)).filter(|s| !s.trim().is_empty()),
        ..Exif::default()
    };
    if let Some(ptr) = ifd0.get(TAG_EXIF_IFD).and_then(|e| tiff.uint(e)) {
        if limits.max_ifd_depth < 2 {
            return Err(Error::TooMany("IFD depth"));
        }
        let sub = tiff.ifd(ptr, &mut visited, limits)?;
        let date = |tag| sub.get(tag).and_then(|e| tiff.ascii(e, 64)).and_then(|s| s.trim().parse::<DateTime>().ok());
        out.date_time_original = date(TAG_DATE_TIME_ORIGINAL);
        out.date_time_digitized = date(TAG_DATE_TIME_DIGITIZED);
    }
    if ifd0.next != 0 {
        let ifd1 = tiff.ifd(ifd0.next, &mut visited, limits)?;
        let offset = ifd1.get(TAG_THUMB_OFFSET).and_then(|e| tiff.uint(e));
        let length = ifd1.get(TAG_THUMB_LENGTH).and_then(|e| tiff.uint(e));
        if let (Some(offset), Some(length)) = (offset, length) {
            if u64::from(length) > limits.max_thumbnail {
                return Err(Error::TooMany("thumbnail bytes"));
            }
            let bytes = tiff.bytes(u64::from(offset), u64::from(length))?;
            // Only a JPEG counts (TIFF-strip thumbnails are not read).
            if bytes.starts_with(b"\xff\xd8") {
                let start = offset as usize;
                out.thumbnail = Some(start..start + length as usize);
            }
        }
        // Further IFDs are not needed, but a loop or bad offset among them is still an error.
        let mut next = ifd1.next;
        while next != 0 {
            next = tiff.ifd(next, &mut visited, limits)?.next;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use dalica_scenes::exif::{ByteOrder, Exif as SceneExif, tiff as scene_tiff};

    use super::*;

    fn block(o: u8, order: ByteOrder, thumb: Option<&[u8]>) -> Vec<u8> {
        let exif = SceneExif {
            orientation: o,
            date_time_original: "2026-06-04T19:42:10".parse().ok(),
            make: Some("Dalica".into()),
            model: Some("Scenes 1".into()),
            thumbnail: None,
            byte_order: order,
        };
        scene_tiff(&exif, 64, 48, thumb).unwrap()
    }

    #[test]
    fn reads_every_field_in_both_byte_orders() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            for o in 1..=8 {
                let b = block(o, order, Some(b"\xff\xd8thumb\xff\xd9"));
                let e = read(&b, &Limits::default()).unwrap();
                assert_eq!(e.orientation, Some(o));
                assert_eq!(e.date_time_original.unwrap().to_string(), "2026-06-04T19:42:10");
                assert_eq!((e.make.as_deref(), e.model.as_deref()), (Some("Dalica"), Some("Scenes 1")));
                assert_eq!(&b[e.thumbnail.unwrap()], b"\xff\xd8thumb\xff\xd9");
            }
        }
    }

    /// Patches the u32 at `at` (little-endian blocks only).
    fn patch(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Offset of IFD0's next-IFD pointer, and of the EXIF IFD pointer's value, in a little-endian block.
    fn pointers(b: &[u8]) -> (usize, usize) {
        let n = u16::from_le_bytes([b[8], b[9]]) as usize;
        let exif_entry = (0..n).map(|k| 10 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_EXIF_IFD).unwrap();
        (10 + 12 * n, exif_entry + 8)
    }

    #[test]
    fn looping_ifds_are_an_error() {
        let base = block(6, ByteOrder::Little, Some(b"\xff\xd8x\xff\xd9"));
        let (next_at, exif_at) = pointers(&base);
        // IFD0's next pointer leads back to IFD0.
        let mut b = base.clone();
        patch(&mut b, next_at, 8);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
        // The EXIF IFD pointer leads back to IFD0.
        let mut b = base.clone();
        patch(&mut b, exif_at, 8);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
        // IFD1's next pointer leads to IFD1 itself.
        let mut b = base.clone();
        let ifd1 = u32::from_le_bytes(b[next_at..next_at + 4].try_into().unwrap()) as usize;
        let n1 = u16::from_le_bytes([b[ifd1], b[ifd1 + 1]]) as usize;
        patch(&mut b, ifd1 + 2 + 12 * n1, ifd1 as u32);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::Loop)));
    }

    #[test]
    fn out_of_range_offsets_are_an_error() {
        let base = block(3, ByteOrder::Little, Some(b"\xff\xd8x\xff\xd9"));
        let (next_at, exif_at) = pointers(&base);
        for (at, v) in [(4, u32::MAX), (4, base.len() as u32), (4, 2), (next_at, 0xffff_fff0), (exif_at, base.len() as u32 + 100)] {
            let mut b = base.clone();
            patch(&mut b, at, v);
            assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)), "pointer at {at} = {v}");
        }
        // A thumbnail whose length runs past the block.
        let mut b = base.clone();
        let ifd1 = u32::from_le_bytes(b[next_at..next_at + 4].try_into().unwrap()) as usize;
        let n1 = u16::from_le_bytes([b[ifd1], b[ifd1 + 1]]) as usize;
        let len_entry = (0..n1).map(|k| ifd1 + 2 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_THUMB_LENGTH).unwrap();
        patch(&mut b, len_entry + 8, 1_000_000);
        assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)));
        // An entry count that cannot fit.
        let mut b = base.clone();
        b[8..10].copy_from_slice(&5000u16.to_le_bytes());
        assert!(matches!(read(&b, &Limits::default()), Err(Error::OutOfRange)));
        b[8..10].copy_from_slice(&9000u16.to_le_bytes());
        assert!(matches!(read(&b, &Limits::default()), Err(Error::TooMany(_))));
    }

    #[test]
    fn caps_and_odd_values() {
        let b = block(8, ByteOrder::Big, None);
        assert!(matches!(read(&b, &Limits { max_ifds: 1, ..Limits::default() }), Err(Error::TooMany(_))));
        assert!(matches!(read(&b, &Limits { max_ifd_depth: 1, ..Limits::default() }), Err(Error::TooMany(_))));
        assert!(read(b"II*\0", &Limits::default()).is_err());
        assert!(read(b"XX*\0\x08\0\0\0", &Limits::default()).is_err());
        // An orientation of 0 or 9 is dropped, not an error.
        for bad in [0u16, 9, 300] {
            let mut b = block(1, ByteOrder::Little, None);
            let n = u16::from_le_bytes([b[8], b[9]]) as usize;
            let e = (0..n).map(|k| 10 + 12 * k).find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == TAG_ORIENTATION).unwrap();
            b[e + 8..e + 10].copy_from_slice(&bad.to_le_bytes());
            assert_eq!(read(&b, &Limits::default()).unwrap().orientation, None);
        }
    }
}
