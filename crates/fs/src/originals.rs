//! The originals backup (`data-model.md` §6, `security.md` §5.3, §5.6): on a photo's first Save
//! its original moves, byte for byte and by one rename, into `<folder>/.dalica-originals/<name>`;
//! Undo Save and Revert move it back over the saved file.
//!
//! - The backup folder is opened, or made, relative to the photo's folder held once (`dirfd`), and
//!   never through a link: a link (or a file) in place of `.dalica-originals` is refused with
//!   nothing moved.
//! - [`back_up`] re-checks the photo's identity right before the move (§5.3) and moves it with a
//!   rename that never replaces, so the *first* original is the one kept: when a backup of that
//!   name already exists it must be the recorded one (size and digest, [`Recorded`]), and then
//!   nothing moves ([`Backup::AlreadyKept`]); any other backup is refused.
//! - [`restore`] moves the backup back only when it is still the recorded one and the saved file
//!   is still the version the caller expects (`Expect::Unchanged`; `Expect::Absent` when a Save
//!   stopped between the two steps and no saved file was written).
//!
//! A rename keeps the file's identity, so at every point, crashes included, the original's bytes
//! are under exactly one of the two names. The digest is the caller's (the originals record's
//! owner): this crate hands it the bytes to check and keeps no hash of its own.

use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use crate::atomic::{Expect, Identity};
use crate::create::plain_name;
use crate::dirfd::{Folder, Kind, NoReplace};
use crate::faults::{self, points};
use crate::{Error, Result, guard, io_err};

/// The backup folder's name, inside each photo folder.
pub const DIR_NAME: &str = ".dalica-originals";

/// What the originals record says a kept original is.
#[derive(Clone, Copy)]
pub struct Recorded<'a> {
    /// Its size in bytes.
    pub size: u64,
    /// Reads the backup's bytes (exactly `size` of them, from the start) and says whether they
    /// have the recorded digest. Only called on a regular file of the recorded size.
    pub digest_matches: &'a dyn Fn(&mut dyn io::Read) -> io::Result<bool>,
}

impl std::fmt::Debug for Recorded<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorded").field("size", &self.size).finish_non_exhaustive()
    }
}

/// What [`back_up`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backup {
    /// The photo moved into the backup; the backup's identity (the photo's, unchanged).
    Moved(Identity),
    /// The recorded backup was already there (a photo saved before); nothing moved.
    AlreadyKept,
}

/// Opens or makes `<folder>/.dalica-originals/`, never through a link, and returns its path (where
/// the originals record is written).
pub fn ensure_dir(folder: &Path) -> Result<PathBuf> {
    guard::check()?;
    let dir = Folder::open(folder)?;
    let backup = open_backup_dir(&dir, true)?.ok_or_else(|| Error::Changed(dir.join(OsStr::new(DIR_NAME))))?;
    Ok(backup.path().to_path_buf())
}

/// Moves the photo `name` in `folder`, which must still have the identity `photo`, into
/// `<folder>/.dalica-originals/<name>`, unless the backup `kept` describes is already there.
///
/// `kept` is the originals record's entry for `name`: `None` when nothing is kept yet (the backup
/// must then not exist, and the folder is made if needed), `Some` for a photo saved before (the
/// backup must exist and match it; nothing moves). Refused with nothing moved: a name that is not
/// one plain component; a photo that changed, vanished or is not a regular file
/// ([`Error::Changed`]); a link in place of the photo or the backup folder ([`Error::Link`]); a
/// backup that exists when none is recorded, or that is missing or does not match the record
/// ([`Error::Changed`] naming the backup). Fault points `faults::BACKUP_POINTS`.
pub fn back_up(folder: &Path, name: &str, photo: Identity, kept: Option<&Recorded<'_>>) -> Result<Backup> {
    guard::check()?;
    let os = checked_name(folder, name)?;
    let dir = Folder::open(folder)?;
    faults::at(points::BACKUP_OPEN_DIR, "open", &dir.join(OsStr::new(DIR_NAME)))?;
    let Some(backups) = open_backup_dir(&dir, kept.is_none())? else {
        // A record without its backup folder: the backup is missing.
        return Err(Error::Changed(dir.join(OsStr::new(DIR_NAME)).join(os)));
    };
    let backup_path = backups.join(os);
    match (kept, backups.entry(os)?) {
        (_, Some(e)) if e.kind == Kind::Link => return Err(Error::Link(backup_path)),
        (Some(rec), Some(_)) => {
            check_backup(&backups, os, rec)?;
            return Ok(Backup::AlreadyKept);
        }
        (None, None) => {}
        // A backup nobody recorded, or a recorded one that is gone: the first original is the one
        // kept, so neither is touched, and the photo stays where it is.
        (None, Some(_)) | (Some(_), None) => return Err(Error::Changed(backup_path)),
    }

    // The photo must be the version the caller read, right before it moves (§5.3).
    faults::at(points::BACKUP_CHECK, "stat", &dir.join(os))?;
    check_file(&dir, os, Expect::Unchanged(photo))?;
    faults::at(points::BACKUP_MOVE, "rename", &backup_path)?;
    move_noreplace(&dir, os, &backups, os)?;
    faults::at(points::BACKUP_FSYNC_DIR, "fsync", backups.path())?;
    backups.sync()?;
    dir.sync()?;
    faults::at(points::BACKUP_VERIFY, "verify", &backup_path)?;
    match backups.entry(os)? {
        Some(e) if e.kind == Kind::File && e.identity == photo => Ok(Backup::Moved(e.identity)),
        _ => Err(Error::VerifyFailed(backup_path)),
    }
}

