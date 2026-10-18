//! The face scan's commands (`faces.md` §4 and §9; the scan is `crate::faces_scan`, its index
//! `crate::faces_index`): `faces.enable {on}`, `faces.detect {ids, apply?}`, `faces.index`,
//! `faces.reset {ids}` (Picture ▸ Find Faces Again) and `faces.removeInFolders {folders, confirm}`
//! (the Folder Manager's Remove Faces).
//!
//! - **Find Faces Again** and **Remove Faces** take the faces the scan found (unnamed and ignored)
//!   off the photos as one library undo step; named faces are the user's and stay. Find Faces
//!   Again then has the photos searched again, first; Remove Faces leaves them searched, so the
//!   faces do not come back.
//! - Every command gives the scan its turn first ([`Session::poll_face_scan`]), so a script that
//!   only asks `faces.index` sees it move.
//! - Replies and errors hold counts and ids, never a name, a rectangle or an embedding.

use std::collections::BTreeMap;
use std::path::PathBuf;

use dalica_core::PhotoId;
use dalica_library::{Face, FacePerson, Library, Op};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::faces::face_id;
use crate::error::CmdError;
use crate::faces_scan::DROP_OFFER;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// The Folder Manager's question before Remove Faces (`organizing.md` §2); without
/// `"confirm": true`, `faces.removeInFolders` is `disabled` with it.
pub const REMOVE_QUESTION: &str =
    "Remove the face rectangles found in these folders? Names you've given stay with the photos that are still scanned.";
/// The most groups `faces.index {groups: true}` lists, and faces per group.
pub const MAX_GROUPS_LISTED: usize = 1_000;
pub const MAX_GROUP_FACES_LISTED: usize = 1_000;
/// The most folders one `faces.removeInFolders` names.
pub const MAX_FOLDERS: usize = 1_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "faces.enable",
            label: "Find Faces in My Photos",
            menu: "",
            keys: &[],
            params: r#"{"on":bool, "dropEmbeddings"?:bool (with on false: also delete every model's cached face data; rectangles and names stay)}: Options ▸ People ▸ Find faces in my photos. On starts the face scan (a recogniser must be installed); off stops it and, without dropEmbeddings, replies the offer to delete the cached data. Replies {on, dropped, offer:string|null}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: enable,
        },
        CommandSpec {
            id: "faces.detect",
            label: "Find Faces Now",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "apply"?:bool=true}: searches the photos for faces now, first, even while the user is busy (one at a time); with apply the faces found are written as Unnamed faces, without it they are only counted (faces.index stats.found). Photos in folders that do not find faces are skipped. Needs a detector and a recogniser. Replies {photos, skipped, job}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: detect,
        },
        CommandSpec {
            id: "faces.index",
            label: "Face Scan Status",
            menu: "",
            keys: &[],
            params: r#"{"groups"?:bool (also list the groups of unnamed faces)}: the face scan for the People group's header: {state:"off"|"noModel"|"loading"|"running"|"paused"|"idle"|"cancelled", model:id|null, detector, problem:string|null, done, total, label:"Finding faces… 312 of 4,980"|null, job:JobId|null, pace:"pause"|"light"|"normal"|"full", faces (embedded), groups (count), stats:{started, read, searched, found, embedded, failed}} and with groups true {groupList:[["<PhotoId>.<n>", …], …]} largest first, each nearest its centre first (at most 1000 groups of 1000 faces)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: index,
        },
        CommandSpec {
            id: "faces.reset",
            label: "Find Faces Again",
            menu: "Picture/Find Faces Again",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: Find Faces Again: the faces found on the photos (unnamed and ignored) go, named faces stay, and the photos are searched again first. One undo step. Replies {photos, removed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: reset,
        },
        CommandSpec {
            id: "faces.removeInFolders",
            label: "Remove Faces",
            menu: "",
            keys: &[],
            params: r#"{"folders":[path], "confirm":bool}: the Folder Manager's Remove Faces: the faces found on the photos in these folders and their subfolders (unnamed and ignored) go; named faces stay. Asks first: needs "confirm": true. One undo step. Replies {photos, removed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: remove_in_folders,
        },
    ]
}

