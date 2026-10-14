//! Reading back the settings a JPEG was saved with (`data-model.md` §6): the quality from its
//! quantisation tables and the chroma subsampling from its frame header. Saved by `scenes` (the
//! `image` crate's encoder) and by our own encoder, estimated within 2; hand-built headers for the
//! layouts neither writes; hostile, truncated and random headers give `None`, never a panic.
//!
//! Adapted from PhotoCraft (MIT OR Apache-2.0), `crates/codecs/tests/source_settings.rs`
//! (`jpeg_quality_is_estimated_within_two`, `jpeg_quality_of_hand_built_tables`,
//! `hostile_jpeg_tables_give_no_estimate_and_never_panic`) at bb17dd55.

use dalica_image::encode::{self, Channels, Image, Metadata, Pixels, Subsampling};
use dalica_image::jpeg_quality::{self, Sampling, Settings};
use dalica_scenes::{Kind, Scene};
use proptest::prelude::*;

/// ITU-T T.81 Annex K.1 in zig-zag order, as a DQT stores it (quality 50).
const ANNEX_K_LUMA_ZIGZAG: [u8; 64] = [
    16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24, 49, 35, 37, 29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72,
    92, 78, 64, 68, 87, 69, 55, 56, 80, 109, 81, 87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92, 101, 103, 99,
];

fn photo(w: u32, h: u32) -> Vec<u8> {
    Scene::new(5, w, h).with_kind(Kind::Blobs).render().unwrap().data
}

fn ours(rgb: &[u8], w: u32, h: u32, grey: bool, q: u8, sub: Subsampling) -> Vec<u8> {
    let g: Vec<u8> = rgb.as_chunks::<3>().0.iter().map(|&[r, _, _]| r).collect();
    let img = if grey {
        Image { width: w, height: h, channels: Channels::Grey, pixels: Pixels::U8(&g) }
    } else {
        Image { width: w, height: h, channels: Channels::Rgb, pixels: Pixels::U8(rgb) }
    };
    encode::jpeg(&img, q, sub, &Metadata::default()).unwrap()
}

/// SOI, a DQT with `tables` (each: Pq/Tq byte and its entries), a frame header with `components`
/// (H, V, Tq), then an SOS.
fn hand_built(tables: &[(u8, Vec<u8>)], components: &[(u8, u8, u8)]) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8];
    for (pq_tq, entries) in tables {
        b.extend([0xFF, 0xDB]);
        b.extend(((entries.len() + 3) as u16).to_be_bytes());
        b.push(*pq_tq);
        b.extend(entries);
    }
    b.extend([0xFF, 0xC0]);
    b.extend(((8 + 3 * components.len()) as u16).to_be_bytes());
    b.extend([8, 0, 16, 0, 16, components.len() as u8]);
    for (i, &(h, v, tq)) in components.iter().enumerate() {
        b.extend([i as u8 + 1, (h << 4) | v, tq]);
    }
    b.extend([0xFF, 0xDA, 0, 2]);
    b
}

#[test]
fn scenes_jpegs_are_estimated_within_two_and_their_subsampling_read() {
    for q in [50u8, 75, 90, 95] {
        let bytes = dalica_scenes::jpeg(&Scene::new(7, 160, 120), q, None).unwrap();
        let Settings { quality, sampling } = jpeg_quality::read(&bytes).unwrap_or_else(|| panic!("q{q}: no estimate"));
        assert!(quality.abs_diff(q) <= 2, "scenes saved at {q}, estimated {quality}");
        // The `image` crate's encoder writes 4:4:4.
        assert_eq!(sampling, Sampling::S444, "q{q}");
        assert_eq!(sampling.for_jpeg(), Subsampling::S444);
        assert_eq!(jpeg_quality::quality(&bytes), Some(quality));
        assert_eq!(jpeg_quality::subsampling(&bytes), Some(Sampling::S444));
    }
}

