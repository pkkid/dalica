//! The photo context menu of a thumbnail (`organizing.md` §9.1, `menus-and-shortcuts.md` §4): a
//! right-click on a cell selects the photo first when it is not selected
//! ([`super::input`]), then opens this menu for the selection, bounded by the window like every
//! menu (`crate::menus::context_menu`). Its rows are [`menus::photo_menu`]'s, the album variant in
//! an album section; rows whose commands do not exist yet are greyed with their reason, and
//! `ui.menu.tree {context: "photo"}` returns the same model without opening it.
//!
//! What this module adds to the catalog's rows ([`finish`], which `menus::photo_menu` calls):
//!
//! - **Add to Album ▸** lists the user albums by name (as `albums.list` orders them), each row
//!   running `album.add {album}` on the selection (`menuitem:album.add/<AlbumId>`), then New
//!   Album… (Album Properties for the photos, `album.new`).
//! - **In an album**, Remove from Album and Set as Album Cover act on the album shown: they are
//!   disabled in Starred Photos, Recently Updated and search results, which choose their own
//!   photos, and Set as Album Cover needs the focused photo to be a member. Set as Album Cover runs
//!   `album.setCover` with the album and the focused photo ([`album_params`]); Remove from Album asks
//!   first (`crate::dialogs::remove_from_album`) and shows Delete, the key that runs it there. Show
//!   in Folder is `photo.showInFolder`.
//! - **View and Edit**, the default action (a double-click), is drawn bold ([`BOLD`]).
//!
//! A frame: [`super::input::finish`] notes the cell whose menu was asked for or is open, and once
//! the library is no longer borrowed [`show`] draws the menu on that cell's response and runs the
//! row clicked.

use dalica_core::{AlbumId, PhotoId};
use dalica_engine::{Context, Key};
use egui::Response;
use serde_json::{Value, json};

use crate::app::App;
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node};
use crate::state::Section;

/// The row drawn bold: View and Edit, the photo's default action.
pub const BOLD: &str = "menuitem:view.edit";
/// The submenu the albums are listed in.
pub const ADD_TO_ALBUM: &str = "Add to Album";
/// Why Remove from Album and Set as Album Cover are off outside a user album.
pub const NOT_A_USER_ALBUM: &str = "Starred Photos, Recently Updated and search results choose their own photos.";
/// Why Set as Album Cover is off for a photo that is not in the album.
pub const NOT_A_MEMBER: &str = "The photo is not in this album.";
/// Why the album rows are off when the album shown is gone.
pub const NO_ALBUM: &str = "The album no longer exists.";

/// Draws the photo menu on `response` (a thumbnail cell's) while it is open, and runs the row
/// clicked. Returns the command id run, if any.
pub fn show(response: &Response, app: &mut App) -> Option<&'static str> {
    let nodes = menus::photo_menu(app, menus::Platform::current());
    menus::context_menu_bold(response, app, &nodes, Some(BOLD))
}

/// The user albums by name (case-insensitively, then exactly, then by id: `albums.list`'s order),
/// with their names.
pub fn albums_by_name(app: &App) -> Vec<(AlbumId, String)> {
    let Some(session) = app.session() else { return Vec::new() };
    let mut albums: Vec<(AlbumId, String)> = session.library().albums().map(|(id, info)| (id.clone(), info.name.clone())).collect();
    albums.sort_by(|(a, x), (b, y)| x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| x.cmp(y)).then_with(|| a.cmp(b)));
    albums
}

/// The user albums, the most recent first: by the album's date (its own, or its newest photo's),
/// newest first, undated ones last, then by name.
pub fn albums_by_date(app: &App) -> Vec<(AlbumId, String)> {
    let Some(session) = app.session() else { return Vec::new() };
    let library = session.library();
    let mut albums: Vec<_> = albums_by_name(app).into_iter().map(|(id, name)| (library.album_stats(&id).date, id, name)).collect();
    // A stable sort keeps the name order among equal dates; `None` sorts first, so reversed last.
    albums.sort_by(|(a, ..), (b, ..)| b.cmp(a));
    albums.into_iter().map(|(_, id, name)| (id, name)).collect()
}

