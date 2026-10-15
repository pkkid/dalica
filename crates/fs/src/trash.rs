//! The OS trash (`security.md` §5.7, `data-model.md` §8): deleting a photo, a video or a folder
//! sends it to the trash of the system it lives on, and nothing in Dalica deletes library content
//! permanently.
//!
//! [`send`] hands one file or folder to the `trash` crate: the freedesktop.org trash on Linux and
//! the BSDs, the Recycle Bin through the Windows shell, `NSFileManager` on macOS (never the
//! Finder: it would ask for permission to control another application). A volume without a trash
//! (some network shares, removable media) answers [`Sent::NoTrash`] and the item stays where it
//! is; it never falls back to a permanent delete:
//!
//! - **Linux and the BSDs:** before the item is handed over, Dalica checks that a trash on the
//!   item's own volume takes it by a rename: the home trash when it is on that volume, else the
//!   volume's `.Trash/<uid>` (a sticky `.Trash` that is not a link) or its `.Trash-<uid>`, made
//!   (owner-only) when missing. When none can be used the answer is `NoTrash`; the item is never
//!   copied into a trash on another volume (the crate's fallback for a top folder it cannot write).
//!   One gap stays: a volume the system does not list as a mount point of its own (a btrfs
//!   subvolume inside another mount) is trashed where the crate's mount table says, which may be
//!   the home trash on another volume, by the crate's copy and delete.
//!   A name longer than 240 bytes leaves no room for the crate's record,
//!   `info/<name>.trashinfo`, within the 255 bytes a name may have. Dalica trashes such an item
//!   itself, in the trash the check found: under a shortened name that is free in that trash
//!   (`<the name's first 200 bytes>~<n>.<extension>`), with a record of that name holding the
//!   item's whole path. It is listed and restored under its own name, as any other item.
//! - **Windows:** the shell is asked to recycle without any dialog and to warn instead of deleting
//!   permanently; that warning cannot be shown, so the shell aborts, which is `NoTrash`.
//! - **macOS:** `NSFileManager` refuses a volume without a trash (`NSFeatureUnsupportedError`),
//!   which is `NoTrash`.
//!
//! [`restore`] brings items back to their old path where the platform's API can list the trash
//! (Linux and the BSDs, Windows); elsewhere it answers [`Restore::Unsupported`]. An item is found
//! by its old path and, where the trashed object can be looked at (the freedesktop `files/` folder;
//! the Recycle Bin's own file on Windows), by its identity, so a newer item of the same name is
//! never taken for it. Something now at the old path is [`Restore::Exists`]; nothing is replaced.
//!
//! **Test builds** (the `faults` feature in a debug build; see the `faults` module) use a stand-in
//! trash instead: `send` renames the item into a folder of this process under the system temporary
//! folder, `restore` renames it back, with the same fault points, the same refusals and the same
//! answers (`Restore::Unsupported` on macOS, `Exists` when something is at the old path, the item
//! found by its old path and identity). It holds what every thread of the process sent. An item on
//! another volume than that folder still goes to the OS trash, so a cross-volume test sees the real
//! thing. Each process's folder is locked while the process lives, and the next test process
//! removes the folders of processes that have ended, so nothing piles up. A test that checks the OS
//! trash itself opts in with `faults::real_trash(true)`.
//!
//! The crate is called under `catch_unwind`: it unwraps on a malformed `.trashinfo` file or a mount
//! point that is not UTF-8, and a panic there must not take the worker down (`never-crash.md`).
//! A link is trashed as itself and its target is left alone (`security.md` §5.6: Dalica's file
//! operations act on a link, never through it); it comes back as the same link.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Component, Path, PathBuf};

use crate::atomic::Identity;
use crate::faults::{self, Fault, points};
use crate::{Error, Result, guard, io_err};

/// An item [`send`] put in the trash: what [`restore`] needs to find it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    /// Where it was: its folder resolved (links and `..` followed), then its own name, as the
    /// trash records it.
    pub original: PathBuf,
    /// Its identity when it was sent; a rename into the trash keeps the object.
    pub identity: Identity,
    /// A folder (with everything in it) rather than a file or a link.
    pub folder: bool,
}

/// What [`send`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    /// The item is in the trash.
    Trashed(Trashed),
    /// The item's volume has no trash Dalica can use; the item is still in place. Delete it from
    /// the file manager if that is what you want.
    NoTrash,
}

