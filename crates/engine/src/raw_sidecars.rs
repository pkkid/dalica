//! A RAW's `.xmp` sidecar travels with it (`xmp-interop.md` §7, `data-model.md` §8): when a move,
//! a copy or a rename takes a RAW file, its sidecar goes in the same item of the file job, right
//! after the RAW, through `dalica-fs` (a move moves it, a copy copies it), named after the RAW's
//! new name. Undo and redo take it back with the RAW (the names it went under are kept with the
//! item). A delete leaves the sidecar where it is (the trash takes only what the user chose), and a
//! folder move needs nothing (the sidecar is inside the folder).
//!
//! - **Which file.** The sidecar the RAW reads, as `dalica-meta`'s [`Sidecars`] resolves it from
//!   the folder's names: `<stem>.xmp` when the RAW owns the stem, else `<file>.xmp`, in any case.
//!   Only RAW files have one, so the JPEG of a RAW+JPEG pair moves alone. Which photos are RAWs is
//!   told by name ([`maybe_raw`]: an extension no other photo format has), as the metadata writes
//!   do; `dalica-meta` compares the rest.
//! - **Its new name** ([`renamed`]): a `<file>.xmp` sidecar becomes `<new file>.xmp`, a stem one
//!   `<new stem>.xmp` (so the `-001` a clash gave the RAW names the sidecar the same), keeping the
//!   extension's spelling; a stem another RAW of the destination has makes it `<new file>.xmp`, so
//!   two photos never share one.
//! - **Never in the way** ([`carry`]): a sidecar whose name is taken at the destination is never
//!   replaced; it stays where it was, the RAW's item is still done, and the item says so (a
//!   notice). The same for a sidecar that could not move (a failure of the disk); one that moved
//!   although a step after its rename failed counts as moved.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{FolderId, names};
use dalica_fs::transfer::{self, Outcome};
use dalica_fs::trash::{self, Sent};
use dalica_library::Library;
use dalica_meta::xmp_sidecar::{EXTENSION, Sidecars};
use dalica_store::carry::Mode;

/// Extensions of the photo formats that are not RAW files (and of videos), compared without case.
const NOT_RAW: [&str; 18] =
    ["jpg", "jpeg", "jpe", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "jxl", "psd", "mp4", "mov", "m4v", "mkv"];

/// Whether a photo named `name` may be a RAW file: it has an extension, and it is neither another
/// photo format's nor `.xmp`.
pub fn maybe_raw(name: &str) -> bool {
    match names::split_ext(name).1 {
        Some(ext) => !ext.eq_ignore_ascii_case(EXTENSION) && !NOT_RAW.iter().any(|n| n.eq_ignore_ascii_case(ext)),
        None => false,
    }
}

/// A name's stem, folded for comparing without case.
pub(crate) fn stem_key(name: &str) -> String {
    names::split_ext(name).0.to_lowercase()
}

/// The names of the photos of `folder` that may be RAW files ([`maybe_raw`]).
pub(crate) fn raws_in(lib: &Library, folder: FolderId) -> Vec<String> {
    lib.photos_in(folder).into_iter().filter(|p| maybe_raw(&p.name)).map(|p| p.name.clone()).collect()
}

/// A sidecar's name before and after it went with its RAW (in one folder for a rename).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub from: String,
    pub to: String,
}

impl Side {
    /// The same sidecar going back.
    pub fn reversed(&self) -> Side {
        Side { from: self.to.clone(), to: self.from.clone() }
    }
}

/// What became of a RAW's sidecar in one item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Went {
    /// It has none (or it is not a RAW).
    #[default]
    Nothing,
    /// Moved (or renamed) under `to`.
    Moved(Side),
    /// Copied under `to`.
    Copied(Side),
    /// It stayed where it was, and why (a name taken at the destination, a failure of the disk).
    Stayed { name: String, why: String },
}

impl Went {
    /// The names it went under, when it went.
    pub fn side(&self) -> Option<&Side> {
        match self {
            Went::Moved(s) | Went::Copied(s) => Some(s),
            Went::Nothing | Went::Stayed { .. } => None,
        }
    }

    /// What the item says about a sidecar that stayed (`None` when nothing is to be said).
    pub fn note(&self, folder: &Path) -> Option<String> {
        match self {
            Went::Stayed { name, why } => Some(format!("The .xmp sidecar {name} stayed in {}: {why}.", folder.display())),
            _ => None,
        }
    }
}

/// How an item finds its RAW's sidecar.
#[derive(Clone, Debug, Default)]
pub(crate) enum Plan {
    /// Not a RAW: nothing goes with it.
    #[default]
    None,
    /// Looked for when the item runs, among its folder's names ([`Finder`]).
    Find(Arc<Finder>),
    /// The names it went under before (undo and redo): `None` when none went.
    Exact(Option<Side>),
}

impl Plan {
    /// For the photo named `name` of a move or copy that `finder` serves.
    pub(crate) fn for_photo(name: &str, finder: &Arc<Finder>) -> Plan {
        if maybe_raw(name) { Plan::Find(Arc::clone(finder)) } else { Plan::None }
    }
}

