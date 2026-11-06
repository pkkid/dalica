//! The palettes of Soft Dark and Soft Light, ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`): PhotoCraft's
//! own low-contrast palettes, muted violet-grey surfaces (never near-black, never white), softened
//! text and a periwinkle accent, with generous radii.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens};

/// Soft Dark.
const SOFT_DARK: Palette = Palette {
    chrome: Color32::from_rgb(58, 58, 68),
    canvas: Color32::from_rgb(40, 40, 48),
    canvas_dot: Color32::from_rgb(40, 40, 48),
    dock: Color32::from_rgb(47, 47, 56),
    card: Color32::from_rgb(54, 54, 64),
    card_border: Color32::from_rgb(70, 70, 82),
    field: Color32::from_rgb(64, 64, 75),
    field_border: Color32::from_rgb(84, 84, 98),
    hover: Color32::from_rgb(68, 68, 80),
    pressed: Color32::from_rgb(74, 74, 88),
    text: Color32::from_rgb(238, 238, 246),
    text_dim: Color32::from_rgb(212, 212, 224),
    text_faint: Color32::from_rgb(190, 190, 206),
    icon: Color32::from_rgb(214, 214, 226),
    accent: Color32::from_rgb(138, 154, 232),
    accent_soft: Color32::from_rgba_unmultiplied_const(138, 154, 232, 48),
    accent_border: Color32::from_rgba_unmultiplied_const(138, 154, 232, 150),
    accent_text: Color32::from_rgb(190, 200, 252),
    separator: Color32::from_rgb(36, 36, 44),
    shadow: Color32::from_black_alpha(110),
    primary_bg: Color32::from_rgb(86, 104, 196),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(250, 160, 172),
    warning: Color32::from_rgb(240, 200, 130),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(40, 40, 48),
    caption_close: Color32::from_rgb(200, 72, 86),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(100),
    radius_sm: 7,
    radius: 10,
    radius_lg: 14,
    bevel: false,
};

/// Soft Light.
const SOFT_LIGHT: Palette = Palette {
    chrome: Color32::from_rgb(232, 230, 236),
    canvas: Color32::from_rgb(218, 216, 224),
    canvas_dot: Color32::from_rgb(218, 216, 224),
    dock: Color32::from_rgb(226, 224, 231),
    card: Color32::from_rgb(240, 238, 243),
    card_border: Color32::from_rgb(208, 205, 215),
    field: Color32::from_rgb(250, 249, 252),
    field_border: Color32::from_rgb(190, 187, 201),
    hover: Color32::from_rgb(228, 226, 234),
    pressed: Color32::from_rgb(212, 209, 221),
    text: Color32::from_rgb(44, 42, 56),
    text_dim: Color32::from_rgb(62, 60, 76),
    text_faint: Color32::from_rgb(80, 78, 96),
    icon: Color32::from_rgb(64, 62, 80),
    accent: Color32::from_rgb(110, 128, 216),
    accent_soft: Color32::from_rgba_unmultiplied_const(110, 128, 216, 44),
    accent_border: Color32::from_rgba_unmultiplied_const(110, 128, 216, 170),
    accent_text: Color32::from_rgb(46, 58, 146),
    separator: Color32::from_rgb(210, 207, 218),
    shadow: Color32::from_black_alpha(40),
    primary_bg: Color32::from_rgb(80, 98, 190),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(166, 44, 62),
    warning: Color32::from_rgb(140, 72, 0),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(44, 42, 56),
    caption_close: Color32::from_rgb(200, 60, 72),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(90),
    radius_sm: 7,
    radius: 10,
    radius_lg: 14,
    bevel: false,
};

/// Soft Dark's overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`): its tokens derived again from the palette with `accent` moved, so the
/// tokens made from it follow.
/// - `accent` `#8a9ae8`: white text under a text selection (the accent at 60 % over a field) is
///   3.78:1 (needs 4.5:1): a deeper periwinkle, `#7080d6` (4.67:1; 3.08:1 as a drop-target
///   outline on the chrome). PhotoCraft's `primary_bg` `#5668c4` would pass the selection but is
///   2.23:1 as the outline (needs 3:1);
/// - `accent_pressed`: that accent takes black text (`on_accent`), and the rule's darker pressed
///   step leaves it at 4.35:1 (needs 4.5:1): 20 % towards white instead (`#8d99de`, 7.74:1).
fn soft_dark(t: &mut Tokens) {
    let accent = Color32::from_rgb(112, 128, 214);
    *t = Tokens::from_palette(t.kind, &Palette { accent, ..SOFT_DARK });
    t.accent_pressed = accent.lerp_to_gamma(Color32::WHITE, 0.2);
}

/// Soft Light's overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`): its tokens derived again from the palette with `accent` moved, so the
/// tokens made from it follow.
/// - `accent` `#6e80d8` is 2.95:1 on the chrome as a drop-target outline (needs 3:1), and black
///   text on its pressed step is 4.36:1 (needs 4.5:1): PhotoCraft's own `primary_bg` of this
///   theme, `#5062be` (4.43:1 as the outline), which takes white text (5.48:1);
/// - `accent_hover`: the rule's 10 % towards white leaves that white text at 4.45:1 (needs
///   4.5:1); on a light theme the hovered primary button goes darker instead, `accent` × 0.93
///   (`#4a5bb1`, 6.14:1), as Studio (Light) does.
fn soft_light(t: &mut Tokens) {
    *t = Tokens::from_palette(t.kind, &Palette { accent: SOFT_LIGHT.primary_bg, ..SOFT_LIGHT });
    t.accent_hover = Color32::from_rgb(74, 91, 177);
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::SoftDark => (SOFT_DARK, soft_dark),
        ThemeKind::SoftLight => (SOFT_LIGHT, soft_light),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
