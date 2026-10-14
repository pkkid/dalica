//! The export watermark (`creating.md` §2): white text with a soft shadow in the bottom-right
//! corner, 3 % of the long side high.
//!
//! The hook the output pipeline calls after resizing and before encoding. It draws nothing yet:
//! M8.11 fills it in (ported from LightCraft's export watermark).

use dalica_image::Rgba8;

/// Draws `text` onto `image` (8-bit sRGB, straight alpha). Empty or blank text draws nothing; text
/// that cannot be drawn (no glyphs, an image too small) is left out rather than failing the export.
pub fn draw(image: &mut Rgba8, text: &str) {
    // M8.11: the text, its shadow and the blending.
    let _ = (image, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_changes_nothing() {
        let mut image = Rgba8 { width: 2, height: 1, data: vec![1, 2, 3, 255, 4, 5, 6, 255] };
        let before = image.clone();
        draw(&mut image, "© Ana Costa");
        assert_eq!(image, before);
    }
}
