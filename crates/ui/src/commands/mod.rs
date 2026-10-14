//! The UI-only commands ([`UI_COMMANDS`]): commands that change only the view state
//! (`architecture.md` §3). One module per area, each with its own `COMMANDS` list, joined here
//! once into [`UI_COMMANDS`]:
//!
//! - [`view`]: the View menu, the view-options popup, the tree's groups, the side pane, the window
//!   (thumbnail size, captions, small and hidden pictures, folder view and sort, group collapse,
//!   panes, full screen, quit);
//! - [`select`]: the light-table selection (`select.set`, `select.none`, `select.all`,
//!   `select.invert`, `select.section`, `select.starred`);
//! - [`scroll`]: scrolling the light table (the scroll bar's keys);
//! - [`tray`]: the Photo Tray (Hold, Clear);
//! - [`dialogs`]: commands that open an in-app dialog (`folders.manager`);
//! - [`caption`]: the inline caption editor (Picture ▸ Edit Caption);
//! - [`info`]: the dialogs that edit what is known about photos and folders (Folder Properties,
//!   Adjust Date and Time, Configure Quick Tags, Rename Tag);
//! - [`help`]: the Help menu's windows (About Dalica, Release Notes);
//! - [`edit_view`]: entering and leaving the edit view, its tabs and its panel toggle;
//! - [`zoom`]: the edit view's zoom (M7.15);
//! - [`tools`]: the edit view's tools, through one interface each tool module implements;
//! - [`export`]: File ▸ Export Pictures to Folder… (`output.export`, the Export dialog);
//! - [`save_as`]: File ▸ Save As… (`photo.saveAs`, the save picker);
//! - [`save`]: no UI command, but which photos the engine's Save, Undo Save, Revert and Save a
//!   Copy act on in the edit view ([`save::route`], called by [`App::run`]).
//!
//! A new command goes in its area's module (a new area gets a new module and one line in
//! [`AREAS`]); its id is the one the menu catalog uses, so its menu item goes live. Every command
//! refuses parameters it does not know with `bad_params` and changes nothing when it fails. A
//! command that cannot always run lists its check in its area's `CHECKS` ([`enabled`]): the menus
//! grey its item with the reason, and the command refuses with it (`disabled`).
//!
//! Each command that opens an editor or a dialog changes only the view state; what the editor or
//! dialog changes in the library is one engine command run when it is confirmed
//! (`photo.caption.set`, `folder.info.set`, `photo.dates.set`, `tags.quick.set`, `tags.rename`,
//! `export.run`, `photo.saveTo`), the way `folders.manager` runs `folders.set`.

pub mod caption;
pub mod dialogs;
pub mod edit_view;
pub mod export;
pub mod help;
pub mod info;
pub mod save;
pub mod save_as;
pub mod scroll;
pub mod select;
pub mod tools;
pub mod tray;
pub mod view;
pub mod zoom;

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::Value;

use crate::app::App;

pub use view::THUMB_SMALL;

/// A command that changes only the view state. Listed with the engine's commands so agents see one
/// catalogue.
#[derive(Clone, Copy)]
pub struct UiCommand {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path, `""` if not in a menu.
    pub menu: &'static str,
    /// Default bindings with their contexts, as `CommandSpec::keys` declares them (a `mac:` /
    /// `win,linux:` qualifier for a per-platform default). The app adds them to the session's
    /// keymap ([`crate::keys::join_keymap`]), so they are conflict-checked, rebindable with
    /// `app.setShortcut` and shown in the menus like the engine's.
    pub keys: &'static [(Context, &'static str)],
    /// Agent-readable parameter doc.
    pub params: &'static str,
    pub run: fn(&mut App, &Value) -> Result<Value, CmdError>,
}

/// Every area's commands, in the order [`UI_COMMANDS`] lists them.
const AREAS: &[&[UiCommand]] = &[
    select::COMMANDS,
    view::COMMANDS,
    scroll::COMMANDS,
    tray::COMMANDS,
    dialogs::COMMANDS,
    caption::COMMANDS,
    info::COMMANDS,
    help::COMMANDS,
    edit_view::COMMANDS,
    zoom::COMMANDS,
    tools::COMMANDS,
    export::COMMANDS,
    save_as::COMMANDS,
];

