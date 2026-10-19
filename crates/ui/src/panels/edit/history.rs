//! The Undo / Redo pair under every tab (`editing.md` §5.1): two buttons filling the width,
//! `button:edit.undo` and `button:edit.redo`. Undo reads "Undo " and the last edit's name ("Undo
//! Crop"), Redo "Redo " and the next one's when there is one to redo, plain "Undo" / "Redo" drawn
//! disabled otherwise; the labels are the photo's own history (`photo.history.state`). A click is a
//! step of that history (`edit.history.undo` / `edit.history.redo`), never of the library stack.
//!
//! A photo saved in place whose edits are all saved (no newer ones) has nothing left to undo in its
//! history, but its Save can be undone: Undo then reads "Undo Save", as `photo.history.state`
//! answers (`undoSave: true`), and a click runs `photo.undoSave` on it, which puts the original
//! back and its saved edits back as unsaved edits (`data-model.md` §6, M8.19).
//!
//! While comparing (`editing.md` §4) the pair is the focused side's: the focused photo's in A│B,
//! and on A│A's right side the variant's history (`photo.history.state {variant: true}`, asked
//! again only when the variant's recipe changes), whose steps the click walks
//! (`commands::compare::route`).
//!
//! **Edits from the previous organizer** (`data-model.md` §9, `editing.md` §13): when the open
//! photo's recipe holds entries kept as `legacy:` (operations Dalica has no equivalent for, kept in
//! their place), a line above the pair counts them: "1 edit from the previous program (not
//! supported)", "2 edits from the previous program (not supported)" ([`legacy_text`],
//! `label:edit.legacyEdits`, `detail` in `text_dim`, its tooltip saying what that means). The edit
//! panel gives the row [`LINE`] more points while there is one ([`line_height`]).

use dalica_core::PhotoId;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind};
use egui::{Align2, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetType, pos2, vec2};
use serde_json::json;

use crate::app::App;

pub const UNDO_ID: &str = "button:edit.undo";
/// Undo's label for a saved photo without newer edits.
pub use dalica_engine::cmd::history::UNDO_SAVE;
pub const REDO_ID: &str = "button:edit.redo";
/// The tooltips (`editing.md` §5.1 gives Undo's).
pub const UNDO_TIP: &str = "Remove the latest edit";
pub const REDO_TIP: &str = "Put back the edit removed last";
/// The gap between the two buttons.
pub const GAP: f32 = 6.0;
/// The line counting the previous organizer's edits: its automation id, height and tooltip.
pub const LEGACY_ID: &str = "label:edit.legacyEdits";
pub const LINE: f32 = 36.0;
pub const LEGACY_TIP: &str = "Kept in the photo's edits in their place, but Dalica cannot show them";

/// The line for `n` edits kept from the previous program; `None` when there are none.
pub fn legacy_text(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some("1 edit from the previous program (not supported)".into()),
        n => Some(format!("{n} edits from the previous program (not supported)")),
    }
}

/// How many entries of the open photo's recipe are kept as `legacy:` (0 with no photo open): a
/// count over one recipe, cheap enough for every frame.
pub fn legacy_count(app: &App) -> usize {
    let Some((id, s)) = crate::commands::edit_view::open_photo(app).zip(app.session()) else { return 0 };
    dalica_engine::cmd::legacy::legacy_entries(s.library().info(id))
}

/// The points the Undo / Redo row needs above its buttons: [`LINE`] while the open photo has edits
/// from the previous program, else 0.
pub fn line_height(app: &App) -> f32 {
    if legacy_count(app) > 0 { LINE } else { 0.0 }
}

/// What the pair shows for a photo: each button's label and whether it can run, and whether Undo
/// is Undo Save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub undo: (String, bool),
    pub redo: (String, bool),
    pub undo_save: bool,
}

impl Default for Pair {
    fn default() -> Pair {
        Pair { undo: ("Undo".into(), false), redo: ("Redo".into(), false), undo_save: false }
    }
}

/// The pair for `photo` from its history (`photo.history.state`'s labels: "Undo Save" for a saved
/// photo without newer edits); both disabled without a photo or a library.
pub fn pair(app: &App, photo: Option<PhotoId>) -> Pair {
    let Some((id, s)) = photo.zip(app.session()) else { return Pair::default() };
    let Some(h) = s.photo_history(id) else { return Pair::default() };
    let redo = (h.redo_text(), h.redo_label.is_some());
    if h.edits.is_empty() && s.library().can_undo_save(id) {
        return Pair { undo: (UNDO_SAVE.into(), true), redo, undo_save: true };
    }
    Pair { undo: (h.undo_text(), h.undo_label.is_some()), redo, undo_save: false }
}

