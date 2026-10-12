//! ISO base media file format (ISO/IEC 14496-12) boxes, shared by CR3, HEIF and AVIF.
//!
//! For now only the `ftyp` major brand that [`crate::sniff`] needs. M5.10 adds the bounded box
//! walk here (nesting ≤ 12, boxes per level capped, `security.md` §4.2).

/// The major brand of a file that starts with an `ftyp` box, read from its first 12 bytes.
pub fn major_brand(head: &[u8]) -> Option<[u8; 4]> {
    if head.get(4..8) != Some(b"ftyp") {
        return None;
    }
    match head.get(8..12) {
        Some(&[a, b, c, d]) => Some([a, b, c, d]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_major_brand_follows_ftyp() {
        assert_eq!(major_brand(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(*b"heic"));
        assert_eq!(major_brand(b"\0\0\0\x18ftyphei"), None);
        assert_eq!(major_brand(b"\0\0\0\x18moovheic"), None);
    }
}
