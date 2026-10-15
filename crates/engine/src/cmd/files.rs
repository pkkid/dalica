//! Moving and copying photos (`organizing.md` §9, `data-model.md` §8): `photo.moveTo`,
//! `photo.copyTo` and `photo.moveToNewFolder`, file operations ([`crate::fileops`]) whose items
//! move or copy one photo's file each through `dalica-fs` (never replacing a file; across volumes
//! copied, verified, then the source sent to the trash) and carry its sidecar entry, its originals
//! backup and its record with it.
//!
//! - `photo.moveTo {ids, folder, collisions?, confirm?, dontAsk?}` moves the photos into library
//!   folder `folder`. While "Ask before moving" is on (`"organize"` → `askBeforeMoving`) it needs
//!   `"confirm": true` and otherwise answers the first-time question ([`MOVE_QUESTION`]);
//!   `"dontAsk": true` turns the setting off. Photos already in the folder are left where they are.
//!   Each photo keeps its id, so its star, caption, tags, recipe, album memberships and order and
//!   its backup go with it.
//! - `photo.copyTo {ids, folder, collisions?}` copies them: each copy is a new photo with the
//!   source's data (star, caption, tags, hidden, date, recipe, album memberships at the source's
//!   position key, saved record, search facts and digest) and the source's sidecar entry and
//!   backup duplicated. The UI asks its own question first; the engine never does.
//! - **Clashes.** Without `collisions`, a destination where the library already holds a photo of
//!   the same name (ignoring case), or two photos of the batch with one name, answers the clash
//!   question ([`CLASH_QUESTION`], naming the files, [`clashing_names`]). `"rename"` (Rename
//!   Duplicates) plans `-001`, `-002` … names free in the library; `"skip"` (Skip Duplicates)
//!   leaves those photos and the destination's files untouched, each listed as skipped. A clash
//!   found only on disk (a file the library does not show) takes the next free name, as Rename
//!   would; the journal holds the new name before the file moves there ([`fileops::retarget`]).
//! - `photo.moveToNewFolder {ids, name, date?, useOldest?, place?, description?}` (Move to New
//!   Folder…, Split Folder Here…) makes a folder beside the photos' folder (all of one folder),
//!   named `name` through `core::names::sanitize` ("New Folder" when nothing is left) and the next
//!   free name, gives it the Folder Properties given (`date` a day, `YYYY-MM-DD`, unless
//!   `useOldest`; `place`, `description`), and moves the photos into it, all one undo step. Undo
//!   moves them back, then sends the folder to the trash once it holds nothing but Dalica's own
//!   bookkeeping (its sidecar without photo entries, an originals folder with nothing but its
//!   record); otherwise the folder stays and the step says why. The new folder must lie inside the
//!   watched folders (a folder beside a watched root is refused).
//! - **Undo.** A move undoes by moving each photo back under the name it had; a copy by sending each
//!   copy still unchanged since it was made to the trash (a changed copy stays and says so; one
//!   already gone counts as undone), with the kept original it brought and that original's entry in
//!   the originals record (the originals folder too when nothing else is left in it). Redo runs the
//!   items again with the names they got. A move
//!   whose source's volume had no trash became a copy (the source and its entry kept) and cannot
//!   be undone.
//! - **The journal.** Every item names a `from` and a `to`: a photo's file and where it goes; the
//!   new folder's item and its undo name the folder twice; undoing a copy names the copy and the
//!   photo it was copied from.
//!
//! Every check runs on the owner thread from the model, without the disk; replies are `{job,
//! pending: true, total, folder}` (Move to New Folder adds `folderId`). The operation's end is a
//! [`crate::FileOpEvent`] with the counts, skips and failures.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::names::{self, candidates};
use dalica_core::{Date, FolderId, PhotoId};
use dalica_fs::Identity;
use dalica_fs::transfer::{self, Finished, Moved, Outcome};
use dalica_fs::trash::{self, Sent};
use dalica_library::{Facts, Folder, FolderInfo, Op, Photo, clean_description, clean_place};
use dalica_store::carry::{Carry, Mode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::fileops::{self, Driver, FileOp, FileOpKind, Ran};
use crate::jobs::JobId;
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;
use crate::settings::OrganizeSettings;

/// The first-time move question (`organizing.md` §9).
pub const MOVE_QUESTION: &str = "Move these files on disk? The files will be moved, not copied.";
/// The clash question; the answer names the files.
pub const CLASH_QUESTION: &str = "The folder already has files with the same names.";
/// A new folder's name when the one given leaves nothing.
pub const NEW_FOLDER: &str = "New Folder";
/// How many clashing names the clash question lists before it counts the rest.
pub const NAMED: usize = 5;
/// The undo labels: "Undo Move", "Undo Copy", "Undo Move to New Folder".
pub const MOVE_LABEL: &str = "Move";
pub const COPY_LABEL: &str = "Copy";
pub const NEW_FOLDER_LABEL: &str = "Move to New Folder";
/// How long undoing a new folder waits for the moved-back photos' entries to leave its sidecar
/// (they leave on the I/O worker after their items).
const SIDECAR_PATIENCE: Duration = Duration::from_secs(30);

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.moveTo",
            label: "Move",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "folder":FolderId, "collisions"?:"rename"|"skip", "confirm"?:bool, "dontAsk"?:bool}: moves the photos' files into the library folder (one undo step, as a file job); each keeps its id and data, and its sidecar entry and originals backup travel with it. While "Ask before moving" is on it needs "confirm": true; "dontAsk": true turns the question off. A name the folder already holds needs "collisions": "rename" (-001, -002 …) or "skip". Replies {job, pending:true, total, folder}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: has_ids,
            run: run_move,
        },
        CommandSpec {
            id: "photo.copyTo",
            label: "Copy",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "folder":FolderId, "collisions"?:"rename"|"skip"}: copies the photos' files into the library folder (one undo step, as a file job); each copy is a new photo with the source's data, sidecar entry and originals backup. A name the folder already holds needs "collisions": "rename" (-001, -002 …) or "skip". Undo trashes the copies still unchanged. Replies {job, pending:true, total, folder}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: has_ids,
            run: run_copy,
        },
        CommandSpec {
            id: "photo.moveToNewFolder",
            label: "Move to New Folder",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId] (of one folder), "name":string, "date"?:"YYYY-MM-DD"|null, "useOldest"?:bool, "place"?:string, "description"?:string}: makes a folder beside the photos' folder (the name sanitized, the next free one) with that Folder Properties and moves the photos into it, one undo step (undo moves them back and trashes the folder while it holds nothing else). Replies {job, pending:true, total, folder, folderId}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: has_ids,
            run: run_new_folder,
        },
    ]
}

