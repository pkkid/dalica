//! The collage canvas's commands (`creating.md` §3): the selection of pictures on the canvas (view
//! state) and the changes to the collage, each one engine command and so one step of the room's
//! history. A picture is named by its number in the engine's placements (`item:<n>`, bottom first,
//! 0 … 999, as `collage.inspect` lists them).
//!
//! The ids are the UI's own (`collageCanvas.*`), never the engine's, which they run:
//! `collage.move`, `collage.swap`, `collage.order`, `collage.background`, `collage.frameCenter`,
//! `collage.scramble`, `collage.shuffle` and `collage.unplace`.
//!
//! - **Selecting**: `collageCanvas.select {items, add?}` (a click; Ctrl and Shift add),
//!   `collageCanvas.selectAll` (Select All, Ctrl+A) and `collageCanvas.selectNone` (Select None,
//!   Esc).
//! - **Moving** (a pile): `collageCanvas.move {item, centre}` (a drag: the picture's new centre,
//!   normalised to the page, kept within half a page of it) and `collageCanvas.turn {item, angle,
//!   scale}` (the wheel handle: the picture's new angle and its size over the layout's, together
//!   round its centre).
//! - **Arranging**: `collageCanvas.order {item?, to}` (Bring to Top / Send to Bottom),
//!   `collageCanvas.swap {a, b}` (the mosaics and the grid: a drag onto another picture),
//!   `collageCanvas.remove {items?}` (Remove, Delete: back to the Clips),
//!   `collageCanvas.setBackground {item?}`, `collageCanvas.setFrameCenter {item?}`,
//!   `collageCanvas.scramble`, `collageCanvas.shuffle` and `collageCanvas.viewAndEdit {item?}`.
//!   Without `item` (`items`) they take the selection: the picture selected last, or all of them
//!   for Remove.
//!
//! A change can renumber the pictures (Bring to Top puts its picture last; a swap, Scramble and
//! Shuffle deal them again): the selection follows its pictures ([`follow`]). Remove empties it.
//!
//! The Collage key context (`menus-and-shortcuts.md` §2.0) is active while the room is in front
//! (`crate::keys::view_context`): Delete removes the selection, Ctrl+A selects every picture and
//! Esc none; Ctrl+Z and Ctrl+Y are the engine's `collage.undo` and `collage.redo` (the room's own
//! history, M14.16).

use dalica_core::PhotoId;
use dalica_create::collage::Style;
use dalica_create::collage::doc::CENTRE_MARGIN;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{MAX_CLIPS, Room};

