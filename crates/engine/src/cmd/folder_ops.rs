//! Moving folders (`organizing.md` §9, `data-model.md` §8): `folder.move` into a library folder,
//! as a file operation (`fileops.rs`) and one undo step.
//!
//! - **Where.** Folders move only within the library: `to` is a library folder (its id, or its
//!   absolute path), or a folder at or under a watched folder (`path` is taken for `to`, as the
//!   folder picker answers). Refused before anything starts: an unknown or offline folder, a volume
//!   root or the home folder, a move into the folder itself or one of its subfolders, a move to
//!   where it already is, and a place where a library folder that is not moving already lies under
//!   the new path.
//! - **A plain move** (no folder of that name in `to` in the library): the folder with everything
//!   in it goes through `dalica-fs` (`transfer::move_folder`: one rename that never replaces on one
//!   volume; across volumes the tree is copied, every file verified byte for byte, and only then
//!   the source tree goes to the OS trash). The library folder and every library folder under it
//!   follow with their ids (`Op::MoveFolder`); the sidecars travel inside the folder, so nothing is
//!   carried. A folder of that name found on disk at run time (not in the library) fails the move
//!   with nothing changed. Across volumes onto a drive without a trash the source stays beside its
//!   copy: the move became a copy, which cannot be undone (the library follows the copy, and the
//!   watcher finds the source again).
//! - **A merge** (`to` holds a library folder of the same name): the folder's photos move into it
//!   one by one (`transfer::move_file`), each keeping its id and data (`Op::MovePhotos`) and its
//!   sidecar entry and originals backup carried (`Driver::carry`). Names the destination's photos
//!   already use are a clash: without `collisions` the command asks first ([`clash_question`]);
//!   `"rename"` gives the next free `-001` name, `"skip"` leaves those photos where they are. A
//!   name taken only on disk (a file Dalica does not list) is renamed the same way unless `"skip"`.
//!   The emptied source is sent to the OS trash only when nothing else is left in it (its own
//!   `.dalica.json` and an empty `.dalica-originals` aside): non-picture files and subfolders stay
//!   in it, and so does the folder. The entries leave its sidecar on the I/O worker after each
//!   photo's library change (`fileops.rs`), so the trash step waits for them with two steps
//!   before it: a settle step, which the owner thread applies after every photo before it (their
//!   carries sent to the I/O worker); and a wait step, which waits for that ([`SETTLE_WAIT`]; past
//!   it the folder stays) and whose journal mark, like every item's, is a round trip through the
//!   I/O worker, so the carries are written before the trash step starts.
//! - **Undo** moves everything back (a merged photo to its old name, the emptied source brought
//!   back from the trash, or made again where the trash cannot give it back); redo moves it again
//!   to the same names.
//! - **Watched-folder entries** at and under a moved folder follow it (`Session::rebase_folders`
//!   from `Driver::followed`, on do, undo and redo), keeping their rules; a merge leaves them.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{FolderId, PhotoId, names};
use dalica_fs::transfer::{self, Finished, FolderMoved, Moved, Outcome};
use dalica_fs::trash::{self, Restore, Sent, Trashed};
use dalica_fs::walk::{self, Limits};
use dalica_library::Op;
use dalica_store::carry::{Carry, Mode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::delete::{Flushed, RESTORES, display_name};
use crate::error::CmdError;
use crate::fileops::{Driver, FileOp, FileOpKind, Ran};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// The undo label.
pub const LABEL: &str = "Move Folder";
/// An offline folder.
pub const OFFLINE: &str = "The folder is offline: connect its drive to move it.";
/// A volume root or the home folder.
pub const NOT_A_DRIVE: &str = "Dalica does not move a whole drive or your home folder.";
/// `to` is outside the library.
pub const NOT_IN_LIBRARY: &str = "Folders move only within the library: choose a folder Dalica watches.";
/// `to` is the folder itself or inside it.
pub const INTO_ITSELF: &str = "A folder cannot move into itself or into one of its subfolders.";
/// `to` is where the folder already is.
pub const ALREADY_THERE: &str = "The folder is already there.";
/// The most names a clash question lists.
const LISTED: usize = 5;
/// The most entries looked at to tell whether a merged folder is empty.
const EMPTY_LOOK: usize = 64;
/// How long the trash step of a merge waits for the photos' entries to leave the folder's sidecar
/// (the owner thread applying the photos, then the I/O worker carrying their entries).
pub const SETTLE_WAIT: Duration = Duration::from_secs(60);

/// The clash question of a merge (the collision dialog's text), naming the first clashing files.
pub fn clash_question(names: &[String]) -> String {
    let mut listed: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
    if names.len() > LISTED {
        listed.push("…");
    }
    format!("The folder already has files with the same names ({}). Rename Duplicates or Skip Duplicates?", listed.join(", "))
}

/// The folder `name` is already in `to` on disk, not in the library.
pub fn folder_taken(name: &str) -> String {
    format!("A folder named \"{name}\" is already there; nothing was moved.")
}

/// Why a move that became a copy cannot be undone.
pub fn copied(name: &str) -> String {
    format!("\"{name}\" was copied, not moved, because its drive has no trash: the original is still in its old place.")
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "folder.move",
        label: "Move Folder…",
        menu: "Folder/Move…",
        keys: &[],
        params: r#"{"folder":FolderId, "to":FolderId|string (absolute; also "path"), "collisions"?:"rename"|"skip"}: moves the folder with everything in it into "to" (a library folder, or a folder at or under a watched folder) as a file job and one undo step; ids, data and subfolders go with it. Onto a library folder of the same name it merges photo by photo: names already used there need "collisions" ("rename" gives -001, "skip" leaves those photos), and the emptied folder goes to the OS trash only when nothing else is left in it. Replies {job, pending:true, total, merge}"#,
        effect: Effect::Files,
        history: History::Library,
        enabled: always,
        run: run_move,
    }]
}

