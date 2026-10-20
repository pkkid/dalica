//! Folder Properties (`organizing.md` §10): opened by `section.properties` (Folder ▸ Edit
//! Description…, a double-click on a tree row or a section title).
//!
//! ```text
//! Folder Properties
//! Name:                  [Lisbon Weekend                 ]
//! Date:                  [2026-06-04]  Thursday, June 4, 2026
//!                        [x] Use the date of the oldest photo
//! Place:                 [Lisbon, Portugal               ]
//! Description:           [A weekend of trams             ]
//! Music for slideshows:  [                     ] [Browse…]  (disabled: not kept per folder yet)
//! Photo data:            Kept in Dalica's file in this folder.
//!                                                   [Cancel] [OK]
//! ```
//!
//! **Photo data** (`data-model.md` §9) says which file holds the folder's data, from the folder's
//! source as `legacy.inspect` gives it ([`source_text`], `label:dialog.source`): Dalica's own
//! file, the previous organizer's (read, never changed; Dalica's is written on the first change),
//! both (Dalica's in use, the old one no longer read), or none yet. It is worked out when the
//! library or the folder's fact changes, never per frame.
//!
//! The draft is `{folder, name, date, useOldest, place, description}`: the folder's id and name,
//! the date the Date field shows (`"YYYY-MM-DD"`; `null` when the folder has none: no photo with a
//! date and none set), whether "Use the date of the oldest photo" is on, its place and its
//! description. It starts from what the library holds: the date set in Folder Properties (the box
//! off), else the oldest photo's (the box on, the field disabled). Turning the box on shows the
//! oldest photo's date again; turning it off keeps the date shown (today's when the folder has
//! none). The Date field is `ui-kit`'s date field (M10.25): typed as `YYYY-MM-DD`, ↑ and ↓ step the
//! year, month or day the cursor is in, and a day that does not exist (`2026-02-30`) is refused,
//! never stored: the draft keeps the last day that read and the field shows it again when the edit
//! ends. OK is disabled while the draft's date does not read as a day (only a script setting the
//! draft through `ui.set` can leave it so). The name renames the folder on disk (M9.20): OK is
//! disabled while it is blank.
//!
//! OK runs `folder.rename {folder, name}` first when the name changed (a file job, one undo step
//! "Rename Folder"; the folder keeps its id, so the tree row and the section title follow it once
//! the job is done), then `folder.info.set` once (one undo step, none when nothing changed), with
//! `date: "oldest"` while the box is on. A rename the engine refuses (a name another library folder
//! has, an offline folder) leaves the dialog open with the reason and changes nothing; Cancel
//! changes nothing.
//!
//! Widget ids (`control-protocol.md` §7, the dialog row): `field:dialog.name`,
//! `field:dialog.date`, `check:dialog.useOldest`, `field:dialog.place`, `field:dialog.description`,
//! `field:dialog.music` and `button:dialog.browse` (both disabled with [`music_reason`]: Dalica
//! plays slideshow music since M15.27, but a folder's own music needs a field of the folder's data
//! that is not kept yet, so Options ▸ Slideshow's music folder plays for every folder),
//! `button:dialog.cancel`,
//! `button:dialog.ok`.

use dalica_core::{Date, DateTime, FolderId};
use dalica_engine::CmdError;
use dalica_library::Library;
use dalica_ui_kit::widgets::{self, ButtonKind, DATE_WIDTH, PickedDate, TextField, checkbox, date_field};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Layout, RichText, Ui, vec2};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look, form_label};
use crate::app::App;
use crate::i18n::format::long_date_with_weekday;
use crate::panels::light_table::folder_name;
use crate::tl;

pub const TITLE: &str = "Folder Properties";
/// The dialog's width (points).
pub const WIDTH: f32 = 500.0;
/// The label column's width (points).
const LABEL_W: f32 = 148.0;
/// Between the label column and the fields, and between rows.
const GAP: f32 = 8.0;
/// The checkbox's label.
pub const USE_OLDEST: &str = "Use the date of the oldest photo";
/// What the Music field says while it is empty.
pub const MUSIC_HINT: &str = "No folder";
/// Why the Music row is greyed: a folder's own music is not kept yet (M15.27).
pub const FOLDER_MUSIC_LATER: &str = "A folder's own music isn't kept yet: the music folder in Options › Slideshow plays";

/// Why the Music row is greyed ([`FOLDER_MUSIC_LATER`]).
pub fn music_reason() -> String {
    tl!("A folder's own music isn't kept yet: the music folder in Options › Slideshow plays")
}
/// Why OK is disabled when the draft's date does not read as a day.
pub const BAD_DATE: &str = "Type the date as YYYY-MM-DD.";
/// Why OK is disabled while the Name field is blank.
pub const NO_NAME: &str = "Enter the folder's name.";
/// What the Name field says while it is empty.
pub const NAME_HINT: &str = "Folder name";