/// Why the canvas's changes are refused while the room shows no collage of the engine's: only the
/// stub room `collage.open` opens does, until the room opens with the engine's collage (M14.18).
pub const LATER: &str = "The collage arrives with the Collage room.";
/// Why a command on the selected picture cannot run.
pub const SELECT_FIRST: &str = "Select a picture on the collage first.";
/// Why Move and Turn cannot run outside a pile.
pub const PILE_ONLY: &str = "Only a Picture Pile's pictures move; drag one onto another to swap them.";
/// Why Swap cannot run in a pile.
pub const NOT_IN_PILE: &str = "A Picture Pile's pictures move freely; swapping is for the other styles.";
/// Why Set as Frame Center cannot run outside a Frame Mosaic.
pub const FRAME_ONLY: &str = "Set as Frame Center is for a Frame Mosaic.";
/// Why Set as Background cannot run in Multiple Exposure.
pub const NO_BACKGROUND: &str = "Multiple Exposure has no background.";
/// The most a picture is scaled by the wheel handle (over the layout's size), and the least: the
/// document's range.
pub const SCALE_MAX: f32 = 20.0;
pub const SCALE_MIN: f32 = 0.05;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "collageCanvas.select",
        label: "Select Pictures",
        menu: "",
        keys: &[],
        params: "{items: [n], add?: bool}: select these pictures on the canvas (`add` keeps the others selected); replies {items}",
        run: select,
    },
    UiCommand {
        id: "collageCanvas.selectAll",
        label: "Select All",
        menu: "",
        keys: &[(Context::Collage, "Ctrl+A")],
        params: "{}: select every picture on the canvas; replies {items}",
        run: select_all,
    },
    UiCommand {
        id: "collageCanvas.selectNone",
        label: "Select None",
        menu: "",
        keys: &[(Context::Collage, "Esc")],
        params: "{}: select no picture on the canvas; replies {items}",
        run: select_none,
    },
    UiCommand {
        id: "collageCanvas.move",
        label: "Move Picture",
        menu: "",
        keys: &[],
        params: "{item: n, centre: [x, y]}: move a picture of a pile, its new centre normalised to the page (−1 … 2; kept within half a page of it)",
        run: move_picture,
    },
    UiCommand {
        id: "collageCanvas.turn",
        label: "Turn and Scale Picture",
        menu: "",
        keys: &[],
        params: "{item: n, angle: degrees clockwise (−360 … 360), scale: 0.05 … 20 (over the layout's size)}: turn and scale a picture of a pile round its centre, as one step",
        run: turn,
    },
    UiCommand {
        id: "collageCanvas.order",
        label: "Bring to Top / Send to Bottom",
        menu: "",
        keys: &[],
        params: r#"{item?: n, to: "top" | "bottom"}: bring a picture (the selected one without `item`) to the top of the pile or send it to the bottom; replies {items} (the selection)"#,
        run: order,
    },
    UiCommand {
        id: "collageCanvas.swap",
        label: "Swap Pictures",
        menu: "",
        keys: &[],
        params: "{a: n, b: n}: swap two pictures of a mosaic or the grid; replies {items} (the selection)",
        run: swap,
    },
    UiCommand {
        id: "collageCanvas.remove",
        label: "Remove",
        menu: "",
        keys: &[(Context::Collage, "Delete")],
        params: "{items?: [n]}: take pictures (the selection without `items`) off the page; they stay in the Clips",
        run: remove,
    },
    UiCommand {
        id: "collageCanvas.setBackground",
        label: "Set as Background",
        menu: "",
        keys: &[],
        params: "{item?: n}: use a picture (the selected one without `item`) as the page's background",
        run: set_background,
    },
    UiCommand {
        id: "collageCanvas.setFrameCenter",
        label: "Set as Frame Center",
        menu: "",
        keys: &[],
        params: "{item?: n}: put a picture (the selected one without `item`) in the centre of a Frame Mosaic; replies {items} (the selection)",
        run: set_frame_center,
    },
    UiCommand {
        id: "collageCanvas.scramble",
        label: "Scramble Collage",
        menu: "",
        keys: &[],
        params: "{}: a new random arrangement (the pictures you moved are laid out again); replies {items} (the selection)",
        run: scramble,
    },
    UiCommand {
        id: "collageCanvas.shuffle",
        label: "Shuffle Pictures",
        menu: "",
        keys: &[],
        params: "{}: the same places, the pictures dealt to them again; replies {items} (the selection)",
        run: shuffle,
    },
    UiCommand {
        id: "collageCanvas.viewAndEdit",
        label: "View and Edit",
        menu: "",
        keys: &[],
        params: "{item?: n}: open a picture (the selected one without `item`) in the edit view, on the Library tab",
        run: view_and_edit,
    },
];

/// The canvas's commands grey their buttons and menu rows while the room is not open, while it
/// shows no collage ([`LATER`]), without a picture to act on, and in a style they do not apply to.
pub const CHECKS: &[(&str, Check)] = &[
    ("collageCanvas.select", super::collage::room_open),
    ("collageCanvas.selectAll", any_picture),
    ("collageCanvas.selectNone", super::collage::room_open),
    ("collageCanvas.move", pile),
    ("collageCanvas.turn", pile),
    ("collageCanvas.order", one_picture),
    ("collageCanvas.swap", not_pile),
    ("collageCanvas.remove", some_pictures),
    ("collageCanvas.setBackground", background),
    ("collageCanvas.setFrameCenter", frame_mosaic),
    ("collageCanvas.scramble", has_collage),
    ("collageCanvas.shuffle", has_collage),
    ("collageCanvas.viewAndEdit", one_picture),
];

/// The engine's collage, with the room open.
fn doc(app: &App) -> Result<&dalica_create::collage::Collage, String> {
    super::collage::room_open(app, &Value::Null)?;
    app.session().and_then(|s| s.collage().document()).ok_or_else(|| LATER.to_string())
}

fn has_collage(app: &App, _: &Value) -> Result<(), String> {
    doc(app).map(|_| ())
}

fn any_picture(app: &App, _: &Value) -> Result<(), String> {
    if doc(app)?.clips.iter().any(|c| c.on_page) { Ok(()) } else { Err("No picture is on the page.".into()) }
}

