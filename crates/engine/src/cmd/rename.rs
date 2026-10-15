//! Renaming (`organizing.md` §9, `data-model.md` §8): `photo.rename` (one photo, or a batch named
//! `-001`, `-002` … in light-table order) and `folder.rename`, each a file operation
//! (`fileops.rs`) that is one undo step.
//!
//! The collision, case-change and rollback rules are adapted from LightCraft
//! `crates/engine/src/rename.rs` @ b7091805 (MIT OR Apache-2.0): a name another file holds, or that
//! another photo of the batch takes, gets the next number, and a file the batch moves away still
//! counts as taken (so no rename ever waits on another, and any order is safe); whether a taken
//! name is the photo itself, spelled differently on a volume that ignores case, is decided by file
//! identity, never by comparing names; the files are renamed before the library changes, and if one
//! rename fails those done are moved back and nothing changes. Here each rename is `dalica-fs`'s
//! `transfer::move_file` (one rename that never replaces a file; a case-only change through a
//! temporary name) and the names come from `core::names`.
//!
//! - **`photo.rename {ids, name, addDate?, addSize?}`.** The typed name goes through
//!   `names::sanitize` (the photo's own name is the fallback); "Add the date taken" adds the date
//!   the photo sorts by (`2026-06-04`), "Add the image size" its pixel size as displayed
//!   (`4000x3000`), each after a `-`, then the photo's own extension: `Lisbon-2026-06-04.jpg`
//!   ([`compose`]). One photo takes that name (`Lisbon-001.jpg` when it is taken); several get
//!   `-001`, `-002` … in light-table order (folders newest first, each folder's photos in its own
//!   sort), a taken number moving that photo and those after it to the next free ones.
//! - **How a batch runs.** The command (owner thread, no file access) orders the photos, works out
//!   the names from what the library knows (the journal's targets) and starts a file operation
//!   with one item per photo. On the job's thread the first item decides the names again with the
//!   disk (`plan_on_disk`: every source still there, a taken target that is not the photo itself
//!   moves on), then the first item of each folder renames all of that folder's photos, all or
//!   nothing (`rename_all`), and brings their library ops and sidecar entries (`Carry::rename`);
//!   the folder's other items report how their photo fared. Undo and redo run the renames that
//!   happened, the same way, as the step's one job. A batch over several folders is all or nothing
//!   per folder (each folder's renames are one item's work); the names are checked for every
//!   folder before the first file moves.
//! - **`folder.rename {folder, name}`.** The directory renamed in its parent
//!   (`transfer::move_folder`, never replacing anything; a case-only change in place), the library
//!   folder and those under it following with their ids (`Op::MoveFolder`), and the watched-folder
//!   entries at and under it too (`Driver::followed`: `Session::rebase_folders`, `folders.json`);
//!   the sidecar is inside the folder, so nothing is carried.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::names::{self, MAX_NAME_BYTES, MAX_NUMBER};
use dalica_core::{Date, DateTime, FolderId, PhotoId};
use dalica_fs::Identity;
use dalica_fs::transfer::{self, Outcome};
use dalica_library::{Library, Op, Photo};
use dalica_store::carry::Carry;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::fileops::{Driver, FileOp, FileOpKind, Ran};
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

