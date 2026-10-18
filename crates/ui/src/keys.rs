//! Shortcut dispatch (`menus-and-shortcuts.md` §2.0): each key press of the frame becomes the
//! keymap's [`Key`], is resolved in the active context and runs its command through [`App::run`]
//! with the parameters the context implies ([`params_for`]: the photos it acts on). While a text
//! field has the focus a key resolves Text input, then Global: the view's shortcuts are off while
//! typing. Plain arrows go to the library tree instead while it has the focus. [`join_keymap`]
//! adds the UI commands to the session's keymap, so their keys are resolved, conflict-checked and
//! rebindable like the engine's; the openers' keys (F2 for Rename…, `crate::commands::OPENERS`)
//! join them, and a key bound to an opener opens its dialog.
//!
//! The Delete key is routed by section (`menus-and-shortcuts.md` §2.2), as Ctrl+1 is by view: in
//! an album section Delete removes the selected photos from the album (`album.remove`, which asks
//! first), anywhere else it deletes them from disk (`photo.delete`, which asks first);
//! Ctrl+Delete always deletes from disk ([`delete_route`]). While the library tree has the focus
//! and its selected row is a library folder, Delete is Folder ▸ Delete… (`folder.delete`, which
//! asks first; `ui-layout.md` §4, [`tree_delete_route`]).
//!
//! While a key cell of the shortcut editor records (Options ▸ Keyboard Shortcuts), every key press
//! is the editor's ([`capture_for_editor`]): neither the keymap nor the dialog's Esc and Enter see
//! it.
//!
//! The view context follows what fills the window ([`view_context`], `menus-and-shortcuts.md`
//! §2.0): a slideshow resolves keys in Slideshow, the photo viewer in Photo viewer, the Import room
//! in front in Import, the library in Library (the edit view resolves its own, the open tool first,
//! `crate::commands::edit_view::dispatch`). Only the library routes Delete by section and to the
//! focused tree.

use dalica_engine::keymap::DefaultKeys;
use dalica_engine::{Context, Key, Session};
use serde_json::{Value, json};

use crate::app::{App, UI_COMMANDS};

/// Every UI command with its default keys, and every opener with its own, as the session's
/// keymap takes them.
pub fn ui_keys() -> Vec<(&'static str, DefaultKeys)> {
    let openers = crate::commands::OPENERS.iter().filter(|o| !o.keys.is_empty()).map(|o| (o.id, o.keys));
    UI_COMMANDS.iter().map(|c| (c.id, c.keys)).chain(openers).collect()
}

/// The command a press of `key` runs in the library, given what the keymap binds it to: Delete
/// and Ctrl+Delete are routed by section ([`delete_route`]); every other key runs its binding.
pub fn route(app: &App, key: &Key, bound: Option<String>) -> Option<String> {
    if let Some(routed) = delete_route(app, key, bound.as_deref()) {
        return Some(routed.to_string());
    }
    // The Unnamed grid shows no photos: Delete there never deletes the selected photos.
    let text = key.to_string();
    let deletes = (text == "Delete" || text == "Ctrl+Delete") && bound.as_deref().is_none_or(|b| b == "photo.delete" || b == "album.remove");
    if deletes && app.state().unnamed.open {
        return None;
    }
    bound
}

/// What Delete and Ctrl+Delete run (`menus-and-shortcuts.md` §2.2), unless the user bound them to
/// another command: Delete in an album section is Remove from Album (`album.remove`), elsewhere
/// Delete from Disk (`photo.delete`); Ctrl+Delete is always Delete from Disk. While the Unnamed
/// grid shows with faces selected, Delete ignores them (`faces.ignoreSelected`, `organizing.md`
/// §6.2) and Ctrl+Delete does nothing. `None` for other keys and for a Delete key the user gave
/// another command.
pub fn delete_route(app: &App, key: &Key, bound: Option<&str>) -> Option<&'static str> {
    let text = key.to_string();
    let (plain, ctrl) = (text == "Delete", text == "Ctrl+Delete");
    if !(plain || ctrl) || bound.is_some_and(|b| b != "photo.delete" && b != "album.remove") {
        return None;
    }
    let unnamed = &app.state().unnamed;
    if unnamed.open {
        return (plain && !unnamed.selection.is_empty()).then_some(FACES_DELETE);
    }
    let in_album = app.state().selection.section.is_some_and(crate::state::Section::is_album);
    Some(if plain && in_album { "album.remove" } else { "photo.delete" })
}

