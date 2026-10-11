//! The Photo Tray (`ui-layout.md` §6.2), on the model of [`crate::tray`]. These change
//! [`UiState::tray`](crate::state::UiState::tray) (the held photos and the tray's own selection)
//! and, when photos leave the tray, the light table's selection it mirrors:
//!
//! - `tray.hold` (Ctrl+H, the Hold button): keeps photos in the tray when the selection moves on,
//!   the selection's by default;
//! - `tray.clear` (the Clear button, the tray menu's Remove from Tray): removes the tray-selected
//!   photos, or the ones named, or with neither everything; emptying a tray that holds held photos
//!   asks first, so over automation it needs `"confirm": true`;
//! - `tray.select`: the tray's own selection (a click on a mini thumbnail, Ctrl-click, the rubber
//!   band in the tray).
//!
//! The star and rotate buttons act on [`crate::tray::photos`] through the engine's commands
//! (`photo.star.toggle {ids}`: one undo step for the whole tray).

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::{MAX_IDS, Selection, UiState};
use crate::tray;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "tray.hold",
        label: "Hold",
        menu: "",
        keys: &[(Context::Library, "Ctrl+H")],
        params: r#"{"ids"?: [PhotoId]}: keep these photos in the tray (default: the selection's) when the selection changes"#,
        run: hold,
    },
    UiCommand {
        id: "tray.clear",
        label: "Clear Tray",
        menu: "",
        keys: &[],
        params: concat!(
            r#"{"ids"?: [PhotoId], "confirm"?: bool}: remove these photos from the tray (default: the tray-selected ones, "#,
            r#"or with none selected all of them; emptying a tray with held photos needs "confirm": true)"#
        ),
        run: clear,
    },
    UiCommand {
        id: "tray.select",
        label: "Select in Tray",
        menu: "",
        keys: &[],
        params: r#"{"ids": [PhotoId], "mode": "replace" | "toggle" | "add" = "replace"}: the tray's own selection, photos the tray shows"#,
        run: select,
    },
];

/// Photo ids a script gave: at most [`MAX_IDS`], each once, in order.
fn checked(ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} ids")));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

/// Refuses the first id the tray does not show.
fn in_tray(state: &UiState, ids: &[PhotoId]) -> Result<(), CmdError> {
    match ids.iter().find(|id| !tray::shows(state, **id)) {
        Some(id) => Err(CmdError::NotFound(format!("photo {id} is not in the tray"))),
        None => Ok(()),
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Hold {
    ids: Option<Vec<PhotoId>>,
}

fn hold(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Hold = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => {
            let ids = checked(ids)?;
            let library = app.session().map(dalica_engine::Session::library);
            if let Some(missing) = ids.iter().find(|id| !library.is_some_and(|l| l.photo(**id).is_some())) {
                return Err(CmdError::NotFound(format!("no photo {missing}")));
            }
            ids
        }
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled(tray::EMPTY.into()));
    }
    let tray = &mut app.state_mut().tray;
    let fresh: Vec<PhotoId> = ids.into_iter().filter(|id| !tray.holds(*id)).collect();
    if tray.held.len().saturating_add(fresh.len()) > MAX_IDS {
        return Err(CmdError::Limit(format!("the tray holds at most {MAX_IDS} photos")));
    }
    let mut held = std::mem::take(&mut tray.held);
    held.extend(fresh);
    tray.set_held(held);
    Ok(json!({ "held": tray.held.len() }))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Clear {
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    confirm: bool,
}

fn clear(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Clear = parse_object(params)?;
    let state = app.state();
    if tray::is_empty(state) {
        return Err(CmdError::Disabled(tray::EMPTY.into()));
    }
    let named = match p.ids {
        Some(ids) => {
            let ids = checked(ids)?;
            in_tray(state, &ids)?;
            Some(ids)
        }
        None => {
            let shown: Vec<PhotoId> = state.tray.selected.iter().copied().filter(|id| tray::shows(state, *id)).collect();
            (!shown.is_empty()).then_some(shown)
        }
    };
    let removed = match named {
        Some(ids) => remove(app.state_mut(), &ids),
        None => {
            if !state.tray.held.is_empty() && !p.confirm {
                return Err(CmdError::Disabled(format!("{} It has held photos: send \"confirm\": true to clear them too.", tray::CLEAR_QUESTION)));
            }
            let n = tray::count(state);
            let state = app.state_mut();
            state.tray.set_held(Vec::new());
            state.tray.selected.clear();
            state.selection = Selection::default();
            n
        }
    };
    Ok(json!({ "removed": removed, "count": tray::count(app.state()) }))
}

/// Takes `ids` (photos the tray shows) out of the held photos, the light table's selection and
/// the tray's own selection. Returns how many left.
fn remove(state: &mut UiState, ids: &[PhotoId]) -> usize {
    let gone: HashSet<PhotoId> = ids.iter().copied().collect();
    let held: Vec<PhotoId> = state.tray.held.iter().copied().filter(|id| !gone.contains(id)).collect();
    state.tray.set_held(held);
    state.tray.selected.retain(|id| !gone.contains(id));
    let sel = &state.selection;
    if sel.ids.iter().any(|id| gone.contains(id)) {
        let kept: Vec<PhotoId> = sel.ids.iter().copied().filter(|id| !gone.contains(id)).collect();
        let anchor = sel.anchor.filter(|a| !gone.contains(a)).or_else(|| kept.last().copied());
        let section = if kept.is_empty() { None } else { sel.section };
        state.selection = Selection::new(kept, anchor, section, false);
    }
    gone.len()
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    #[default]
    Replace,
    Toggle,
    Add,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Select {
    ids: Vec<PhotoId>,
    #[serde(default)]
    mode: Mode,
}

fn select(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Select = parse(params)?;
    let ids = checked(p.ids)?;
    let state = app.state_mut();
    in_tray(state, &ids)?;
    tray::prune(state);
    let selected = &mut state.tray.selected;
    match p.mode {
        Mode::Replace => *selected = ids,
        Mode::Add => {
            let on: HashSet<PhotoId> = selected.iter().copied().collect();
            selected.extend(ids.into_iter().filter(|id| !on.contains(id)));
        }
        Mode::Toggle => {
            let on: HashSet<PhotoId> = selected.iter().copied().collect();
            let off: HashSet<PhotoId> = ids.iter().copied().filter(|id| on.contains(id)).collect();
            selected.retain(|id| !off.contains(id));
            selected.extend(ids.into_iter().filter(|id| !on.contains(id)));
        }
    }
    Ok(json!({ "selected": state.tray.selected }))
}