/// The undo labels: "Undo Rename", "Undo Rename Folder".
pub const LABEL: &str = "Rename";
pub const FOLDER_LABEL: &str = "Rename Folder";
/// Why a photo of an offline folder cannot be renamed.
pub const OFFLINE: &str = "The folder is offline: connect its drive to rename its pictures.";
/// Why an offline folder cannot be renamed.
pub const FOLDER_OFFLINE: &str = "The folder is offline: connect its drive to rename it.";
/// The longest name accepted as typed, in bytes (the name used is cut to a file name's length).
pub const MAX_TYPED: usize = 4096;
/// The bytes kept free for a batch number (`-001`) when a name is shortened.
const NUMBER_ROOM: usize = 4;
/// An extension longer than this is left out of a new name (no photo format has one).
const MAX_EXT: usize = 32;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.rename",
            label: "Rename…",
            menu: "File/Rename…",
            keys: &[],
            params: r#"{"ids":[PhotoId], "name":string, "addDate"?:bool, "addSize"?:bool}: renames the photos' files as a file job, one undo step: one photo takes the name (sanitized) with its own extension, several get -001, -002 … in light-table order; "addDate" adds the date taken (2026-06-04), "addSize" the pixel size (4000x3000); a taken name gets the next free number; if one rename fails the photos of its folder keep their names. Replies {job, pending, total, changed} (job null when no name changes)"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: has_ids,
            run: run_photos,
        },
        CommandSpec {
            id: "folder.rename",
            label: "Rename Folder",
            menu: "",
            keys: &[],
            params: r#"{"folder":FolderId, "name":string}: renames the folder on disk (sanitized; never over another file or folder) as a file job, one undo step; the library folder and those under it keep their ids, and their watched-folder entries follow. Replies {job, pending, path} (job null when the name is unchanged)"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: always,
            run: run_folder,
        },
    ]
}

fn always(_: &Session, _: &Value) -> Result<(), Disabled> {
    Ok(())
}

/// `photo.rename`'s parameters.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PhotoParams {
    ids: Vec<PhotoId>,
    name: String,
    #[serde(default)]
    add_date: bool,
    #[serde(default)]
    add_size: bool,
}

/// `folder.rename`'s parameters.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderParams {
    folder: FolderId,
    name: String,
}

/// The typed name, trimmed: refused when blank or longer than [`MAX_TYPED`].
fn typed(name: &str) -> Result<&str, CmdError> {
    if name.len() > MAX_TYPED {
        return Err(CmdError::Limit(format!("the name is longer than {MAX_TYPED} bytes")));
    }
    let name = name.trim();
    if name.is_empty() { Err(CmdError::BadParams("enter a new name".into())) } else { Ok(name) }
}

/// A renamed photo's name before any number: `base`, then `-<date>` and `-<width>x<height>` when
/// given, then `.ext`; `base` is shortened on a character boundary so that the name, with room for
/// a batch number when `several`, fits a file name ([`MAX_NAME_BYTES`]).
pub fn compose(base: &str, date: Option<Date>, size: Option<(u32, u32)>, ext: Option<&str>, several: bool) -> String {
    let mut tail = String::new();
    if let Some(d) = date {
        tail.push_str(&format!("-{d}"));
    }
    if let Some((w, h)) = size {
        tail.push_str(&format!("-{w}x{h}"));
    }
    let ext = ext.filter(|e| e.len() <= MAX_EXT).map(|e| format!(".{e}")).unwrap_or_default();
    let room = MAX_NAME_BYTES.saturating_sub(tail.len() + ext.len() + if several { NUMBER_ROOM } else { 0 });
    let base = names::fit(base, room).trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    format!("{base}{tail}{ext}")
}

/// The name `photo` asks for (before any number) when renamed `typed` (see [`compose`]); the
/// example line of Rename Files shows it for the first photo, `names::numbered(…, 1)` when
/// several.
pub fn wanted(lib: &Library, photo: &Photo, typed: &str, add_date: bool, add_size: bool, several: bool) -> String {
    let (stem, ext) = names::split_ext(&photo.name);
    let base = names::sanitize(typed.trim(), stem);
    let date = if add_date { lib.sort_date(photo).map(|d| d.date()) } else { None };
    let size = (add_size && photo.width > 0 && photo.height > 0).then_some((photo.width, photo.height));
    compose(&base, date, size, ext, several)
}

/// A name as compared for collisions: case ignored (a volume may ignore it; on one that does not,
/// a name taken in another case only moves the photo on to the next number).
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// One photo of a batch as the plan sees it: its folder, its name now and the name it asks for.
#[derive(Clone, Debug)]
struct Want {
    photo: PhotoId,
    dir: PathBuf,
    from: String,
    name: String,
}

