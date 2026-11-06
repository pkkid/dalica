//! A theme's palette and the rule table that makes Dalica's [`Tokens`] from it
//! (`design-system.md` §2, Themes).
//!
//! [`Palette`] is PhotoCraft's token set from `crates/ui-egui/src/theme.rs` at revision 3a896a5e
//! (its colours, its three radii and `bevel`; MIT OR Apache-2.0, see `NOTICE`), so a theme's
//! values are ported value for value. Of PhotoCraft's layout flags only `bevel` is kept: the others
//! change a layout Dalica does not have. [`Tokens::from_palette`] derives Dalica's richer token set
//! from it by the one rule table written in `design-system.md` §2; a palette file may then override
//! a derived token ([`Theme::overrides`]), with its reason, where a theme would otherwise fail a
//! contrast check.

use egui::{Color32, Shadow};

use super::{ThemeKind, Tokens};

/// PhotoCraft's token set for one theme. The comments say what each colour is in PhotoCraft; the
/// rule table on [`Tokens::from_palette`] says what it becomes in Dalica.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window chrome: title bar, toolbars, panels.
    pub chrome: Color32,
    /// The photo's surround.
    pub canvas: Color32,
    /// The dots of PhotoCraft's canvas grid pattern (unused in Dalica).
    pub canvas_dot: Color32,
    /// The dock behind the cards.
    pub dock: Color32,
    /// Cards and panels.
    pub card: Color32,
    /// A card's border (unused in Dalica).
    pub card_border: Color32,
    /// Inputs, fields, dropdowns.
    pub field: Color32,
    /// A field's border.
    pub field_border: Color32,
    /// A hovered control.
    pub hover: Color32,
    /// A pressed control.
    pub pressed: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text.
    pub text_dim: Color32,
    /// Faint text: placeholders, disabled controls.
    pub text_faint: Color32,
    /// Icon glyphs.
    pub icon: Color32,
    /// The accent: focus, selection, the primary action.
    pub accent: Color32,
    /// A soft accent fill (unused in Dalica).
    pub accent_soft: Color32,
    /// An accent border (unused in Dalica).
    pub accent_border: Color32,
    /// Text in the accent colour (unused in Dalica).
    pub accent_text: Color32,
    /// Separators between regions.
    pub separator: Color32,
    /// Window and popup shadows.
    pub shadow: Color32,
    /// PhotoCraft's primary button fill (unused in Dalica: its primary button is `accent`).
    pub primary_bg: Color32,
    /// PhotoCraft's primary button text (unused in Dalica).
    pub primary_text: Color32,
    /// Destructive actions and errors.
    pub danger: Color32,
    /// Warnings.
    pub warning: Color32,
    /// PhotoCraft's panel tab strip (unused in Dalica).
    pub tab_strip: Color32,
    /// PhotoCraft's selected list row (unused in Dalica).
    pub row_selected: Color32,
    /// PhotoCraft's scope plot background (unused in Dalica).
    pub histogram_bg: Color32,
    /// The title bar's Close button hovered.
    pub caption_close: Color32,
    /// Its glyph (unused in Dalica: `on_accent` is checked on it).
    pub caption_close_text: Color32,
    /// PhotoCraft's scrim behind a modal (unused in Dalica).
    pub scrim: Color32,
    /// Small corner radius (points): buttons, fields, toggles.
    pub radius_sm: u8,
    /// Corner radius (points): menus, toasts.
    pub radius: u8,
    /// Large corner radius (points): windows' cards, the overlay bar.
    pub radius_lg: u8,
    /// Draw buttons, fields, check boxes and panels raised or sunken with a light and a dark edge
    /// (Classic) instead of flat.
    pub bevel: bool,
}

/// A palette as its family file gives it: the palette, and the derived tokens it overrides.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// The ported values.
    pub palette: Palette,
    /// Run on the tokens [`Tokens::from_palette`] derived: sets the tokens this theme overrides
    /// (each with its reason in the palette file). [`no_overrides`] for none.
    pub overrides: fn(&mut Tokens),
}

/// [`Theme::overrides`] for a theme that overrides nothing.
pub fn no_overrides(_: &mut Tokens) {}

