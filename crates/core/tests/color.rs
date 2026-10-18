//! `core::color` (`editing.md` §8.1): sRGB ↔ HSV and `#rrggbb` round-trip every 8-bit colour (a
//! lattice of them in a task's gate, all 16.7 million at full strength), bad hex is refused, and
//! hostile numbers never give a value out of range.

use crate::common::full;
use dalica_core::color::{hex, hex_u8, hsv_to_rgb, parse_hex, parse_hex_u8, rgb_from_u8, rgb_to_hsv, rgb_to_u8};

/// The 8-bit levels a channel is tried at: all 256 at full strength, else every fifth and 255.
fn levels() -> Vec<u8> {
    if full() { (0..=255).collect() } else { (0..=255).step_by(5).chain([1, 128, 254, 255]).collect() }
}

#[test]
fn hsv_and_hex_round_trip_every_8_bit_colour() {
    let levels = levels();
    for &r in &levels {
        for &g in &levels {
            for &b in &levels {
                let c = [r, g, b];
                let hsv = rgb_to_hsv(rgb_from_u8(c));
                assert!((0.0..360.0).contains(&hsv[0]) && (0.0..=1.0).contains(&hsv[1]) && (0.0..=1.0).contains(&hsv[2]), "{c:?}: {hsv:?}");
                assert_eq!(rgb_to_u8(hsv_to_rgb(hsv[0], hsv[1], hsv[2])), c, "HSV {hsv:?}");
                let text = hex_u8(c);
                assert_eq!(parse_hex_u8(&text), Some(c), "{text}");
                assert_eq!(hex(rgb_from_u8(c)), text);
            }
        }
    }
}

#[test]
fn hex_is_printed_lower_case_and_read_in_either_case() {
    assert_eq!(hex_u8([0x80, 0xcf, 0xff]), "#80cfff");
    assert_eq!(parse_hex_u8("#80CFFF"), Some([0x80, 0xcf, 0xff]));
    assert_eq!(parse_hex_u8("  1a0e03 "), Some([0x1a, 0x0e, 0x03]));
    assert_eq!(parse_hex("#ffffff"), Some([1.0, 1.0, 1.0]));
    assert_eq!(hex([0.5, 0.0, 1.0]), "#8000ff");
}

#[test]
fn bad_hex_is_refused() {
    for bad in [
        "",
        "#",
        "#fff",
        "#ffff",
        "#fffffff",
        "#ff ff ff",
        "#gggggg",
        "#+fffff",
        "#-12345",
        "ff#ffff",
        "##ffffff",
        "#ffééff",
        "#ffffé",
        "0x123456",
        "#12345\u{0}",
    ] {
        assert_eq!(parse_hex_u8(bad), None, "{bad:?}");
        assert_eq!(parse_hex(bad), None, "{bad:?}");
    }
}

#[test]
fn hostile_numbers_stay_in_range() {
    for h in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -720.0, 1e30, 359.999_99, 360.0] {
        for s in [f32::NAN, -1.0, 0.5, 2.0] {
            for v in [f32::NAN, -1.0, 0.5, 2.0, f32::INFINITY] {
                let c = hsv_to_rgb(h, s, v);
                assert!(c.iter().all(|x| (0.0..=1.0).contains(x)), "hsv({h}, {s}, {v}) = {c:?}");
            }
        }
    }
    assert_eq!(hsv_to_rgb(-120.0, 1.0, 1.0), hsv_to_rgb(240.0, 1.0, 1.0), "hue wraps");
    let hsv = rgb_to_hsv([f32::NAN, 2.0, -1.0]);
    assert_eq!(hsv, rgb_to_hsv([0.0, 1.0, 0.0]));
    assert_eq!(hex([f32::NAN, f32::INFINITY, -3.0]), "#00ff00");
    assert_eq!(rgb_to_hsv([0.5, 0.5, 0.5]), [0.0, 0.0, 0.5], "a grey has no hue");
}