/// A picture to act on: `item` in the parameters, or one selected.
fn one_picture(app: &App, params: &Value) -> Result<(), String> {
    doc(app)?;
    if params.get("item").is_some_and(|v| !v.is_null()) || !app.state().collage.selection.is_empty() { Ok(()) } else { Err(SELECT_FIRST.into()) }
}

fn some_pictures(app: &App, params: &Value) -> Result<(), String> {
    doc(app)?;
    if params.get("items").is_some_and(|v| !v.is_null()) || !app.state().collage.selection.is_empty() { Ok(()) } else { Err(SELECT_FIRST.into()) }
}

fn pile(app: &App, _: &Value) -> Result<(), String> {
    if doc(app)?.style == Style::Pile { Ok(()) } else { Err(PILE_ONLY.into()) }
}

fn not_pile(app: &App, _: &Value) -> Result<(), String> {
    if doc(app)?.style == Style::Pile { Err(NOT_IN_PILE.into()) } else { Ok(()) }
}

fn background(app: &App, params: &Value) -> Result<(), String> {
    if !doc(app)?.style.has_background() {
        return Err(NO_BACKGROUND.into());
    }
    one_picture(app, params)
}

fn frame_mosaic(app: &App, params: &Value) -> Result<(), String> {
    if doc(app)?.style != Style::FrameMosaic {
        return Err(FRAME_ONLY.into());
    }
    one_picture(app, params)
}

/// Runs `check` as a command's refusal.
fn need(check: Check, app: &App, params: &Value) -> Result<(), CmdError> {
    check(app, params).map_err(CmdError::Disabled)
}

/// Refuses an item number past the last picture a collage can hold.
fn item(n: u32) -> Result<u32, CmdError> {
    if (n as usize) < MAX_CLIPS { Ok(n) } else { Err(CmdError::BadParams(format!("pictures are 0 … {}, not {n}", MAX_CLIPS - 1))) }
}

fn items(list: &[u32]) -> Result<(), CmdError> {
    if list.len() > MAX_CLIPS {
        return Err(CmdError::BadParams(format!("a collage holds at most {MAX_CLIPS} pictures")));
    }
    list.iter().try_for_each(|n| item(*n).map(|_| ()))
}

/// The photo of each picture on the page, by its number (bottom first); empty without a collage.
pub fn photos(app: &mut App) -> Vec<PhotoId> {
    app.session_mut().map(|s| s.collage_placements().into_iter().map(|p| p.id).collect()).unwrap_or_default()
}

/// The photo of picture `n`.
fn photo_of(app: &mut App, n: u32) -> Result<PhotoId, CmdError> {
    let all = photos(app);
    all.get(n as usize).copied().ok_or_else(|| CmdError::NotFound(format!("no picture {n} on the page ({} there)", all.len())))
}

/// `item`, or the picture selected last.
fn chosen(app: &App, item_param: Option<u32>) -> Result<u32, CmdError> {
    match item_param {
        Some(n) => item(n),
        None => app.state().collage.selection.last().copied().ok_or_else(|| CmdError::Disabled(SELECT_FIRST.into())),
    }
}

/// Runs the engine's command `id`.
fn engine(app: &mut App, id: &str, params: &Value) -> Result<Value, CmdError> {
    match app.session_mut() {
        Some(s) => s.execute(id, params),
        None => Err(CmdError::Disabled("The library is still opening.".into())),
    }
}

/// After a change that may renumber the pictures: the selection follows the photos it held before
/// (`before`, by number), those no longer on the page dropped. Replies `{items}`.
fn follow(app: &mut App, before: &[PhotoId]) -> Value {
    let selected: Vec<PhotoId> = app.state().collage.selection.iter().filter_map(|n| before.get(*n as usize).copied()).collect();
    let now = photos(app);
    let items: Vec<u32> = selected.iter().filter_map(|id| now.iter().position(|p| p == id)).filter_map(|i| u32::try_from(i).ok()).collect();
    app.state_mut().collage.selection.clone_from(&items);
    json!({ "items": items })
}

/// The engine's reply `reply` with the selection's `{items}` added.
fn with_items(mut reply: Value, items: Value) -> Value {
    if let (Some(o), Some(i)) = (reply.as_object_mut(), items.get("items")) {
        o.insert("items".into(), i.clone());
    }
    reply
}

