//! The room tab strip's commands (`ui-layout.md` §8): `room.select {room}` brings an open room to
//! the front (a click on its tab), `room.close {room?}` closes one (its ×; the front room without
//! `room`). The Library tab is always open and never closes. Opening a room is its own command
//! (`import.open`, `crate::commands::import`).
//!
//! Closing the Import room while an import job copies asks first through the host: "Stop
//! importing?" [Stop Import] [Keep Importing] (`crate::dialogs::confirm`, [`close_asks`]); Stop
//! Import runs `room.close {confirm: true}`, which cancels the import job (what it copied stays,
//! as for any cancelled job) and closes the room. Keep Importing closes nothing.

use dalica_engine::{CmdError, JobId, JobKind};
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
        params: r#"{room?: "import", confirm?: bool}: close a room (its tab's ×), the front one without `room`; the Library tab never closes; the Import room while an import copies asks "Stop importing?" first unless `confirm` is true, which cancels the import; replies {room} (the room now in front)"#,
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
    #[serde(default)]
    confirm: bool,
}

/// Stop Import's title and default button.
pub const STOP_TITLE: &str = "Stop Import";
/// The question asked closing the Import room while it copies.
pub const STOP_QUESTION: &str = "Stop importing?";
/// Stop Import's Cancel button.
pub const KEEP_IMPORTING: &str = "Keep Importing";

/// The import job copying now, if one is.
pub fn importing(app: &App) -> Option<JobId> {
    let jobs = app.session()?.jobs();
    jobs.into_iter().find(|j| j.kind == JobKind::Import && !j.state.is_finished()).map(|j| j.job)
}

/// The room `room.close` with `params` would close: `params.room`, or the front room.
fn closing(app: &App, params: &Value) -> Option<Room> {
    match params.get("room") {
        None | Some(Value::Null) => Some(app.state().room),
        Some(v) => serde_json::from_value(v.clone()).ok(),
    }
}

/// `room.close` asks "Stop importing?" when it would close the open Import room while an import
/// job copies.
pub fn close_asks(app: &App, params: &Value) -> bool {
    closing(app, params) == Some(Room::Import) && app.state().rooms.contains(&Room::Import) && importing(app).is_some()
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
    if !app.state().rooms.contains(&room) {
        return Err(CmdError::NotFound(format!("the {} room is not open", room.name())));
    }
    if room == Room::Import
        && let Some(job) = importing(app)
    {
        if !p.confirm {
            return Err(CmdError::Disabled(format!("{STOP_QUESTION} Close the Import room with `confirm: true` to stop the import.")));
        }
        if let Some(session) = app.session_mut() {
            session.cancel_jobs(Some(job))?;
        }
    }
    let state = app.state_mut();
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
