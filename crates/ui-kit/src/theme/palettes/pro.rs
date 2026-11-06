//! The palettes of Pro (Dark), Pro (Medium Gray), Pro (Light Gray) and Pro (Light), ported value
//! for value from PhotoCraft `crates/ui-egui/src/theme.rs` at revision 3a896a5e (MIT OR
//! Apache-2.0, see `NOTICE`). As in PhotoCraft, the last three are Pro (Dark) with the colours of
//! their brightness step on top: a monochrome grey interface in four steps from near-black to
//! near-white panels, with one blue accent.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens, no_overrides};

/// Pro (Dark): charcoal panels, a darker canvas, a blue accent.
const PRO: Palette = Palette {
    chrome: Color32::from_rgb(50, 50, 50),
    canvas: Color32::from_rgb(40, 40, 40),
    canvas_dot: Color32::from_rgb(40, 40, 40),
    dock: Color32::from_rgb(30, 30, 30),
    card: Color32::from_rgb(50, 50, 50),
    card_border: Color32::from_rgb(30, 30, 30),
    field: Color32::from_rgb(36, 36, 36),
    field_border: Color32::from_rgb(74, 74, 74),
    hover: Color32::from_rgb(66, 66, 66),
    pressed: Color32::from_rgb(78, 78, 78),
    text: Color32::from_rgb(222, 222, 222),
    text_dim: Color32::from_rgb(178, 178, 178),
    text_faint: Color32::from_rgb(128, 128, 128),
    icon: Color32::from_rgb(200, 200, 200),
    accent: Color32::from_rgb(55, 142, 240),
    accent_soft: Color32::from_rgb(78, 78, 78),
    accent_border: Color32::from_rgb(55, 142, 240),
    accent_text: Color32::WHITE,
    separator: Color32::from_rgb(30, 30, 30),
    shadow: Color32::from_black_alpha(150),
    primary_bg: Color32::from_rgb(55, 142, 240),
    primary_text: Color32::WHITE,
    danger: Color32::from_rgb(236, 91, 98),
    warning: Color32::from_rgb(232, 176, 70),
    tab_strip: Color32::from_rgb(38, 38, 38),
    row_selected: Color32::from_rgb(82, 82, 82),
    histogram_bg: Color32::from_rgb(40, 40, 40),
    caption_close: Color32::from_rgb(196, 43, 28),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 3,
    radius: 4,
    radius_lg: 6,
    bevel: false,
};

/// Pro (Medium Gray): mid-grey panels on Pro (Dark)'s canvas.
const PRO_MEDIUM: Palette = Palette {
    chrome: Color32::from_rgb(83, 83, 83),
    dock: Color32::from_rgb(66, 66, 66),
    card: Color32::from_rgb(83, 83, 83),
    card_border: Color32::from_rgb(66, 66, 66),
    field: Color32::from_rgb(69, 69, 69),
    field_border: Color32::from_rgb(104, 104, 104),
    hover: Color32::from_rgb(98, 98, 98),
    pressed: Color32::from_rgb(112, 112, 112),
    text: Color32::from_rgb(238, 238, 238),
    text_dim: Color32::from_rgb(212, 212, 212),
    text_faint: Color32::from_rgb(160, 160, 160),
    icon: Color32::from_rgb(226, 226, 226),
    accent_soft: Color32::from_rgb(110, 110, 110),
    separator: Color32::from_rgb(62, 62, 62),
    tab_strip: Color32::from_rgb(74, 74, 74),
    row_selected: Color32::from_rgb(107, 107, 107),
    ..PRO
};

/// Pro (Light Gray): `#B8B8B8` panels, dark text, a mid-grey canvas.
const PRO_LIGHT_GRAY: Palette = Palette {
    chrome: Color32::from_rgb(184, 184, 184),
    canvas: Color32::from_rgb(160, 160, 160),
    canvas_dot: Color32::from_rgb(160, 160, 160),
    dock: Color32::from_rgb(168, 168, 168),
    card: Color32::from_rgb(184, 184, 184),
    card_border: Color32::from_rgb(160, 160, 160),
    field: Color32::from_rgb(214, 214, 214),
    field_border: Color32::from_rgb(140, 140, 140),
    hover: Color32::from_rgb(198, 198, 198),
    pressed: Color32::from_rgb(170, 170, 170),
    text: Color32::from_rgb(20, 20, 20),
    text_dim: Color32::from_rgb(52, 52, 52),
    text_faint: Color32::from_rgb(96, 96, 96),
    icon: Color32::from_rgb(38, 38, 38),
    accent_soft: Color32::from_rgb(208, 208, 208),
    accent_text: Color32::from_rgb(0, 66, 150),
    separator: Color32::from_rgb(158, 158, 158),
    shadow: Color32::from_black_alpha(70),
    tab_strip: Color32::from_rgb(170, 170, 170),
    row_selected: Color32::from_rgb(212, 212, 212),
    ..PRO
};

