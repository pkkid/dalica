//! The room tab strip's commands (`ui-layout.md` §8): `room.select {room}` brings an open room to
//! the front (a click on its tab), `room.close {room?}` closes one (its ×; the front room without
//! `room`). The Library tab is always open and never closes. Opening a room is its own command
//! (`import.open`, `crate::commands::import`).
//!
//! M11.2 lays the ground (the view state's `room` and `rooms`, the strip's region, these two
//! commands); M11.13 draws the strip with the room tabs and asks before closing Import while it
//! copies.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::Room;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "room.select",
        label: "Show Room",
        menu: "",
        keys: &[],
        params: r#"{room: "library" | "import"}: bring an open room to the front (its tab); replies {room}"#,
        run: select,
    },
    UiCommand {
        id: "room.close",
        label: "Close Room",
        menu: "",
        keys: &[],
        params: r#"{room?: "import"}: close a room (its tab's ×), the front one without `room`; the Library tab never closes; replies {room} (the room now in front)"#,
        run: close,
    },
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Select {
    room: Room,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Close {
    #[serde(default)]
    room: Option<Room>,
}

/// Opens `room` if it is not open (its tab goes last) and brings it to the front.
pub fn open(app: &mut App, room: Room) -> Value {
    let state = app.state_mut();
    if !state.rooms.contains(&room) {
        state.rooms.push(room);
    }
    state.room = room;
    json!({ "room": room.name() })
}

fn select(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Select = parse(params)?;
    if !app.state().rooms.contains(&p.room) {
        return Err(CmdError::NotFound(format!("the {} room is not open", p.room.name())));
    }
    app.state_mut().room = p.room;
    Ok(json!({ "room": p.room.name() }))
}

fn close(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Close = parse_object(params)?;
    let room = p.room.unwrap_or(app.state().room);
    if room == Room::Library {
        return Err(CmdError::Disabled("The Library tab cannot be closed.".into()));
    }
    let state = app.state_mut();
    if !state.rooms.contains(&room) {
        return Err(CmdError::NotFound(format!("the {} room is not open", room.name())));
    }
    state.rooms.retain(|r| *r != room);
    if state.room == room {
        state.room = Room::Library;
    }
    Ok(json!({ "room": state.room.name() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooms_open_come_to_the_front_and_close_but_the_library_stays() {
        let mut app = App::new(None);
        assert!(matches!(app.run("room.select", &json!({ "room": "import" })), Err(CmdError::NotFound(_))), "not open");
        assert!(matches!(app.run("room.close", &json!({})), Err(CmdError::Disabled(_))), "the Library tab");
        assert_eq!(open(&mut app, Room::Import), json!({ "room": "import" }));
        assert_eq!(app.state().rooms, [Room::Library, Room::Import]);
        assert_eq!(app.run("room.select", &json!({ "room": "library" })).unwrap(), json!({ "room": "library" }));
        assert_eq!(app.state().room, Room::Library);
        assert_eq!(app.run("room.close", &json!({ "room": "import" })).unwrap(), json!({ "room": "library" }));
        assert_eq!(app.state().rooms, [Room::Library]);
        assert!(matches!(app.run("room.close", &json!({ "room": "import" })), Err(CmdError::NotFound(_))));
        for bad in [json!({ "room": "collage" }), json!({}), json!([1])] {
            assert!(matches!(app.run("room.select", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }
}
