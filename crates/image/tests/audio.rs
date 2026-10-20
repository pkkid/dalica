//! Slideshow music (`formats::audio`, M15.27): each format Dalica plays decodes a file made here to
//! its length and rate (WAV through `hound`'s writer, FLAC of verbatim subframes and MP3 of silent
//! layer III frames written by hand, Ogg Vorbis through the pure-Rust `rusty_vorbis` encoder in Ogg
//! pages written here); a file of another kind, a truncated file and hostile headers are refused
//! with their reasons, never a panic.

use dalica_image::formats::audio::{CANNOT_PLAY, Music, MusicKind, is_music_name, sniff};

// --- WAV ---------------------------------------------------------------------------------------------

fn wav(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
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
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
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

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

fn crc16(data: &[u8]) -> u16 {
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
fn flac(rate: u32, channels: u8, blocks: &[usize]) -> Vec<u8> {
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
fn mp3(frames: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..frames {
        let mut f = vec![0xff, 0xfb, 0x90, 0x00];
        f.resize(417, 0);
        out.extend(f);
    }
    out
}

// --- Ogg Vorbis ----------------------------------------------------------------------------------------------

fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04c1_1db7 } else { crc << 1 };
        }
    }
    crc
}

fn ogg_page(seq: u32, granule: u64, kind: u8, packet: &[u8]) -> Vec<u8> {
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
fn vorbis(frames: usize, length: u64) -> Vec<u8> {
    let mut enc = rusty_vorbis::VorbisEncoder::new(rusty_vorbis::VorbisEncoderConfig::default());
    let pcm: Vec<f32> = (0..frames * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3).collect();
    enc.push_pcm_f32(&pcm, 2, 44_100).unwrap();
    enc.finish();
    let mut packets = Vec::new();
    while let Ok(p) = enc.next_packet() {
        packets.push(p.data);
    }
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

// --- The tests -------------------------------------------------------------------------------------------------

/// Every frame of `music`, interleaved, and the reason it ended early if it did.
fn all(music: &mut Music) -> (Vec<f32>, Option<String>) {
    let mut out = Vec::new();
    loop {
        match music.next_chunk() {
            Ok(Some(c)) => {
                assert!(!c.is_empty() && c.len() % usize::from(music.channels()) == 0, "a block of whole frames");
                out.extend(c);
            }
            Ok(None) => return (out, None),
            Err(e) => return (out, Some(e.reason)),
        }
    }
}

#[test]
fn each_format_decodes_to_its_length_and_rate() {
    let cases: [(MusicKind, Vec<u8>, u32, u16, usize); 4] = [
        (MusicKind::Wav, wav(22_050, 2, 11_025), 22_050, 2, 11_025),
        (MusicKind::Flac, flac(48_000, 2, &[4096, 1000]), 48_000, 2, 5_096),
        (MusicKind::Mp3, mp3(20), 44_100, 2, 20 * 1152),
        (MusicKind::Vorbis, vorbis(30_000, 28_000), 44_100, 2, 28_000),
    ];
    for (kind, bytes, rate, channels, frames) in cases {
        assert_eq!(sniff(&bytes), Some(kind));
        let mut m = Music::from_bytes(bytes).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        assert_eq!((m.kind(), m.rate(), m.channels()), (kind, rate, channels), "{kind:?}");
        let (samples, failed) = all(&mut m);
        assert_eq!(failed, None, "{kind:?}");
        assert_eq!(samples.len(), frames * usize::from(channels), "{kind:?}: its length");
        assert!(samples.iter().all(|s| (-1.0..=1.0).contains(s)), "{kind:?}: in range");
        if kind != MusicKind::Mp3 {
            assert!(samples.iter().any(|s| *s != 0.0), "{kind:?}: the sound, not silence");
        }
    }
    // A file on disk reads the same, by its bytes whatever its name.
    let dir = std::env::temp_dir().join(format!("dalica-image-audio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("track.mp3");
    std::fs::write(&path, wav(8_000, 1, 800)).unwrap();
    let mut m = Music::open(&path).unwrap();
    assert_eq!((m.kind(), m.frames()), (MusicKind::Wav, Some(800)));
    assert_eq!(m.read_frames(500).unwrap().len(), 500);
    assert_eq!(m.read_frames(500).unwrap().len(), 300, "the rest");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(is_music_name("a.MP3") && is_music_name("b.ogg") && is_music_name("c.flac") && is_music_name("d.wav"));
    assert!(!is_music_name("e.m4a") && !is_music_name("f.jpg") && !is_music_name("mp3"));
}

#[test]
fn other_kinds_truncated_files_and_hostile_headers_are_refused_with_their_reasons() {
    let refused = |bytes: Vec<u8>| Music::from_bytes(bytes).map(|_| ()).unwrap_err().reason;
    // Another kind of file: AAC in MP4, an Opus stream in Ogg, nothing at all.
    let m4a = [&[0, 0, 0, 0x18][..], b"ftypM4A ", &[0; 12]].concat();
    assert_eq!(refused(m4a), format!("{CANNOT_PLAY} (not MP3, Ogg Vorbis, FLAC or WAV)"));
    assert_eq!(refused(Vec::new()), format!("{CANNOT_PLAY} (not MP3, Ogg Vorbis, FLAC or WAV)"));
    let opus = ogg_page(0, 0, 0x02, &[&b"OpusHead"[..], &[1, 2, 0x38, 1, 0x80, 0xbb, 0, 0, 0, 0, 0]].concat());
    assert_eq!(refused(opus), format!("{CANNOT_PLAY} (not an Ogg Vorbis stream, or a damaged one)"));
    // Truncated: a WAV cut inside its header, a FLAC cut inside its second block (the first plays).
    assert!(refused(wav(8_000, 1, 100).get(..30).unwrap().to_vec()).starts_with(CANNOT_PLAY));
    let cut = flac(48_000, 1, &[4096, 4096]);
    let mut m = Music::from_bytes(cut.get(..cut.len() - 3000).unwrap().to_vec()).unwrap();
    let (samples, failed) = all(&mut m);
    assert_eq!((samples.len(), failed), (4096, Some(format!("{CANNOT_PLAY} (the FLAC data is damaged)"))));
    assert!(m.next_chunk().unwrap().is_none(), "the file ends after its failure");
    // Hostile headers: a WAV of 200 channels, one at 1 Hz, a FLAC at 655 kHz.
    let mut many = wav(8_000, 1, 10);
    many[22..24].copy_from_slice(&200u16.to_le_bytes());
    assert!(refused(many).starts_with(CANNOT_PLAY));
    let mut slow = wav(8_000, 1, 10);
    slow[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert!(refused(slow).starts_with(CANNOT_PLAY));
    let fast = flac(655_350, 1, &[4096]);
    assert_eq!(refused(fast), format!("{CANNOT_PLAY} (1 channels at 655350 Hz)"));
    // Garbage after each kind's magic: refused or ended, never a panic.
    for magic in [&b"ID3\x04\0\0\0\0\0\x10"[..], b"OggS\0\x02", b"fLaC\0\0\0\x22", b"RIFF\x10\0\0\0WAVEfmt ", &[0xff, 0xfb, 0x90, 0]] {
        for len in [0usize, 7, 100, 3000] {
            let mut bytes = magic.to_vec();
            bytes.extend((0..len).map(|i| (i * 37 % 251) as u8));
            if let Ok(mut m) = Music::from_bytes(bytes) {
                let _ = all(&mut m);
            }
        }
    }
}
