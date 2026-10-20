//! Cameras in the Import room (`organizing.md` §3; the state is `crate::camera`): `camera.list`,
//! the cameras and phones connected over PTP/MTP, and `camera.open`, a camera's storage as the
//! Import room's source. Neither is an undo step and neither talks to a device on the owner
//! thread: the list is `dalica-scan`'s cached one, and the listing runs on the import's worker. The
//! import itself, and "Delete copied files from the card" for a camera, are `import.run` and
//! `import.removeFromSource`.

use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "camera.list",
            label: "Cameras",
            menu: "",
            keys: &[],
            params: r#"{}: the cameras and phones connected over PTP/MTP, never waiting on a device (the list is refreshed off the owner thread at most every 2 s): {cameras: [{id, name, vendor, model, access: "open"|"held"|"denied"|"failed", reason (why it cannot be opened, or null), storages: [{id, name, capacity, free}]}], usb (whether this system reaches cameras over USB at all)}. A camera another program or driver holds is listed with its reason and is never taken from it"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "camera.open",
            label: "Open Camera",
            menu: "",
            keys: &[],
            params: r#"{"id":text, "storage"?:int}: list the camera id (from camera.list; its storage storage, else every storage) in the Import room, as import.openSource lists a card: drops the listing before it and starts a new one on a worker that opens the camera's session; replies {generation, source: {kind: "camera", name, path}, excludeDuplicates} at once. Its pictures (and videos, with the Video file type on) come with the camera's thumbnails; import.list, import.run and import.removeFromSource then work as for a card. A camera that cannot be opened is refused with its reason. No undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: open,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    Ok(s.camera_list())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenParams {
    id: String,
    storage: Option<u32>,
}

fn open(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let OpenParams { id, storage } = params(p)?;
    if id.trim().is_empty() {
        return Err(CmdError::BadParams("`id` is empty: give a camera from camera.list".into()));
    }
    s.import_open_camera(&id, storage)
}
