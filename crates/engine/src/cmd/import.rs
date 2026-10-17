//! `import.sources`, `import.open`, `import.list`, `import.exclude`, `import.excludeAll`,
//! `import.mark`, `import.options` and `import.close` (`organizing.md` §3 items 1–3): the Import
//! room's source, its listing and the room's choices (`import.rs`). None is an undo step and none
//! touches the disk on the owner thread: the listing runs on a worker, and the queries read what it
//! found so far. The room's Ctrl+M opener is the UI's own `import.open` without parameters; these
//! have no keys and no menu items.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::import::{MAX_IDS, MAX_PAGE, NOTHING_OPEN};
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "import.sources",
            label: "Import Sources",
            menu: "",
            keys: &[],
            params: r#"{}: the sources to import from, never waiting on the disk: {sources: [{kind: "card"|"drive", name, path, root} | {kind: "recent", name, path}]}, cards and drives from the device list (refreshed off the owner thread at most every 2 s; a card is a volume holding DCIM, path its DCIM folder; a drive another removable volume, path its root), then the recent sources not mounted as a device"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: sources,
        },
        CommandSpec {
            id: "import.open",
            label: "Open Import Source",
            menu: "",
            keys: &[],
            params: r#"{"device"?:path, "path"?:path}: list a source in the Import room, exactly one of: device, a card's or drive's path (or root) from import.sources; path, an absolute folder (automation checks it against its read roots). Drops the listing before it and starts a new one on a worker; replies {generation, source: {kind, name, path}, excludeDuplicates} at once (excludeDuplicates from Options ▸ General ▸ Detect duplicates while importing). A folder that cannot be read shows in import.list's status. No undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: open,
        },
        CommandSpec {
            id: "import.list",
            label: "Import Listing",
            menu: "",
            keys: &[],
            params: r#"{"offset"?:int, "limit"?:int}: the listing so far: {generation, source, state: "loading"|"done"|"cancelled"|"failed", status ("Loading 112 of 340 files…", "340 files", "No pictures found"), progress {files, done, batches}, total, duplicates, excluded, toImport, excludeDuplicates, truncated, warnings [{path, reason}], warningCount, offset, groups [{start, end, count}] (capture times more than 2 hours apart start a group), candidates}; candidates in capture order from offset, at most limit (default and most 1000): [{id, name, path, folder, size, captured, timeSource: "metadata"|"file", group, duplicate: null | {photo: PhotoId} | {candidate: id} (the same size, MD5 digest and bytes as a library photo or an earlier candidate), excluded, willImport, starred, rotation (0, 90, 180, 270), thumbnail (ready in memory)}]. With nothing open: source null and no candidates"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "import.exclude",
            label: "Exclude",
            menu: "",
            keys: &[],
            params: r#"{"ids":[int], "on":bool}: exclude (on) or include the candidates ids (import.list's ids); an excluded candidate is never copied. Replies {total, toImport, excluded, duplicates, excludeDuplicates}. The room's choice, no undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: open_source,
            run: exclude,
        },
        CommandSpec {
            id: "import.excludeAll",
            label: "Exclude All",
            menu: "",
            keys: &[],
            params: r#"{"on":bool}: Exclude All (on) or Include All: every candidate, those still to be listed too, each one's own choice cleared. Replies as import.exclude. No undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: open_source,
            run: exclude_all,
        },
        CommandSpec {
            id: "import.mark",
            label: "Mark for Import",
            menu: "",
            keys: &[],
            params: r#"{"ids":[int], "star"?:bool, "rotate"?:int}: the room's star (set or cleared) and rotation (rotate: quarter turns clockwise to add, -3 to 3) for the candidates ids; the import gives the photos these. Replies as import.exclude. No undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: open_source,
            run: mark,
        },
        CommandSpec {
            id: "import.options",
            label: "Exclude Duplicates",
            menu: "",
            keys: &[],
            params: r#"{"excludeDuplicates":bool}: the room's "Exclude duplicates" (on: duplicates are not copied). Replies as import.exclude. No undo step"#,
            effect: Effect::Query,
            history: History::None,
            enabled: open_source,
            run: options,
        },
        CommandSpec {
            id: "import.close",
            label: "Close Import Source",
            menu: "",
            keys: &[],
            params: r#"{}: drop the listing and the room's choices (the worker stops after its current file); nothing open is no error. Replies {closed, generation}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: close,
        },
    ]
}

fn open_source(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.import_is_open() { Ok(()) } else { Err(Disabled(NOTHING_OPEN.into())) }
}

/// `ids`, checked for size.
fn ids(ids: Vec<u64>) -> Result<Vec<u64>, CmdError> {
    if ids.is_empty() {
        return Err(CmdError::BadParams("`ids` is empty".into()));
    }
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} ids at once")));
    }
    Ok(ids)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn sources(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    Ok(s.import_sources())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenParams {
    device: Option<PathBuf>,
    path: Option<PathBuf>,
}

fn open(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let OpenParams { device, path } = params(p)?;
    s.import_open(device.as_deref(), path.as_deref())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    offset: Option<usize>,
    limit: Option<usize>,
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ListParams { offset, limit } = params(p)?;
    let limit = limit.unwrap_or(MAX_PAGE);
    if limit > MAX_PAGE {
        return Err(CmdError::BadParams(format!("`limit` is at most {MAX_PAGE}")));
    }
    Ok(s.import_list(offset.unwrap_or(0), limit))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExcludeParams {
    ids: Vec<u64>,
    on: bool,
}

fn exclude(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ExcludeParams { ids: list, on } = params(p)?;
    s.import_exclude(&ids(list)?, on)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OnParams {
    on: bool,
}

fn exclude_all(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let OnParams { on } = params(p)?;
    Ok(s.import_exclude_all(on))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkParams {
    ids: Vec<u64>,
    star: Option<bool>,
    rotate: Option<i8>,
}

fn mark(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let MarkParams { ids: list, star, rotate } = params(p)?;
    if star.is_none() && rotate.is_none() {
        return Err(CmdError::BadParams("give `star`, `rotate` or both".into()));
    }
    if rotate.is_some_and(|r| !(-3..=3).contains(&r)) {
        return Err(CmdError::BadParams("`rotate` is a number of quarter turns from -3 to 3".into()));
    }
    s.import_mark(&ids(list)?, star, rotate)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct OptionsParams {
    exclude_duplicates: bool,
}

fn options(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let OptionsParams { exclude_duplicates } = params(p)?;
    Ok(s.import_options(exclude_duplicates))
}

fn close(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    Ok(s.import_close())
}
