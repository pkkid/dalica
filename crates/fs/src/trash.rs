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
    if !platform::usable_trash(&item.original, &meta)? {
        return Ok(Sent::NoTrash);
    }
    faults::at(points::TRASH_SEND, "trash", &item.original)?;
    let sent = catch(|| platform::context().delete(&item.original));
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
    platform::restore(items)
}

/// The error of a `trash` crate call, or of a panic inside it.
#[derive(Debug)]
pub(crate) enum CrateError {
    Trash(::trash::Error),
    Panicked,
}

impl std::fmt::Display for CrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrateError::Trash(e) => write!(f, "{e}"),
            CrateError::Panicked => f.write_str("the system trash library failed unexpectedly"),
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
#[cfg(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))))]
fn restore_listed(items: &[Trashed]) -> Result<Vec<Restore>> {
    use ::trash::TrashItem;
    let first = items.first().map_or_else(PathBuf::new, |t| t.original.clone());
    let mut listed: Vec<TrashItem> = catch(::trash::os_limited::list).map_err(|e| io_err("list the trash", &first)(io::Error::other(e)))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let want = comparable(&item.original);
        let same_path: Vec<usize> = listed.iter().enumerate().filter(|(_, t)| comparable(&t.original_path()) == want).map(|(i, _)| i).collect();
        // The one with the item's identity; where no candidate can be looked at (a shell item
        // without a file path), the newest of that path.
        let ids: Vec<(usize, Option<Identity>)> = same_path.iter().map(|&i| (i, listed.get(i).and_then(platform::in_trash_identity))).collect();
        let pick = match ids.iter().find(|(_, id)| id.is_some_and(|id| id.same_object(&item.identity))) {
            Some(&(i, _)) => Some(i),
            None if ids.iter().all(|(_, id)| id.is_none()) => {
                same_path.iter().copied().max_by_key(|&i| listed.get(i).map_or(i64::MIN, |t| t.time_deleted))
            }
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
        let chosen = listed.swap_remove(pick);
        match catch(|| ::trash::os_limited::restore_all([chosen])) {
            Ok(()) => out.push(Restore::Restored),
            Err(CrateError::Trash(::trash::Error::RestoreCollision { .. })) => out.push(Restore::Exists),
            Err(e) => return Err(io_err("restore", &item.original)(io::Error::other(e))),
        }
    }
    Ok(out)
}

/// Linux and the BSDs: the freedesktop.org trash.
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
mod platform {
    use std::fs::{self, DirBuilder, Metadata};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
    use std::path::{Path, PathBuf};

    use super::{CrateError, Restore, Trashed};
    use crate::Result;
    use crate::atomic::Identity;

    pub(super) fn context() -> ::trash::TrashContext {
        ::trash::TrashContext::default()
    }

    /// Whether a trash on the item's own volume takes it by a rename (see the module docs): the
    /// home trash on that volume, the volume's `.Trash/<uid>` or its `.Trash-<uid>` (made when
    /// missing). Never a trash on another volume, which would mean a copy and a delete.
    pub(super) fn usable_trash(item: &Path, meta: &Metadata) -> Result<bool> {
        let dev = meta.dev();
        if home_trash().and_then(|h| nearest_existing(&h)).is_some_and(|m| m.dev() == dev) {
            return Ok(true);
        }
        let Some(top) = item.parent().map(|p| volume_top(p, dev)) else { return Ok(false) };
        let uid = rustix::process::getuid().as_raw().to_string();
        let shared = top.join(".Trash");
        if let Ok(m) = fs::symlink_metadata(&shared)
            && m.is_dir()
            && m.permissions().mode() & 0o1000 != 0
            && fs::symlink_metadata(shared.join(&uid)).is_ok_and(|m| m.is_dir())
        {
            return Ok(true);
        }
        let own = top.join(format!(".Trash-{uid}"));
        match fs::symlink_metadata(&own) {
            Ok(m) => return Ok(m.is_dir() && m.dev() == dev),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Ok(false),
        }
        Ok(match DirBuilder::new().mode(0o700).create(&own) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fs::symlink_metadata(&own).is_ok_and(|m| m.is_dir() && m.dev() == dev),
            Err(_) => false,
        })
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

    /// The shell aborts the operation (it would have to delete permanently, which the crate asks
    /// it to warn about, and no warning can be shown).
    pub(super) fn means_no_trash(e: &CrateError) -> bool {
        matches!(e, CrateError::Trash(::trash::Error::Unknown { description }) if description.contains("aborted"))
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

    #[test]
    fn a_panic_in_the_trash_library_is_an_error() {
        let r: std::result::Result<(), CrateError> = catch(|| -> std::result::Result<(), ::trash::Error> { panic!("boom") });
        assert!(matches!(r, Err(CrateError::Panicked)));
    }
}
