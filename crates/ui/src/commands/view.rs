//! The view's commands: the View menu (thumbnail size, the side panes, Small and Hidden Pictures,
//! Thumbnail Caption, Folder View, Full Screen), the toolbar's view-options popup (the folder sort
//! and Reverse Order), the tree's group and folder disclosure, and the window (File ▸ Exit).
//!
//! A checkable item's command toggles when it is given nothing (what a click passes) and sets the
//! value given otherwise (`on` / `collapsed`), so menus and scripts share one command. [`checked`]
//! answers whether a menu or popup row is on, from the view state.

use dalica_core::FolderId;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{Caption, FolderSort, Group, Pane, THUMB_DEFAULT, TreeMode, UiState};

/// The small thumbnail size Ctrl+1 jumps to (`ui-layout.md` §6.4); Ctrl+2 jumps to
/// [`THUMB_DEFAULT`].
pub const THUMB_SMALL: f32 = 96.0;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "view.thumbsSmall",
        label: "Small Thumbnails",
        menu: "View/Small Thumbnails",
        keys: &[(Context::Library, "Ctrl+1")],
        params: "{}: thumbnails at the small size (96)",
        run: thumbs_small,
    },
    UiCommand {
        id: "view.thumbsNormal",
        label: "Normal Thumbnails",
        menu: "View/Normal Thumbnails",
        keys: &[(Context::Library, "Ctrl+2")],
        params: "{}: thumbnails at the normal size (144)",
        run: thumbs_normal,
    },
    UiCommand {
        id: "pane.properties",
        label: "Properties",
        menu: "View/Properties",
        keys: &[(Context::Global, "Alt+Enter")],
        params: "{}: open the Properties pane, or close it when it is open",
        run: pane_properties,
    },
    UiCommand {
        id: "pane.tags",
        label: "Tags",
        menu: "View/Tags",
        keys: &[(Context::Global, "Ctrl+T")],
        params: "{}: open the Tags pane, or close it when it is open",
        run: pane_tags,
    },
    UiCommand {
        id: "pane.people",
        label: "People",
        menu: "View/People",
        keys: &[],
        params: "{}: open the People pane, or close it when it is open",
        run: pane_people,
    },
    UiCommand {
        id: "pane.places",
        label: "Places",
        menu: "View/Places",
        keys: &[],
        params: "{}: open the Places pane, or close it when it is open",
        run: pane_places,
    },
    UiCommand {
        id: "pane.toggle",
        label: "Toggle Side Pane",
        menu: "",
        keys: &[(Context::Global, "Ctrl+0")],
        params: "{}: close the side pane, or open it (Properties) when closed",
        run: pane_toggle,
    },
    UiCommand {
        id: "view.smallPictures",
        label: "Small Pictures",
        menu: "View/Small Pictures",
        keys: &[],
        params: r#"{"on"?: bool}: show small pictures (both sides under 64 px); toggles without "on""#,
        run: small_pictures,
    },
    UiCommand {
        id: "view.hiddenPictures",
        label: "Hidden Pictures",
        menu: "View/Hidden Pictures",
        keys: &[],
        params: r#"{"on"?: bool}: show hidden pictures and the Hidden Folders group; toggles without "on""#,
        run: hidden_pictures,
    },
    UiCommand {
        id: "view.caption",
        label: "Thumbnail Caption",
        menu: "View/Thumbnail Caption",
        keys: &[],
        params: r#"{"caption": "none" | "filename" | "caption" | "tags" | "resolution" | "date"}"#,
        run: caption,
    },
    UiCommand {
        id: "view.folderView",
        label: "Folder View",
        menu: "View/Folder View",
        keys: &[],
        params: r#"{"mode": "flat" | "tree"}: the library tree as a flat list of folders or as the folder tree"#,
        run: folder_view,
    },
    UiCommand {
        id: "view.folderSort",
        label: "Sort Folders",
        menu: "",
        keys: &[],
        params: concat!(
            r#"{"by"?: "date" | "recent" | "size" | "name", "reverse"?: true}: "by" picks the folder order, "#,
            r#""reverse": true flips Reverse Order (the popup's check row); at least one"#
        ),
        run: folder_sort,
    },
    UiCommand {
        id: "view.groupCollapse",
        label: "Collapse Group",
        menu: "",
        keys: &[],
        params: concat!(
            r#"{"group": "albums" | "people" | "projects" | "folders" | "other" | "hidden", "collapsed"?: bool}: "#,
            r#"fold or unfold a tree group (its sections leave the light table); toggles without "collapsed""#
        ),
        run: group_collapse,
    },
    UiCommand {
        id: "view.folderCollapse",
        label: "Collapse Folder",
        menu: "",
        keys: &[],
        params: r#"{"folder": FolderId, "collapsed"?: bool}: tree mode: fold or unfold a folder's children; toggles without "collapsed""#,
        run: folder_collapse,
    },
    UiCommand {
        id: "window.fullscreen",
        label: "Full Screen",
        menu: "View/Full Screen",
        keys: &[(Context::Global, "F11")],
        params: "{}: full screen on or off",
        run: full_screen,
    },
    UiCommand {
        id: "app.quit",
        label: "Exit",
        menu: "File/Exit",
        keys: &[(Context::Global, "Ctrl+Q")],
        params: "{}: close the window, as its Close button does (changes are written first)",
        run: quit,
    },
];

fn thumbs_small(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().thumb_size = THUMB_SMALL;
    Ok(json!({ "thumbSize": THUMB_SMALL }))
}

fn thumbs_normal(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.state_mut().thumb_size = THUMB_DEFAULT;
    Ok(json!({ "thumbSize": THUMB_DEFAULT }))
}