/// What [`restore`] did with one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Restore {
    /// The item is back at its old path.
    Restored,
    /// Something is at the old path now; the item stays in the trash.
    Exists,
    /// The item is no longer in the trash (emptied, or restored another way).
    NotInTrash,
    /// This platform's trash cannot be listed (macOS): restore it from the file manager.
    Unsupported,
}

/// Sends the file or folder at `path` (absolute) to the OS trash. A folder goes with everything in
/// it; a link goes as itself, never its target. Refused with nothing changed: a relative path, a
/// path ending in `..` or naming a root, something that is neither a file, a folder nor a link
/// (a device, a pipe). Fault points
/// [`points::TRASH_NO_TRASH`] (armed as an error: the volume has no trash) and
/// [`points::TRASH_SEND`].
pub fn send(path: &Path) -> Result<Sent> {
    guard::check()?;
    let path = checked(path)?;
    let meta = std::fs::symlink_metadata(&path).map_err(io_err("trash", &path))?;
    let kind = meta.file_type();
    if !kind.is_file() && !kind.is_dir() && !kind.is_symlink() {
        return Err(io_err("trash", &path)(io::Error::other("neither a file nor a folder")));
    }
    let original = resolved(&path)?;
    let identity = Identity::of_path(&original, &meta, false).map_err(io_err("trash", &original))?;
    let item = Trashed { original, identity, folder: kind.is_dir() };
    match faults::hit(points::TRASH_NO_TRASH) {
        Some(Fault::Error) => return Ok(Sent::NoTrash),
        Some(Fault::Crash) => return Err(Error::Crashed(points::TRASH_NO_TRASH)),
        None => {}
    }
    let stand_in = faults::stand_in_trash() && stand_in::takes(&item);
    if !stand_in && !platform::usable_trash(&item.original, &meta)? {
        return Ok(Sent::NoTrash);
    }
    faults::at(points::TRASH_SEND, "trash", &item.original)?;
    if stand_in {
        return stand_in::send(item);
    }
    let sent = match platform::send_long_name(&item.original, &meta) {
        Some(own) => own.map_err(CrateError::Own),
        None => catch(|| platform::context().delete(&item.original)),
    };
    let still_there = match std::fs::symlink_metadata(&item.original) {
        Ok(m) => Identity::of_path(&item.original, &m, false).is_ok_and(|id| id.same_object(&item.identity)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(io_err("trash", &item.original)(e)),
    };
    match sent {
        Ok(()) if !still_there => Ok(Sent::Trashed(item)),
        Ok(()) => Err(io_err("trash", &item.original)(io::Error::other("the trash reported success but the item is still in place"))),
        Err(e) if still_there && platform::means_no_trash(&e) => Ok(Sent::NoTrash),
        Err(e) => Err(io_err("trash", &item.original)(io::Error::other(e))),
    }
}

/// Brings each of `items` back from the OS trash to its old path (one listing of the trash for
/// all of them), and says per item what happened, in order. [`Restore::Unsupported`] for every
/// item where the platform cannot list its trash (macOS). Never replaces anything at an old path.
/// Fault point [`points::TRASH_RESTORE`].
pub fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
    guard::check()?;
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let first = items.first().map_or_else(PathBuf::new, |t| t.original.clone());
    faults::at(points::TRASH_RESTORE, "restore", &first)?;
    if faults::stand_in_trash() {
        return stand_in::restore(items);
    }
    platform::restore(items)
}

/// The stand-in trash of a test build (module docs). Off a test build it takes nothing.
#[cfg(not(all(feature = "faults", debug_assertions)))]
mod stand_in {
    use super::{Restore, Sent, Trashed};
    use crate::Result;

    pub(super) fn takes(_item: &Trashed) -> bool {
        false
    }

    pub(super) fn send(_item: Trashed) -> Result<Sent> {
        Ok(Sent::NoTrash)
    }

    pub(super) fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
        super::platform::restore(items)
    }
}

/// The stand-in trash of a test build (module docs): `<temp>/dalica-test-trash/<pid>-<nanos>/`,
/// with a `lock` file this process holds locked and the items in `files/`, one numbered name each.
#[cfg(all(feature = "faults", debug_assertions))]
mod stand_in {
    use std::fs::{self, File, OpenOptions, TryLockError};
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{Restore, Sent, Trashed};
    use crate::atomic::Identity;
    use crate::{Result, io_err};

    /// One item in the stand-in trash (never read back where the platform cannot restore).
    #[cfg_attr(not(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android")))), allow(dead_code))]
    struct Kept {
        item: Trashed,
        at: PathBuf,
        /// When it was sent, in the order of sending.
        seq: u64,
    }