// --- the driver ---------------------------------------------------------------------------------

/// What happens at a name already taken on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Collisions {
    /// The next free `-001` name.
    Rename,
    /// The photo stays where it is.
    Skip,
}

/// What an item does.
#[derive(Clone, Debug)]
pub enum Step {
    /// Moves the folder `name` in `parent` into `to` under the same name.
    Folder { id: FolderId, parent: PathBuf, name: String, to: PathBuf },
    /// Moves photo `photo` (`name` in `from`) into `to` as `new_name`. `clash` is what a name taken
    /// on disk does (`None`: the item fails; undo and redo); `taken` holds the names (folded case)
    /// a renamed photo must not take; `skip` leaves it alone at once (a clash the library knows).
    Photo {
        photo: PhotoId,
        from: PathBuf,
        name: String,
        to: PathBuf,
        new_name: String,
        clash: Option<Collisions>,
        taken: Arc<BTreeSet<String>>,
        skip: bool,
    },
    /// Done at once; applied by the owner thread only after every photo before it (their carries
    /// sent), which the wait step after it waits for. `path` and `into` as below.
    Settle { path: PathBuf, into: PathBuf },
    /// Waits until the settle step before it was applied; its journal mark then follows the
    /// carries through the I/O worker. `path` and `into` as below.
    Wait { path: PathBuf, into: PathBuf },
    /// Sends the merged folder `id` at `path` to the trash when nothing else is left in it. `into`
    /// is the folder its photos went to: the journal's `to` for this item (see [`Item::new`]).
    TrashEmptied { id: FolderId, path: PathBuf, into: PathBuf },
    /// Brings the emptied folder back (undo): from the trash where it can, else made again.
    BringBack { path: PathBuf, into: PathBuf, trashed: Trashed },
}

/// One item: its step, with the file or folder it is about and where it goes.
#[derive(Clone, Debug)]
pub struct Item {
    pub step: Step,
    src: PathBuf,
    dst: Option<PathBuf>,
}

impl Item {
    /// The item of `step`. A merge is journaled as a move of photos (`data-model.md` §8: a repair
    /// carries their entries), and every item of a move names where it goes: the step that trashes
    /// the emptied folder names the folder its photos went to. A repair finds that folder no copy
    /// of it while it is still there (back: nothing to do), or finds it gone and the destination
    /// there (forward: no entry of that name to carry).
    fn new(step: Step) -> Item {
        let (src, dst) = match &step {
            Step::Folder { parent, name, to, .. } => (parent.join(name), Some(to.join(name))),
            Step::Photo { from, name, to, new_name, .. } => (from.join(name), Some(to.join(new_name))),
            Step::Settle { path, into } | Step::Wait { path, into } | Step::TrashEmptied { path, into, .. } | Step::BringBack { path, into, .. } => {
                (path.clone(), Some(into.clone()))
            }
        };
        Item { step, src, dst }
    }
}

