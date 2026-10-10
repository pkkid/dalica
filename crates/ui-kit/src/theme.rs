//! Design tokens (`docs/design-system.md` §2–§4). Every colour, metric and font size the UI paints
//! with comes from [`Tokens`]; widgets never hard-code a colour. [`Tokens::apply`] turns them into
//! egui's style and visuals.

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

use crate::fonts;

const fn grey(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// `#2d2d2d`: toolbar, side panes, bottom tray area, menu bar, dialogs (`panel_fill`, `window_fill`).
    pub chrome: Color32,
    /// `#1c1c1c`: edit-view canvas, filmstrip, folder header blocks, selected tree row, badge plates.
    pub canvas: Color32,
    /// `#0f0f0f`: background of the light table.
    pub grid_bg: Color32,
    /// `#1c1c1c`: thumbnail cell surround.
    pub cell: Color32,
    /// `#2d2d2d`: selected cell surround.
    pub cell_selected: Color32,
    /// `#1c1c1c`: 1 pt separators between regions.
    pub divider: Color32,
    /// `#242424`: button fill at rest.
    pub button: Color32,
    /// `#4c4c4c`: button stroke, window stroke.
    pub button_border: Color32,
    /// `#3a3a3a`: hovered rows and buttons.
    pub hover: Color32,
    /// `#3f3f3f`: pressed or toggled-on buttons.
    pub pressed: Color32,
    /// `#3f3f3f`: the active tool, an active filter toggle.
    pub tool_active: Color32,
    /// `#e2e2e2`: primary text.
    pub text: Color32,
    /// `#bcbcbc`: default text.
    pub text_label: Color32,
    /// `#8e8e8e`: counts, captions, hints.
    pub text_dim: Color32,
    /// `#5c5c5c`: disabled controls.
    pub text_disabled: Color32,
    /// `#9a9a9a`: icon glyphs at rest.
    pub icon: Color32,
    /// `#0165dd`: focus border, selection, drop targets.
    pub accent: Color32,
    /// `#d8d8d8`: the star badge glyph, star buttons.
    pub star: Color32,
    /// `#f0f0f0`: active icon glyph.
    pub pick: Color32,
    /// `#e04a4a`: destructive confirmations, error badges.
    pub danger: Color32,
    /// `#e3a83c`: warnings: an offline folder, a missing or unreadable file.
    pub caution: Color32,
    /// `black α130`: the plate behind thumbnail badges.
    pub badge_plate: Color32,
    /// `gray 38`: a thumbnail whose texture has not arrived.
    pub placeholder: Color32,
    /// Corner radius of widgets (points).
    pub radius: u8,
    /// Body text size (points).
    pub body: f32,
    /// Small text size (points).
    pub small: f32,
    /// Title size, SemiBold (points).
    pub title: f32,
}

impl Tokens {
    /// The dark theme, the only theme until after 1.0.
    pub const DARK: Tokens = Tokens {
        chrome: grey(0x2d),
        canvas: grey(0x1c),
        grid_bg: grey(0x0f),
        cell: grey(0x1c),
        cell_selected: grey(0x2d),
        divider: grey(0x1c),
        button: grey(0x24),
        button_border: grey(0x4c),
        hover: grey(0x3a),
        pressed: grey(0x3f),
        tool_active: grey(0x3f),
        text: grey(0xe2),
        text_label: grey(0xbc),
        text_dim: grey(0x8e),
        text_disabled: grey(0x5c),
        icon: grey(0x9a),
        accent: Color32::from_rgb(0x01, 0x65, 0xdd),
        star: grey(0xd8),
        pick: grey(0xf0),
        danger: Color32::from_rgb(0xe0, 0x4a, 0x4a),
        caution: Color32::from_rgb(0xe3, 0xa8, 0x3c),
        badge_plate: Color32::from_black_alpha(130),
        placeholder: grey(38),
        radius: 4,
        body: 13.0,
        small: 11.0,
        title: 15.0,
    };

