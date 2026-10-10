//! Decode and parse limits (`security.md` §4.2). Every reader takes them explicitly; nothing
//! relies on a library's defaults. A change to a value needs a test at the new boundary.

use crate::{Error, Result};

/// Limits for one read or decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Longest edge, in pixels.
    pub max_side: u32,
    /// Most pixels in one image.
    pub max_pixels: u64,
    /// Most bytes one decode may allocate for its output.
    pub max_alloc: u64,
    /// Most IFDs followed in one TIFF structure.
    pub max_ifds: usize,
    /// Deepest chain of sub-IFDs (IFD0 → EXIF IFD → …).
    pub max_ifd_depth: usize,
    /// Most entries in one IFD.
    pub max_ifd_entries: usize,
    /// Longest string value read.
    pub max_string: usize,
    /// Largest embedded thumbnail or preview.
    pub max_thumbnail: u64,
    /// Most JPEG segments looked at before the image data.
    pub max_segments: usize,
    /// Most bytes of a file read to find its header.
    pub max_header_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_side: 65_535,
            max_pixels: 1 << 30,
            max_alloc: 1536 * 1024 * 1024,
            max_ifds: 1024,
            max_ifd_depth: 10,
            max_ifd_entries: 8192,
            max_string: 64 * 1024,
            max_thumbnail: 64 * 1024 * 1024,
            max_segments: 4096,
            max_header_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// Checks a declared size before anything is allocated for it: each side 1..=`max_side`,
    /// at most `max_pixels`, and `bytes_per_pixel` × pixels within `max_alloc`. Returns the
    /// allocation size in bytes.
    pub fn check_size(&self, width: u32, height: u32, bytes_per_pixel: u64) -> Result<usize> {
        let too_large = || Error::TooLarge { width, height };
        if width == 0 || height == 0 {
            return Err(Error::Malformed(format!("image size {width} × {height}")));
        }
        if width > self.max_side || height > self.max_side {
            return Err(too_large());
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.max_pixels {
            return Err(too_large());
        }
        let bytes = pixels.checked_mul(bytes_per_pixel).filter(|&b| b <= self.max_alloc).ok_or_else(too_large)?;
        usize::try_from(bytes).map_err(|_| too_large())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_checked_at_their_boundaries() {
        let l = Limits::default();
        assert_eq!(l.check_size(1, 1, 4).unwrap(), 4);
        assert!(l.check_size(65_535, 1, 4).is_ok());
        assert!(matches!(l.check_size(65_536, 1, 4), Err(Error::TooLarge { .. })));
        assert!(matches!(l.check_size(0, 10, 4), Err(Error::Malformed(_))));
        // 2^30 pixels is the cap; one row more is refused.
        let tight = Limits { max_alloc: u64::MAX, ..l };
        assert!(tight.check_size(32_768, 32_768, 4).is_ok());
        assert!(matches!(tight.check_size(32_768, 32_769, 4), Err(Error::TooLarge { .. })));
        // The allocation budget bites before the pixel cap.
        assert!(matches!(l.check_size(30_000, 30_000, 4), Err(Error::TooLarge { .. })));
        assert!(l.check_size(100_000, 100_000, 4).is_err());
    }
}