/// What Delete runs in the Unnamed grid with faces selected.
pub const FACES_DELETE: &str = "faces.ignoreSelected";

/// What Delete runs while the library tree has the focus (`tree_focused`): Folder ▸ Delete…
/// (`folder.delete`) when the tree's selected row is a library folder (the selection is not in an
/// album) and the user did not give Delete another command; `None` otherwise (the key goes to
/// [`route`]).
pub fn tree_delete_route(app: &App, key: &Key, bound: Option<&str>, tree_focused: bool) -> Option<&'static str> {
    let plain = key.to_string() == "Delete";
    if !(tree_focused && plain) || bound.is_some_and(|b| b != "photo.delete" && b != "album.remove") {
        return None;
    }
    let state = app.state();
    if state.selection.section.is_some_and(crate::state::Section::is_album) {
        return None;
    }
    crate::menus::current_folder(app).filter(|f| app.session().is_some_and(|s| s.library().folder(*f).is_some())).map(|_| "folder.delete")
}

/// Adds the UI commands to `session`'s keymap (`Session::add_ui_keys`). A default that does not
/// parse is a bug the tests catch; here it is logged and the engine's keys keep working.
pub fn join_keymap(session: &mut Session) {
    if let Err(e) = session.add_ui_keys(&ui_keys()) {
        log::error!("a UI command's default shortcut does not parse: {e}");
    }
}

/// The keymap's key for an egui key press, if it has one. `command` is the primary modifier (Ctrl,
/// or Cmd on macOS).
pub fn from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Option<Key> {
    let code = match key {
        egui::Key::Escape => "Esc".to_string(),
        egui::Key::Plus => "Plus".to_string(),
        // egui names Minus by the Unicode minus sign, which the keymap does not take.
        egui::Key::Minus => "-".to_string(),
        // Punctuation goes by its character (`Ctrl+,`); arrows have symbols too, but go by name.
        k if k.symbol_or_name().len() == 1 && k.symbol_or_name().bytes().all(|b| b.is_ascii_punctuation()) => k.symbol_or_name().to_string(),
        k => k.name().to_string(),
    };
    let mut text = String::new();
    for (on, name) in [(modifiers.command, "Ctrl+"), (modifiers.alt, "Alt+"), (modifiers.shift, "Shift+")] {
        if on {
            text.push_str(name);
        }
    }
    text.push_str(&code);
    Key::parse(&text).ok()
}

/// The photo commands that act on the Photo Tray's photos (the held ones and the selection,
/// [`crate::tray::photos`]) rather than only the selection: the star and rotate buttons under the
/// tray act on it (`ui-layout.md` §6.3), and their shortcuts and menu items act on the same set;
/// so do the quick tags' Ctrl+Alt+1 … Ctrl+Alt+0 (`organizing.md` §5.2).
pub const TRAY_COMMANDS: [&str; 14] = [
    "photo.star.toggle",
    "photo.star.set",
    "photo.rotate.cw",
    "photo.rotate.ccw",
    "tags.quick.1",
    "tags.quick.2",
    "tags.quick.3",
    "tags.quick.4",
    "tags.quick.5",
    "tags.quick.6",
    "tags.quick.7",
    "tags.quick.8",
    "tags.quick.9",
    "tags.quick.10",
];

/// Whether command `id` acts on the tray's photos ([`TRAY_COMMANDS`]).
pub fn acts_on_tray(id: &str) -> bool {
    TRAY_COMMANDS.contains(&id)
}

/// The photos command `id` acts on when run from a shortcut, a menu or a bottom button: the tray's
/// for [`TRAY_COMMANDS`], else the selection.
pub fn targets(app: &App, id: &str) -> Vec<dalica_core::PhotoId> {
    let state = app.state();
    if acts_on_tray(id) { crate::tray::photos(state) } else { state.selection.ids.clone() }
}

/// The parameters a shortcut passes to `id`: for commands that take `ids`, the photos it acts on
/// ([`targets`]); Remove from Album passes the album of the selected section too, and Delete from
/// Disk its photos whether or not the engine has registered it yet (the question counts them).
pub fn params_for(app: &App, id: &str) -> Value {
    if let Some(p) = crate::commands::batch::params_for(app, id).or_else(|| crate::commands::places::params_for(app, id)) {
        return p;
    }
    let takes_ids = app.session().and_then(|s| s.spec(id)).is_some_and(|spec| spec.params.contains("\"ids\""));
    match id {
        "album.remove" => {
            let album = app.state().selection.section.and_then(|s| s.to_string().strip_prefix("album:").map(str::to_string));
            json!({ "album": album, "ids": targets(app, id) })
        }
        "photo.delete" => json!({ "ids": targets(app, id) }),
        "folder.delete" => crate::menus::current_folder(app).map_or_else(|| json!({}), |f| json!({ "folder": f })),
        _ if takes_ids => json!({ "ids": targets(app, id) }),
        _ => json!({}),
    }
}