/// Runs the engine's `id` with `params` and lets the selection follow its pictures.
fn renumbering(app: &mut App, id: &str, params: &Value) -> Result<Value, CmdError> {
    let before = photos(app);
    let reply = engine(app, id, params)?;
    let items = follow(app, &before);
    Ok(with_items(reply, items))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Select {
    items: Vec<u32>,
    #[serde(default)]
    add: bool,
}

fn select(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Select = parse(params)?;
    items(&p.items)?;
    need(super::collage::room_open, app, params)?;
    if app.session().is_some_and(|s| s.collage().is_open()) {
        let count = photos(app).len();
        if let Some(n) = p.items.iter().find(|n| **n as usize >= count) {
            return Err(CmdError::NotFound(format!("no picture {n} on the page ({count} there)")));
        }
    }
    let sel = &mut app.state_mut().collage.selection;
    if !p.add {
        sel.clear();
    }
    for n in p.items {
        if !sel.contains(&n) {
            sel.push(n);
        }
    }
    Ok(json!({ "items": sel }))
}

fn select_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(has_collage, app, params)?;
    let count = photos(app).len().min(MAX_CLIPS);
    let all: Vec<u32> = (0..count).filter_map(|n| u32::try_from(n).ok()).collect();
    app.state_mut().collage.selection.clone_from(&all);
    Ok(json!({ "items": all }))
}

