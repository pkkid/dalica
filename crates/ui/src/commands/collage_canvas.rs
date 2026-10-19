//! The collage canvas's commands (`creating.md` §3), stubs until M14.19 fills them: the selection
//! of pictures on the canvas acts now (view state), what changes the collage checks its parameters
//! and then refuses with [`LATER`]. A picture is named by its number in the engine's placements
//! (`item:<n>`, 0 … 999).
//!
//! The ids are the UI's own (`collageCanvas.*`), never the engine's (`collage.move`,
//! `collage.swap`, `collage.order`, `collage.background`, `collage.frameCenter`,
//! `collage.scramble`, `collage.shuffle`, `collage.unplace`), which they will run.
//!
//! - **Selecting**: `collageCanvas.select {items, add?}` (a click; Ctrl and Shift add),
//!   `collageCanvas.selectAll` (Select All, Ctrl+A) and `collageCanvas.selectNone` (Select None,
//!   Esc).
//! - **Moving** (a pile): `collageCanvas.move {item, centre}` (a drag) and `collageCanvas.turn
//!   {item, angle, scale}` (the wheel handle: turn and scale together around the centre).
//! - **Arranging**: `collageCanvas.order {item, to}` (Bring to Top / Send to Bottom),
//!   `collageCanvas.swap {a, b}` (the mosaics and the grid: a drag onto another picture),
//!   `collageCanvas.remove {items?}` (Remove, Delete: back to the Clips),
//!   `collageCanvas.setBackground {item}`, `collageCanvas.setFrameCenter {item}`,
//!   `collageCanvas.scramble`, `collageCanvas.shuffle` and `collageCanvas.viewAndEdit {item?}`.
//!
//! The Collage key context (`menus-and-shortcuts.md` §2.0) is active while the room is in front
//! (`crate::keys::view_context`): Delete removes the selection, Ctrl+A selects every picture and
//! Esc none; Ctrl+Z and Ctrl+Y are the engine's `collage.undo` and `collage.redo` (the room's own
//! history, M14.16).

use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::MAX_CLIPS;

/// What the canvas's stub commands say until M14.19 makes them act.
pub const LATER: &str = "Arrives with the collage canvas.";
/// The most a picture is scaled by the wheel handle, and the least.
pub const SCALE_MAX: f32 = 10.0;
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
        params: "{}: select every picture on the canvas",
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
        params: "{item: n, centre: [x, y]}: move a picture of a pile, its centre normalised to the page (−1 … 2)",
        run: move_picture,
    },
    UiCommand {
        id: "collageCanvas.turn",
        label: "Turn and Scale Picture",
        menu: "",
        keys: &[],
        params: "{item: n, angle: degrees (−360 … 360), scale: 0.05 … 10}: turn and scale a picture of a pile around its centre",
        run: turn,
    },
    UiCommand {
        id: "collageCanvas.order",
        label: "Bring to Top / Send to Bottom",
        menu: "",
        keys: &[],
        params: r#"{item?: n, to: "top" | "bottom"}: bring a picture (the selected one without `item`) to the top of the pile or send it to the bottom"#,
        run: order,
    },
    UiCommand {
        id: "collageCanvas.swap",
        label: "Swap Pictures",
        menu: "",
        keys: &[],
        params: "{a: n, b: n}: swap two pictures of a mosaic or the grid",
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
        params: "{item?: n}: put a picture (the selected one without `item`) in the centre of a Frame Mosaic",
        run: set_frame_center,
    },
    UiCommand {
        id: "collageCanvas.scramble",
        label: "Scramble Collage",
        menu: "",
        keys: &[],
        params: "{}: a new random arrangement (the pictures you moved are laid out again)",
        run: scramble,
    },
    UiCommand {
        id: "collageCanvas.shuffle",
        label: "Shuffle Pictures",
        menu: "",
        keys: &[],
        params: "{}: the same places, the pictures dealt to them again",
        run: shuffle,
    },
    UiCommand {
        id: "collageCanvas.viewAndEdit",
        label: "View and Edit",
        menu: "",
        keys: &[],
        params: "{item?: n}: open a picture (the selected one without `item`) in the edit view",
        run: view_and_edit,
    },
];