/// The sidecars of the folders one move or copy takes RAWs from, each folder listed once (on the
/// job's thread, the first time one of its RAWs moves), and the stems the destination's RAWs have.
#[derive(Debug, Default)]
pub(crate) struct Finder {
    /// The library's RAW names per source folder.
    raws: HashMap<PathBuf, Vec<String>>,
    /// The folded stems of the destination's RAWs, as the library holds them.
    dest_stems: HashSet<String>,
    found: Mutex<HashMap<PathBuf, Result<Arc<Sidecars>, String>>>,
}

impl Finder {
    /// `raws`: the library's RAW names per source folder; `dest`: the destination's.
    pub(crate) fn new(raws: HashMap<PathBuf, Vec<String>>, dest: &[String]) -> Finder {
        Finder { raws, dest_stems: dest.iter().map(|n| stem_key(n)).collect(), found: Mutex::new(HashMap::new()) }
    }

    /// The sidecar the RAW `raw` of `dir` reads, by name. On the job's thread.
    fn sidecar(&self, dir: &Path, raw: &str) -> Result<Option<String>, String> {
        let mut found = self.found.lock().unwrap_or_else(PoisonError::into_inner);
        let sidecars = match found.get(dir) {
            Some(s) => s.clone(),
            None => {
                let raws = self.raws.get(dir).cloned().unwrap_or_default();
                let s = resolve(dir, |n| n == raw || raws.iter().any(|r| r == n)).map(Arc::new);
                found.insert(dir.to_path_buf(), s.clone());
                s
            }
        }?;
        Ok(sidecars.get(raw).and_then(|s| s.existing.clone()))
    }

    /// Whether a RAW of the destination other than `name` has `name`'s stem.
    fn stem_taken(&self, name: &str) -> bool {
        self.dest_stems.contains(&stem_key(name))
    }
}

/// The file names of `dir` (files only, no links).
fn names_in(dir: &Path) -> Result<Vec<String>, String> {
    let limits = dalica_fs::Limits { max_depth: 1, ..dalica_fs::Limits::default() };
    let walk = dalica_fs::walk(dir, limits, |_| false).map_err(|e| e.to_string())?;
    Ok(walk
        .entries
        .into_iter()
        .filter(|e| e.is_file && !e.is_link)
        .filter_map(|e| e.path.file_name().and_then(|n| n.to_str()).map(str::to_owned))
        .collect())
}

/// The sidecars of `dir`'s RAWs (`is_raw` of a name), from one listing. On the job's thread.
pub(crate) fn resolve(dir: &Path, is_raw: impl Fn(&str) -> bool) -> Result<Sidecars, String> {
    Ok(Sidecars::new(&names_in(dir)?, is_raw))
}

/// The name sidecar `sidecar` of the RAW `raw_from` takes when the RAW is named `raw_to`: a
/// `<file>.xmp` sidecar `<raw_to>.xmp`, a stem one `<stem of raw_to>.xmp` unless `stem_taken`
/// (another RAW where it goes has that stem), then `<raw_to>.xmp`. The extension keeps its
/// spelling.
pub fn renamed(sidecar: &str, raw_from: &str, raw_to: &str, stem_taken: bool) -> String {
    let (base, ext) = names::split_ext(sidecar);
    let ext = ext.unwrap_or(EXTENSION);
    let by_file = base.to_lowercase() == raw_from.to_lowercase();
    if by_file || stem_taken { format!("{raw_to}.{ext}") } else { format!("{}.{ext}", names::split_ext(raw_to).0) }
}

/// A RAW's sidecar as found before the RAW goes (the folder listed with the RAW in it).
#[derive(Debug)]
pub(crate) enum Found {
    Nothing,
    /// The names it goes under (undo and redo).
    Exact(Side),
    /// Its name now; its new name follows the RAW's ([`renamed`]).
    Named(String, Arc<Finder>),
    /// Why it could not be looked for.
    Failed(String),
}

/// Looks for the sidecar of the RAW `raw` of `from` as `plan` says, before the RAW goes. On the
/// job's thread.
pub(crate) fn find(plan: &Plan, from: &Path, raw: &str) -> Found {
    match plan {
        Plan::None | Plan::Exact(None) => Found::Nothing,
        Plan::Exact(Some(side)) => Found::Exact(side.clone()),
        Plan::Find(finder) => match finder.sidecar(from, raw) {
            Ok(None) => Found::Nothing,
            Ok(Some(name)) => Found::Named(name, Arc::clone(finder)),
            Err(e) => Found::Failed(e),
        },
    }
}

/// Sends the sidecar `found` of the RAW `raw` after it (just moved or copied from `from` to
/// `to`/`new_name`). On the job's thread; never fails (see [`carry`]).
pub(crate) fn follow(mode: Mode, found: &Found, from: &Path, raw: &str, to: &Path, new_name: &str) -> Went {
    let side = match found {
        Found::Nothing => return Went::Nothing,
        Found::Exact(side) => side.clone(),
        Found::Named(name, finder) => {
            let taken = to != from && finder.stem_taken(new_name);
            Side { to: renamed(name, raw, new_name, taken), from: name.clone() }
        }
        Found::Failed(e) => return Went::Stayed { name: format!("of {raw}"), why: format!("its folder could not be listed ({e})") },
    };
    carry(mode, from, side, to)
}

