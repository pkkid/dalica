//! Albums in the UI (`organizing.md` §4): the commands that open Album Properties and the album
//! rows of the photo menu.
//!
//! - `album.new {ids?}` (File ▸ New Album…, Ctrl+N, the toolbar's New Album, Add to Album ▸ New
//!   Album…): Album Properties for a new album with the photos named, else the tray's (an empty
//!   album is allowed). Its OK runs the engine's `album.create`, the way `output.export` runs
//!   `export.run` (`crate::dialogs::album_properties`).
//! - `section.properties` on an album section (the fourth menu's Album Properties…, a double-click
//!   on the album's name or header) opens Album Properties for that album ([`route`]); its OK runs
//!   `album.info.set`. On a folder it stays Folder Properties (`crate::commands::info`).
//! - `photo.showInFolder {id?}` (the photo menu in an album): selects the photo in its folder and
//!   scrolls there.
//!
//! What the UI knows of an album (its name for the questions) is [`album_name`], the library's;
//! Album Properties' draft starts from what the library holds
//! ([`album_properties::draft_of`]), a new album's from its photos ([`album_properties::new_draft`]).

use dalica_core::{AlbumId, PhotoId};
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::album_properties;
use crate::state::{AlbumDraft, MAX_IDS, Section};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "album.new",
        label: "New Album…",
        menu: "File/New Album…",
        keys: &[(Context::Global, "Ctrl+N")],
        params: "{\"ids\":[PhotoId]=tray}: open Album Properties for a new album of these photos (replies {dialog: \"albumProperties\"}; its OK runs album.create)",
        run: new_album,
    },
    UiCommand {
        id: "photo.showInFolder",
        label: "Show in Folder",
        menu: "",
        keys: &[],
        params: "{\"id\":PhotoId=focused}: select the photo in its folder's section and scroll there",
        run: show_in_folder,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("photo.showInFolder", can_show_in_folder)];

/// The name of album `id` as the library knows it; `None` for an album it does not have.
pub fn album_name(app: &App, id: &AlbumId) -> Option<String> {
    app.session()?.library().album(id).map(|a| a.name.clone())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAlbum {
    ids: Option<Vec<PhotoId>>,
}

/// The photos `ids` names, each one the library's (`not_found` otherwise), at most [`MAX_IDS`].
pub(crate) fn known_photos(app: &App, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("`ids` holds at most {MAX_IDS} photos")));
    }
    let library = app.session().map(dalica_engine::Session::library);
    if let Some(id) = ids.iter().find(|id| !library.is_some_and(|l| l.photo(**id).is_some())) {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    Ok(ids)
}

/// `album.new {ids?}`: Album Properties for a new album of the photos named, else the tray's.
fn new_album(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: NewAlbum = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => known_photos(app, ids)?,
        None => crate::tray::photos(app.state()),
    };
    let draft = album_properties::new_draft(app.session().map(dalica_engine::Session::library), ids);
    album_properties::open(app, draft)
}

/// The user album the current section is, if it is one.
fn current_album(app: &App) -> Option<AlbumId> {
    app.state().selection.section.and_then(Section::user_album)
}

/// `section.properties` without a folder named, while the current section is an album: Album
/// Properties for it. `None`: run the command as it is (Folder Properties).
pub fn route(app: &mut App, id: &str, params: &Value) -> Option<Result<Value, CmdError>> {
    if id != "section.properties" || params.get("folder").is_some() {
        return None;
    }
    let section = app.state().selection.section?;
    if !section.is_album() {
        return None;
    }
    let Some(album) = current_album(app) else { return Some(Err(CmdError::Disabled(NOT_A_USER_ALBUM.into()))) };
    let draft = match app.session().map(dalica_engine::Session::library) {
        Some(library) => match album_properties::draft_of(library, &album) {
            Some(d) => d,
            None => return Some(Err(CmdError::NotFound(format!("no album {album}")))),
        },
        None => AlbumDraft { album: Some(album), ..AlbumDraft::default() },
    };
    Some(album_properties::open(app, draft))
}

/// Why an automatic album has no properties.
pub const NOT_A_USER_ALBUM: &str = "Starred Photos, Recently Updated and search results have no properties.";

