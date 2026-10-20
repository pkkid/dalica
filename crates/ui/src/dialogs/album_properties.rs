//! Album Properties (`organizing.md` §4.2): opened by File ▸ New Album… (`album.new`, Ctrl+N, the
//! toolbar's New Album) for a new album with the tray's photos, by `section.properties` on an
//! album section for that album (`crate::commands::albums`: the Album menu's Album Properties…, a
//! double-click on the album's tree row or header title), and by Tools ▸ Experimental ▸ Save
//! Search Results… for a new album of the search's matches ([`open_for_search`]: named
//! `Search results for "x"`, its dialog carrying the command `search.saveAsAlbum`).
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
//! `album.info.set {album, …}` for an existing one ([`run_params`]); for Save Search Results… it
//! runs `search.saveAsAlbum {name}` (the matches in light-table order, one undo step), then, when a
//! date, place or description was given, `album.info.set` with them (a second step), and selects
//! the album as New Album does. Cancel and Esc change nothing.
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

use super::folder_properties::shown;
use super::{Button, Dialog, DialogKind, Look, form_label};
use crate::app::App;
use crate::commands::search::SAVE_COMMAND;
use crate::i18n::format::long_date_with_weekday;
use crate::state::{AlbumDraft, AlbumKey, AlbumSection, DialogCommand, ScrollAnchor, Section, Selection};
use crate::tl;

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

/// Opens the dialog for Save Search Results… with `draft` (a new album's, named for the search):
/// its OK runs `search.saveAsAlbum`. Replies `{dialog: "albumProperties"}`.
pub fn open_for_search(app: &mut App, draft: AlbumDraft) -> Result<Value, CmdError> {
    let command = DialogCommand { id: SAVE_COMMAND.into(), params: json!({}) };
    let reply = super::open(app, Dialog { command: Some(command), ..Dialog::new(DialogKind::AlbumProperties) })?;
    app.state_mut().album_properties = Some(AlbumDraft { album: None, ids: Vec::new(), ..draft }.sanitized());
    Ok(reply)
}

/// Whether `d` is Save Search Results…' dialog.
fn saves_search(d: &Dialog) -> bool {
    d.command.as_ref().is_some_and(|c| c.id == SAVE_COMMAND)
}