    /// Installs the fonts and this theme's style into `ctx`. Fonts take effect on the next pass:
    /// paint nothing that matters on the first one (`design-system.md` §3).
    pub fn apply(&self, ctx: &Context) {
        fonts::install(ctx);
        let t = *self;
        ctx.all_styles_mut(move |style| {
            style.text_styles = [
                (TextStyle::Small, FontId::new(t.small, FontFamily::Proportional)),
                (TextStyle::Body, FontId::new(t.body, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(t.body, FontFamily::Proportional)),
                (TextStyle::Heading, fonts::semibold(16.0)),
                (TextStyle::Monospace, FontId::new(t.body, FontFamily::Monospace)),
            ]
            .into();
            style.spacing.item_spacing = Vec2::new(6.0, 4.0);
            style.spacing.button_padding = Vec2::new(8.0, 3.0);
            style.spacing.interact_size.y = 22.0;
            style.animation_time = 0.08;
            let v = &mut style.visuals;
            v.dark_mode = true;
            v.panel_fill = t.chrome;
            v.window_fill = t.chrome;
            v.window_stroke = Stroke::new(1.0, t.button_border);
            v.window_corner_radius = CornerRadius::same(6);
            v.menu_corner_radius = CornerRadius::same(6);
            v.extreme_bg_color = t.canvas;
            v.faint_bg_color = t.canvas;
            v.override_text_color = Some(t.text_label);
            v.selection.bg_fill = t.accent.gamma_multiply(0.6);
            v.selection.stroke = Stroke::new(1.0, t.text);
            v.hyperlink_color = t.accent;
            let r = CornerRadius::same(t.radius);
            for (w, fill) in [
                (&mut v.widgets.noninteractive, t.chrome),
                (&mut v.widgets.inactive, t.button),
                (&mut v.widgets.hovered, t.hover),
                (&mut v.widgets.active, t.pressed),
                (&mut v.widgets.open, t.pressed),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.corner_radius = r;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_label);
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text_label);
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
            v.widgets.active.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.active.fg_stroke = Stroke::new(1.0, t.pick);
        });
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Tokens::DARK
    }
}

/// WCAG 2 contrast ratio of two opaque colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    fn channel(v: u8) -> f64 {
        let c = f64::from(v) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    fn luminance(c: Color32) -> f64 {
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_the_spec() {
        let t = Tokens::default();
        let hex = |c: Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        for (got, want) in [
            (t.chrome, "#2d2d2d"),
            (t.canvas, "#1c1c1c"),
            (t.grid_bg, "#0f0f0f"),
            (t.cell, "#1c1c1c"),
            (t.cell_selected, "#2d2d2d"),
            (t.button, "#242424"),
            (t.button_border, "#4c4c4c"),
            (t.hover, "#3a3a3a"),
            (t.pressed, "#3f3f3f"),
            (t.text, "#e2e2e2"),
            (t.text_label, "#bcbcbc"),
            (t.text_dim, "#8e8e8e"),
            (t.text_disabled, "#5c5c5c"),
            (t.icon, "#9a9a9a"),
            (t.accent, "#0165dd"),
            (t.star, "#d8d8d8"),
            (t.pick, "#f0f0f0"),
            (t.danger, "#e04a4a"),
            (t.caution, "#e3a83c"),
        ] {
            assert_eq!(hex(got), want);
        }
        assert_eq!(t.badge_plate.to_array(), [0, 0, 0, 130]);
    }

    /// `design-system.md` §9: text on `chrome` and captions on `grid_bg` stay ≥ 4.5:1 (WCAG AA).
    #[test]
    fn text_contrast_is_at_least_4_5_to_1() {
        let t = Tokens::default();
        for (name, fg, bg) in [
            ("text on chrome", t.text, t.chrome),
            ("text_label on chrome", t.text_label, t.chrome),
            ("text on canvas", t.text, t.canvas),
            ("text_label on canvas", t.text_label, t.canvas),
            ("text_dim on grid_bg", t.text_dim, t.grid_bg),
            ("text_dim on canvas", t.text_dim, t.canvas),
            ("star on black", t.star, Color32::BLACK),
        ] {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{name}: {c:.2}:1");
        }
        // The formula itself: black on white is 21:1, a colour on itself 1:1.
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(t.chrome, t.chrome) - 1.0).abs() < 1e-9);
    }
}