// --- parameters ---------------------------------------------------------------------------------

/// The answer to the clash question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Collisions {
    Rename,
    Skip,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MoveParams {
    ids: Vec<PhotoId>,
    folder: FolderId,
    collisions: Option<Collisions>,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    dont_ask: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CopyParams {
    ids: Vec<PhotoId>,
    folder: FolderId,
    collisions: Option<Collisions>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NewFolderParams {
    ids: Vec<PhotoId>,
    name: String,
    date: Option<String>,
    use_oldest: Option<bool>,
    place: Option<String>,
    description: Option<String>,
}

// --- the items ----------------------------------------------------------------------------------

/// How an item's work answers a taken name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Policy {
    /// The next free name (Rename Duplicates, and a clash the library did not show).
    Rename,
    /// Leave the photo where it is (Skip Duplicates).
    Skip,
    /// The name it has, or fail (undo and redo).
    Exact,
}

/// Where a moved or copied photo goes: a folder known now, or the new folder an earlier item of
/// the same operation makes (its name known once made).
#[derive(Clone, Debug)]
enum Dest {
    Known(PathBuf),
    Made(Arc<Mutex<Option<PathBuf>>>),
}

impl Dest {
    fn path(&self) -> Result<PathBuf, String> {
        match self {
            Dest::Known(p) => Ok(p.clone()),
            Dest::Made(cell) => {
                cell.lock().unwrap_or_else(PoisonError::into_inner).clone().ok_or_else(|| "The new folder could not be made.".to_string())
            }
        }
    }
}

/// One photo's file moved or copied.
#[derive(Clone, Debug)]
struct Transfer {
    mode: Mode,
    photo: PhotoId,
    /// The copy's id (a copy), reserved when the command ran.
    copy: Option<PhotoId>,
    from: PathBuf,
    from_id: FolderId,
    name: String,
    to: Dest,
    to_id: FolderId,
    /// The name it is meant to get.
    new_name: String,
    policy: Policy,
    /// Skipped without touching the disk: the library shows the name taken (Skip Duplicates).
    skip: bool,
    /// Into a folder this operation makes.
    made_folder: bool,
    /// Back out of a folder this operation made (its undo).
    leaves_made_folder: bool,
}

/// The new folder of Move to New Folder.
#[derive(Clone, Debug)]
struct MakeFolder {
    parent: PathBuf,
    name: String,
    id: FolderId,
    info: FolderInfo,
    policy: Policy,
    /// Where the photos of the operation learn its path.
    made: Arc<Mutex<Option<PathBuf>>>,
}

#[derive(Clone, Debug)]
enum Step {
    Transfer(Transfer),
    /// Undoing a copy: the copy to the trash, while unchanged.
    Uncopy {
        path: PathBuf,
        expect: Identity,
    },
    MakeFolder(MakeFolder),
    /// Undoing a new folder: the folder to the trash, while it holds nothing but bookkeeping.
    RemoveFolder {
        path: PathBuf,
    },
}

/// One item of a move or a copy: the file it is about, where it goes (the journal's `from` and
/// `to`) and what to do.
#[derive(Clone, Debug)]
pub struct Item {
    source: PathBuf,
    target: PathBuf,
    step: Step,
}

/// What an item did.
#[derive(Clone, Debug)]
pub enum Did {
    /// Moved to `to`/`name`; `kept` when the source's volume had no trash (the move became a copy).
    Moved {
        to: PathBuf,
        name: String,
        kept: bool,
    },
    /// Copied to `to`/`name`, the copy's identity.
    Copied {
        to: PathBuf,
        name: String,
        identity: Identity,
    },
    Uncopied,
    MadeFolder {
        path: PathBuf,
        name: String,
    },
    RemovedFolder,
}

/// The driver of moves and copies.
pub struct Files;

fn fs_err(e: dalica_fs::Error) -> String {
    e.to_string()
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The names to try for `planned`, a name planned from `original`: itself, then `original`'s
/// numbered names after it.
fn names_for<'a>(original: &'a str, planned: &'a str) -> impl Iterator<Item = String> + 'a {
    std::iter::once(planned.to_string()).chain(candidates(original).skip(1).filter(move |c| c.as_str() != planned))
}

impl Files {
    /// Moves or copies `t`'s file, taking the next free name on a clash when its policy says so.
    fn transfer(item: &Item, t: &Transfer) -> Result<Ran<Did>, String> {
        let to = t.to.path()?;
        if t.skip {
            return Ok(Ran::Skipped(format!("{} already has a file named {}", file_name(&to), t.new_name)));
        }
        let identity = dalica_fs::identity(&t.from.join(&t.name)).map_err(fs_err)?.ok_or_else(|| format!("{} is no longer there", t.name))?;
        for (i, name) in names_for(&t.name, &t.new_name).enumerate() {
            let path = to.join(&name);
            if i > 0 && dalica_fs::stat(&path).map_err(fs_err)?.is_some() {
                continue;
            }
            if path != item.target {
                fileops::retarget(&path)?;
            }
            let did = match t.mode {
                Mode::Move => match transfer::move_file(&t.from, &t.name, identity, &to, &name).map_err(fs_err)? {
                    Outcome::Done(m) => {
                        Some(Did::Moved { to: to.clone(), name: name.clone(), kept: matches!(m, Moved::Copied { source: Finished::NoTrash, .. }) })
                    }
                    Outcome::Exists => None,
                },
                Mode::Copy => match transfer::copy_file(&t.from, &t.name, identity, &to, &name).map_err(fs_err)? {
                    Outcome::Done(identity) => Some(Did::Copied { to: to.clone(), name: name.clone(), identity }),
                    Outcome::Exists => None,
                },
            };
            match (did, t.policy) {
                (Some(did), _) => return Ok(Ran::Done(did)),
                (None, Policy::Rename) => {}
                (None, Policy::Skip) => return Ok(Ran::Skipped(format!("{} already has a file named {name}", file_name(&to)))),
                (None, Policy::Exact) => return Err(format!("{} already has a file named {name}", file_name(&to))),
            }
        }
        Err(format!("no free name for {} in {}", t.name, file_name(&to)))
    }

    fn make_folder(item: &Item, m: &MakeFolder) -> Result<Ran<Did>, String> {
        for (i, name) in names_for(&m.name, &m.name).enumerate() {
            let path = m.parent.join(&name);
            if i > 0 && dalica_fs::stat(&path).map_err(fs_err)?.is_some() {
                continue;
            }
            if path != item.target {
                fileops::retarget(&path)?;
            }
            match transfer::new_folder(&m.parent, &name).map_err(fs_err)? {
                Outcome::Done(()) => {
                    *m.made.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
                    return Ok(Ran::Done(Did::MadeFolder { path, name }));
                }
                Outcome::Exists if m.policy == Policy::Rename => {}
                Outcome::Exists => return Err(format!("{} already exists", path.display())),
            }
        }
        Err(format!("no free name for the folder {}", m.name))
    }

    fn uncopy(path: &Path, expect: Identity) -> Result<Ran<Did>, String> {
        match dalica_fs::identity(path).map_err(fs_err)? {
            None => Ok(Ran::Done(Did::Uncopied)),
            Some(id) if id != expect => Err(format!("The copy {} changed since it was made, so it stays.", file_name(path))),
            Some(_) => match trash::send(path).map_err(fs_err)? {
                Sent::Trashed(_) => {
                    // The copy is gone: what went wrong with its kept original is only logged.
                    if let (Some(folder), Some(name)) = (path.parent(), path.file_name()) {
                        let name = name.to_string_lossy();
                        if let Err(e) = uncopy_backup(folder, &name) {
                            log::warn!("undoing the copy {}: its kept original stays: {e}", path.display());
                        }
                    }
                    Ok(Ran::Done(Did::Uncopied))
                }
                Sent::NoTrash => Err(format!("The copy {} stays: its drive has no trash.", file_name(path))),
            },
        }
    }

    fn remove_folder(path: &Path) -> Result<Ran<Did>, String> {
        let deadline = Instant::now() + SIDECAR_PATIENCE;
        loop {
            match holds(path)? {
                Holds::Gone => return Ok(Ran::Done(Did::RemovedFolder)),
                Holds::Bookkeeping => break,
                Holds::Entries | Holds::Busy(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Holds::Entries => return Err(format!("The folder {} still holds photo entries, so it stays.", file_name(path))),
                Holds::Busy(what) | Holds::Other(what) => return Err(format!("The folder {} is not empty ({what}), so it stays.", file_name(path))),
            }
        }
        match trash::send(path).map_err(fs_err)? {
            Sent::Trashed(_) => Ok(Ran::Done(Did::RemovedFolder)),
            Sent::NoTrash => Err(format!("The folder {} stays: its drive has no trash.", file_name(path))),
        }
    }
}

/// After an unchanged copy `name` in `folder` went to the trash (its undo): the kept original the
/// copy brought (`.dalica-originals/<name>`, the backup of the record's size) goes to the trash
/// too, and its entry leaves the originals record. A record left with nothing but its version, in
/// an originals folder that holds nothing else, goes to the trash with the folder. A backup of
/// another size, a record Dalica cannot read or one that changed meanwhile stays as it is.
fn uncopy_backup(folder: &Path, name: &str) -> Result<(), String> {
    use dalica_sidecar::originals::{DIR_NAME, FILE_NAME};
    use dalica_sidecar::{Field, Limits, Originals};
    let dir = folder.join(DIR_NAME);
    let record = dir.join(FILE_NAME);
    let limits = Limits::default();
    let Some(read) = dalica_fs::read(&record, u64::try_from(limits.max_bytes).unwrap_or(u64::MAX)).map_err(fs_err)? else { return Ok(()) };
    let mut doc = Originals::parse(&read.bytes, &limits).map_err(|e| format!("{} cannot be read: {e}", record.display()))?;
    let Field::Value(entry) = doc.entry(name) else { return Ok(()) };
    let backup = dir.join(name);
    match dalica_fs::identity(&backup).map_err(fs_err)? {
        Some(id) if id.size() == entry.size => match trash::send(&backup).map_err(fs_err)? {
            Sent::Trashed(_) => {}
            Sent::NoTrash => return Err("its drive has no trash".into()),
        },
        Some(_) => return Err(format!("{} is not the one recorded", backup.display())),
        // A record without its backup: the entry still goes.
        None => {}
    }
    doc.remove(name).map_err(|e| e.to_string())?;
    let limits = dalica_fs::Limits { max_depth: 1, max_entries: 4, cancel: None };
    let only_record = || {
        dalica_fs::walk(&dir, limits, |_| false)
            .is_ok_and(|w| w.warnings.is_empty() && w.entries.len() == 1 && w.entries.iter().all(|e| e.path == record && e.is_file && !e.is_link))
    };
    if doc.print() == Originals::new().print() && only_record() {
        return match trash::send(&dir).map_err(fs_err)? {
            Sent::Trashed(_) => Ok(()),
            Sent::NoTrash => Err("its drive has no trash".into()),
        };
    }
    dalica_fs::replace(&record, &doc.print(), dalica_fs::Expect::Unchanged(read.identity)).map(|_| ()).map_err(fs_err)
}

/// Whether `folder` holds anything of photo `name` to carry: an entry in its sidecar, or a backup
/// in its originals folder. When it cannot be told, yes.
fn has_data(folder: &Path, name: &str) -> bool {
    if dalica_fs::stat(&folder.join(dalica_fs::originals::DIR_NAME).join(name)).map_or(true, |s| s.is_some()) {
        return true;
    }
    let limit = u64::try_from(dalica_sidecar::Limits::default().max_bytes).unwrap_or(u64::MAX);
    match dalica_fs::read(&folder.join(dalica_sidecar::FILE_NAME), limit) {
        Ok(None) => false,
        Ok(Some(read)) => serde_json::from_slice::<Value>(&read.bytes).map_or(true, |v| v.get("photos").and_then(|p| p.get(name)).is_some()),
        Err(_) => true,
    }
}

/// What a folder made by Move to New Folder holds.
enum Holds {
    Gone,
    /// Nothing but its sidecar (without photo entries) and an originals folder with nothing but its
    /// record.
    Bookkeeping,
    /// Its sidecar still has photo entries.
    Entries,
    /// Something passing: a temporary file of a write in progress, an entry or the sidecar that
    /// could not be read just now.
    Busy(String),
    /// Something else, named.
    Other(String),
}

fn holds(path: &Path) -> Result<Holds, String> {
    match dalica_fs::stat(path).map_err(fs_err)? {
        None => return Ok(Holds::Gone),
        Some(st) if st.is_file => return Ok(Holds::Other("a file".into())),
        Some(_) => {}
    }
    let limits = dalica_fs::Limits { max_depth: 2, max_entries: 16, cancel: None };
    let walk = dalica_fs::walk(path, limits, |_| false).map_err(fs_err)?;
    if let Some(w) = walk.warnings.first() {
        return Ok(Holds::Busy(format!("{w:?}")));
    }
    let (sidecar, originals) = (dalica_sidecar::FILE_NAME, dalica_fs::originals::DIR_NAME);
    for e in &walk.entries {
        let rel = e.path.strip_prefix(path).unwrap_or(&e.path);
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let ok = match parts.as_slice() {
            [one] if one == sidecar => e.is_file && !e.is_link,
            [one] if one == originals => e.is_dir && !e.is_link,
            [dir, one] if dir == originals && one == sidecar => e.is_file && !e.is_link,
            _ => false,
        };
        if !ok {
            let shown = parts.join("/");
            let temp = e.path.file_name().is_some_and(dalica_fs::atomic::is_temp_name);
            return Ok(if temp { Holds::Busy(shown) } else { Holds::Other(shown) });
        }
    }
    let limit = u64::try_from(dalica_sidecar::Limits::default().max_bytes).unwrap_or(u64::MAX);
    let read = match dalica_fs::read(&path.join(sidecar), limit) {
        Ok(Some(read)) => read,
        Ok(None) => return Ok(Holds::Bookkeeping),
        Err(e) => return Ok(Holds::Busy(fs_err(e))),
    };
    // An unreadable sidecar is not Dalica's bookkeeping to throw away.
    let Ok(root) = serde_json::from_slice::<Value>(&read.bytes) else { return Ok(Holds::Other(format!("an unreadable {sidecar}"))) };
    Ok(match root.get("photos").and_then(Value::as_object) {
        Some(photos) if !photos.is_empty() => Holds::Entries,
        _ => Holds::Bookkeeping,
    })
}

impl Driver for Files {
    type Item = Item;
    type Done = Did;

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.source
    }

    fn target<'a>(&self, item: &'a Item) -> Option<&'a Path> {
        Some(&item.target)
    }

    fn run(&self, item: &Item) -> Result<Ran<Did>, String> {
        match &item.step {
            Step::Transfer(t) => Files::transfer(item, t),
            Step::Uncopy { path, expect } => Files::uncopy(path, *expect),
            Step::MakeFolder(m) => Files::make_folder(item, m),
            Step::RemoveFolder { path } => Files::remove_folder(path),
        }
    }

    fn ops(&self, s: &Session, item: &Item, done: &Did) -> Result<Vec<Op>, String> {
        let lib = s.library();
        let mut ops = match (&item.step, done) {
            (Step::Transfer(t), Did::Moved { name, .. }) => vec![Op::MovePhotos { moves: vec![(t.photo, t.to_id, name.clone())] }],
            (Step::Transfer(t), Did::Copied { name, .. }) => {
                let copy = t.copy.ok_or("a copy without an id")?;
                let source = lib.photo(t.photo).ok_or_else(|| format!("photo {} is no longer in the library", t.photo))?;
                copy_ops(s, source, Photo { id: copy, folder: t.to_id, name: name.clone(), ..source.clone() })
            }
            (Step::MakeFolder(m), Did::MadeFolder { path, .. }) => {
                let mut ops = vec![Op::AddFolder { folder: Folder { id: m.id, path: path.clone() } }];
                if !m.info.is_empty() {
                    ops.push(Op::SetFolderInfo { folders: vec![(m.id, m.info.clone())] });
                }
                ops
            }
            _ => Vec::new(),
        };
        let op = match ops.len() {
            0 => return Ok(ops),
            1 => ops.pop().unwrap_or(Op::Batch { ops: Vec::new() }),
            _ => Op::Batch { ops },
        };
        Ok(vec![lib.stamped(op, s.clock().now())])
    }

    fn reverse(&self, item: &Item, done: &Did) -> Result<Item, String> {
        match (&item.step, done) {
            (Step::Transfer(_), Did::Moved { kept: true, name, .. }) => {
                Err(format!("Moving {name} became a copy (its drive has no trash), so it cannot be moved back."))
            }
            (Step::Transfer(t), Did::Moved { to, name, .. }) => Ok(Item {
                source: to.join(name),
                target: t.from.join(&t.name),
                step: Step::Transfer(Transfer {
                    mode: Mode::Move,
                    photo: t.photo,
                    copy: None,
                    from: to.clone(),
                    from_id: t.to_id,
                    name: name.clone(),
                    to: Dest::Known(t.from.clone()),
                    to_id: t.from_id,
                    new_name: t.name.clone(),
                    policy: Policy::Exact,
                    skip: false,
                    made_folder: false,
                    leaves_made_folder: t.made_folder,
                }),
            }),
            (Step::Transfer(t), Did::Copied { to, name, identity }) => {
                Ok(Item { source: to.join(name), target: t.from.join(&t.name), step: Step::Uncopy { path: to.join(name), expect: *identity } })
            }
            (Step::MakeFolder(_), Did::MadeFolder { path, .. }) => {
                Ok(Item { source: path.clone(), target: path.clone(), step: Step::RemoveFolder { path: path.clone() } })
            }
            _ => Err("This step cannot be undone.".into()),
        }
    }

    fn settled(&self, item: &Item, done: &Did) -> Item {
        match (&item.step, done) {
            (Step::Transfer(t), Did::Moved { to, name, .. } | Did::Copied { to, name, .. }) => Item {
                source: item.source.clone(),
                target: to.join(name),
                step: Step::Transfer(Transfer {
                    to: Dest::Known(to.clone()),
                    new_name: name.clone(),
                    policy: Policy::Exact,
                    skip: false,
                    ..t.clone()
                }),
            },
            (Step::MakeFolder(m), Did::MadeFolder { path, name }) => Item {
                source: path.clone(),
                target: path.clone(),
                step: Step::MakeFolder(MakeFolder { name: name.clone(), policy: Policy::Exact, ..m.clone() }),
            },
            _ => item.clone(),
        }
    }

    fn carry(&self, item: &Item, done: &Did) -> Option<Carry> {
        match (&item.step, done) {
            // Back out of a folder this operation made, which goes once empty: only what is there
            // to carry, so the carry never meets the folder gone.
            (Step::Transfer(t), Did::Moved { .. }) if t.leaves_made_folder && !has_data(&t.from, &t.name) => None,
            (Step::Transfer(t), Did::Moved { to, name, kept }) => {
                Some(Carry::one(if *kept { Mode::Copy } else { Mode::Move }, &t.from, &t.name, to, name))
            }
            (Step::Transfer(t), Did::Copied { to, name, .. }) => Some(Carry::one(Mode::Copy, &t.from, &t.name, to, name)),
            _ => None,
        }
    }
}

