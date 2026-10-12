//! Decode and parse limits (`security.md` §4.2). Every reader takes them explicitly; nothing
//! relies on a library's defaults. A change to a value needs a test at the new boundary.
//!
//! Besides the static [`Limits`], a decode carries a [`Budget`]: every buffer it allocates for
//! pixels is charged to the allocation limit and reserved fallibly, so running out of memory is an
//! [`Error::TooLarge`], never an abort. Container walks we own count their steps with [`Work`].

use crate::{Error, Result};

/// Limits for one read or decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Longest edge, in pixels.
    pub max_side: u32,
    /// Most pixels in one image.
    pub max_pixels: u64,
    /// Most bytes one decode may allocate for its pixels (decoded samples and the 8-bit output
    /// together, charged through a [`Budget`]).
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
    /// Most JPEG segments or PNG chunks looked at in one file.
    pub max_segments: usize,
    /// Most bytes of a file read to find its header.
    pub max_header_bytes: u64,
    /// Largest ICC profile, after APP2 reassembly or `iCCP` inflation (`security.md` §4.2).
    pub max_icc: usize,
    /// Most tags in an ICC profile's tag table.
    pub max_icc_tags: u32,
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
            max_icc: 16 * 1024 * 1024,
            max_icc_tags: 1000,
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

/// The bytes one decode may still allocate. Every pixel buffer is taken through [`Budget::vec`],
/// which charges it here first and then reserves it with `try_reserve_exact`, so neither a hostile
/// header nor a real allocation failure can abort the process.
#[derive(Debug)]
pub struct Budget {
    left: u64,
    /// The image size to name in a "too large" error.
    size: (u32, u32),
}

impl Budget {
    /// A fresh budget of `limits.max_alloc` bytes for an image of `width × height`.
    pub fn new(limits: &Limits, width: u32, height: u32) -> Budget {
        Budget { left: limits.max_alloc, size: (width, height) }
    }

    /// Bytes not yet charged.
    pub fn left(&self) -> u64 {
        self.left
    }

    fn too_large(&self) -> Error {
        Error::TooLarge { width: self.size.0, height: self.size.1 }
    }

    /// Charges `bytes` without allocating (for memory a third-party decoder allocates itself).
    pub fn charge(&mut self, bytes: u64) -> Result<()> {
        self.left = self.left.checked_sub(bytes).ok_or_else(|| self.too_large())?;
        Ok(())
    }

    /// Gives back `bytes` charged earlier (a buffer that was dropped).
    pub fn refund(&mut self, bytes: u64) {
        self.left = self.left.saturating_add(bytes);
    }

    /// A vector of `len` copies of `fill`, charged and reserved fallibly.
    pub fn vec<T: Clone>(&mut self, len: usize, fill: T) -> Result<Vec<T>> {
        let bytes = (len as u64).checked_mul(std::mem::size_of::<T>() as u64).ok_or_else(|| self.too_large())?;
        self.charge(bytes)?;
        let mut v = Vec::new();
        if v.try_reserve_exact(len).is_err() {
            self.refund(bytes);
            return Err(self.too_large());
        }
        v.resize(len, fill);
        Ok(v)
    }
}

/// A step counter for a container walk we own (JPEG segments, PNG chunks, IFD chains, APP2
/// chunks): highly fragmented but size-bounded files stop at `max` steps (`security.md` §4.3).
#[derive(Debug)]
pub struct Work {
    left: usize,
    what: &'static str,
}

impl Work {
    pub fn new(max: usize, what: &'static str) -> Work {
        Work { left: max, what }
    }

    /// Counts one step; `Err(TooMany(what))` once the limit is used up.
    pub fn step(&mut self) -> Result<()> {
        self.left = self.left.checked_sub(1).ok_or(Error::TooMany(self.what))?;
        Ok(())
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

    #[test]
    fn the_budget_is_charged_at_its_boundary() {
        let l = Limits { max_alloc: 1000, ..Limits::default() };
        let mut b = Budget::new(&l, 10, 10);
        let v = b.vec(249, 0u32).unwrap();
        assert_eq!((v.len(), b.left()), (249, 4));
        assert!(b.vec(1, 0u32).is_ok());
        assert_eq!(b.left(), 0);
        assert_eq!(b.vec(1, 0u8), Err(Error::TooLarge { width: 10, height: 10 }));
        b.refund(8);
        assert!(b.charge(8).is_ok() && b.charge(1).is_err());
        // A length whose byte size overflows is refused, not wrapped.
        let mut b = Budget::new(&Limits { max_alloc: u64::MAX, ..l }, 1, 1);
        assert!(b.vec(usize::MAX, 0u64).is_err());
        // An allocation the system cannot satisfy is an error, not an abort.
        assert!(b.vec(usize::MAX / 16, 0u8).is_err());
    }

    #[test]
    fn work_stops_at_its_limit() {
        let mut w = Work::new(2, "steps");
        assert!(w.step().is_ok() && w.step().is_ok());
        assert_eq!(w.step(), Err(Error::TooMany("steps")));
    }
}
