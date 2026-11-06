//! The History pane (`ui-layout.md` §7, `editing.md` §5.1; issue #726, M16.33): the edits of one
//! photo as rows, oldest at the top, in the side pane. In the edit view it shows the open photo; in
//! the library the selected photo when exactly one is selected, else "Select one photo to see its
//! history" ([`EMPTY`]).
//!
//! - **The rows** come from the engine's `photo.history {id}` (asked again only when the library's
//!   revision, the photo or the language changed): a first row, "Original" (the photo with no
//!   edits; "Save" for a saved photo with no newer edits, marked with the Save glyph and a rule
//!   under it, since Undo Save restores the original file), then one row per step with its glyph,
//!   its name ("Crop", "Tuning") and a short detail at the right ("4:3", "Fill Light 0.30").
//! - **The current row** (where the photo is) is filled `tool_active` with an `accent` bar at its
//!   left and its name in SemiBold; the redoable rows after it are drawn dimmed.
//! - **A click** on a row runs `photo.history.goto {id, index}`: one command that undoes or redoes
//!   as many steps as needed. The redoable steps stay until a new edit replaces them, so a click is
//!   never destructive: clicking the row left returns there. "Original" undoes every step and keeps
//!   them all redoable (it is not Undo All Edits, which clears them).
//! - Each row is `history:<index>` for automation (active on the current row); the list scrolls
//!   with the wheel, and only the rows in view are painted.

use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{self, EditIcon, Glyph, Icon, OutputIcon, ViewingIcon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, FontId, Id, Painter, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::commands::edit_view::open_photo;
use crate::tl;

/// What the pane says without exactly one photo to show.
pub const EMPTY: &str = "Select one photo to see its history";
/// The command a click runs, and the query the rows come from.
pub const GOTO: &str = "photo.history.goto";
pub const LIST: &str = "photo.history";
/// A row's height, the glyph's edge and the pane's inset (points).
pub const ROW: f32 = 26.0;
const GLYPH: f32 = 16.0;
const INSET: f32 = 8.0;
/// The current row's bar at its left edge.
const BAR: f32 = 3.0;

/// A row's automation id: `history:<index>`.
pub fn row_id(index: usize) -> String {
    format!("history:{index}")
}

/// What a row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The photo with no edits.
    Original,
    /// The saved file, for a saved photo with no newer edits.
    Save,
    /// One step of the photo's history.
    Edit,
}

/// One row of the list, as `photo.history` answered it (the label and detail in English).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub index: usize,
    pub kind: Kind,
    pub label: String,
    pub detail: Option<String>,
    /// The operation's text (`crop(…)`), for the glyph.
    pub op: String,
    pub redo: bool,
}

/// The photo's rows and the index of the current one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rows {
    pub id: PhotoId,
    pub current: usize,
    pub rows: Vec<Row>,
}

