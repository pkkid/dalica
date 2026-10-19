//! Recognising a file by its content, never by its name (`data-model.md` §7, `security.md` §4).
//! A `.jpg` that holds a PNG is a PNG, and a `.tif` that holds a camera's raw data is a RAW.
//!
//! Magic numbers decide almost everything from the first [`SNIFF_LEN`] bytes. The RAW containers
//! are told apart by their own modules (`formats::raw::*::recognise`), so the tasks that fill them
//! refine recognition without touching this file. TIFF-structured raws (DNG, NEF, ARW, PEF, SRW)
//! share TIFF's magic number: when `head` holds more than the magic number, a bounded walk of IFD0
//! and its sub-IFDs tells them apart; with only the first bytes they sniff as [`Format::Tiff`].

use crate::formats;

/// The formats Dalica can tell apart from their content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    /// TIFF (not a camera raw).
    Tiff,
    WebP,
    Gif,
    Bmp,
    /// HEIF/HEIC (`ftyp` brands `heic`, `heix`, `mif1`, …).
    Heif,
    /// Recognised and listed, never decoded (`formats.md` §1).
    Avif,
    JpegXl,
    Psd,
    /// A camera raw file; only its embedded preview is shown (`formats.md` §2).
    Raw(RawKind),
    /// A video: recognised and probed for its facts (`formats::video::probe`), not decoded yet.
    Video(VideoKind),
}

/// The RAW container families, each with its own module under `formats/raw/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RawKind {
    /// TIFF-structured raws: DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW, …
    TiffBased,
    /// Canon CR3 (ISOBMFF).
    Cr3,
    /// Fujifilm RAF.
    Raf,
    /// Canon CRW (CIFF).
    Crw,
    /// Minolta MRW.
    Mrw,
    /// Sigma X3F.
    X3f,
}

impl RawKind {
    pub const ALL: [RawKind; 6] = [RawKind::TiffBased, RawKind::Cr3, RawKind::Raf, RawKind::Crw, RawKind::Mrw, RawKind::X3f];
}

/// The video containers (`formats.md` §1), told apart by `formats::video::recognise`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VideoKind {
    /// ISO base media (MP4, 3GP): an `ftyp` brand such as `isom`, `mp41`, `mp42` or `avc1`.
    Mp4,
    /// The M4V flavour of MP4 (brands `M4V `, `M4VH`, `M4VP`).
    M4v,
    /// QuickTime (brand `qt  `, or an older movie that starts with its `moov` or `mdat` box).
    QuickTime,
    /// Matroska (an EBML header whose DocType is `matroska`).
    Matroska,
}

impl VideoKind {
    pub const ALL: [VideoKind; 4] = [VideoKind::Mp4, VideoKind::M4v, VideoKind::QuickTime, VideoKind::Matroska];

    /// The name shown to people ("MP4", "MOV", …).
    pub fn name(self) -> &'static str {
        match self {
            VideoKind::Mp4 => "MP4",
            VideoKind::M4v => "M4V",
            VideoKind::QuickTime => "MOV",
            VideoKind::Matroska => "MKV",
        }
    }
}

impl Format {
    /// Every still-image format, RAW kinds included (what the image paths iterate). The video
    /// kinds are [`VideoKind::ALL`]: no image path reads them.
    pub const ALL: [Format; 16] = [
        Format::Jpeg,
        Format::Png,
        Format::Tiff,
        Format::WebP,
        Format::Gif,
        Format::Bmp,
        Format::Heif,
        Format::Avif,
        Format::JpegXl,
        Format::Psd,
        Format::Raw(RawKind::TiffBased),
        Format::Raw(RawKind::Cr3),
        Format::Raw(RawKind::Raf),
        Format::Raw(RawKind::Crw),
        Format::Raw(RawKind::Mrw),
        Format::Raw(RawKind::X3f),
    ];

    /// Whether this build reads and decodes it (each format module says so for itself).
    pub fn supported(self) -> bool {
        formats::decodes(self)
    }

    /// The name shown to people ("JPEG", "JPEG XL", "RAW").
    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Tiff => "TIFF",
            Format::WebP => "WebP",
            Format::Gif => "GIF",
            Format::Bmp => "BMP",
            Format::Heif => "HEIC",
            Format::Avif => "AVIF",
            Format::JpegXl => "JPEG XL",
            Format::Psd => "PSD",
            Format::Raw(_) => "RAW",
            Format::Video(kind) => kind.name(),
        }
    }
}

/// Bytes [`sniff`] needs to decide by magic number.
pub const SNIFF_LEN: usize = 32;