    struct Bin {
        files: PathBuf,
        volume: Identity,
        /// Held locked while the process lives; the next process removes the folder once it can
        /// take the lock.
        _lock: File,
        kept: Vec<Kept>,
        next: u64,
    }

    /// Every thread's items: engine tests send on worker threads and restore on the test's own.
    static BIN: Mutex<Option<Bin>> = Mutex::new(None);

    /// Folders younger than this are never removed without their lock (one may be being made).
    const GRACE: Duration = Duration::from_secs(60);

    fn bin() -> std::sync::MutexGuard<'static, Option<Bin>> {
        let mut bin = BIN.lock().unwrap_or_else(PoisonError::into_inner);
        if bin.is_none() {
            *bin = make().ok();
        }
        bin
    }

    /// This process's folder, after removing the folders of processes that have ended.
    fn make() -> io::Result<Bin> {
        let root = std::env::temp_dir().join("dalica-test-trash");
        fs::create_dir_all(&root)?;
        sweep(&root);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = root.join(format!("{}-{nanos}", std::process::id()));
        fs::create_dir(&dir)?;
        let lock = OpenOptions::new().read(true).write(true).create_new(true).open(dir.join("lock"))?;
        lock.try_lock().map_err(io::Error::other)?;
        let files = dir.join("files");
        fs::create_dir(&files)?;
        let volume = Identity::of_path(&files, &fs::symlink_metadata(&files)?, false)?;
        Ok(Bin { files, volume, _lock: lock, kept: Vec::new(), next: 0 })
    }

    /// Removes the folders under `root` whose process has ended (their lock can be taken).
    fn sweep(root: &Path) {
        let Ok(entries) = fs::read_dir(root) else { return };
        for entry in entries.flatten().take(10_000) {
            let dir = entry.path();
            let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > GRACE);
            let ended = match OpenOptions::new().read(true).write(true).open(dir.join("lock")) {
                Ok(f) => match f.try_lock() {
                    Ok(()) => true,
                    Err(TryLockError::WouldBlock) => false,
                    Err(TryLockError::Error(_)) => old,
                },
                Err(_) => old,
            };
            if ended {
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }

    /// Whether the stand-in takes `item`: it is on the stand-in folder's volume.
    pub(super) fn takes(item: &Trashed) -> bool {
        bin().as_ref().is_some_and(|b| b.volume.same_volume(&item.identity))
    }

    pub(super) fn send(item: Trashed) -> Result<Sent> {
        let mut guard = bin();
        let Some(bin) = guard.as_mut() else { return Err(io_err("trash", &item.original)(io::Error::other("no stand-in trash"))) };
        let at = bin.files.join(bin.next.to_string());
        fs::rename(&item.original, &at).map_err(io_err("trash", &item.original))?;
        bin.kept.push(Kept { item: item.clone(), at, seq: bin.next });
        bin.next = bin.next.saturating_add(1);
        Ok(Sent::Trashed(item))
    }

    /// Restores the items the stand-in holds (macOS: `Unsupported`, as its OS trash answers);
    /// items it does not hold (sent from another volume) go to the OS trash's restore, in one
    /// call, and every answer comes back in the order of `items`.
    pub(super) fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
        let mut guard = bin();
        let volume = guard.as_ref().map(|b| b.volume);
        let ours = |t: &Trashed| volume.is_some_and(|v| v.same_volume(&t.identity));
        let others: Vec<Trashed> = items.iter().filter(|t| !ours(t)).cloned().collect();
        let mut from_os = if others.is_empty() { Vec::new() } else { super::platform::restore(&others)? }.into_iter();
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            if !ours(item) {
                out.push(from_os.next().unwrap_or(Restore::NotInTrash));
                continue;
            }
            match guard.as_mut() {
                Some(bin) => out.push(restore_one(bin, item)?),
                None => out.push(Restore::NotInTrash),
            }
        }
        Ok(out)
    }

    #[cfg(not(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android")))))]
    fn restore_one(_bin: &mut Bin, _item: &Trashed) -> Result<Restore> {
        Ok(Restore::Unsupported)
    }

    /// One item, matched as the OS trash's restore matches it: its old path and its identity, or
    /// where the platform's trash loses identities (Windows), the newest of the same size.
    #[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
    fn restore_one(bin: &mut Bin, item: &Trashed) -> Result<Restore> {
        let key = super::comparable(&item.original);
        let candidates = || bin.kept.iter().enumerate().filter(|(_, k)| super::comparable(&k.item.original) == key);
        let pick = candidates().find(|(_, k)| k.item.identity.same_object(&item.identity)).map(|(i, _)| i).or_else(|| {
            if super::platform::IDENTITY_SURVIVES {
                return None;
            }
            let size = |k: &Kept| k.item.folder == item.folder && (item.folder || k.item.identity.size() == item.identity.size());
            candidates().filter(|(_, k)| size(k)).max_by_key(|(_, k)| k.seq).map(|(i, _)| i)
        });
        let Some(pick) = pick else { return Ok(Restore::NotInTrash) };
        match fs::symlink_metadata(&item.original) {
            Ok(_) => return Ok(Restore::Exists),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("restore", &item.original)(e)),
        }
        let Some(kept) = bin.kept.get(pick) else { return Ok(Restore::NotInTrash) };
        fs::rename(&kept.at, &item.original).map_err(io_err("restore", &item.original))?;
        bin.kept.remove(pick);
        Ok(Restore::Restored)
    }
}

