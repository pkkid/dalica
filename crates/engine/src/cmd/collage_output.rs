//! Create Collage, Save Draft and Edit Collage (`creating.md` §1 and §3; the state and the jobs are
//! `crate::collage_output`): `collage.create {path?, display?}` and `collage.saveDraft {path?,
//! display?, close?}` as a `collage` job (render, then write; no undo step: a collage is a new
//! file, as an export is), `collage.edit {id}` (the room reopened from the photo's recipe, read on a
//! worker) and `collage.canEdit {id}` (the edit view's top bar: whether the photo has a readable
//! recipe beside it). Over automation `path`, or the folder written into without it
//! (`Session::collage_destination`), is checked against the write roots (`control-protocol.md`
//! §5.7).

use std::path::PathBuf;

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::Value;

use crate::collage_output::{NOT_OPEN, Request};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "collage.create",
            label: "Create Collage",
            menu: "",
            keys: &[],
            params: r#"{"path"?:string (an absolute folder, made when missing; else the folder of the collage the room was reopened from, else <Pictures>/Dalica/Collages, marked as the Collages project), "display"?:[w, h] (the display's size in pixels: a Current Display page is written at its long side)}: renders the open collage as a JPEG (3000 px on the long side, or the display's size for Current Display; quality 92, the date of making as its date) with its .dalica-collage recipe beside it, as a collage job (render, then write; Cancel during the render writes nothing). Never replaces a file: "Collage 2026-10-19.jpg", else "-001", … The new photo joins the library and the window opens it in the edit view; no undo step. Replies {job, pending:true, total:2, folder, name, draft:false, size}; the job's result is {path, recipe, photo, draft, folder, size, unread, cancelled, failed}"#,
            effect: Effect::Files,
            history: History::None,
            enabled: room_open,
            run: create,
        },
        CommandSpec {
            id: "collage.saveDraft",
            label: "Save Draft",
            menu: "",
            keys: &[],
            params: r#"{"path"?:string, "display"?:[w, h] (as collage.create), "close"?:bool (closes the room once the job has the collage: Close's Save Draft)}: writes the picture and its recipe as collage.create does, with "draft": true, and does not open the edit view. Replies as collage.create, draft:true"#,
            effect: Effect::Files,
            history: History::None,
            enabled: room_open,
            run: save_draft,
        },
        CommandSpec {
            id: "collage.edit",
            label: "Edit Collage",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: reopens the Collage room from the photo's recipe (<stem>.dalica-collage beside it), read on a worker: each clip matched by its id, then by its path among the photos already indexed (a path is never opened); the clips not found are left out and reported. Refused while the open collage has unsaved changes. A Create Collage from the reopened collage writes a new one beside it. Replies {job, pending:true, recipe}; the job's result is {opened, clips, missing:[{id, path}]}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: edit,
        },
        CommandSpec {
            id: "collage.canEdit",
            label: "Can Edit Collage",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: whether the photo has a readable collage recipe beside it (the edit view's Edit Collage button). Answered from what was found for the photo as it is now; a photo not looked at yet starts a look on a worker and answers pending:true (ask again once its job ends). Replies {id, canEdit, pending, job?}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: can_edit,
        },
    ]
}

fn room_open(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.collage().is_open() { Ok(()) } else { Err(Disabled(NOT_OPEN.into())) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputParams {
    path: Option<PathBuf>,
    display: Option<[u32; 2]>,
    #[serde(default)]
    close: bool,
}

fn display_param(display: Option<[u32; 2]>) -> Result<Option<(u32, u32)>, CmdError> {
    match display {
        None => Ok(None),
        Some([w, h]) if dalica_create::collage::Format::display(w, h).is_some() => Ok(Some((w, h))),
        Some(_) => Err(CmdError::BadParams(format!("display: [width, height] in pixels, each 1 to {}", dalica_create::collage::page::MAX_PIXELS))),
    }
}

fn output(s: &mut Session, p: &Value, draft: bool) -> Result<Value, CmdError> {
    let OutputParams { path, display, close } = params(p)?;
    if close && !draft {
        return Err(CmdError::BadParams("close: only Save Draft closes the room".into()));
    }
    let display = display_param(display)?;
    s.start_collage_output(Request { draft, path, display, close })
}

fn create(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    output(s, p, false)
}

fn save_draft(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    output(s, p, true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
}

fn edit(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    s.start_collage_edit(id)
}

fn can_edit(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    s.collage_can_edit(id)
}
