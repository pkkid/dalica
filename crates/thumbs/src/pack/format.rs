//! The on-disk shape of a pack: file names, headers, index records and their checksums.
//!
//! - `<size>-<gen as 8 hex>.pack`: a 16-byte header, then entry bytes back to back.
//! - `<size>-<gen as 8 hex>.idx`: a 16-byte header, then 40-byte records, appended one per put or
//!   removal. A record carries its own CRC-32, so a torn or damaged record is skipped alone.
//!
//! Header: 8 magic bytes, the size code (u32 LE), CRC-32 of the first 12 bytes (u32 LE).
//! Record: kind (u8), 3 zero bytes, entry length (u32 LE), key (u128 LE), offset in the pack
//! (u64 LE), CRC-32 of the entry bytes (u32 LE), CRC-32 of the first 36 record bytes (u32 LE).

use super::{Key, Size};

pub(crate) const PACK_MAGIC: [u8; 8] = *b"DLCPACK\x01";
pub(crate) const INDEX_MAGIC: [u8; 8] = *b"DLCINDX\x01";
pub(crate) const HEADER_LEN: u64 = 16;
pub(crate) const RECORD_LEN: u64 = 40;
pub(crate) const RECORD: usize = 40;

/// The two files of one pack generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Pack,
    Index,
}

impl Part {
    fn ext(self) -> &'static str {
        match self {
            Part::Pack => "pack",
            Part::Index => "idx",
        }
    }
}

/// `<size>-<gen>.<ext>`.
pub(crate) fn file_name(size: Size, gen_: u32, part: Part) -> String {
    format!("{}-{gen_:08x}.{}", size.name(), part.ext())
}

/// The size, generation and part a file name of the cache's own shape stands for.
pub(crate) fn parse_name(name: &str) -> Option<(Size, u32, Part)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let part = match ext {
        "pack" => Part::Pack,
        "idx" => Part::Index,
        _ => return None,
    };
    let (size, hex) = stem.rsplit_once('-')?;
    let size = Size::ALL.into_iter().find(|s| s.name() == size)?;
    if hex.len() != 8 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    Some((size, u32::from_str_radix(hex, 16).ok()?, part))
}

/// A streamed temporary file of one of the cache's files: `.<own name>.dalica-tmp-<16 hex>`.
pub(crate) fn parse_temp_name(name: &str) -> Option<(Size, u32, Part)> {
    let rest = name.strip_prefix('.')?;
    let (target, hex) = rest.rsplit_once(".dalica-tmp-")?;
    if hex.len() != 16 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    parse_name(target)
}

/// Whether `name` has the shape of a file this cache made: the only names it lists or deletes.
pub fn is_own_name(name: &str) -> bool {
    parse_name(name).is_some() || parse_temp_name(name).is_some()
}

pub(crate) fn header(magic: [u8; 8], size: Size) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[..8].copy_from_slice(&magic);
    h[8..12].copy_from_slice(&size.code().to_le_bytes());
    let crc = crc32(&h[..12]);
    h[12..].copy_from_slice(&crc.to_le_bytes());
    h
}

pub(crate) fn header_ok(bytes: &[u8], magic: [u8; 8], size: Size) -> bool {
    bytes == header(magic, size)
}

/// One index record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Record {
    Put { key: Key, offset: u64, len: u32, crc: u32 },
    Remove { key: Key },
}

const KIND_PUT: u8 = 1;
const KIND_REMOVE: u8 = 2;

impl Record {
    pub(crate) fn encode(self) -> [u8; RECORD] {
        let (kind, key, offset, len, crc) = match self {
            Record::Put { key, offset, len, crc } => (KIND_PUT, key, offset, len, crc),
            Record::Remove { key } => (KIND_REMOVE, key, 0, 0, 0),
        };
        let mut r = [0u8; RECORD];
        r[0] = kind;
        r[4..8].copy_from_slice(&len.to_le_bytes());
        r[8..24].copy_from_slice(&key.0.to_le_bytes());
        r[24..32].copy_from_slice(&offset.to_le_bytes());
        r[32..36].copy_from_slice(&crc.to_le_bytes());
        let check = crc32(&r[..36]);
        r[36..].copy_from_slice(&check.to_le_bytes());
        r
    }