/// The error of a `trash` crate call, or of a panic inside it, or of Dalica's own send of an item
/// whose name is too long for the crate's record.
#[derive(Debug)]
pub(crate) enum CrateError {
    Trash(::trash::Error),
    Panicked,
    #[cfg_attr(not(all(unix, not(target_vendor = "apple"), not(target_os = "android"))), allow(dead_code))]
    Own(Error),
}

impl std::fmt::Display for CrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrateError::Trash(e) => write!(f, "{e}"),
            CrateError::Panicked => f.write_str("the system trash library failed unexpectedly"),
            CrateError::Own(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CrateError {}

/// Runs a `trash` crate call, turning a panic inside it into an error.
fn catch<T>(call: impl FnOnce() -> std::result::Result<T, ::trash::Error>) -> std::result::Result<T, CrateError> {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(r) => r.map_err(CrateError::Trash),
        Err(_) => Err(CrateError::Panicked),
    }
}

/// `path` if it is absolute and ends in a normal name (not `..`, not a root).
fn checked(path: &Path) -> Result<PathBuf> {
    let refuse = |why: &str| io_err("trash", path)(io::Error::new(io::ErrorKind::InvalidInput, why.to_string()));
    if !path.is_absolute() {
        return Err(refuse("not an absolute path"));
    }
    match path.components().next_back() {
        Some(Component::Normal(_)) => Ok(path.to_path_buf()),
        _ => Err(refuse("not a file or folder name")),
    }
}

/// `path` with its folder resolved (links and `..` followed) and its own name kept, the form the
/// trash records.
fn resolved(path: &Path) -> Result<PathBuf> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io_err("trash", path)(io::Error::new(io::ErrorKind::InvalidInput, "not a file or folder name")));
    };
    Ok(std::fs::canonicalize(parent).map_err(io_err("trash", parent))?.join(name))
}

/// `path` in the form two spellings of one place compare equal in (Windows: without the verbatim
/// `\\?\` prefix the trash does not record, ignoring case).
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn comparable(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        let s = if let Some(unc) = s.strip_prefix(r"\\?\UNC\") { format!(r"\\{unc}") } else { s.strip_prefix(r"\\?\").unwrap_or(&s).to_string() };
        s.to_lowercase()
    } else {
        s.into_owned()
    }
}

