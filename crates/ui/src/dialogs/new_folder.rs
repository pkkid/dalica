//! The new-folder dialog (`organizing.md` §9): File ▸ Move to New Folder… and the photo menu's Move
//! to New Folder… (`photo.moveToNewFolder` without a name: the selection) and Split Folder Here…
//! (`photo.splitFolder`: the photo and every photo after it in its folder) open the folder
//! properties of a folder that does not exist yet, made beside the photos' folder. It is titled
//! "New Folder", not "Folder Properties" (the dialog of a folder that exists, `folder_properties`).
//!
//! ```text
//! New Folder
//! Name:          [New Folder                       ]   (selected, ready for typing)
//! Date:          [2026-06-04]  Thursday, June 4, 2026
//!                [x] Use the date of the oldest photo
//! Place:         [                                 ]
//! Description:   [                                 ]
//! 3 photos move into the new folder beside Lisbon.
//!                                          [Cancel] [OK]
//! ```
//!
//! Its draft is the view state's `newFolder` ([`NewFolderDraft`]): the photos, Name ("New
//! Folder"), the date the Date field shows (`"YYYY-MM-DD"`, the oldest photo's while "Use the date
//! of the oldest photo" is on, the field then disabled), Place and Description. OK (enabled while
//! the name is not blank, there are photos and, with the box off, the date reads as a day or is
//! empty) runs `photo.moveToNewFolder {ids, name, date?, useOldest, place, description}`
//! ([`run_params`]): one undo step that makes the folder, gives it this information and moves the
//! photos into it; Edit ▸ Undo moves them back and removes the folder. Cancel and Esc change
//! nothing.
//!
//! Widget ids (`control-protocol.md` §7, the Folder Properties ids): `dialog:newFolder`,
//! `field:dialog.name`, `field:dialog.date`, `check:dialog.useOldest`, `field:dialog.place`,
//! `field:dialog.description`, `button:dialog.cancel`, `button:dialog.ok`.

use dalica_core::{Date, PhotoId};
use dalica_engine::CmdError;
use dalica_library::Library;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target, TextField};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Id, Label, Layout, RichText, Ui, vec2};
use serde_json::{Map, Value, json};

use super::folder_properties::shown;
use super::{Button, Dialog, DialogKind, Look, form_label};
use crate::app::App;
use crate::i18n::format::long_date_with_weekday;
use crate::panels::light_table::folder_name;
use crate::state::NewFolderDraft;
use crate::{tl, tln};

/// The dialog's title (a folder still to be made) and width (points).
pub const TITLE: &str = "New Folder";
pub const WIDTH: f32 = 440.0;
/// "Use the date of the oldest photo".
pub const USE_OLDEST: &str = "Use the date of the oldest photo";
/// The label column's width, the gap between columns and rows, the Date field's width (points).
const LABEL_W: f32 = 96.0;
const GAP: f32 = 8.0;
const DATE_W: f32 = 112.0;
/// What the Date field says while empty.
pub const DATE_HINT: &str = "YYYY-MM-DD";
/// Why OK is disabled: a blank name, no photos, a date that is no day.
pub const NO_NAME: &str = "Name the folder.";
pub const NO_PHOTOS: &str = "Select some photos first.";
pub const BAD_DATE: &str = "Type the date as YYYY-MM-DD.";

pub const NAME_ID: &str = "field:dialog.name";
pub const DATE_ID: &str = "field:dialog.date";
pub const USE_OLDEST_ID: &str = "check:dialog.useOldest";
pub const PLACE_ID: &str = "field:dialog.place";
pub const DESCRIPTION_ID: &str = "field:dialog.description";

/// Opens the dialog with `draft`; while "Use the date of the oldest photo" is on and no date is
/// given, the Date field shows the oldest photo's. Replies `{dialog: "newFolder"}`.
pub fn open(app: &mut App, mut draft: NewFolderDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::NewFolder))?;
    if draft.use_oldest && draft.date.trim().is_empty() {
        let oldest = app.session().and_then(|s| oldest_date(s.library(), &draft.ids));
        draft.date = oldest.map(|d| d.to_string()).unwrap_or_default();
    }
    app.state_mut().new_folder = Some(draft.sanitized());
    Ok(reply)
}

