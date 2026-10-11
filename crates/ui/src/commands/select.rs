//! The light-table selection (`ui-layout.md` §5.4): `select.set` and `select.none`. The selection
//! model (gestures, keyboard navigation, `select.all` / `select.invert` / `select.starred` /
//! `select.section`) adds its commands here.

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, no_params, parse};
use crate::app::App;
use crate::state::{MAX_IDS, Section, Selection};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "select.set",
        label: "Select",
        menu: "",
        keys: &[],
        params: r#"{"ids": [PhotoId], "mode": "replace" | "toggle" = "replace"}"#,
        run: select_set,
    },
    UiCommand {
        id: "select.none",
        label: "Select None",
        menu: "Edit/Select None",
        keys: &[(Context::Library, "Ctrl+D")],
        params: "{}",
        run: select_none,
    },
];

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    #[default]
    Replace,
    Toggle,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectSet {
    ids: Vec<PhotoId>,
    #[serde(default)]
    mode: Mode,
}

fn select_set(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SelectSet = parse(params)?;
    if p.ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} ids")));
    }
    let library = app.session().map(dalica_engine::Session::library);
    let folder = |id: PhotoId| library.and_then(|l| l.photo(id)).map(|p| p.folder);
    if let Some(missing) = p.ids.iter().find(|&&id| folder(id).is_none()) {
        return Err(CmdError::NotFound(format!("no photo {missing}")));
    }
    let mut ids = match p.mode {
        Mode::Replace => Vec::with_capacity(p.ids.len()),
        Mode::Toggle => app.state().selection.ids.clone(),
    };
    for id in &p.ids {
        match (p.mode, ids.iter().position(|x| x == id)) {
            (Mode::Replace, Some(_)) => {}
            (Mode::Toggle, Some(i)) => {
                ids.remove(i);
            }
            (_, None) => ids.push(*id),
        }
    }
    let anchor = p.ids.last().copied().filter(|id| ids.contains(id)).or_else(|| ids.last().copied());
    // The section: the folder every selected photo is in, if they share one.
    let mut folders = ids.iter().map(|id| folder(*id));
    let first = folders.next().flatten();
    let section = if folders.all(|f| f == first) { first.map(Section::Folder) } else { None };
    let sel = &mut app.state_mut().selection;
    *sel = Selection { ids, anchor, section, whole: false };
    Ok(json!({ "selection": sel.ids }))
}

fn select_none(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().selection = Selection::default();
    Ok(json!({ "selection": [] }))
}
