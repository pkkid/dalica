//! `import.run {ids?, destination, folderName, after?}`, `import.status` and
//! `import.removeFromSource {confirm}` (`organizing.md` §3 items 4–6): copying, verifying and
//! entering the photos the Import room chose as one undo step, what the room shows of it, and
//! removing the copied files from the card (`import_run.rs`). None has keys or a menu item: the
//! room's buttons run them. Every check runs on the owner thread without the disk; the copying and
//! the removal run as `import` jobs.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::import::MAX_IDS;
use crate::import_run::FolderName;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;
use crate::settings::AfterImport;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "import.run",
            label: "Import",
            menu: "",
            keys: &[],
            params: r#"{"ids"?:[int], "destination":path, "folderName":text | {"dateTaken":true} | {"today":true}, "after"?:"leave"|"delete"}: copies the candidates of the open listing (import.list's ids; omitted: every candidate) that are not excluded and, with Exclude duplicates on, not duplicates, into <destination>/<folder>: the text given (made fit for a folder name), one folder per capture date (YYYY-MM-DD) for dateTaken, the session's date for today. A folder is made when new and imported into when it exists; every copy is verified byte for byte, keeps the source's modification time and never replaces a file (a taken name gets -001); the destination becomes a watched folder (Scan Always) unless one covers it. Runs as an import job (Cancel stops after the current file); the photos copied enter the library with the stars and rotations chosen in the room, as one undo step "Import" whose undo sends the copies still unchanged to the trash and whose redo copies them again. after (default: the last choice) is remembered; "delete" removes nothing by itself (import.removeFromSource asks). An import with nothing left to copy ends at once and reports what it skipped. Refused while the source is still listed or an import runs. Replies {job, pending, total, destination, folders (planned), skipped {duplicates, excluded}, watched}; import.status gives the result"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_run,
            run,
        },
        CommandSpec {
            id: "import.status",
            label: "Import Status",
            menu: "",
            keys: &[],
            params: r#"{}: the import running or last finished in this session: {state: null|"copying"|"done"|"cancelled", job, status ("Copying 45 of 340 at 38 MB/s", "340 pictures imported"), source {kind, name, path}, destination, after, total, imported, folders, skipped {duplicates, excluded}, failed [{file, reason}], bytes, seconds, speed ("38 MB/s" or null), removable (from a card or a drive), undoBlocked (why Undo Import is disabled, or null), removal (null, or {job, running, removed, trashed, deleted, kept [{file, reason}]})}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: status,
        },
        CommandSpec {
            id: "import.removeFromSource",
            label: "Delete Copied Files from the Card",
            menu: "",
            keys: &[],
            params: r#"{"confirm"?:bool}: removes from the card or drive of the last finished import the files it copied and verified, and nothing else (never a duplicate or an excluded file): each only while it is unchanged since its copy and the copy is still the verified one, to the card's trash where it has one and otherwise deleted permanently. Asks first: without "confirm": true it is refused with the question ("Remove the imported files from the card? This cannot be undone."). Never runs over the control channel or MCP. From then on Undo Import is disabled ("The originals were removed from the card."). Runs as an import job; replies {job, pending, total, source}; import.status gives the result"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_remove,
            run: remove,
        },
    ]
}

fn can_run(s: &Session, _: &Value) -> Result<(), Disabled> {
    match s.import_run_disabled() {
        None => Ok(()),
        Some(why) => Err(Disabled(why.into())),
    }
}

fn can_remove(s: &Session, _: &Value) -> Result<(), Disabled> {
    match s.import_remove_disabled() {
        None => Ok(()),
        Some(why) => Err(Disabled(why)),
    }
}

/// `folderName`: a text, `{"dateTaken": true}` or `{"today": true}`.
#[derive(Deserialize)]
#[serde(untagged)]
enum FolderParam {
    Text(String),
    DateTaken(DateTaken),
    Today(Today),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DateTaken {
    date_taken: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Today {
    today: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RunParams {
    ids: Option<Vec<u64>>,
    destination: PathBuf,
    folder_name: FolderParam,
    after: Option<String>,
}

fn run(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let RunParams { ids, destination, folder_name, after } = params(p)?;
    let folder = match folder_name {
        FolderParam::Text(t) => FolderName::Text(t),
        FolderParam::DateTaken(DateTaken { date_taken: true }) => FolderName::DateTaken,
        FolderParam::Today(Today { today: true }) => FolderName::Today,
        FolderParam::DateTaken(_) | FolderParam::Today(_) => {
            return Err(CmdError::BadParams("`folderName` is a text, {\"dateTaken\": true} or {\"today\": true}".into()));
        }
    };
    let after = match after.as_deref() {
        None => None,
        Some(a) => Some(AfterImport::from_id(a).ok_or_else(|| CmdError::BadParams(format!("`after` is \"leave\" or \"delete\", not {a:?}")))?),
    };
    if let Some(ids) = &ids {
        if ids.is_empty() {
            return Err(CmdError::BadParams("`ids` is empty (leave it out to import every candidate)".into()));
        }
        if ids.len() > MAX_IDS {
            return Err(CmdError::Limit(format!("at most {MAX_IDS} ids at once")));
        }
    }
    s.import_start(ids.as_deref(), destination, folder, after)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn status(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    Ok(s.import_status())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveParams {
    confirm: Option<bool>,
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let RemoveParams { confirm } = params(p)?;
    s.import_remove(confirm.unwrap_or(false))
}
