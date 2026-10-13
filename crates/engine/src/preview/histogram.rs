//! The 256-bin RGB histogram of a preview shown (`editing.md` §5.2): counted on the preview's
//! workers from the very pixels the edit view draws, never on the owner thread.

use dalica_image::Rgba8;

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
        let (mut red, mut green, mut blue) = ([0u32; BINS], [0u32; BINS], [0u32; BINS]);
        let (pixels, _) = image.data.as_chunks::<4>();
        for &[r, g, b, _] in pixels {
            // A `u8` always indexes the 256 bins.
            red[usize::from(r)] = red[usize::from(r)].saturating_add(1);
            green[usize::from(g)] = green[usize::from(g)].saturating_add(1);
            blue[usize::from(b)] = blue[usize::from(b)].saturating_add(1);
        }
        Histogram { red: red.to_vec(), green: green.to_vec(), blue: blue.to_vec(), pixels: pixels.len() as u64 }
    }
}

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