/// Pro (Light): `#F0F0F0` panels, dark text, a light-grey canvas.
const PRO_LIGHT: Palette = Palette {
    chrome: Color32::from_rgb(240, 240, 240),
    canvas: Color32::from_rgb(214, 214, 214),
    canvas_dot: Color32::from_rgb(214, 214, 214),
    dock: Color32::from_rgb(226, 226, 226),
    card: Color32::from_rgb(240, 240, 240),
    card_border: Color32::from_rgb(214, 214, 214),
    field: Color32::WHITE,
    field_border: Color32::from_rgb(186, 186, 186),
    hover: Color32::from_rgb(226, 226, 226),
    pressed: Color32::from_rgb(206, 206, 206),
    text: Color32::from_rgb(20, 20, 20),
    text_dim: Color32::from_rgb(64, 64, 64),
    text_faint: Color32::from_rgb(120, 120, 120),
    icon: Color32::from_rgb(46, 46, 46),
    accent_soft: Color32::from_rgb(214, 214, 214),
    accent_text: Color32::from_rgb(16, 98, 196),
    separator: Color32::from_rgb(214, 214, 214),
    shadow: Color32::from_black_alpha(50),
    tab_strip: Color32::from_rgb(226, 226, 226),
    row_selected: Color32::from_rgb(204, 204, 204),
    ..PRO
};

/// Pro (Medium Gray)'s overrides: its tokens derived again from the palette with five colours
/// moved, so the tokens made from them (the primary button's states, the drop target, the filter
/// banner, the text levels) follow. On the mid-grey chrome PhotoCraft's values miss the contrast
/// thresholds (`tests/contrast.rs`):
/// - `accent` `#378ef0` is 2.30:1 on the chrome as a drop-target outline (needs 3:1): `#3caaff`
///   (lighter, the same blue);
/// - `field` `#454545` under a text selection in that lighter accent leaves white text below 4.5:1:
///   `#303030`, a darker well;
/// - `text` `#eeeeee` is 4.27:1 on a pressed button: white;
/// - `text_dim` `#d4d4d4` is 4.12:1 on a hovered row: `#dedede`;
/// - `text_faint` `#a0a0a0` (the slider track) is 2.94:1 on the chrome (needs 3:1): `#a6a6a6`;
/// - `danger` `#ec5b62` is 2.29:1 on the chrome (needs 3:1): `#ff9696`.
fn pro_medium(t: &mut Tokens) {
    *t = Tokens::from_palette(
        t.kind,
        &Palette {
            accent: Color32::from_rgb(60, 170, 255),
            field: Color32::from_rgb(48, 48, 48),
            text: Color32::WHITE,
            text_dim: Color32::from_rgb(222, 222, 222),
            text_faint: Color32::from_rgb(166, 166, 166),
            danger: Color32::from_rgb(255, 150, 150),
            ..PRO_MEDIUM
        },
    );
}

/// Pro (Light Gray)'s overrides: its tokens derived again from the palette with three colours
/// moved, so the tokens made from them follow. PhotoCraft's light-step values are made for dark
/// panels and miss the contrast thresholds on `#b8b8b8` (`tests/contrast.rs`):
/// - `accent` `#378ef0` is 1.68:1 on the chrome as a drop-target outline (needs 3:1): `#1e55d7`, a
///   deeper blue (PhotoCraft's own `accent_text` of this step, `#004296`, would pass on the chrome
///   but leaves dark text below 4.5:1 on a text selection);
/// - `warning` `#e8b046` is 1.01:1 on the chrome and 1.21:1 on the light table: `#704800`, a dark
///   amber;
/// - `danger` `#ec5b62` is 1.70:1 on the chrome: `#b02020`.
fn pro_light_gray(t: &mut Tokens) {
    *t = Tokens::from_palette(
        t.kind,
        &Palette {
            accent: Color32::from_rgb(30, 85, 215),
            warning: Color32::from_rgb(112, 72, 0),
            danger: Color32::from_rgb(176, 32, 32),
            ..PRO_LIGHT_GRAY
        },
    );
}

/// Pro (Light)'s overrides: its tokens derived again from the palette with three colours moved, so
/// the tokens made from them follow. On `#f0f0f0` (`tests/contrast.rs`):
/// - `accent` `#378ef0` is 2.93:1 on the chrome as a drop-target outline (needs 3:1): PhotoCraft's
///   own `accent_text` of this step, `#1062c4`;
/// - `warning` `#e8b046` is 1.72:1 on the chrome and 1.51:1 on the light table: `#a06a00`, a dark
///   amber;
/// - `danger` `#ec5b62` is 2.95:1 on the chrome: `#d23c3c`.
fn pro_light(t: &mut Tokens) {
    *t = Tokens::from_palette(
        t.kind,
        &Palette { accent: PRO_LIGHT.accent_text, warning: Color32::from_rgb(160, 106, 0), danger: Color32::from_rgb(210, 60, 60), ..PRO_LIGHT },
    );
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::Pro => (PRO, no_overrides),
        ThemeKind::ProMedium => (PRO_MEDIUM, pro_medium),
        ThemeKind::ProLightGray => (PRO_LIGHT_GRAY, pro_light_gray),
        ThemeKind::ProLight => (PRO_LIGHT, pro_light),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