/// Disabled with no photos to act on.
fn has_ids(_: &Session, p: &Value) -> Result<(), crate::registry::Disabled> {
    match p.get("ids").and_then(Value::as_array) {
        Some(list) if list.is_empty() => Err(crate::registry::Disabled("Select some photos first.".into())),
        _ => Ok(()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EnableParams {
    on: bool,
    #[serde(default)]
    drop_embeddings: bool,
}

fn enable(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: EnableParams = params(p)?;
    if p.on && p.drop_embeddings {
        return Err(CmdError::BadParams("dropEmbeddings goes with on false only".into()));
    }
    s.change_settings(|st| {
        let mut people = st.people();
        people.find_faces = p.on;
        st.set_people(&people);
    });
    if p.on {
        s.resume_face_scan();
    } else if p.drop_embeddings {
        s.drop_face_embeddings();
    }
    s.poll_face_scan();
    let offer = (!p.on && !p.drop_embeddings).then_some(DROP_OFFER);
    Ok(json!({ "on": p.on, "dropped": p.drop_embeddings, "offer": offer }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectParams {
    ids: Vec<PhotoId>,
    #[serde(default = "yes")]
    apply: bool,
}

fn yes() -> bool {
    true
}

fn detect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: DetectParams = params(p)?;
    let ids = crate::cmd::photo::known(s, p.ids)?;
    s.poll_face_scan();
    if s.face_scan().models_dir().is_none() {
        return Err(CmdError::Disabled("Finding faces needs a face model: add one in Options ▸ People.".into()));
    }
    if !s.face_scan().has_detector() {
        let why = if s.faces_index().model().is_none() { "No face recogniser is ready yet." } else { "No face detector is installed." };
        return Err(CmdError::Disabled(why.into()));
    }
    let (read, skipped): (Vec<PhotoId>, Vec<PhotoId>) = ids.into_iter().partition(|id| s.face_scan_may_read(*id));
    s.search_faces_again(&read, p.apply);
    s.poll_face_scan();
    Ok(json!({ "photos": read.len(), "skipped": skipped.len(), "job": s.face_scan().job() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexParams {
    #[serde(default)]
    groups: bool,
}

fn index(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: IndexParams = params(p)?;
    s.poll_face_scan();
    let mut reply = s.face_scan_status();
    if p.groups {
        let lib = s.library();
        let list: Vec<Vec<String>> = s
            .faces_index()
            .groups()
            .iter()
            .take(MAX_GROUPS_LISTED)
            .map(|g| g.iter().filter_map(|f| f.resolve(lib)).take(MAX_GROUP_FACES_LISTED).map(face_id).collect())
            .collect();
        if let Some(o) = reply.as_object_mut() {
            o.insert("groupList".into(), json!(list));
        }
    }
    Ok(reply)
}

/// The op taking the found faces (unnamed and ignored) off `ids`, one `SetFaces`, and how many
/// faces it takes; `None` when there are none.
fn without_found(lib: &Library, ids: &[PhotoId]) -> (Option<Op>, usize) {
    let mut removed = 0usize;
    let mut changes: BTreeMap<PhotoId, Vec<Face>> = BTreeMap::new();
    for id in ids {
        let faces = lib.faces(*id);
        let kept: Vec<Face> = faces.iter().filter(|f| matches!(f.person, FacePerson::Person(_))).cloned().collect();
        if kept.len() != faces.len() {
            removed += faces.len() - kept.len();
            changes.insert(*id, kept);
        }
    }
    if changes.is_empty() { (None, 0) } else { (Some(Op::SetFaces { faces: changes.into_iter().collect() }), removed) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetParams {
    ids: Vec<PhotoId>,
}

fn reset(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: ResetParams = params(p)?;
    let ids = crate::cmd::photo::known(s, p.ids)?;
    let (op, removed) = without_found(s.library(), &ids);
    if let Some(op) = op {
        s.apply(op)?;
    }
    s.search_faces_again(&ids, true);
    s.poll_face_scan();
    Ok(json!({ "photos": ids.len(), "removed": removed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveParams {
    folders: Vec<PathBuf>,
    #[serde(default)]
    confirm: bool,
}

fn remove_in_folders(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: RemoveParams = params(p)?;
    if p.folders.is_empty() {
        return Err(CmdError::BadParams("give at least one folder".into()));
    }
    if p.folders.len() > MAX_FOLDERS {
        return Err(CmdError::Limit(format!("at most {MAX_FOLDERS} folders at once")));
    }
    let lib = s.library();
    let mut ids = Vec::new();
    for root in &p.folders {
        let inside: Vec<_> = lib.folders().filter(|f| f.path.starts_with(root)).map(|f| f.id).collect();
        if inside.is_empty() {
            return Err(CmdError::NotFound(format!("the library has no folder at or under {}", root.display())));
        }
        for f in inside {
            ids.extend(lib.photos_in(f).into_iter().map(|p| p.id));
        }
    }
    ids.sort_unstable();
    ids.dedup();
    if !p.confirm {
        return Err(CmdError::Disabled(REMOVE_QUESTION.into()));
    }
    s.poll_face_scan();
    let (op, removed) = without_found(s.library(), &ids);
    if let Some(op) = op {
        s.apply(op)?;
    }
    Ok(json!({ "photos": ids.len(), "removed": removed }))
}