/// Restores `items` from one listing of the trash (Linux and the BSDs, Windows).
///
/// A listed item stands for one of `items` when its name is the item's name (on Windows the shell
/// lists a display name, which leaves out the extension of a known file type while the system
/// hides those, and ignores case) and its folder is the item's folder (compared as recorded, then
/// resolved, so a short `RUNNER~1` name or another case still matches). Among those, the one with
/// the item's identity wins; where none can be looked at, or on Windows where the shell may not
/// say, the newest of the same size. It is restored under the item's own name, never the display
/// name.
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn restore_listed(items: &[Trashed]) -> Result<Vec<Restore>> {
    use ::trash::TrashItem;
    let first = items.first().map_or_else(PathBuf::new, |t| t.original.clone());
    let mut listed: Vec<TrashItem> = catch(::trash::os_limited::list).map_err(|e| io_err("list the trash", &first)(io::Error::other(e)))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let (Some(parent), Some(name)) = (item.original.parent(), item.original.file_name()) else {
            out.push(Restore::NotInTrash);
            continue;
        };
        let parent_key = comparable(parent);
        let candidates: Vec<usize> = listed
            .iter()
            .enumerate()
            .filter(|(_, t)| platform::name_matches(&t.name, name) && same_folder(&t.original_parent, &parent_key))
            .map(|(i, _)| i)
            .collect();
        let ids: Vec<(usize, Option<Identity>)> = candidates.iter().map(|&i| (i, listed.get(i).and_then(platform::in_trash_identity))).collect();
        let by_identity = ids.iter().find(|(_, id)| id.is_some_and(|id| id.same_object(&item.identity))).map(|&(i, _)| i);
        let pick = match by_identity {
            Some(i) => Some(i),
            None if !platform::IDENTITY_SURVIVES || ids.iter().all(|(_, id)| id.is_none()) => candidates
                .iter()
                .copied()
                .filter(|&i| listed.get(i).is_some_and(|t| size_fits(t, item)))
                .max_by_key(|&i| listed.get(i).map_or(i64::MIN, |t| t.time_deleted)),
            None => None,
        };
        let Some(pick) = pick.filter(|&i| i < listed.len()) else {
            out.push(Restore::NotInTrash);
            continue;
        };
        match std::fs::symlink_metadata(&item.original) {
            Ok(_) => {
                out.push(Restore::Exists);
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("restore", &item.original)(e)),
        }
        let mut chosen = listed.swap_remove(pick);
        // The item's own name: the listed one may be a display name without its extension.
        chosen.name = name.to_os_string();
        match catch(|| ::trash::os_limited::restore_all([chosen])) {
            Ok(()) => out.push(Restore::Restored),
            Err(CrateError::Trash(::trash::Error::RestoreCollision { .. })) => out.push(Restore::Exists),
            Err(e) => return Err(io_err("restore", &item.original)(io::Error::other(e))),
        }
    }
    Ok(out)
}

/// The folder a listed item came from is the one `key` ([`comparable`]) names: as recorded, or
/// once resolved (short names, case, links).
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn same_folder(listed: &Path, key: &str) -> bool {
    comparable(listed) == key || std::fs::canonicalize(listed).is_ok_and(|p| comparable(&p) == key)
}

/// Whether a listed item can be `item` by its size (a file of the same size, or a folder); an
/// item whose size cannot be read is not ruled out.
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn size_fits(listed: &::trash::TrashItem, item: &Trashed) -> bool {
    match catch(|| ::trash::os_limited::metadata(listed)) {
        Ok(m) => match m.size {
            ::trash::TrashItemSize::Bytes(n) => !item.folder && n == item.identity.size(),
            ::trash::TrashItemSize::Entries(_) => item.folder,
        },
        Err(_) => true,
    }
}

/// Linux and the BSDs: the freedesktop.org trash.
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
mod platform {
    use std::ffi::{OsStr, OsString};
    use std::fmt::Write as _;
    use std::fs::{self, DirBuilder, Metadata};
    use std::io::{self, Write as _};
    use std::os::unix::ffi::{OsStrExt, OsStringExt as _};
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
    use std::path::{Path, PathBuf};

    use super::{CrateError, Restore, Trashed};
    use crate::atomic::Identity;
    use crate::dirfd::{Folder, NoReplace};
    use crate::{Result, io_err};

    pub(super) fn context() -> ::trash::TrashContext {
        ::trash::TrashContext::default()
    }

    /// Whether a trash on the item's own volume takes it by a rename (see the module docs).
    pub(super) fn usable_trash(item: &Path, meta: &Metadata) -> Result<bool> {
        Ok(trash_folder(item, meta).is_some())
    }

    /// The trash on the item's own volume that takes it by a rename: the home trash on that
    /// volume, the volume's `.Trash/<uid>` or its `.Trash-<uid>` (made when missing). Never a
    /// trash on another volume, which would mean a copy and a delete.
    fn trash_folder(item: &Path, meta: &Metadata) -> Option<PathBuf> {
        let dev = meta.dev();
        if let Some(home) = home_trash()
            && nearest_existing(&home).is_some_and(|m| m.dev() == dev)
        {
            return Some(home);
        }
        let top = volume_top(item.parent()?, dev);
        let uid = rustix::process::getuid().as_raw().to_string();
        let shared = top.join(".Trash");
        if let Ok(m) = fs::symlink_metadata(&shared)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
            && fs::symlink_metadata(shared.join(&uid)).is_ok_and(|m| m.is_dir())
        {
            return Some(shared.join(&uid));
        }
        let own = top.join(format!(".Trash-{uid}"));
        match fs::symlink_metadata(&own) {
            Ok(m) => return (m.is_dir() && m.dev() == dev).then_some(own),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        let made = match DirBuilder::new().mode(0o700).create(&own) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fs::symlink_metadata(&own).is_ok_and(|m| m.is_dir() && m.dev() == dev),
            Err(_) => false,
        };
        made.then_some(own)
    }