/// The pair of A│A's variant from its history (`photo.history.state {variant: true}`), asked again
/// only when the variant's recipe changes (kept in the context's data with that recipe).
fn variant_pair(app: &mut App, ctx: &egui::Context) -> Pair {
    let Some(photo) = crate::commands::edit_view::open_photo(app) else { return Pair::default() };
    let edits = crate::commands::edit_view::focused_edits(app).unwrap_or_default();
    let id = Id::new(UNDO_ID).with("variant");
    if let Some((p, e, pair)) = ctx.data(|d| d.get_temp::<(PhotoId, Vec<String>, Pair)>(id))
        && p == photo
        && e == edits
    {
        return pair;
    }
    let Some(state) = app.session_mut().and_then(|s| s.execute("photo.history.state", &json!({ "id": photo, "variant": true })).ok()) else {
        return Pair::default();
    };
    let text = |k: &str, or: &str| state.get(k).and_then(serde_json::Value::as_str).unwrap_or(or).to_owned();
    let flag = |k: &str| state.get(k).and_then(serde_json::Value::as_bool).unwrap_or(false);
    let pair = Pair { undo: (text("undoLabel", "Undo"), flag("canUndo")), redo: (text("redoLabel", "Redo"), flag("canRedo")), undo_save: false };
    ctx.data_mut(|d| d.insert_temp(id, (photo, edits, pair.clone())));
    pair
}

/// Draws the pair into `ui`'s rect: the line counting the previous program's edits at its top when
/// the rect is taller than the buttons ([`line_height`]), the buttons below.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let full = ui.max_rect();
    let buttons = full.height().min(super::panel::HISTORY);
    let rect = Rect::from_min_max(pos2(full.min.x, full.max.y - buttons), full.max);
    // The line keeps the gap the buttons keep between them clear above them.
    let line = Rect::from_min_max(full.min, pos2(full.max.x, (rect.min.y - GAP).max(full.min.y)));
    if line.height() > 0.0
        && let Some(text) = legacy_text(legacy_count(app))
    {
        legacy_line(ui, &t, line, &text);
    }
    let half = ((rect.width() - GAP) / 2.0).max(0.0).floor();
    let undo_rect = Rect::from_min_size(rect.min, vec2(half, rect.height()));
    let redo_rect = Rect::from_min_max(pos2(rect.max.x - half, rect.min.y), rect.max);
    let photo = crate::commands::edit_view::open_photo(app);
    let p = if crate::commands::compare::on_variant(app) { variant_pair(app, ui.ctx()) } else { pair(app, photo) };
    let undo = button(ui, &t, UNDO_ID, &p.undo.0, undo_rect, p.undo.1, UNDO_TIP).clicked();
    let redo = button(ui, &t, REDO_ID, &p.redo.0, redo_rect, p.redo.1, REDO_TIP).clicked();
    let run = match (undo, redo, photo) {
        (true, _, Some(photo)) if p.undo_save => app.run("photo.undoSave", &json!({ "ids": [photo] })),
        (true, _, _) => app.run("edit.history.undo", &json!({})),
        (false, true, _) => app.run("edit.history.redo", &json!({})),
        _ => return,
    };
    if let Err(e) = run {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// The line `text` in `rect`, left-aligned and centred vertically: `detail` in `text_dim`, wrapped
/// to the width on at most two lines (elided past that), with [`LEGACY_TIP`].
fn legacy_line(ui: &mut Ui, t: &Tokens, rect: Rect, text: &str) {
    let r = ui.interact(rect, Id::new(LEGACY_ID), Sense::hover());
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), fonts::regular(t.detail), t.text_dim, rect.width().max(1.0));
    job.wrap.max_rows = 2;
    let galley = ui.painter().layout_job(job);
    let at = pos2(rect.min.x, rect.center().y - galley.size().y / 2.0).round();
    ui.painter().galley(at, galley, t.text_dim);
    widgets::register(ui.ctx(), LEGACY_ID, Role::Other, text, Target::Widget(&r), false);
    widgets::tooltip(r, t, LEGACY_TIP, None);
}

/// A button filling `rect` with a centred label, as `widgets::text_button` draws it: SemiBold
/// `button_label` `text`, radius 4, fill `button` / `hover` / `pressed`, 1 pt `button_border`;
/// `text_disabled` when disabled; the label elided to fit. The tooltip shows disabled too.
fn button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, rect: Rect, enabled: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let fill = if enabled && r.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if enabled { t.text } else { t.text_disabled };
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::semibold(t.button_label), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 12.0).max(1.0));
    let galley = p.layout_job(job);
    p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), rect).min.round(), galley, color);
    widgets::register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::tooltip(r, t, tip, None)
}