/// The rows of a `photo.history` reply; `None` when it is not as documented. Rows that are not are
/// left out.
pub fn rows_of(reply: &Value) -> Option<Rows> {
    let id = PhotoId::new(reply.get("id")?.as_u64()?);
    let current = usize::try_from(reply.get("current")?.as_u64()?).ok()?;
    let rows = reply
        .get("steps")?
        .as_array()?
        .iter()
        .filter_map(|r| {
            let kind = match r.get("kind")?.as_str()? {
                "original" => Kind::Original,
                "save" => Kind::Save,
                "edit" => Kind::Edit,
                _ => return None,
            };
            Some(Row {
                index: usize::try_from(r.get("index")?.as_u64()?).ok()?,
                kind,
                label: r.get("label")?.as_str()?.to_owned(),
                detail: r.get("detail").and_then(Value::as_str).map(str::to_owned),
                op: r.get("op").and_then(Value::as_str).unwrap_or_default().to_owned(),
                redo: r.get("redo").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();
    Some(Rows { id, current, rows })
}

/// The photo the pane shows: the open photo in the edit view, else the one selected photo.
pub fn subject(app: &App) -> Option<PhotoId> {
    if let Some(id) = open_photo(app) {
        return Some(id);
    }
    match app.state().selection.ids.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The rows asked for last, and what for.
#[derive(Clone, Default)]
struct Cached {
    key: Option<(PhotoId, u64, &'static str)>,
    rows: Option<Arc<Rows>>,
}

/// The rows of photo `id`, asked of `photo.history` only when the library, the photo or the
/// language changed; `None` before the library is open or for a photo it does not know.
pub fn rows(app: &mut App, ctx: &egui::Context, id: PhotoId) -> Option<Arc<Rows>> {
    let cache = Id::new("history.rows");
    let revision = app.session()?.library().revision();
    let key = Some((id, revision, crate::i18n::current().code));
    let hit = ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cached>(cache);
        (c.key == key).then(|| c.rows.clone())
    });
    if let Some(rows) = hit {
        return rows;
    }
    let reply = app.session_mut()?.execute(LIST, &json!({ "id": id })).ok();
    let rows = reply.as_ref().and_then(rows_of).map(Arc::new);
    ctx.data_mut(|d| d.insert_temp(cache, Cached { key, rows: rows.clone() }));
    rows
}

/// What a row's glyph is: one of `ui-kit`'s sets, or the Save disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowGlyph {
    Glyph(Glyph),
    Save,
}

/// The glyph of `row`: the edit panel's own for its tools and fixes, the bottom bar's rotations,
/// the effects tab's brush for every other edit.
fn glyph(row: &Row) -> RowGlyph {
    let name = row.op.split('(').next().unwrap_or_default();
    let g: Glyph = match (row.kind, name) {
        (Kind::Save, _) => return RowGlyph::Save,
        (Kind::Original, _) => ViewingIcon::ShowOriginal.into(),
        (_, "crop") => EditIcon::Crop.into(),
        (_, "straighten") => EditIcon::Straighten.into(),
        (_, "retouch") => EditIcon::Retouch.into(),
        (_, "autofix") => EditIcon::AutoFix.into(),
        (_, "autocontrast") => EditIcon::AutoContrast.into(),
        (_, "autocolor") => EditIcon::AutoColor.into(),
        (_, "fill_light") => EditIcon::FillLight.into(),
        (_, "tuning") => EditIcon::Tuning.into(),
        (_, "rotate") if row.op == "rotate(270)" => Icon::RotateCcw.into(),
        (_, "rotate" | "flip_h" | "flip_v") => Icon::RotateCw.into(),
        _ => EditIcon::Effects.into(),
    };
    RowGlyph::Glyph(g)
}

/// A row's name in the language in effect.
fn label(row: &Row) -> String {
    match row.kind {
        Kind::Original => tl!(ctx = "history", "Original"),
        Kind::Save => tl!(ctx = "history", "Save"),
        Kind::Edit => super::edit::words::op_label(&row.label),
    }
}

/// A row's tooltip: what a click does.
fn tooltip(row: &Row, current: usize) -> String {
    if row.index == current {
        return match row.kind {
            Kind::Save => tl!("The saved file. Undo Save in the edit panel restores the original."),
            _ => tl!("The photo is here"),
        };
    }
    match row.kind {
        Kind::Original => tl!("Go back to the photo with no edits; every step stays redoable"),
        Kind::Save => tl!("Go back to the saved file; every step stays redoable"),
        Kind::Edit if row.index > current => tl!("Redo up to this step"),
        Kind::Edit => tl!("Go back to this step; the steps after it stay redoable"),
    }
}

/// `text` on one line in `font`, cut with "…" past `max` points.
fn line(painter: &Painter, text: &str, font: FontId, color: Color32, max: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(if max.is_finite() { max.max(1.0) } else { 1.0 });
    painter.layout_job(job)
}

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let area = ui.max_rect();
    let rows = subject(app).and_then(|id| rows(app, &ctx, id));
    let Some(rows) = rows else {
        let text = Rect::from_min_max(pos2(area.min.x + INSET, area.min.y + INSET), pos2(area.max.x - INSET, area.min.y + INSET + ROW));
        let galley = line(ui.painter(), &tl!("Select one photo to see its history"), fonts::regular(t.body), t.text_dim, text.width());
        ui.painter().galley(Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text).min.round(), galley, t.text_dim);
        return;
    };
    let width = area.width();
    let mut goto: Option<usize> = None;
    let body = Rect::from_min_max(pos2(area.min.x, area.min.y + INSET / 2.0), area.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    // The wheel scrolls the rows; the bar only shows where they are.
    let scroll = egui::ScrollArea::vertical()
        .id_salt(("history.list", rows.id.get()))
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL);
    scroll.show_rows(&mut child, ROW, rows.rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let clip = ui.clip_rect();
        for row in rows.rows.get(range).unwrap_or_default() {
            let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
            let r = ui.interact(rect, Id::new(("history.row", row.index)), Sense::click());
            let on = row.index == rows.current;
            let painter = ui.painter();
            if on {
                painter.rect_filled(rect, 0, t.tool_active);
                painter.rect_filled(Rect::from_min_size(rect.min, vec2(BAR, rect.height())), 0, t.accent);
            } else if ui.rect_contains_pointer(rect.intersect(clip)) {
                painter.rect_filled(rect, 0, t.hover);
            }
            let (text, glyph_color) = if row.redo {
                (t.text_dim, t.text_disabled)
            } else if on {
                (t.text, t.pick)
            } else {
                (t.text, t.icon)
            };
            let g = Rect::from_center_size(pos2(rect.min.x + INSET + BAR + GLYPH / 2.0, rect.center().y), Vec2::splat(GLYPH));
            match glyph(row) {
                RowGlyph::Glyph(glyph) => icons::paint_glyph(painter, g, glyph, glyph_color),
                RowGlyph::Save => icons::paint_output(painter, g, OutputIcon::Save, glyph_color),
            }
            let left = g.max.x + 8.0;
            let right = rect.max.x - INSET;
            let font = if on || row.kind == Kind::Save { fonts::semibold(t.body) } else { fonts::regular(t.body) };
            let name = label(row);
            let detail = row.detail.as_deref().map(|d| line(painter, d, fonts::regular(t.body), t.text_dim, (right - left) * 0.5));
            let detail_w = detail.as_ref().map_or(0.0, |d| d.size().x + 8.0);
            let galley = line(painter, &name, font, text, right - left - detail_w);
            let y = rect.center().y - galley.size().y / 2.0;
            painter.galley(pos2(left, y).round(), galley, text);
            if let Some(d) = detail {
                let at = pos2(right - d.size().x, rect.center().y - d.size().y / 2.0).round();
                painter.galley(at, d, if row.redo { t.text_disabled } else { t.text_dim });
            }
            if row.kind == Kind::Save {
                // The rule under the saved file: the edits below it are made on the saved pixels.
                painter.hline(rect.x_range(), rect.max.y - 0.5, egui::Stroke::new(1.0, t.divider));
            }
            let spoken = match &row.detail {
                Some(d) => format!("{name} {d}"),
                None => name,
            };
            widgets::register(&ctx, &row_id(row.index), Role::Other, &spoken, Target::Widget(&r), on);
            let r = r.on_hover_text(tooltip(row, rows.current));
            if r.clicked() && !on {
                goto = Some(row.index);
            }
        }
    });
    if let Some(index) = goto {
        // A refusal is on the status line.
        let _ = app.run(GOTO, &json!({ "id": rows.id, "index": index }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_come_from_the_reply_and_odd_entries_are_left_out() {
        let reply = json!({ "id": 7, "current": 1, "steps": [
            { "index": 0, "label": "Original", "kind": "original", "redo": false },
            { "index": 1, "label": "Crop", "detail": "4:3", "op": "crop(0,0,1,0.75)", "kind": "edit", "redo": false },
            { "index": 2, "label": "Sepia", "op": "sepia()", "kind": "edit", "redo": true },
            { "index": -1, "label": "x", "kind": "edit" },
            { "index": 4, "label": "y", "kind": "attic" },
            "z",
        ] });
        let rows = rows_of(&reply).unwrap();
        assert_eq!((rows.id, rows.current), (PhotoId::new(7), 1));
        let shown: Vec<(usize, &str, Option<&str>, bool)> =
            rows.rows.iter().map(|r| (r.index, r.label.as_str(), r.detail.as_deref(), r.redo)).collect();
        assert_eq!(shown, [(0, "Original", None, false), (1, "Crop", Some("4:3"), false), (2, "Sepia", None, true)]);
        assert_eq!(glyph(&rows.rows[1]), RowGlyph::Glyph(EditIcon::Crop.into()));
        assert_eq!(glyph(&rows.rows[2]), RowGlyph::Glyph(EditIcon::Effects.into()));
        assert_eq!(rows_of(&json!({ "id": 7 })), None);
        assert_eq!(rows_of(&json!({ "id": "7", "current": 0, "steps": [] })), None);
        assert_eq!(row_id(3), "history:3");
    }
}
