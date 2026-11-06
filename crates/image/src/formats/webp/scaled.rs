//! A lossy WebP's thumbnail at 1/2, 1/4 or 1/8 of its size, reduced as the decoder's rows come
//! (`formats.md` §1): each output pixel is the exact mean of the `s × s` source pixels it covers
//! (fewer at the right and bottom edges), so the result is what fitting the full decode would
//! start from, and the memory held is one row of sums and the reduced picture.

use crate::limits::Budget;
use crate::{Error, Result};

/// The largest reduction (8, 4, 2) whose long side still covers `long_side`; 1 if none does.
pub(crate) fn factor(width: u32, height: u32, long_side: u32) -> u32 {
    let long = width.max(height);
    [8u32, 4, 2].into_iter().find(|&s| long_side > 0 && long.div_ceil(s) >= long_side).unwrap_or(1)
}

/// Sums the rows of a `width × height` RGB picture into its `1/s` reduction.
pub(crate) struct Reducer {
    shift: u32,
    width: usize,
    height: usize,
    pub(crate) out_width: usize,
    pub(crate) out_height: usize,
    sums: Vec<u32>,
    pub(crate) out: Vec<u8>,
}

impl Reducer {
    /// `s` is 1, 2, 4 or 8.
    pub(crate) fn new(width: u32, height: u32, s: u32, budget: &mut Budget) -> Result<Reducer> {
        if !s.is_power_of_two() || s > 8 {
            return Err(Error::Malformed(format!("a reduction of 1/{s}")));
        }
        let (width, height) = (width as usize, height as usize);
        let s = s as usize;
        let (out_width, out_height) = (width.div_ceil(s), height.div_ceil(s));
        let len = out_width.checked_mul(out_height).and_then(|p| p.checked_mul(3)).ok_or(Error::OutOfRange)?;
        Ok(Reducer {
            shift: s.trailing_zeros(),
            width,
            height,
            out_width,
            out_height,
            sums: budget.vec(out_width * 3, 0u32)?,
            out: budget.vec(len, 0u8)?,
        })
    }

    /// Adds source row `y` (`width × 3` bytes); the last row of each band writes its output row.
    pub(crate) fn row(&mut self, y: usize, rgb: &[u8]) -> Result<()> {
        let shift = self.shift;
        for (x, px) in rgb.as_chunks::<3>().0.iter().take(self.width).enumerate() {
            let at = (x >> shift) * 3;
            if let Some(sum) = self.sums.get_mut(at..at + 3) {
                for (s, &v) in sum.iter_mut().zip(px) {
                    *s += u32::from(v);
                }
            }
        }
        let s = 1usize << shift;
        if !(y + 1).is_multiple_of(s) && y + 1 != self.height {
            return Ok(());
        }
        let band = y >> shift;
        let rows = (y - (band << shift) + 1) as u32;
        let start = band.checked_mul(self.out_width * 3).ok_or(Error::OutOfRange)?;
        let out = self.out.get_mut(start..start + self.out_width * 3).ok_or(Error::OutOfRange)?;
        for (ox, (o, sum)) in out.as_chunks_mut::<3>().0.iter_mut().zip(self.sums.as_chunks_mut::<3>().0).enumerate() {
            let cols = (self.width - (ox << shift)).min(s) as u32;
            let n = cols * rows;
            for (o, sum) in o.iter_mut().zip(sum.iter_mut()) {
                *o = ((*sum + n / 2) / n) as u8;
                *sum = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    #[test]
    fn the_factor_is_the_largest_that_covers_the_long_side() {
        assert_eq!(factor(16_000, 12_000, 256), 8);
        assert_eq!(factor(6000, 4000, 256), 8);
        assert_eq!(factor(1000, 800, 256), 2);
        assert_eq!(factor(1000, 800, 200), 4);
        assert_eq!(factor(300, 200, 256), 1);
        assert_eq!(factor(2041, 10, 256), 8);
        assert_eq!(factor(2040, 10, 256), 4);
        assert_eq!(factor(100, 100, 0), 1);
    }

    #[test]
    fn bands_are_exact_means_with_short_edges() {
        // 5 × 3 at 1/2: output 3 × 2; the last column and row cover one source pixel each.
        let mut b = Budget::new(&Limits::default(), 5, 3);
        let mut r = Reducer::new(5, 3, 2, &mut b).unwrap();
        let px = |y: usize| -> Vec<u8> { (0..5).flat_map(|x| [(10 * x + y) as u8, 100, 255]).collect() };
        for y in 0..3 {
            r.row(y, &px(y)).unwrap();
        }
        assert_eq!((r.out_width, r.out_height), (3, 2));
        // (0, 1, 10, 11) / 4 = 5.5 → 6; (20, 21, 30, 31) → 25.5 → 26; (40, 41) → 40.5 → 41.
        assert_eq!(&r.out[..9], &[6, 100, 255, 26, 100, 255, 41, 100, 255]);
        // The last row alone: (2 + 12) / 2 = 7, (22 + 32) / 2 = 27, 42.
        assert_eq!(&r.out[9..], &[7, 100, 255, 27, 100, 255, 42, 100, 255]);
    }
}