#[test]
fn our_jpegs_are_estimated_within_two() {
    let (w, h) = (40, 24);
    let rgb = photo(w, h);
    for (grey, sub, want) in
        [(false, Subsampling::S444, Sampling::S444), (false, Subsampling::S420, Sampling::S420), (true, Subsampling::S444, Sampling::Grey)]
    {
        for q in [1u8, 5, 10, 25, 40, 50, 60, 70, 75, 80, 85, 90, 95, 98, 100] {
            let bytes = ours(&rgb, w, h, grey, q, sub);
            let s = jpeg_quality::read(&bytes).unwrap_or_else(|| panic!("{want:?} q{q}: no estimate"));
            assert!(s.quality.abs_diff(q) <= 2, "{want:?}: saved at {q}, estimated {}", s.quality);
            assert_eq!(s.sampling, want);
        }
    }
    // Our scaling is the one the estimate assumes, so it is exact.
    let small = photo(16, 8);
    for q in 1..=100u8 {
        assert_eq!(jpeg_quality::quality(&ours(&small, 16, 8, false, q, Subsampling::S444)), Some(q));
    }
}

#[test]
fn hand_built_tables_and_layouts() {
    let luma = ANNEX_K_LUMA_ZIGZAG.to_vec();
    assert_eq!(jpeg_quality::read(&hand_built(&[(0x00, luma.clone())], &[(1, 1, 0)])), Some(Settings { quality: 50, sampling: Sampling::Grey }));
    // The components' table selectors decide which tables count: slot 2 holds the luma table.
    let flat = vec![1u8; 64];
    let b = hand_built(&[(0x00, flat.clone()), (0x02, luma.clone())], &[(1, 1, 2)]);
    assert_eq!(jpeg_quality::quality(&b), Some(50));
    // A 16-bit table (Pq = 1) holding the same values.
    let wide: Vec<u8> = luma.iter().flat_map(|&v| [0, v]).collect();
    assert_eq!(jpeg_quality::quality(&hand_built(&[(0x10, wide)], &[(1, 1, 0)])), Some(50));
    // Layouts our encoder never writes: 4:2:2, 4:4:0 and 4:1:1 are saved as 4:4:4.
    for (h, v, want) in [(2, 1, Sampling::S422), (1, 2, Sampling::S440), (4, 1, Sampling::S411), (2, 2, Sampling::S420), (1, 1, Sampling::S444)] {
        let b = hand_built(&[(0x00, luma.clone()), (0x01, luma.clone())], &[(h, v, 0), (1, 1, 1), (1, 1, 1)]);
        let s = jpeg_quality::read(&b).unwrap();
        assert_eq!(s.sampling, want);
        assert_eq!(s.sampling.for_jpeg(), if want == Sampling::S420 { Subsampling::S420 } else { Subsampling::S444 });
    }
    // CMYK: four components at full resolution.
    let b = hand_built(&[(0x00, luma.clone())], &[(1, 1, 0), (1, 1, 0), (1, 1, 0), (1, 1, 0)]);
    assert_eq!(jpeg_quality::subsampling(&b), Some(Sampling::S444));
}