/// Whether `id` can run now, for the commands [`route`] takes over: `None` for the others.
pub fn enabled_route(app: &App, id: &str, params: &Value) -> Option<Result<(), String>> {
    if id != "section.properties" || params.get("folder").is_some() || !app.state().selection.section.is_some_and(Section::is_album) {
        return None;
    }
    Some(current_album(app).map(|_| ()).ok_or_else(|| NOT_A_USER_ALBUM.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowInFolder {
    id: Option<PhotoId>,
}

/// The photo Show in Folder acts on: the one named, else the focused or first selected.
fn shown_photo(app: &App, params: &Value) -> Result<PhotoId, CmdError> {
    let p: ShowInFolder = parse_object(params)?;
    let sel = &app.state().selection;
    let id = p.id.or(sel.anchor).or_else(|| sel.ids.first().copied()).ok_or_else(|| CmdError::Disabled(NO_PHOTO.into()))?;
    known_photos(app, vec![id])?;
    Ok(id)
}

/// Why Show in Folder cannot run.
pub const NO_PHOTO: &str = "Select a photo first.";

fn can_show_in_folder(app: &App, params: &Value) -> Result<(), String> {
    shown_photo(app, params).map(|_| ()).map_err(|e| e.to_string())
}

/// `photo.showInFolder {id?}`: the photo selected alone in its folder's section, scrolled to.
fn show_in_folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = shown_photo(app, params)?;
    app.run("select.set", &json!({ "ids": [id] }))?;
    let folder = app.session().and_then(|s| s.library().photo(id)).map(|p| p.folder).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let section = Section::Folder(folder);
    // Hidden or small pictures are not on the light table: selected, but nothing to scroll to.
    let _ = app.run("section.scrollTo", &json!({ "section": section }));
    Ok(json!({ "id": id, "section": section }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::state::{AlbumKey, AlbumSection, Selection};

    #[test]
    fn album_new_opens_album_properties_for_the_tray_or_the_photos_named() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        app.run("select.set", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(app.run("album.new", &json!({})).unwrap(), json!({ "dialog": "albumProperties" }));
        let d = app.state().album_properties.clone().unwrap();
        assert_eq!((d.album, d.ids, d.name.as_str(), d.use_newest), (None, vec![ids[0], ids[1]], "New Album", true));
        assert!(app.run("album.new", &json!({})).is_err(), "one at a time");
        crate::dialogs::cancel(&mut app).unwrap();
        app.run("album.new", &json!({ "ids": [ids[2]] })).unwrap();
        assert_eq!(app.state().album_properties.as_ref().map(|d| d.ids.clone()), Some(vec![ids[2]]));
        crate::dialogs::cancel(&mut app).unwrap();
        // An empty album is allowed.
        app.run("select.none", &json!({})).unwrap();
        app.run("album.new", &Value::Null).unwrap();
        assert_eq!(app.state().album_properties.as_ref().map(|d| d.ids.len()), Some(0));
        crate::dialogs::cancel(&mut app).unwrap();
        // Graceful failures, nothing opened.
        for bad in [json!({ "ids": "1" }), json!({ "ids": [-1] }), json!({ "x": 1 }), json!([1]), json!("x")] {
            assert!(matches!(app.run("album.new", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(app.run("album.new", &json!({ "ids": [ids[2].get() + 99] })), Err(CmdError::NotFound(_))));
        assert!(app.state().dialogs.is_empty());
    }

    #[test]
    fn section_properties_on_an_album_opens_album_properties() {
        let mut app = App::new(None);
        let album = AlbumId::parse("a91c0e").unwrap();
        let section = Section::Album(AlbumSection::User(AlbumKey::of(&album)));
        assert_eq!(enabled_route(&app, "section.properties", &json!({})), None, "no album selected");
        app.state_mut().selection = Selection::new(vec![], None, Some(section), true);
        assert_eq!(enabled_route(&app, "section.properties", &json!({})), Some(Ok(())));
        assert_eq!(enabled_route(&app, "section.properties", &json!({ "folder": 1 })), None, "a folder named is Folder Properties");
        assert_eq!(app.run("section.properties", &json!({})).unwrap(), json!({ "dialog": "albumProperties" }));
        assert_eq!(app.state().album_properties.as_ref().and_then(|d| d.album.clone()), Some(album));
        crate::dialogs::cancel(&mut app).unwrap();
        // Starred Photos has no properties.
        app.state_mut().selection = Selection::new(vec![], None, Some(Section::Album(AlbumSection::Starred)), true);
        assert!(matches!(enabled_route(&app, "section.properties", &json!({})), Some(Err(_))));
        assert!(matches!(app.run("section.properties", &json!({})), Err(CmdError::Disabled(_))));
        assert!(app.state().dialogs.is_empty());
    }

    #[test]
    fn show_in_folder_selects_the_photo_in_its_folder() {
        let (_dir, s, ids) = session_with_photos(3);
        let folder = s.library().photo(ids[1]).unwrap().folder;
        let mut app = App::new(Some(s));
        assert!(can_show_in_folder(&app, &json!({})).is_err(), "nothing selected");
        assert!(matches!(app.run("photo.showInFolder", &json!({})), Err(CmdError::Disabled(_))));
        let r = app.run("photo.showInFolder", &json!({ "id": ids[1] })).unwrap();
        assert_eq!(r, json!({ "id": ids[1], "section": format!("folder:{folder}") }));
        assert_eq!(app.state().selection.ids, [ids[1]]);
        assert_eq!(app.state().selection.section, Some(Section::Folder(folder)));
        for bad in [json!({ "id": "x" }), json!({ "id": 1, "y": 2 }), json!([])] {
            assert!(matches!(app.run("photo.showInFolder", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(app.run("photo.showInFolder", &json!({ "id": 9999 })), Err(CmdError::NotFound(_))));
    }
}
