//! The destination strip (`organizing.md` §3 items 4–6): the bottom of the Import room, the whole
//! width, under the tray and the preview.
//!
//! - **The first row**: "Import to:" ([`PLACE_ID`]: Options ▸ General's "Save imported pictures
//!   in", the recent destinations, then Choose…, the folder picker); "Folder name:" (a field for any
//!   name, [`NAME_FIELD_ID`], and beside it the choices [`NAME_ID`]: Date taken (YYYY-MM-DD), one
//!   folder per capture date; Today's date; the recent names); "After copying:" ([`AFTER_ID`]:
//!   Leave files on the card, Delete copied files from the card), shown only for a card or a drive.
//!   Each choice drafts the view state through `import.destination`; nothing is applied before an
//!   import button.
//! - **The failed files** of an import that ended with failures, one line each, between the rows
//!   (`super::progress`).
//! - **The second row**: the hint line with its light bulb ([`HINT_ID`]: "Choose a folder name to
//!   continue", then what the name chosen does) or, while an import copies, its progress (and after
//!   one cancelled or failed, its result); at the right **Import All** (primary), **Import Selected
//!   (n)** and **Cancel** (`import.importAll`, `import.importSelected`, `import.cancel`), the two
//!   import buttons disabled until a folder name is chosen, while the source is still listed and
//!   while an import runs.
//!
//! The light bulb is drawn here from two strokes and a circle in the `icon` colour (no glyph set
//! has one yet). The strip reads the settings' recent destinations and names in memory, never the
//! disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dalica_engine::import::SourceKind;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, Id, Layout, Painter, Rect, Response, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::progress::{self, Shown};
use super::{Action, Snapshot};
use crate::app::App;
use crate::state::{AfterCopy, FolderName};

/// The strip's automation id (its whole rect).
pub const ID: &str = "importDestination";
/// A row's height.
pub const ROW_H: f32 = 44.0;
/// A failed file's line height.
pub const LINE_H: f32 = 18.0;
/// The space at the strip's left and right.
pub const PAD: f32 = 12.0;
/// The space between a label and its control, and between the groups.
pub const LABEL_GAP: f32 = 6.0;
pub const GROUP_GAP: f32 = 20.0;
/// The space between the buttons.
pub const BUTTON_GAP: f32 = 8.0;
/// The folder name field's width.
pub const NAME_W: f32 = 200.0;

/// "Import to:" and its dropdown; its items are `<id>/default`, `<id>/<n>` (the recent
/// destinations, from 0) and `<id>/choose`.
pub const PLACE_LABEL: &str = "Import to:";
pub const PLACE_ID: &str = "choice:import.destination";
/// "Folder name:", its field, and the choices beside it: `<id>/dateTaken`, `<id>/today`,
/// `<id>/<n>` (the recent names, from 0).
pub const NAME_LABEL: &str = "Folder name:";
pub const NAME_FIELD_ID: &str = "field:import.folderName";
pub const NAME_ID: &str = "choice:import.folderName";
pub const NAME_HINT: &str = "Folder name";
/// "After copying:" and its dropdown: `<id>/leave`, `<id>/delete`.
pub const AFTER_LABEL: &str = "After copying:";
pub const AFTER_ID: &str = "choice:import.after";
pub const LEAVE: &str = "Leave files on the card";
pub const DELETE: &str = "Delete copied files from the card";
/// The folder name choices.
pub const DATE_TAKEN: &str = "Date taken (YYYY-MM-DD)";
pub const TODAY: &str = "Today's date";
/// Choose….
pub const CHOOSE: &str = "Choose\u{2026}";
/// "Import to:" with no folder known.
pub const NO_PLACE: &str = "Choose a folder";
/// The hint line (labelled with its text).
pub const HINT_ID: &str = "text:import.hint";
/// The buttons.
pub const IMPORT_ALL_ID: &str = "button:import.importAll";
pub const IMPORT_SELECTED_ID: &str = "button:import.importSelected";
pub const CANCEL_ID: &str = "button:import.cancel";
pub const IMPORT_ALL: &str = "Import All";
pub const CANCEL: &str = "Cancel";

