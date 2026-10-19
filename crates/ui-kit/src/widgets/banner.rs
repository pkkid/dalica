//! The filter banner (`ui-layout.md` §9, `organizing.md` §8.1): the strip across the top of the
//! light table while a search or a filter is active, [Show All] at its left and what is shown
//! after it ("Showing 33 pictures in 4 folders (0.01 s) · 439.8 MB").

use egui::{Align, Layout, Rect, Response, Ui, UiBuilder, pos2, vec2};

use super::{Target, one_line, register, text_button};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;
use crate::text::{self, Text};

/// The banner's height.
pub const BANNER_H: f32 = 32.0;
/// Room left of [Show All], between it and the text, and right of the text.
pub const BANNER_PAD: f32 = 8.0;
pub const BANNER_TEXT_GAP: f32 = 12.0;

/// What [`filter_banner`] drew.
pub struct BannerOut {
    /// The banner's rect.
    pub rect: Rect,
    /// [Show All]: `clicked()` leaves search mode.
    pub show_all: Response,
    /// Where the text went.
    pub text_rect: Rect,
    /// The text as painted: the whole text, or its start with "…" when it did not fit.
    pub shown: String,
    /// The text was cut short.
    pub elided: bool,
}

/// The filter banner (`design-system.md` §6): the full width of `ui`, [`BANNER_H`] high, filled
/// `filter_banner`; a `text_button` [Show All] (`show_all_id`, hover text "Leave search")
/// [`BANNER_PAD`] from its left, and after it, [`BANNER_TEXT_GAP`] on, `text` in 13 pt `text` on
/// one line, cut from its end with "…" where it would come closer than [`BANNER_TEXT_GAP`] to the
/// right edge. Show All always stays whole; a banner too narrow for it clips it. Registers `id`
/// (role `other`, labelled with the whole text) over the strip.
pub fn filter_banner(ui: &mut Ui, t: &Tokens, id: &str, show_all_id: &str, text: &str) -> BannerOut {
    let width = ui.available_width();
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (_, rect) = ui.allocate_space(vec2(width, BANNER_H));
    ui.painter().rect_filled(rect, 0, t.filter_banner);
    let button_area = Rect::from_min_max(pos2(rect.min.x + BANNER_PAD, rect.min.y), pos2(rect.max.x.max(rect.min.x + BANNER_PAD), rect.max.y));
    let show_all = ui
        .scope_builder(UiBuilder::new().max_rect(button_area).layout(Layout::left_to_right(Align::Center)), |ui| {
            let texts = text::texts(ui.ctx());
            text_button(ui, t, show_all_id, texts.get(Text::ShowAll), false).on_hover_text(texts.get(Text::LeaveSearch))
        })
        .inner;
    let left = show_all.rect.max.x + BANNER_TEXT_GAP;
    let max = rect.max.x - BANNER_TEXT_GAP - left;
    let p = ui.painter();
    let (shown, elided, text_rect) = if max >= 1.0 && !text.is_empty() {
        let galley = one_line(p, text, fonts::regular(t.body), t.text, max);
        let shown = galley.rows.first().map(|r| r.text()).unwrap_or_default();
        let elided = galley.elided;
        let pos = pos2(left, rect.center().y - galley.size().y / 2.0).round();
        let text_rect = Rect::from_min_size(pos, galley.size());
        p.galley(pos, galley, t.text);
        (shown, elided, text_rect)
    } else {
        (String::new(), !text.is_empty(), Rect::from_min_size(pos2(left, rect.center().y), vec2(0.0, 0.0)))
    };
    register(ui.ctx(), id, Role::Other, text, Target::Painted { rect, clip: ui.clip_rect() }, true);
    BannerOut { rect, show_all, text_rect, shown, elided }
}