/// The ops that add `copy` (the source's file facts under its new id, folder and name) with
/// everything the library says about `source`: its info, star, album memberships at the source's
/// keys, saved record, search facts and digest. The copy's sidecar entry is the source's
/// duplicated, so without these the destination's next write would clear the duplicated values.
fn copy_ops(s: &Session, source: &Photo, copy: Photo) -> Vec<Op> {
    let lib = s.library();
    let (from, id) = (source.id, copy.id);
    let mut ops = vec![Op::AddPhotos { photos: vec![copy] }];
    let info = lib.info(from);
    if !info.is_empty() {
        ops.push(Op::SetPhotoInfo { photos: vec![(id, info.clone())] });
    }
    let members: Vec<_> = lib.albums_of(from).into_iter().map(|(album, key)| (album.clone(), id, key.clone())).collect();
    if !members.is_empty() {
        ops.push(Op::AddMembers { members });
    }
    if let Some(saved) = lib.saved(from) {
        ops.push(Op::SetSaved { saved: vec![(id, Some(saved.clone()))] });
    }
    let facts = lib.facts(from);
    if *facts != Facts::default() {
        ops.push(Op::SetFacts { facts: vec![(id, facts.clone())] });
    }
    if let Some(digest) = lib.digest_record(from) {
        ops.push(Op::SetDigests { digests: vec![(id, Some(digest.clone()))] });
    }
    ops
}