    /// The record in `bytes`, or `None` when it is torn or damaged.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Record> {
        let r: &[u8; RECORD] = bytes.try_into().ok()?;
        let body = r.get(..36)?;
        if crc32(body) != u32::from_le_bytes(r.get(36..40)?.try_into().ok()?) {
            return None;
        }
        if r.get(1..4)? != [0, 0, 0] {
            return None;
        }
        let len = u32::from_le_bytes(r.get(4..8)?.try_into().ok()?);
        let key = Key(u128::from_le_bytes(r.get(8..24)?.try_into().ok()?));
        let offset = u64::from_le_bytes(r.get(24..32)?.try_into().ok()?);
        let crc = u32::from_le_bytes(r.get(32..36)?.try_into().ok()?);
        match r.first().copied()? {
            KIND_PUT => Some(Record::Put { key, offset, len, crc }),
            KIND_REMOVE if len == 0 && offset == 0 && crc == 0 => Some(Record::Remove { key }),
            _ => None,
        }
    }
}

/// CRC-32 (IEEE 802.3, the zlib/PNG one), eight bytes at a time.
pub fn crc32(bytes: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut c = !0u32;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for &[a, b, d, e, f, g, h, i] in chunks {
        let lo = u32::from_le_bytes([a, b, d, e]) ^ c;
        let hi = u32::from_le_bytes([f, g, h, i]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in rest {
        c = t[0][((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

static CRC_TABLES: [[u32; 256]; 8] = crc_tables();

const fn crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut j = 1;
        while j < 8 {
            let prev = t[j - 1][i];
            t[j][i] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            j += 1;
        }
        i += 1;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        // The eight-at-a-time path agrees with the bytewise one at every length.
        let data: Vec<u8> = (0..100u32).map(|i| (i * 37 % 251) as u8).collect();
        for n in 0..data.len() {
            let mut c = !0u32;
            for &b in &data[..n] {
                c = CRC_TABLES[0][((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
            }
            assert_eq!(crc32(&data[..n]), !c, "{n}");
        }
    }

    #[test]
    fn records_round_trip_and_any_flipped_bit_is_caught() {
        let recs = [
            Record::Put { key: Key(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210), offset: 16, len: 9000, crc: 0xdead_beef },
            Record::Remove { key: Key(7) },
        ];
        for rec in recs {
            let bytes = rec.encode();
            assert_eq!(Record::decode(&bytes), Some(rec));
            for bit in 0..RECORD * 8 {
                let mut b = bytes;
                b[bit / 8] ^= 1 << (bit % 8);
                assert_eq!(Record::decode(&b), None, "bit {bit}");
            }
            assert_eq!(Record::decode(&bytes[..39]), None);
        }
    }

    #[test]
    fn names_have_one_exact_shape() {
        for size in Size::ALL {
            for part in [Part::Pack, Part::Index] {
                let n = file_name(size, 0xab, part);
                assert_eq!(parse_name(&n), Some((size, 0xab, part)), "{n}");
                assert!(is_own_name(&n));
                assert_eq!(parse_temp_name(&format!(".{n}.dalica-tmp-0123456789abcdef")), Some((size, 0xab, part)));
            }
        }
        for foreign in [
            "128.pack",
            "128-1.pack",
            "128-0000000g.pack",
            "128-0000000A.pack",
            "128-00000001.pack.bak",
            "128-00000001.jpg",
            "64-00000001.pack",
            "screen-000000001.idx",
            "notes.txt",
            "lock",
            ".128-00000001.pack.dalica-tmp-0123",
            ".notes.dalica-tmp-0123456789abcdef",
        ] {
            assert!(!is_own_name(foreign), "{foreign}");
        }
    }
}