/// Runs `id` as a key press runs it: an opener opens its dialog, any other command runs through
/// [`App::run`] with [`params_for`].
pub fn run_key(app: &mut App, id: &str) -> Result<Value, dalica_engine::CmdError> {
    let params = params_for(app, id);
    match crate::commands::opener(id) {
        Some(o) => {
            let r = (o.open)(app, &params);
            if let Err(e) = &r {
                app.state_mut().status = e.to_string();
            }
            r
        }
        None => app.run(id, &params),
    }
}

/// While a key cell of the shortcut editor records (`options.shortcuts.recording`,
/// `menus-and-shortcuts.md` §2.0), takes this frame's key presses out of the input before the
/// dialog host (Esc, Enter), the keymap and any field see them, and keeps them, as the keymap
/// writes them, in the editor's `captured` keys (at most [`MAX_CAPTURED`], the newest) for it to
/// read: Esc cancels the recording there and Backspace removes the key. Typed text and clipboard
/// events go with them; keys the keymap has no name for are dropped. Returns whether it took the
/// frame's keys. Call at the start of the frame.
///
/// [`MAX_CAPTURED`]: crate::state::MAX_CAPTURED
pub fn capture_for_editor(app: &mut App, ctx: &egui::Context) -> bool {
    let Some(editor) = app.state_mut().options.as_mut().map(|o| &mut o.shortcuts).filter(|e| e.recording.is_some()) else {
        return false;
    };
    let presses: Vec<Key> = ctx.input_mut(|i| {
        let mut out = Vec::new();
        let mods = i.modifiers;
        i.events.retain(|e| match e {
            egui::Event::Key { key, pressed, modifiers, .. } => {
                if *pressed && let Some(k) = from_egui(*key, *modifiers) {
                    out.push(k);
                }
                false
            }
            // The window turns Ctrl+C, Ctrl+X and Ctrl+V (Cmd on macOS) into these, without the
            // key press: the key they came from, so they can be recorded (LightCraft's `capture`).
            egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => {
                let key = match e {
                    egui::Event::Copy => egui::Key::C,
                    egui::Event::Cut => egui::Key::X,
                    _ => egui::Key::V,
                };
                out.extend(from_egui(key, mods));
                false
            }
            egui::Event::Text(_) => false,
            _ => true,
        });
        out
    });
    editor.captured.extend(presses);
    let extra = editor.captured.len().saturating_sub(crate::state::MAX_CAPTURED);
    editor.captured.drain(..extra);
    true
}

/// The view context a key press is resolved in (then Global): what fills the window. A slideshow and
/// the photo viewer fill it whatever room is in front; otherwise the room in front, and in the
/// Library tab the library or the edit view.
pub fn view_context(state: &crate::state::UiState) -> Context {
    use crate::state::{Room, View};
    match (state.view, state.room) {
        (View::Slideshow, _) => Context::Slideshow,
        (View::Viewer, _) => Context::PhotoViewer,
        (_, Room::Import) => Context::Import,
        (View::Edit, Room::Library) => Context::EditView,
        (View::Library, Room::Library) => Context::Library,
    }
}

