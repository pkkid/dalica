//! Album Properties (`organizing.md` §4.2): opened by File ▸ New Album… (`album.new`, Ctrl+N, the
//! toolbar's New Album) for a new album with the tray's photos, and by `section.properties` on an
//! album section for that album (`crate::commands::albums`). Its draft is the view state's
//! `albumProperties` ([`AlbumDraft`]): Name ("New Album", selected, for a new one), Date with "Use
//! the date of the newest photo" (on), Place and Description. OK (enabled while the name is not
//! blank) runs `album.create {name, date?, useNewest, place, description, ids}` for a new album,
//! `album.info.set {album, …}` for an existing one ([`run_params`]); Cancel and Esc change nothing.
//!
//! A stub until M9.17: the body shows the draft as text.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::AlbumDraft;

/// The dialog's title and width (points).
pub const TITLE: &str = "Album Properties";
pub const WIDTH: f32 = 440.0;
/// "Use the date of the newest photo".
pub const USE_NEWEST: &str = "Use the date of the newest photo";

/// Opens the dialog with `draft`. Replies `{dialog: "albumProperties"}`.
pub fn open(app: &mut App, draft: AlbumDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::AlbumProperties))?;
    app.state_mut().album_properties = Some(draft.sanitized());
    Ok(reply)
}

/// Why OK cannot run with `draft`, if it cannot: a blank name.
pub fn ready(draft: &AlbumDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() { Err("Name the album.".into()) } else { Ok(()) }
}

/// The command OK runs for `draft` and its parameters: `album.create` for a new album (with its
/// photos), `album.info.set` for an existing one. The date is sent only when typed and not taken
/// from the newest photo.
pub fn run_params(draft: &AlbumDraft) -> (&'static str, Value) {
    let mut p = Map::new();
    p.insert("name".into(), json!(draft.name.trim()));
    if !draft.use_newest && !draft.date.trim().is_empty() {
        p.insert("date".into(), json!(draft.date.trim()));
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

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(d) = app.state().album_properties.clone() else { return };
    let tick = if d.use_newest { "[x]" } else { "[ ]" };
    let lines = [
        format!("Name: {}", d.name),
        format!("Date: {}", d.date),
        format!("{tick} {USE_NEWEST}"),
        format!("Place: {}", d.place),
        format!("Description: {}", d.description),
        format!("{} photos", d.ids.len()),
    ];
    super::text_lines(app, ui, &lines);
}

/// OK: creates the album or sets its information.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().album_properties.clone().ok_or_else(|| CmdError::Internal("Album Properties has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    let (id, params) = run_params(&draft);
    app.run(id, &params)
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
