//! Slideshow music (`formats::audio`, M15.27): each format Dalica plays decodes a file made here to
//! its length and rate (WAV through `hound`'s writer, FLAC of verbatim subframes and MP3 of silent
//! layer III frames written by hand, Ogg Vorbis through the pure-Rust `rusty_vorbis` encoder in Ogg
//! pages written here); a file of another kind, a truncated file and hostile headers are refused
//! with their reasons, never a panic.

use dalica_image::formats::audio::{CANNOT_PLAY, Music, MusicKind, is_music_name, sniff};

use crate::music_writer::*;

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

/// Found by the `music` fuzz target (M15.R): `lewton` reserves memory for the sizes the Vorbis
/// headers declare before it reads them, and a failed reservation aborts the process. A comment
/// header declaring 2³² − 1 comments, a codebook of 2²⁴ − 1 entries of 65,535 dimensions, and one
/// whose lookup values need more bits than its packet holds are refused with a reason first; the
/// stream's own headers still play.
#[test]
fn an_ogg_vorbis_stream_declaring_more_than_its_headers_hold_is_refused_before_it_is_read() {
    let refused = |packets: &[Vec<u8>]| Music::from_bytes(vorbis_pages(packets, 1024)).map(|_| ()).unwrap_err().reason;
    let packets = vorbis_packets(2048);
    assert!(Music::from_bytes(vorbis_pages(&packets, 1024)).is_ok());

    let mut comments = packets.clone();
    comments[1] = [&b"\x03vorbis"[..], &0u32.to_le_bytes(), &u32::MAX.to_le_bytes(), &[1]].concat();
    assert_eq!(refused(&comments), format!("{CANNOT_PLAY} (a damaged Ogg Vorbis comment header)"));

    /// Bits least significant first, as Vorbis packs them.
    fn pack(fields: &[(u32, u64)]) -> Vec<u8> {
        let mut out = b"\x05vorbis".to_vec();
        let mut at = 0usize;
        for &(n, v) in fields {
            for i in 0..n {
                if at.is_multiple_of(8) {
                    out.push(0);
                }
                *out.last_mut().unwrap() |= (((v >> i) & 1) as u8) << (at % 8);
                at += 1;
            }
        }
        out
    }
    // One codebook (count − 1 = 0), ordered: all its entries of one length, then a lookup of type 1.
    let huge = |dims: u64, entries: u64, value_bits: u64, values: u64| {
        // The ordered lengths: one run of every entry, in ilog(entries) bits.
        let run = 64 - entries.leading_zeros();
        let mut f = vec![(8, 0), (24, 0x56_4342), (16, dims), (24, entries), (1, 1), (5, 0), (run, entries), (4, 1), (32, 0), (32, 0)];
        f.extend([(4, value_bits - 1), (1, 0)]);
        f.extend(std::iter::repeat_n((value_bits as u32, 1), values as usize));
        pack(&f)
    };
    let mut setup = packets.clone();
    setup[2] = huge(65_535, (1 << 24) - 1, 1, 1);
    assert_eq!(refused(&setup), format!("{CANNOT_PLAY} (an Ogg Vorbis stream with codebooks too large)"));
    // No dimensions: `lewton` takes 2³² − 1 lookup values, more than the packet's bits.
    setup[2] = huge(0, 1, 8, 1);
    assert_eq!(refused(&setup), format!("{CANNOT_PLAY} (a damaged Ogg Vorbis setup header)"));
}

/// Found by the `music` fuzz target (M15.R): nine bytes changed in the audio pages of an Ogg
/// Vorbis file made `lewton` give samples that are not numbers. Samples come out finite and within
/// −1 ..= 1 whatever the decoder gives.
#[test]
fn samples_a_decoder_gives_out_of_range_come_out_finite_and_in_range() {
    let mut bytes = vorbis(2048, 2048);
    for (at, b) in [(3110, 0xff), (3111, 0xff), (3112, 0xff), (3113, 0x53), (4453, 0x8f), (4454, 0xff), (4455, 0x9f), (4456, 0x67), (4462, 0x37)] {
        bytes[at] = b;
    }
    // Every page's checksum made right again, so that only the audio is damaged.
    let mut at = 0;
    while at + 27 <= bytes.len() {
        assert_eq!(&bytes[at..at + 4], b"OggS");
        let segs = usize::from(bytes[at + 26]);
        let body: usize = bytes[at + 27..at + 27 + segs].iter().map(|&s| usize::from(s)).sum();
        let end = at + 27 + segs + body;
        bytes[at + 22..at + 26].fill(0);
        let crc = ogg_crc(&bytes[at..end]);
        bytes[at + 22..at + 26].copy_from_slice(&crc.to_le_bytes());
        at = end;
    }
    let mut m = Music::from_bytes(bytes).unwrap();
    let (samples, _) = all(&mut m);
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|s| s.is_finite() && (-1.0..=1.0).contains(s)), "a sample out of range");
}