/// Whether a UI command can run now with its parameters; why not (the status line's text) when it
/// cannot.
pub type Check = fn(&App, &Value) -> Result<(), String>;

/// The checks of the UI commands that cannot always run, by area.
const CHECKS: &[&[(&str, Check)]] = &[caption::CHECKS, info::CHECKS, edit_view::CHECKS, export::CHECKS, save_as::CHECKS];

/// Whether UI command `id` can run now with `params` (what a click passes: `{}`); why not when it
/// cannot. Commands without a check always can.
pub fn enabled(app: &App, id: &str, params: &Value) -> Result<(), String> {
    match CHECKS.iter().flat_map(|a| a.iter()).find(|(c, _)| *c == id) {
        Some((_, check)) => check(app, params),
        None => Ok(()),
    }
}

/// Every UI command: the areas' lists joined.
pub const UI_COMMANDS: &[UiCommand] = &JOINED;

const COUNT: usize = count(AREAS);
const JOINED: [UiCommand; COUNT] = join(AREAS);

const fn count(areas: &[&[UiCommand]]) -> usize {
    let (mut n, mut a) = (0, 0);
    while a < areas.len() {
        n += areas[a].len();
        a += 1;
    }
    n
}

/// The filler [`join`] starts from; every slot is overwritten (there are exactly [`COUNT`]).
const FILLER: UiCommand = UiCommand { id: "", label: "", menu: "", keys: &[], params: "", run: unknown };

fn unknown(_: &mut App, _: &Value) -> Result<Value, CmdError> {
    Err(CmdError::UnknownCommand(String::new()))
}

/// The areas' lists one after the other, at compile time.
const fn join<const N: usize>(areas: &[&[UiCommand]]) -> [UiCommand; N] {
    let mut out = [FILLER; N];
    let (mut k, mut a) = (0, 0);
    while a < areas.len() {
        let mut i = 0;
        while i < areas[a].len() {
            out[k] = areas[a][i];
            k += 1;
            i += 1;
        }
        a += 1;
    }
    out
}

/// `params`, an object, as `T`; serde's message when they do not fit.
pub(crate) fn parse<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, CmdError> {
    if !params.is_object() {
        return Err(CmdError::BadParams(format!("params must be an object, not {}", kind(params))));
    }
    serde_json::from_value(params.clone()).map_err(|e| CmdError::BadParams(e.to_string()))
}

/// What a JSON value is, for an error message.
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// `params` as `T`, `null` read as `{}`.
pub(crate) fn parse_object<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, CmdError> {
    if params.is_null() { parse(&Value::Object(Default::default())) } else { parse(params) }
}

/// Refuses parameters for a command that takes none (`{}`; `null` counts as `{}`).
pub(crate) fn no_params(params: &Value) -> Result<(), CmdError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NoParams {}
    let _: NoParams = parse_object(params)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_is_joined_once_in_order() {
        let want: Vec<&str> = AREAS.iter().flat_map(|a| a.iter().map(|c| c.id)).collect();
        let got: Vec<&str> = UI_COMMANDS.iter().map(|c| c.id).collect();
        assert_eq!(got, want);
        assert!(UI_COMMANDS.iter().all(|c| !c.id.is_empty() && !c.label.is_empty()), "no filler left");
        let mut ids = got.clone();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), got.len(), "ids are unique");
    }

    #[test]
    fn every_check_names_a_ui_command() {
        for (id, _) in CHECKS.iter().flat_map(|a| a.iter()) {
            assert!(UI_COMMANDS.iter().any(|c| c.id == *id), "{id}");
        }
        let app = App::new(None);
        assert_eq!(enabled(&app, "view.thumbsSmall", &Value::Null), Ok(()));
        assert!(enabled(&app, "photo.adjustDate", &Value::Null).is_err(), "nothing selected");
    }

    #[test]
    fn no_params_takes_an_empty_object_or_null_only() {
        assert!(no_params(&Value::Null).is_ok());
        assert!(no_params(&serde_json::json!({})).is_ok());
        for bad in [serde_json::json!({ "a": 1 }), serde_json::json!([]), serde_json::json!("x"), serde_json::json!(0)] {
            assert!(matches!(no_params(&bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }
}