    /// The longest name the `trash` crate's record takes: `info/<name>.trashinfo`, with a `.<n>`
    /// the crate adds when the name is taken, within the 255 bytes a name may have.
    pub(super) const LONGEST_RECORDED: usize = 240;
    /// How much of a long name its name in the trash keeps.
    const SHORT_STEM: usize = 200;
    /// The longest extension a shortened name keeps.
    const SHORT_EXTENSION: usize = 16;
    /// How many shortened names are tried before the send fails.
    const SHORT_TRIES: u32 = 1000;

    /// Sends an item whose name is too long for the crate's record (longer than
    /// [`LONGEST_RECORDED`] bytes) to the trash on its own volume, which the check before found;
    /// `None` for any other name (the crate sends it). See the module docs.
    pub(super) fn send_long_name(item: &Path, meta: &Metadata) -> Option<Result<()>> {
        let name = item.file_name()?;
        if name.len() <= LONGEST_RECORDED {
            return None;
        }
        Some(send_shortened(item, name, meta))
    }

    fn send_shortened(item: &Path, name: &OsStr, meta: &Metadata) -> Result<()> {
        let fail = |why: &str| io_err("trash", item)(io::Error::other(why.to_string()));
        let trash = trash_folder(item, meta).ok_or_else(|| fail("no trash on the item's volume"))?;
        let parent = item.parent().ok_or_else(|| fail("not a file or folder name"))?;
        let (files, info) = (trash.join("files"), trash.join("info"));
        for dir in [&files, &info] {
            DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(io_err("create folder", dir))?;
        }
        let (source, files, info) = (Folder::open(parent)?, Folder::open(&files)?, Folder::open(&info)?);
        let record = record(item, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string());
        for n in 1..=SHORT_TRIES {
            let short = shortened(name, n);
            let mut record_name = short.clone();
            record_name.push(".trashinfo");
            // The record is made first, exclusively: the name is this item's in the trash.
            let Some(mut file) = info.create_new(&record_name, true)? else { continue };
            let at = info.join(&record_name);
            let written = file.write_all(record.as_bytes()).and_then(|()| crate::faults::fsync(&at, || file.sync_all()));
            drop(file);
            if let Err(e) = written {
                let _ = info.remove(&record_name);
                return Err(io_err("write", &at)(e));
            }
            let moved = source.rename_noreplace(name, &files, &short);
            match moved {
                Ok(NoReplace::Done) => {
                    files.sync()?;
                    return Ok(());
                }
                Ok(NoReplace::Taken) => {
                    let _ = info.remove(&record_name);
                }
                Ok(NoReplace::Unsupported) => {
                    let _ = info.remove(&record_name);
                    return Err(fail("this volume cannot move the item into its trash without risking another item there"));
                }
                Err(e) => {
                    let _ = info.remove(&record_name);
                    return Err(e);
                }
            }
        }
        Err(fail("no free name in the trash"))
    }

    /// The item's name in the trash: its first [`SHORT_STEM`] bytes (whole characters when the name
    /// is UTF-8), `~<n>`, and its extension when that is short.
    pub(super) fn shortened(name: &OsStr, n: u32) -> OsString {
        let bytes = name.as_bytes();
        let mut cut = SHORT_STEM.min(bytes.len());
        if let Ok(text) = std::str::from_utf8(bytes) {
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
        }
        let mut out = bytes.get(..cut).unwrap_or_default().to_vec();
        out.extend_from_slice(format!("~{n}").as_bytes());
        if let Some(ext) = Path::new(name).extension().map(OsStrExt::as_bytes).filter(|e| e.len() <= SHORT_EXTENSION) {
            out.push(b'.');
            out.extend_from_slice(ext);
        }
        OsString::from_vec(out)
    }

