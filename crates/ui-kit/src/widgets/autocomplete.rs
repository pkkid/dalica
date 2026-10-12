//! The autocomplete list under a text field (`design-system.md` §6): the options holding every word
//! typed, the ones starting with the text first; ↑ / ↓ move a highlight, Enter picks the highlighted
//! option, a click picks one, Esc closes the list (a second Esc gives back the field's text). The
//! list opens below the field, above it when there is no room below, and always stays inside the
//! window. For the Tags pane's tag entry and the search field's suggestions.
//!
//! Adapted from LightCraft `crates/ui-egui/src/album_picker.rs` at revision 76b53778 (the search
//! that matches every word and puts names starting with the text first, the ↑ / ↓ / Enter keys
//! taken before the text box sees them, the highlighted row scrolled into view) and PhotoCraft
//! `crates/ui-egui/src/widgets/type_ahead.rs` at revision 9a9d23d2 (an open list owns the keys it
//! uses; a list opened anew starts with nothing chosen; case-insensitive matching beyond ASCII),
//! both MIT OR Apache-2.0 (<https://github.com/storytold/lightcraft>,
//! <https://github.com/storytold/photocraft>; `NOTICE`). Changes for Dalica: a list of plain
//! strings under any `ui-kit` field instead of an album tree behind a button; nothing highlighted
//! until ↓ (Enter then commits the typed text, so a new tag can be typed); colours and sizes from
//! [`Tokens`]; rows registered as `<field id>/option/<index>` (role `suggestion`); never a panic.

use egui::{Align2, CornerRadius, Id, Key, Modifiers, Popup, PopupKind, Rect, RectAlign, Sense, Ui, WidgetType, pos2, vec2};

