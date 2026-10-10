//! dalica-image: Sniffing, scaled decoding, encoding, orientation, ICC → sRGB, decode limits, embedded thumbnails and RAW previews (a bounded TIFF/IFD walker of our own).
//!
//! Layer L1; see `docs/architecture.md` §2. Every input is hostile (`security.md` §4): readers
//! take [`Limits`], check every range before reading it and return an [`Error`] instead of
//! panicking or looping.
//!
//! - [`sniff`]: the format from the first bytes, never from the name.
//! - [`read_header`] / [`read_header_file`]: size, EXIF orientation, dates, camera and the
//!   embedded thumbnail's location, without decoding pixels. EXIF trouble does not fail the header:
//!   the photo still shows, upright as stored, and [`Header::exif_error`] says why.
//! - [`ifd`]: the bounded TIFF/IFD reader; [`exif`]: the fields read from it; [`orient`]: turning
//!   pixels upright.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod exif;
pub mod ifd;
pub mod limits;
pub mod orient;
pub mod sniff;

use std::io::Read as _;
use std::ops::Range;
use std::path::Path;

pub use exif::Exif;
pub use limits::Limits;
pub use orient::Orientation;
pub use sniff::{Format, sniff};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not an image format Dalica knows")]
    Unknown,
    #[error("{0:?} is not supported yet")]
    Unsupported(Format),
    #[error("the file ends too early")]
    Truncated,
    #[error("malformed: {0}")]
    Malformed(String),
    #[error("an offset points outside the data")]
    OutOfRange,
    #[error("an IFD chain loops")]
    Loop,
    #[error("too many {0}")]
    TooMany(&'static str),
    #[error("{width} × {height} is larger than Dalica decodes")]
    TooLarge { width: u32, height: u32 },
    #[error("cannot read the file: {0}")]
    Io(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// 8-bit RGBA pixels, sRGB, rows top to bottom, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// What a file says about itself before any pixel is decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub format: Format,
    /// Size of the stored pixels (before orientation).
    pub width: u32,
    pub height: u32,
    /// From EXIF; upright when absent or unreadable.
    pub orientation: Orientation,
    pub exif: Option<Exif>,
    /// Why the EXIF block was ignored, if it was.
    pub exif_error: Option<Error>,
    /// The embedded JPEG thumbnail, as a byte range of the file.
    pub thumbnail: Option<Range<usize>>,
}

impl Header {
    /// The size as displayed, after orientation.
    pub fn display_size(&self) -> (u32, u32) {
        self.orientation.display_size(self.width, self.height)
    }

    /// The embedded thumbnail's bytes in `file` (the bytes this header was read from).
    pub fn thumbnail<'a>(&self, file: &'a [u8]) -> Option<&'a [u8]> {
        file.get(self.thumbnail.clone()?)
    }

    fn with_exif(mut self, block: Option<(&[u8], usize)>, limits: &Limits) -> Header {
        if let Some((tiff, at)) = block {
            match exif::read(tiff, limits) {
                Ok(e) => {
                    self.orientation = e.orientation.and_then(Orientation::new).unwrap_or_default();
                    self.thumbnail = e.thumbnail.as_ref().map(|r| r.start + at..r.end + at);
                    self.exif = Some(e);
                }
                Err(e) => self.exif_error = Some(e),
            }
        }
        self
    }
}

/// Reads the header of an image held in memory.
pub fn read_header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    match sniff(bytes) {
        Some(Format::Jpeg) => jpeg_header(bytes, limits),
        Some(Format::Png) => png_header(bytes, limits),
        Some(other) => Err(Error::Unsupported(other)),
        None if bytes.len() < sniff::SNIFF_LEN => Err(Error::Truncated),
        None => Err(Error::Unknown),
    }
}

/// Reads the header of an image file, reading only as much of it as the header needs (from
/// 256 KiB, doubling up to [`Limits::max_header_bytes`]). Returns the header and the bytes read,
/// which hold the embedded thumbnail if there is one.
pub fn read_header_file(path: &Path, limits: &Limits) -> Result<(Header, Vec<u8>)> {
    let file = std::fs::File::open(path).map_err(|e| Error::Io(e.to_string()))?;
    let size = file.metadata().map_err(|e| Error::Io(e.to_string()))?.len();
    let mut file = file.take(limits.max_header_bytes);
    let mut bytes = Vec::new();
    let mut want: u64 = 256 * 1024;
    loop {
        let have = bytes.len() as u64;
        let target = want.min(limits.max_header_bytes);
        if target > have {
            (&mut file).take(target - have).read_to_end(&mut bytes).map_err(|e| Error::Io(e.to_string()))?;
        }
        let complete = bytes.len() as u64 >= size.min(limits.max_header_bytes);
        match read_header(&bytes, limits) {
            Err(Error::Truncated) if !complete => want = want.saturating_mul(2),
            result => return result.map(|h| (h, bytes)),
        }
    }
}

