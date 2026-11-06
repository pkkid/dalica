//! The boolean entropy decoder of VP8 (RFC 6386 §7), over a borrowed partition.
//!
//! Adapted from `image-webp` 0.2.4 `src/vp8_arithmetic_decoder.rs` (github.com/image-rs/image-webp,
//! MIT OR Apache-2.0; `NOTICE`): the same arithmetic, read a byte at a time from the file's own
//! bytes instead of copied 4-byte chunks. As there, one zero byte past the end is allowed (libwebp
//! accepts streams that read it); a read beyond that sets [`BoolDecoder::past_end`] and returns 0,
//! and the caller turns that into "the file ends too early" at the next check.

use super::tables::Prob;

pub(super) struct BoolDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    value: u64,
    range: u32,
    bit_count: i32,
    /// The zero byte past the end has been used.
    padded: bool,
    past_end: bool,
}

impl<'a> BoolDecoder<'a> {
    pub(super) fn new(data: &'a [u8]) -> BoolDecoder<'a> {
        BoolDecoder { data, pos: 0, value: 0, range: 255, bit_count: -8, padded: false, past_end: false }
    }

    /// A read went past the end of the partition (and its one byte of slack).
    pub(super) fn past_end(&self) -> bool {
        self.past_end
    }

    pub(super) fn read_bool(&mut self, prob: Prob) -> bool {
        if self.bit_count < 0 {
            // Four bytes at once while they last (the arithmetic only sees more bits waiting).
            if let Some(&[a, b, c, d]) = self.data.get(self.pos..self.pos + 4) {
                self.pos += 4;
                self.value = (self.value << 32) | u64::from(u32::from_be_bytes([a, b, c, d]));
                self.bit_count += 32;
                return self.decide(prob);
            }
            let byte = match self.data.get(self.pos) {
                Some(&b) => {
                    self.pos += 1;
                    b
                }
                None if !self.padded => {
                    self.padded = true;
                    0
                }
                None => {
                    self.past_end = true;
                    return false;
                }
            };
            self.value = (self.value << 8) | u64::from(byte);
            self.bit_count += 8;
        }
        self.decide(prob)
    }

    /// One bool from the bits already loaded (`bit_count >= 0`).
    fn decide(&mut self, prob: Prob) -> bool {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        let bigsplit = u64::from(split) << self.bit_count;
        let bit = match self.value.checked_sub(bigsplit) {
            Some(v) => {
                self.range -= split;
                self.value = v;
                true
            }
            None => {
                self.range = split;
                false
            }
        };
        // Renormalise: `range` back to 128..=255.
        let shift = self.range.leading_zeros().saturating_sub(24);
        self.range <<= shift;
        self.bit_count -= shift as i32;
        bit
    }

    pub(super) fn read_flag(&mut self) -> bool {
        self.read_bool(128)
    }

    /// An unsigned `n`-bit literal, most significant bit first.
    pub(super) fn read_literal(&mut self, n: u8) -> u8 {
        (0..n).fold(0u8, |v, _| (v << 1) | u8::from(self.read_flag()))
    }

    /// A flag, then if set an `n`-bit magnitude and a sign (§9.3, §9.6).
    pub(super) fn read_optional_signed(&mut self, n: u8) -> i32 {
        if !self.read_flag() {
            return 0;
        }
        let magnitude = i32::from(self.read_literal(n));
        if self.read_flag() { -magnitude } else { magnitude }
    }

    /// A value coded with `tree` and its node probabilities, starting at entry `start` (§8.1).
    pub(super) fn read_tree(&mut self, tree: &[i8], probs: &[Prob], start: usize) -> i8 {
        let mut i = start;
        // A well-formed tree ends within its length in reads; the bound stops a malformed one.
        for _ in 0..tree.len() {
            let prob = probs.get(i >> 1).copied().unwrap_or(128);
            let next = tree.get(i + usize::from(self.read_bool(prob))).copied().unwrap_or(0);
            if next <= 0 {
                return -next;
            }
            i = next as usize;
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6386 §7.3's encoder, for round trips.
    pub(crate) struct Encoder {
        out: Vec<u8>,
        range: u32,
        bottom: u32,
        bit_count: i32,
    }

    impl Encoder {
        pub(crate) fn new() -> Encoder {
            Encoder { out: Vec::new(), range: 255, bottom: 0, bit_count: 24 }
        }

        fn add_one(&mut self) {
            for b in self.out.iter_mut().rev() {
                if *b == 255 {
                    *b = 0;
                } else {
                    *b += 1;
                    return;
                }
            }
        }

        pub(crate) fn put(&mut self, prob: u8, bit: bool) {
            let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
            if bit {
                self.bottom = self.bottom.wrapping_add(split);
                self.range -= split;
            } else {
                self.range = split;
            }
            while self.range < 128 {
                self.range <<= 1;
                if self.bottom & (1 << 31) != 0 {
                    self.add_one();
                }
                self.bottom <<= 1;
                self.bit_count -= 1;
                if self.bit_count == 0 {
                    self.out.push((self.bottom >> 24) as u8);
                    self.bottom &= (1 << 24) - 1;
                    self.bit_count = 8;
                }
            }
        }

        pub(crate) fn finish(mut self) -> Vec<u8> {
            for _ in 0..32 {
                self.put(128, false);
            }
            self.out
        }
    }

    #[test]
    fn bools_round_trip_and_the_end_is_noticed() {
        let bits: Vec<(u8, bool)> = (0..2000u32).map(|i| ((i * 37 % 254 + 1) as u8, (i * 7919) % 5 < 2)).collect();
        let mut e = Encoder::new();
        for &(p, b) in &bits {
            e.put(p, b);
        }
        let data = e.finish();
        let mut d = BoolDecoder::new(&data);
        for &(p, b) in &bits {
            assert_eq!(d.read_bool(p), b);
        }
        assert!(!d.past_end());
        // Reading on runs out: one byte of slack, then the end.
        let mut d = BoolDecoder::new(&[0x55]);
        for _ in 0..64 {
            d.read_flag();
        }
        assert!(d.past_end());
        // A tree whose entries loop is still read in bounded steps.
        let mut d = BoolDecoder::new(&[0xff; 8]);
        assert_eq!(d.read_tree(&[2, 2, 2, 2], &[1, 1], 0), 0);
    }
}
