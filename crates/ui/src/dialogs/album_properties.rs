//! Album Properties (`organizing.md` §4.2): opened by File ▸ New Album… (`album.new`, Ctrl+N, the
//! toolbar's New Album) for a new album with the tray's photos, and by `section.properties` on an
//! album section for that album (`crate::commands::albums`: the Album menu's Album Properties…, a
//! double-click on the album's tree row or header title).
//!
//! ```text
//! Album Properties
//! Name:          [New Album                        ]   (selected, ready for typing)
//! Date:          [2026-06-04]  Thursday, June 4, 2026
//!                [x] Use the date of the newest photo
//! Place:         [                                 ]
//! Description:   [                                 ]
//!                                          [Cancel] [OK]
//! ```
//!
//! Its draft is the view state's `albumProperties` ([`AlbumDraft`]): the album (`null` for a new
//! one), its photos (a new one), Name ("New Album" for a new one), the date the Date field shows
//! (`"YYYY-MM-DD"`, the newest photo's while "Use the date of the newest photo" is on, the field
//! then disabled), Place and Description. OK (enabled while the name is not blank and, with the box
//! off, the date reads as a day or is empty) runs `album.create {name, date?, useNewest, place,
//! description, ids}` for a new album, then selects it as a unit and scrolls to it, or
//! `album.info.set {album, …}` for an existing one ([`run_params`]); Cancel and Esc change nothing.
//!
//! Widget ids (`control-protocol.md` §7): `dialog:albumProperties`, `field:dialog.name`,
//! `field:dialog.date`, `check:dialog.useNewest`, `field:dialog.place`, `field:dialog.description`,
//! `button:dialog.cancel`, `button:dialog.ok`.

use dalica_core::{AlbumId, Date, PhotoId};
use dalica_engine::CmdError;
use dalica_library::Library;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target, TextField};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Id, Layout, RichText, Ui, vec2};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::panels::light_table::header::weekday;
use crate::panels::light_table::long_date;
use crate::state::{AlbumDraft, AlbumKey, AlbumSection, ScrollAnchor, Section, Selection};

/// The dialog's title and width (points).
pub const TITLE: &str = "Album Properties";
pub const WIDTH: f32 = 440.0;
/// "Use the date of the newest photo".
pub const USE_NEWEST: &str = "Use the date of the newest photo";
/// The label column's width, the gap between columns and rows, the Date field's width (points).
const LABEL_W: f32 = 96.0;
const GAP: f32 = 8.0;
const DATE_W: f32 = 112.0;
/// What the Date field says while empty.
pub const DATE_HINT: &str = "YYYY-MM-DD";
/// Why OK is disabled when the Date field does not read as a day.
pub const BAD_DATE: &str = "Type the date as YYYY-MM-DD.";
/// Why OK is disabled with a blank name.
pub const NO_NAME: &str = "Name the album.";

pub const NAME_ID: &str = "field:dialog.name";
pub const DATE_ID: &str = "field:dialog.date";
pub const USE_NEWEST_ID: &str = "check:dialog.useNewest";
pub const PLACE_ID: &str = "field:dialog.place";
pub const DESCRIPTION_ID: &str = "field:dialog.description";

/// Opens the dialog with `draft`. Replies `{dialog: "albumProperties"}`.
pub fn open(app: &mut App, draft: AlbumDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::AlbumProperties))?;
    app.state_mut().album_properties = Some(draft.sanitized());
    Ok(reply)
}

/// The newest effective date of `ids` (a new album's date while "Use the date of the newest photo"
/// is on). A pass over the photos named: on opening and on a click, never per frame.
pub fn newest_date(library: &Library, ids: &[PhotoId]) -> Option<Date> {
    ids.iter().filter_map(|&id| library.photo(id).and_then(|p| library.sort_date(p))).max().map(|d| d.date())
}

/// The date the box shows while on: an existing album's newest photo's, a new album's photos'.
fn newest_for(library: &Library, draft: &AlbumDraft) -> Option<Date> {
    match &draft.album {
        Some(album) => library.album_members(album).iter().filter_map(|&id| library.photo(id).and_then(|p| library.sort_date(p))).max(),
        None => draft.ids.iter().filter_map(|&id| library.photo(id).and_then(|p| library.sort_date(p))).max(),
    }
    .map(|d| d.date())
}