    /// The freedesktop.org record of `item` (absolute) sent at `when` (local time,
    /// `YYYY-MM-DDThh:mm:ss`): its path with every byte but letters, digits, `-._~` and `/`
    /// percent-encoded.
    pub(super) fn record(item: &Path, when: &str) -> String {
        let mut path = String::new();
        for &b in item.as_os_str().as_bytes() {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                path.push(char::from(b));
            } else {
                let _ = write!(path, "%{b:02X}");
            }
        }
        format!("[Trash Info]\nPath={path}\nDeletionDate={when}\n")
    }

    /// The home trash, as the freedesktop.org specification places it.
    fn home_trash() -> Option<PathBuf> {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(data).join("Trash"));
        }
        std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".local/share/Trash"))
    }

    /// The metadata of `path`, or of its nearest folder that exists.
    fn nearest_existing(path: &Path) -> Option<Metadata> {
        path.ancestors().take(64).find_map(|p| fs::metadata(p).ok())
    }

    /// The highest folder above `dir` (itself included) still on device `dev`: the volume's top
    /// folder.
    fn volume_top(dir: &Path, dev: u64) -> PathBuf {
        let mut top = dir.to_path_buf();
        for up in dir.ancestors().skip(1).take(256) {
            match fs::metadata(up) {
                Ok(m) if m.dev() == dev => top = up.to_path_buf(),
                _ => break,
            }
        }
        top
    }

    /// The crate's errors after the pre-check found a trash: none of them means "no trash".
    pub(super) fn means_no_trash(_e: &CrateError) -> bool {
        false
    }

    /// The trashed object is renamed into `files/`, so its identity is the item's.
    pub(super) const IDENTITY_SURVIVES: bool = true;

    /// The trash records the item's own name.
    pub(super) fn name_matches(listed: &std::ffi::OsStr, name: &std::ffi::OsStr) -> bool {
        listed == name
    }

    /// The identity of the trashed object a listed item stands for: `<trash>/files/<name>` for
    /// `<trash>/info/<name>.trashinfo`.
    pub(super) fn in_trash_identity(item: &::trash::TrashItem) -> Option<Identity> {
        let info = Path::new(&item.id);
        let file = info.parent()?.parent()?.join("files").join(info.file_stem()?);
        fs::symlink_metadata(&file).ok().map(|m| Identity::of(&m))
    }

    pub(super) fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
        super::restore_listed(items)
    }
}

/// Windows: the Recycle Bin through the shell.
#[cfg(windows)]
mod platform {
    use std::fs::Metadata;
    use std::path::Path;

    use super::{CrateError, Restore, Trashed};
    use crate::Result;
    use crate::atomic::Identity;

    pub(super) fn context() -> ::trash::TrashContext {
        ::trash::TrashContext::default()
    }

    /// The shell decides, and aborts rather than deleting permanently.
    pub(super) fn usable_trash(_item: &Path, _meta: &Metadata) -> Result<bool> {
        Ok(true)
    }

    /// The shell takes every name the volume allows.
    pub(super) fn send_long_name(_item: &Path, _meta: &Metadata) -> Option<Result<()>> {
        None
    }

    /// The shell aborts the operation (it would have to delete permanently, which the crate asks
    /// it to warn about, and no warning can be shown).
    pub(super) fn means_no_trash(e: &CrateError) -> bool {
        matches!(e, CrateError::Trash(::trash::Error::Unknown { description }) if description.contains("aborted"))
    }

    /// The shell's item id need not be a file system path, so an identity that cannot be matched
    /// does not rule an item out.
    pub(super) const IDENTITY_SURVIVES: bool = false;

    /// The shell lists a display name: case ignored, and without the extension of a known file
    /// type while the system hides those.
    pub(super) fn name_matches(listed: &std::ffi::OsStr, name: &std::ffi::OsStr) -> bool {
        let (listed, name) = (listed.to_string_lossy().to_lowercase(), name.to_string_lossy().to_lowercase());
        listed == name || Path::new(&name).file_stem().is_some_and(|stem| stem.to_string_lossy() == listed)
    }

    /// The Recycle Bin's own file for a listed item, when its id is a file system path.
    pub(super) fn in_trash_identity(item: &::trash::TrashItem) -> Option<Identity> {
        let path = Path::new(&item.id);
        let meta = std::fs::symlink_metadata(path).ok()?;
        Identity::of_path(path, &meta, false).ok()
    }

    pub(super) fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
        super::restore_listed(items)
    }
}

/// macOS (and other systems whose trash cannot be listed): `NSFileManager`, no restore.
#[cfg(not(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android")))))]
mod platform {
    use std::fs::Metadata;
    use std::path::Path;

    use super::{CrateError, Restore, Trashed};
    use crate::Result;