/// What an item did.
#[derive(Clone, Debug)]
pub enum Did {
    /// The folder moved; `copy`: across volumes onto a drive without a trash, the source stayed.
    Folder {
        copy: bool,
    },
    /// The photo moved under `name`; `copy` as above.
    Photo {
        name: String,
        copy: bool,
    },
    /// Settled: raised once the owner thread applied it.
    Settle(Arc<Flag>),
    Waited,
    Trashed(Trashed),
    BroughtBack,
}

/// A flag one thread raises and another waits for.
#[derive(Debug, Default)]
pub struct Flag {
    raised: Mutex<bool>,
    cv: Condvar,
}

impl Flag {
    fn raise(&self) {
        *self.raised.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }

    /// Whether it was raised within `wait`.
    fn wait(&self, wait: Duration) -> bool {
        let start = Instant::now();
        let mut raised = self.raised.lock().unwrap_or_else(PoisonError::into_inner);
        while !*raised {
            let Some(left) = wait.checked_sub(start.elapsed()) else { return false };
            raised = self.cv.wait_timeout(raised, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        true
    }
}

/// The folder-move driver: the sidecar writes pending when the move started, which its first item
/// waits for ([`Flushed`]), and a merge's settling between its steps (the flag of the last settle
/// step run, and whether the wait step after it saw it raised).
#[derive(Default)]
pub struct MoveFolder {
    flushed: Flushed,
    settle: Mutex<Option<Arc<Flag>>>,
    ready: Mutex<bool>,
}

impl Driver for MoveFolder {
    type Item = Item;
    type Done = Did;

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.src
    }

    fn target<'a>(&self, item: &'a Item) -> Option<&'a Path> {
        item.dst.as_deref()
    }

    fn run(&self, item: &Item) -> Result<Ran<Did>, String> {
        self.flushed.wait();
        match &item.step {
            Step::Folder { parent, name, to, .. } => match transfer::move_folder(parent, name, to, name).map_err(|e| e.to_string())? {
                Outcome::Done(FolderMoved::Renamed) => Ok(Ran::Done(Did::Folder { copy: false })),
                Outcome::Done(FolderMoved::Copied { source, .. }) => Ok(Ran::Done(Did::Folder { copy: source == Finished::NoTrash })),
                Outcome::Exists => Err(folder_taken(name)),
            },
            Step::Photo { skip: true, .. } => Ok(Ran::Skipped(skipped())),
            Step::Photo { from, name, to, new_name, clash, taken, .. } => move_photo(from, name, to, new_name, *clash, taken),
            Step::Settle { .. } => {
                let flag = Arc::new(Flag::default());
                *self.settle.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&flag));
                Ok(Ran::Done(Did::Settle(flag)))
            }
            Step::Wait { .. } => {
                let flag = self.settle.lock().unwrap_or_else(PoisonError::into_inner).take();
                let raised = flag.is_some_and(|f| f.wait(SETTLE_WAIT));
                *self.ready.lock().unwrap_or_else(PoisonError::into_inner) = raised;
                Ok(Ran::Done(Did::Waited))
            }
            Step::TrashEmptied { path, .. } => {
                let ready = std::mem::take(&mut *self.ready.lock().unwrap_or_else(PoisonError::into_inner));
                if !ready {
                    return Ok(Ran::Skipped(format!("\"{}\" stays: its photos' entries had not left it in time.", display_name(path))));
                }
                if !emptied(path)? {
                    return Ok(Ran::Skipped(format!("\"{}\" stays: it still holds other files or folders.", display_name(path))));
                }
                match trash::send(path).map_err(|e| e.to_string())? {
                    Sent::Trashed(t) => Ok(Ran::Done(Did::Trashed(t))),
                    Sent::NoTrash => Ok(Ran::Skipped(format!("\"{}\" stays, empty: its drive has no trash.", display_name(path)))),
                }
            }
            Step::BringBack { path, trashed, .. } => {
                if RESTORES {
                    match trash::restore(std::slice::from_ref(trashed)).map_err(|e| e.to_string())?.first() {
                        Some(Restore::Restored | Restore::Exists) => return Ok(Ran::Done(Did::BroughtBack)),
                        Some(Restore::NotInTrash | Restore::Unsupported) | None => {}
                    }
                }
                // It was empty (Dalica's own files aside): made again, the library writes its sidecar.
                let parent = path.parent().ok_or("the folder has no parent")?;
                let name = path.file_name().and_then(|n| n.to_str()).ok_or("the folder has no name")?;
                transfer::new_folder(parent, name).map_err(|e| e.to_string())?;
                Ok(Ran::Done(Did::BroughtBack))
            }
        }
    }

    fn ops(&self, s: &Session, item: &Item, done: &Did) -> Result<Vec<Op>, String> {
        let lib = s.library();
        let op = match (&item.step, done) {
            (Step::Folder { id, to, name, .. }, _) => {
                lib.folder(*id).filter(|f| f.path == item.src).ok_or("the folder had moved in the library meanwhile")?;
                Op::MoveFolder { id: *id, path: to.join(name) }
            }
            (Step::Photo { photo, from, to, .. }, Did::Photo { name, .. }) => {
                let p = lib.photo(*photo).ok_or("the photo had left the library meanwhile")?;
                if lib.folder(p.folder).map(|f| f.path.as_path()) != Some(from.as_path()) {
                    return Err("the photo had moved in the library meanwhile".into());
                }
                let dest = lib.folder_by_path(to).ok_or("the destination folder left the library meanwhile")?;
                Op::MovePhotos { moves: vec![(*photo, dest, name.clone())] }
            }
            (Step::Settle { .. }, Did::Settle(flag)) => {
                flag.raise();
                return Ok(Vec::new());
            }
            (Step::TrashEmptied { id, .. }, _) => match lib.folder(*id) {
                Some(_) => Op::RemoveFolder { id: *id },
                None => return Ok(Vec::new()),
            },
            _ => return Ok(Vec::new()),
        };
        Ok(vec![lib.stamped(op, s.clock().now())])
    }

    fn reverse(&self, item: &Item, done: &Did) -> Result<Item, String> {
        let step = match (&item.step, done) {
            (Step::Folder { name, .. }, Did::Folder { copy: true }) | (Step::Photo { name, .. }, Did::Photo { copy: true, .. }) => {
                return Err(copied(name));
            }
            (Step::Folder { id, parent, name, to }, _) => Step::Folder { id: *id, parent: to.clone(), name: name.clone(), to: parent.clone() },
            (Step::Photo { photo, from, name, to, .. }, Did::Photo { name: now, .. }) => Step::Photo {
                photo: *photo,
                from: to.clone(),
                name: now.clone(),
                to: from.clone(),
                new_name: name.clone(),
                clash: None,
                taken: Arc::default(),
                skip: false,
            },
            (Step::TrashEmptied { path, into, .. }, Did::Trashed(t)) => {
                Step::BringBack { path: path.clone(), into: into.clone(), trashed: t.clone() }
            }
            (Step::Settle { .. } | Step::Wait { .. }, _) => item.step.clone(),
            _ => return Err("this item has no way back".into()),
        };
        Ok(Item::new(step))
    }

    fn settled(&self, item: &Item, done: &Did) -> Item {
        match (&item.step, done) {
            // Redo takes the name the clash gave, exactly.
            (Step::Photo { photo, from, name, to, .. }, Did::Photo { name: now, .. }) => Item::new(Step::Photo {
                photo: *photo,
                from: from.clone(),
                name: name.clone(),
                to: to.clone(),
                new_name: now.clone(),
                clash: None,
                taken: Arc::default(),
                skip: false,
            }),
            // Redo applies no ops: the settle step is raised here (the owner thread, in order).
            (Step::Settle { .. }, Did::Settle(flag)) => {
                flag.raise();
                item.clone()
            }
            _ => item.clone(),
        }
    }

    fn carry(&self, item: &Item, done: &Did) -> Option<Carry> {
        match (&item.step, done) {
            (Step::Photo { from, name, to, .. }, Did::Photo { name: now, copy }) => {
                Some(Carry::one(if *copy { Mode::Copy } else { Mode::Move }, from, name, to, now))
            }
            _ => None,
        }
    }

    /// A folder that moved takes its watched-folder entries along (`folders.json`): on do, undo
    /// (the reverse item: back again) and redo. A merge leaves them where they are.
    fn followed(&self, s: &mut Session, item: &Item, _done: &Did) {
        if let (Step::Folder { .. }, Some(dst)) = (&item.step, &item.dst) {
            s.rebase_folders(&item.src, dst);
        }
    }
}