/// The strip's height with `lines` failed files listed.
pub fn height(lines: usize) -> f32 {
    2.0 * ROW_H + lines as f32 * LINE_H
}

/// The hint for folder name `name`.
pub fn hint(name: Option<&FolderName>) -> &'static str {
    match name {
        None => crate::commands::import::CHOOSE_NAME,
        Some(FolderName::Text(_)) => "Click Import All or Import Selected",
        Some(FolderName::DateTaken) => "Each capture date gets its own folder",
        Some(FolderName::Today) => "Pictures will go into a folder named with today's date",
    }
}

/// What the folder name field shows for `name`.
pub fn name_text(name: Option<&FolderName>) -> &str {
    match name {
        None => "",
        Some(FolderName::Text(t)) => t,
        Some(FolderName::DateTaken) => DATE_TAKEN,
        Some(FolderName::Today) => TODAY,
    }
}

/// "Import Selected (3)".
pub fn import_selected(n: usize) -> String {
    format!("Import Selected ({n})")
}

/// A folder's name as the dropdown shows it (the whole path when it has none).
fn shown_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// What the folder name field keeps between frames: its text, and the draft it was last given.
#[derive(Debug, Clone, Default)]
struct NameField {
    text: String,
    drafted: String,
}

/// Draws the strip into `ui`'s rect.
pub fn show(app: &App, ui: &mut Ui, t: &Tokens, snap: &Snapshot, shown: &Shown, actions: &mut Vec<Action>) {
    let Some(session) = app.session() else { return };
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(rect, 0, t.chrome);
    p.hline(rect.x_range(), rect.min.y + 0.5, Stroke::new(1.0, t.divider));
    widgets::register(ui.ctx(), ID, Role::Other, "Destination", Target::Painted { rect, clip: ui.clip_rect() }, false);
    let row1 = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + ROW_H));
    let row2 = Rect::from_min_max(pos2(rect.min.x, rect.max.y - ROW_H), rect.max);
    let draft = &app.state().import.destination;
    let settings = session.settings().import();
    let copying = matches!(shown, Shown::Copying { .. });
    let font = fonts::regular(t.body);

    // "Import to:".
    let mut x = rect.min.x + PAD;
    x = widgets::label_in(&p, Rect::from_min_max(pos2(x, row1.min.y), row1.max), Align2::LEFT_CENTER, PLACE_LABEL, font.clone(), t.text_label).max.x
        + LABEL_GAP;
    let default = session.import_folder();
    let place = draft.place.as_ref().map(PathBuf::from).or_else(|| default.clone());
    let text = place.as_deref().map_or_else(|| NO_PLACE.to_string(), shown_name);
    let r = slot(ui, x, row1, !copying, |ui| {
        widgets::dropdown(ui, t, PLACE_ID, &text, |ui| {
            if let Some(d) = &default
                && widgets::dropdown_item(ui, t, &format!("{PLACE_ID}/default"), &shown_name(d), &d.display().to_string(), draft.place.is_none())
                    .clicked()
            {
                actions.push(("import.destination", json!({ "place": Value::Null })));
            }
            for (n, recent) in settings.recent_destinations.iter().enumerate() {
                let checked = draft.place.as_deref().is_some_and(|p| Path::new(p) == recent.as_path());
                if widgets::dropdown_item(ui, t, &format!("{PLACE_ID}/{n}"), &shown_name(recent), &recent.display().to_string(), checked).clicked() {
                    actions.push(("import.destination", json!({ "place": recent })));
                }
            }
            ui.separator();
            if widgets::dropdown_item(ui, t, &format!("{PLACE_ID}/choose"), CHOOSE, "Choose another folder", false).clicked() {
                actions.push(("import.chooseDestination", json!({})));
            }
        })
    });
    x = r.rect.max.x + GROUP_GAP;

    // "Folder name:": the field and its choices.
    x = widgets::label_in(&p, Rect::from_min_max(pos2(x, row1.min.y), row1.max), Align2::LEFT_CENTER, NAME_LABEL, font.clone(), t.text_label).max.x
        + LABEL_GAP;
    let ctx = ui.ctx().clone();
    let key = Id::new("import.folderName.field");
    let mut field: NameField = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let drafted = name_text(draft.folder_name.as_ref()).to_string();
    if field.drafted != drafted {
        field.text = drafted.clone();
        field.drafted = drafted;
    }
    let before = field.text.clone();
    let r = slot(ui, x, row1, !copying, |ui| widgets::text_field(ui, t, NAME_FIELD_ID, &mut field.text, NAME_HINT, NAME_W).response);
    if field.text != before {
        let name = if field.text.trim().is_empty() { Value::Null } else { Value::String(field.text.clone()) };
        field.drafted = if name.is_null() { String::new() } else { field.text.clone() };
        actions.push(("import.destination", json!({ "folderName": name })));
    }
    ctx.data_mut(|d| d.insert_temp(key, field));
    let r = slot(ui, r.rect.max.x + 2.0, row1, !copying, |ui| {
        widgets::dropdown(ui, t, NAME_ID, "", |ui| {
            let name = draft.folder_name.as_ref();
            if widgets::dropdown_item(
                ui,
                t,
                &format!("{NAME_ID}/dateTaken"),
                DATE_TAKEN,
                "One folder for each capture date",
                name == Some(&FolderName::DateTaken),
            )
            .clicked()
            {
                actions.push(("import.destination", json!({ "folderName": { "dateTaken": true } })));
            }
            if widgets::dropdown_item(
                ui,
                t,
                &format!("{NAME_ID}/today"),
                TODAY,
                "One folder named with today's date",
                name == Some(&FolderName::Today),
            )
            .clicked()
            {
                actions.push(("import.destination", json!({ "folderName": { "today": true } })));
            }
            if !settings.recent_folder_names.is_empty() {
                ui.separator();
            }
            for (n, recent) in settings.recent_folder_names.iter().enumerate() {
                let checked = matches!(name, Some(FolderName::Text(t)) if t == recent);
                if widgets::dropdown_item(ui, t, &format!("{NAME_ID}/{n}"), recent, "Recent folder name", checked).clicked() {
                    actions.push(("import.destination", json!({ "folderName": recent })));
                }
            }
        })
    });
    x = r.rect.max.x + GROUP_GAP;

    // "After copying:", for a card or a drive.
    if snap.source.as_ref().is_some_and(|s| s.kind != SourceKind::Folder) {
        x = widgets::label_in(&p, Rect::from_min_max(pos2(x, row1.min.y), row1.max), Align2::LEFT_CENTER, AFTER_LABEL, font, t.text_label).max.x
            + LABEL_GAP;
        let text = match draft.after {
            AfterCopy::Leave => LEAVE,
            AfterCopy::Delete => DELETE,
        };
        slot(ui, x, row1, !copying, |ui| {
            widgets::dropdown(ui, t, AFTER_ID, text, |ui| {
                if widgets::dropdown_item(ui, t, &format!("{AFTER_ID}/leave"), LEAVE, "", draft.after == AfterCopy::Leave).clicked() {
                    actions.push(("import.destination", json!({ "after": "leave" })));
                }
                if widgets::dropdown_item(ui, t, &format!("{AFTER_ID}/delete"), DELETE, "", draft.after == AfterCopy::Delete).clicked() {
                    actions.push(("import.destination", json!({ "after": "delete" })));
                }
            })
        });
    }

    // The failed files.
    let lines = Rect::from_min_max(pos2(rect.min.x + PAD, row1.max.y), pos2(rect.max.x - PAD, row2.min.y));
    progress::show_failures(ui, t, lines, shown, LINE_H);

    // The buttons, right to left.
    let listed: HashSet<u64> = snap.candidates.iter().map(|c| c.id).collect();
    let selected = app.state().import.selection.iter().filter(|id| listed.contains(id)).count();
    let can = |id: &str| !snap.loading && crate::commands::enabled(app, id, &json!({})).is_ok();
    let buttons = row2.shrink2(vec2(PAD, 0.0));
    let left = ui
        .scope_builder(UiBuilder::new().max_rect(buttons).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = BUTTON_GAP;
            if widgets::text_button(ui, t, CANCEL_ID, CANCEL, false).clicked() {
                actions.push(("import.cancel", json!({})));
            }
            let label = import_selected(selected);
            if ui.add_enabled_ui(can("import.importSelected"), |ui| widgets::text_button(ui, t, IMPORT_SELECTED_ID, &label, false)).inner.clicked() {
                actions.push(("import.importSelected", json!({})));
            }
            let all = ui.add_enabled_ui(can("import.importAll"), |ui| widgets::primary_button(ui, t, IMPORT_ALL_ID, IMPORT_ALL)).inner;
            if all.clicked() {
                actions.push(("import.importAll", json!({})));
            }
            all.rect.min.x
        })
        .inner;

    // The hint line, or the import's progress or result.
    let line = Rect::from_min_max(pos2(rect.min.x + PAD, row2.min.y), pos2((left - GROUP_GAP).max(rect.min.x + PAD), row2.max.y));
    match shown {
        Shown::Idle => hint_line(ui, t, line, hint(draft.folder_name.as_ref())),
        _ => progress::show_line(ui, t, line, shown),
    }
}