/// Bytes a JPEG segment's own length field says it has (after the two marker bytes).
fn u16_be(b: &[u8], at: usize) -> Result<u16> {
    match b.get(at..at + 2) {
        Some(&[hi, lo]) => Ok(u16::from_be_bytes([hi, lo])),
        _ => Err(Error::Truncated),
    }
}

fn u32_be(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at + 4) {
        Some(&[a, b2, c, d]) => Ok(u32::from_be_bytes([a, b2, c, d])),
        _ => Err(Error::Truncated),
    }
}

/// Walks JPEG segments from SOI to the first SOF (ITU T.81 §B.1), picking up an EXIF APP1 on the way.
fn jpeg_header(b: &[u8], limits: &Limits) -> Result<Header> {
    let mut pos = 2;
    let mut exif_block = None;
    for _ in 0..limits.max_segments {
        if b.get(pos) != Some(&0xff) {
            return Err(if pos >= b.len() { Error::Truncated } else { Error::Malformed(format!("no marker at byte {pos}")) });
        }
        // Fill bytes: any number of 0xFF before the marker code.
        while b.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *b.get(pos).ok_or(Error::Truncated)?;
        pos += 1;
        match marker {
            // Markers without a length: TEM, RSTn, SOI.
            0x01 | 0xd0..=0xd8 => continue,
            0xd9 => return Err(Error::Malformed("end of image before a frame header".into())),
            0xda => return Err(Error::Malformed("scan data before a frame header".into())),
            _ => {}
        }
        let len = usize::from(u16_be(b, pos)?);
        if len < 2 {
            return Err(Error::Malformed(format!("segment length {len}")));
        }
        let seg = b.get(pos + 2..pos + len).ok_or(Error::Truncated)?;
        match marker {
            // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC).
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let (height, width) = (u16_be(seg, 1)?, u16_be(seg, 3)?);
                if width == 0 || height == 0 {
                    return Err(Error::Malformed(format!("frame size {width} × {height}")));
                }
                let header = Header {
                    format: Format::Jpeg,
                    width: u32::from(width),
                    height: u32::from(height),
                    orientation: Orientation::UPRIGHT,
                    exif: None,
                    exif_error: None,
                    thumbnail: None,
                };
                return Ok(header.with_exif(exif_block, limits));
            }
            0xe1 if exif_block.is_none() && seg.starts_with(b"Exif\0\0") => {
                exif_block = seg.get(6..).map(|t| (t, pos + 2 + 6));
            }
            _ => {}
        }
        pos += len;
    }
    Err(Error::TooMany("JPEG segments"))
}

