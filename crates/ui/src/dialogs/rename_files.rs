//! Rename Files (`organizing.md` §9): File ▸ Rename… and F2 (`photo.rename` without a name: the
//! selection) open it. Its draft is the view state's `renameFiles` ([`RenameDraft`]).
//!
//! ```text
//! Rename Files
//! 3 files selected. Enter a new name:
//! [Lisbon                                        ]
//! Example: Lisbon-001.jpg
//! [ ] Add the date taken to the name
//! [ ] Add the image size to the name
//!                                     [Cancel] [Rename]
//! ```
//!
//! The name field (`field:dialog.name`) has the focus when the dialog opens and starts empty. The
//! example line follows the typing and the two boxes (`check:dialog.addDate`,
//! `check:dialog.addSize`): it is the name the first photo (in light-table order) gets, worked out
//! by the engine's own [`rename::wanted`] and, for several photos, its first number
//! (`names::numbered(…, 1)`), so it reads exactly as the files will; with the field empty it shows
//! the first photo's name as it is. Rename (the default, enabled while the name is not blank) runs
//! `photo.rename {ids, name, addDate, addSize}`: one undo step, several files getting `-001`,
//! `-002` … in display order (the engine's, M9.14).

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use dalica_core::names;
use dalica_core::{DateTime, FolderId, PhotoId};
use dalica_engine::CmdError;
use dalica_engine::cmd::rename;
use dalica_library::{Library, Photo};
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind, TextField};
use egui::{Id, Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::RenameDraft;

/// The dialog's title, its default button and its width (points).
pub const TITLE: &str = "Rename Files";
pub const RENAME: &str = "Rename";
pub const WIDTH: f32 = 420.0;
/// The checkboxes' labels.
pub const ADD_DATE: &str = "Add the date taken to the name";
pub const ADD_SIZE: &str = "Add the image size to the name";
/// The widgets (`control-protocol.md` §7).
pub const NAME_ID: &str = "field:dialog.name";
pub const ADD_DATE_ID: &str = "check:dialog.addDate";
pub const ADD_SIZE_ID: &str = "check:dialog.addSize";
/// What the name field says while it is empty.
pub const NAME_HINT: &str = "New name";
/// Between the rows (points).
const GAP: f32 = 8.0;

/// Opens the dialog with `draft`. Replies `{dialog: "renameFiles"}`.
pub fn open(app: &mut App, draft: RenameDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::RenameFiles))?;
    app.state_mut().rename_files = Some(draft.sanitized());
    Ok(reply)
}

/// "%d files selected. Enter a new name:" (one file: "1 file selected.").
pub fn heading(n: usize) -> String {
    let noun = if n == 1 { "file" } else { "files" };
    format!("{n} {noun} selected. Enter a new name:")
}

/// Why Rename cannot run with `draft`, if it cannot: a blank name, no photos.
pub fn ready(draft: &RenameDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        Err("Enter a new name.".into())
    } else if draft.ids.is_empty() {
        Err("Select some photos first.".into())
    } else {
        Ok(())
    }
}

/// `photo.rename`'s parameters for `draft`.
pub fn run_params(draft: &RenameDraft) -> Value {
    json!({ "ids": draft.ids, "name": draft.name.trim(), "addDate": draft.add_date, "addSize": draft.add_size })
}

/// The first of `ids` in light-table order, as the engine numbers a batch: the folders newest
/// first (by their oldest photo's date; folders without one last), then by path; in a folder, its
/// own sort. Two passes over the library: once per set of photos, never per frame.
pub fn first_in_order(lib: &Library, ids: &[PhotoId]) -> Option<PhotoId> {
    let wanted: HashSet<PhotoId> = ids.iter().copied().collect();
    let mut dates: HashMap<FolderId, Option<DateTime>> = ids.iter().filter_map(|id| lib.photo(*id)).map(|p| (p.folder, None)).collect();
    for p in lib.photos() {
        if let Some(slot) = dates.get_mut(&p.folder)
            && let Some(d) = lib.sort_date(p)
        {
            *slot = Some(slot.map_or(d, |s| s.min(d)));
        }
    }
    let folder = dates.into_iter().map(|(f, d)| (Reverse(d), lib.folder(f).map(|f| f.path.clone()), f)).min()?.2;
    lib.photos_sorted(folder).into_iter().find(|id| wanted.contains(id))
}

/// The name the example line shows for `photo` (the batch's first) with `draft`: the engine's
/// [`rename::wanted`], numbered `-001` when several photos are renamed; with no name typed, the
/// photo's own name with what the boxes add.
pub fn example_name(lib: &Library, photo: &Photo, draft: &RenameDraft) -> String {
    let typed = draft.name.trim();
    if typed.is_empty() {
        return rename::wanted(lib, photo, "", draft.add_date, draft.add_size, false);
    }
    let several = draft.ids.len() > 1;
    let name = rename::wanted(lib, photo, typed, draft.add_date, draft.add_size, several);
    if several { names::numbered(&name, 1) } else { name }
}

/// "Example: Lisbon-001.jpg", or nothing while there is no photo to show.
pub fn example(lib: &Library, first: Option<PhotoId>, draft: &RenameDraft) -> String {
    match first.and_then(|id| lib.photo(id)) {
        Some(photo) => format!("Example: {}", example_name(lib, photo, draft)),
        None => String::new(),
    }
}

