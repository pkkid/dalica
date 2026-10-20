//! The sizes an Ogg Vorbis stream's headers declare, checked before `lewton` reads them
//! (`security.md` §4.2; found by the `music` fuzz target at M15.R).
//!
//! `lewton` (=0.10.2) reserves memory for what a header declares before it reads it: the comment
//! header's comment count (48 bytes each, so a count of 2³² asks for 200 GB), and each codebook of
//! the setup header its entries times its dimensions (up to 2⁴⁰ values); a failed reservation
//! aborts the process, which no panic guard catches. Here the first three packets of the stream are
//! read as `lewton` reads them (the first packet, then the next two of its stream, through the same
//! `ogg` reader), and a stream is refused when:
//!
//! - its comment header declares a vendor string, more comments, or a comment longer than the
//!   packet holds (`lewton` would refuse it too, after reserving);
//! - a codebook's lookup values need more bits than the setup packet holds (likewise);
//! - its codebooks declare more than [`MAX_CODEBOOK_VALUES`] entries and values together (every
//!   encoder's books are a few thousand; the limit is 64 MB of samples).
//!
//! The bits are read as the Vorbis I specification packs them (§2.1.4: least significant first).

use std::io::{Read, Seek, SeekFrom};

/// Most codebook entries and lookup values one stream may declare, all codebooks together.
pub const MAX_CODEBOOK_VALUES: u64 = 1 << 24;

/// Reads the three header packets from `source`, checks them, and hands `source` back at its
/// start; `Err` says why the stream is refused.
pub fn check<R: Read + Seek>(source: R) -> Result<R, &'static str> {
    let mut packets = ogg::PacketReader::new(source);
    let damaged = "not an Ogg Vorbis stream, or a damaged one";
    let first = packets.read_packet_expected().map_err(|_| damaged)?;
    let serial = first.stream_serial();
    let mut next = || loop {
        match packets.read_packet_expected() {
            Ok(p) if p.stream_serial() == serial => return Ok(p),
            Ok(_) => continue,
            Err(_) => return Err(damaged),
        }
    };
    let comment = next()?;
    comment_header(&comment.data)?;
    let setup = next()?;
    setup_header(&setup.data)?;
    let mut source = packets.into_inner();
    source.seek(SeekFrom::Start(0)).map_err(|_| damaged)?;
    Ok(source)
}

fn u32_le(b: &[u8], at: usize) -> Option<usize> {
    let v = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as usize)
}

/// The comment header (§5.2): every length within the packet.
pub fn comment_header(p: &[u8]) -> Result<(), &'static str> {
    // Another packet is refused by `lewton` before it reserves anything.
    if p.get(..7) != Some(b"\x03vorbis") {
        return Ok(());
    }
    let cut = "a damaged Ogg Vorbis comment header";
    let mut at = 7usize;
    let vendor = u32_le(p, at).ok_or(cut)?;
    at += 4;
    at = at.checked_add(vendor).filter(|&e| e <= p.len()).ok_or(cut)?;
    let count = u32_le(p, at).ok_or(cut)?;
    at += 4;
    if count > (p.len() - at) / 4 {
        return Err(cut);
    }
    for _ in 0..count {
        let n = u32_le(p, at).ok_or(cut)?;
        at += 4;
        at = at.checked_add(n).filter(|&e| e <= p.len()).ok_or(cut)?;
    }
    Ok(())
}

/// Bits of a packet, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    pos: u64,
}

impl Bits<'_> {
    fn left(&self) -> u64 {
        (self.data.len() as u64 * 8).saturating_sub(self.pos)
    }

    fn read(&mut self, n: u32) -> Option<u32> {
        if u64::from(n) > self.left() {
            return None;
        }
        let mut v = 0u32;
        for i in 0..n {
            let byte = *self.data.get((self.pos / 8) as usize)?;
            v |= u32::from((byte >> (self.pos % 8)) & 1) << i;
            self.pos += 1;
        }
        Some(v)
    }

    fn skip(&mut self, n: u64) -> Option<()> {
        if n > self.left() {
            return None;
        }
        self.pos += n;
        Some(())
    }
}