/// The bevel's light edge (PhotoCraft's white).
pub const BEVEL_LIGHT: Color32 = Color32::WHITE;
/// The bevel's dark edge (PhotoCraft's `gray 64`).
pub const BEVEL_DARK: Color32 = Color32::from_gray(64);

/// `c` × `k` per channel, opaque.
fn scaled(c: Color32, k: f32) -> Color32 {
    let s = |v: u8| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(s(c.r()), s(c.g()), s(c.b()))
}

/// White or black, whichever reads better on `bg`.
fn readable_on(bg: Color32) -> Color32 {
    if super::contrast(Color32::WHITE, bg) >= super::contrast(Color32::BLACK, bg) { Color32::WHITE } else { Color32::BLACK }
}

impl Tokens {
    /// The tokens of theme `kind` made from `p` by the rule table of `design-system.md` §2
    /// (Themes); a palette file's overrides come on top in [`Tokens::for_kind`]. What never
    /// changes with a theme (the type sizes, the metrics, the focus width, the photo overlays,
    /// the slideshow and viewer, the dialog backdrop) stays [`Tokens::DARK`]'s.
    pub fn from_palette(kind: ThemeKind, p: &Palette) -> Tokens {
        let d = Tokens::DARK;
        let dark = super::contrast(p.text, Color32::BLACK) > super::contrast(p.chrome, Color32::BLACK);
        let photo_focus = readable_on(p.dock);
        Tokens {
            kind,
            chrome: p.chrome,
            canvas: p.canvas,
            grid_bg: p.dock,
            cell: p.canvas,
            cell_selected: p.card,
            divider: p.separator,
            inset: p.field,
            field: p.field,
            field_border: p.field_border,
            button: p.card,
            button_border: p.field_border,
            hover: p.hover,
            pressed: p.pressed,
            tool_active: p.pressed,
            text: p.text,
            text_label: p.text.lerp_to_gamma(p.text_dim, 0.5),
            text_dim: p.text_dim,
            text_disabled: p.text_faint,
            icon: p.icon,
            track: p.text_faint,
            thumb: p.icon,
            thumb_hover: p.text,
            accent: p.accent,
            accent_hover: p.accent.lerp_to_gamma(Color32::WHITE, 0.1),
            accent_pressed: scaled(p.accent, 0.85),
            on_accent: readable_on(p.accent),
            star: p.text,
            pick: p.text,
            danger: p.danger,
            caution: p.warning,
            caption_close: p.caption_close,
            folder_header: p.canvas,
            tray_bg: p.field,
            drop_target: p.accent,
            drop_target_fill: p.accent.gamma_multiply(0.15),
            filter_banner: p.chrome.lerp_to_gamma(p.accent, 0.15),
            photo_focus,
            photo_selected: photo_focus.lerp_to_gamma(p.dock, 0.35),
            badge_plate: if dark { d.badge_plate } else { Color32::from_white_alpha(170) },
            placeholder: p.canvas.lerp_to_gamma(p.text, 0.06),
            toast_bg: p.card.gamma_multiply(0.94),
            popup_shadow: Shadow { color: p.shadow, ..d.popup_shadow },
            window_shadow: Shadow { color: p.shadow, ..d.window_shadow },
            radius: p.radius_sm,
            window_radius: p.radius,
            toast_radius: p.radius,
            overlay_radius: p.radius_lg,
            filter_radius: p.radius_sm,
            bevel: p.bevel,
            ..d
        }
    }

    /// The tokens of theme `kind`: [`Tokens::DARK`] for Dark, unchanged; any other its palette
    /// through [`Tokens::from_palette`] with its overrides on top. A theme whose palette has not
    /// been ported yet answers Dark's values (with its own `kind`).
    pub fn for_kind(kind: ThemeKind) -> Tokens {
        match (kind, super::palettes::theme(kind)) {
            (ThemeKind::Dark, _) => Tokens::DARK,
            (_, Some(theme)) => {
                let mut t = Tokens::from_palette(kind, &theme.palette);
                (theme.overrides)(&mut t);
                t.kind = kind;
                t
            }
            (_, None) => Tokens { kind, ..Tokens::DARK },
        }
    }

    /// Light text on dark chrome (Dark and the other dark themes), or dark text on light.
    pub fn is_dark(&self) -> bool {
        super::contrast(self.text, Color32::BLACK) > super::contrast(self.chrome, Color32::BLACK)
    }
}
