//! The eight ways to turn and mirror a picture (the dihedral group of the rectangle): what a
//! recipe's `rotate`, `flip_h` and `flip_v` operations add up to, composed after the EXIF
//! orientation that turns the stored pixels upright, the displayed size that gives, and applying it
//! to RGBA pixels (the CPU oracle that Save and the renderer use; `editing.md` §10).
//!
//! A [`Dihedral`] mirrors left–right first (when it mirrors), then turns clockwise by whole quarter
//! turns. EXIF orientations are the same eight transforms: [`Dihedral::from_exif`] maps the value
//! that turns stored pixels upright, so `Dihedral::from_exif(o).then(recipe)` turns stored pixels
//! into what the user sees.

use dalica_image::{Orientation, Rgba8};

use crate::{Error, Result};

/// One of the eight rotations and mirrorings of a picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dihedral {
    /// Clockwise quarter turns after the mirroring, 0–3.
    quarters: u8,
    /// Whether the picture is mirrored left–right before turning.
    mirror: bool,
}

impl Dihedral {
    pub const IDENTITY: Dihedral = Dihedral { quarters: 0, mirror: false };
    pub const ROTATE_90: Dihedral = Dihedral { quarters: 1, mirror: false };
    pub const ROTATE_180: Dihedral = Dihedral { quarters: 2, mirror: false };
    pub const ROTATE_270: Dihedral = Dihedral { quarters: 3, mirror: false };
    /// Mirror left–right (`flip_h()`).
    pub const FLIP_H: Dihedral = Dihedral { quarters: 0, mirror: true };
    /// Mirror top–bottom (`flip_v()`): a left–right mirror, then a half turn.
    pub const FLIP_V: Dihedral = Dihedral { quarters: 2, mirror: true };
    /// Mirror across the top-left to bottom-right diagonal (EXIF 5).
    pub const TRANSPOSE: Dihedral = Dihedral { quarters: 3, mirror: true };
    /// Mirror across the top-right to bottom-left diagonal (EXIF 7).
    pub const TRANSVERSE: Dihedral = Dihedral { quarters: 1, mirror: true };

    /// All eight, in EXIF order (1–8).
    pub const ALL: [Dihedral; 8] = [
        Dihedral::IDENTITY,
        Dihedral::FLIP_H,
        Dihedral::ROTATE_180,
        Dihedral::FLIP_V,
        Dihedral::TRANSPOSE,
        Dihedral::ROTATE_90,
        Dihedral::TRANSVERSE,
        Dihedral::ROTATE_270,
    ];

    /// A left–right mirror (when `mirror`) followed by `quarters` clockwise quarter turns (modulo 4).
    pub fn new(quarters: u8, mirror: bool) -> Dihedral {
        Dihedral { quarters: quarters % 4, mirror }
    }

    /// Clockwise quarter turns after the mirroring, 0–3.
    pub fn quarters(self) -> u8 {
        self.quarters % 4
    }

    /// Whether the picture is mirrored left–right before turning.
    pub fn mirrored(self) -> bool {
        self.mirror
    }

    /// `self`, then `next`.
    pub fn then(self, next: Dihedral) -> Dihedral {
        // A mirror reverses the direction of the turns before it: F·Rᵃ = R⁻ᵃ·F.
        let carried = if next.mirror { 4 - self.quarters() } else { self.quarters() };
        Dihedral::new(next.quarters() + carried, self.mirror ^ next.mirror)
    }

    /// The transform that undoes `self`.
    pub fn inverse(self) -> Dihedral {
        // Every mirrored transform is its own inverse.
        if self.mirror { self } else { Dihedral::new(4 - self.quarters(), false) }
    }

    /// The transform that turns pixels stored with EXIF orientation `o` upright.
    pub fn from_exif(o: Orientation) -> Dihedral {
        Dihedral::ALL.get(usize::from(o.get()).saturating_sub(1)).copied().unwrap_or_default()
    }

    /// The EXIF orientation that stands for this transform.
    pub fn to_exif(self) -> Orientation {
        let value = Dihedral::ALL.iter().position(|&d| d == self).unwrap_or(0) + 1;
        u8::try_from(value).ok().and_then(Orientation::new).unwrap_or_default()
    }

