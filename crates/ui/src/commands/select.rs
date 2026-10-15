//! The light-table selection (`ui-layout.md` §5.4), on the model of [`crate::selection`]:
//!
//! - `select.set`: photos by id: `replace`, `toggle` (Ctrl-click) or `range` (Shift-click: from the
//!   anchor to the one id given, inside its section). A script may select photos of several
//!   folders with `replace` or `toggle` (the selection then has no section); the gestures and every
//!   other command keep it inside one section. `section` names the section the photos are selected
//!   in: a photo of an album is also shown in the album's section, and a click there selects it in
//!   the album (`thumb:<PhotoId>/in/<album>`).
//! - `select.none` (Ctrl+D); `select.all` (Ctrl+A), `select.invert` (Ctrl+I) and `select.starred`
//!   in the current section or the one named; `select.section`: a whole section as a unit (the
//!   tree's rows).
//! - The arrows, Home and End (`select.left` … `select.last`) and with Shift their extending forms
//!   (`select.extendLeft` … `select.extendLast`): they stop at the section's ends and scroll the
//!   photo they reach into view.
//!
//! A command that selects photos of one section makes that section's folder the tree's folder.

use std::collections::HashSet;

use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::selection::{self, Scope, Step, Table};
use crate::state::{MAX_IDS, Section, Selection};

/// The step commands (arrows, Home, End, and with Shift): id, label, key, function, step, extend.
macro_rules! steps {
    ($( $id:literal, $label:literal, $key:literal, $f:ident, $step:expr, $extend:expr; )*) => {
        $(
            fn $f(app: &mut App, params: &Value) -> Result<Value, CmdError> {
                step(app, params, $step, $extend)
            }
        )*
        const STEPS: [UiCommand; 12] = [$(UiCommand {
            id: $id,
            label: $label,
            menu: "",
            keys: &[(Context::Library, $key)],
            params: "{}",
            run: $f,
        }),*];
    };
}

steps! {
    "select.left", "Select Previous Photo", "Left", left, Step::Left, false;
    "select.right", "Select Next Photo", "Right", right, Step::Right, false;
    "select.up", "Select Photo Above", "Up", up, Step::Up, false;
    "select.down", "Select Photo Below", "Down", down, Step::Down, false;
    "select.first", "Select First Photo of Section", "Home", first, Step::First, false;
    "select.last", "Select Last Photo of Section", "End", last, Step::Last, false;
    "select.extendLeft", "Extend Selection to Previous Photo", "Shift+Left", extend_left, Step::Left, true;
    "select.extendRight", "Extend Selection to Next Photo", "Shift+Right", extend_right, Step::Right, true;
    "select.extendUp", "Extend Selection to Photo Above", "Shift+Up", extend_up, Step::Up, true;
    "select.extendDown", "Extend Selection to Photo Below", "Shift+Down", extend_down, Step::Down, true;
    "select.extendFirst", "Extend Selection to First Photo", "Shift+Home", extend_first, Step::First, true;
    "select.extendLast", "Extend Selection to Last Photo", "Shift+End", extend_last, Step::Last, true;
}