// --- planning -----------------------------------------------------------------------------------

/// The names (lowercased) the library holds in `folder`.
fn names_in(s: &Session, folder: FolderId) -> HashSet<String> {
    s.library().photos_in(folder).into_iter().map(|p| p.name.to_lowercase()).collect()
}

/// The names of `ids` that clash in `folder`: a photo the library holds there under the same name
/// (ignoring case; for a move the photos already there stay and do not count), or an earlier photo
/// of the batch with that name. In the order of `ids`. What the clash question names.
pub fn clashing_names(s: &Session, ids: &[PhotoId], folder: FolderId, copying: bool) -> Vec<String> {
    let lib = s.library();
    let mut taken = names_in(s, folder);
    let mut out = Vec::new();
    for id in ids {
        let Some(p) = lib.photo(*id) else { continue };
        if !copying && p.folder == folder {
            continue;
        }
        if !taken.insert(p.name.to_lowercase()) {
            out.push(p.name.clone());
        }
    }
    out
}

/// "a.jpg, b.jpg and 3 more".
fn named(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(NAMED).map(String::as_str).collect();
    match files.len().saturating_sub(NAMED) {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

/// Where the items of [`transfers`] go.
struct Destination<'a> {
    dest: Dest,
    path: &'a Path,
    id: FolderId,
    /// The names the library holds there (lowercased).
    taken: HashSet<String>,
}

/// The items moving (or copying) `ids` into `into`, with their planned names. The photos already
/// there are left out of a move.
fn transfers(s: &mut Session, mode: Mode, ids: &[PhotoId], into: Destination<'_>, collisions: Option<Collisions>) -> Result<Vec<Item>, CmdError> {
    let Destination { dest, path: to_path, id: to_id, mut taken } = into;
    let made_folder = matches!(dest, Dest::Made(_));
    let policy = match collisions {
        Some(Collisions::Skip) => Policy::Skip,
        Some(Collisions::Rename) | None => Policy::Rename,
    };
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let lib = s.library();
        let photo = lib.photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        if mode == Mode::Move && photo.folder == to_id {
            continue;
        }
        let from = lib.folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder {}", photo.folder)))?.path.clone();
        let (from_id, name) = (photo.folder, photo.name.clone());
        let clash = taken.contains(&name.to_lowercase());
        let (new_name, skip) = match (clash, policy) {
            (false, _) => (name.clone(), false),
            (true, Policy::Skip) => (name.clone(), true),
            (true, _) => (names::next_free(&name, |c| taken.contains(&c.to_lowercase())).map_err(|e| CmdError::Limit(e.to_string()))?, false),
        };
        if !skip {
            taken.insert(new_name.to_lowercase());
        }
        let copy = (mode == Mode::Copy).then(|| s.library.next_photo_id());
        items.push(Item {
            source: from.join(&name),
            target: to_path.join(&new_name),
            step: Step::Transfer(Transfer {
                mode,
                photo: *id,
                copy,
                from,
                from_id,
                name,
                to: dest.clone(),
                to_id,
                new_name,
                policy,
                skip,
                made_folder,
                leaves_made_folder: false,
            }),
        });
    }
    Ok(items)
}