/// Opens `pane`, or closes it when it is the one open: what its toggle at the bottom right does.
pub fn toggle_pane(state: &mut UiState, pane: Pane) {
    state.pane = if state.pane == Some(pane) { None } else { Some(pane) };
}

fn pane(app: &mut App, params: &Value, pane: Pane) -> Result<Value, CmdError> {
    no_params(params)?;
    let state = app.state_mut();
    toggle_pane(state, pane);
    Ok(json!({ "pane": state.pane }))
}

fn pane_properties(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    pane(app, params, Pane::Properties)
}

fn pane_tags(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    pane(app, params, Pane::Tags)
}

fn pane_people(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    pane(app, params, Pane::People)
}

fn pane_places(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    pane(app, params, Pane::Places)
}

fn pane_toggle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let state = app.state_mut();
    state.pane = match state.pane {
        Some(_) => None,
        None => Some(Pane::Properties),
    };
    Ok(json!({ "pane": state.pane }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OnOff {
    on: Option<bool>,
}

/// Sets `flag` to `on`, or toggles it without.
fn switch(params: &Value, flag: &mut bool) -> Result<bool, CmdError> {
    let p: OnOff = parse_object(params)?;
    *flag = p.on.unwrap_or(!*flag);
    Ok(*flag)
}

fn small_pictures(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let on = switch(params, &mut app.state_mut().small_pictures)?;
    Ok(json!({ "smallPictures": on }))
}

fn hidden_pictures(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let on = switch(params, &mut app.state_mut().hidden_pictures)?;
    Ok(json!({ "hiddenPictures": on }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCaption {
    caption: Caption,
}

fn caption(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SetCaption = parse(params)?;
    app.state_mut().caption = p.caption;
    Ok(json!({ "caption": p.caption }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderView {
    mode: TreeMode,
}

fn folder_view(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: FolderView = parse(params)?;
    app.state_mut().tree_mode = p.mode;
    Ok(json!({ "treeMode": p.mode }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SortFolders {
    by: Option<FolderSort>,
    reverse: Option<bool>,
}

fn folder_sort(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: SortFolders = parse(params)?;
    match (p.by, p.reverse) {
        (None, None) => return Err(CmdError::BadParams("give `by` (date, recent, size or name) or `reverse: true`".into())),
        (_, Some(false)) => {
            return Err(CmdError::BadParams("`reverse: true` flips Reverse Order; ui.set {folderSortReverse} sets it".into()));
        }
        _ => {}
    }
    let state = app.state_mut();
    if let Some(by) = p.by {
        state.folder_sort = by;
    }
    if p.reverse == Some(true) {
        state.folder_sort_reverse = !state.folder_sort_reverse;
    }
    Ok(json!({ "folderSort": state.folder_sort, "folderSortReverse": state.folder_sort_reverse }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupCollapse {
    group: Group,
    collapsed: Option<bool>,
}

/// Adds `x` to `list` (`true`) or removes it (`false`); without `want`, whichever it is not now.
/// Returns whether `x` is in the list afterwards.
fn fold<T: PartialEq + Copy>(list: &mut Vec<T>, x: T, want: Option<bool>) -> bool {
    let now = list.contains(&x);
    let on = want.unwrap_or(!now);
    if on && !now {
        list.push(x);
    } else if !on {
        list.retain(|y| *y != x);
    }
    on
}

fn group_collapse(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: GroupCollapse = parse(params)?;
    let state = app.state_mut();
    let collapsed = fold(&mut state.collapsed_groups, p.group, p.collapsed);
    Ok(json!({ "group": p.group, "collapsed": collapsed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderCollapse {
    folder: FolderId,
    collapsed: Option<bool>,
}

fn folder_collapse(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: FolderCollapse = parse(params)?;
    if !app.session().is_some_and(|s| s.library().folder(p.folder).is_some()) {
        return Err(CmdError::NotFound(format!("no folder {}", p.folder)));
    }
    let collapsed = fold(&mut app.state_mut().collapsed_folders, p.folder, p.collapsed);
    Ok(json!({ "folder": p.folder, "collapsed": collapsed }))
}

fn full_screen(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let state = app.state_mut();
    state.full_screen = !state.full_screen;
    Ok(json!({ "fullScreen": state.full_screen }))
}

fn quit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    app.request_quit();
    Ok(json!({ "quitting": true }))
}

/// Whether the menu or popup row running `id` with its fixed `params` is on now: a check mark or a
/// radio dot. `None` for a command this module does not answer for.
pub fn checked(state: &UiState, id: &str, params: &Value) -> Option<bool> {
    let param = |key: &str| params.get(key).cloned().unwrap_or(Value::Null);
    let is = |v: Value, key: &str| v == param(key);
    Some(match id {
        "view.thumbsSmall" => state.thumb_size == THUMB_SMALL,
        "view.thumbsNormal" => state.thumb_size == THUMB_DEFAULT,
        "pane.properties" => state.pane == Some(Pane::Properties),
        "pane.tags" => state.pane == Some(Pane::Tags),
        "pane.people" => state.pane == Some(Pane::People),
        "pane.places" => state.pane == Some(Pane::Places),
        "view.smallPictures" => state.small_pictures,
        "view.hiddenPictures" => state.hidden_pictures,
        "window.fullscreen" => state.full_screen,
        "view.caption" => is(json!(state.caption), "caption"),
        "view.folderView" => is(json!(state.tree_mode), "mode"),
        "view.folderSort" if param("reverse") == Value::Bool(true) => state.folder_sort_reverse,
        "view.folderSort" => is(json!(state.folder_sort), "by"),
        _ => return None,
    })
}
