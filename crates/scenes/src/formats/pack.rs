//! The compressions our TIFF and PSD writers use: PackBits and TIFF-flavoured LZW written from the
//! public TIFF 6.0 specification (§9 and §13), and zlib through `fdeflate`.

use std::collections::HashMap;

/// PackBits (TIFF compression 32773; PSD RLE) of one row: runs of 2 to 128 equal bytes become
/// `(1 − n, byte)`, everything else literal blocks of up to 128 bytes `(n − 1, bytes…)`.
/// The header byte −128 is never written.
pub fn packbits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len() + row.len() / 128 + 1);
    let mut i = 0;
    while i < row.len() {
        let byte = row.get(i).copied().unwrap_or(0);
        let run = row.get(i..).unwrap_or(&[]).iter().take(128).take_while(|&&b| b == byte).count();
        if run >= 2 {
            out.push((1 - run as i16) as u8);
            out.push(byte);
            i += run;
            continue;
        }
        // A literal block: up to the next pair of equal bytes, at most 128 bytes.
        let start = i;
        while i < row.len() && i - start < 128 {
            if row.get(i + 1).is_some() && row.get(i) == row.get(i + 1) {
                break;
            }
            i += 1;
        }
        let block = row.get(start..i).unwrap_or(&[]);
        out.push((block.len() - 1) as u8);
        out.extend(block);
    }
    out
}

const CLEAR: u16 = 256;
const END: u16 = 257;
const FIRST: u16 = 258;
/// The table is reset when its next code would be 4094 (the TIFF limit is 4095 codes).
const LAST: u16 = 4094;

/// Packs codes most-significant bit first.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, code: u16, width: u32) {
        self.acc = (self.acc << width) | u32::from(code);
        self.n += width;
        while self.n >= 8 {
            self.n -= 8;
            self.out.push((self.acc >> self.n) as u8);
        }
        self.acc &= (1 << self.n) - 1;
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.acc << (8 - self.n)) as u8);
        }
        self.out
    }
}

/// TIFF LZW (compression 5) of one strip: MSB-first codes of 9 to 12 bits, a Clear code first and
/// whenever the table fills, and the code width growing one code early, as TIFF requires.
pub fn lzw(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits { out: Vec::with_capacity(data.len() / 2 + 8), acc: 0, n: 0 };
    let mut table: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = FIRST;
    let mut width = 9;
    bits.put(CLEAR, width);
    let Some((&first, rest)) = data.split_first() else {
        bits.put(END, width);
        return bits.finish();
    };
    let mut prefix = u16::from(first);
    for &byte in rest {
        if let Some(&code) = table.get(&(prefix, byte)) {
            prefix = code;
            continue;
        }
        bits.put(prefix, width);
        table.insert((prefix, byte), next);
        next += 1;
        if next == LAST {
            bits.put(CLEAR, width);
            table.clear();
            next = FIRST;
            width = 9;
        } else if u32::from(next) + 1 > (1 << width) {
            width += 1;
        }
        prefix = u16::from(byte);
    }
    bits.put(prefix, width);
    // The decoder adds a table entry for this last code too, and may widen before End.
    next += 1;
    if next == LAST {
        bits.put(CLEAR, width);
        width = 9;
    } else if u32::from(next) + 1 > (1 << width) {
        width += 1;
    }
    bits.put(END, width);
    bits.finish()
}

/// A zlib stream (TIFF compression 8, "Adobe Deflate").
pub fn deflate(data: &[u8]) -> Vec<u8> {
    fdeflate::compress_to_vec(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(mut p: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some((&h, rest)) = p.split_first() {
            let h = h as i8;
            assert_ne!(h, -128);
            if h >= 0 {
                let n = h as usize + 1;
                out.extend(&rest[..n]);
                p = &rest[n..];
            } else {
                out.extend(std::iter::repeat_n(rest[0], (1 - h as isize) as usize));
                p = &rest[1..];
            }
        }
        out
    }

    /// A reference TIFF LZW decoder (early change), enough to check the encoder.
    fn unlzw(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut acc, mut n, mut pos) = (0u32, 0u32, 0usize);
        let mut width = 9u32;
        let mut table: Vec<Vec<u8>> = Vec::new();
        let reset = |t: &mut Vec<Vec<u8>>| {
            t.clear();
            t.extend((0..=255u8).map(|b| vec![b]));
            t.push(vec![]);
            t.push(vec![]);
        };
        reset(&mut table);
        let mut prev: Option<Vec<u8>> = None;
        loop {
            while n < width {
                acc = (acc << 8) | u32::from(data[pos]);
                pos += 1;
                n += 8;
            }
            n -= width;
            let code = ((acc >> n) & ((1 << width) - 1)) as usize;
            if code == 257 {
                return out;
            }
            if code == 256 {
                reset(&mut table);
                width = 9;
                prev = None;
                continue;
            }
            let entry = if code < table.len() {
                table[code].clone()
            } else {
                let p = prev.clone().unwrap();
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            if let Some(p) = prev {
                let mut e = p;
                e.push(entry[0]);
                table.push(e);
            }
            out.extend(&entry);
            prev = Some(entry);
            if table.len() + 1 >= (1 << width) && width < 12 {
                width += 1;
            }
        }
    }

    #[test]
    fn packbits_round_trips_runs_literals_and_long_rows() {
        let mut long = vec![7u8; 300];
        long.extend((0..=255u8).cycle().take(400));
        long.extend([1, 1, 2, 3, 3, 3]);
        for row in [&[][..], &[5], &[5, 5], &[1, 2, 3], &long] {
            let p = packbits(row);
            assert_eq!(unpack(&p), row, "{row:?}");
        }
        assert_eq!(packbits(&[9; 3]), [0xfe, 9]);
        assert_eq!(packbits(&[1, 2, 3]), [2, 1, 2, 3]);
    }

    #[test]
    fn lzw_round_trips_including_table_resets() {
        let mut noisy = Vec::new();
        let mut s = 1u32;
        for _ in 0..60_000 {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            noisy.push((s >> 16) as u8);
        }
        for data in [&[][..], &[42], b"TOBEORNOTTOBEORTOBEORNOT", &[0; 10_000], &noisy] {
            assert_eq!(unlzw(&lzw(data)), data, "{} bytes", data.len());
        }
        // Each length around the width changes and the table limit ends cleanly.
        for len in [509, 510, 511, 512, 1021, 1022, 1023, 2045, 2046, 2047, 4092, 4093, 4094, 4095, 4096] {
            let data: Vec<u8> = noisy.iter().take(len).copied().collect();
            assert_eq!(unlzw(&lzw(&data)), data, "{len} bytes");
        }
    }
}