/// The library folder `id`'s path, refused when unknown or offline.
fn destination(s: &Session, id: FolderId) -> Result<PathBuf, CmdError> {
    let path = s.library().folder(id).ok_or_else(|| CmdError::NotFound(format!("no folder {id}")))?.path.clone();
    if s.is_folder_offline(id) {
        return Err(CmdError::Disabled(format!("{} is offline: connect its drive first.", file_name(&path))));
    }
    Ok(path)
}

/// Refuses photos whose folder is offline.
fn sources_online(s: &Session, ids: &[PhotoId]) -> Result<(), CmdError> {
    let offline = ids.iter().filter_map(|id| s.library().photo(*id)).any(|p| s.is_folder_offline(p.folder));
    if offline {
        return Err(CmdError::Disabled("Some of these photos are on a drive that is offline: connect it first.".into()));
    }
    Ok(())
}

/// The clash question when `ids` clash in `folder` and no answer was given.
fn ask_clashes(s: &Session, ids: &[PhotoId], folder: FolderId, copying: bool, collisions: Option<Collisions>) -> Result<(), CmdError> {
    if collisions.is_some() {
        return Ok(());
    }
    let clashes = clashing_names(s, ids, folder, copying);
    if clashes.is_empty() {
        return Ok(());
    }
    Err(CmdError::Disabled(format!(
        "{CLASH_QUESTION} ({}) Send \"collisions\": \"rename\" to Rename Duplicates or \"skip\" to Skip Duplicates.",
        named(&clashes)
    )))
}

