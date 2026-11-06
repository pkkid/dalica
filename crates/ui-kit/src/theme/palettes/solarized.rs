//! The palette of Solarized Dark, ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`): Ethan
//! Schoonover's Solarized colour scheme (<https://ethanschoonover.com/solarized/>, MIT, see
//! `LICENSE-solarized.txt` beside this file and `ATTRIBUTION.md`), with a base03 canvas, base02
//! surfaces, base0 body text and the Solarized blue accent.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens};

/// Solarized Dark.
const SOLARIZED_DARK: Palette = Palette {
    chrome: Color32::from_rgb(7, 54, 66),
    canvas: Color32::from_rgb(0, 43, 54),
    canvas_dot: Color32::from_rgb(10, 61, 74),
    dock: Color32::from_rgb(0, 37, 47),
    card: Color32::from_rgb(7, 54, 66),
    card_border: Color32::from_rgb(0, 37, 47),
    field: Color32::from_rgb(0, 37, 47),
    field_border: Color32::from_rgb(21, 80, 95),
    hover: Color32::from_rgb(14, 74, 90),
    pressed: Color32::from_rgb(18, 89, 107),
    text: Color32::from_rgb(131, 148, 150),
    text_dim: Color32::from_rgb(101, 123, 131),
    text_faint: Color32::from_rgb(88, 110, 117),
    icon: Color32::from_rgb(147, 161, 161),
    accent: Color32::from_rgb(38, 139, 210),
    accent_soft: Color32::from_rgba_unmultiplied_const(38, 139, 210, 46),
    accent_border: Color32::from_rgba_unmultiplied_const(38, 139, 210, 120),
    accent_text: Color32::from_rgb(253, 246, 227),
    separator: Color32::from_rgb(0, 37, 47),
    shadow: Color32::from_black_alpha(150),
    primary_bg: Color32::from_rgb(38, 139, 210),
    primary_text: Color32::from_rgb(253, 246, 227),
    danger: Color32::from_rgb(220, 50, 47),
    warning: Color32::from_rgb(181, 137, 0),
    tab_strip: Color32::TRANSPARENT,
    row_selected: Color32::TRANSPARENT,
    histogram_bg: Color32::from_rgb(0, 43, 54),
    caption_close: Color32::from_rgb(220, 50, 47),
    caption_close_text: Color32::from_rgb(253, 246, 227),
    scrim: Color32::from_black_alpha(110),
    radius_sm: 6,
    radius: 8,
    radius_lg: 12,
    bevel: false,
};

/// Solarized's base2, its light background tone.
const BASE2: Color32 = Color32::from_rgb(238, 232, 213);
/// Solarized's base1, its emphasised content tone on a dark background.
const BASE1: Color32 = Color32::from_rgb(147, 161, 161);

/// Solarized Dark's overrides, where PhotoCraft's values miss a contrast threshold
/// (`tests/contrast.rs`). Solarized's body text (base0) is a low-contrast tone by design: it is
/// 4.11:1 on the chrome (base02), 2.49:1 on a pressed button and 2.08:1 under a text selection,
/// and the dim text (base00) 2.92:1 on the chrome (all need 4.5:1). The tokens are derived again
/// from the palette with four colours moved, so the tokens made from them follow:
/// - `text`: base2 (`#eee8d5`, 10.61:1 on the chrome, 6.43:1 pressed, 5.37:1 selected);
/// - `text_dim`: base1 taken 30 % towards base2, `#aeb6b1` (6.27:1 on the chrome, 4.72:1 on a
///   hovered row); base1 itself is 3.66:1 hovered;
/// - `text_faint` (disabled text and the slider track): base0, `#839496` (4.11:1 on the chrome;
///   PhotoCraft's base01 is 2.42:1 as the track, needs 3:1);
/// - `danger` `#dc322f` (Solarized red) is 2.81:1 on the chrome (needs 3:1): 15 % towards white,
///   `#e1514e` (3.40:1).
///
/// And on top: `accent_pressed`: the Solarized blue takes black text (`on_accent`), and the rule's
/// darker pressed step leaves it at 4.30:1 (needs 4.5:1): 20 % towards white instead (`#51a2db`,
/// 7.54:1).
fn solarized_dark(t: &mut Tokens) {
    *t = Tokens::from_palette(
        t.kind,
        &Palette {
            text: BASE2,
            text_dim: BASE1.lerp_to_gamma(BASE2, 0.3),
            text_faint: SOLARIZED_DARK.text,
            danger: SOLARIZED_DARK.danger.lerp_to_gamma(Color32::WHITE, 0.15),
            ..SOLARIZED_DARK
        },
    );
    t.accent_pressed = SOLARIZED_DARK.accent.lerp_to_gamma(Color32::WHITE, 0.2);
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::SolarizedDark => (SOLARIZED_DARK, solarized_dark),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