/// The format of a file starting with `head`, or `None` if it is not one Dalica knows.
pub fn sniff(head: &[u8]) -> Option<Format> {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    if at(0, b"\xff\xd8\xff") {
        return Some(Format::Jpeg);
    }
    if at(0, b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    // The RAW containers first: CR2, ORF and RW2 start like (or almost like) a TIFF.
    if let Some(kind) = formats::raw::recognise(head) {
        return Some(Format::Raw(kind));
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") || at(0, b"II+\0") || at(0, b"MM\0+") {
        return Some(Format::Tiff);
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some(Format::WebP);
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some(Format::Gif);
    }
    // "BM", then a reserved field of zeros and a pixel offset past the 14-byte file header.
    if at(0, b"BM") && at(6, b"\0\0\0\0") && head.get(10).is_some_and(|&b| b >= 14) {
        return Some(Format::Bmp);
    }
    if at(0, b"\xff\x0a") || at(0, b"\0\0\0\x0cJXL \r\n\x87\n") {
        return Some(Format::JpegXl);
    }
    if at(0, b"8BPS") {
        return Some(Format::Psd);
    }
    // Videos: ISO BMFF brands that name MP4, M4V or QuickTime (never a still-image brand), and
    // Matroska by its EBML DocType.
    if let Some(kind) = formats::video::recognise(head) {
        return Some(Format::Video(kind));
    }
    match &formats::bmff::major_brand(head)? {
        b"avif" | b"avis" => Some(Format::Avif),
        b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => Some(Format::Heif),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_numbers() {
        let cases: [(&[u8], Option<Format>); 21] = [
            (b"\xff\xd8\xff\xe0\0\x10JFIF", Some(Format::Jpeg)),
            (b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", Some(Format::Png)),
            (b"II*\0\x08\0\0\0", Some(Format::Tiff)),
            (b"MM\0*\0\0\0\x08", Some(Format::Tiff)),
            (b"RIFF\x24\0\0\0WEBPVP8 ", Some(Format::WebP)),
            (b"GIF89a\x01\0\x01\0", Some(Format::Gif)),
            (b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0", Some(Format::Bmp)),
            (b"\0\0\0\x18ftypheic\0\0\0\0", Some(Format::Heif)),
            (b"\0\0\0\x1cftypavif\0\0\0\0", Some(Format::Avif)),
            (b"\xff\x0a\xfa", Some(Format::JpegXl)),
            (b"8BPS\0\x01", Some(Format::Psd)),
            (b"II*\0\x10\0\0\0CR\x02\0", Some(Format::Raw(RawKind::TiffBased))),
            (b"IIRO\x08\0\0\0", Some(Format::Raw(RawKind::TiffBased))),
            (b"IIU\0\x08\0\0\0", Some(Format::Raw(RawKind::TiffBased))),
            (b"\0\0\0\x18ftypcrx \0\0\0\x01", Some(Format::Raw(RawKind::Cr3))),
            (b"FUJIFILMCCD-RAW 0201", Some(Format::Raw(RawKind::Raf))),
            (b"II\x1a\0\0\0HEAPCCDR", Some(Format::Raw(RawKind::Crw))),
            (b"\0MRM\0\0\0\x08", Some(Format::Raw(RawKind::Mrw))),
            (b"\0\0\0\x18ftypmp42\0\0\0\0", Some(Format::Video(VideoKind::Mp4))),
            (b"BMx", None),
            (b"", None),
        ];
        for (head, want) in cases {
            assert_eq!(sniff(head), want, "{head:?}");
        }
        assert_eq!(sniff(b"FOVb\0\0\x04\0"), Some(Format::Raw(RawKind::X3f)));
        // A prefix of a magic number is not enough.
        assert_eq!(sniff(b"\xff\xd8"), None);
        assert_eq!(sniff(b"\x89PNG\r\n"), None);
    }

    #[test]
    fn content_wins_over_the_name() {
        // A PNG saved as `photo.jpg`: the bytes say PNG.
        let png = dalica_scenes::png(&dalica_scenes::Scene::new(1, 8, 8)).unwrap();
        assert_eq!(sniff(&png), Some(Format::Png));
        let jpg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, 8, 8), 80, None).unwrap();
        assert_eq!(sniff(&jpg), Some(Format::Jpeg));
        assert!(Format::Jpeg.supported() && Format::Png.supported());
        assert_eq!(Format::Heif.supported(), cfg!(feature = "heif"));
    }

    /// A TIFF whose IFD0 says DNG, or whose sub-IFD holds CFA data, is a raw; a plain TIFF is not.
    #[test]
    fn tiff_structured_raws_are_told_apart_by_their_ifds() {
        // Little-endian TIFF: IFD0 at 8 with one entry, next IFD 0.
        let tiff = |tag: u16, kind: u16, value: u32| {
            let mut b = b"II*\0\x08\0\0\0".to_vec();
            b.extend(1u16.to_le_bytes());
            b.extend(tag.to_le_bytes());
            b.extend(kind.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(value.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b
        };
        assert_eq!(sniff(&tiff(0xc612, 1, 0x0001_0401)), Some(Format::Raw(RawKind::TiffBased)), "DNGVersion");
        assert_eq!(sniff(&tiff(0x0106, 3, 32803)), Some(Format::Raw(RawKind::TiffBased)), "CFA");
        assert_eq!(sniff(&tiff(0x0106, 3, 2)), Some(Format::Tiff), "RGB");
        // A SubIFDs entry pointing at a CFA IFD (NEF, ARW).
        let mut b = tiff(0x014a, 4, 26);
        b.extend(1u16.to_le_bytes());
        b.extend(0x0106u16.to_le_bytes());
        b.extend(3u16.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(32803u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        assert_eq!(sniff(&b), Some(Format::Raw(RawKind::TiffBased)), "sub-IFD CFA");
        // A sub-IFD pointing back at IFD0 is a loop: still a TIFF, no hang.
        assert_eq!(sniff(&tiff(0x014a, 4, 8)), Some(Format::Tiff));
    }
}