/// The newest effective date of the search's matches (the engine's last answer): on opening Save
/// Search Results… and on a click of its box, never per frame.
pub fn newest_match(session: &dalica_engine::Session) -> Option<Date> {
    let result = session.search_state().result.as_ref()?;
    newest_date(session.library(), result.photos())
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
    // The name it offers, in the language in effect; saved as it is unless changed, so without
    // the catalog's line-break marks.
    AlbumDraft { ids, date, name: crate::i18n::user_text(tl!("New Album")), ..AlbumDraft::default() }
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
    Look {
        title: tl!("Album Properties"),
        width: WIDTH,
        buttons: vec![Button::cancel(), Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(ok)],
    }
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
        Ok(day) => long_date_with_weekday(day),
        Err(_) if draft.use_newest => tl!("No photo here has a date."),
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

pub fn body(app: &mut App, ui: &mut Ui, dialog: &mut Dialog) {
    let search = saves_search(dialog);
    let Some(before) = app.state().album_properties.clone() else { return };
    let t = *app.tokens();
    let mut d = before.clone();
    let full = (ui.available_width() - LABEL_W - GAP).max(24.0);
    ui.spacing_mut().item_spacing.y = GAP;
    if opening(ui) {
        widgets::focus_field(ui.ctx(), NAME_ID);
    }
    row(ui, &t, &form_label(&tl!("Name")), |ui| TextField::new(NAME_ID, &tl!("Album name"), full).select_on_focus(true).show(ui, &t, &mut d.name));
    row(ui, &t, &form_label(&tl!("Date")), |ui| {
        ui.add_enabled_ui(!d.use_newest, |ui| TextField::new(DATE_ID, &tl!(ctx = "date", "YYYY-MM-DD"), DATE_W).show(ui, &t, &mut d.date));
        let (note, color) = match date_note(&d) {
            Ok(note) => (note, t.text_dim),
            Err(why) => (shown(why), t.caution),
        };
        ui.label(RichText::new(note).font(fonts::regular(t.detail)).color(color));
    });
    row(ui, &t, "", |ui| {
        let mut on = d.use_newest;
        let label = tl!("Use the date of the newest photo");
        let r = ui.checkbox(&mut on, RichText::new(&label).font(fonts::regular(t.body)));
        widgets::register(ui.ctx(), USE_NEWEST_ID, Role::Toggle, &label, Target::Widget(&r), on);
        if on != d.use_newest {
            d.use_newest = on;
            if on {
                // Back to the newest photo's date: a pass over the album's photos, on this click.
                let date = app.session().and_then(|s| if search { newest_match(s) } else { newest_for(s.library(), &d) });
                d.date = date.map(|d| d.to_string()).unwrap_or_default();
            }
        }
    });
    row(ui, &t, &form_label(&tl!("Place")), |ui| TextField::new(PLACE_ID, &tl!("Where these photos were taken"), full).show(ui, &t, &mut d.place));
    row(ui, &t, &form_label(&tl!("Description")), |ui| {
        TextField::new(DESCRIPTION_ID, &tl!("Add a description"), full).show(ui, &t, &mut d.description)
    });
    if d != before {
        app.state_mut().album_properties = Some(d.sanitized());
    }
}

/// OK: creates the album (then selects it as a unit and scrolls to it) or sets its information;
/// for Save Search Results… saves the search as an album ([`save_search`]).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().album_properties.clone().ok_or_else(|| CmdError::Internal("Album Properties has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    if saves_search(d) {
        return save_search(app, &draft);
    }
    let (id, params) = run_params(&draft);
    let reply = app.run(id, &params)?;
    if draft.album.is_none()
        && let Some(album) = reply.get("album").and_then(Value::as_str).and_then(|a| AlbumId::parse(a).ok())
    {
        select_new(app, &album);
    }
    Ok(reply)
}

/// Save Search Results…' OK: `search.saveAsAlbum {name}`, then the date, place and description
/// typed (`album.info.set`, only when one was given), then the album selected and scrolled to.
fn save_search(app: &mut App, draft: &AlbumDraft) -> Result<Value, CmdError> {
    let reply = app.run(SAVE_COMMAND, &json!({ "name": draft.name.trim() }))?;
    let Some(album) = reply.get("album").and_then(Value::as_str).and_then(|a| AlbumId::parse(a).ok()) else { return Ok(reply) };
    let typed = !draft.use_newest || !draft.place.trim().is_empty() || !draft.description.trim().is_empty();
    if typed {
        let (id, params) = run_params(&AlbumDraft { album: Some(album.clone()), ..draft.clone() });
        // The album is made: a refusal here leaves it named, and says why on the status line.
        let _ = app.run(id, &params);
    }
    select_new(app, &album);
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
    fn a_new_albums_translated_name_is_saved_without_line_break_marks() {
        use dalica_ui_kit::linebreak::{self, MARK};
        // In the pseudo-locale the offered name is the translated one, and OK saves it as shown.
        let draft = crate::i18n::with_language(&crate::i18n::languages::PSEUDO, || new_draft(None, vec![PhotoId::new(2)]));
        assert_eq!(draft.name, crate::i18n::pseudo::translate("New Album"));
        assert!(!run_params(&draft).1["name"].as_str().unwrap().contains(MARK));
        // The Thai catalog's text carries marks between its words (placed at load); the name OK
        // saves keeps none of them.
        let thai = crate::i18n::languages::by_code("th").unwrap();
        let (shown, draft) = crate::i18n::with_language(thai, || (tl!("New Album"), new_draft(None, vec![PhotoId::new(2)])));
        assert!(shown.contains(MARK), "{shown:?}");
        assert_eq!(draft.name, linebreak::unmark(&shown));
        assert_eq!(draft.name, "อัลบั้มใหม่");
        assert!(!run_params(&draft).1["name"].as_str().unwrap().contains(MARK));
    }

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