/// One `album.add {album}` row per album of `albums`, in that order (`menuitem:album.add/<AlbumId>`,
/// labelled with the album's name); `live` says whether `album.add` exists and `ready` whether it
/// can run on the photos a click passes, and why not.
pub fn album_rows(albums: &[(AlbumId, String)], live: bool, ready: &Result<(), String>) -> Vec<Node> {
    albums
        .iter()
        .map(|(id, name)| {
            let (enabled, reason) = match ready {
                _ if !live => (false, Some("Not available yet.".to_string())),
                Ok(()) => (true, None),
                Err(why) => (false, Some(why.clone())),
            };
            Node::Item(ItemNode {
                id: "album.add",
                label: name.clone(),
                // No catalog label: a menu path finds the row by the album's name only.
                catalog_label: "",
                params: json!({ "album": id }),
                widget: format!("menuitem:album.add/{id}"),
                shortcut: None,
                live,
                enabled,
                reason,
                mark: Mark::None,
                checked: false,
                dialog: None,
            })
        })
        .collect()
}

/// Whether `album.add` exists, and whether it can run on the selection (what a row of Add to
/// Album ▸ passes): asked once for every row, as its `enabled` reads the photos only.
fn add_ready(app: &App, album: &AlbumId) -> (bool, Result<(), String>) {
    let Some(session) = app.session() else { return (false, Err("The library is still opening.".into())) };
    let Some(spec) = session.spec("album.add") else { return (false, Err("Not available yet.".into())) };
    let params = menus::params_for(app, "album.add", &json!({ "album": album }));
    (true, (spec.enabled)(session, &params).map_err(|d| d.0))
}

/// The user album the selection's section is, if it is one.
pub fn current_album(app: &App) -> Option<AlbumId> {
    app.state().selection.section.and_then(Section::user_album)
}

/// The photo the album rows act on alone (Set as Album Cover): the focused one, else the first
/// selected.
fn focused(app: &App) -> Option<PhotoId> {
    let sel = &app.state().selection;
    sel.anchor.filter(|id| sel.contains(*id)).or_else(|| sel.ids.first().copied())
}

/// The parameters a click on Set as Album Cover passes besides its fixed ones: the album shown and
/// the focused photo (`crate::menus::params_for` adds them). Nothing for other commands, or
/// outside a user album.
pub fn album_params(app: &App, id: &str) -> Vec<(String, Value)> {
    if id != "album.setCover" {
        return Vec::new();
    }
    let Some(album) = current_album(app) else { return Vec::new() };
    let mut out = vec![("album".to_string(), json!(album))];
    if let Some(photo) = focused(app) {
        out.push(("id".to_string(), json!(photo)));
    }
    out
}

/// Whether an album row (Remove from Album, Set as Album Cover) can act on the album shown.
fn album_row_ready(app: &App, id: &str) -> Result<(), String> {
    let album = current_album(app).ok_or_else(|| NOT_A_USER_ALBUM.to_string())?;
    let library = app.session().map(dalica_engine::Session::library).ok_or_else(|| "The library is still opening.".to_string())?;
    if library.album(&album).is_none() {
        return Err(NO_ALBUM.into());
    }
    if id == "album.setCover" {
        let photo = focused(app).ok_or_else(|| crate::commands::albums::NO_PHOTO.to_string())?;
        if library.member_key(&album, photo).is_none() {
            return Err(NOT_A_MEMBER.into());
        }
    }
    Ok(())
}

/// Remove from Album's shortcut: Delete, which runs it in an album section (`crate::keys::delete_route`)
/// unless the user gave Delete another command.
fn delete_key(app: &App) -> Option<String> {
    let key = Key::parse("Delete").ok()?;
    let bound = app.session()?.keymap().resolve(&key, Context::Library, false);
    (crate::keys::delete_route(app, &key, bound.as_deref()) == Some("album.remove")).then(|| key.to_string())
}

