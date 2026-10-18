//! Colour maths shared by the colour picker (`ui-kit`'s swatch) and the engine's colour pick
//! (`editing.md` §8.1, `design-system.md` §6): sRGB ↔ HSV and the `#rrggbb` text every colour
//! parameter is stored as.
//!
//! Ported from PhotoCraft `crates/ui-egui/src/color_picker_ui.rs` (`hsv_to_rgb`, `rgb_to_hsv`,
//! `hex`, `parse_hex`) at e4b2868a, MIT OR Apache-2.0; changed so that no input, however hostile,
//! gives a value outside 0 … 1 (or 0 … 360 for a hue), and with 8-bit forms beside the float ones.
//!
//! Every channel is sRGB-encoded (what `#rrggbb` means), 0 … 1. A hue is in degrees, 0 … 360
//! (360 excluded); saturation and value are 0 … 1.

/// An sRGB colour: red, green and blue, each 0 … 1.
pub type Rgb = [f32; 3];

/// A colour as hue (degrees, 0 … 360), saturation and value (each 0 … 1).
pub type Hsv = [f32; 3];

/// `v` inside 0 … 1; NaN is 0.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// The colour of hue `h` (degrees, any value: it wraps), saturation `s` and value `v` (clamped to
/// 0 … 1). A hue that is not finite reads as 0.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb {
    let h = if h.is_finite() { h.rem_euclid(360.0) / 60.0 } else { 0.0 };
    let (s, v) = (unit(s), unit(v));
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    // `h` is in 0 … 6; the sector is its whole part (6 itself, from rounding, is the last).
    let sector = h.floor();
    let (r, g, b) = if sector < 1.0 {
        (c, x, 0.0)
    } else if sector < 2.0 {
        (x, c, 0.0)
    } else if sector < 3.0 {
        (0.0, c, x)
    } else if sector < 4.0 {
        (0.0, x, c)
    } else if sector < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = v - c;
    [unit(r + m), unit(g + m), unit(b + m)]
}

/// The hue (degrees, 0 … 360), saturation and value of `c` (each channel clamped to 0 … 1, NaN
/// as 0). A grey has hue 0 and saturation 0; black has saturation 0.
pub fn rgb_to_hsv(c: Rgb) -> Hsv {
    let c = c.map(unit);
    let [r, g, b] = c;
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d <= 0.0 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    // A hue a rounding error short of 360 is 0.
    let h = if h >= 360.0 { 0.0 } else { h };
    [h, if mx <= 0.0 { 0.0 } else { d / mx }, mx]
}

/// A channel (0 … 1) in 8 bits, rounded.
pub fn to_u8(v: f32) -> u8 {
    // In 0 … 255 after the clamp, so the cast is exact.
    (unit(v) * 255.0).round() as u8
}

/// The 8-bit form of `c`, each channel rounded.
pub fn rgb_to_u8(c: Rgb) -> [u8; 3] {
    c.map(to_u8)
}

/// `c` with each 8-bit channel as 0 … 1.
pub fn rgb_from_u8(c: [u8; 3]) -> Rgb {
    c.map(|v| f32::from(v) / 255.0)
}

/// `c` as `#rrggbb`, lower case (each channel rounded to 8 bits).
pub fn hex(c: Rgb) -> String {
    hex_u8(rgb_to_u8(c))
}

/// `c` as `#rrggbb`, lower case.
pub fn hex_u8(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The colour `#rrggbb` (or `rrggbb`; either case; spaces round it ignored) names, in 8 bits.
/// Anything else is refused: another length, a non-hex digit, a sign, text that is not ASCII.
pub fn parse_hex_u8(s: &str) -> Option<[u8; 3]> {
    let h = s.trim();
    let h = h.strip_prefix('#').unwrap_or(h);
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // `get`, not `[..]`: never split a character (the digits are ASCII here, but say so in code).
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

/// The colour `#rrggbb` names, each channel 0 … 1 ([`parse_hex_u8`]).
pub fn parse_hex(s: &str) -> Option<Rgb> {
    parse_hex_u8(s).map(rgb_from_u8)
}
