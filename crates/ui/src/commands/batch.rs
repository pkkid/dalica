//! Batch editing in the window (`editing.md` §9, `menus-and-shortcuts.md` §1.2 and §1.5): Picture ▸
//! Batch Edit ▸ on the tray's photos (`edit.apply`), Edit ▸ Copy All Edits and Paste All Edits
//! (`edit.copyRecipe`, `edit.pasteRecipe`) and Picture ▸ Undo All Edits… (`edit.undoAll`), all
//! engine commands; the UI's part is what they act on, Undo All Edits' question and what Paste
//! skipped.
//!
//! - **What they act on** ([`params_for`], asked first by `crate::keys::params_for`, so a menu row,
//!   its enabled state, a shortcut and `ui.menu.invoke` agree): in the library Batch Edit's
//!   operations, its Rename…, Paste All Edits and Undo All Edits… take the Photo Tray's photos
//!   (the held ones and the selection, [`crate::tray::photos`]), as the tray's own buttons do;
//!   Copy All Edits takes one photo (the tray's only photo, else the photo last clicked in the
//!   selection, else the tray's first). In the edit view they act on the open photo: Batch Edit's
//!   operations become steps of its history (`commands::edit_view::route`), Paste All Edits is
//!   `{id, history: true}` (one step of its history, which its Undo button takes back) and Undo
//!   All Edits… asks about it alone, then removes its edits as one step of its history.
//! - **Undo All Edits…** ([`UNDO_ALL`], an [opener](super::Opener)): asks "Remove all edits from
//!   %d pictures?" (`dialog:undoAllEdits`, `crate::dialogs::undo_all`), naming the saved photos,
//!   which keep their saved pixels; its Remove Edits runs `edit.undoAll` with `"confirm": true`.
//!   Greyed with the engine's reason when none of the photos has edits.
//! - **What Paste skipped** ([`ran`], called by [`App::run`] after every command): the photos the
//!   library paste left alone (videos, photos holding entries Dalica cannot read that the copied
//!   recipe lacks) become one caution toast naming them with the reason.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ToastKind;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Opener, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::undo_all;

/// No UI command of its own: Batch Edit's rows run engine commands.
pub const COMMANDS: &[UiCommand] = &[];

/// The commands that act on the tray's photos in the library ([`params_for`]).
pub const TRAY: [&str; 4] = ["edit.apply", "edit.pasteRecipe", "edit.undoAll", "photo.rename"];

/// Picture ▸ Undo All Edits…: its question for the tray's photos (the open photo in the edit view).
pub const UNDO_ALL: Opener = Opener { id: "edit.undoAll", dialog: "undoAllEdits", keys: &[], open: open_undo_all, check: check_undo_all };

/// How many skipped photos the toast names.
const NAMED: usize = 3;

/// The parameters a click, a shortcut or `ui.menu.invoke` passes to `id`, for the batch commands
/// (see the module); `None` for every other command.
pub fn params_for(app: &App, id: &str) -> Option<Value> {
    let open = super::edit_view::open_photo(app);
    match (id, open) {
        ("edit.copyRecipe", Some(photo)) => Some(json!({ "id": photo })),
        ("edit.copyRecipe", None) => Some(copy_source(app).map_or_else(|| json!({}), |p| json!({ "id": p }))),
        ("edit.apply", Some(photo)) => Some(json!({ "ids": [photo] })),
        ("edit.pasteRecipe" | "edit.undoAll", Some(photo)) => Some(json!({ "id": photo, "history": true })),
        // Rename… in the edit view keeps what the edit view passes.
        ("photo.rename", Some(_)) => None,
        (_, None) if TRAY.contains(&id) => Some(json!({ "ids": crate::tray::photos(app.state()) })),
        _ => None,
    }
}

/// Why Copy All Edits has no photo to copy from.
pub const NO_PHOTO: &str = "Select a photo to copy its edits from.";

/// Why the menu row of `id` is greyed though its engine command would accept the click: Copy All
/// Edits with no photo to copy from (nothing in the tray, not in the edit view). `None` for every
/// other command; asked by `crate::menus` before the engine's `enabled`.
pub fn disabled(app: &App, id: &str) -> Option<String> {
    let none = id == "edit.copyRecipe" && super::edit_view::open_photo(app).is_none() && copy_source(app).is_none();
    none.then(|| NO_PHOTO.to_string())
}

/// The photo Copy All Edits copies from in the library: the tray's only photo, else the photo last
/// clicked in the selection, else the tray's first.
fn copy_source(app: &App) -> Option<PhotoId> {
    let state = app.state();
    let tray = crate::tray::photos(state);
    if tray.len() == 1 {
        return tray.first().copied();
    }
    state.selection.anchor.filter(|a| tray.contains(a)).or_else(|| tray.first().copied())
}

// ----- Undo All Edits… -----

/// What Undo All Edits… is asked for: `ids` (a library step), or `id` with `history: true` (a step
/// of that photo's history); neither: the tray's photos, or the open photo in the edit view.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    id: Option<PhotoId>,
    #[serde(default)]
    history: bool,
}

