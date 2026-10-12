//! Painting the Properties pane's list: label/value rows (the label in Small `text_dim` over the
//! value in `text`, a note in Small under it, a divider below), Show All's groups (a SemiBold
//! heading, then one line per field: the label at the left, the value beside it), and message
//! lines ("Reading the file…"). The list scrolls with the wheel; only the items in view are laid out
//! and painted, so a long listing costs what is on screen. A value too long for its line is cut
//! with "…" (a path keeps its end: "…/Lisbon/IMG_0361.jpg") and shows in full in a tooltip.

use std::sync::Arc;

use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, FontId, Galley, Id, Painter, Rect, Sense, Ui, pos2, vec2};

use super::answer::{CHANGED_ELSEWHERE, Row};

/// The space above and below a row's text, and between a field's label and value (points).
const PAD: f32 = 5.0;
/// The share of a Show All line the label takes.
const LABEL_SHARE: f32 = 0.42;
/// The space above a group's heading (points).
const GROUP_GAP: f32 = 10.0;

/// What a message line is about: its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Dim,
    Caution,
}

/// One item of the list.
#[derive(Clone, Copy, Debug)]
pub enum Item<'a> {
    /// A label/value row of the main list.
    Row(&'a Row),
    /// A line of text: loading, an error, a note.
    Line(&'a str, Tone),
    /// A Show All group's heading.
    Group(&'a str),
    /// A Show All field: label and value on one line.
    Field(&'a Row),
}

/// Where an item in view was painted, and whether its value was cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Painted {
    pub rect: Rect,
    pub elided: bool,
}

/// The line heights of the pane's two fonts.
#[derive(Clone, Copy)]
struct Heights {
    body: f32,
    small: f32,
}

fn height(item: &Item<'_>, h: Heights) -> f32 {
    match item {
        Item::Row(r) => PAD + h.small + 1.0 + h.body + if r.note.is_some() { 1.0 + h.small } else { 0.0 } + PAD,
        Item::Line(..) => PAD + h.body + PAD,
        Item::Group(_) => GROUP_GAP + h.body + 3.0,
        Item::Field(_) => h.body + 4.0,
    }
}

/// `text` laid out on one line within `max`: cut at its end with "…", or with `keep_end` at its
/// start ("…/Lisbon/IMG_0361.jpg"). Returns the galley and whether it was cut.
pub fn fit(painter: &Painter, text: &str, font: &FontId, color: Color32, max: f32, keep_end: bool) -> (Arc<Galley>, bool) {
    let max = if max.is_finite() { max.max(1.0) } else { 1.0 };
    if !keep_end {
        let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font.clone(), color));
        job.wrap = TextWrapping::truncate_at_width(max);
        let galley = painter.layout_job(job);
        let elided = galley.elided;
        return (galley, elided);
    }
    let whole = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    if whole.size().x <= max {
        return (whole, false);
    }
    let chars: Vec<char> = text.chars().collect();
    let tail = |k: usize| -> String { std::iter::once('…').chain(chars.iter().skip(chars.len().saturating_sub(k)).copied()).collect() };
    // The most characters kept at the end that still fit, by bisection (width grows with them).
    let (mut lo, mut hi) = (0usize, chars.len().saturating_sub(1));
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if painter.layout_no_wrap(tail(mid), font.clone(), color).size().x <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (painter.layout_no_wrap(tail(lo), font.clone(), color), true)
}

/// Paints `galley` at the left of `rect`, centred vertically.
fn put(painter: &Painter, rect: Rect, galley: Arc<Galley>, color: Color32) {
    let at = pos2(rect.min.x, rect.center().y - galley.size().y / 2.0).round();
    painter.galley(at, galley, color);
}

