//! The slideshow's caption (`viewing.md` §1.2, `design-system.md` §6): the photo's caption centred
//! near the bottom of the screen in 22 pt Inter on a rounded `caption_plate` (`black α160`), at most
//! 70 % of the screen's width, wrapping to two lines and then cut short with "…".

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, CornerRadius, Galley, Painter, Rect, Vec2, pos2, vec2};

use super::{Target, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The caption's size (points). Fixed: a slideshow caption is read from across a room, whatever
/// the UI Font Size.
pub const CAPTION_PLATE_SIZE: f32 = 22.0;
/// The most lines a caption takes before it is cut short.
pub const CAPTION_PLATE_LINES: usize = 2;
/// The widest the plate gets, as a share of its area's width.
pub const CAPTION_PLATE_WIDTH: f32 = 0.7;
/// The space inside the plate round the text (points).
pub const CAPTION_PLATE_PAD: Vec2 = vec2(14.0, 6.0);
/// The plate's corner radius (points).
pub const CAPTION_PLATE_RADIUS: u8 = 6;

/// Where a [`caption_plate`] went and what it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionPlateOut {
    /// The plate.
    pub rect: Rect,
    /// The lines as painted (the last with its "…" when cut short).
    pub lines: Vec<String>,
    /// Whether the text was cut short.
    pub elided: bool,
}

/// The caption's text laid out to at most `max_width` points: centred lines of
/// [`CAPTION_PLATE_SIZE`] Inter in `overlay_text`, wrapped at word breaks (anywhere in a word
/// longer than a line), at most [`CAPTION_PLATE_LINES`], the last cut short with "…" when the text
/// goes on. Line breaks in the caption are kept.
pub fn caption_galley(painter: &Painter, t: &Tokens, text: &str, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.trim().to_owned(), TextFormat::simple(fonts::regular(CAPTION_PLATE_SIZE), t.overlay_text));
    job.wrap = TextWrapping {
        max_width: if max_width.is_finite() { max_width.max(1.0) } else { 1.0 },
        max_rows: CAPTION_PLATE_LINES,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job.halign = Align::Center;
    painter.layout_job(job)
}

/// Paints the caption `text` in `area` (the slideshow's screen): [`caption_galley`] at most
/// [`CAPTION_PLATE_WIDTH`] of the area's width less the padding, on a `caption_plate` with
/// [`CAPTION_PLATE_PAD`] round it and radius [`CAPTION_PLATE_RADIUS`], centred across the area,
/// its bottom `bottom` points above the area's bottom (kept inside the area). An empty or blank
/// caption, or an area that cannot hold a line, paints nothing. Registered as `id` (role `other`,
/// painted, labelled with the whole caption).
pub fn caption_plate(painter: &Painter, t: &Tokens, id: &str, area: Rect, bottom: f32, text: &str) -> Option<CaptionPlateOut> {
    if text.trim().is_empty() || !(area.is_finite() && area.is_positive()) {
        return None;
    }
    let max_text = area.width() * CAPTION_PLATE_WIDTH - 2.0 * CAPTION_PLATE_PAD.x;
    if max_text < CAPTION_PLATE_SIZE {
        return None;
    }
    let galley = caption_galley(painter, t, text, max_text);
    let size = galley.rect.size() + 2.0 * CAPTION_PLATE_PAD;
    if size.y > area.height() {
        return None;
    }
    let bottom = if bottom.is_finite() { bottom.max(0.0) } else { 0.0 };
    let plate_bottom = (area.max.y - bottom).max(area.min.y + size.y);
    let rect = Rect::from_min_size(pos2((area.center().x - size.x / 2.0).round(), (plate_bottom - size.y).round()), size);
    painter.rect_filled(rect, CornerRadius::same(CAPTION_PLATE_RADIUS), t.caption_plate);
    // A centred galley's rows sit round x = 0: put that line on the plate's middle.
    let origin = pos2(rect.center().x - galley.rect.center().x, rect.min.y + CAPTION_PLATE_PAD.y - galley.rect.min.y);
    let lines: Vec<String> = galley.rows.iter().map(|r| r.text()).collect();
    let elided = galley.elided;
    painter.galley(origin, galley, t.overlay_text);
    register(painter.ctx(), id, Role::Other, text.trim(), Target::Painted { rect, clip: painter.clip_rect() }, false);
    Some(CaptionPlateOut { rect, lines, elided })
}