/// The question Undo All Edits… asks for `params`: its draft (`ids`, or `id` and `history`, and
/// the question's text), or why it cannot ask.
fn question(app: &App, params: &Value) -> Result<serde_json::Map<String, Value>, CmdError> {
    let mut t: Target = parse_object(params)?;
    if t.ids.is_none() && t.id.is_none() && !t.history {
        let filled = params_for(app, UNDO_ALL.id).unwrap_or_else(|| json!({}));
        t = parse_object(&filled)?;
    }
    let mut draft = serde_json::Map::new();
    let ids = match (t.ids, t.id, t.history) {
        (Some(ids), None, false) => {
            draft.insert("ids".into(), json!(ids));
            ids
        }
        (None, Some(id), true) => {
            draft.insert("id".into(), json!(id));
            draft.insert("history".into(), json!(true));
            vec![id]
        }
        _ => return Err(CmdError::BadParams("give \"ids\", or \"id\" with \"history\": true".into())),
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let ids = super::albums::known_photos(app, ids)?;
    let session = app.session().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    let library = session.library();
    let edited = ids.iter().filter(|id| !library.info(**id).edits.is_empty()).count();
    if edited == 0 {
        return Err(CmdError::Disabled(dalica_engine::cmd::batch::NO_EDITS.into()));
    }
    let saved: Vec<String> =
        ids.iter().filter(|id| library.saved(**id).is_some()).filter_map(|id| library.photo(*id).map(|p| p.name.clone())).collect();
    draft.insert("question".into(), json!(dalica_engine::cmd::batch::question(edited, &saved)));
    Ok(draft)
}

fn check_undo_all(app: &App, params: &Value) -> Result<(), String> {
    question(app, params).map(|_| ()).map_err(|e| e.to_string())
}

fn open_undo_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let draft = question(app, params)?;
    undo_all::open(app, draft)
}

// ----- What Paste skipped -----

/// After command `id` ran: a library Paste All Edits that skipped photos says which and why, as one
/// caution toast ("Pasted the edits onto 2 photos; skipped 1: IMG_0003.jpg: …").
pub fn ran(app: &mut App, id: &str, result: &Result<Value, CmdError>) {
    if id != "edit.pasteRecipe" {
        return;
    }
    let Ok(reply) = result else { return };
    let Some(skipped) = reply.get("skipped").and_then(Value::as_array).filter(|s| !s.is_empty()) else { return };
    let changed = reply.get("ids").and_then(Value::as_array).map_or(0, Vec::len);
    let text = skipped_text(app, changed, skipped);
    app.toast(&text, ToastKind::Caution);
}

/// The toast's text for a paste that changed `changed` photos and skipped `skipped`
/// (`[{id, reason}]`): the first [`NAMED`] by name with their reasons, then how many more.
fn skipped_text(app: &App, changed: usize, skipped: &[Value]) -> String {
    let photos = |n: usize| if n == 1 { "1 photo".to_string() } else { format!("{n} photos") };
    let name = |v: &Value| {
        let id = v.get("id").and_then(Value::as_u64).map(PhotoId::new);
        let named = id.and_then(|id| app.session().and_then(|s| s.library().photo(id).map(|p| p.name.clone())));
        named.unwrap_or_else(|| id.map_or_else(|| "a photo".to_string(), |id| format!("photo {id}")))
    };
    let mut parts: Vec<String> = skipped
        .iter()
        .take(NAMED)
        .map(|v| match v.get("reason").and_then(Value::as_str) {
            Some(why) => format!("{}: {why}", name(v)),
            None => name(v),
        })
        .collect();
    if skipped.len() > NAMED {
        parts.push(format!("and {} more", skipped.len() - NAMED));
    }
    format!("Pasted the edits onto {}; skipped {}: {}", photos(changed), skipped.len(), parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_batch_commands_take_the_tray_and_copy_takes_one_photo() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        app.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        app.run("tray.hold", &json!({})).unwrap();
        app.run("select.set", &json!({ "ids": [ids[1], ids[2]] })).unwrap();
        for id in TRAY {
            assert_eq!(params_for(&app, id), Some(json!({ "ids": [ids[0], ids[1], ids[2]] })), "{id}");
        }
        assert_eq!(params_for(&app, "edit.copyRecipe"), Some(json!({ "id": ids[2] })), "the photo last clicked");
        assert_eq!(params_for(&app, "photo.star.toggle"), None);
        assert_eq!(disabled(&app, "edit.copyRecipe"), None);
        app.run("tray.clear", &json!({ "confirm": true })).unwrap();
        app.run("select.none", &json!({})).unwrap();
        assert_eq!(params_for(&app, "edit.copyRecipe"), Some(json!({})), "nothing to copy from");
        assert_eq!(disabled(&app, "edit.copyRecipe").as_deref(), Some(NO_PHOTO));
        assert_eq!(disabled(&app, "edit.pasteRecipe"), None);
    }

    #[test]
    fn the_toast_names_the_skipped_photos_with_their_reasons() {
        let (_dir, s, ids) = session_with_photos(5);
        let app = App::new(Some(s));
        let name = |i: usize| app.session().unwrap().library().photo(ids[i]).unwrap().name.clone();
        let skipped: Vec<Value> = (0..5).map(|i| json!({ "id": ids[i], "reason": "why" })).collect();
        let text = skipped_text(&app, 1, &skipped);
        assert!(text.starts_with(&format!("Pasted the edits onto 1 photo; skipped 5: {}: why; ", name(0))), "{text}");
        assert!(text.ends_with("; and 2 more"), "{text}");
    }
}
