//! Huffman coding for the lossless transforms, written from ITU-T T.81: the table specification
//! (§B.2.4.2, Annex C: code sizes and codes from BITS and HUFFVAL), decoding (§F.2.2.3 and Figure
//! F.16: MAXCODE, MINCODE and VALPTR, plus a 9-bit lookup of our own for the short codes), the
//! entropy-coded segment's byte stuffing (§F.1.2.3, §B.1.1.5), and optimal tables for given symbol
//! frequencies (Annex K.2: Figures K.1 code sizes, K.2 counting, K.3 limiting to 16 bits, K.4
//! sorting).

use crate::{Error, Result};

/// Bits looked up at once when decoding; longer codes take the MAXCODE walk.
const LOOKUP_BITS: u32 = 9;

/// A decoding table built from one DHT table (Annex C, §F.2.2.3).
#[derive(Clone, Debug)]
pub(super) struct Decoder {
    /// Per code length 1..=16 (index 0 unused): the largest code of that length, -1 if none.
    maxcode: [i32; 18],
    /// Per code length: the index in `values` of its first code, minus that code.
    valoff: [i32; 17],
    values: Vec<u8>,
    /// For each `LOOKUP_BITS`-bit prefix: `(length << 8) | value`, or 0 when the code is longer.
    lookup: Vec<u16>,
}

impl Decoder {
    /// Builds the table from BITS (code counts per length 1–16) and HUFFVAL. Counts whose codes
    /// do not fit their length are malformed (§C.2).
    pub(super) fn new(bits: &[u8; 16], values: &[u8]) -> Result<Decoder> {
        let total: usize = bits.iter().map(|&b| usize::from(b)).sum();
        if total != values.len() || total > 256 {
            return Err(Error::Malformed(format!("Huffman table with {total} codes for {} values", values.len())));
        }
        let mut maxcode = [-1i32; 18];
        let mut valoff = [0i32; 17];
        let mut lookup = vec![0u16; 1 << LOOKUP_BITS];
        let mut code: u32 = 0;
        let mut k: usize = 0;
        for (i, &count) in bits.iter().enumerate() {
            let len = i as u32 + 1;
            if count > 0 {
                valoff[len as usize] = k as i32 - code as i32;
                for _ in 0..count {
                    if code >= 1 << len {
                        return Err(Error::Malformed("Huffman codes overflow their length".into()));
                    }
                    if len <= LOOKUP_BITS {
                        let shift = LOOKUP_BITS - len;
                        let value = values.get(k).copied().unwrap_or(0);
                        let first = (code << shift) as usize;
                        for slot in lookup.iter_mut().skip(first).take(1 << shift) {
                            *slot = ((len as u16) << 8) | u16::from(value);
                        }
                    }
                    code += 1;
                    k += 1;
                }
                maxcode[len as usize] = code as i32 - 1;
            }
            code <<= 1;
        }
        // A sentinel so the decode loop always stops at length 17.
        maxcode[17] = i32::MAX;
        Ok(Decoder { maxcode, valoff, values: values.to_vec(), lookup })
    }
}

/// Reads an entropy-coded segment (§F.2.2.5): stuffed `FF 00` pairs are one `FF` byte; a marker
/// ends the data. Bits past the data are counted, and using any of them is [`Error::Truncated`]:
/// the transform never invents coefficients for a damaged file.
pub(super) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    /// Valid bits in `acc`, counted from its top.
    n: u32,
    /// How many of the last `n` bits are padding past the data.
    fake: u32,
}

impl<'a> BitReader<'a> {
    pub(super) fn new(data: &'a [u8], pos: usize) -> BitReader<'a> {
        BitReader { data, pos, acc: 0, n: 0, fake: 0 }
    }

    /// The position of the first byte not yet read into the bit buffer.
    pub(super) fn pos(&self) -> usize {
        self.pos
    }

    fn fill(&mut self) {
        while self.n <= 56 {
            let byte = match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
                (Some(0xff), Some(0x00)) => {
                    self.pos += 2;
                    0xff
                }
                // A marker or the end: pad with ones and stay put.
                (Some(0xff), _) | (None, _) => {
                    self.fake += 8;
                    0xff
                }
                (Some(&b), _) => {
                    self.pos += 1;
                    b
                }
            };
            self.acc |= u64::from(byte) << (56 - self.n);
            self.n += 8;
        }
    }

