//! The edit catalogue's words in the language in effect (`localization.md` → Writing translatable
//! code): the operations' labels (the effect tiles, the panels' titles, the history's steps), the
//! effects' tooltips, their parameters' labels and their choices' words. The catalogue
//! (`dalica_edit::catalog`) keeps the English, which agents read and the catalogs key on; each
//! function lists every `tl!` literal, so the key set finds them, and a text the list lacks is
//! looked up whole by its English (a test keeps the lists whole).

use crate::tl;

/// An operation's label (`Crop`, `Soft Focus`, `Undo` names it as a step).
pub fn op_label(english: &str) -> String {
    known_op(english).unwrap_or_else(|| crate::i18n::tr(english))
}

/// An effect tile's tooltip.
pub fn tooltip(english: &str) -> String {
    known_tooltip(english).unwrap_or_else(|| crate::i18n::tr(english))
}

/// An effect parameter's control label (`Fade`, `Tint color`).
pub fn param_label(english: &str) -> String {
    known_param(english).unwrap_or_else(|| crate::i18n::tr_ctx("effect", english))
}

/// A choice's word as the menu shows it (`soft_light` → `Soft Light`).
pub fn choice_word(word: &str) -> String {
    known_word(word).unwrap_or_else(|| crate::i18n::tr_ctx("blend mode", &super::effect_panel::controls::choice_label(word)))
}

fn known_op(english: &str) -> Option<String> {
    Some(match english {
        "Rotate" => tl!("Rotate"),
        "Flip Horizontal" => tl!("Flip Horizontal"),
        "Flip Vertical" => tl!("Flip Vertical"),
        "Crop" => tl!("Crop"),
        "Straighten" => tl!("Straighten"),
        "Retouch" => tl!("Retouch"),
        "Auto Fix" => tl!("Auto Fix"),
        "Auto Contrast" => tl!("Auto Contrast"),
        "Auto Color" => tl!("Auto Color"),
        "Fill Light" => tl!("Fill Light"),
        "Tuning" => tl!("Tuning"),
        "Trim" => tl!("Trim"),
        "Sharpen" => tl!("Sharpen"),
        "Sepia" => tl!("Sepia"),
        "B&W" => tl!("B&W"),
        "Warmify" => tl!("Warmify"),
        "Film Grain" => tl!("Film Grain"),
        "Tint" => tl!("Tint"),
        "Saturation" => tl!("Saturation"),
        "Soft Focus" => tl!("Soft Focus"),
        "Glow" => tl!("Glow"),
        "Filtered B&W" => tl!("Filtered B&W"),
        "Focal B&W" => tl!("Focal B&W"),
        "Graduated Tint" => tl!("Graduated Tint"),
        "Infrared Film" => tl!("Infrared Film"),
        "Plastic Lens" => tl!("Plastic Lens"),
        "HDR Look" => tl!("HDR Look"),
        "Widescreen Film" => tl!("Widescreen Film"),
        "Dream Glow" => tl!("Dream Glow"),
        "Invert Colors" => tl!("Invert Colors"),
        "Duo-Tone" => tl!("Duo-Tone"),
        "Boost" => tl!("Boost"),
        "Soften" => tl!("Soften"),
        "Vignette" => tl!("Vignette"),
        "Pixelate" => tl!("Pixelate"),
        "Focal Zoom" => tl!("Focal Zoom"),
        "Border" => tl!("Border"),
        "Drop Shadow" => tl!("Drop Shadow"),
        "Museum Matte" => tl!("Museum Matte"),
        _ => return None,
    })
}

fn known_tooltip(english: &str) -> Option<String> {
    Some(match english {
        "Sharpens detail and edges." => tl!("Sharpens detail and edges."),
        "Turns the photo warm brown, like an old print." => tl!("Turns the photo warm brown, like an old print."),
        "Turns the photo black and white." => tl!("Turns the photo black and white."),
        "Warms the colours and gives skin a golden tone." => tl!("Warms the colours and gives skin a golden tone."),
        "Adds the grain of film." => tl!("Adds the grain of film."),
        "Takes the colour out, then tints the photo with one colour." => tl!("Takes the colour out, then tints the photo with one colour."),
        "Makes the colours richer or paler." => tl!("Makes the colours richer or paler."),
        "Blurs the photo outside a sharp circle." => tl!("Blurs the photo outside a sharp circle."),
        "Makes the bright areas glow." => tl!("Makes the bright areas glow."),
        "Black and white as if shot through a coloured filter." => tl!("Black and white as if shot through a coloured filter."),
        "Keeps the colour inside a circle and turns the rest black and white." => {
            tl!("Keeps the colour inside a circle and turns the rest black and white.")
        }
        "Tints the photo with a gradient, such as a deeper sky." => tl!("Tints the photo with a gradient, such as a deeper sky."),
        "Black and white infrared film: bright foliage, dark skies." => tl!("Black and white infrared film: bright foliage, dark skies."),
        "The soft edges, grain and dark corners of a toy camera." => tl!("The soft edges, grain and dark corners of a toy camera."),
        "Brings out detail with strong local contrast." => tl!("Brings out detail with strong local contrast."),
        "Crops to a wide 1.7:1 frame with a film look and black bars. Changes the picture's size." => {
            tl!("Crops to a wide 1.7:1 frame with a film look and black bars. Changes the picture's size.")
        }
        "A soft, bright, dreamy glow." => tl!("A soft, bright, dreamy glow."),
        "Makes a negative, or turns a scanned negative positive." => tl!("Makes a negative, or turns a scanned negative positive."),
        "Maps the photo's tones onto two colours." => tl!("Maps the photo's tones onto two colours."),
        "Stronger colour and contrast." => tl!("Stronger colour and contrast."),
        "Softens the photo, or only where you paint." => tl!("Softens the photo, or only where you paint."),
        "Darkens the edges, or takes them toward a colour." => tl!("Darkens the edges, or takes them toward a colour."),
        "Turns the photo into large square pixels." => tl!("Turns the photo into large square pixels."),
        "A zoom blur outside a sharp circle." => tl!("A zoom blur outside a sharp circle."),
        "Frames the photo with two bands of colour. Changes the picture's size." => {
            tl!("Frames the photo with two bands of colour. Changes the picture's size.")
        }
        "Floats the photo over a background with a shadow. Changes the picture's size." => {
            tl!("Floats the photo over a background with a shadow. Changes the picture's size.")
        }
        "Mounts the photo in a double mat. Changes the picture's size." => tl!("Mounts the photo in a double mat. Changes the picture's size."),
        _ => return None,
    })
}