/// The draft for album `id` as the library holds it: its name, the date set (the box off) or its
/// newest photo's (the box on), place and description; `None` when the library has no such album.
pub fn draft_of(library: &Library, id: &AlbumId) -> Option<AlbumDraft> {
    let info = library.album(id)?;
    let mut d = AlbumDraft {
        album: Some(id.clone()),
        ids: Vec::new(),
        name: info.name.clone(),
        date: String::new(),
        use_newest: info.use_newest || info.date.is_none(),
        place: info.place.clone(),
        description: info.description.clone(),
    };
    let date = if d.use_newest { newest_for(library, &d) } else { info.date };
    d.date = date.map(|d| d.to_string()).unwrap_or_default();
    Some(d)
}

/// A new album's draft for `ids`: "New Album", dated by its newest photo.
pub fn new_draft(library: Option<&Library>, ids: Vec<PhotoId>) -> AlbumDraft {
    let date = library.and_then(|l| newest_date(l, &ids)).map(|d| d.to_string()).unwrap_or_default();
    AlbumDraft { ids, date, ..AlbumDraft::default() }
}

/// The date the draft gives: `Ok(None)` for the newest photo's (the box on) or none typed,
/// `Ok(Some)` for a day typed, `Err` when the field does not read as one.
pub fn chosen_date(draft: &AlbumDraft) -> Result<Option<Date>, &'static str> {
    let typed = draft.date.trim();
    if draft.use_newest || typed.is_empty() {
        return Ok(None);
    }
    typed.parse::<Date>().map(Some).map_err(|_| BAD_DATE)
}

/// Why OK cannot run with `draft`, if it cannot: a blank name, or a date that is no day.
pub fn ready(draft: &AlbumDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        return Err(NO_NAME.into());
    }
    chosen_date(draft).map(|_| ()).map_err(str::to_string)
}

/// The command OK runs for `draft` and its parameters: `album.create` for a new album (with its
/// photos), `album.info.set` for an existing one. The date is sent when typed with the box off;
/// with the box off and the field empty an existing album's date is cleared (`null`).
pub fn run_params(draft: &AlbumDraft) -> (&'static str, Value) {
    let mut p = Map::new();
    p.insert("name".into(), json!(draft.name.trim()));
    if !draft.use_newest {
        let typed = draft.date.trim();
        if !typed.is_empty() {
            p.insert("date".into(), json!(typed));
        } else if draft.album.is_some() {
            p.insert("date".into(), Value::Null);
        }
    }
    p.insert("useNewest".into(), json!(draft.use_newest));
    p.insert("place".into(), json!(draft.place));
    p.insert("description".into(), json!(draft.description));
    match &draft.album {
        None => {
            p.insert("ids".into(), json!(draft.ids));
            ("album.create", Value::Object(p))
        }
        Some(a) => {
            p.insert("album".into(), json!(a));
            ("album.info.set", Value::Object(p))
        }
    }
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ok = app.state().album_properties.as_ref().is_some_and(|d| ready(d).is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", "OK", ButtonKind::Default).enabled(ok)] }
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
fn date_note(draft: &AlbumDraft) -> Result<String, &'static str> {
    chosen_date(draft)?;
    Ok(match draft.date.trim().parse::<Date>() {
        Ok(day) => format!("{}, {}", weekday(day), long_date(day)),
        Err(_) if draft.use_newest => "No photo here has a date.".to_string(),
        Err(_) => String::new(),
    })
}

/// Whether this is the dialog's first or second pass since it opened: the Name field asks for the
/// focus then (the window takes no input on its very first pass, while egui sizes it).
fn opening(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    let seen = Id::new(NAME_ID).with("albumProperties.passes");
    let pass = ctx.cumulative_pass_nr();
    let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
        Some((first, last)) if last.checked_add(1) == Some(pass) => first,
        _ => pass,
    };
    ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
    pass.saturating_sub(first) < 2
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(before) = app.state().album_properties.clone() else { return };
    let t = *app.tokens();
    let mut d = before.clone();
    let full = (ui.available_width() - LABEL_W - GAP).max(24.0);
    ui.spacing_mut().item_spacing.y = GAP;
    if opening(ui) {
        widgets::focus_field(ui.ctx(), NAME_ID);
    }
    row(ui, &t, "Name:", |ui| TextField::new(NAME_ID, "Album name", full).select_on_focus(true).show(ui, &t, &mut d.name));
    row(ui, &t, "Date:", |ui| {
        ui.add_enabled_ui(!d.use_newest, |ui| TextField::new(DATE_ID, DATE_HINT, DATE_W).show(ui, &t, &mut d.date));
        let (note, color) = match date_note(&d) {
            Ok(note) => (note, t.text_dim),
            Err(why) => (why.to_string(), t.caution),
        };
        ui.label(RichText::new(note).font(fonts::regular(t.detail)).color(color));
    });
    row(ui, &t, "", |ui| {
        let mut on = d.use_newest;
        let r = ui.checkbox(&mut on, RichText::new(USE_NEWEST).font(fonts::regular(t.body)));
        widgets::register(ui.ctx(), USE_NEWEST_ID, Role::Toggle, USE_NEWEST, Target::Widget(&r), on);
        if on != d.use_newest {
            d.use_newest = on;
            if on {
                // Back to the newest photo's date: a pass over the album's photos, on this click.
                let date = app.session().and_then(|s| newest_for(s.library(), &d));
                d.date = date.map(|d| d.to_string()).unwrap_or_default();
            }
        }
    });
    row(ui, &t, "Place:", |ui| TextField::new(PLACE_ID, "Where these photos were taken", full).show(ui, &t, &mut d.place));
    row(ui, &t, "Description:", |ui| TextField::new(DESCRIPTION_ID, "Add a description", full).show(ui, &t, &mut d.description));
    if d != before {
        app.state_mut().album_properties = Some(d.sanitized());
    }
}