/// What a batch's names are decided from: the photos in light-table order, whether they are
/// several (numbered), and per folder the folded names of the library's other photos there.
#[derive(Debug)]
struct Spec {
    wants: Vec<Want>,
    several: bool,
    held: HashMap<PathBuf, HashSet<String>>,
}

/// Gives each photo of `spec` its name, in order: the name it asks for (one photo) or the next
/// number (several), passing over a name another photo of the folder has (the library's or the
/// batch's, whose files count as taken until they moved), one an earlier photo of the batch took,
/// and one `on_disk(i, name)` says another file holds.
fn assign(spec: &Spec, mut on_disk: impl FnMut(usize, &str) -> Result<bool, String>) -> Result<Vec<String>, String> {
    let mut current: HashMap<(&Path, String), Vec<PhotoId>> = HashMap::new();
    for w in &spec.wants {
        current.entry((w.dir.as_path(), fold(&w.from))).or_default().push(w.photo);
    }
    let mut claimed: HashSet<(&Path, String)> = HashSet::new();
    let mut next: u32 = 1;
    let mut out = Vec::with_capacity(spec.wants.len());
    for (i, w) in spec.wants.iter().enumerate() {
        let held = spec.held.get(&w.dir);
        let first = if spec.several { next } else { 0 };
        let mut chosen = None;
        for n in first..=MAX_NUMBER {
            let candidate = if n == 0 { w.name.clone() } else { names::numbered(&w.name, n) };
            let key = (w.dir.as_path(), fold(&candidate));
            let others = current.get(&key).is_some_and(|ids| ids.iter().any(|id| *id != w.photo));
            if others || claimed.contains(&key) || held.is_some_and(|h| h.contains(&key.1)) || on_disk(i, &candidate)? {
                continue;
            }
            claimed.insert(key);
            next = n.saturating_add(1);
            chosen = Some(candidate);
            break;
        }
        out.push(chosen.ok_or_else(|| format!("there is no free name like {} in {}", w.name, w.dir.display()))?);
    }
    Ok(out)
}

/// [`assign`] with the disk (on the job's thread): every photo's file must still be there, and a
/// name held on disk by anything but the photo's own file (by identity: on a volume that ignores
/// case `IMG.jpg` is `img.jpg` itself) is taken.
fn plan_on_disk(spec: &Spec) -> Result<HashMap<PhotoId, String>, String> {
    let mut own: Vec<Identity> = Vec::with_capacity(spec.wants.len());
    for w in &spec.wants {
        let path = w.dir.join(&w.from);
        own.push(dalica_fs::identity(&path).map_err(|e| e.to_string())?.ok_or_else(|| format!("{} is no longer there", path.display()))?);
    }
    let names = assign(spec, |i, name| {
        let (Some(w), Some(id)) = (spec.wants.get(i), own.get(i)) else { return Ok(true) };
        match dalica_fs::identity(&w.dir.join(name)).map_err(|e| e.to_string())? {
            Some(other) => Ok(other != *id),
            None => Ok(false),
        }
    })?;
    Ok(spec.wants.iter().map(|w| w.photo).zip(names).collect())
}

/// One rename of a batch: photo `photo`'s file `from` becomes `to` in the item's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub photo: PhotoId,
    pub from: String,
    pub to: String,
}

/// Why [`rename_all`] failed, and the renames that could not be moved back (those files keep
/// their new names, and the library follows them).
#[derive(Debug)]
struct Failed {
    why: String,
    stuck: Vec<Pair>,
}

/// Renames `from` to `to` in `dir` through `fs`, never replacing a file.
fn rename_one(dir: &Path, from: &str, to: &str) -> Result<(), String> {
    let path = dir.join(from);
    let identity = dalica_fs::identity(&path).map_err(|e| e.to_string())?.ok_or_else(|| format!("{} is no longer there", path.display()))?;
    match transfer::move_file(dir, from, identity, dir, to).map_err(|e| e.to_string())? {
        Outcome::Done(_) => Ok(()),
        Outcome::Exists => Err(format!("{} is already there", dir.join(to).display())),
    }
}

