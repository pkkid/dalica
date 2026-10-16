//! Tooltips (`design-system.md` §3, §6): a title line and an optional secondary line, in the popup
//! frame of the theme (`chrome`, `button_border`, radius 6, the popup shadow).
//!
//! Options ▸ General ▸ Show tooltips off is egui's tooltip delay made infinite (the UI's
//! `scale::apply` sets it every frame): [`tooltip`] then adds none, and neither do egui's own hover
//! tooltips. A widget that times its own tooltip reads the same delay.

use egui::{Response, RichText, Ui};

use crate::Tokens;
use crate::fonts;

/// The widest a tooltip grows before it wraps (points).
pub const TOOLTIP_WIDTH: f32 = 280.0;

fn content(ui: &mut Ui, t: &Tokens, title: &str, detail: Option<&str>) {
    ui.set_max_width(TOOLTIP_WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.label(RichText::new(title).font(fonts::regular(t.body)).color(t.text));
    if let Some(detail) = detail.filter(|d| !d.is_empty()) {
        ui.label(RichText::new(detail).font(fonts::regular(t.small)).color(t.text_dim));
    }
}

/// Shows a tooltip on `response` while it is hovered, enabled or not (a disabled control says why
/// it is disabled): `title` in 13 pt `text`, `detail` below in 11 pt `text_dim`, wrapped at
/// [`TOOLTIP_WIDTH`]. egui shows it after the pointer rests for the style's tooltip delay; nothing
/// while that delay is infinite (tooltips off).
pub fn tooltip(response: Response, t: &Tokens, title: &str, detail: Option<&str>) -> Response {
    let off = !response.ctx.global_style().interaction.tooltip_delay.is_finite();
    if off || (title.is_empty() && detail.is_none_or(str::is_empty)) {
        return response;
    }
    response.on_hover_ui(|ui| content(ui, t, title, detail)).on_disabled_hover_ui(|ui| content(ui, t, title, detail))
}