    pub(super) fn context() -> ::trash::TrashContext {
        #[allow(unused_mut)]
        let mut context = ::trash::TrashContext::default();
        #[cfg(target_os = "macos")]
        {
            use ::trash::macos::{DeleteMethod, TrashContextExtMacos as _};
            context.set_delete_method(DeleteMethod::NsFileManager);
        }
        context
    }

    pub(super) fn usable_trash(_item: &Path, _meta: &Metadata) -> Result<bool> {
        Ok(true)
    }

    /// `NSFileManager` takes every name the volume allows.
    pub(super) fn send_long_name(_item: &Path, _meta: &Metadata) -> Option<Result<()>> {
        None
    }

    /// `NSFileManager` answers `NSFeatureUnsupportedError` (3328) for a volume without a trash.
    pub(super) fn means_no_trash(e: &CrateError) -> bool {
        matches!(e, CrateError::Trash(::trash::Error::Unknown { description }) if description.contains("3328"))
    }

    pub(super) fn restore(items: &[Trashed]) -> Result<Vec<Restore>> {
        Ok(vec![Restore::Unsupported; items.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_absolute_paths_ending_in_a_name_are_taken() {
        for bad in ["relative.jpg", "/", "/tmp/.."] {
            assert!(matches!(checked(Path::new(bad)), Err(Error::Io { .. })), "{bad}");
        }
        assert!(checked(&std::env::temp_dir().join("x.jpg")).is_ok());
    }

    #[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
    #[test]
    fn comparable_paths_drop_the_verbatim_prefix_on_windows_only() {
        let p = Path::new(r"\\?\C:\Photos\A.jpg");
        if cfg!(windows) {
            assert_eq!(comparable(p), r"c:\photos\a.jpg");
            assert_eq!(comparable(Path::new(r"\\?\UNC\host\share\x")), r"\\host\share\x");
        } else {
            assert_eq!(comparable(p), r"\\?\C:\Photos\A.jpg");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_shell_display_name_matches_with_or_without_its_extension() {
        use std::ffi::OsStr;
        assert!(platform::name_matches(OsStr::new("photo"), OsStr::new("Photo.jpg")));
        assert!(platform::name_matches(OsStr::new("PHOTO.JPG"), OsStr::new("photo.jpg")));
        assert!(platform::name_matches(OsStr::new("Trip"), OsStr::new("Trip")));
        assert!(!platform::name_matches(OsStr::new("other"), OsStr::new("photo.jpg")));
    }

    #[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
    #[test]
    fn a_freedesktop_name_matches_exactly() {
        use std::ffi::OsStr;
        assert!(platform::name_matches(OsStr::new("photo.jpg"), OsStr::new("photo.jpg")));
        assert!(!platform::name_matches(OsStr::new("photo"), OsStr::new("photo.jpg")));
        assert!(!platform::name_matches(OsStr::new("Photo.jpg"), OsStr::new("photo.jpg")));
    }

    /// A long name's name in the trash keeps its start (whole characters) and its extension, numbered;
    /// its record holds the whole path, percent-encoded.
    #[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
    #[test]
    fn a_long_name_is_shortened_in_the_trash_and_recorded_whole() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;
        // 'é' is two bytes: 199 'a's put one across the 200-byte cut, which moves before it.
        let name = format!("{}é{}.jpeg", "a".repeat(199), "b".repeat(60));
        let short = platform::shortened(OsStr::new(&name), 3);
        assert_eq!(short, OsStr::new(&format!("{}~3.jpeg", "a".repeat(199))));
        // A name that is not UTF-8 is cut at 200 bytes; a long extension is left out.
        let raw: Vec<u8> = [vec![0xff_u8; 250], b".".to_vec(), vec![b'x'; 20]].concat();
        let short = platform::shortened(OsStr::from_bytes(&raw), 1);
        assert_eq!(short.as_bytes(), [vec![0xff_u8; 200], b"~1".to_vec()].concat());
        assert!(platform::shortened(OsStr::new(&"x".repeat(255)), 999).len() + ".trashinfo".len() < 255);
        let record = platform::record(Path::new("/photos/a b%=é.jpg"), "2026-10-15T09:30:00");
        assert_eq!(record, "[Trash Info]\nPath=/photos/a%20b%25%3D%C3%A9.jpg\nDeletionDate=2026-10-15T09:30:00\n");
    }

    #[test]
    fn a_panic_in_the_trash_library_is_an_error() {
        let r: std::result::Result<(), CrateError> = catch(|| -> std::result::Result<(), ::trash::Error> { panic!("boom") });
        assert!(matches!(r, Err(CrateError::Panicked)));
    }
}