/// Renames every pair that changes a name in `dir`, all or nothing: after a failure the renames
/// done are moved back, last first. Returns the renames done.
fn rename_all(dir: &Path, pairs: &[Pair]) -> Result<Vec<Pair>, Failed> {
    let mut done: Vec<Pair> = Vec::new();
    for p in pairs.iter().filter(|p| p.from != p.to) {
        #[cfg(feature = "faults")]
        planted::reach(&dir.join(&p.from));
        let before = dalica_fs::identity(&dir.join(&p.from)).ok().flatten();
        if let Err(e) = rename_one(dir, &p.from, &p.to) {
            // A step after the rename may have failed (the folder's `fsync`, the check after it):
            // a file that got its new name all the same goes back with the others.
            let moved = before.is_some()
                && matches!(dalica_fs::identity(&dir.join(&p.from)), Ok(None))
                && matches!(dalica_fs::identity(&dir.join(&p.to)), Ok(now) if now == before);
            if moved {
                done.push(p.clone());
            }
            let mut why = format!("{} could not be renamed {}: {e}", p.from, p.to);
            let mut stuck = Vec::new();
            for d in done.iter().rev() {
                if let Err(e) = rename_one(dir, &d.to, &d.from) {
                    why.push_str(&format!("; {} could not get its name back ({e}): it is now {}", d.from, d.to));
                    stuck.push(d.clone());
                }
            }
            stuck.reverse();
            return Err(Failed { why, stuck });
        }
        done.push(p.clone());
    }
    Ok(done)
}

/// Planted failures for tests (the `faults` feature): the batch's rename of a given file fails
/// as the disk would (`fs`'s rename step armed on the job's thread), once.
#[cfg(feature = "faults")]
pub mod planted {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    static FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static AFTER: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    /// The next rename of the file at `path` by `photo.rename` fails.
    pub fn fail_rename_of(path: &Path) {
        FILES.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_path_buf());
    }

    /// The next rename of the file at `path` by `photo.rename` fails after the file got its new
    /// name (the check after the rename fails).
    pub fn fail_after_rename_of(path: &Path) {
        AFTER.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_path_buf());
    }

    pub(super) fn reach(path: &Path) {
        use dalica_fs::faults::{Fault, arm, points};
        for (list, point) in [(&FILES, points::MOVE_RENAME), (&AFTER, points::MOVE_VERIFY)] {
            let mut files = list.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(i) = files.iter().position(|p| p == path) {
                files.remove(i);
                arm(point, Fault::Error);
            }
        }
    }
}

/// What an item does when it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// Renames its folder's photos (the first of its folder in the batch; undoing, the reverse).
    Perform,
    /// Reports how its photo fared in its folder's renames (run after them: doing and redoing).
    Follow,
    /// Nothing to do (undoing: its folder's renames are reversed by the performer's item).
    Idle,
}

/// One photo of a rename (the journal's item).
#[derive(Clone, Debug)]
pub struct Item {
    photo: PhotoId,
    dir: PathBuf,
    from: String,
    to: String,
    src: PathBuf,
    dst: PathBuf,
    /// The folder's renames when the item performs them with fixed names (undo, redo).
    group: Vec<Pair>,
    role: Role,
    /// Doing: the names are decided on the job's thread before the first file moves.
    planned: bool,
}

/// What an item did: its photo's name now, and the renames it performed.
#[derive(Clone, Debug)]
pub struct Renamed {
    name: String,
    group: Vec<Pair>,
}

/// The names a batch's photos get, by photo.
type Plan = Arc<HashMap<PhotoId, String>>;

/// The photo rename operation: the batch's spec (doing), the names decided on the disk, and how
/// each photo fared in the run going on.
struct Photos {
    spec: Arc<Spec>,
    plan: Mutex<Option<Result<Plan, String>>>,
    fared: Mutex<HashMap<PhotoId, Result<String, String>>>,
}