    fn consume(&mut self, k: u32) -> Result<()> {
        self.acc <<= k;
        self.n -= k;
        if self.n < self.fake { Err(Error::Truncated) } else { Ok(()) }
    }

    /// The next `k` (≤ 16) bits as an unsigned number (RECEIVE, Figure F.17).
    pub(super) fn receive(&mut self, k: u32) -> Result<u32> {
        if k == 0 {
            return Ok(0);
        }
        if self.n < k {
            self.fill();
        }
        let v = (self.acc >> (64 - k)) as u32;
        self.consume(k)?;
        Ok(v)
    }

    /// One Huffman-coded value (DECODE, Figure F.16).
    pub(super) fn decode(&mut self, table: &Decoder) -> Result<u8> {
        if self.n < 16 {
            self.fill();
        }
        let peek = (self.acc >> (64 - LOOKUP_BITS)) as usize;
        if let Some(&hit) = table.lookup.get(peek)
            && hit != 0
        {
            self.consume(u32::from(hit >> 8))?;
            return Ok(hit as u8);
        }
        let mut len = LOOKUP_BITS + 1;
        let mut code = (self.acc >> (64 - len)) as i32;
        while len <= 16 && code > table.maxcode[len as usize] {
            len += 1;
            code = (self.acc >> (64 - len)) as i32;
        }
        if len > 16 {
            return Err(Error::Malformed("a Huffman code longer than 16 bits".into()));
        }
        self.consume(len)?;
        let index = table.valoff[len as usize] + code;
        usize::try_from(index)
            .ok()
            .and_then(|i| table.values.get(i))
            .copied()
            .ok_or_else(|| Error::Malformed("a Huffman code outside its table".into()))
    }

    /// Ends a restart interval (§F.2.2.5): the padding bits are dropped and the next marker must
    /// be RST`expected`.
    pub(super) fn restart(&mut self, expected: u8) -> Result<()> {
        self.acc = 0;
        self.n = 0;
        self.fake = 0;
        // Fill bytes may precede the marker (§B.1.1.2).
        while self.data.get(self.pos) == Some(&0xff) && self.data.get(self.pos + 1) == Some(&0xff) {
            self.pos += 1;
        }
        match (self.data.get(self.pos), self.data.get(self.pos + 1)) {
            (Some(0xff), Some(&m)) if m == 0xd0 + (expected & 7) => {
                self.pos += 2;
                Ok(())
            }
            (Some(0xff), Some(&m)) => Err(Error::Malformed(format!("marker {m:02X} where RST{} was due", expected & 7))),
            (None, _) | (_, None) => Err(Error::Truncated),
            _ => Err(Error::Malformed("data where a restart marker was due".into())),
        }
    }
}

/// The value of `bits` read as a `size`-bit magnitude category (EXTEND, Figure F.12).
pub(super) fn extend(bits: u32, size: u32) -> i32 {
    if size == 0 {
        0
    } else if bits < 1 << (size - 1) {
        bits as i32 - (1 << size) + 1
    } else {
        bits as i32
    }
}

/// The magnitude category of `v` (§F.1.2.1.1, Table F.1): the number of bits of `|v|`.
pub(super) fn category(v: i32) -> u32 {
    32 - v.unsigned_abs().leading_zeros()
}

/// An encoding table: code and length per symbol (Annex C, EHUFCO and EHUFSI).
#[derive(Clone, Debug)]
pub(super) struct Encoder {
    codes: [(u16, u8); 256],
    /// BITS and HUFFVAL, for the DHT segment.
    pub(super) bits: [u8; 16],
    pub(super) values: Vec<u8>,
}