/// The bits needed to hold `x` (§9.2.1).
fn ilog(x: u64) -> u32 {
    64 - x.leading_zeros()
}

/// `lookup1_values` (§9.2.3): the largest `r` with `r^dims <= entries`; as `lewton` answers it
/// for no dimensions (`u32::MAX` for any entries).
fn lookup1_values(entries: u32, dims: u16) -> u64 {
    if dims == 0 {
        return if entries == 0 { 0 } else { u64::from(u32::MAX) };
    }
    let fits = |r: u64| r.checked_pow(u32::from(dims)).is_some_and(|p| p <= u64::from(entries));
    let mut r = (f64::from(entries).powf(1.0 / f64::from(dims)).floor() as u64).min(u64::from(entries));
    while r > 0 && !fits(r) {
        r -= 1;
    }
    while fits(r + 1) {
        r += 1;
    }
    r
}

/// The setup header's codebooks (§3.2.1, §4.2.4): their lookup values within the packet, and
/// their entries and values together within [`MAX_CODEBOOK_VALUES`]. What follows the codebooks
/// declares only small counts.
pub fn setup_header(p: &[u8]) -> Result<(), &'static str> {
    if p.get(..7) != Some(b"\x05vorbis") {
        return Ok(());
    }
    let cut = "a damaged Ogg Vorbis setup header";
    let mut bits = Bits { data: p.get(7..).unwrap_or_default(), pos: 0 };
    let books = bits.read(8).ok_or(cut)? + 1;
    let mut values = 0u64;
    for _ in 0..books {
        if bits.read(24).ok_or(cut)? != 0x56_4342 {
            return Err(cut);
        }
        let dims = bits.read(16).ok_or(cut)? as u16;
        let entries = bits.read(24).ok_or(cut)?;
        values += u64::from(entries);
        if values > MAX_CODEBOOK_VALUES {
            return Err("an Ogg Vorbis stream with codebooks too large");
        }
        if bits.read(1).ok_or(cut)? == 0 {
            let sparse = bits.read(1).ok_or(cut)? == 1;
            for _ in 0..entries {
                if !sparse || bits.read(1).ok_or(cut)? == 1 {
                    bits.read(5).ok_or(cut)?;
                }
            }
        } else {
            bits.read(5).ok_or(cut)?;
            let mut entry = 0u32;
            while entry < entries {
                let number = bits.read(ilog(u64::from(entries - entry))).ok_or(cut)?;
                entry = entry.checked_add(number).filter(|&e| e <= entries).ok_or(cut)?;
            }
        }
        let lookup = bits.read(4).ok_or(cut)?;
        match lookup {
            0 => {}
            1 | 2 => {
                bits.skip(64).ok_or(cut)?;
                let value_bits = u64::from(bits.read(4).ok_or(cut)? + 1);
                bits.read(1).ok_or(cut)?;
                let count = if lookup == 1 { lookup1_values(entries, dims) } else { u64::from(entries) * u64::from(dims) };
                bits.skip(count.saturating_mul(value_bits)).ok_or(cut)?;
                values += u64::from(entries) * u64::from(dims);
                if values > MAX_CODEBOOK_VALUES {
                    return Err("an Ogg Vorbis stream with codebooks too large");
                }
            }
            _ => return Err(cut),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup1_values_is_the_largest_root() {
        for (entries, dims, r) in [(1025, 10, 2), (1024, 10, 2), (1023, 10, 1), (3126, 5, 5), (3125, 5, 5), (3124, 5, 4), (1, 1, 1), (0, 15, 0)] {
            assert_eq!(lookup1_values(entries, dims), r, "{entries} {dims}");
        }
        assert_eq!(lookup1_values(0, 0), 0);
        assert_eq!(lookup1_values(400, 0), u64::from(u32::MAX));
        assert_eq!(lookup1_values(u32::MAX >> 8, 1), u64::from(u32::MAX >> 8));
    }
}
