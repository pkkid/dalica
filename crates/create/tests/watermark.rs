//! The export watermark (M8.11, `creating.md` §2): its size, place, text cleaning, shrinking,
//! missing glyphs, the pipeline hook and its look (fingerprints on a light and a dark scene).

mod common;

use common::*;
use dalica_create::output::{Choice, OutFormat, Purpose, Spec, render};
use dalica_create::watermark::{MARGIN, MAX_CHARS, SIZE, clean, draw, layout};
use dalica_image::{Rgba8, resize};

const TEXT: &str = "© Ana Costa 2026";

/// An opaque image of one grey.
fn flat(width: u32, height: u32, grey: u8) -> Rgba8 {
    Rgba8 { width, height, data: [grey, grey, grey, 255].repeat((width * height) as usize) }
}

/// The bounding box (x0, y0, x1, y1, inclusive) of the pixels where `keep(before, after)` holds.
fn bounds(before: &Rgba8, after: &Rgba8, keep: impl Fn(&[u8], &[u8]) -> bool) -> Option<(u32, u32, u32, u32)> {
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for (i, (p, q)) in before.data.chunks(4).zip(after.data.chunks(4)).enumerate() {
        if keep(p, q) {
            let (x, y) = (i as u32 % before.width, i as u32 / before.width);
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    b
}

/// Pixels the text made brighter (the white text, not its shadow).
fn ink(before: &[u8], after: &[u8]) -> bool {
    after[0] > before[0].saturating_add(40)
}

fn changed(before: &[u8], after: &[u8]) -> bool {
    before != after
}

fn watermarked(img: &Rgba8, text: &str) -> Rgba8 {
    let mut out = img.clone();
    draw(&mut out, text);
    out
}

#[test]
fn the_text_is_3_percent_of_the_long_side_at_three_sizes() {
    let mut ratios = Vec::new();
    for (w, h) in [(640, 427), (1600, 1067), (2400, 3600)] {
        let long = w.max(h) as f32;
        let p = layout(w, h, TEXT).unwrap();
        assert!((p.height - SIZE * long).abs() < 0.01, "{w}×{h}: line height {} for {}", p.height, SIZE * long);
        assert!((p.margin_x - MARGIN * long).abs() < 0.01 && (p.margin_y - MARGIN * long).abs() < 0.01, "{w}×{h}: {p:?}");
        assert!((p.x + p.width - (w as f32 - p.margin_x)).abs() < 0.01, "{w}×{h}: right edge {p:?}");
        assert!((p.y + p.height - (h as f32 - p.margin_y)).abs() < 0.01, "{w}×{h}: bottom edge {p:?}");

        // The drawn letters: "©" to "6" span the cap height and more; their height follows the
        // long side, inside the line box.
        let before = flat(w, h, 110);
        let after = watermarked(&before, TEXT);
        let (x0, y0, x1, y1) = bounds(&before, &after, ink).unwrap();
        let ink_h = (y1 - y0 + 1) as f32;
        assert!(ink_h > 0.5 * p.height && ink_h <= p.height + 1.0, "{w}×{h}: ink {ink_h} px in a {} px line", p.height);
        assert!(x0 as f32 >= p.x - 1.0 && x1 as f32 <= p.x + p.width + 1.0, "{w}×{h}: ink x {x0}..{x1} outside {p:?}");
        assert!(y0 as f32 >= p.y - 1.0 && y1 as f32 <= p.y + p.height + 1.0, "{w}×{h}: ink y {y0}..{y1} outside {p:?}");
        ratios.push((ink_h / long, long));
    }
    let (first, _) = ratios[0];
    for (ratio, long) in &ratios {
        // Within two pixels of the same fraction at every size.
        assert!(((ratio - first) * long).abs() <= 2.0, "ink heights over the long side: {ratios:?}");
    }
}

#[test]
fn two_sizes_agree_after_downscale() {
    let big_src = pixels(11, 2400, 1600);
    let small_src = resize::fit(&big_src, 600).unwrap();
    let big = resize::fit(&watermarked(&big_src, TEXT), 600).unwrap();
    let small = watermarked(&small_src, TEXT);
    let p = layout(600, 400, TEXT).unwrap();
    // Compare the watermark's corner: the box with a few pixels of shadow room.
    let (x0, y0) = ((p.x - 4.0).max(0.0) as u32, (p.y - 4.0).max(0.0) as u32);
    let crop = |img: &Rgba8| -> Rgba8 {
        let (w, h) = (img.width - x0, img.height - y0);
        let mut data = Vec::new();
        for y in y0..img.height {
            let row = ((y * img.width + x0) * 4) as usize;
            data.extend_from_slice(&img.data[row..row + (w * 4) as usize]);
        }
        Rgba8 { width: w, height: h, data }
    };
    let (a, b) = (crop(&big), crop(&small));
    let (max, mean) = difference(&a, &b);
    assert!(mean < 4.0, "the downscaled big watermark and the small one differ by {mean:.2} on average (max {max})");
    assert!(psnr(&a, &b) > 25.0, "PSNR {:.1} dB", psnr(&a, &b));
}

#[test]
fn it_stays_inside_the_bottom_right_margins() {
    for (w, h) in [(800, 600), (600, 800), (4000, 300), (300, 4000), (100, 100), (37, 23)] {
        let before = flat(w, h, 128);
        let after = watermarked(&before, TEXT);
        let Some(p) = layout(w, h, TEXT) else {
            assert_eq!(after, before, "{w}×{h}: no layout but pixels changed");
            continue;
        };
        let (x0, y0, x1, y1) = bounds(&before, &after, changed).unwrap();
        // The text box ends at the margins; only its shadow reaches past them (a few pixels: its
        // offset and blur, each at least a pixel), never out of the picture. From 400 pixels up the
        // shadow stays inside half of each margin.
        let reach = p.height * 0.15 + 2.0;
        assert!((x1 as f32) <= p.x + p.width + reach && x1 < w, "{w}×{h}: reaches x {x1}, {p:?}");
        assert!((y1 as f32) <= p.y + p.height + reach && y1 < h, "{w}×{h}: reaches y {y1}, {p:?}");
        if w.max(h) >= 400 {
            assert!((x1 as f32) < w as f32 - p.margin_x / 2.0, "{w}×{h}: reaches x {x1}, {p:?}");
            assert!((y1 as f32) < h as f32 - p.margin_y / 2.0, "{w}×{h}: reaches y {y1}, {p:?}");
        }
        assert!(x0 as f32 >= p.x - p.height * 0.25 - 2.0 && y0 as f32 >= p.y - p.height * 0.25 - 2.0, "{w}×{h}: starts at {x0},{y0}, {p:?}");
        assert!(p.x >= p.margin_x - 0.01 && p.y >= p.margin_y - 0.01, "{w}×{h}: {p:?}");
    }
}

#[test]
fn empty_and_blank_text_draw_nothing() {
    let before = pixels(3, 320, 240);
    for text in ["", "   ", "\n\t", "\u{202E}\u{200B}\u{FEFF}\u{7}", "\u{2066}\u{2069}"] {
        assert_eq!(layout(320, 240, text), None, "{text:?}");
        assert_eq!(watermarked(&before, text), before, "{text:?} changed the picture");
    }
}

#[test]
fn very_long_text_shrinks_to_fit() {
    let long_text = "Copyright Ana Costa, all rights reserved. ".repeat(8);
    let (w, h) = (1200, 800);
    let p = layout(w, h, &long_text).unwrap();
    assert!(p.height < SIZE * 1200.0, "not shrunk: {p:?}");
    assert!(p.x >= p.margin_x - 0.01 && p.width <= w as f32 - 2.0 * p.margin_x + 0.01, "{p:?}");
    let before = flat(w, h, 90);
    let after = watermarked(&before, &long_text);
    let (x0, _, x1, _) = bounds(&before, &after, ink).unwrap();
    assert!(x0 as f32 >= p.margin_x - 1.0 && (x1 as f32) < w as f32 - p.margin_x + 1.0, "ink {x0}..{x1}, {p:?}");

    // Only the first 200 characters are drawn.
    let first: String = long_text.chars().take(MAX_CHARS).collect();
    assert_eq!(clean(&long_text), first.trim_end());
    assert!(clean(&"x".repeat(1000)).chars().count() == MAX_CHARS);
    assert_eq!(layout(w, h, &long_text), layout(w, h, &first));
    assert_eq!(after, watermarked(&before, &first));

    // A text that cannot shrink enough on a tiny picture is left out.
    let tiny = flat(12, 8, 90);
    assert_eq!(layout(12, 8, &long_text), None);
    assert_eq!(watermarked(&tiny, &long_text), tiny);
}

#[test]
fn control_and_bidirectional_characters_are_removed() {
    assert_eq!(clean("  a\u{202E}b\u{7}c\td\u{200F}e\r\nf  "), "abc de  f");
    assert_eq!(clean("\u{2067}© Ana\u{2069}"), "© Ana");
    // Reversing controls change nothing on the picture.
    let before = flat(400, 300, 100);
    assert_eq!(watermarked(&before, "\u{202E}© Ana\u{202C}"), watermarked(&before, "© Ana"));
}

#[test]
fn a_character_without_a_glyph_draws_the_fallback() {
    let before = flat(600, 400, 100);
    for text in ["日本", "☃", "𝄞", "\u{10FFFD}", "Ana 日本 Costa"] {
        let after = watermarked(&before, text);
        assert!(bounds(&before, &after, ink).is_some(), "{text:?} drew nothing");
    }
    // Two characters Inter lacks draw the same missing-glyph box.
    assert_eq!(watermarked(&before, "日"), watermarked(&before, "本"));
}

#[test]
fn transparent_pixels_show_the_text() {
    let clear = Rgba8 { width: 400, height: 300, data: vec![0; 400 * 300 * 4] };
    let after = watermarked(&clear, TEXT);
    let opaque = after.data.chunks(4).filter(|p| p[3] > 0).count();
    assert!(opaque > 100, "only {opaque} pixels gained alpha");
    // Where the text is solid the pixel is light.
    assert!(after.data.chunks(4).any(|p| p[3] > 150 && p[0] > 200));
}

#[test]
fn hostile_images_never_panic() {
    for (w, h) in [(0, 0), (0, 10), (10, 0), (1, 1), (2, 3000), (3000, 2)] {
        let mut img = Rgba8 { width: w, height: h, data: vec![7; (w * h * 4) as usize] };
        let before = img.clone();
        draw(&mut img, TEXT);
        assert_eq!((img.width, img.height, img.data.len()), (before.width, before.height, before.data.len()));
    }
    // Pixel data that does not match the size is left alone.
    for (w, h, len) in [(100, 100, 12), (u32::MAX, u32::MAX, 0), (100, 100, 100 * 100 * 4 + 4)] {
        let mut img = Rgba8 { width: w, height: h, data: vec![9; len] };
        let before = img.clone();
        draw(&mut img, TEXT);
        assert_eq!(img, before);
    }
    assert!(layout(u32::MAX, u32::MAX, TEXT).is_some());
    assert_eq!(layout(0, 0, TEXT), None);
}

#[test]
fn the_output_pipeline_draws_it() {
    let source = png(5, 640, 480);
    let spec = Spec { format: Choice::Exactly(OutFormat::Png), watermark: Some(TEXT.into()), threads: 2, ..Spec::new(Purpose::Export) };
    let out = render(&source, &recipe(&[]), &spec).unwrap();
    assert_eq!(decode(&out.bytes), watermarked(&decode(&source), TEXT));
    let blank = Spec { watermark: Some("  ".into()), ..spec };
    assert_eq!(decode(&render(&source, &recipe(&[]), &blank).unwrap().bytes), decode(&source));
}

/// A light and a dark scene, each as a whole picture (its look) and as the watermark's corner (so
/// the fingerprint's cells see the letters).
#[test]
fn fingerprints_on_a_light_and_a_dark_scene() {
    let scene = pixels(21, 960, 640);
    let tone = |f: &dyn Fn(u8) -> u8| Rgba8 { data: scene.data.chunks(4).flat_map(|p| [f(p[0]), f(p[1]), f(p[2]), 255]).collect(), ..scene.clone() };
    let light = tone(&|v| 170 + (u16::from(v) * 85 / 255) as u8);
    let dark = tone(&|v| (u16::from(v) * 70 / 255) as u8);
    for (name, img) in [("light", light), ("dark", dark)] {
        let marked = watermarked(&img, TEXT);
        check_fingerprint(&format!("watermark-{name}"), &marked, 1.0);
        let corner = {
            let (x0, y0) = (640, 560);
            let mut data = Vec::new();
            for y in y0..marked.height {
                let row = ((y * marked.width + x0) * 4) as usize;
                data.extend_from_slice(&marked.data[row..row + ((marked.width - x0) * 4) as usize]);
            }
            Rgba8 { width: marked.width - x0, height: marked.height - y0, data }
        };
        check_fingerprint(&format!("watermark-{name}-corner"), &corner, 2.0);
    }
}