/// Moves the backup of `name` back from `<folder>/.dalica-originals/` to `<folder>/<name>`, over
/// the saved file, and returns the restored photo's identity.
///
/// Refused with nothing moved: a name that is not one plain component; a backup that is missing, a
/// link, or not the recorded one (`kept`); a saved file that is not what `saved` expects (the
/// version the caller recorded, or absent when a Save stopped before writing it). The saved file
/// is replaced, not sent to the trash: it can always be rendered again from the original and its
/// recipe. Fault points `faults::RESTORE_POINTS`.
pub fn restore(folder: &Path, name: &str, saved: Expect, kept: &Recorded<'_>) -> Result<Identity> {
    guard::check()?;
    let os = checked_name(folder, name)?;
    let dir = Folder::open(folder)?;
    let Some(backups) = open_backup_dir(&dir, false)? else {
        return Err(Error::Changed(dir.join(OsStr::new(DIR_NAME)).join(os)));
    };
    let backup_path = backups.join(os);
    let original = check_backup(&backups, os, kept)?;

    // Right before the move: the saved file is what the caller expects, the backup unchanged.
    faults::at(points::RESTORE_CHECK, "stat", &dir.join(os))?;
    check_file(&dir, os, saved)?;
    check_file(&backups, os, Expect::Unchanged(original))?;
    faults::at(points::RESTORE_MOVE, "rename", &dir.join(os))?;
    match saved {
        Expect::Absent => move_noreplace(&backups, os, &dir, os)?,
        Expect::Unchanged(_) => backups.rename_to(os, &dir, os)?,
    }
    faults::at(points::RESTORE_FSYNC_DIR, "fsync", dir.path())?;
    dir.sync()?;
    backups.sync()?;
    faults::at(points::RESTORE_VERIFY, "verify", &dir.join(os))?;
    match dir.entry(os)? {
        Some(e) if e.kind == Kind::File && e.identity == original => Ok(original),
        _ => Err(Error::VerifyFailed(backup_path)),
    }
}

/// `name` as a component, or the refusal of a name that is not one plain component.
fn checked_name<'a>(folder: &Path, name: &'a str) -> Result<&'a OsStr> {
    if plain_name(name) {
        Ok(OsStr::new(name))
    } else {
        Err(io_err("back up", folder)(io::Error::new(io::ErrorKind::InvalidInput, format!("not a plain file name: {name:?}"))))
    }
}

/// The backup folder in `dir`, opened never through a link; made (and the making made durable)
/// when missing and `create`, else `None` when missing.
fn open_backup_dir(dir: &Folder, create: bool) -> Result<Option<Folder>> {
    let name = OsStr::new(DIR_NAME);
    if let Some(backups) = dir.open_dir(name)? {
        return Ok(Some(backups));
    }
    if !create {
        return Ok(None);
    }
    let made = dir.make_dir(name)?;
    // Taken by something else since the open (a file or a link is refused here).
    let backups = dir.open_dir(name)?.ok_or_else(|| Error::Changed(dir.join(name)))?;
    if made {
        dir.sync()?;
    }
    Ok(Some(backups))
}

/// The regular file `name` in `dir` matches `expect` (never through a link).
fn check_file(dir: &Folder, name: &OsStr, expect: Expect) -> Result<()> {
    let path = dir.join(name);
    match (dir.entry(name)?, expect) {
        (Some(e), _) if e.kind == Kind::Link => Err(Error::Link(path)),
        (None, Expect::Absent) => Ok(()),
        (Some(e), Expect::Unchanged(id)) if e.kind == Kind::File && e.identity == id => Ok(()),
        _ => Err(Error::Changed(path)),
    }
}

/// The backup `name` is a regular file of the recorded size and digest, unchanged while it was
/// read; its identity.
fn check_backup(backups: &Folder, name: &OsStr, kept: &Recorded<'_>) -> Result<Identity> {
    let path = backups.join(name);
    let entry = match backups.entry(name)? {
        Some(e) if e.kind == Kind::Link => return Err(Error::Link(path)),
        Some(e) if e.kind == Kind::File && e.identity.size() == kept.size => e,
        _ => return Err(Error::Changed(path)),
    };
    let file = backups.open_read(name)?;
    let opened = Identity::of_file(&file).map_err(io_err("stat", &path))?;
    if opened != entry.identity {
        return Err(Error::Changed(path));
    }
    let mut bytes = file.take(kept.size);
    if !(kept.digest_matches)(&mut bytes).map_err(io_err("read", &path))? {
        return Err(Error::Changed(path));
    }
    check_file(backups, name, Expect::Unchanged(entry.identity))?;
    Ok(entry.identity)
}

/// Moves `from` in `src` to `to` in `dest` only if `to` is free ([`Error::Changed`] naming it
/// otherwise). On a volume that can do neither a non-replacing rename nor a hard link, the free
/// name is checked right before a plain rename: one process per library (the library lock) keeps
/// other Dalica processes out of that gap.
fn move_noreplace(src: &Folder, from: &OsStr, dest: &Folder, to: &OsStr) -> Result<()> {
    match src.rename_noreplace(from, dest, to)? {
        NoReplace::Done => Ok(()),
        NoReplace::Taken => Err(Error::Changed(dest.join(to))),
        NoReplace::Unsupported => {
            if dest.entry(to)?.is_some() {
                return Err(Error::Changed(dest.join(to)));
            }
            src.rename_to(from, dest, to)
        }
    }
}
