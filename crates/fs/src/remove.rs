//! Removing a source file after its import ("Delete copied files from the card", `organizing.md` §3,
//! `security.md` §5.7): the only place Dalica may delete a file permanently.
//!
//! [`remove_copied`] removes one file that an import copied and verified, and only while both
//! still stand: the source still has the identity (object, size, modification time) recorded when
//! it was copied, and its copy still has the identity [`crate::transfer::import_copy`] returned.
//! A source changed since its copy, or whose copy changed or went, stays where it is
//! ([`Error::Changed`]). The file goes to the OS trash when its volume has one
//! ([`Removed::Trashed`], through [`crate::trash::send`]); a volume without a trash (most cards)
//! gets it deleted permanently ([`Removed::Deleted`]). An error from the trash is never turned into
//! a delete: only the trash's own answer that the volume has none is.
//!
//! **Refused with nothing removed:** a path outside the given `root` (the card's or drive's
//! folder the import listed), a path with a component that is not one plain name (`..`, `.`, a
//! separator inside a name, Dalica's own names), a link in place of the root, of any folder
//! between it and the file, or of the file itself ([`Error::Link`]: never followed, `security.md`
//! §5.6), and a folder in place of the file. The checks run on the held folder right before the
//! file goes; a file swapped into its name in the instant between that check and the trash's own
//! look is answered [`Error::Changed`] (it is then in the trash, never deleted).

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::atomic::Identity;
use crate::dirfd::{Folder, Kind};
use crate::faults::{self, points};
use crate::transfer::{existing_name, not_a_link};
use crate::trash::{self, Sent, Trashed};
use crate::{Error, Result, guard, io_err};

/// What [`remove_copied`] did with a source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removed {
    /// The file is in the OS trash of its volume.
    Trashed(Trashed),
    /// Its volume has no trash: the file was deleted permanently.
    Deleted,
}

/// Removes the source file `source`, inside the folder `root`, which must still have the identity
/// `expect` recorded when it was copied, while its verified copy `copy` still has the identity
/// `copy_identity` (see the module docs). Fault points `faults::REMOVE_POINTS`; on a volume without
/// a trash (the [`points::TRASH_NO_TRASH`] switch in tests) `faults::REMOVE_DELETE_POINTS`.
pub fn remove_copied(root: &Path, source: &Path, expect: Identity, copy: &Path, copy_identity: Identity) -> Result<Removed> {
    guard::check()?;
    let (parent, name) = inside(root, source)?;
    let name = OsStr::new(&name);

    faults::at(points::REMOVE_OPEN_DIR, "open", &parent)?;
    let dir = Folder::open(&parent)?;
    // A folder swapped for a link before it was opened was followed by the opening: refused here.
    not_a_link(&parent)?;

    faults::at(points::REMOVE_CHECK, "stat", &dir.join(name))?;
    still_copied(&dir, name, expect, copy, copy_identity)?;
    let path = dir.join(name);
    match trash::send(&path)? {
        Sent::Trashed(t) if t.identity == expect => return Ok(Removed::Trashed(t)),
        Sent::Trashed(t) => return Err(Error::Changed(t.original)),
        Sent::NoTrash => {}
    }

    // The volume has no trash: the one permanent delete, after one more look.
    still_copied(&dir, name, expect, copy, copy_identity)?;
    faults::at(points::REMOVE_DELETE, "remove", &path)?;
    dir.remove(name)?;
    faults::at(points::REMOVE_FSYNC_DIR, "fsync", dir.path())?;
    dir.sync()?;
    faults::at(points::REMOVE_VERIFY, "verify", &path)?;
    match dir.entry(name)? {
        Some(e) if e.identity.same_object(&expect) => Err(Error::VerifyFailed(path)),
        _ => Ok(Removed::Deleted),
    }
}

/// The source `name` in `dir` is the regular file `expect` and the copy is still `copy_identity`.
fn still_copied(dir: &Folder, name: &OsStr, expect: Identity, copy: &Path, copy_identity: Identity) -> Result<()> {
    match dir.entry(name)? {
        Some(e) if e.kind == Kind::Link => return Err(Error::Link(dir.join(name))),
        Some(e) if e.kind == Kind::Dir => return Err(refuse(&dir.join(name), "a folder is never removed this way")),
        Some(e) if e.kind == Kind::File && e.identity == expect => {}
        Some(_) => return Err(Error::Changed(dir.join(name))),
        None => return Err(io_err("stat", &dir.join(name))(io::ErrorKind::NotFound.into())),
    }
    match crate::atomic::identity(copy) {
        Ok(Some(id)) if id == copy_identity => Ok(()),
        Ok(_) | Err(Error::Link(_)) => Err(Error::Changed(copy.to_path_buf())),
        Err(e) => Err(e),
    }
}

/// The folder holding `source` and its name, when `source` lies inside `root` by plain names
/// only, with no link in place of the root or of any folder between them (module docs).
fn inside(root: &Path, source: &Path) -> Result<(PathBuf, String)> {
    // Before `absolute`, which on Windows resolves `..` by the letters (`a\b\..\c` is `a\c`) and
    // would let a path that names a step up through it pass as plain names; Unix keeps them.
    for path in [root, source] {
        if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
            return Err(refuse(path, "a `..` or `.` in the path"));
        }
    }
    let root_abs = std::path::absolute(root).map_err(io_err("remove", root))?;
    let source_abs = std::path::absolute(source).map_err(io_err("remove", source))?;
    let Ok(rel) = source_abs.strip_prefix(&root_abs) else {
        return Err(refuse(source, "outside the folder being imported from"));
    };
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(n) => match n.to_str() {
                Some(n) if existing_name(n) => parts.push(n),
                _ => return Err(refuse(source, "not a plain file name")),
            },
            _ => return Err(refuse(source, "not a plain file name")),
        }
    }
    let Some((name, folders)) = parts.split_last() else {
        return Err(refuse(source, "not a file inside the folder being imported from"));
    };
    not_a_link(&root_abs)?;
    if !std::fs::symlink_metadata(&root_abs).map_err(io_err("stat", &root_abs))?.is_dir() {
        return Err(refuse(&root_abs, "not a folder"));
    }
    let mut parent = root_abs;
    for f in folders {
        parent.push(f);
        let meta = std::fs::symlink_metadata(&parent).map_err(io_err("stat", &parent))?;
        if meta.file_type().is_symlink() {
            return Err(Error::Link(parent));
        }
        if !meta.is_dir() {
            return Err(refuse(&parent, "not a folder"));
        }
    }
    Ok((parent, (*name).to_owned()))
}

fn refuse(path: &Path, why: &str) -> Error {
    io_err("remove", path)(io::Error::new(io::ErrorKind::InvalidInput, why.to_owned()))
}