use super::field::{FieldOut, egui_id};
use super::{Target, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The most rows the list holds; a longer list of matches is cut (type more to narrow it).
pub const MAX_SUGGESTIONS: usize = 100;

/// The most rows shown at once; more scroll.
pub const VISIBLE_SUGGESTIONS: usize = 8;

/// Gap between the field and the list, in points.
const GAP: f32 = 2.0;

/// What an [`autocomplete`] did this pass.
#[derive(Debug, Clone)]
pub struct AutocompleteOut {
    /// The field's own result. A pick ends the edit like any other: Enter as [`super::Ending::Return`],
    /// a click as [`super::Ending::Away`], with the picked text already in the field.
    pub field: FieldOut,
    /// The index into the options of the one picked this pass, if any.
    pub picked: Option<usize>,
    /// The list is shown.
    pub open: bool,
}

/// What the list remembers between passes.
#[derive(Clone, Default)]
struct State {
    /// The highlighted row (an index into the matches), moved by ↑ / ↓.
    highlight: Option<usize>,
    /// Esc closed the list: it stays closed until the text changes.
    dismissed: bool,
    /// The text the matches were made for.
    query: String,
    /// The list was shown at the end of the last pass, with these visible rows (screen rect, option
    /// index).
    open: bool,
    rows: Vec<(Rect, usize)>,
    /// The pass the list was last drawn in.
    pass: u64,
}

/// The options holding every word of `query` (any case, any script), those starting with the
/// whole query first, then the rest, each group in the options' order. Indexes into `options`;
/// empty for an empty query.
pub fn autocomplete_matches<S: AsRef<str>>(options: &[S], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let (mut first, mut rest) = (Vec::new(), Vec::new());
    for (i, option) in options.iter().enumerate() {
        let lower = option.as_ref().to_lowercase();
        if !words.iter().all(|w| lower.contains(w)) {
            continue;
        }
        if lower.starts_with(&query) {
            first.push(i);
        } else {
            rest.push(i);
        }
    }
    first.extend(rest);
    first
}

/// A text field with an autocomplete list of `options` under it. `field` draws the field (a
/// [`super::text_field`], or a [`super::search_field`] converted with `.into()`) registered as
/// `id`, editing the text it is given; the list shows while that field has the focus and its text
/// matches some options (at most [`MAX_SUGGESTIONS`]). See the module docs for the keys.
pub fn autocomplete<S: AsRef<str>>(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    text: &mut String,
    options: &[S],
    field: impl FnOnce(&mut Ui, &mut String) -> FieldOut,
) -> AutocompleteOut {
    let ctx = ui.ctx().clone();
    let key = Id::new(id).with("autocomplete");
    let pass = ctx.cumulative_pass_nr();
    let mut state: State = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    // A list not drawn last pass (its field went away) opens anew, with nothing highlighted.
    if state.pass.checked_add(1) != Some(pass) {
        state = State::default();
    }
    state.pass = pass;
    let focus = egui_id(id);
    let mut picked = None;
    let mut moved = false;

    // The keys and clicks of the list shown last pass (its field had the focus), taken before the
    // field sees them.
    if state.open && ctx.memory(|m| m.had_focus_last_frame(focus)) {
        let hits = capped(autocomplete_matches(options, &state.query));
        let (down, up, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down || up {
            // egui would also move the focus to the next widget up or down.
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        let last = hits.len().saturating_sub(1);
        if down {
            state.highlight = Some(state.highlight.map_or(0, |h| h.saturating_add(1).min(last)));
            moved = true;
        }
        if up {
            // Up from the first row goes back to the text as typed.
            state.highlight = state.highlight.and_then(|h| h.checked_sub(1));
            moved = true;
        }
        if escape {
            // egui drops the focus on Esc before anything runs: the field keeps it, the list closes.
            ctx.memory_mut(|m| m.request_focus(focus));
            state.dismissed = true;
            state.highlight = None;
        }
        // Enter on a highlighted row: its text goes into the field, and the Enter goes on to the
        // field, which ends the edit with it (one commit, of the picked text).
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        if enter
            && !escape
            && let Some(i) = state.highlight.and_then(|h| hits.get(h).copied())
        {
            picked = Some(i);
        }
        // A click on a row of last pass: its text goes into the field before the field sees the
        // click, which takes the focus away and ends the edit (again one commit, of the picked
        // text).
        let click = ui.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None });
        if let Some(at) = click
            && let Some(&(_, i)) = state.rows.iter().find(|(r, _)| r.contains(at))
        {
            picked = Some(i);
        }
    }
    if let Some(option) = picked.and_then(|i| options.get(i)) {
        *text = option.as_ref().to_owned();
        state.query = text.clone();
    }

    let out = field(ui, text);
    if *text != state.query {
        // New text: new matches, nothing highlighted, and a list closed by Esc opens again.
        state.query = text.clone();
        state.highlight = None;
        state.dismissed = false;
    }
    let focused = ctx.memory(|m| m.has_focus(focus));
    let hits = if focused && out.ending.is_none() && picked.is_none() && !state.dismissed {
        capped(autocomplete_matches(options, text))
    } else {
        Vec::new()
    };
    state.highlight = state.highlight.filter(|h| *h < hits.len());
    state.open = !hits.is_empty();
    state.rows.clear();
    if state.open {
        state.rows = list(ui, t, id, &out, options, &hits, state.highlight, moved);
    }
    let open = state.open;
    ctx.data_mut(|d| d.insert_temp(key, state));
    AutocompleteOut { field: out, picked, open }
}

fn capped(mut hits: Vec<usize>) -> Vec<usize> {
    hits.truncate(MAX_SUGGESTIONS);
    hits
}

/// Draws the open list under (or above) the field; returns its visible rows.
#[allow(clippy::too_many_arguments)] // the list's inputs, kept apart from the state for clarity
fn list<S: AsRef<str>>(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    field: &FieldOut,
    options: &[S],
    hits: &[usize],
    highlight: Option<usize>,
    moved: bool,
) -> Vec<(Rect, usize)> {
    let ctx = ui.ctx().clone();
    let anchor = field.rect;
    let row_h = t.metrics.interact_h;
    let margin = ui.style().spacing.menu_margin;
    let frame_v = f32::from(margin.top) + f32::from(margin.bottom) + 2.0;
    // As many rows as fit on the roomier side of the field, at most VISIBLE_SUGGESTIONS.
    let screen = ctx.content_rect();
    let below = screen.max.y - anchor.max.y - GAP - frame_v;
    let above = anchor.min.y - screen.min.y - GAP - frame_v;
    let room = below.max(above).max(row_h);
    let wanted = row_h * hits.len().min(VISIBLE_SUGGESTIONS) as f32;
    let height = wanted.min(room);
    let width = anchor.width().max(120.0);
    let inner_w = (width - f32::from(margin.left) - f32::from(margin.right)).max(row_h);
    let aligns = [RectAlign::TOP_START, RectAlign::BOTTOM_END, RectAlign::TOP_END];
    let mut rows = Vec::new();
    Popup::new(Id::new(id).with("autocomplete.popup"), ctx.clone(), anchor, ui.layer_id())
        .kind(PopupKind::Popup)
        .open(true)
        .align(RectAlign::BOTTOM_START)
        .align_alternatives(&aligns)
        .gap(GAP)
        .width(width)
        .show(|ui| {
            ui.set_width(inner_w);
            ui.spacing_mut().item_spacing.y = 0.0;
            // The wheel and the arrow keys scroll it; the bar only shows where the list is (a bar
            // that takes drags would be a control without an automation id).
            let scroll = egui::ScrollArea::vertical().max_height(height).auto_shrink([false, true]);
            scroll.scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL).show(ui, |ui| {
                let clip = ui.clip_rect();
                for (row, &i) in hits.iter().enumerate() {
                    let label = options.get(i).map_or("", |o| o.as_ref());
                    let lit = highlight == Some(row);
                    // Not focusable: the keys reach the rows through the field.
                    let (rect, r) = ui.allocate_exact_size(vec2(inner_w, row_h), Sense::CLICK);
                    let hovered = r.hovered();
                    let p = ui.painter();
                    if lit {
                        p.rect_filled(rect, CornerRadius::same(t.radius), t.accent);
                    } else if hovered {
                        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
                    }
                    let color = if lit {
                        t.on_accent
                    } else if hovered {
                        t.text
                    } else {
                        t.text_label
                    };
                    let galley = one_line(p, label, fonts::regular(t.body), color, rect.width() - 16.0);
                    let at = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect.shrink2(vec2(8.0, 0.0))).min;
                    p.galley(pos2(at.x, at.y).round(), galley, color);
                    register(ui.ctx(), &format!("{id}/option/{i}"), Role::Suggestion, label, Target::Widget(&r), lit);
                    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::SelectableLabel, true, lit, label));
                    if lit && moved {
                        r.scroll_to_me(None);
                    }
                    let visible = rect.intersect(clip);
                    if visible.is_positive() {
                        rows.push((visible, i));
                    }
                }
            });
        });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [UTILS] Excluded | Hidden Photos, [UTILS] Uncurated, Trip, Weddings Tuscany Wedding: the
    /// album picker's sample, folders written into the names.
    fn sample() -> Vec<&'static str> {
        vec!["[UTILS] / Excluded | Hidden Photos", "[UTILS] / Uncurated", "Trip", "Weddings / Tuscany Wedding"]
    }

    /// Every word typed is in the option, in any case; options that start with it come first.
    #[test]
    fn matches_hold_every_word_and_starts_come_first() {
        let v = sample();
        assert_eq!(autocomplete_matches(&v, "excl"), vec![0]);
        assert_eq!(autocomplete_matches(&v, "UTILS"), vec![0, 1], "any case");
        assert_eq!(autocomplete_matches(&v, "utils uncur"), vec![1], "every word must match");
        assert_eq!(autocomplete_matches(&v, "t"), vec![2, 0, 1, 3], "Trip starts with it; then the rest in order");
        assert_eq!(autocomplete_matches(&v, "  "), Vec::<usize>::new(), "an empty query is not a query");
        assert_eq!(autocomplete_matches(&v, "zzz"), Vec::<usize>::new());
        assert_eq!(autocomplete_matches::<&str>(&[], "a"), Vec::<usize>::new());
    }

    /// Case folds beyond ASCII (accents, Cyrillic), as the type-ahead list's does.
    #[test]
    fn case_and_scripts_beyond_ascii_match() {
        let v = ["Normal", "Éclaircir", "Ébène", "Нормальный", "Затемнение"];
        assert_eq!(autocomplete_matches(&v, "é"), vec![1, 2]);
        assert_eq!(autocomplete_matches(&v, "ÉBÈ"), vec![2]);
        assert_eq!(autocomplete_matches(&v, "з"), vec![4]);
        assert_eq!(autocomplete_matches(&v, "но"), vec![3]);
    }

    /// Big lists stay quick: 20,000 options are searched in well under a second.
    #[test]
    fn many_options_stay_quick() {
        let options: Vec<String> = (0..200).flat_map(|f| (0..100).map(move |a| format!("Folder {f} / Wedding {f}-{a}"))).collect();
        let want = options.iter().filter(|p| ["folder", "7", "wedding"].iter().all(|w| p.to_lowercase().contains(w))).count();
        let start = std::time::Instant::now();
        assert_eq!(autocomplete_matches(&options, "folder 7 wedding").len(), want);
        assert!(start.elapsed() < std::time::Duration::from_secs(1), "took {:?}", start.elapsed());
    }
}