fn skipped() -> String {
    "A file with that name is already there (Skip Duplicates).".into()
}

/// Moves one photo, renaming or skipping at a name taken on disk as `clash` says.
fn move_photo(from: &Path, name: &str, to: &Path, new_name: &str, clash: Option<Collisions>, taken: &BTreeSet<String>) -> Result<Ran<Did>, String> {
    let identity = dalica_fs::identity(&from.join(name)).map_err(|e| e.to_string())?.ok_or_else(|| format!("\"{name}\" is no longer there"))?;
    let mut tried: Vec<String> = Vec::new();
    let mut candidates = names::candidates(new_name);
    let mut candidate = new_name.to_string();
    loop {
        match transfer::move_file(from, name, identity, to, &candidate).map_err(|e| e.to_string())? {
            Outcome::Done(m) => {
                let copy = matches!(m, Moved::Copied { source: Finished::NoTrash, .. });
                return Ok(Ran::Done(Did::Photo { name: candidate, copy }));
            }
            Outcome::Exists => match clash {
                None => return Err(format!("\"{candidate}\" is already in \"{}\"; nothing was moved.", display_name(to))),
                Some(Collisions::Skip) => return Ok(Ran::Skipped(skipped())),
                Some(Collisions::Rename) => {
                    tried.push(fold(&candidate));
                    candidate = candidates
                        .find(|c| {
                            let folded = fold(c);
                            !taken.contains(&folded) && !tried.contains(&folded)
                        })
                        .ok_or_else(|| format!("no free name for \"{new_name}\""))?;
                }
            },
        }
    }
}

