//! The photo viewer's engine commands (`viewing.md` §3; the session is `crate::viewer`):
//!
//! - `viewer.list {path?, refresh?}`: the photos Dalica can show in the file's folder, in name
//!   order, and the file's place among them (read on a worker: `{pending: true}` until there);
//! - `viewer.star {path, on?}`, `viewer.rotate {path, direction}` and `viewer.delete {path,
//!   confirm?}`: the viewer's changes to a listed photo, through `photo.star.*`,
//!   `photo.rotate.*` and `photo.delete`; the first change to a photo of a folder the library does
//!   not hold adds that folder as Scan Once in the same undo step (`viewer.rs`);
//! - `viewer.adopt {path}`: what Open in Dalica runs: the photo's folder joins the library as Scan
//!   Once if it is not there (one undo step), and its id is replied.
//!
//! The changes and `viewer.adopt` are disabled with "Dalica is already open" while another Dalica
//! holds the library. The viewer's keys and view state are the UI's (`viewer.next`, `viewer.close`
//! …): these ids leave those alone.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "viewer.list",
            label: "Photo Viewer Photos",
            menu: "",
            keys: &[],
            params: r#"{"path"?:string (absolute; the viewer's file when left out), "refresh"?:bool}: the photos Dalica can show in the file's folder, in name order, and the file's place: {file, folder, index, photos:[{name, path, id (null outside the library), star, width, height}], inLibrary, readOnly, reason}; only the viewer's folder or a folder Dalica indexes; read on a worker: {"pending":true} until it is there (ask again after a frame); a file Dalica cannot show is refused with its reason"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "viewer.star",
            label: "Star / Unstar",
            menu: "",
            keys: &[],
            params: r#"{"path":string (a photo viewer.list lists), "on"?:bool (toggles when left out)}: stars or unstars the photo; a photo of a folder outside the library adds its folder as Scan Once in the same undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: change_enabled,
            run: star,
        },
        CommandSpec {
            id: "viewer.rotate",
            label: "Rotate",
            menu: "",
            keys: &[],
            params: r#"{"path":string (a photo viewer.list lists), "direction":"cw"|"ccw"}: turns the photo a quarter turn; a photo of a folder outside the library adds its folder as Scan Once in the same undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: change_enabled,
            run: rotate,
        },
        CommandSpec {
            id: "viewer.delete",
            label: "Delete",
            menu: "",
            keys: &[],
            params: r#"{"path":string (a photo viewer.list lists), "confirm"?:bool}: moves the photo's file to the OS trash as photo.delete does (asks first: needs "confirm": true unless "Delete from disk without asking" is on); a photo of a folder outside the library adds its folder as Scan Once first, as an undo step of its own. Replies photo.delete's {job, pending:true, total}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: change_enabled,
            run: delete,
        },
        CommandSpec {
            id: "viewer.adopt",
            label: "Open in Dalica",
            menu: "",
            keys: &[],
            params: r#"{"path":string (a photo viewer.list lists)}: what Open in Dalica runs: the photo's folder joins the library as Scan Once when it is not there (one undo step); replies {id}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: change_enabled,
            run: adopt,
        },
    ]
}

/// The viewer's changes and Open in Dalica: disabled while another Dalica holds the library.
fn change_enabled(s: &Session, _: &Value) -> Result<(), Disabled> {
    match s.viewer_change_disabled() {
        Some(why) => Err(Disabled(why)),
        None => Ok(()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    path: Option<PathBuf>,
    #[serde(default)]
    refresh: bool,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let List { path, refresh } = params(p)?;
    s.viewer_list(path.as_deref(), refresh)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Star {
    path: PathBuf,
    on: Option<bool>,
}

fn star(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Star { path, on } = params(p)?;
    let id = s.viewer_adopt(&path)?;
    let mut reply = match on {
        Some(on) => delegate(s, "photo.star.set", &json!({ "ids": [id], "on": on }))?,
        None => delegate(s, "photo.star.toggle", &json!({ "ids": [id] }))?,
    };
    let starred = s.library().photo(id).is_some_and(|ph| ph.star);
    with(&mut reply, id, json!({ "star": starred }));
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rotate {
    path: PathBuf,
    direction: String,
}

fn rotate(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Rotate { path, direction } = params(p)?;
    let command = match direction.as_str() {
        "cw" => "photo.rotate.cw",
        "ccw" => "photo.rotate.ccw",
        other => return Err(CmdError::BadParams(format!("`direction` is \"cw\" or \"ccw\", not {other:?}"))),
    };
    let id = s.viewer_adopt(&path)?;
    let mut reply = delegate(s, command, &json!({ "ids": [id] }))?;
    with(&mut reply, id, json!({}));
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    path: PathBuf,
    #[serde(default)]
    confirm: bool,
}

fn delete(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Delete { path, confirm } = params(p)?;
    let id = s.viewer_adopt(&path)?;
    let mut params = json!({ "ids": [id] });
    if confirm {
        params["confirm"] = json!(true);
    }
    let mut reply = delegate(s, "photo.delete", &params)?;
    s.viewer_forget(&path);
    with(&mut reply, id, json!({}));
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adopt {
    path: PathBuf,
}

fn adopt(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Adopt { path } = params(p)?;
    let id = s.viewer_adopt(&path)?;
    Ok(json!({ "id": id, "path": path }))
}

/// Runs the library command `id` inside the running viewer command (its change joins this
/// command's undo step), after its own `enabled` check.
fn delegate(s: &mut Session, id: &str, params: &Value) -> Result<Value, CmdError> {
    let spec = *s.spec(id).ok_or_else(|| CmdError::Internal(format!("no command {id}")))?;
    (spec.enabled)(s, params).map_err(|d| CmdError::Disabled(d.0))?;
    (spec.run)(s, params)
}

/// `reply` (an object, or replaced by one) with the photo's `id` and `extra`'s fields.
fn with(reply: &mut Value, id: dalica_core::PhotoId, extra: Value) {
    if !reply.is_object() {
        *reply = json!({});
    }
    if let (Some(r), Some(e)) = (reply.as_object_mut(), extra.as_object()) {
        r.insert("id".into(), json!(id));
        for (k, v) in e {
            r.insert(k.clone(), v.clone());
        }
    }
}