pub const NAME_ID: &str = "field:dialog.name";
pub const DATE_ID: &str = "field:dialog.date";
pub const USE_OLDEST_ID: &str = "check:dialog.useOldest";
pub const PLACE_ID: &str = "field:dialog.place";
pub const DESCRIPTION_ID: &str = "field:dialog.description";
pub const MUSIC_ID: &str = "field:dialog.music";
pub const BROWSE_ID: &str = "button:dialog.browse";
pub const SOURCE_ID: &str = "label:dialog.source";

/// What the Photo data row says for a folder's source (`legacy.inspect`'s `dalica`, `legacy`,
/// `both` or `none`).
pub fn source_text(source: &str) -> String {
    match source {
        "dalica" => tl!("Kept in Dalica's file in this folder."),
        "legacy" => tl!("Read from the previous organizer's file, which is never changed. Dalica writes its own file on the first change."),
        "both" => tl!("Kept in Dalica's file. The previous organizer's file is also here; it is no longer read and never changed."),
        _ => tl!("None saved for this folder yet."),
    }
}

/// A reason of [`chosen_date`] or [`chosen_name`] (English, which a refused OK answers with) in
/// the language in effect; New Folder and Album Properties show theirs through it too.
pub fn shown(why: &str) -> String {
    match why {
        "Type the date as YYYY-MM-DD." => tl!("Type the date as YYYY-MM-DD."),
        "Enter the folder's name." => tl!("Enter the folder's name."),
        "Name the album." => tl!("Name the album."),
        "Name the folder." => tl!("Name the folder."),
        other => crate::i18n::tr(other),
    }
}

/// Folder `folder`'s source (`legacy.inspect`'s word), kept in the context's data with what it
/// was worked out from (the library's revision and the folder's fact, which moves no revision):
/// one pass over the library when either changes, none per frame.
fn source(app: &App, ctx: &egui::Context, folder: FolderId) -> &'static str {
    let Some(lib) = app.session().map(dalica_engine::Session::library) else { return "none" };
    let key = (folder, lib.revision(), dalica_engine::cmd::legacy::source_name(lib.folder_sidecar(folder)));
    let id = egui::Id::new(SOURCE_ID);
    if let Some((k, s)) = ctx.data(|d| d.get_temp::<((FolderId, u64, &'static str), &'static str)>(id))
        && k == key
    {
        return s;
    }
    let s = dalica_engine::cmd::legacy::Sources::new(lib).of(folder);
    ctx.data_mut(|d| d.insert_temp(id, (key, s)));
    s
}

/// The dialog for `folder`, its draft filled from what the library holds.
pub fn dialog(app: &App, folder: FolderId) -> Dialog {
    let mut d = Dialog::new(DialogKind::FolderProperties);
    d.fields = match app.session().map(dalica_engine::Session::library) {
        Some(library) => {
            let name = library.folder(folder).map(|f| folder_name(&f.path)).unwrap_or_default();
            let info = library.folder_info(folder);
            let date = info.date.or_else(|| oldest_date(library, folder));
            draft(folder, &name, date, info.date.is_none(), &info.place, &info.description)
        }
        None => draft(folder, "", None, true, "", ""),
    };
    d
}

/// The date of `folder`'s oldest photo (by effective date, else the file's time): the folder's date
/// while none is set. A pass over the library: on opening and on a click, never per frame.
pub fn oldest_date(library: &Library, folder: FolderId) -> Option<Date> {
    library.photos().filter(|p| p.folder == folder).filter_map(|p| library.sort_date(p)).min().map(|d| d.date())
}