impl Photos {
    /// The names decided on the disk, the first time asked (the first item, before any file moves).
    fn plan(&self) -> Result<Plan, String> {
        let mut slot = self.plan.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(plan) = slot.as_ref() {
            return plan.clone();
        }
        let plan = plan_on_disk(&self.spec).map(Arc::new);
        *slot = Some(plan.clone());
        plan
    }

    /// Doing: every photo of the batch in `dir`, with the name the plan gave it.
    fn planned_pairs(&self, dir: &Path) -> Result<Vec<Pair>, String> {
        let plan = self.plan()?;
        self.spec
            .wants
            .iter()
            .filter(|w| w.dir == dir)
            .map(|w| {
                let to = plan.get(&w.photo).cloned().ok_or_else(|| format!("no name was decided for {}", w.from))?;
                Ok(Pair { photo: w.photo, from: w.from.clone(), to })
            })
            .collect()
    }

    fn perform(&self, item: &Item) -> Result<Ran<Renamed>, String> {
        let pairs = if item.planned { self.planned_pairs(&item.dir)? } else { item.group.clone() };
        let (done, failed) = match rename_all(&item.dir, &pairs) {
            Ok(done) => (done, None),
            Err(Failed { why, stuck }) => (stuck, Some(why)),
        };
        {
            let mut fared = self.fared.lock().unwrap_or_else(PoisonError::into_inner);
            for p in &pairs {
                let r = match (done.iter().find(|d| d.photo == p.photo), &failed) {
                    (Some(d), _) => Ok(d.to.clone()),
                    (None, Some(why)) if p.from != p.to => Err(why.clone()),
                    (None, _) => Ok(p.from.clone()),
                };
                fared.insert(p.photo, r);
            }
        }
        if let Some(why) = failed {
            if done.is_empty() {
                return Err(why);
            }
            log::warn!("photo.rename in {}: {why}", item.dir.display());
        }
        let name = done.iter().find(|d| d.photo == item.photo).map_or_else(|| item.from.clone(), |d| d.to.clone());
        Ok(Ran::Done(Renamed { name, group: done }))
    }

    fn follow(&self, item: &Item) -> Result<Ran<Renamed>, String> {
        if item.planned {
            self.plan()?;
        }
        let fared = self.fared.lock().unwrap_or_else(PoisonError::into_inner).get(&item.photo).cloned();
        match fared {
            Some(Ok(name)) => Ok(Ran::Done(Renamed { name, group: Vec::new() })),
            Some(Err(why)) => Err(why),
            None if item.planned => Err("its folder's renames did not run".into()),
            None => Ok(Ran::Done(Renamed { name: item.to.clone(), group: Vec::new() })),
        }
    }
}

impl Driver for Photos {
    type Item = Item;
    type Done = Renamed;

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.src
    }

    fn target<'a>(&self, item: &'a Item) -> Option<&'a Path> {
        Some(&item.dst)
    }

    fn run(&self, item: &Item) -> Result<Ran<Renamed>, String> {
        match item.role {
            Role::Perform => self.perform(item),
            Role::Follow => self.follow(item),
            Role::Idle => Ok(Ran::Done(Renamed { name: item.to.clone(), group: Vec::new() })),
        }
    }

    fn ops(&self, s: &Session, item: &Item, done: &Renamed) -> Result<Vec<Op>, String> {
        if done.group.is_empty() {
            return Ok(Vec::new());
        }
        let folder = s.library().folder_by_path(&item.dir).ok_or_else(|| format!("{} is no longer in the library", item.dir.display()))?;
        let op = Op::MovePhotos { moves: done.group.iter().map(|p| (p.photo, folder, p.to.clone())).collect() };
        Ok(vec![s.library().stamped(op, s.clock().now())])
    }

    fn reverse(&self, item: &Item, done: &Renamed) -> Result<Item, String> {
        let group: Vec<Pair> = done.group.iter().rev().map(|p| Pair { photo: p.photo, from: p.to.clone(), to: p.from.clone() }).collect();
        Ok(Item {
            photo: item.photo,
            dir: item.dir.clone(),
            from: done.name.clone(),
            to: item.from.clone(),
            src: item.dir.join(&done.name),
            dst: item.src.clone(),
            group,
            role: if item.role == Role::Perform { Role::Perform } else { Role::Idle },
            planned: false,
        })
    }

    fn settled(&self, item: &Item, done: &Renamed) -> Item {
        Item {
            to: done.name.clone(),
            dst: item.dir.join(&done.name),
            group: done.group.clone(),
            role: if item.role == Role::Perform { Role::Perform } else { Role::Follow },
            planned: false,
            ..item.clone()
        }
    }

    fn carry(&self, item: &Item, done: &Renamed) -> Option<Carry> {
        (!done.group.is_empty()).then(|| Carry::rename(&item.dir, done.group.iter().map(|p| (p.from.clone(), p.to.clone())).collect()))
    }
}