const SECTION_DOC: &str =
    r#"{"section"?: "folder:<FolderId>"}: the current section (the selection's, the tree's folder, the one at the top of the view) when left out"#;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "select.set",
        label: "Select",
        menu: "",
        keys: &[],
        params: r#"{"ids": [PhotoId], "mode": "replace" | "toggle" | "range" = "replace", "section"?: "folder:<FolderId>" | "album:<AlbumId>" | "album:starred" | "album:search"}: range = from the anchor to the one id given, inside its section; section = the light-table section the photos are selected in (default: the folder they share)"#,
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
    UiCommand {
        id: "select.all",
        label: "Select All",
        menu: "Edit/Select All",
        keys: &[(Context::Library, "Ctrl+A")],
        params: SECTION_DOC,
        run: select_all,
    },
    UiCommand { id: "select.starred", label: "Select Starred", menu: "Edit/Select Starred", keys: &[], params: SECTION_DOC, run: select_starred },
    UiCommand {
        id: "select.invert",
        label: "Invert Selection",
        menu: "Edit/Invert Selection",
        keys: &[(Context::Library, "Ctrl+I")],
        params: SECTION_DOC,
        run: select_invert,
    },
    UiCommand {
        id: "select.section",
        label: "Select Section",
        menu: "",
        keys: &[],
        params: r#"{"section": "folder:<FolderId>"}: every photo of the section, as a unit"#,
        run: select_section,
    },
    STEPS[0],
    STEPS[1],
    STEPS[2],
    STEPS[3],
    STEPS[4],
    STEPS[5],
    STEPS[6],
    STEPS[7],
    STEPS[8],
    STEPS[9],
    STEPS[10],
    STEPS[11],
];

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Mode {
    #[default]
    Replace,
    Toggle,
    Range,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectSet {
    ids: Vec<PhotoId>,
    #[serde(default)]
    mode: Mode,
    #[serde(default)]
    section: Option<Section>,
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
    let sel = match p.mode {
        Mode::Range => {
            let [to] = p.ids[..] else {
                return Err(CmdError::BadParams("a range takes exactly one id: the end it runs to from the anchor".into()));
            };
            let table = Table::of(app).ok_or_else(|| CmdError::NotFound(format!("no photo {to}")))?;
            match p.section.or_else(|| table.section_of(to)).and_then(|s| table.scope(s)) {
                Some(scope) => selection::shift_click(&app.state().selection, &scope, to),
                None => loose(vec![to], Some(to), &folder),
            }
        }
        Mode::Replace => {
            let mut seen = HashSet::with_capacity(p.ids.len());
            let ids: Vec<PhotoId> = p.ids.iter().copied().filter(|id| seen.insert(*id)).collect();
            let anchor = p.ids.last().copied();
            loose(ids, anchor, &folder)
        }
        Mode::Toggle => {
            let mut ids = app.state().selection.ids.clone();
            let mut on: HashSet<PhotoId> = ids.iter().copied().collect();
            for id in &p.ids {
                if on.remove(id) {
                    ids.retain(|x| x != id);
                } else {
                    on.insert(*id);
                    ids.push(*id);
                }
            }
            let anchor = p.ids.last().copied().filter(|id| on.contains(id)).or_else(|| ids.last().copied());
            loose(ids, anchor, &folder)
        }
    };
    let sel = match p.section {
        Some(section) => in_named_section(app, sel, section)?,
        None => sel,
    };
    let state = app.state_mut();
    selection::commit(state, sel);
    Ok(json!({ "selection": state.selection.ids }))
}

/// `sel` as a selection in `section`: the light table must show the section and every photo of
/// `sel` in it (`not_found` otherwise).
fn in_named_section(app: &App, mut sel: Selection, section: Section) -> Result<Selection, CmdError> {
    let table = Table::of(app).ok_or_else(opening)?;
    let scope = table.scope(section).ok_or_else(|| CmdError::NotFound(format!("no section {section} on the light table")))?;
    if let Some(id) = sel.ids.iter().find(|id| scope.index(**id).is_none()) {
        return Err(CmdError::NotFound(format!("photo {id} is not in {section}")));
    }
    sel.section = (!sel.ids.is_empty()).then_some(section);
    Ok(sel)
}

/// A selection of `ids` as a script gave them: its section is the folder they all share, if any.
fn loose(ids: Vec<PhotoId>, anchor: Option<PhotoId>, folder: &impl Fn(PhotoId) -> Option<FolderId>) -> Selection {
    let mut folders = ids.iter().map(|id| folder(*id));
    let first = folders.next().flatten();
    let section = if folders.all(|f| f == first) { first.map(Section::Folder) } else { None };
    Selection::new(ids, anchor, section, false)
}

fn select_none(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().selection = Selection::default();
    Ok(json!({ "selection": [] }))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InSection {
    section: Option<Section>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfSection {
    section: Section,
}

fn opening() -> CmdError {
    CmdError::Disabled("The library is still opening.".into())
}

fn nothing() -> CmdError {
    CmdError::Disabled("There are no photos to select.".into())
}

/// Runs `f` on the section asked for (or the current one) and makes its result the selection.
fn in_section(
    app: &mut App,
    section: Option<Section>,
    f: impl FnOnce(&Selection, &Scope<'_>, &Table<'_>) -> Result<Selection, CmdError>,
) -> Result<Value, CmdError> {
    let sel = {
        let table = Table::of(app).ok_or_else(opening)?;
        let id = match section {
            Some(s) => s,
            None => table.current(app.state()).ok_or_else(nothing)?,
        };
        let scope = table.scope(id).ok_or_else(|| CmdError::NotFound(format!("no section {id} on the light table")))?;
        f(&app.state().selection, &scope, &table)?
    };
    let state = app.state_mut();
    selection::commit(state, sel);
    Ok(json!({ "selection": state.selection.ids, "section": state.selection.section }))
}

fn select_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: InSection = parse_object(params)?;
    in_section(app, p.section, |sel, scope, _| Ok(selection::all(sel, scope)))
}

fn select_invert(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: InSection = parse_object(params)?;
    in_section(app, p.section, |sel, scope, _| Ok(selection::invert(sel, scope)))
}

fn select_starred(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: InSection = parse_object(params)?;
    in_section(app, p.section, |_, scope, table| {
        selection::starred(scope, table.library).ok_or_else(|| CmdError::Disabled("There are no starred photos in this folder.".into()))
    })
}

fn select_section(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: OfSection = parse(params)?;
    in_section(app, Some(p.section), |_, scope, _| Ok(selection::whole(scope)))
}

/// An arrow, Home or End (`extend`: with Shift); the photo it reaches is scrolled into view.
fn step(app: &mut App, params: &Value, step: Step, extend: bool) -> Result<Value, CmdError> {
    no_params(params)?;
    let (sel, scroll) = {
        let table = Table::of(app).ok_or_else(opening)?;
        let id = table.current(app.state()).ok_or_else(nothing)?;
        let scope = table.scope(id).ok_or_else(nothing)?;
        let (sel, to) = selection::moved(&app.state().selection, &scope, step, extend).ok_or_else(nothing)?;
        (sel, table.reveal(id, to))
    };
    let state = app.state_mut();
    selection::commit(state, sel);
    if let Some(anchor) = scroll {
        state.scroll = anchor;
    }
    Ok(json!({ "selection": state.selection.ids, "anchor": state.selection.anchor }))
}