/// Runs `add` in a child of `ui` from `x` to the end of `row`, laid out left to right and centred,
/// enabled when `enabled`; returns what it returns.
fn slot(ui: &mut Ui, x: f32, row: Rect, enabled: bool, add: impl FnOnce(&mut Ui) -> Response) -> Response {
    let rect = Rect::from_min_max(pos2(x.min(row.max.x), row.min.y), row.max);
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)), |ui| ui.add_enabled_ui(enabled, add).inner).inner
}

/// The hint line in `rect`: the light bulb, then `text` in `text_dim`.
fn hint_line(ui: &mut Ui, t: &Tokens, rect: Rect, text: &str) {
    let p = ui.painter().clone();
    let bulb = Rect::from_center_size(pos2(rect.min.x + 8.0, rect.center().y), vec2(16.0, 16.0));
    light_bulb(&p, bulb, t.icon);
    let room = Rect::from_min_max(pos2(bulb.max.x + 6.0, rect.min.y), rect.max);
    let g = super::one_line(&p, text, fonts::regular(t.body), t.text_dim, room.width());
    let at = pos2(room.min.x, room.center().y - g.size().y / 2.0).round();
    let drawn = Rect::from_min_size(at, g.size());
    p.galley(at, g, t.text_dim);
    widgets::register(ui.ctx(), HINT_ID, Role::Other, text, Target::Painted { rect: drawn, clip: ui.clip_rect() }, false);
}