/// `ids` in light-table order (folders newest first, then by path; each folder's photos in its
/// own sort), with the folded names of each folder's other photos.
fn in_order(lib: &Library, ids: &[PhotoId]) -> (Vec<PhotoId>, HashMap<FolderId, HashSet<String>>) {
    let wanted: HashSet<PhotoId> = ids.iter().copied().collect();
    let mut folders: Vec<FolderId> = Vec::new();
    for id in ids {
        if let Some(p) = lib.photo(*id)
            && !folders.contains(&p.folder)
        {
            folders.push(p.folder);
        }
    }
    let mut keyed: Vec<(Reverse<Option<DateTime>>, Option<PathBuf>, FolderId)> =
        folders.into_iter().map(|f| (Reverse(lib.folder_date(f)), lib.folder(f).map(|f| f.path.clone()), f)).collect();
    keyed.sort();
    let (mut order, mut held) = (Vec::with_capacity(ids.len()), HashMap::new());
    for (_, _, folder) in keyed {
        let others: &mut HashSet<String> = held.entry(folder).or_default();
        for id in lib.photos_sorted(folder) {
            if wanted.contains(&id) {
                order.push(id);
            } else if let Some(p) = lib.photo(id) {
                others.insert(fold(&p.name));
            }
        }
    }
    (order, held)
}

fn run_photos(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let PhotoParams { ids, name, add_date, add_size } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let name = typed(&name)?;
    let lib = s.library();
    if ids.iter().filter_map(|id| lib.photo(*id)).any(|p| s.is_folder_offline(p.folder)) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    let (order, held_by_folder) = in_order(lib, &ids);
    let several = order.len() > 1;
    let mut wants = Vec::with_capacity(order.len());
    let mut held = HashMap::new();
    for id in &order {
        let photo = lib.photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let dir = lib.folder(photo.folder).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {}", photo.folder)))?;
        if let Some(h) = held_by_folder.get(&photo.folder) {
            held.entry(dir.clone()).or_insert_with(|| h.clone());
        }
        wants.push(Want { photo: *id, dir, from: photo.name.clone(), name: wanted(lib, photo, name, add_date, add_size, several) });
    }
    let spec = Spec { wants, several, held };
    // What the library knows gives the journal's targets; the job decides again with the disk.
    let names = assign(&spec, |_, _| Ok(false)).map_err(CmdError::Limit)?;
    let total = spec.wants.len();
    let changed = spec.wants.iter().zip(&names).filter(|(w, n)| w.from != **n).count();
    if changed == 0 {
        return Ok(json!({ "job": null, "pending": false, "total": total, "changed": 0 }));
    }
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let items: Vec<Item> = spec
        .wants
        .iter()
        .zip(&names)
        .map(|(w, to)| Item {
            photo: w.photo,
            dir: w.dir.clone(),
            from: w.from.clone(),
            to: to.clone(),
            src: w.dir.join(&w.from),
            dst: w.dir.join(to),
            group: Vec::new(),
            role: if seen.insert(w.dir.clone()) { Role::Perform } else { Role::Follow },
            planned: true,
        })
        .collect();
    let destination = items.first().map(|i| i.dir.clone()).filter(|_| seen.len() == 1);
    let driver = Photos { spec: Arc::new(spec), plan: Mutex::new(None), fared: Mutex::new(HashMap::new()) };
    let mut op = FileOp::new(driver, FileOpKind::Rename, LABEL, items);
    if let Some(dir) = destination {
        op = op.destination(dir);
    }
    let job = s.start_file_op(op);
    Ok(json!({ "job": job, "pending": true, "total": total, "changed": changed }))
}