/// OK: creates the album (then selects it as a unit and scrolls to it) or sets its information.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().album_properties.clone().ok_or_else(|| CmdError::Internal("Album Properties has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    let (id, params) = run_params(&draft);
    let reply = app.run(id, &params)?;
    if draft.album.is_none()
        && let Some(album) = reply.get("album").and_then(Value::as_str).and_then(|a| AlbumId::parse(a).ok())
    {
        select_new(app, &album);
    }
    Ok(reply)
}

/// Selects a new album as a unit (its tree row and header show it selected) and scrolls the light
/// table to its section, which the next frame's sections hold: its photos are the album's members,
/// in its order.
fn select_new(app: &mut App, album: &AlbumId) {
    let Some(library) = app.session().map(dalica_engine::Session::library) else { return };
    let ids = library.album_members(album).to_vec();
    let section = Section::Album(AlbumSection::User(AlbumKey::of(album)));
    let state = app.state_mut();
    state.selection = Selection::new(ids.clone(), ids.first().copied(), Some(section), true);
    if !ids.is_empty() {
        state.scroll = ScrollAnchor { section: Some(section), offset: 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use dalica_core::{AlbumId, PhotoId};

    use super::*;
    use crate::dialogs;

    #[test]
    fn ok_creates_a_new_album_or_sets_an_existing_ones_information() {
        let new = AlbumDraft { ids: vec![PhotoId::new(2), PhotoId::new(5)], ..AlbumDraft::default() };
        assert_eq!(
            run_params(&new),
            ("album.create", json!({ "name": "New Album", "useNewest": true, "place": "", "description": "", "ids": [2, 5] }))
        );
        let existing = AlbumDraft {
            album: Some(AlbumId::parse("a91c0e").unwrap()),
            name: " Trip ".into(),
            date: "2026-06-04".into(),
            use_newest: false,
            ..AlbumDraft::default()
        };
        assert_eq!(
            run_params(&existing),
            (
                "album.info.set",
                json!({ "name": "Trip", "date": "2026-06-04", "useNewest": false, "place": "", "description": "", "album": "a91c0e" })
            )
        );
        assert!(ready(&AlbumDraft { name: "  ".into(), ..AlbumDraft::default() }).is_err());
        let mut app = App::new(None);
        open(&mut app, new.clone()).unwrap();
        assert_eq!(app.state().album_properties, Some(new));
        assert_eq!(dialogs::inspect(&app)["default"], "button:dialog.ok");
        app.state_mut().album_properties.as_mut().unwrap().name.clear();
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "OK waits for a name");
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().album_properties, None);
    }
}
