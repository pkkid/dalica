//! The palettes of Adwaita (Light) and Adwaita (Dark), ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`): the
//! named colours of GNOME's libadwaita style (window, sidebar, card, view, accent, destructive) and
//! its radii (6 px buttons, 12 px cards). GNOME's close button is a neutral circle, not a red one,
//! so the caption colours stay grey in PhotoCraft.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens};

/// Adwaita (Light).
const ADWAITA: Palette = Palette {
    chrome: Color32::WHITE,
    canvas: Color32::from_rgb(222, 222, 223),
    canvas_dot: Color32::from_rgb(222, 222, 223),
    dock: Color32::from_rgb(235, 235, 237),
    card: Color32::WHITE,
    card_border: Color32::from_rgb(218, 218, 221),
    field: Color32::from_rgb(235, 235, 237),
    field_border: Color32::from_rgb(218, 218, 221),
    hover: Color32::from_rgb(226, 226, 229),
    pressed: Color32::from_rgb(212, 212, 216),
    text: Color32::from_rgb(50, 50, 54),
    text_dim: Color32::from_rgb(100, 100, 104),
    text_faint: Color32::from_rgb(155, 155, 160),
    icon: Color32::from_rgb(56, 56, 60),
    accent: Color32::from_rgb(53, 132, 228),
    accent_soft: Color32::from_rgba_unmultiplied_const(53, 132, 228, 40),
    accent_border: Color32::from_rgba_unmultiplied_const(53, 132, 228, 130),
    accent_text: Color32::from_rgb(28, 113, 216),
    separator: Color32::from_rgb(222, 222, 225),
    shadow: Color32::from_black_alpha(45),
    primary_bg: Color32::from_rgb(53, 132, 228),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(192, 28, 40),
    warning: Color32::from_rgb(156, 110, 3),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_gray(40),
    caption_close: Color32::from_rgb(212, 212, 216),
    caption_close_text: Color32::from_rgb(50, 50, 54),
    scrim: Color32::from_black_alpha(110),
    radius_sm: 6,
    radius: 9,
    radius_lg: 12,
    bevel: false,
};

/// Adwaita (Dark).
const ADWAITA_DARK: Palette = Palette {
    chrome: Color32::from_rgb(46, 46, 50),
    canvas: Color32::from_rgb(29, 29, 32),
    canvas_dot: Color32::from_rgb(29, 29, 32),
    dock: Color32::from_rgb(34, 34, 38),
    card: Color32::from_rgb(54, 54, 58),
    card_border: Color32::from_rgb(64, 64, 68),
    field: Color32::from_rgb(64, 64, 68),
    field_border: Color32::from_rgb(76, 76, 80),
    hover: Color32::from_rgb(72, 72, 76),
    pressed: Color32::from_rgb(86, 86, 90),
    text: Color32::WHITE,
    text_dim: Color32::from_rgb(192, 192, 194),
    text_faint: Color32::from_rgb(145, 145, 148),
    icon: Color32::from_rgb(235, 235, 237),
    accent: Color32::from_rgb(53, 132, 228),
    accent_soft: Color32::from_rgba_unmultiplied_const(53, 132, 228, 60),
    accent_border: Color32::from_rgba_unmultiplied_const(120, 174, 237, 140),
    accent_text: Color32::from_rgb(120, 174, 237),
    separator: Color32::from_rgb(24, 24, 27),
    shadow: Color32::from_black_alpha(140),
    primary_bg: Color32::from_rgb(53, 132, 228),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(255, 123, 99),
    warning: Color32::from_rgb(248, 228, 92),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(29, 29, 32),
    caption_close: Color32::from_rgb(72, 72, 76),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 6,
    radius: 9,
    radius_lg: 12,
    bevel: false,
};

/// Adwaita (Light)'s overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`); its tokens are derived again from the palette with `text_dim` moved, so
/// `text_label` follows:
/// - `text_dim` `#646468` is 4.38:1 on the canvas, which in Dalica is also a focused field and the
///   folder headers (needs 4.5:1): `#5e5e62`;
/// - `accent_pressed`: the blue accent takes black text (`on_accent`), and the rule's darker
///   pressed step (`accent` × 0.85) leaves it at 4.21:1 (needs 4.5:1); a pressed primary button
///   goes lighter instead, 20 % towards white (`#5d9de9`, 7.46:1);
/// - `track` (`text_faint` `#9b9ba0`) is 2.77:1 on the white chrome (needs 3:1): `#919196`.
fn adwaita(t: &mut Tokens) {
    *t = Tokens::from_palette(t.kind, &Palette { text_dim: Color32::from_rgb(94, 94, 98), ..ADWAITA });
    t.accent_pressed = ADWAITA.accent.lerp_to_gamma(Color32::WHITE, 0.2);
    t.track = Color32::from_rgb(145, 145, 150);
}

/// Adwaita (Dark)'s overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`):
/// - `accent_pressed`: as in Adwaita (Light), black text on the rule's darker pressed step is
///   4.21:1 (needs 4.5:1): 20 % towards white instead (`#5d9de9`, 7.46:1);
/// - `caption_close`: PhotoCraft keeps GNOME's neutral grey close button (`#48484c`) with a white
///   glyph; Dalica draws the glyph in `on_accent` (black here), 2.31:1 on it and 2.13:1 held
///   (needs 3:1): a lighter neutral grey, `#6e6e72` (4.14:1, 3.41:1 held).
fn adwaita_dark(t: &mut Tokens) {
    t.accent_pressed = ADWAITA_DARK.accent.lerp_to_gamma(Color32::WHITE, 0.2);
    t.caption_close = Color32::from_rgb(110, 110, 114);
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::Adwaita => (ADWAITA, adwaita),
        ThemeKind::AdwaitaDark => (ADWAITA_DARK, adwaita_dark),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