/// The IHDR chunk and an `eXIf` chunk before the image data (PNG 1.2, Extensions 1.5).
fn png_header(b: &[u8], limits: &Limits) -> Result<Header> {
    let mut pos = 8;
    let mut size = None;
    let mut exif_block = None;
    for _ in 0..limits.max_segments {
        let len = u32_be(b, pos)? as usize;
        let kind = b.get(pos + 4..pos + 8).ok_or(Error::Truncated)?;
        let data_end = (pos + 8).checked_add(len).ok_or(Error::OutOfRange)?;
        if size.is_none() && kind != b"IHDR" {
            return Err(Error::Malformed("the first chunk is not IHDR".into()));
        }
        match kind {
            b"IHDR" => {
                let data = b.get(pos + 8..data_end).ok_or(Error::Truncated)?;
                let (w, h) = (u32_be(data, 0)?, u32_be(data, 4)?);
                if len != 13 || w == 0 || h == 0 || w > i32::MAX as u32 || h > i32::MAX as u32 {
                    return Err(Error::Malformed(format!("IHDR {w} × {h}, {len} bytes")));
                }
                size = Some((w, h));
            }
            b"eXIf" => {
                let data = b.get(pos + 8..data_end).ok_or(Error::Truncated)?;
                exif_block = Some((data, pos + 8));
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        pos = data_end.checked_add(4).ok_or(Error::OutOfRange)?;
    }
    let (width, height) = size.ok_or(Error::Truncated)?;
    let header = Header { format: Format::Png, width, height, orientation: Orientation::UPRIGHT, exif: None, exif_error: None, thumbnail: None };
    Ok(header.with_exif(exif_block, limits))
}

#[cfg(test)]
mod tests {
    use dalica_scenes::{ByteOrder, Exif as SceneExif, Scene, jpeg, png};
    use proptest::prelude::*;

    use super::*;

    fn camera_jpeg(o: u8) -> Vec<u8> {
        let exif = SceneExif {
            orientation: o,
            date_time_original: "2026-06-04T19:42:10".parse().ok(),
            make: Some("Dalica".into()),
            model: Some("Scenes 1".into()),
            thumbnail: Some(64),
            byte_order: if o.is_multiple_of(2) { ByteOrder::Big } else { ByteOrder::Little },
        };
        jpeg(&Scene::new(u64::from(o), 120, 80), 85, Some(&exif)).unwrap()
    }

    #[test]
    fn all_eight_orientations_read_from_scenes_files() {
        for o in 1..=8 {
            let bytes = camera_jpeg(o);
            let h = read_header(&bytes, &Limits::default()).unwrap();
            assert_eq!(h.format, Format::Jpeg);
            assert_eq!(h.orientation.get(), o);
            assert_eq!((h.width, h.height), if o >= 5 { (80, 120) } else { (120, 80) });
            assert_eq!(h.display_size(), (120, 80));
            let e = h.exif.as_ref().unwrap();
            assert_eq!(e.date_time_original.unwrap().to_string(), "2026-06-04T19:42:10");
            assert_eq!(e.make.as_deref(), Some("Dalica"));
            let thumb = h.thumbnail(&bytes).unwrap();
            let th = read_header(thumb, &Limits::default()).unwrap();
            assert_eq!((th.width, th.height), if o >= 5 { (42, 64) } else { (64, 42) });
        }
    }

    #[test]
    fn a_jpg_that_is_really_a_png_reads_as_png() {
        let dir = std::env::temp_dir().join(format!("dalica-image-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("holiday.jpg");
        std::fs::write(&path, png(&Scene::new(2, 30, 20)).unwrap()).unwrap();
        let (h, _) = read_header_file(&path, &Limits::default()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((h.format, h.width, h.height, h.orientation.get()), (Format::Png, 30, 20, 1));
        assert!(h.exif.is_none());
    }

    #[test]
    fn plain_files_have_no_exif() {
        let h = read_header(&jpeg(&Scene::new(3, 40, 30), 80, None).unwrap(), &Limits::default()).unwrap();
        assert_eq!((h.width, h.height, h.orientation.get(), h.exif, h.thumbnail), (40, 30, 1, None, None));
        assert!(matches!(
            read_header(b"GIF89a\x01\0\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", &Limits::default()),
            Err(Error::Unsupported(Format::Gif))
        ));
        assert!(matches!(read_header(&[0u8; 64], &Limits::default()), Err(Error::Unknown)));
        assert!(matches!(read_header(b"\xff\xd8\xff", &Limits::default()), Err(Error::Truncated)));
    }

    #[test]
    fn bad_exif_does_not_hide_the_photo() {
        let mut bytes = camera_jpeg(6);
        // Point IFD0 outside the EXIF block: the header still reads, upright as stored.
        let at = bytes.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6;
        let order_big = &bytes[at..at + 2] == b"MM";
        let v = if order_big { 0xffff_fff0u32.to_be_bytes() } else { 0xffff_fff0u32.to_le_bytes() };
        bytes[at + 4..at + 8].copy_from_slice(&v);
        let h = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((h.width, h.height, h.orientation.get()), (80, 120, 1));
        assert_eq!(h.exif_error, Some(Error::OutOfRange));
    }

    #[test]
    fn png_with_an_exif_chunk() {
        // IHDR, then an eXIf chunk carrying orientation 6, then the rest of a real PNG.
        let real = png(&Scene::new(4, 16, 10)).unwrap();
        let tiff = dalica_scenes::exif::tiff(&SceneExif { orientation: 6, ..SceneExif::default() }, 16, 10, None).unwrap();
        let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
        chunk.extend(b"eXIf");
        chunk.extend(&tiff);
        chunk.extend([0, 0, 0, 0]); // CRC (not checked by the header reader)
        let ihdr_end = 8 + 8 + 13 + 4;
        let bytes = [&real[..ihdr_end], &chunk, &real[ihdr_end..]].concat();
        let h = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((h.width, h.height, h.orientation.get(), h.display_size()), (16, 10, 6, (10, 16)));
    }

    /// Every prefix of real files: `Ok` or `Err`, never a panic, and never a wrong size.
    #[test]
    fn truncation_at_every_offset_never_panics() {
        for bytes in [camera_jpeg(6), camera_jpeg(1), png(&Scene::new(5, 50, 40)).unwrap()] {
            let full = read_header(&bytes, &Limits::default()).unwrap();
            for len in 0..bytes.len() {
                if let Ok(h) = read_header(&bytes[..len], &Limits::default()) {
                    assert_eq!((h.width, h.height), (full.width, full.height), "prefix {len}");
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Truncation and corruption anywhere in a real file never panic.
        #[test]
        fn truncated_or_corrupted_files_never_panic(o in 1u8..=8, cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8)) {
            let mut bytes = camera_jpeg(o);
            for (at, v) in flips {
                let i = at.index(bytes.len());
                bytes[i] ^= v;
            }
            bytes.truncate(cut.index(bytes.len() + 1));
            let _ = read_header(&bytes, &Limits::default());
            if let Some(start) = bytes.windows(6).position(|w| w == b"Exif\0\0") {
                let _ = exif::read(&bytes[start + 6..], &Limits::default());
            }
        }

        #[test]
        fn random_bytes_never_panic(head in prop_oneof![Just(b"\xff\xd8\xff".to_vec()), Just(b"\x89PNG\r\n\x1a\n".to_vec()), Just(b"II*\0".to_vec()), Just(b"MM\0*".to_vec())], tail in proptest::collection::vec(any::<u8>(), 0..512)) {
            let bytes = [head, tail].concat();
            let _ = read_header(&bytes, &Limits::default());
            let _ = exif::read(&bytes, &Limits::default());
        }
    }
}