/// Moves (or copies) the sidecar `side.from` of `from` to `side.to` in `to` through `dalica-fs`,
/// never replacing a file. A sidecar gone meanwhile is nothing; one whose name is taken, or that
/// could not go, stays and says why; one that got its new name although a step after the rename
/// failed has moved.
pub(crate) fn carry(mode: Mode, from: &Path, side: Side, to: &Path) -> Went {
    let src = from.join(&side.from);
    #[cfg(feature = "faults")]
    planted::reach(&src);
    let stayed = |why: String| Went::Stayed { name: side.from.clone(), why };
    let identity = match dalica_fs::identity(&src) {
        Ok(Some(id)) => id,
        Ok(None) => return Went::Nothing,
        Err(e) => return stayed(e.to_string()),
    };
    let taken = || format!("{} already has a file named {}", folder_name(to), side.to);
    match mode {
        Mode::Move => match transfer::move_file(from, &side.from, identity, to, &side.to) {
            Ok(Outcome::Done(_)) => Went::Moved(side),
            Ok(Outcome::Exists) => stayed(taken()),
            Err(e) => {
                let moved = matches!(dalica_fs::identity(&src), Ok(None))
                    && matches!(dalica_fs::identity(&to.join(&side.to)), Ok(Some(now)) if now == identity);
                if moved { Went::Moved(side) } else { stayed(e.to_string()) }
            }
        },
        Mode::Copy => match transfer::copy_file(from, &side.from, identity, to, &side.to) {
            Ok(Outcome::Done(_)) => Went::Copied(side),
            Ok(Outcome::Exists) => stayed(taken()),
            Err(e) => stayed(e.to_string()),
        },
    }
}

/// Undoing a copy whose RAW went to the trash: its copied sidecar `path` goes too (to the trash,
/// whence it can come back). `Some` with why when it stays.
pub(crate) fn uncopy(path: &Path) -> Option<String> {
    let stays = |why: String| Some(format!("the .xmp sidecar {} stays: {why}", path.display()));
    match dalica_fs::identity(path) {
        Ok(None) => return None,
        Ok(Some(_)) => {}
        Err(e) => return stays(e.to_string()),
    }
    match trash::send(path) {
        Ok(Sent::Trashed(_)) => None,
        Ok(Sent::NoTrash) => stays("its drive has no trash".into()),
        Err(e) => stays(e.to_string()),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Planted failures for tests (the `faults` feature): the next carry of the sidecar at a path arms
/// a fault point of `dalica-fs` on the job's thread, once.
#[cfg(feature = "faults")]
pub mod planted {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    use dalica_fs::faults::{self, Fault};

    type Armed = (PathBuf, &'static str, Fault);

    static ARMED: Mutex<Vec<Armed>> = Mutex::new(Vec::new());

    /// The next carry of the sidecar at `path` (its place before it goes) fails at `point` with
    /// `fault`.
    pub fn fault_at(path: &Path, point: &'static str, fault: Fault) {
        ARMED.lock().unwrap_or_else(PoisonError::into_inner).push((path.to_path_buf(), point, fault));
    }

    /// Whether a failure planted for the sidecar at `path` was not reached yet.
    pub fn pending(path: &Path) -> bool {
        ARMED.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(p, _, _)| p == path)
    }

    pub(super) fn reach(path: &Path) {
        let mut armed = ARMED.lock().unwrap_or_else(PoisonError::into_inner);
        while let Some(i) = armed.iter().position(|(p, _, _)| p == path) {
            let (_, point, fault) = armed.remove(i);
            faults::arm(point, fault);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raws_are_told_by_name() {
        assert!(maybe_raw("IMG_0001.CR3") && maybe_raw("a.nef") && maybe_raw("x.DNG"));
        assert!(!maybe_raw("IMG_0001.JPG") && !maybe_raw("a.tiff") && !maybe_raw("noext") && !maybe_raw(".hidden") && !maybe_raw("IMG.XMP"));
    }

    #[test]
    fn a_sidecar_is_named_after_the_raws_new_name_in_its_own_form() {
        assert_eq!(renamed("IMG_0001.xmp", "IMG_0001.CR3", "trip.CR3", false), "trip.xmp");
        assert_eq!(renamed("IMG_0001.XMP", "IMG_0001.CR3", "IMG_0001-001.CR3", false), "IMG_0001-001.XMP");
        assert_eq!(renamed("IMG_0002.CR3.xmp", "IMG_0002.CR3", "trip2.CR3", false), "trip2.CR3.xmp");
        assert_eq!(renamed("img_0002.cr3.xmp", "IMG_0002.CR3", "trip2.CR3", false), "trip2.CR3.xmp");
        // Another RAW where it goes has the stem: the file form.
        assert_eq!(renamed("IMG_0001.xmp", "IMG_0001.CR3", "IMG_0001.CR3", true), "IMG_0001.CR3.xmp");
    }
}