fn select_none(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(super::collage::room_open, app, params)?;
    app.state_mut().collage.selection.clear();
    Ok(json!({ "items": [] }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    item: u32,
    centre: [f32; 2],
}

fn move_picture(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Move = parse(params)?;
    item(p.item)?;
    if !p.centre.iter().all(|v| v.is_finite() && (crate::state::GESTURE_MIN..=crate::state::GESTURE_MAX).contains(v)) {
        return Err(CmdError::BadParams("`centre` is [x, y], each −1 … 2 (the page is 0 … 1)".into()));
    }
    need(pile, app, params)?;
    // The document keeps a centre within half a page of it: a drag further out stops at its edge.
    let keep = |v: f32| f64::from(v).clamp(-CENTRE_MARGIN, 1.0 + CENTRE_MARGIN);
    engine(app, "collage.move", &json!({ "item": p.item, "centre": [keep(p.centre[0]), keep(p.centre[1])] }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Turn {
    item: u32,
    angle: f32,
    scale: f32,
}

fn turn(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Turn = parse(params)?;
    item(p.item)?;
    if !(p.angle.is_finite() && (-360.0..=360.0).contains(&p.angle)) {
        return Err(CmdError::BadParams("`angle` is −360 … 360 degrees".into()));
    }
    if !(p.scale.is_finite() && (SCALE_MIN..=SCALE_MAX).contains(&p.scale)) {
        return Err(CmdError::BadParams(format!("`scale` is {SCALE_MIN} … {SCALE_MAX}")));
    }
    need(pile, app, params)?;
    engine(app, "collage.move", &json!({ "item": p.item, "angle": f64::from(p.angle), "scale": f64::from(p.scale) }))
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum To {
    Top,
    Bottom,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Order {
    #[serde(default)]
    item: Option<u32>,
    to: To,
}

fn order(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Order = parse(params)?;
    p.item.map(item).transpose()?;
    need(one_picture, app, params)?;
    let n = chosen(app, p.item)?;
    let to = match p.to {
        To::Top => "top",
        To::Bottom => "bottom",
    };
    renumbering(app, "collage.order", &json!({ "item": n, "to": to }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Swap {
    a: u32,
    b: u32,
}

fn swap(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Swap = parse(params)?;
    item(p.a)?;
    item(p.b)?;
    if p.a == p.b {
        return Err(CmdError::BadParams("`a` and `b` are two different pictures".into()));
    }
    need(not_pile, app, params)?;
    renumbering(app, "collage.swap", &json!({ "a": p.a, "b": p.b }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    #[serde(default)]
    items: Option<Vec<u32>>,
}

fn remove(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Remove = parse_object(params)?;
    items(p.items.as_deref().unwrap_or_default())?;
    need(some_pictures, app, params)?;
    let list = p.items.unwrap_or_else(|| app.state().collage.selection.clone());
    let ids = list.iter().map(|n| photo_of(app, *n)).collect::<Result<Vec<_>, _>>()?;
    let reply = engine(app, "collage.unplace", &json!({ "ids": ids }))?;
    app.state_mut().collage.selection.clear();
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    #[serde(default)]
    item: Option<u32>,
}

/// The photo a command on one picture acts on (`item`, or the one selected last), its checks
/// passed.
fn one(app: &mut App, params: &Value, check: Check) -> Result<PhotoId, CmdError> {
    let p: One = parse_object(params)?;
    p.item.map(item).transpose()?;
    need(check, app, params)?;
    let n = chosen(app, p.item)?;
    photo_of(app, n)
}

fn set_background(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = one(app, params, background)?;
    engine(app, "collage.background", &json!({ "id": id }))
}

fn set_frame_center(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = one(app, params, frame_mosaic)?;
    renumbering(app, "collage.frameCenter", &json!({ "id": id }))
}

fn view_and_edit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let id = one(app, params, one_picture)?;
    app.state_mut().room = Room::Library;
    app.run("view.edit", &json!({ "id": id }))
}

fn scramble(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(has_collage, app, params)?;
    renumbering(app, "collage.scramble", &json!({}))
}

fn shuffle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need(has_collage, app, params)?;
    renumbering(app, "collage.shuffle", &json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    /// An app with the room open on a collage of `n` photos in `style` (seeded, so it lays out the
    /// same every run).
    fn with_collage(n: usize, style: &str) -> (tempfile::TempDir, App, Vec<PhotoId>) {
        let (dir, s, ids) = session_with_photos(n);
        let mut app = App::new(Some(s));
        app.run("create.collage", &json!({ "ids": ids, "seed": "c011a6e" })).unwrap();
        app.run("collage.set", &json!({ "style": style })).unwrap();
        crate::commands::collage::show_room(&mut app);
        (dir, app, ids)
    }

    #[test]
    fn the_canvas_checks_its_parameters_and_the_room() {
        let mut app = App::new(None);
        assert!(matches!(app.run("collageCanvas.select", &json!({ "items": [1] })), Err(CmdError::Disabled(_))), "no room");
        crate::commands::collage::show_room(&mut app);
        // The stub room (no collage of the engine's): the selection is view state, changes wait.
        assert_eq!(app.run("collageCanvas.select", &json!({ "items": [3, 1, 3] })).unwrap(), json!({ "items": [3, 1] }));
        app.run("collageCanvas.select", &json!({ "items": [2], "add": true })).unwrap();
        assert_eq!(app.state().collage.selection, [3, 1, 2]);
        assert!(matches!(app.run("collageCanvas.select", &json!({ "items": [1000] })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("collageCanvas.remove", &json!({})), Err(CmdError::Disabled(m)) if m == LATER));
        app.run("collageCanvas.selectNone", &json!({})).unwrap();
        assert!(app.state().collage.selection.is_empty());
        for (id, bad) in [
            ("collageCanvas.move", json!({ "item": 0, "centre": [0.5, 3] })),
            ("collageCanvas.turn", json!({ "item": 0, "angle": 0, "scale": 0 })),
            ("collageCanvas.turn", json!({ "item": 0, "angle": 400, "scale": 1 })),
            ("collageCanvas.swap", json!({ "a": 2, "b": 2 })),
            ("collageCanvas.order", json!({ "to": "middle" })),
            ("collageCanvas.remove", json!({ "items": [5000] })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
    }

    #[test]
    fn the_selection_follows_its_pictures_and_remove_empties_it() {
        let (_dir, mut app, _) = with_collage(4, "pile");
        let before = photos(&mut app);
        app.run("collageCanvas.select", &json!({ "items": [0, 2] })).unwrap();
        // Bring to Top: the picture selected last goes last, and the selection goes with it.
        let r = app.run("collageCanvas.order", &json!({ "to": "top" })).unwrap();
        let after = photos(&mut app);
        assert_eq!(after.last(), before.get(2));
        let held: Vec<PhotoId> = app.state().collage.selection.iter().map(|n| after[*n as usize]).collect();
        assert_eq!(held, [before[0], before[2]]);
        assert_eq!(r["items"], json!(app.state().collage.selection));
        // In a pile there is nothing to swap; Remove takes the selection off the page.
        assert!(matches!(app.run("collageCanvas.swap", &json!({ "a": 0, "b": 1 })), Err(CmdError::Disabled(m)) if m == NOT_IN_PILE));
        assert!(matches!(app.run("collageCanvas.select", &json!({ "items": [4] })), Err(CmdError::NotFound(_))));
        app.run("collageCanvas.remove", &json!({})).unwrap();
        assert_eq!((photos(&mut app).len(), app.state().collage.selection.len()), (2, 0));
        assert!(matches!(app.run("collageCanvas.setBackground", &json!({})), Err(CmdError::Disabled(m)) if m == SELECT_FIRST));
    }
}