/// The oldest effective date of `ids` (the new folder's date while the box is on). A pass over the
/// photos named: on opening and on a click, never per frame.
pub fn oldest_date(library: &Library, ids: &[PhotoId]) -> Option<Date> {
    ids.iter().filter_map(|&id| library.photo(id).and_then(|p| library.sort_date(p))).min().map(|d| d.date())
}

/// The date the draft gives: `Ok(None)` for the oldest photo's (the box on) or none typed,
/// `Ok(Some)` for a day typed, `Err` when the field does not read as one.
pub fn chosen_date(draft: &NewFolderDraft) -> Result<Option<Date>, &'static str> {
    let typed = draft.date.trim();
    if draft.use_oldest || typed.is_empty() {
        return Ok(None);
    }
    typed.parse::<Date>().map(Some).map_err(|_| BAD_DATE)
}

/// Why OK cannot run with `draft`, if it cannot: a blank name, no photos, a date that is no day.
pub fn ready(draft: &NewFolderDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        Err(NO_NAME.into())
    } else if draft.ids.is_empty() {
        Err(NO_PHOTOS.into())
    } else {
        chosen_date(draft).map(|_| ()).map_err(str::to_string)
    }
}

/// `photo.moveToNewFolder`'s parameters for `draft`; the date only when typed and not taken from
/// the oldest photo.
pub fn run_params(draft: &NewFolderDraft) -> Value {
    let mut p = Map::new();
    p.insert("ids".into(), json!(draft.ids));
    p.insert("name".into(), json!(draft.name.trim()));
    if !draft.use_oldest && !draft.date.trim().is_empty() {
        p.insert("date".into(), json!(draft.date.trim()));
    }
    p.insert("useOldest".into(), json!(draft.use_oldest));
    p.insert("place".into(), json!(draft.place));
    p.insert("description".into(), json!(draft.description));
    Value::Object(p)
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ok = app.state().new_folder.as_ref().is_some_and(|d| ready(d).is_ok());
    Look { title: tl!("New Folder"), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(ok)] }
}

/// One row: `label` in its column, then what `add` draws.
fn row<R>(ui: &mut Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.allocate_ui_with_layout(vec2(LABEL_W, t.metrics.text_field_h), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(LABEL_W);
            ui.label(RichText::new(label).font(fonts::regular(t.body)).color(t.text_label));
        });
        add(ui)
    })
    .inner
}

/// The long form of the date the Date row shows ("Thursday, June 4, 2026"), or why there is none.
fn date_note(draft: &NewFolderDraft) -> Result<String, &'static str> {
    chosen_date(draft)?;
    Ok(match draft.date.trim().parse::<Date>() {
        Ok(day) => long_date_with_weekday(day),
        Err(_) if draft.use_oldest => tl!("No photo here has a date."),
        Err(_) => String::new(),
    })
}

/// "3 photos move into the new folder beside Lisbon."
pub fn summary(count: usize, beside: Option<&str>) -> String {
    match beside {
        Some(folder) => tln!(
            count,
            "{n} photo moves into the new folder beside {folder}.",
            "{n} photos move into the new folder beside {folder}.",
            folder = folder
        ),
        None => tln!(count, "{n} photo moves into the new folder.", "{n} photos move into the new folder."),
    }
}

