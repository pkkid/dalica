//! Music files made for the tests and the fuzz seeds (`crates/image/fuzz`, which compiles this
//! file in with `#[path]`): WAV through `hound`'s writer, FLAC of verbatim subframes and MP3 of
//! silent layer III frames written by hand, Ogg Vorbis through the pure-Rust `rusty_vorbis`
//! encoder in Ogg pages written here.

#![allow(dead_code)]

// --- WAV ---------------------------------------------------------------------------------------------

pub fn wav(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
    let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..frames * usize::from(channels) {
        w.write_sample(((i % 200) as i16 - 100) * 100).unwrap();
    }
    w.finalize().unwrap();
    out.into_inner()
}

// --- FLAC: verbatim subframes ----------------------------------------------------------------------------

#[derive(Default)]
pub struct Bits {
    pub out: Vec<u8>,
    pub acc: u64,
    pub n: u32,
}

impl Bits {
    pub fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                (self.acc, self.n) = (0, 0);
            }
        }
        self
    }
}

pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
        }
    }
    crc
}

/// A 16-bit FLAC of `channels` channels at `rate`: blocks of the sizes given, every sample verbatim.
pub fn flac(rate: u32, channels: u8, blocks: &[usize]) -> Vec<u8> {
    let total: usize = blocks.iter().sum();
    let mut out = b"fLaC".to_vec();
    let mut info = Bits::default();
    info.u(16, 4096).u(16, 4096).u(24, 0).u(24, 0);
    info.u(20, u64::from(rate)).u(3, u64::from(channels - 1)).u(5, 15).u(36, total as u64);
    info.u(64, 0).u(64, 0);
    out.extend([0x80, 0, 0, 34]);
    out.extend(info.out);
    for (k, &size) in blocks.iter().enumerate() {
        let mut h = Bits::default();
        h.u(14, 0b11_1111_1111_1110).u(1, 0).u(1, 0);
        // 4096 by its code, any other size as a 16-bit number after the frame number.
        h.u(4, if size == 4096 { 0b1100 } else { 0b0111 }).u(4, 0);
        h.u(4, u64::from(channels - 1)).u(3, 0b100).u(1, 0);
        h.u(8, k as u64);
        if size != 4096 {
            h.u(16, size as u64 - 1);
        }
        let mut frame = h.out;
        frame.push(crc8(&frame));
        let mut b = Bits::default();
        for c in 0..channels {
            b.u(8, 0b0000_0010);
            for i in 0..size {
                b.u(16, (((i + usize::from(c) * 7) % 300) as i16 - 150) as u16 as u64);
            }
        }
        frame.extend(b.out);
        let crc = crc16(&frame);
        frame.extend(crc.to_be_bytes());
        out.extend(frame);
    }
    out
}

// --- MP3: silent MPEG-1 layer III frames ------------------------------------------------------------------

/// `frames` frames of 128 kbit/s stereo at 44.1 kHz, every granule empty (silence).
pub fn mp3(frames: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..frames {
        let mut f = vec![0xff, 0xfb, 0x90, 0x00];
        f.resize(417, 0);
        out.extend(f);
    }
    out
}

// --- Ogg Vorbis ----------------------------------------------------------------------------------------------

pub fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04c1_1db7 } else { crc << 1 };
        }
    }
    crc
}

pub fn ogg_page(seq: u32, granule: u64, kind: u8, packet: &[u8]) -> Vec<u8> {
    let mut segs = vec![255u8; packet.len() / 255];
    segs.push((packet.len() % 255) as u8);
    let mut page = b"OggS".to_vec();
    page.extend([0, kind]);
    page.extend(granule.to_le_bytes());
    page.extend(0x00da_11ca_u32.to_le_bytes());
    page.extend(seq.to_le_bytes());
    page.extend([0; 4]);
    page.push(segs.len() as u8);
    page.extend(segs);
    page.extend(packet);
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

/// `frames` frames of a stereo tone at 44.1 kHz as Ogg Vorbis, one packet a page, the last page's
/// granule `length` (the stream ends there).
pub fn vorbis(frames: usize, length: u64) -> Vec<u8> {
    vorbis_pages(&vorbis_packets(frames), length)
}

/// The Vorbis packets of `frames` frames of a stereo tone at 44.1 kHz: the three headers
/// (identification, comment, setup), then the audio.
pub fn vorbis_packets(frames: usize) -> Vec<Vec<u8>> {
    let mut enc = rusty_vorbis::VorbisEncoder::new(rusty_vorbis::VorbisEncoderConfig::default());
    let pcm: Vec<f32> = (0..frames * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3).collect();
    enc.push_pcm_f32(&pcm, 2, 44_100).unwrap();
    enc.finish();
    let mut packets = Vec::new();
    while let Ok(p) = enc.next_packet() {
        packets.push(p.data);
    }
    packets
}

/// `packets` in Ogg pages, one a page, the last page's granule `length`.
pub fn vorbis_pages(packets: &[Vec<u8>], length: u64) -> Vec<u8> {
    let (headers, audio) = packets.split_at(3);
    let mut out = ogg_page(0, 0, 0x02, &headers[0]);
    out.extend(ogg_page(1, 0, 0, &headers[1]));
    out.extend(ogg_page(2, 0, 0, &headers[2]));
    for (i, p) in audio.iter().enumerate() {
        let last = i + 1 == audio.len();
        let granule = if last { length } else { i as u64 * 1024 };
        out.extend(ogg_page(3 + i as u32, granule, if last { 0x04 } else { 0 }, p));
    }
    out
}