    /// Whether the transform swaps width and height.
    pub fn swaps(self) -> bool {
        self.quarters() % 2 == 1
    }

    /// The size of a `width × height` picture after the transform.
    pub fn display_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.swaps() { (height, width) } else { (width, height) }
    }

    /// A short name: `identity`, `rotate90`, `rotate180`, `rotate270`, `flip_h`, `flip_v`,
    /// `transpose` or `transverse`.
    pub fn name(self) -> &'static str {
        match (self.quarters(), self.mirror) {
            (1, false) => "rotate90",
            (2, false) => "rotate180",
            (3, false) => "rotate270",
            (0, true) => "flip_h",
            (2, true) => "flip_v",
            (3, true) => "transpose",
            (1, true) => "transverse",
            _ => "identity",
        }
    }

    /// Where the pixel at `(x, y)` of a `width × height` picture lands after the transform.
    pub fn map_pixel(self, x: u32, y: u32, width: u32, height: u32) -> (u32, u32) {
        let (w1, h1) = (width.saturating_sub(1), height.saturating_sub(1));
        let x = if self.mirror { w1.saturating_sub(x) } else { x };
        match self.quarters() {
            1 => (h1.saturating_sub(y), x),
            2 => (w1.saturating_sub(x), h1.saturating_sub(y)),
            3 => (y, w1.saturating_sub(x)),
            _ => (x, y),
        }
    }

    /// The picture after the transform. Pixel-exact: every pixel moves, none is resampled. The
    /// identity returns `img` as it is.
    pub fn apply(self, img: Rgba8) -> Result<Rgba8> {
        let (width, height) = (img.width, img.height);
        let expected = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
        if expected != Some(img.data.len()) {
            return Err(Error::BadImage { width, height, len: img.data.len() });
        }
        if self == Dihedral::IDENTITY {
            return Ok(img);
        }
        let (out_w, out_h) = self.display_size(width, height);
        let mut out = vec![0u8; img.data.len()];
        let row_len = width as usize * 4;
        for (y, row) in (0..height).zip(img.data.chunks_exact(row_len.max(1))) {
            for (x, px) in (0..width).zip(row.as_chunks::<4>().0) {
                let (dx, dy) = self.map_pixel(x, y, width, height);
                let at = (dy as usize * out_w as usize + dx as usize) * 4;
                if let Some(dst) = out.get_mut(at..at + 4) {
                    dst.copy_from_slice(px);
                }
            }
        }
        Ok(Rgba8 { width: out_w, height: out_h, data: out })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_values_and_transforms_correspond_one_to_one() {
        for value in 1..=8u8 {
            let o = Orientation::new(value).unwrap();
            assert_eq!(Dihedral::from_exif(o).to_exif(), o);
            assert_eq!(Dihedral::from_exif(o).swaps(), o.swaps(), "EXIF {value}");
        }
    }

    #[test]
    fn composition_is_associative_with_an_identity_and_inverses() {
        for a in Dihedral::ALL {
            assert_eq!(a.then(Dihedral::IDENTITY), a);
            assert_eq!(Dihedral::IDENTITY.then(a), a);
            assert_eq!(a.then(a.inverse()), Dihedral::IDENTITY, "{}", a.name());
            assert_eq!(a.inverse().then(a), Dihedral::IDENTITY, "{}", a.name());
            for b in Dihedral::ALL {
                for c in Dihedral::ALL {
                    assert_eq!(a.then(b).then(c), a.then(b.then(c)));
                }
            }
        }
        let names: std::collections::HashSet<_> = Dihedral::ALL.iter().map(|d| d.name()).collect();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn a_buffer_of_the_wrong_length_is_refused() {
        let bad = Rgba8 { width: 2, height: 2, data: vec![0; 15] };
        assert_eq!(Dihedral::ROTATE_90.apply(bad), Err(Error::BadImage { width: 2, height: 2, len: 15 }));
        let huge = Rgba8 { width: u32::MAX, height: u32::MAX, data: vec![] };
        assert!(Dihedral::FLIP_H.apply(huge).is_err());
        let empty = Rgba8 { width: 0, height: 5, data: vec![] };
        assert_eq!(Dihedral::ROTATE_90.apply(empty).unwrap(), Rgba8 { width: 5, height: 0, data: vec![] });
    }
}