/// Completes the photo menu's rows (`menus::photo_menu` calls it): Add to Album ▸ gets the albums by
/// name before New Album…, and Remove from Album and Set as Album Cover are off where they cannot
/// act, with the reason.
pub fn finish(app: &App, nodes: &mut [Node]) {
    for node in nodes.iter_mut() {
        match node {
            Node::Submenu { label, children, .. } if *label == ADD_TO_ALBUM => {
                let albums = albums_by_name(app);
                let Some((first, _)) = albums.first() else { continue };
                let (live, ready) = add_ready(app, first);
                let mut rows = album_rows(&albums, live, &ready);
                rows.push(Node::Separator);
                rows.append(children);
                *children = rows;
            }
            Node::Item(i) if matches!(i.id, "album.remove" | "album.setCover") => {
                if i.id == "album.remove" && i.shortcut.is_none() {
                    i.shortcut = delete_key(app);
                }
                if !i.enabled {
                    continue;
                }
                if let Err(why) = album_row_ready(app, i.id) {
                    i.enabled = false;
                    i.reason = Some(why);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::state::{AlbumKey, AlbumSection, Selection};

    fn items(nodes: &[Node]) -> Vec<&ItemNode> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                Node::Item(i) => out.push(i),
                Node::Submenu { children, .. } => out.extend(items(children)),
                Node::Separator => {}
            }
        }
        out
    }

    fn add_to_album(menu: &[Node]) -> &[Node] {
        match menu.iter().find(|n| matches!(n, Node::Submenu { label: ADD_TO_ALBUM, .. })) {
            Some(Node::Submenu { children, .. }) => children,
            _ => panic!("no Add to Album"),
        }
    }

    #[test]
    fn add_to_album_lists_the_albums_by_name_then_new_album() {
        let (_dir, mut s, ids) = session_with_photos(3);
        for name in ["trams", "Beach", "Alps"] {
            s.execute("album.create", &json!({ "name": name })).unwrap();
        }
        let mut app = App::new(Some(s));
        app.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        let children = add_to_album(&menu);
        let labels: Vec<&str> = children.iter().map(|n| if let Node::Item(i) = n { i.label.as_str() } else { "—" }).collect();
        assert_eq!(labels, ["Alps", "Beach", "trams", "—", "New Album…"]);
        let Node::Item(alps) = &children[0] else { panic!() };
        assert_eq!((alps.id, alps.enabled, alps.live), ("album.add", true, true));
        assert!(alps.widget.starts_with("menuitem:album.add/a"), "{}", alps.widget);
        assert_eq!(menus::find_id(children, "album.add", &alps.params).map(|i| &i.label), Some(&alps.label));
        assert!(items(&menu).iter().any(|i| i.widget == BOLD), "View and Edit is the bold row");
        // With nothing selected the album rows say why they are off.
        app.run("select.none", &json!({})).unwrap();
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        let alps = items(&menu).into_iter().find(|i| i.label == "Alps").unwrap().clone();
        assert!(!alps.enabled && alps.reason.is_some());
        // Most recent first: undated albums (empty ones) keep the name order.
        let names: Vec<String> = albums_by_date(&app).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["Alps", "Beach", "trams"]);
    }

    #[test]
    fn album_rows_act_on_the_album_shown_and_the_focused_photo() {
        let (_dir, mut s, ids) = session_with_photos(3);
        let a = s.execute("album.create", &json!({ "name": "Trams", "ids": [ids[0], ids[1]] })).unwrap()["album"].as_str().unwrap().to_string();
        let album = AlbumId::parse(&a).unwrap();
        let mut app = App::new(Some(s));
        let section = Section::Album(AlbumSection::User(AlbumKey::of(&album)));
        app.state_mut().selection = Selection::new(vec![ids[1]], Some(ids[1]), Some(section), false);
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        let row = |id: &str| items(&menu).into_iter().find(|i| i.id == id).unwrap().clone();
        assert!(row("album.setCover").enabled && row("album.remove").enabled, "{:?}", row("album.setCover").reason);
        assert_eq!(menus::params_for(&app, "album.setCover", &Value::Null), json!({ "album": a, "id": ids[1] }));
        // A photo outside the album cannot be its cover.
        app.state_mut().selection = Selection::new(vec![ids[2]], Some(ids[2]), Some(section), false);
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        let cover = items(&menu).into_iter().find(|i| i.id == "album.setCover").unwrap().clone();
        assert_eq!((cover.enabled, cover.reason.as_deref()), (false, Some(NOT_A_MEMBER)));
        // Starred Photos chooses its own photos.
        app.state_mut().selection = Selection::new(vec![ids[0]], Some(ids[0]), Some(Section::Album(AlbumSection::Starred)), false);
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        for id in ["album.setCover", "album.remove"] {
            let i = items(&menu).into_iter().find(|i| i.id == id).unwrap().clone();
            assert_eq!((i.enabled, i.reason.as_deref()), (false, Some(NOT_A_USER_ALBUM)), "{id}");
        }
        assert!(album_params(&app, "album.setCover").is_empty() && album_params(&app, "album.remove").is_empty());
        // A folder's menu has neither row.
        app.state_mut().selection = Selection::new(vec![ids[0]], None, None, false);
        assert!(items(&menus::photo_menu(&app, menus::Platform::Other)).iter().all(|i| i.id != "album.setCover"));
    }

    #[test]
    fn without_a_library_there_are_no_albums_and_no_panic() {
        let app = App::new(None);
        assert!(albums_by_name(&app).is_empty() && albums_by_date(&app).is_empty());
        let menu = menus::photo_menu(&app, menus::Platform::Other);
        assert_eq!(add_to_album(&menu).len(), 1, "New Album… only");
        assert_eq!(album_row_ready(&app, "album.remove"), Err(NOT_A_USER_ALBUM.to_string()));
    }
}