impl Encoder {
    /// The optimal table for `freq` (Annex K.2), every symbol with a non-zero count getting a code
    /// of at most 16 bits and no code being all ones.
    pub(super) fn optimal(freq: &[u64; 256]) -> Encoder {
        const N: usize = 257;
        let mut f = [0u64; N];
        f[..256].copy_from_slice(freq);
        // The reserved symbol keeps the all-ones code out of the table (K.2).
        f[256] = 1;
        let mut size = [0usize; N];
        let mut others = [usize::MAX; N];
        // Figure K.1: repeatedly merge the two least frequent trees.
        loop {
            let mut v1 = usize::MAX;
            for i in 0..N {
                if f[i] > 0 && (v1 == usize::MAX || f[i] <= f[v1]) {
                    v1 = i;
                }
            }
            let mut v2 = usize::MAX;
            for i in 0..N {
                if f[i] > 0 && i != v1 && (v2 == usize::MAX || f[i] <= f[v2]) {
                    v2 = i;
                }
            }
            if v1 == usize::MAX || v2 == usize::MAX {
                break;
            }
            f[v1] = f[v1].saturating_add(f[v2]);
            f[v2] = 0;
            let mut a = v1;
            size[a] += 1;
            while others[a] != usize::MAX {
                a = others[a];
                size[a] += 1;
            }
            others[a] = v2;
            let mut b = v2;
            size[b] += 1;
            while others[b] != usize::MAX {
                b = others[b];
                size[b] += 1;
            }
        }
        // Figure K.2: count codes per size (sizes are below N).
        let mut count = [0u32; N + 1];
        for &s in &size {
            if s > 0 {
                count[s] += 1;
            }
        }
        // Figure K.3: move codes longer than 16 bits up, then drop the reserved code.
        let mut i = N;
        while i > 16 {
            if count[i] > 0 {
                // Codes of the longest length come in pairs and shorter codes exist (at most
                // 257 symbols), so `j` stops above 0; saturation only guards the impossible.
                let mut j = i - 2;
                while j > 1 && count[j] == 0 {
                    j -= 1;
                }
                count[i] = count[i].saturating_sub(2);
                count[i - 1] += 1;
                count[j + 1] += 2;
                count[j] = count[j].saturating_sub(1);
            } else {
                i -= 1;
            }
        }
        let mut longest = 16;
        while longest > 0 && count[longest] == 0 {
            longest -= 1;
        }
        if longest > 0 {
            count[longest] -= 1;
        }
        let mut bits = [0u8; 16];
        for (b, &c) in bits.iter_mut().zip(&count[1..=16]) {
            *b = c as u8;
        }
        // Figure K.4: symbols by code size, then by value.
        let mut values = Vec::new();
        for s in 1..N {
            for (v, &sz) in size.iter().enumerate().take(256) {
                if sz == s {
                    values.push(v as u8);
                }
            }
        }
        Encoder::from_spec(bits, values)
    }