/// Runs the commands bound to this frame's key presses in the [view context](view_context) (the
/// library, a slideshow, the photo viewer, a room; not the edit view, which dispatches its own).
/// Returns the ids that ran (or were refused; a refusal's reason is on the status line).
pub fn dispatch(app: &mut App, ctx: &egui::Context) -> Vec<String> {
    let context = view_context(app.state());
    let library = context == Context::Library;
    // With the library tree focused, its pane moves its own selection with the plain arrows.
    let tree_focused = library && crate::panels::folders::has_focus(ctx);
    let presses: Vec<Key> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if tree_focused && crate::panels::folders::is_tree_key(*key, *modifiers) => {
                    None
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => from_egui(*key, *modifiers),
                _ => None,
            })
            .collect()
    });
    let text_input = ctx.egui_wants_keyboard_input();
    // While a text field has the focus, the view's shortcuts are off (Ctrl+8, Ctrl+A, Ctrl+Z are
    // the field's or nothing): Text input, then Global.
    let view = if text_input { Context::Global } else { context };
    let mut ran = Vec::new();
    for key in presses {
        let bound = app.session().and_then(|s| s.keymap().resolve(&key, view, text_input));
        // Delete and Ctrl+Delete are the library's, never a text field's; Delete in the focused
        // tree is the folder's.
        let routed = if text_input || !library {
            bound
        } else {
            tree_delete_route(app, &key, bound.as_deref(), tree_focused).map(str::to_string).or_else(|| route(app, &key, bound))
        };
        let Some(id) = routed else { continue };
        let _ = run_key(app, &id);
        ran.push(id);
    }
    ran
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    #[test]
    fn egui_presses_become_keymap_keys() {
        let ctrl = Modifiers { command: true, ctrl: true, ..Modifiers::NONE };
        for (key, mods, want) in [
            (egui::Key::Num8, ctrl, "Ctrl+8"),
            (egui::Key::F4, Modifiers::NONE, "F4"),
            (egui::Key::Escape, Modifiers::NONE, "Esc"),
            (egui::Key::Z, Modifiers { shift: true, ..ctrl }, "Ctrl+Shift+Z"),
            (egui::Key::Comma, ctrl, "Ctrl+,"),
            (egui::Key::Plus, ctrl, "Ctrl+Plus"),
            (egui::Key::Minus, ctrl, "Ctrl+-"),
            (egui::Key::ArrowUp, Modifiers::SHIFT, "Shift+Up"),
            (egui::Key::A, Modifiers::ALT, "Alt+A"),
        ] {
            assert_eq!(from_egui(key, mods).map(|k| k.to_string()).as_deref(), Some(want), "{key:?}");
        }
        // Keys the keymap has no name for are ignored, never a panic.
        assert_eq!(from_egui(egui::Key::Copy, Modifiers::NONE), None);
    }

    /// While a text field has the focus the view's shortcuts are off (`menus-and-shortcuts.md`
    /// §2.0): Ctrl+8, Ctrl+A and Ctrl+Z typed into the caption editor star, select and undo
    /// nothing in the library, while Global's F11 still works.
    #[test]
    fn the_views_shortcuts_are_off_while_a_field_has_the_focus() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(3);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        d.run("photo.star.toggle", &json!({ "ids": [ids[2].get()] })).unwrap();
        d.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
        d.run("photo.caption.edit", &json!({})).unwrap();
        d.settle(3600).unwrap();
        assert!(d.ctx().egui_wants_keyboard_input(), "the editor has the focus");
        let starred = |d: &crate::headless::Driver| -> Vec<bool> {
            let lib = d.app().session().unwrap().library();
            ids.iter().map(|id| lib.photo(*id).unwrap().star).collect()
        };
        for key in ["Ctrl+8", "Ctrl+A", "Ctrl+Z"] {
            d.key(key).unwrap();
        }
        assert_eq!(starred(&d), [false, false, true], "no star set, none undone");
        assert_eq!(d.state().selection.ids, vec![ids[0]], "nothing selected");
        assert_eq!(d.app().session().unwrap().undo_stack().len(), 1);
        d.key("F11").unwrap();
        assert!(d.app().state().full_screen, "Global keys still work");
        assert!(d.ctx().egui_wants_keyboard_input(), "still typing");
    }

    /// F2 (an opener's key) opens Rename Files for the selection; Delete and Ctrl+Delete follow
    /// the section.
    #[test]
    fn f2_opens_rename_files_and_delete_follows_the_section() {
        assert!(ui_keys().iter().any(|(id, keys)| *id == "photo.rename" && keys.iter().any(|(_, k)| *k == "F2")));
        let (_dir, s, ids) = crate::app::tests::session_with_photos(2);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        d.key("F2").unwrap();
        assert!(d.app().state().dialogs.is_empty(), "nothing selected: nothing to rename");
        d.run("select.set", &json!({ "ids": [ids[1]] })).unwrap();
        d.key("F2").unwrap();
        assert_eq!(d.app().state().rename_files.as_ref().map(|r| r.ids.clone()), Some(vec![ids[1]]));
        let (del, ctrl) = (Key::parse("Delete").unwrap(), Key::parse("Ctrl+Delete").unwrap());
        assert_eq!(delete_route(d.app(), &del, None), Some("photo.delete"));
        assert_eq!(delete_route(d.app(), &Key::parse("F2").unwrap(), None), None);
        let album = crate::state::Section::Album(crate::state::AlbumSection::Search);
        d.app_mut().state_mut().selection = crate::state::Selection::new(vec![ids[0]], None, Some(album), false);
        assert_eq!(delete_route(d.app(), &del, Some("photo.delete")), Some("album.remove"));
        assert_eq!(delete_route(d.app(), &ctrl, None), Some("photo.delete"));
        assert_eq!(delete_route(d.app(), &del, Some("pane.toggle")), None, "the user's own binding");
    }

    /// Delete on a focused tree's folder row asks Folder ▸ Delete…'s question for that folder; in the
    /// light table it still deletes the selected photos.
    #[test]
    fn delete_on_a_focused_folder_row_asks_to_delete_the_folder() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(2);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        let del = Key::parse("Delete").unwrap();
        assert_eq!(tree_delete_route(d.app(), &del, None, false), None, "the tree has no focus");
        d.click(&format!("tree:folder:{}", folder.get())).unwrap();
        d.settle(3600).unwrap();
        assert!(crate::panels::folders::has_focus(d.ctx()), "a click gives the tree the focus");
        assert_eq!(tree_delete_route(d.app(), &del, None, true), Some("folder.delete"));
        assert_eq!(tree_delete_route(d.app(), &Key::parse("Ctrl+Delete").unwrap(), None, true), None, "Ctrl+Delete deletes photos");
        assert_eq!(tree_delete_route(d.app(), &del, Some("pane.toggle"), true), None, "the user's own binding");
        d.key("Delete").unwrap();
        assert_eq!(d.app().state().delete_folder, Some(crate::state::DeleteFolder { folder }));
        assert_eq!(d.app().state().dialogs.last().map(|x| x.kind), Some(crate::state::DialogKind::DeleteFolder));
        d.key("Esc").unwrap();
        assert!(d.app().state().dialogs.is_empty(), "Esc keeps the folder");
        assert!(d.app().session().unwrap().library().folder(folder).is_some());
        // An album selected in the tree is no folder to delete.
        let album = crate::state::Section::Album(crate::state::AlbumSection::Starred);
        d.app_mut().state_mut().selection = crate::state::Selection::new(vec![], None, Some(album), true);
        assert_eq!(tree_delete_route(d.app(), &del, None, true), None);
    }

    /// While a key cell of the shortcut editor records, Esc, Enter and Ctrl+8 go to the editor:
    /// the dialog stays open and nothing is starred; once it stops, Esc closes the dialog again.
    #[test]
    fn a_recording_key_cell_takes_every_key_press() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(1);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        d.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        d.run("app.shortcuts", &json!({})).unwrap();
        d.settle(3600).unwrap();
        let cell = crate::state::KeySlot { command: "photo.star.toggle".into(), context: Context::Library, slot: 1 };
        let record = |d: &mut crate::headless::Driver| d.app_mut().state_mut().options.as_mut().unwrap().shortcuts.recording = Some(cell.clone());
        // Enter is View and Edit's in the Library: the editor asks, the dialog's Enter never runs.
        record(&mut d);
        d.key("Enter").unwrap();
        let editor = d.state().options.as_ref().unwrap().shortcuts.clone();
        assert_eq!(editor.conflict.map(|c| c.command).as_deref(), Some("view.edit"));
        assert_eq!(d.state().dialogs.len(), 1, "Enter did not close the dialog");
        // Esc cancels the recording, not the dialog.
        d.key("Esc").unwrap();
        assert!(d.state().options.as_ref().unwrap().shortcuts.recording.is_none());
        assert_eq!(d.state().dialogs.len(), 1, "Esc did not close the dialog");
        // Ctrl+8 goes to the cell, not the keymap.
        record(&mut d);
        d.key("Ctrl+8").unwrap();
        assert!(!d.app().session().unwrap().library().photo(ids[0]).unwrap().star, "Ctrl+8 starred nothing");
        d.key("Esc").unwrap();
        assert!(d.state().dialogs.is_empty() && d.state().options.is_none());
    }

    /// The window reports Ctrl+C, Ctrl+X and Ctrl+V as copy, cut and paste events without their key
    /// presses: a recording cell takes them as those keys.
    #[test]
    fn a_recording_cell_takes_copy_cut_and_paste_as_their_keys() {
        let mut app = App::new(None);
        let mut draft = crate::state::OptionsDraft::on(crate::state::OptionsTab::Shortcuts);
        draft.shortcuts.recording = Some(crate::state::KeySlot { command: "photo.star.toggle".into(), context: Context::Library, slot: 0 });
        app.state_mut().options = Some(draft);
        let ctx = egui::Context::default();
        let events = vec![egui::Event::ModifiersChanged(Modifiers::COMMAND), egui::Event::Copy, egui::Event::Cut, egui::Event::Paste("x".into())];
        ctx.begin_pass(egui::RawInput { events, ..Default::default() });
        assert!(capture_for_editor(&mut app, &ctx));
        let left = ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))).count());
        assert_eq!(left, 0, "nothing else sees them");
        ctx.end_pass().textures_delta.clear();
        let captured: Vec<String> = app.state().options.as_ref().unwrap().shortcuts.captured.iter().map(Key::to_string).collect();
        assert_eq!(captured, ["Ctrl+C", "Ctrl+X", "Ctrl+V"]);
    }

    /// In the Unnamed grid Delete ignores the faces selected and never deletes photos.
    #[test]
    fn delete_in_the_unnamed_grid_ignores_faces_and_never_deletes_photos() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(2);
        let mut app = App::new(Some(s));
        app.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        let delete = Key::parse("Delete").unwrap();
        let ctrl = Key::parse("Ctrl+Delete").unwrap();
        assert_eq!(route(&app, &delete, Some("photo.delete".into())).as_deref(), Some("photo.delete"));
        app.state_mut().unnamed.open = true;
        assert_eq!(route(&app, &delete, Some("photo.delete".into())), None, "no face selected");
        assert_eq!(route(&app, &ctrl, Some("photo.delete".into())), None);
        app.state_mut().unnamed.selection = vec![crate::state::FaceRef { photo: ids[0], index: 0 }];
        assert_eq!(route(&app, &delete, Some("photo.delete".into())).as_deref(), Some(FACES_DELETE));
        assert_eq!(route(&app, &ctrl, Some("photo.delete".into())), None);
        assert_eq!(route(&app, &delete, Some("photo.star.toggle".into())).as_deref(), Some("photo.star.toggle"), "a key the user rebound");
    }

    /// Ctrl+− reaches the keymap and runs the edit view's `edit.zoom.out` once (×1/1.25).
    #[test]
    fn ctrl_minus_zooms_the_photo_out_through_the_keymap() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(1);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        d.run("view.edit", &json!({ "id": ids[0].get() })).unwrap();
        d.settle(3600).unwrap();
        let key = from_egui(egui::Key::Minus, Modifiers { command: true, ctrl: true, ..Modifiers::NONE }).unwrap();
        let bound = d.app().session().unwrap().keymap().resolve(&key, Context::EditView, false);
        assert_eq!(bound.as_deref(), Some("edit.zoom.out"));
        d.app_mut().state_mut().edit.zoom = crate::state::Zoom::Scale(2.0);
        d.settle(3600).unwrap();
        d.key("Ctrl+-").unwrap();
        d.settle(3600).unwrap();
        let crate::state::Zoom::Scale(z) = d.app().state().edit.zoom else { panic!("still zoomed") };
        assert!((z - 2.0 / 1.25).abs() < 1e-4, "one step out, not two: {z}");
        assert_eq!(d.ctx().zoom_factor(), 1.0, "never egui's zoom of the whole window");
    }

    /// UI commands' keys run them like the engine's: F11, Ctrl+1 / Ctrl+2, Ctrl+0, and a rebinding
    /// moves the command to its new key.
    #[test]
    fn ui_command_keys_run_their_commands() {
        let (_dir, s, _) = crate::app::tests::session_with_photos(2);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.key("F11").unwrap();
        assert!(d.app().state().full_screen);
        d.key("F11").unwrap();
        assert!(!d.app().state().full_screen);
        d.key("Ctrl+1").unwrap();
        assert_eq!(d.app().state().thumb_size, crate::menus::THUMB_SMALL);
        d.key("Ctrl+2").unwrap();
        assert_eq!(d.app().state().thumb_size, crate::state::THUMB_DEFAULT);
        d.key("Ctrl+0").unwrap();
        assert_eq!(d.app().state().pane, Some(crate::state::Pane::Properties));
        d.key("Ctrl+0").unwrap();
        assert_eq!(d.app().state().pane, None);
        d.app_mut().run("app.setShortcut", &json!({ "command": "window.fullscreen", "context": "global", "key": "F12" })).unwrap();
        d.key("F11").unwrap();
        assert!(!d.app().state().full_screen, "F11 is no longer bound");
        d.key("F12").unwrap();
        assert!(d.app().state().full_screen);
    }
}