/// A draft for `folder` called `name`, showing `date`.
pub fn draft(folder: FolderId, name: &str, date: Option<Date>, use_oldest: bool, place: &str, description: &str) -> Map<String, Value> {
    let v = json!({
        "folder": folder,
        "name": name,
        "date": date.map(|d| d.to_string()),
        "useOldest": use_oldest,
        "place": place,
        "description": description,
    });
    match v {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

fn text(fields: &Map<String, Value>, key: &str) -> String {
    fields.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn use_oldest(fields: &Map<String, Value>) -> bool {
    fields.get("useOldest").and_then(Value::as_bool).unwrap_or(true)
}

/// The date the draft gives: `Ok(None)` for the oldest photo's, `Ok(Some)` for a day typed, `Err`
/// (why, for the dialog) when the field does not read as one.
pub fn chosen_date(fields: &Map<String, Value>) -> Result<Option<Date>, &'static str> {
    if use_oldest(fields) {
        return Ok(None);
    }
    text(fields, "date").trim().parse::<Date>().map(Some).map_err(|_| BAD_DATE)
}

/// The name the draft gives, trimmed: `Err` (why, for the dialog) while it is blank.
pub fn chosen_name(fields: &Map<String, Value>) -> Result<String, &'static str> {
    let name = text(fields, "name");
    let name = name.trim();
    if name.is_empty() { Err(NO_NAME) } else { Ok(name.to_string()) }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(chosen_date(&d.fields).is_ok() && chosen_name(&d.fields).is_ok());
    Look { title: tl!("Folder Properties"), width: WIDTH, buttons: vec![Button::cancel(), ok] }
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

/// A text field for draft field `key`, `width` wide; the draft takes what is typed.
fn field(ui: &mut Ui, t: &Tokens, fields: &mut Map<String, Value>, key: &str, id: &str, hint: &str, width: f32) {
    let mut value = text(fields, key);
    let before = value.clone();
    TextField::new(id, hint, width).show(ui, t, &mut value);
    if value != before {
        fields.insert(key.into(), Value::String(value));
    }
}

/// The day `text` names, as the date field takes it.
fn picked(text: &str) -> Option<PickedDate> {
    let day = text.trim().parse::<Date>().ok()?;
    PickedDate::new(day.year(), day.month(), day.day())
}

/// The Date field (`ui-kit`'s date field) on draft field `date`: a day typed or stepped goes into
/// the draft; one that does not exist never does. With no day in the draft it shows `today` (else
/// 2000-01-01) and writes nothing until it is edited.
fn date_row_field(ui: &mut Ui, t: &Tokens, fields: &mut Map<String, Value>, today: Option<Date>) {
    let shown = picked(&text(fields, "date"));
    let fallback = today.and_then(|d| PickedDate::new(d.year(), d.month(), d.day())).or_else(|| PickedDate::new(2000, 1, 1));
    let Some(mut day) = shown.or(fallback) else { return };
    let out = date_field(ui, t, DATE_ID, &mut day, DATE_WIDTH);
    if out.changed
        && let Ok(day) = Date::new(day.year(), day.month(), day.day())
    {
        fields.insert("date".into(), Value::String(day.to_string()));
    }
}

/// The long form of the date the Date row shows ("Thursday, June 4, 2026"), or why there is none.
fn date_note(fields: &Map<String, Value>) -> Result<String, &'static str> {
    match chosen_date(fields)? {
        Some(day) => Ok(long_date_with_weekday(day)),
        None => match text(fields, "date").parse::<Date>() {
            Ok(day) => Ok(long_date_with_weekday(day)),
            Err(_) => Ok(tl!("No photo here has a date.")),
        },
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let full = (ui.available_width() - LABEL_W - GAP).max(24.0);
    ui.spacing_mut().item_spacing.y = GAP;
    let fields = &mut d.fields;

    row(ui, &t, &form_label(&tl!("Name")), |ui| field(ui, &t, fields, "name", NAME_ID, &tl!("Folder name"), full));

    let oldest = use_oldest(fields);
    let today = app.session().and_then(|s| DateTime::from_unix_secs(s.clock().now().as_secs())).map(|d| d.date());
    row(ui, &t, &form_label(&tl!("Date")), |ui| {
        ui.add_enabled_ui(!oldest, |ui| date_row_field(ui, &t, fields, today));
        let (note, color) = match date_note(fields) {
            Ok(note) => (note, t.text_dim),
            Err(why) => (shown(why), t.caution),
        };
        ui.label(RichText::new(note).font(fonts::regular(t.detail)).color(color));
    });
    row(ui, &t, "", |ui| {
        let mut on = oldest;
        checkbox(ui, &t, USE_OLDEST_ID, &tl!("Use the date of the oldest photo"), &mut on);
        if on != oldest {
            fields.insert("useOldest".into(), Value::Bool(on));
            if on {
                // Back to the oldest photo's date: one pass over the library, on this click only.
                let folder = fields.get("folder").and_then(|v| serde_json::from_value::<FolderId>(v.clone()).ok());
                let date = folder.and_then(|f| app.session().and_then(|s| oldest_date(s.library(), f)));
                fields.insert("date".into(), json!(date.map(|d| d.to_string())));
            } else if text(fields, "date").parse::<Date>().is_err() {
                // A folder with no date: the field starts from today.
                fields.insert("date".into(), json!(today.map(|d| d.to_string())));
            }
        }
    });
    row(ui, &t, &form_label(&tl!("Place")), |ui| field(ui, &t, fields, "place", PLACE_ID, &tl!("Where these photos were taken"), full));
    row(ui, &t, &form_label(&tl!("Description")), |ui| field(ui, &t, fields, "description", DESCRIPTION_ID, &tl!("Add a description"), full));
    row(ui, &t, &form_label(&tl!("Music for slideshows")), |ui| {
        // A folder's own music is not kept yet: the row is there, disabled with its reason.
        ui.add_enabled_ui(false, |ui| {
            let browse = tl!("Browse…");
            let bw = (ui.painter().layout_no_wrap(browse.clone(), fonts::semibold(t.button_label), t.text).size().x + 18.0).max(37.0).round();
            let mut music = String::new();
            let field = TextField::new(MUSIC_ID, &tl!("No folder"), (full - bw - GAP).max(24.0)).show(ui, &t, &mut music);
            let browse = widgets::text_button(ui, &t, BROWSE_ID, &browse, false);
            widgets::tooltip(field.response, &t, &music_reason(), None);
            widgets::tooltip(browse, &t, &music_reason(), None);
        });
    });
    let folder = fields.get("folder").and_then(|v| serde_json::from_value::<FolderId>(v.clone()).ok());
    if let Some(folder) = folder {
        let said = source_text(source(app, ui.ctx(), folder));
        row(ui, &t, &form_label(&tl!("Photo data")), |ui| {
            let r = ui.add(egui::Label::new(RichText::new(&said).font(fonts::regular(t.detail)).color(t.text_dim)).wrap());
            widgets::register(ui.ctx(), SOURCE_ID, dalica_ui_kit::registry::Role::Other, &said, widgets::Target::Widget(&r), false);
        });
    }
}

/// The `folder.rename` the draft asks for: `Some(params)` when its name is not the folder's now.
pub fn rename_params(app: &App, fields: &Map<String, Value>) -> Result<Option<Value>, CmdError> {
    let folder: FolderId = fields
        .get("folder")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| CmdError::Internal("the Folder Properties draft has no folder".into()))?;
    let name = chosen_name(fields).map_err(|why| CmdError::Disabled(why.into()))?;
    let now = app.session().and_then(|s| s.library().folder(folder)).map(|f| folder_name(&f.path));
    Ok((now.as_deref() != Some(name.as_str())).then(|| json!({ "folder": folder, "name": name })))
}

/// OK: `folder.rename {folder, name}` when the name changed, then `folder.info.set {id,
/// description, place, date}`, `date` being `"oldest"` while "Use the date of the oldest photo" is
/// on. The reply is `folder.info.set`'s, with the rename's under `rename` when there was one.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let f = &d.fields;
    let id = f.get("folder").cloned().ok_or_else(|| CmdError::Internal("the Folder Properties draft has no folder".into()))?;
    let date = match chosen_date(f) {
        Ok(None) => json!("oldest"),
        Ok(Some(day)) => json!(day.to_string()),
        Err(why) => return Err(CmdError::BadParams(why.into())),
    };
    // The rename first: a refusal (a name taken, an offline folder) changes nothing at all.
    let renamed = match rename_params(app, f)? {
        Some(params) => Some(app.run("folder.rename", &params)?),
        None => None,
    };
    let mut reply =
        app.run("folder.info.set", &json!({ "id": id, "description": text(f, "description"), "place": text(f, "place"), "date": date }))?;
    if let (Some(r), Some(o)) = (renamed, reply.as_object_mut()) {
        o.insert("rename".into(), r);
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_is_the_oldest_photos_or_a_day_typed() {
        let folder = FolderId::new(3);
        let day = Date::new(2026, 6, 4).unwrap();
        let mut f = draft(folder, "Lisbon", Some(day), true, "", "");
        assert_eq!(f["date"], json!("2026-06-04"));
        assert_eq!(chosen_date(&f), Ok(None), "the box on: the oldest photo's");
        assert_eq!(date_note(&f).unwrap(), "Thursday, June 4, 2026");
        f.insert("useOldest".into(), json!(false));
        assert_eq!(chosen_date(&f), Ok(Some(day)));
        for bad in ["", "June 4", "2026-02-30", "2026-06-04T10:00:00"] {
            f.insert("date".into(), json!(bad));
            assert_eq!(chosen_date(&f), Err(BAD_DATE), "{bad:?}");
            assert_eq!(date_note(&f), Err(BAD_DATE));
        }
        f.insert("date".into(), json!(" 2026-06-05 "));
        assert_eq!(chosen_date(&f), Ok(Some(Date::new(2026, 6, 5).unwrap())), "spaces around it are fine");
        let none = draft(folder, "x", None, true, "", "");
        assert_eq!((&none["date"], date_note(&none).unwrap().as_str()), (&Value::Null, "No photo here has a date."));
    }
}
