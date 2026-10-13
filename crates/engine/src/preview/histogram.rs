//! The 256-bin RGB histogram of a preview shown (`editing.md` §5.2): counted on the preview's
//! workers from the very pixels the edit view draws, never on the owner thread.

use dalica_image::Rgba8;
use rayon::prelude::*;

/// Bins per channel.
pub const BINS: usize = 256;

/// Counts of each 8-bit value of the red, green and blue channels (alpha is not counted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Histogram {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    /// The pixels counted.
    pub pixels: u64,
}

impl Default for Histogram {
    fn default() -> Histogram {
        Histogram { red: vec![0; BINS], green: vec![0; BINS], blue: vec![0; BINS], pixels: 0 }
    }
}

impl Histogram {
    /// The histogram of `image`; a buffer that does not hold whole pixels counts the whole pixels
    /// it holds.
    pub fn of(image: &Rgba8) -> Histogram {
        let (pixels, _) = image.data.as_chunks::<4>();
        // Counted in parallel runs of rows' worth of pixels, then summed.
        let counts = pixels
            .par_chunks(RUN)
            .map(|run| {
                let mut c = [[0u32; BINS]; 3];
                for &[r, g, b, _] in run {
                    // A `u8` always indexes the 256 bins.
                    c[0][usize::from(r)] += 1;
                    c[1][usize::from(g)] += 1;
                    c[2][usize::from(b)] += 1;
                }
                c
            })
            .reduce(
                || [[0u32; BINS]; 3],
                |mut a, b| {
                    for (x, y) in a.iter_mut().flatten().zip(b.iter().flatten()) {
                        *x = x.saturating_add(*y);
                    }
                    a
                },
            );
        let [red, green, blue] = counts;
        Histogram { red: red.to_vec(), green: green.to_vec(), blue: blue.to_vec(), pixels: pixels.len() as u64 }
    }
}

/// Pixels per parallel run (a run counts at most this many, so its counts never overflow).
const RUN: usize = 1 << 16;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_each_channel_and_ignores_a_partial_pixel() {
        let image = Rgba8 { width: 2, height: 1, data: vec![1, 2, 3, 255, 1, 9, 3, 0, 7] };
        let h = Histogram::of(&image);
        assert_eq!(h.pixels, 2);
        assert_eq!((h.red[1], h.green[2], h.green[9], h.blue[3]), (2, 1, 1, 2));
        assert_eq!(h.red.iter().sum::<u32>(), 2);
    }
}