/// A light bulb in `rect` (16 × 16): the glass as a circle, its neck and base as two strokes.
fn light_bulb(p: &Painter, rect: Rect, color: egui::Color32) {
    let s = rect.width() / 16.0;
    let c = rect.center();
    let stroke = Stroke::new(1.3 * s, color);
    p.circle_stroke(pos2(c.x, c.y - 2.0 * s), 4.5 * s, stroke);
    p.line_segment([pos2(c.x - 2.2 * s, c.y + 4.0 * s), pos2(c.x + 2.2 * s, c.y + 4.0 * s)], stroke);
    p.line_segment([pos2(c.x - 1.4 * s, c.y + 6.0 * s), pos2(c.x + 1.4 * s, c.y + 6.0 * s)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hint_and_the_field_follow_the_folder_name() {
        assert_eq!(hint(None), "Choose a folder name to continue");
        assert_eq!(hint(Some(&FolderName::Text("Lisbon".into()))), "Click Import All or Import Selected");
        assert_eq!(hint(Some(&FolderName::DateTaken)), "Each capture date gets its own folder");
        assert_eq!(hint(Some(&FolderName::Today)), "Pictures will go into a folder named with today's date");
        assert_eq!(name_text(Some(&FolderName::DateTaken)), DATE_TAKEN);
        assert_eq!(name_text(None), "");
        assert_eq!(import_selected(3), "Import Selected (3)");
        assert_eq!(height(0), 2.0 * ROW_H);
        assert_eq!(height(2), 2.0 * ROW_H + 2.0 * LINE_H);
    }
}