fn known_param(english: &str) -> Option<String> {
    Some(match english {
        "Amount" => tl!(ctx = "effect", "Amount"),
        "Grain" => tl!(ctx = "effect", "Grain"),
        "Lighten" => tl!(ctx = "effect", "Lighten"),
        "Tint color" => tl!(ctx = "effect", "Tint color"),
        "Fade" => tl!(ctx = "effect", "Fade"),
        "Size" => tl!(ctx = "effect", "Size"),
        "Intensity" => tl!(ctx = "effect", "Intensity"),
        "Radius" => tl!(ctx = "effect", "Radius"),
        "Filter color" => tl!(ctx = "effect", "Filter color"),
        "Sharpness" => tl!(ctx = "effect", "Sharpness"),
        "Feather" => tl!(ctx = "effect", "Feather"),
        "Shade" => tl!(ctx = "effect", "Shade"),
        "Color" => tl!(ctx = "effect", "Color"),
        "Blur Edges" => tl!(ctx = "effect", "Blur Edges"),
        "Strength" => tl!(ctx = "effect", "Strength"),
        "Letterbox" => tl!(ctx = "effect", "Letterbox"),
        "Bloom" => tl!(ctx = "effect", "Bloom"),
        "Brightness" => tl!(ctx = "effect", "Brightness"),
        "First color" => tl!(ctx = "effect", "First color"),
        "Second color" => tl!(ctx = "effect", "Second color"),
        "Contrast" => tl!(ctx = "effect", "Contrast"),
        "Softness" => tl!(ctx = "effect", "Softness"),
        "Pixel Size" => tl!(ctx = "effect", "Pixel Size"),
        "Blend mode" => tl!(ctx = "effect", "Blend mode"),
        "Focal Size" => tl!(ctx = "effect", "Focal Size"),
        "Zoom Amount" => tl!(ctx = "effect", "Zoom Amount"),
        "Edge Hardness" => tl!(ctx = "effect", "Edge Hardness"),
        "Outer color" => tl!(ctx = "effect", "Outer color"),
        "Outer thickness" => tl!(ctx = "effect", "Outer thickness"),
        "Inner color" => tl!(ctx = "effect", "Inner color"),
        "Inner thickness" => tl!(ctx = "effect", "Inner thickness"),
        "Corner radius" => tl!(ctx = "effect", "Corner radius"),
        "Caption height" => tl!(ctx = "effect", "Caption height"),
        "Shadow color" => tl!(ctx = "effect", "Shadow color"),
        "Distance" => tl!(ctx = "effect", "Distance"),
        "Angle" => tl!(ctx = "effect", "Angle"),
        "Blur" => tl!(ctx = "effect", "Blur"),
        "Background" => tl!(ctx = "effect", "Background"),
        _ => return None,
    })
}

fn known_word(word: &str) -> Option<String> {
    Some(match word {
        "normal" => tl!(ctx = "blend mode", "Normal"),
        "multiply" => tl!(ctx = "blend mode", "Multiply"),
        "screen" => tl!(ctx = "blend mode", "Screen"),
        "overlay" => tl!(ctx = "blend mode", "Overlay"),
        "soft_light" => tl!(ctx = "blend mode", "Soft Light"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_edit::catalog::{self, Kind};

    use super::super::effect_panel::controls;

    #[test]
    fn every_word_the_catalogue_shows_is_listed() {
        for op in catalog::catalog() {
            assert!(known_op(op.label).is_some(), "{}", op.label);
            if op.effect.is_none() {
                continue;
            }
            assert!(op.effect.is_some_and(|e| known_tooltip(e.tooltip).is_some()), "{}", op.name);
            for c in controls::controls(op) {
                let p = c.param();
                assert!(known_param(p.label).is_some(), "{}: {}", op.name, p.label);
                if let Kind::Choice(words) = p.kind {
                    assert!(words.iter().all(|w| known_word(w).is_some()), "{}: {words:?}", op.name);
                }
            }
        }
    }

    #[test]
    fn english_reads_as_the_catalogue() {
        for op in catalog::catalog() {
            assert_eq!(op_label(op.label), op.label);
            for c in op.effect.map(|_| controls::controls(op)).unwrap_or_default() {
                assert_eq!(param_label(c.param().label), c.param().label);
                if let Kind::Choice(words) = c.param().kind {
                    assert!(words.iter().all(|w| choice_word(w) == controls::choice_label(w)));
                }
            }
            if let Some(e) = op.effect {
                assert_eq!(tooltip(e.tooltip), e.tooltip);
            }
        }
    }
}