fn reply(job: JobId, total: usize, folder: &Path) -> Value {
    json!({ "job": job, "pending": true, "total": total, "folder": folder })
}

// --- the commands -------------------------------------------------------------------------------

fn run_move(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let MoveParams { ids, folder, collisions, confirm, dont_ask } = params(p)?;
    let ids = known(s, ids)?;
    let to = destination(s, folder)?;
    sources_online(s, &ids)?;
    if ids.iter().all(|id| s.library().photo(*id).is_some_and(|ph| ph.folder == folder)) {
        return Err(CmdError::Disabled(format!("These photos are already in {}.", file_name(&to))));
    }
    let organize = s.settings().organize();
    if dont_ask && organize.ask_before_moving {
        s.change_settings(|st| st.set_organize(&OrganizeSettings { ask_before_moving: false, ..organize.clone() }));
    }
    if organize.ask_before_moving && !confirm && !dont_ask {
        return Err(CmdError::Disabled(format!("{MOVE_QUESTION} Send \"confirm\": true to move (and \"dontAsk\": true to stop asking).")));
    }
    ask_clashes(s, &ids, folder, false, collisions)?;
    let into = Destination { dest: Dest::Known(to.clone()), path: &to, id: folder, taken: names_in(s, folder) };
    let items = transfers(s, Mode::Move, &ids, into, collisions)?;
    let total = items.len();
    let job = s.start_file_op(FileOp::new(Files, FileOpKind::Move, MOVE_LABEL, items).destination(to.clone()));
    Ok(reply(job, total, &to))
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let CopyParams { ids, folder, collisions } = params(p)?;
    let ids = known(s, ids)?;
    let to = destination(s, folder)?;
    sources_online(s, &ids)?;
    ask_clashes(s, &ids, folder, true, collisions)?;
    let into = Destination { dest: Dest::Known(to.clone()), path: &to, id: folder, taken: names_in(s, folder) };
    let items = transfers(s, Mode::Copy, &ids, into, collisions)?;
    let total = items.len();
    let job = s.start_file_op(FileOp::new(Files, FileOpKind::Copy, COPY_LABEL, items).destination(to.clone()));
    Ok(reply(job, total, &to))
}