/// One folder renamed in its parent.
#[derive(Clone, Debug)]
pub struct FolderItem {
    folder: FolderId,
    parent: PathBuf,
    from: String,
    to: String,
    src: PathBuf,
    dst: PathBuf,
}

/// The folder rename operation.
struct Folders;

impl Driver for Folders {
    type Item = FolderItem;
    type Done = ();

    fn source<'a>(&self, item: &'a FolderItem) -> &'a Path {
        &item.src
    }

    fn target<'a>(&self, item: &'a FolderItem) -> Option<&'a Path> {
        Some(&item.dst)
    }

    fn run(&self, item: &FolderItem) -> Result<Ran<()>, String> {
        match transfer::move_folder(&item.parent, &item.from, &item.parent, &item.to).map_err(|e| e.to_string())? {
            Outcome::Done(_) => Ok(Ran::Done(())),
            Outcome::Exists => Err(format!("{} is already there", item.dst.display())),
        }
    }

    fn ops(&self, s: &Session, item: &FolderItem, _: &()) -> Result<Vec<Op>, String> {
        Ok(vec![s.library().stamped(Op::MoveFolder { id: item.folder, path: item.dst.clone() }, s.clock().now())])
    }

    fn reverse(&self, item: &FolderItem, _: &()) -> Result<FolderItem, String> {
        Ok(FolderItem { from: item.to.clone(), to: item.from.clone(), src: item.dst.clone(), dst: item.src.clone(), ..item.clone() })
    }

    /// The watched-folder entries at and under the folder follow it (`folders.json`).
    fn followed(&self, s: &mut Session, item: &FolderItem, _: &()) {
        s.rebase_folders(&item.src, &item.dst);
    }
}