    /// The codes of a table given as BITS and HUFFVAL (Figures C.1–C.3).
    fn from_spec(bits: [u8; 16], values: Vec<u8>) -> Encoder {
        let mut codes = [(0u16, 0u8); 256];
        let mut code: u32 = 0;
        let mut k = 0usize;
        for (i, &n) in bits.iter().enumerate() {
            for _ in 0..n {
                if let Some(&v) = values.get(k) {
                    codes[usize::from(v)] = (code as u16, i as u8 + 1);
                }
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        Encoder { codes, bits, values }
    }

    /// The code and length of `symbol`; length 0 when the table has no code for it.
    pub(super) fn code(&self, symbol: u8) -> (u16, u8) {
        self.codes[usize::from(symbol)]
    }
}

/// Writes an entropy-coded segment: most significant bit first, an `FF` byte followed by a
/// stuffed `00` (§F.1.2.3), padding with one bits (§F.1.2.3).
pub(super) struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    acc: u64,
    n: u32,
}

impl<'a> BitWriter<'a> {
    pub(super) fn new(out: &'a mut Vec<u8>) -> BitWriter<'a> {
        BitWriter { out, acc: 0, n: 0 }
    }

    /// Appends the low `len` (≤ 16) bits of `bits`.
    pub(super) fn put(&mut self, bits: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | u64::from(bits & ((1u32 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            self.n -= 8;
            let byte = (self.acc >> self.n) as u8;
            self.out.push(byte);
            if byte == 0xff {
                self.out.push(0);
            }
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    /// Pads the last byte with one bits.
    pub(super) fn align(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
    }

    /// Pads, then writes a marker (a restart marker between intervals).
    pub(super) fn marker(&mut self, code: u8) {
        self.align();
        self.out.extend_from_slice(&[0xff, code]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_extension_are_inverse() {
        for v in -2047..=2047 {
            let s = category(v);
            let bits = if v < 0 { (v - 1) as u32 & ((1 << s) - 1) } else { v as u32 };
            assert_eq!(extend(bits, s), v, "{v}");
        }
        assert_eq!((category(0), category(1), category(-1), category(1023), category(-1024)), (0, 1, 1, 10, 11));
    }

    #[test]
    fn optimal_tables_are_complete_prefix_codes_of_at_most_16_bits() {
        // Fibonacci counts force code lengths far past 16 before the limit.
        let mut freq = [0u64; 256];
        let (mut a, mut b) = (1u64, 1u64);
        for f in freq.iter_mut().take(40) {
            *f = a;
            (a, b) = (b, a + b);
        }
        for f in [freq, [1; 256], {
            let mut one = [0; 256];
            one[7] = 5;
            one
        }] {
            let enc = Encoder::optimal(&f);
            let used = f.iter().filter(|&&c| c > 0).count();
            assert_eq!(enc.values.len(), used);
            assert_eq!(enc.bits.iter().map(|&b| usize::from(b)).sum::<usize>(), used);
            // Kraft sum below 1: the all-ones code stays free.
            let kraft: u64 = enc.bits.iter().enumerate().map(|(i, &n)| u64::from(n) << (15 - i)).sum();
            assert!(kraft < 1 << 16, "{kraft}");
            for (s, &c) in f.iter().enumerate() {
                let (code, len) = enc.code(s as u8);
                assert_eq!(len > 0, c > 0);
                assert!(len == 0 || u32::from(code) != (1 << len) - 1);
            }
            // The decoder built from the same spec reads every code back.
            let dec = Decoder::new(&enc.bits, &enc.values).unwrap();
            let mut out = Vec::new();
            let mut w = BitWriter::new(&mut out);
            for &v in &enc.values {
                let (code, len) = enc.code(v);
                w.put(u32::from(code), u32::from(len));
            }
            w.align();
            let mut r = BitReader::new(&out, 0);
            for &v in &enc.values {
                assert_eq!(r.decode(&dec).unwrap(), v);
            }
        }
    }

    #[test]
    fn reading_past_the_data_is_truncation() {
        let data = [0b1010_0000u8, 0xff, 0xd9];
        let mut r = BitReader::new(&data, 0);
        assert_eq!(r.receive(8).unwrap(), 0b1010_0000);
        assert_eq!(r.receive(1), Err(Error::Truncated));
        // A stuffed FF is one data byte.
        let mut r = BitReader::new(&[0xff, 0x00, 0x12], 0);
        assert_eq!((r.receive(8).unwrap(), r.receive(8).unwrap()), (0xff, 0x12));
    }

    #[test]
    fn bad_tables_are_refused() {
        let mut bits = [0u8; 16];
        bits[0] = 3; // three 1-bit codes
        assert!(Decoder::new(&bits, &[1, 2, 3]).is_err());
        bits[0] = 1;
        assert!(Decoder::new(&bits, &[1, 2]).is_err());
    }
}