/// The new folder's Folder Properties from the parameters: the date only when given and the
/// oldest photo's is not asked for.
fn new_folder_info(
    date: Option<String>,
    use_oldest: Option<bool>,
    place: Option<String>,
    description: Option<String>,
) -> Result<FolderInfo, CmdError> {
    let date = date.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    let date = match (use_oldest.unwrap_or(date.is_none()), date) {
        (true, _) | (false, None) => None,
        (false, Some(d)) => Some(d.parse::<Date>().map_err(|e| CmdError::BadParams(format!("date: {e}")))?),
    };
    Ok(FolderInfo {
        description: description.as_deref().map(clean_description).unwrap_or_default(),
        date,
        place: place.as_deref().map(clean_place).unwrap_or_default(),
        hidden: false,
    })
}

fn run_new_folder(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let NewFolderParams { ids, name, date, use_oldest, place, description } = params(p)?;
    let info = new_folder_info(date, use_oldest, place, description)?;
    let ids = known(s, ids)?;
    sources_online(s, &ids)?;
    let mut folders = ids.iter().filter_map(|id| s.library().photo(*id).map(|p| p.folder));
    let Some(source) = folders.next() else { return Err(CmdError::Disabled("Select some photos first.".into())) };
    if folders.any(|f| f != source) {
        return Err(CmdError::Disabled("Select photos of one folder.".into()));
    }
    let source_path = destination(s, source)?;
    let parent = source_path.parent().map(Path::to_path_buf).ok_or_else(|| CmdError::Disabled("This folder has no folder around it.".into()))?;
    let wanted = names::sanitize(&name, NEW_FOLDER);
    let siblings: HashSet<String> = s
        .library()
        .folders()
        .filter(|f| f.path.parent() == Some(parent.as_path()))
        .filter_map(|f| f.path.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .collect();
    let folder_name = names::next_free(&wanted, |c| siblings.contains(&c.to_lowercase())).map_err(|e| CmdError::Limit(e.to_string()))?;
    let path = parent.join(&folder_name);
    if !s.folder_rules().indexes(&path) {
        return Err(CmdError::Disabled(format!(
            "{folder_name} would be outside the folders Dalica watches: add {} in the Folder Manager first.",
            parent.display()
        )));
    }
    let id = s.library.next_folder_id();
    let made = Arc::new(Mutex::new(None));
    let make = Item {
        source: path.clone(),
        target: path.clone(),
        step: Step::MakeFolder(MakeFolder { parent, name: folder_name, id, info, policy: Policy::Rename, made: Arc::clone(&made) }),
    };
    let into = Destination { dest: Dest::Made(made), path: &path, id, taken: HashSet::new() };
    let mut items = vec![make];
    items.extend(transfers(s, Mode::Move, &ids, into, Some(Collisions::Rename))?);
    let total = items.len().saturating_sub(1);
    let job = s.start_file_op(FileOp::new(Files, FileOpKind::Move, NEW_FOLDER_LABEL, items).destination(path.clone()));
    let mut r = reply(job, total, &path);
    r["folderId"] = json!(id);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clash_question_names_five_then_counts() {
        let files: Vec<String> = (1..=7).map(|i| format!("IMG_{i}.jpg")).collect();
        assert_eq!(named(&files[..2]), "IMG_1.jpg, IMG_2.jpg");
        assert_eq!(named(&files), "IMG_1.jpg, IMG_2.jpg, IMG_3.jpg, IMG_4.jpg, IMG_5.jpg and 2 more");
    }

    #[test]
    fn names_to_try_start_with_the_planned_one() {
        let names: Vec<String> = names_for("a.jpg", "a-002.jpg").take(4).collect();
        assert_eq!(names, ["a-002.jpg", "a-001.jpg", "a-003.jpg", "a-004.jpg"]);
        let names: Vec<String> = names_for("a.jpg", "a.jpg").take(3).collect();
        assert_eq!(names, ["a.jpg", "a-001.jpg", "a-002.jpg"]);
    }

    #[test]
    fn a_made_folder_is_bookkeeping_only_with_its_sidecar_and_waits_out_a_write_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Trams");
        assert!(matches!(holds(&folder), Ok(Holds::Gone)));
        std::fs::create_dir(&folder).unwrap();
        assert!(matches!(holds(&folder), Ok(Holds::Bookkeeping)));
        std::fs::write(folder.join(".dalica.json"), br#"{"folder": {"place": "Lisbon"}, "photos": {"a.jpg": {"star": true}}}"#).unwrap();
        assert!(matches!(holds(&folder), Ok(Holds::Entries)));
        std::fs::write(folder.join(".dalica.json"), br#"{"folder": {"place": "Lisbon"}, "photos": {}}"#).unwrap();
        std::fs::create_dir(folder.join(".dalica-originals")).unwrap();
        std::fs::write(folder.join(".dalica-originals").join(".dalica.json"), b"{}").unwrap();
        assert!(matches!(holds(&folder), Ok(Holds::Bookkeeping)));
        // A sidecar write in progress is passing; anything else stays.
        let temp = folder.join("..dalica.json.dalica-tmp-0123456789abcdef");
        std::fs::write(&temp, b"{").unwrap();
        assert!(matches!(holds(&folder), Ok(Holds::Busy(_))));
        std::fs::remove_file(&temp).unwrap();
        std::fs::write(folder.join("notes.txt"), b"mine").unwrap();
        assert!(matches!(holds(&folder), Ok(Holds::Other(what)) if what == "notes.txt"));
    }

    #[test]
    fn new_folder_info_takes_the_date_unless_the_oldest_is_used() {
        let day = "2026-06-04".parse::<Date>().unwrap();
        assert_eq!(new_folder_info(Some("2026-06-04".into()), None, None, None).unwrap().date, Some(day));
        assert_eq!(new_folder_info(Some("2026-06-04".into()), Some(true), None, None).unwrap().date, None);
        assert_eq!(new_folder_info(None, Some(false), None, None).unwrap().date, None);
        assert_eq!(new_folder_info(Some(" ".into()), Some(false), None, None).unwrap().date, None);
        assert!(matches!(new_folder_info(Some("June".into()), Some(false), None, None), Err(CmdError::BadParams(_))));
        let info = new_folder_info(None, None, Some("  Lisbon ".into()), Some("Trams".into())).unwrap();
        assert_eq!((info.place.as_str(), info.description.as_str()), ("Lisbon", "Trams"));
    }
}