/// The canvas's commands grey their buttons and menu rows while the room is not open, and those
/// that wait for M14.19 with [`LATER`] while it is.
pub const CHECKS: &[(&str, Check)] = &[
    ("collageCanvas.select", super::collage::room_open),
    ("collageCanvas.selectAll", waits),
    ("collageCanvas.selectNone", super::collage::room_open),
    ("collageCanvas.move", waits),
    ("collageCanvas.turn", waits),
    ("collageCanvas.order", waits),
    ("collageCanvas.swap", waits),
    ("collageCanvas.remove", waits),
    ("collageCanvas.setBackground", waits),
    ("collageCanvas.setFrameCenter", waits),
    ("collageCanvas.scramble", waits),
    ("collageCanvas.shuffle", waits),
    ("collageCanvas.viewAndEdit", waits),
];

/// The room's [`NOT_OPEN`](super::collage::NOT_OPEN) while it is not open, else [`LATER`].
fn waits(app: &App, params: &Value) -> Result<(), String> {
    super::collage::room_open(app, params)?;
    Err(LATER.into())
}

fn need_room(app: &App) -> Result<(), CmdError> {
    super::collage::room_open(app, &Value::Null).map_err(CmdError::Disabled)
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

/// A canvas command that waits for M14.19: its parameters checked, the room open, then [`LATER`].
fn later(app: &App) -> Result<Value, CmdError> {
    need_room(app)?;
    Err(CmdError::Disabled(LATER.into()))
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
    need_room(app)?;
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
    later(app)
}

fn select_none(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    need_room(app)?;
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
    later(app)
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
    later(app)
}

#[derive(Deserialize)]
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
    // M14.19 runs the engine's `collage.order {item, top | bottom}` with it.
    let (To::Top | To::Bottom) = p.to;
    later(app)
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
    later(app)
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
    later(app)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    #[serde(default)]
    item: Option<u32>,
}

/// A command on one picture (`item`, or the selected one): checked, then [`LATER`].
fn one(app: &App, params: &Value) -> Result<Value, CmdError> {
    let p: One = parse_object(params)?;
    p.item.map(item).transpose()?;
    later(app)
}

fn set_background(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    one(app, params)
}

fn set_frame_center(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    one(app, params)
}

fn view_and_edit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    one(app, params)
}

fn scramble(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    later(app)
}

fn shuffle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    later(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_selects_and_its_changes_wait() {
        let mut app = App::new(None);
        assert!(matches!(app.run("collageCanvas.select", &json!({ "items": [1] })), Err(CmdError::Disabled(_))), "no room");
        crate::commands::collage::show_room(&mut app);
        assert_eq!(app.run("collageCanvas.select", &json!({ "items": [3, 1, 3] })).unwrap(), json!({ "items": [3, 1] }));
        app.run("collageCanvas.select", &json!({ "items": [2], "add": true })).unwrap();
        assert_eq!(app.state().collage.selection, [3, 1, 2]);
        assert!(matches!(app.run("collageCanvas.select", &json!({ "items": [1000] })), Err(CmdError::BadParams(_))));
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
        for (id, good) in [
            ("collageCanvas.move", json!({ "item": 0, "centre": [0.5, 0.5] })),
            ("collageCanvas.turn", json!({ "item": 0, "angle": 90, "scale": 2 })),
            ("collageCanvas.order", json!({ "to": "top" })),
            ("collageCanvas.swap", json!({ "a": 0, "b": 1 })),
            ("collageCanvas.remove", json!({})),
            ("collageCanvas.setFrameCenter", json!({ "item": 1 })),
            ("collageCanvas.scramble", json!({})),
        ] {
            assert!(matches!(app.run(id, &good), Err(CmdError::Disabled(m)) if m == LATER), "{id} {good}");
        }
    }
}