#[test]
fn hostile_tables_give_no_estimate_and_never_panic() {
    let good = ours(&photo(16, 16), 16, 16, false, 70, Subsampling::S420);
    let sos = good.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
    // Every truncation: `None` while the header is cut before the first scan, the same estimate
    // from there on.
    let full = jpeg_quality::read(&good).unwrap();
    for n in 0..good.len() {
        let got = jpeg_quality::read(&good[..n]);
        if n < sos + 2 {
            assert_eq!(got, None, "cut at {n}");
        } else {
            assert_eq!(got, Some(full), "cut at {n}");
        }
    }
    let luma = ANNEX_K_LUMA_ZIGZAG.to_vec();
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0xFF],
        vec![0xFF, 0xD8],
        // A DQT whose length runs past the end.
        vec![0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xFF, 0x00, 1, 2, 3],
        // A DQT of length 0 and 1 (less than its own length field).
        vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x00, 0xFF, 0xDA],
        vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x01, 0xFF, 0xDA],
        // A table cut short, a bad precision, a slot past 3, a 16-bit table cut mid-entry.
        hand_built(&[(0x00, vec![9; 9])], &[(1, 1, 0)]),
        hand_built(&[(0x70, vec![9; 64])], &[(1, 1, 0)]),
        hand_built(&[(0x0F, vec![9; 64])], &[(1, 1, 0)]),
        hand_built(&[(0x10, vec![0xFF; 65])], &[(1, 1, 0)]),
        // An all-zero table (illegal), and a table with one zero.
        hand_built(&[(0x00, vec![0; 64])], &[(1, 1, 0)]),
        hand_built(&[(0x00, [&luma[..63], &[0]].concat())], &[(1, 1, 0)]),
        // No DQT; a frame selecting an undefined slot; no frame header; a frame with no
        // components, five components, a zero sampling factor, a table slot past 3.
        hand_built(&[], &[(1, 1, 0)]),
        hand_built(&[(0x00, luma.clone())], &[(1, 1, 1)]),
        [vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 67, 0x00], luma.clone(), vec![0xFF, 0xDA, 0, 2]].concat(),
        hand_built(&[(0x00, luma.clone())], &[]),
        hand_built(&[(0x00, luma.clone())], &[(1, 1, 0); 5]),
        hand_built(&[(0x00, luma.clone())], &[(0, 1, 0)]),
        hand_built(&[(0x00, luma.clone())], &[(1, 1, 4)]),
        // An EOI before any scan, and a header made of fill bytes.
        [vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 67, 0x00], luma.clone(), vec![0xFF, 0xD9]].concat(),
        vec![0xFF; 64],
        [vec![0xFF, 0xD8], vec![0xFF; 10_000]].concat(),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_eq!(jpeg_quality::read(c), None, "case {i}");
    }
    assert_eq!(jpeg_quality::quality(b"not a jpeg at all"), None);
    let png = encode::png(&Image { width: 2, height: 1, channels: Channels::Grey, pixels: Pixels::U8(&[1, 2]) }, &Metadata::default()).unwrap();
    assert_eq!(jpeg_quality::quality(&png), None);
    // A table of all 255s (8-bit) and all 0xFFFF (16-bit): an estimate at the bottom of the scale.
    assert_eq!(jpeg_quality::quality(&hand_built(&[(0x00, vec![0xFF; 64])], &[(1, 1, 0)])), Some(1));
    assert_eq!(jpeg_quality::quality(&hand_built(&[(0x10, vec![0xFF; 128])], &[(1, 1, 0)])), Some(1));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Random bytes never panic, and give `None` unless they start like a JPEG.
    #[test]
    fn random_headers_never_panic(head in prop_oneof![Just(vec![]), Just(vec![0xFF, 0xD8]), Just(vec![0xFF, 0xD8, 0xFF, 0xDB]), Just(vec![0xFF, 0xD8, 0xFF, 0xC0])], tail in proptest::collection::vec(any::<u8>(), 0..400)) {
        let bytes = [head.clone(), tail].concat();
        let got = jpeg_quality::read(&bytes);
        if head.is_empty() && bytes.get(..2) != Some(&[0xFF, 0xD8]) {
            prop_assert_eq!(got, None);
        }
        if let Some(s) = got {
            prop_assert!((1..=100).contains(&s.quality));
        }
    }

    /// Random corruption of a real header never panics; the estimate stays on the scale.
    #[test]
    fn corrupted_headers_never_panic(flips in proptest::collection::vec((0usize..700, any::<u8>()), 1..8)) {
        let mut bytes = ours(&photo(24, 16), 24, 16, false, 80, Subsampling::S420);
        for (at, v) in flips {
            let i = at % bytes.len();
            bytes[i] ^= v;
        }
        if let Some(s) = jpeg_quality::read(&bytes) {
            prop_assert!((1..=100).contains(&s.quality));
        }
    }
}