/// Whether nothing but Dalica's own files is left in `folder`: its `.dalica.json`, and a
/// `.dalica-originals` holding at most its record.
fn emptied(folder: &Path) -> Result<bool, String> {
    let limits = Limits { max_depth: 2, max_entries: EMPTY_LOOK, cancel: None };
    let found = walk::walk(folder, limits, |_| false).map_err(|e| e.to_string())?;
    if !found.warnings.is_empty() {
        return Ok(false);
    }
    Ok(found.entries.iter().all(|e| {
        let name = e.name();
        match e.depth {
            1 => (name == dalica_sidecar::FILE_NAME && e.is_file && !e.is_link) || (name == ORIGINALS && e.is_dir && !e.is_link),
            _ => {
                e.path.parent().and_then(Path::file_name).is_some_and(|p| p == ORIGINALS)
                    && name == dalica_sidecar::FILE_NAME
                    && e.is_file
                    && !e.is_link
            }
        }
    }))
}

/// The originals folder's name.
const ORIGINALS: &str = dalica_fs::originals::DIR_NAME;

/// A name as clashes compare it: ignoring case, as some volumes do.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

// --- the command --------------------------------------------------------------------------------

/// Where the folder goes: a library folder's id, or an absolute path.
#[derive(Deserialize)]
#[serde(untagged)]
enum To {
    Id(FolderId),
    Path(PathBuf),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveParams {
    folder: FolderId,
    #[serde(alias = "path")]
    to: To,
    #[serde(default)]
    collisions: Option<Collisions>,
}

/// `path` written out plainly: absolute, without `.` or `..`.
fn plain_absolute(path: &Path) -> bool {
    path.is_absolute() && path.components().all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

fn run_move(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let MoveParams { folder, to, collisions } = params(p)?;
    let lib = s.library();
    let path = lib.folder(folder).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {folder}")))?;
    let to = match to {
        To::Id(id) => {
            if s.is_folder_offline(id) {
                return Err(CmdError::Disabled(OFFLINE.into()));
            }
            lib.folder(id).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {id}")))?
        }
        To::Path(p) => {
            if !plain_absolute(&p) {
                return Err(CmdError::BadParams(format!("`to` must be an absolute path without `.` or `..`, not {}", p.display())));
            }
            if lib.folder_by_path(&p).is_none() && !s.roots().iter().any(|r| p.starts_with(r)) {
                return Err(CmdError::Disabled(NOT_IN_LIBRARY.into()));
            }
            p
        }
    };
    if s.is_folder_offline(folder) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Err(CmdError::Disabled(NOT_A_DRIVE.into()));
    };
    if s.is_drive_or_home(&path) {
        return Err(CmdError::Disabled(NOT_A_DRIVE.into()));
    }
    if to.starts_with(&path) {
        return Err(CmdError::Disabled(INTO_ITSELF.into()));
    }
    if to == parent {
        return Err(CmdError::Disabled(ALREADY_THERE.into()));
    }
    let new = to.join(name);
    let (items, merge) = match lib.folder_by_path(&new) {
        Some(dest) => (merge_items(s, folder, &path, dest, &new, collisions)?, true),
        None => {
            if lib.folders().any(|f| f.path.starts_with(&new) && !f.path.starts_with(&path)) {
                return Err(CmdError::Disabled(folder_taken(name)));
            }
            let step = Step::Folder { id: folder, parent: parent.to_path_buf(), name: name.to_string(), to: to.clone() };
            (vec![Item::new(step)], false)
        }
    };
    let total = items.len();
    // A merge moves photos (their entries travel, and a repair carries them); a plain move moves a
    // folder, its sidecar inside it.
    let (kind, destination) = if merge { (FileOpKind::Move, new) } else { (FileOpKind::MoveFolder, to) };
    let driver = MoveFolder { flushed: Flushed::ask(s), ..MoveFolder::default() };
    let job = s.start_file_op(FileOp::new(driver, kind, LABEL, items).destination(destination));
    Ok(json!({ "job": job, "pending": true, "total": total, "merge": merge }))
}

/// The items of merging folder `id` (at `from`) into library folder `dest` (at `to`): its photos one
/// by one in name order, then its trash step when no library folder lies under it.
fn merge_items(s: &Session, id: FolderId, from: &Path, dest: FolderId, to: &Path, collisions: Option<Collisions>) -> Result<Vec<Item>, CmdError> {
    let lib = s.library();
    if s.is_folder_offline(dest) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    let mut photos: Vec<(String, PhotoId)> = lib.photos_in(id).into_iter().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    let mut taken: BTreeSet<String> = lib.photos_in(dest).iter().map(|p| fold(&p.name)).collect();
    let clashes: Vec<String> = photos.iter().filter(|(n, _)| taken.contains(&fold(n))).map(|(n, _)| n.clone()).collect();
    if !clashes.is_empty() && collisions.is_none() {
        return Err(CmdError::Disabled(format!("{} Send \"collisions\": \"rename\" or \"skip\" to go ahead.", clash_question(&clashes))));
    }
    // A clash only the disk knows is renamed unless the caller said to skip.
    let clash = Some(collisions.unwrap_or(Collisions::Rename));
    let mut planned = Vec::with_capacity(photos.len());
    for (name, photo) in photos {
        let folded = fold(&name);
        let (new_name, skip) = if taken.contains(&folded) {
            match collisions {
                Some(Collisions::Skip) | None => (name.clone(), true),
                Some(Collisions::Rename) => {
                    let free = names::next_free(&name, |c| taken.contains(&fold(c))).map_err(|e| CmdError::Limit(e.to_string()))?;
                    (free, false)
                }
            }
        } else {
            (name.clone(), false)
        };
        if !skip {
            taken.insert(fold(&new_name));
        }
        planned.push((photo, name, new_name, skip));
    }
    let taken = Arc::new(taken);
    let mut items: Vec<Item> = planned
        .into_iter()
        .map(|(photo, name, new_name, skip)| {
            Item::new(Step::Photo { photo, from: from.to_path_buf(), name, to: to.to_path_buf(), new_name, clash, taken: Arc::clone(&taken), skip })
        })
        .collect();
    if !lib.folders().any(|f| f.id != id && f.path.starts_with(from)) {
        items.push(Item::new(Step::Settle { path: from.to_path_buf(), into: to.to_path_buf() }));
        items.push(Item::new(Step::Wait { path: from.to_path_buf(), into: to.to_path_buf() }));
        items.push(Item::new(Step::TrashEmptied { id, path: from.to_path_buf(), into: to.to_path_buf() }));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clash_question_lists_the_first_names() {
        let names: Vec<String> = (1..=7).map(|n| format!("IMG_{n}.jpg")).collect();
        let q = clash_question(&names);
        assert!(q.starts_with("The folder already has files with the same names (IMG_1.jpg, IMG_2.jpg"), "{q}");
        assert!(q.contains("IMG_5.jpg, …)") && !q.contains("IMG_6.jpg"), "{q}");
    }

    #[test]
    fn only_plain_absolute_paths_are_places() {
        let abs = std::env::temp_dir().join("x");
        assert!(plain_absolute(&abs));
        assert!(!plain_absolute(Path::new("relative")));
        assert!(!plain_absolute(&abs.join("..").join("y")));
    }
}
