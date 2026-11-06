//! The palette of Classic, ported value for value from PhotoCraft `crates/ui-egui/src/theme.rs` at
//! revision 3a896a5e (MIT OR Apache-2.0, see `NOTICE`): a deliberately old desktop look, warm grey
//! panels drawn with bevels, square corners and a navy selection.

use egui::Color32;

use super::super::{Palette, Theme, ThemeKind, Tokens};

/// Classic.
const CLASSIC: Palette = Palette {
    chrome: Color32::from_rgb(212, 208, 200),
    canvas: Color32::from_rgb(128, 128, 128),
    canvas_dot: Color32::from_rgb(128, 128, 128),
    dock: Color32::from_rgb(212, 208, 200),
    card: Color32::from_rgb(212, 208, 200),
    card_border: Color32::from_rgb(128, 128, 128),
    field: Color32::WHITE,
    field_border: Color32::from_rgb(128, 128, 128),
    hover: Color32::from_rgb(226, 222, 214),
    pressed: Color32::from_rgb(190, 186, 178),
    text: Color32::BLACK,
    text_dim: Color32::from_rgb(64, 64, 64),
    text_faint: Color32::from_rgb(128, 128, 128),
    icon: Color32::BLACK,
    accent: Color32::from_rgb(10, 36, 106),
    accent_soft: Color32::from_rgb(10, 36, 106),
    accent_border: Color32::from_rgb(10, 36, 106),
    accent_text: Color32::WHITE,
    separator: Color32::from_rgb(128, 128, 128),
    shadow: Color32::from_black_alpha(0),
    primary_bg: Color32::from_rgb(212, 208, 200),
    primary_text: Color32::BLACK,
    danger: Color32::from_rgb(160, 0, 0),
    warning: Color32::from_rgb(128, 96, 0),
    tab_strip: Color32::from_rgb(212, 208, 200),
    row_selected: Color32::from_rgb(10, 36, 106),
    histogram_bg: Color32::from_gray(40),
    caption_close: Color32::from_rgb(196, 43, 28),
    caption_close_text: Color32::WHITE,
    scrim: Color32::from_black_alpha(110),
    radius_sm: 0,
    radius: 0,
    radius_lg: 0,
    bevel: true,
};

/// Classic's overrides, where PhotoCraft's values miss a contrast threshold (`tests/contrast.rs`):
/// - `canvas` and `folder_header`: PhotoCraft's `#808080` is its photo's surround; in Dalica the
///   canvas is also a focused field and the folder headers, where dim text on it is 2.63:1 (needs
///   4.5:1) and disabled text (`#808080`) vanishes: white, Classic's field colour, so a focused
///   field and a header read as white wells. The thumbnail cells keep the grey surround;
/// - `track` (`text_faint` `#808080`) is 2.57:1 on the chrome (needs 3:1): `#6e6e6e`.
fn classic(t: &mut Tokens) {
    t.canvas = Color32::WHITE;
    t.folder_header = Color32::WHITE;
    t.track = Color32::from_gray(110);
}

/// The palette and overrides of `kind`; `None` for a kind of another family.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let (palette, overrides): (Palette, fn(&mut Tokens)) = match kind {
        ThemeKind::Classic => (CLASSIC, classic),
        _ => return None,
    };
    Some(Theme { palette, overrides })
}
