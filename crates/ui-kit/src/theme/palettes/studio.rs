//! The palettes of Studio (Dark) and Studio (Light), ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`):
//! near-black (or near-white) surfaces, rounded corners and a soft violet accent.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens};

/// Studio (Dark).
const STUDIO: Palette = Palette {
    chrome: Color32::from_rgb(20, 20, 21),
    canvas: Color32::from_rgb(14, 14, 15),
    canvas_dot: Color32::from_rgb(46, 46, 50),
    dock: Color32::from_rgb(17, 17, 18),
    card: Color32::from_rgb(26, 26, 28),
    card_border: Color32::from_rgb(40, 40, 44),
    field: Color32::from_rgb(35, 35, 38),
    field_border: Color32::from_rgb(52, 52, 57),
    hover: Color32::from_rgb(44, 44, 48),
    pressed: Color32::from_rgb(56, 56, 62),
    text: Color32::from_rgb(236, 236, 240),
    text_dim: Color32::from_rgb(150, 150, 158),
    text_faint: Color32::from_rgb(96, 96, 104),
    icon: Color32::from_rgb(196, 196, 204),
    accent: Color32::from_rgb(139, 124, 246),
    accent_soft: Color32::from_rgba_unmultiplied_const(139, 124, 246, 46),
    accent_border: Color32::from_rgba_unmultiplied_const(160, 148, 255, 110),
    accent_text: Color32::from_rgb(214, 208, 255),
    separator: Color32::from_rgb(38, 38, 42),
    shadow: Color32::from_black_alpha(140),
    primary_bg: Color32::from_rgb(246, 246, 248),
    primary_text: Color32::from_rgb(12, 12, 14),
    danger: Color32::from_rgb(240, 96, 96),
    warning: Color32::from_rgb(240, 190, 90),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(14, 14, 15),
    caption_close: Color32::from_rgb(196, 43, 28),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 6,
    radius: 8,
    radius_lg: 12,
    bevel: false,
};

/// Studio (Light): the same system on light surfaces.
const STUDIO_LIGHT: Palette = Palette {
    chrome: Color32::from_rgb(246, 246, 248),
    canvas: Color32::from_rgb(226, 226, 230),
    canvas_dot: Color32::from_rgb(200, 200, 206),
    dock: Color32::from_rgb(240, 240, 243),
    card: Color32::from_rgb(252, 252, 253),
    card_border: Color32::from_rgb(222, 222, 228),
    field: Color32::from_rgb(242, 242, 245),
    field_border: Color32::from_rgb(214, 214, 220),
    hover: Color32::from_rgb(232, 232, 237),
    pressed: Color32::from_rgb(220, 220, 226),
    text: Color32::from_rgb(24, 24, 28),
    text_dim: Color32::from_rgb(96, 96, 106),
    text_faint: Color32::from_rgb(150, 150, 160),
    icon: Color32::from_rgb(60, 60, 68),
    accent: Color32::from_rgb(108, 92, 231),
    accent_soft: Color32::from_rgba_unmultiplied_const(108, 92, 231, 36),
    accent_border: Color32::from_rgba_unmultiplied_const(108, 92, 231, 120),
    accent_text: Color32::from_rgb(80, 64, 200),
    separator: Color32::from_rgb(226, 226, 232),
    shadow: Color32::from_black_alpha(50),
    primary_bg: Color32::from_rgb(20, 20, 24),
    primary_text: Color32::from_rgb(250, 250, 252),
    danger: Color32::from_rgb(210, 60, 60),
    warning: Color32::from_rgb(190, 130, 20),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_gray(40),
    caption_close: Color32::from_rgb(196, 43, 28),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 6,
    radius: 8,
    radius_lg: 12,
    bevel: false,
};

/// Studio (Dark)'s overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`):
/// - `caption_close`: the violet accent takes black text (`on_accent`), so the title bar's Close
///   glyph is black, and on PhotoCraft's `#c42b1c` held over the near-black chrome it is 2.81:1
///   (needs 3:1): `#e8483c`, a lighter red;
/// - `track` (`text_faint` `#606068`) is 2.95:1 on the chrome (needs 3:1): `#64646c`.
fn studio(t: &mut Tokens) {
    t.caption_close = Color32::from_rgb(232, 72, 60);
    t.track = Color32::from_rgb(100, 100, 108);
}

/// Studio (Light)'s overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`):
/// - `accent_hover`: the rule's 10 % towards white leaves the white button text at 4.07:1 (needs
///   4.5:1); on a light theme the hovered primary button goes darker instead, `accent` × 0.93;
/// - `caution` (`warning` `#be8214`) is 2.89:1 on the light table (needs 3:1): `#b07810`;
/// - `track` (`text_faint` `#9696a0`) is 2.71:1 on the chrome (needs 3:1): `#888892`.
fn studio_light(t: &mut Tokens) {
    t.accent_hover = Color32::from_rgb(100, 86, 215);
    t.caution = Color32::from_rgb(176, 120, 16);
    t.track = Color32::from_rgb(136, 136, 146);
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::Studio => (STUDIO, studio),
        ThemeKind::StudioLight => (STUDIO_LIGHT, studio_light),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
