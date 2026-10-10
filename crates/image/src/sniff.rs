//! Recognising a file by its content, never by its name (`data-model.md` §7, `security.md` §4).
//! A `.jpg` that holds a PNG is a PNG.

/// The formats Dalica can tell apart from their first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    /// TIFF, and the TIFF-based RAW formats (told apart later, M8).
    Tiff,
    WebP,
    Gif,
    Bmp,
    /// HEIF/HEIC (`ftyp` brands `heic`, `heix`, `mif1`, …).
    Heif,
    Avif,
    JpegXl,
    Psd,
}

impl Format {
    /// Whether M1 reads and decodes it (JPEG and PNG; the rest arrive in M8).
    pub fn supported(self) -> bool {
        matches!(self, Format::Jpeg | Format::Png)
    }
}

/// Bytes [`sniff`] needs to decide.
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
    if at(4, b"ftyp") {
        let brand = head.get(8..12)?;
        return match brand {
            b"avif" | b"avis" => Some(Format::Avif),
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => Some(Format::Heif),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_numbers() {
        let cases: [(&[u8], Option<Format>); 14] = [
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
            (b"\0\0\0\x18ftypmp42\0\0\0\0", None),
            (b"BMx", None),
            (b"", None),
        ];
        for (head, want) in cases {
            assert_eq!(sniff(head), want, "{head:?}");
        }
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
        assert!(Format::Jpeg.supported() && Format::Png.supported() && !Format::Heif.supported());
    }
}
