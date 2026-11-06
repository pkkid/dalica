//! `core::color_summary` (`organizing.md` §8.1): the seven search colours, the summary's byte
//! form, and the classifier on plain grids (one hue, greys, two halves, a faint tint, a dot).

use dalica_core::color::{hsv_to_rgb, rgb_to_u8};
use dalica_core::color_summary::{ColourSummary, HUE_BOUNDS, MAX_CELLS, SearchColour, classify, hue_colour};
use proptest::prelude::*;

/// A grid of `n` cells of one colour.
fn field(c: [u8; 3], n: usize) -> Vec<[u8; 3]> {
    vec![c; n]
}

/// The 8-bit colour of hue `h` at saturation `s` and value `v`.
fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    rgb_to_u8(hsv_to_rgb(h, s, v))
}

/// Hues inside the colour's ranges, from each bound up to just short of the next, with the
/// range's start and end beside each (8-bit rounding moves a hue by up to about a degree).
fn hues_of(colour: SearchColour) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for (i, &(start, c)) in HUE_BOUNDS.iter().enumerate() {
        if c != colour {
            continue;
        }
        let end = HUE_BOUNDS.get(i + 1).map_or(360.0, |b| b.0);
        let mut h = start;
        while h < end {
            out.push((h, start, end));
            h += 2.5;
        }
        out.push((end - 0.5, start, end));
    }
    out
}

#[test]
fn words_and_labels_name_the_seven_and_words_read_in_any_case() {
    let words: Vec<_> = SearchColour::ALL.iter().map(|c| c.word()).collect();
    assert_eq!(words, ["red", "orange", "yellow", "green", "blue", "purple", "bw"]);
    assert_eq!(SearchColour::Bw.label(), "Black and White");
    for c in SearchColour::ALL {
        assert_eq!(SearchColour::from_word(c.word()), Some(c));
        assert_eq!(SearchColour::from_word(&c.word().to_uppercase()), Some(c));
    }
    assert_eq!(SearchColour::from_word("Green"), Some(SearchColour::Green));
    for bad in ["", "teal", "black", "re d", "redd"] {
        assert_eq!(SearchColour::from_word(bad), None, "{bad:?}");
    }
}

#[test]
fn a_field_of_one_hue_gives_that_colour_alone_as_main() {
    for colour in SearchColour::HUES {
        let hues = hues_of(colour);
        assert!(!hues.is_empty(), "{colour:?}");
        for (h, start, end) in hues {
            assert_eq!(hue_colour(h), colour, "hue {h}");
            // Two degrees in from either bound, so rounding to 8 bits keeps it in the range.
            let h = h.clamp(start + 2.0, end - 2.0);
            for (s, v) in [(1.0, 1.0), (0.5, 0.7), (0.3, 0.35)] {
                let summary = classify(&field(hsv(h, s, v), 64));
                assert_eq!(summary.main(), Some(colour), "hue {h} s {s} v {v}");
                assert_eq!(summary.colours().collect::<Vec<_>>(), [colour], "hue {h} s {s} v {v}");
            }
        }
    }
    // Pink is red, brown is orange, cyan is blue, magenta is purple.
    assert_eq!(classify(&field([0xff, 0xc0, 0xcb], 16)).main(), Some(SearchColour::Red));
    assert_eq!(classify(&field([0x8b, 0x45, 0x13], 16)).main(), Some(SearchColour::Orange));
    assert_eq!(classify(&field([0x00, 0xff, 0xff], 16)).main(), Some(SearchColour::Blue));
    assert_eq!(classify(&field([0xff, 0x00, 0xff], 16)).main(), Some(SearchColour::Purple));
}