/// Whether this is the dialog's first or second pass since it opened: the Name field asks for the
/// focus then (the window takes no input on its very first pass, while egui sizes it).
fn opening(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    let seen = Id::new(NAME_ID).with("newFolder.passes");
    let pass = ctx.cumulative_pass_nr();
    let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
        Some((first, last)) if last.checked_add(1) == Some(pass) => first,
        _ => pass,
    };
    ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
    pass.saturating_sub(first) < 2
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(before) = app.state().new_folder.clone() else { return };
    let t = *app.tokens();
    let mut d = before.clone();
    let full = (ui.available_width() - LABEL_W - GAP).max(24.0);
    ui.spacing_mut().item_spacing.y = GAP;
    if opening(ui) {
        widgets::focus_field(ui.ctx(), NAME_ID);
    }
    row(ui, &t, &form_label(&tl!("Name")), |ui| TextField::new(NAME_ID, &tl!("Folder name"), full).select_on_focus(true).show(ui, &t, &mut d.name));
    row(ui, &t, &form_label(&tl!("Date")), |ui| {
        ui.add_enabled_ui(!d.use_oldest, |ui| TextField::new(DATE_ID, &tl!(ctx = "date", "YYYY-MM-DD"), DATE_W).show(ui, &t, &mut d.date));
        let (note, color) = match date_note(&d) {
            Ok(note) => (note, t.text_dim),
            Err(why) => (shown(why), t.caution),
        };
        ui.label(RichText::new(note).font(fonts::regular(t.detail)).color(color));
    });
    row(ui, &t, "", |ui| {
        let mut on = d.use_oldest;
        let label = tl!("Use the date of the oldest photo");
        let r = ui.checkbox(&mut on, RichText::new(&label).font(fonts::regular(t.body)));
        widgets::register(ui.ctx(), USE_OLDEST_ID, Role::Toggle, &label, Target::Widget(&r), on);
        if on != d.use_oldest {
            d.use_oldest = on;
            if on {
                // Back to the oldest photo's date: a pass over the photos moved, on this click.
                let date = app.session().and_then(|s| oldest_date(s.library(), &d.ids));
                d.date = date.map(|d| d.to_string()).unwrap_or_default();
            }
        }
    });
    row(ui, &t, &form_label(&tl!("Place")), |ui| TextField::new(PLACE_ID, &tl!("Where these photos were taken"), full).show(ui, &t, &mut d.place));
    row(ui, &t, &form_label(&tl!("Description")), |ui| {
        TextField::new(DESCRIPTION_ID, &tl!("Add a description"), full).show(ui, &t, &mut d.description)
    });
    let library = app.session().map(dalica_engine::Session::library);
    let beside = d.ids.first().and_then(|id| library.and_then(|l| l.photo(*id)).and_then(|p| library.and_then(|l| l.folder(p.folder))));
    let text = summary(d.ids.len(), beside.map(|f| folder_name(&f.path)).as_deref());
    ui.add(Label::new(RichText::new(text).font(fonts::regular(t.detail)).color(t.text_dim)).wrap());
    if d != before {
        app.state_mut().new_folder = Some(d.sanitized());
    }
}

/// OK: moves the photos into the new folder.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().new_folder.clone().ok_or_else(|| CmdError::Internal("the new-folder dialog has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    app.run("photo.moveToNewFolder", &run_params(&draft))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;
    use crate::dialogs;

    #[test]
    fn ok_moves_the_photos_into_a_new_folder_with_its_information() {
        let d = NewFolderDraft { ids: vec![PhotoId::new(3)], ..NewFolderDraft::default() };
        assert_eq!(run_params(&d), json!({ "ids": [3], "name": "New Folder", "useOldest": true, "place": "", "description": "" }));
        let dated = NewFolderDraft { date: "2026-06-01".into(), use_oldest: false, ..d.clone() };
        assert_eq!(run_params(&dated)["date"], "2026-06-01");
        assert_eq!(ready(&NewFolderDraft::default()), Err(NO_PHOTOS.into()));
        assert_eq!(ready(&NewFolderDraft { name: "  ".into(), ..d.clone() }), Err(NO_NAME.into()));
        let bad = NewFolderDraft { date: "June 1".into(), use_oldest: false, ..d.clone() };
        assert_eq!(ready(&bad), Err(BAD_DATE.into()));
        assert_eq!(ready(&NewFolderDraft { use_oldest: true, ..bad }), Ok(()), "the box on: the field is not read");
        assert_eq!(ready(&NewFolderDraft { date: String::new(), use_oldest: false, ..d.clone() }), Ok(()), "no date at all");
        assert_eq!(summary(1, Some("Lisbon")), "1 photo moves into the new folder beside Lisbon.");
        assert_eq!(summary(3, None), "3 photos move into the new folder.");
        let mut app = App::new(None);
        open(&mut app, d.clone()).unwrap();
        assert_eq!(app.state().new_folder, Some(d));
        assert_eq!(dialogs::inspect(&app)["title"], TITLE);
        // No library: OK fails and the dialog stays.
        assert!(dialogs::answer(&mut app, None).is_err());
        assert_eq!(app.state().dialogs.len(), 1);
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().new_folder, None);
    }
}