/// What the dialog says for `draft` when `first` is its batch's first photo: the heading and the
/// example line.
fn lines(app: &App, draft: &RenameDraft, first: Option<PhotoId>) -> (String, String) {
    (heading(draft.ids.len()), app.session().map(|s| example(s.library(), first, draft)).unwrap_or_default())
}

/// What the open dialog says now: "%d files selected. Enter a new name:" and the example line
/// (`None` while it is closed). The body draws exactly these.
pub fn texts(app: &App) -> Option<(String, String)> {
    let d = app.state().rename_files.as_ref()?;
    let first = app.session().and_then(|s| first_in_order(s.library(), &d.ids));
    Some(lines(app, d, first))
}

/// The batch's first photo, worked out once per set of photos (kept in egui's memory, keyed by
/// their number and ends): the example line reads it every frame.
fn first_cached(app: &App, ui: &Ui, draft: &RenameDraft) -> Option<PhotoId> {
    type Key = (usize, Option<PhotoId>, Option<PhotoId>);
    let key: Key = (draft.ids.len(), draft.ids.first().copied(), draft.ids.last().copied());
    let slot = Id::new(NAME_ID).with("renameFiles.first");
    if let Some((k, first)) = ui.ctx().data(|m| m.get_temp::<(Key, Option<PhotoId>)>(slot))
        && k == key
    {
        return first;
    }
    let first = app.session().and_then(|s| first_in_order(s.library(), &draft.ids));
    ui.ctx().data_mut(|m| m.insert_temp(slot, (key, first)));
    first
}

/// Whether this is the dialog's first or second pass since it opened: the name field asks for the
/// focus then (the window takes no input on its very first pass, while egui sizes it).
fn opening(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    let seen = Id::new(NAME_ID).with("renameFiles.passes");
    let pass = ctx.cumulative_pass_nr();
    let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
        Some((first, last)) if last.checked_add(1) == Some(pass) => first,
        _ => pass,
    };
    ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
    pass.saturating_sub(first) < 2
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ok = app.state().rename_files.as_ref().is_some_and(|d| ready(d).is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("rename", RENAME, ButtonKind::Default).enabled(ok)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(before) = app.state().rename_files.clone() else { return };
    let t = *app.tokens();
    let mut d = before.clone();
    ui.spacing_mut().item_spacing.y = GAP;
    ui.label(RichText::new(heading(d.ids.len())).font(fonts::regular(t.body)).color(t.text_label));
    if opening(ui) {
        widgets::focus_field(ui.ctx(), NAME_ID);
    }
    TextField::new(NAME_ID, NAME_HINT, ui.available_width()).show(ui, &t, &mut d.name);
    let first = first_cached(app, ui, &d);
    let (_, example) = lines(app, &d, first);
    ui.add(Label::new(RichText::new(example).font(fonts::regular(t.detail)).color(t.text_dim)).truncate());
    // The two boxes as one group of lines.
    ui.add_space(4.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    widgets::checkbox(ui, &t, ADD_DATE_ID, ADD_DATE, &mut d.add_date);
    widgets::checkbox(ui, &t, ADD_SIZE_ID, ADD_SIZE, &mut d.add_size);
    if d != before {
        app.state_mut().rename_files = Some(d.sanitized());
    }
}

/// Rename: renames the photos (one undo step).
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().rename_files.clone().ok_or_else(|| CmdError::Internal("Rename Files has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    app.run("photo.rename", &run_params(&draft))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs;

    #[test]
    fn rename_runs_photo_rename_with_the_draft() {
        assert_eq!(heading(1), "1 file selected. Enter a new name:");
        assert_eq!(heading(3), "3 files selected. Enter a new name:");
        let d = RenameDraft { ids: vec![PhotoId::new(1), PhotoId::new(2)], name: " Lisbon ".into(), add_date: true, add_size: false };
        assert_eq!(run_params(&d), json!({ "ids": [1, 2], "name": "Lisbon", "addDate": true, "addSize": false }));
        let mut app = App::new(None);
        open(&mut app, RenameDraft { ids: vec![PhotoId::new(1)], ..RenameDraft::default() }).unwrap();
        let v = dialogs::inspect(&app);
        assert_eq!((v["kind"].as_str(), v["default"].as_str()), (Some("renameFiles"), Some("button:dialog.rename")));
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "Rename waits for a name");
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().rename_files, None);
    }

    #[test]
    fn the_example_is_the_engines_name_for_the_first_photo() {
        let (_dir, s, ids) = session_with_photos(3);
        let lib = s.library();
        let first = first_in_order(lib, &[ids[2], ids[0], ids[1]]).unwrap();
        assert!(ids.contains(&first));
        let photo = lib.photo(first).unwrap();
        let ext = names::split_ext(&photo.name).1.map(|e| format!(".{e}")).unwrap_or_default();
        let mut d = RenameDraft { ids: ids.clone(), ..RenameDraft::default() };
        assert_eq!(example(lib, Some(first), &d), format!("Example: {}", photo.name), "nothing typed: the name as it is");
        d.name = "Lisbon".into();
        assert_eq!(example(lib, Some(first), &d), format!("Example: Lisbon-001{ext}"));
        d.ids.truncate(1);
        assert_eq!(example(lib, Some(first), &d), format!("Example: Lisbon{ext}"), "one photo: no number");
        assert_eq!(example(lib, None, &d), "", "no photo, no example");
        assert_eq!(first_in_order(lib, &[]), None);
        assert_eq!(first_in_order(lib, &[PhotoId::new(9999)]), None);
    }
}