#[test]
fn greys_white_black_and_a_faint_tint_are_bw() {
    let bw = ColourSummary::new(SearchColour::Bw, &[]);
    let ramp: Vec<[u8; 3]> = (0..=255u8).map(|v| [v, v, v]).collect();
    assert_eq!(Some(classify(&ramp)), bw, "grey ramp");
    assert_eq!(Some(classify(&field([255; 3], 16))), bw, "white");
    assert_eq!(Some(classify(&field([0; 3], 16))), bw, "black");
    // A warm grey, saturation about 0.1.
    let tint: Vec<[u8; 3]> = (40..=250u8).map(|v| [v, v.saturating_sub(v / 12), v.saturating_sub(v / 10)]).collect();
    assert_eq!(Some(classify(&tint)), bw, "faint tint");
    // A grey picture with one coloured cell in 64 is still black and white.
    let mut speck = field([128; 3], 64);
    speck[10] = [255, 0, 0];
    assert_eq!(Some(classify(&speck)), bw, "speck");
    assert!(!classify(&speck).contains(SearchColour::Red));
}

#[test]
fn a_half_red_half_blue_grid_holds_both() {
    let mut grid = field([220, 20, 30], 32);
    grid.extend(field([20, 60, 230], 32));
    let s = classify(&grid);
    assert!(s.contains(SearchColour::Red) && s.contains(SearchColour::Blue), "{s:?}");
    assert!(!s.contains(SearchColour::Bw));
    assert_eq!(s.colours().count(), 2);
}

#[test]
fn a_green_field_with_a_small_red_dot_is_green_only() {
    let mut grid = field([30, 180, 40], 16);
    grid[5] = [230, 10, 10];
    let s = classify(&grid);
    assert_eq!(s.colours().collect::<Vec<_>>(), [SearchColour::Green], "{s:?}");
}

#[test]
fn an_empty_or_oversized_grid_is_the_empty_summary() {
    assert_eq!(classify(&[]), ColourSummary::EMPTY);
    assert_eq!(classify(&field([255, 0, 0], MAX_CELLS + 1)), ColourSummary::EMPTY);
    assert_eq!(classify(&field([255, 0, 0], MAX_CELLS)).main(), Some(SearchColour::Red));
    assert!(ColourSummary::EMPTY.is_empty() && ColourSummary::EMPTY.main().is_none());
}

#[test]
fn the_byte_form_round_trips_and_an_unknown_bit_is_refused() {
    let mut known = 0;
    for byte in 0..=u8::MAX {
        if let Some(s) = ColourSummary::from_byte(byte) {
            known += 1;
            assert_eq!(s.to_byte(), byte, "{s:?}");
        }
    }
    // The empty summary, `bw`, and a main hue with any set of the five other hues: 1 + 1 + 6 × 32.
    assert_eq!(known, 194);
    assert_eq!(ColourSummary::EMPTY.to_byte(), 0);
    let barn = ColourSummary::new(SearchColour::Red, &[SearchColour::Blue, SearchColour::Green]);
    let barn = barn.expect("hues mix");
    assert_eq!(ColourSummary::from_byte(barn.to_byte()), Some(barn));
    assert_eq!(barn.main(), Some(SearchColour::Red));
    assert_eq!(barn.colours().collect::<Vec<_>>(), [SearchColour::Red, SearchColour::Green, SearchColour::Blue]);
    // Other hues beside no main colour, or beside `bw`, are bits the form does not know.
    assert_eq!(ColourSummary::from_byte(0b0000_1000), None);
    assert_eq!(ColourSummary::from_byte(0b1000_0111), None);
    assert_eq!(ColourSummary::new(SearchColour::Bw, &[SearchColour::Red]), None);
}

/// Grids of any size up to the cap, colours of any value, plus the empty one.
fn grid() -> impl Strategy<Value = Vec<[u8; 3]>> {
    prop::collection::vec(any::<[u8; 3]>(), 0..=MAX_CELLS)
}

proptest! {
    #![proptest_config(crate::common::cases(1024, 16))]

    #[test]
    fn any_grid_classifies_without_a_panic_and_the_same_twice(cells in grid()) {
        let a = classify(&cells);
        prop_assert_eq!(a, classify(&cells));
        prop_assert_eq!(ColourSummary::from_byte(a.to_byte()), Some(a));
        prop_assert_eq!(a.is_empty(), cells.is_empty());
        if let Some(main) = a.main() {
            prop_assert!(a.contains(main));
        }
        prop_assert!(!(a.contains(SearchColour::Bw) && a.colours().count() > 1));
    }
}