fn run_folder(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let FolderParams { folder, name } = params(p)?;
    let name = typed(&name)?;
    let path = s.library().folder(folder).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {folder}")))?;
    if s.is_folder_offline(folder) {
        return Err(CmdError::Disabled(FOLDER_OFFLINE.into()));
    }
    let (Some(parent), Some(from)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Err(CmdError::Disabled("A drive's top folder cannot be renamed.".into()));
    };
    let to = names::sanitize(name, from);
    if to == from {
        return Ok(json!({ "job": null, "pending": false, "path": path }));
    }
    let dst = parent.join(&to);
    if s.library().folder_by_path(&dst).is_some_and(|other| other != folder) {
        return Err(CmdError::Disabled(format!("A folder named \"{to}\" is already in the library.")));
    }
    let item = FolderItem { folder, parent: parent.to_path_buf(), from: from.to_string(), to, src: path.clone(), dst: dst.clone() };
    let job = s.start_file_op(FileOp::new(Folders, FileOpKind::RenameFolder, FOLDER_LABEL, vec![item]).destination(dst.clone()));
    Ok(json!({ "job": job, "pending": true, "path": dst }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(photo: u64, from: &str, name: &str) -> Want {
        Want { photo: PhotoId::new(photo), dir: PathBuf::from("/p/Lisbon"), from: from.into(), name: name.into() }
    }

    fn spec(wants: Vec<Want>, held: &[&str]) -> Spec {
        let several = wants.len() > 1;
        Spec { wants, several, held: HashMap::from([(PathBuf::from("/p/Lisbon"), held.iter().map(|h| fold(h)).collect())]) }
    }

    fn none(_: usize, _: &str) -> Result<bool, String> {
        Ok(false)
    }

    #[test]
    fn names_are_composed_with_the_date_the_size_and_the_extension() {
        let d = Date::new(2026, 6, 4).unwrap();
        assert_eq!(compose("Lisbon", None, None, Some("jpg"), false), "Lisbon.jpg");
        assert_eq!(compose("Lisbon", Some(d), Some((4000, 3000)), Some("JPG"), true), "Lisbon-2026-06-04-4000x3000.JPG");
        assert_eq!(compose("Lisbon", None, Some((40, 30)), None, false), "Lisbon-40x30");
        // Shortened to fit, with room for the number when several.
        let long = "é".repeat(200);
        let one = compose(&long, None, None, Some("jpg"), false);
        assert!(one.len() <= MAX_NAME_BYTES && one.len() > MAX_NAME_BYTES - 2 && one.ends_with(".jpg"), "{}", one.len());
        let many = compose(&long, Some(d), None, Some("jpg"), true);
        assert!(many.len() + NUMBER_ROOM <= MAX_NAME_BYTES && many.ends_with("-2026-06-04.jpg"));
        assert!(names::numbered(&many, 1).ends_with("-2026-06-04-001.jpg"));
    }

    #[test]
    fn several_photos_are_numbered_in_order_and_a_taken_number_moves_them_on() {
        let three = || vec![want(1, "a.jpg", "Lisbon.jpg"), want(2, "b.jpg", "Lisbon.jpg"), want(3, "c.jpg", "Lisbon.jpg")];
        assert_eq!(assign(&spec(three(), &[]), none).unwrap(), ["Lisbon-001.jpg", "Lisbon-002.jpg", "Lisbon-003.jpg"]);
        assert_eq!(assign(&spec(three(), &["lisbon-002.JPG"]), none).unwrap(), ["Lisbon-001.jpg", "Lisbon-003.jpg", "Lisbon-004.jpg"]);
        // Taken on the disk only.
        let two = || vec![want(1, "a.jpg", "Lisbon.jpg"), want(2, "b.jpg", "Lisbon.jpg")];
        assert_eq!(assign(&spec(two(), &[]), |_, n| Ok(n == "Lisbon-001.jpg")).unwrap(), ["Lisbon-002.jpg", "Lisbon-003.jpg"]);
        // A name the batch moves away from still counts as taken; a photo may keep its own.
        let swapped = vec![want(1, "Lisbon-002.jpg", "Lisbon.jpg"), want(2, "Lisbon-001.jpg", "Lisbon.jpg")];
        assert_eq!(assign(&spec(swapped, &[]), none).unwrap(), ["Lisbon-002.jpg", "Lisbon-003.jpg"]);
        let kept = vec![want(1, "Lisbon-001.jpg", "Lisbon.jpg"), want(2, "Lisbon-002.jpg", "Lisbon.jpg")];
        assert_eq!(assign(&spec(kept, &[]), none).unwrap(), ["Lisbon-001.jpg", "Lisbon-002.jpg"]);
    }

    #[test]
    fn one_photo_takes_the_name_or_the_first_free_number() {
        assert_eq!(assign(&spec(vec![want(1, "a.jpg", "Lisbon.jpg")], &[]), none).unwrap(), ["Lisbon.jpg"]);
        assert_eq!(assign(&spec(vec![want(1, "a.jpg", "Lisbon.jpg")], &["Lisbon.jpg"]), none).unwrap(), ["Lisbon-001.jpg"]);
        // A change of case keeps the photo's own name free; another photo's in another case is taken.
        assert_eq!(assign(&spec(vec![want(1, "img.jpg", "IMG.jpg")], &[]), none).unwrap(), ["IMG.jpg"]);
        assert_eq!(assign(&spec(vec![want(1, "img.jpg", "IMG.jpg")], &["IMG.jpg"]), none).unwrap(), ["IMG-001.jpg"]);
        // A disk that cannot be looked at fails the plan.
        assert!(assign(&spec(vec![want(1, "a.jpg", "Lisbon.jpg")], &[]), |_, _| Err("offline".into())).is_err());
    }
}