/// Paints one item into `rect`; returns whether its value was cut.
fn paint(ui: &Ui, t: &Tokens, item: &Item<'_>, rect: Rect, h: Heights) -> bool {
    let painter = ui.painter();
    let (body, small) = (fonts::regular(t.body), fonts::regular(t.small));
    let w = rect.width();
    match item {
        Item::Row(r) => {
            let mut y = rect.min.y + PAD;
            let line = |y: f32, height: f32| Rect::from_min_size(pos2(rect.min.x, y), vec2(w, height));
            let (label, _) = fit(painter, &r.label, &small, t.text_dim, w, false);
            put(painter, line(y, h.small), label, t.text_dim);
            y += h.small + 1.0;
            let (value, elided) = fit(painter, &r.value, &body, t.text, w, r.key == "path");
            put(painter, line(y, h.body), value, t.text);
            y += h.body + 1.0;
            if let Some(note) = &r.note {
                let color = if note == CHANGED_ELSEWHERE { t.caution } else { t.text_dim };
                let (galley, _) = fit(painter, note, &small, color, w, false);
                put(painter, line(y, h.small), galley, color);
            }
            painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, rect.max.y - 1.0), rect.max), 0, t.divider);
            elided
        }
        Item::Line(text, tone) => {
            let color = match tone {
                Tone::Dim => t.text_dim,
                Tone::Caution => t.caution,
            };
            let (galley, elided) = fit(painter, text, &body, color, w, false);
            put(painter, rect, galley, color);
            elided
        }
        Item::Group(name) => {
            let text = Rect::from_min_max(pos2(rect.min.x, rect.min.y + GROUP_GAP), pos2(rect.max.x, rect.max.y - 3.0));
            let (galley, _) = fit(painter, name, &fonts::semibold(t.body), t.text_label, w, false);
            put(painter, text, galley, t.text_label);
            painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, rect.max.y - 1.0), rect.max), 0, t.divider);
            false
        }
        Item::Field(r) => {
            let label_w = (w * LABEL_SHARE).round();
            let label_rect = Rect::from_min_size(rect.min, vec2(label_w - PAD, rect.height()));
            let value_rect = Rect::from_min_max(pos2(rect.min.x + label_w, rect.min.y), rect.max);
            let (label, label_cut) = fit(painter, &r.label, &small, t.text_dim, label_rect.width(), false);
            put(painter, label_rect, label, t.text_dim);
            let (value, value_cut) = fit(painter, &r.value, &body, t.text, value_rect.width(), false);
            put(painter, value_rect, value, t.text);
            label_cut || value_cut
        }
    }
}

/// The text a cut item's tooltip shows in full.
fn full(item: &Item<'_>) -> Option<(String, Option<String>)> {
    match item {
        Item::Row(r) => Some((r.value.clone(), None)),
        Item::Field(r) => Some((r.label.clone(), Some(r.value.clone()))),
        Item::Line(text, _) => Some(((*text).to_owned(), None)),
        Item::Group(_) => None,
    }
}

/// Lays out `items` top to bottom in a list that scrolls with the wheel, filling the rest of `ui`.
/// Returns, per item, where it was painted when it was in view.
pub fn list(ui: &mut Ui, t: &Tokens, items: &[Item<'_>]) -> Vec<Option<Painted>> {
    let h = ui.fonts_mut(|f| Heights { body: f.row_height(&fonts::regular(t.body)), small: f.row_height(&fonts::regular(t.small)) });
    let heights: Vec<f32> = items.iter().map(|i| height(i, h)).collect();
    let total: f32 = heights.iter().sum();
    let width = ui.available_width().max(1.0);
    let mut out = vec![None; items.len()];
    // The wheel scrolls the list; the bar only shows where it is (a bar that takes drags would be a
    // control without an automation id).
    egui::ScrollArea::vertical()
        .id_salt("properties.list")
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show_viewport(ui, |ui, _| {
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let clip = ui.clip_rect();
            let mut y = area.min.y;
            for (n, (item, height)) in items.iter().zip(&heights).enumerate() {
                let rect = Rect::from_min_size(pos2(area.min.x, y), vec2(width, *height));
                y += height;
                if rect.max.y < clip.min.y || rect.min.y > clip.max.y {
                    continue;
                }
                let elided = paint(ui, t, item, rect, h);
                if elided && let Some((title, detail)) = full(item) {
                    let response = ui.interact(rect.intersect(clip), Id::new(("properties.item", n)), Sense::hover());
                    widgets::tooltip(response, t, &title, detail.as_deref());
                }
                if let Some(slot) = out.get_mut(n) {
                    *slot = Some(Painted { rect, elided });
                }
            }
        });
    out
}
