//! The palettes of Breeze Light and Breeze Dark, ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`). They are
//! PhotoCraft's adaptation of KDE Plasma's Breeze look, not a copy: its own values, chosen a few
//! levels from Breeze's surfaces so the app blends into a Plasma desktop, with Breeze's 5 px frame
//! radius. Readability wins over fidelity: the secondary and faint text, the accent and danger text
//! and the status hues are moved from Breeze's to hold 4.5:1.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens, no_overrides};

/// Breeze Light.
const BREEZE_LIGHT: Palette = Palette {
    chrome: Color32::from_rgb(219, 222, 226),
    canvas: Color32::from_rgb(219, 222, 226),
    canvas_dot: Color32::from_rgb(219, 222, 226),
    dock: Color32::from_rgb(224, 227, 231),
    card: Color32::from_rgb(236, 238, 241),
    card_border: Color32::from_rgb(203, 208, 213),
    field: Color32::from_rgb(249, 250, 252),
    field_border: Color32::from_rgb(184, 191, 198),
    hover: Color32::from_rgb(226, 230, 235),
    pressed: Color32::from_rgb(208, 214, 221),
    text: Color32::from_rgb(33, 37, 42),
    text_dim: Color32::from_rgb(54, 63, 72),
    text_faint: Color32::from_rgb(81, 90, 100),
    icon: Color32::from_rgb(52, 58, 64),
    accent: Color32::from_rgb(58, 172, 232),
    accent_soft: Color32::from_rgba_unmultiplied_const(58, 172, 232, 48),
    accent_border: Color32::from_rgba_unmultiplied_const(58, 172, 232, 170),
    accent_text: Color32::from_rgb(30, 94, 136),
    separator: Color32::from_rgb(214, 218, 222),
    shadow: Color32::from_black_alpha(45),
    primary_bg: Color32::from_rgb(35, 122, 180),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(170, 50, 64),
    warning: Color32::from_rgb(153, 72, 0),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(33, 37, 42),
    caption_close: Color32::from_rgb(213, 51, 60),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 5,
    radius: 5,
    radius_lg: 6,
    bevel: false,
};

/// Breeze Dark.
const BREEZE_DARK: Palette = Palette {
    chrome: Color32::from_rgb(38, 42, 48),
    canvas: Color32::from_rgb(17, 20, 24),
    canvas_dot: Color32::from_rgb(17, 20, 24),
    dock: Color32::from_rgb(26, 29, 34),
    card: Color32::from_rgb(29, 33, 38),
    card_border: Color32::from_rgb(52, 56, 61),
    field: Color32::from_rgb(38, 42, 48),
    field_border: Color32::from_rgb(68, 73, 79),
    hover: Color32::from_rgb(50, 54, 59),
    pressed: Color32::from_rgb(58, 63, 69),
    text: Color32::from_rgb(250, 251, 253),
    text_dim: Color32::from_rgb(200, 206, 213),
    text_faint: Color32::from_rgb(164, 170, 177),
    icon: Color32::from_rgb(236, 238, 240),
    accent: Color32::from_rgb(58, 172, 232),
    accent_soft: Color32::from_rgba_unmultiplied_const(58, 172, 232, 40),
    accent_border: Color32::from_rgba_unmultiplied_const(58, 172, 232, 160),
    accent_text: Color32::from_rgb(80, 184, 238),
    separator: Color32::from_rgb(46, 50, 54),
    shadow: Color32::from_black_alpha(150),
    primary_bg: Color32::from_rgb(30, 118, 176),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(227, 111, 123),
    warning: Color32::from_rgb(244, 120, 6),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(17, 20, 24),
    caption_close: Color32::from_rgb(213, 51, 60),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 5,
    radius: 5,
    radius_lg: 6,
    bevel: false,
};

/// Breeze Light's overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`): its tokens derived again from the palette with `accent` moved, so the
/// tokens made from it (the primary button's states, the drop target, the filter banner) follow.
/// - `accent` `#3aace8` is 1.89:1 on the chrome as a drop-target outline (needs 3:1): PhotoCraft's
///   own `accent_text` of this theme, `#1e5e88` (5.17:1), which takes white text (6.97:1). Its
///   `primary_bg` `#237ab4` would pass the outline but leaves white text on the hovered button at
///   3.91:1.
fn breeze_light(t: &mut Tokens) {
    *t = Tokens::from_palette(t.kind, &Palette { accent: BREEZE_LIGHT.accent_text, ..BREEZE_LIGHT });
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::BreezeLight => (BREEZE_LIGHT, breeze_light),
        ThemeKind::BreezeDark => (BREEZE_DARK, no_overrides),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
