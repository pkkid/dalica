//! Moves, renames and copies that never replace a file (`security.md` §5.3, §5.7, `data-model.md`
//! §8): a photo moved to another folder or renamed in its own, a photo copied, a folder made,
//! renamed or moved.
//!
//! Adapted from LightCraft `crates/engine/src/import_move.rs` @ b7091805 (MIT OR Apache-2.0): the two
//! phases of a move that has to copy, `place` (the copy is put in place and verified without
//! touching the source) and `finish` (only then the source goes, after checking once more that the
//! copy is still there), and the byte-for-byte comparison (`same_bytes`, `fill`). Here the copy is
//! committed by a rename that never replaces, every step is relative to held folders, and the
//! source goes to the OS trash instead of being removed.
//!
//! **One volume.** [`move_file`] re-checks the source's identity right before one rename that never
//! replaces (`renameat2(RENAME_NOREPLACE)` on Linux and Android, `renameatx_np(RENAME_EXCL)` on
//! Apple systems, a hard link then an unlink elsewhere, Windows included). A volume with neither
//! (FAT, exFAT, some shares) gets the free name checked right before a plain rename; one process
//! per library (the library lock) keeps other Dalica processes out of that gap. A taken name is
//! [`Outcome::Exists`], with nothing changed, for the caller to rename or skip; that holds for a
//! file created between the check and the move too, since the rename itself refuses it. A rename
//! that only changes the case of a name (on a volume that ignores case) goes through a temporary
//! name.
//!
//! **Across volumes** (the rename answers `EXDEV`, or the [`points::MOVE_CROSS_VOLUME`] switch
//! forces it): the first phase, [`copy_file`], writes a temporary file in the destination, `fsync`s
//! it, keeps the source's modification time (and permissions on Unix), compares it byte for byte with the
//! source, then gives it the name by the same rename that never replaces. The second phase,
//! [`finish_move`], checks that the source is unchanged and the copy is still the verified one,
//! then sends the source to the OS trash ([`crate::trash`]): never before the copy verifies, never
//! a permanent delete. A source whose volume has no trash stays where it is
//! ([`Finished::NoTrash`]): the move became a copy, and the caller says so.
//!
//! **Imports** (`organizing.md` §3, `security.md` §5.7). [`import_copy`] is the copy phase for a
//! file from a card, a drive or a folder: the same steps as [`copy_file`] (a new temporary file,
//! `fsync`, the source's modification time, a byte-for-byte check, a commit that never replaces;
//! [`Imported::Exists`] on a clash for the caller to rename), with the bytes written counted in a
//! shared [`Progress`] as they go (for a speed read-out) and a stop that takes effect between
//! files: a file already started is finished, the next one answers [`Imported::Stopped`] with
//! nothing written. The source is never changed; removing it later is [`crate::remove`]'s.
//!
//! **Folders.** [`new_folder`] makes a folder whose name must be free. [`move_folder`] renames or
//! moves a folder in one step that never replaces one; across volumes it copies the tree into a
//! temporary folder beside the target (at most [`MAX_DEPTH`] levels and [`MAX_ENTRIES`] entries;
//! a link or a special file inside refuses the move), verifying each file byte for byte, gives the
//! copy its name, then sends the source tree to the trash. A failed copy removes only what it made.
//!
//! **Refused with nothing moved:** a link (`security.md` §5.6: never followed) in place of either
//! folder, of the file or folder being moved, or inside a tree copied across volumes
//! ([`Error::Link`]); a source that changed since the caller read it
//! ([`Error::Changed`]); a folder moved into itself; a name that is not one plain component, or
//! one of Dalica's own (`.dalica.json`, `.dalica-originals`, a temporary file). A new name must
//! also be fit for a new file ([`crate::create::plain_name`], no control characters, no trailing dot
//! or space), unless it is the source's own name kept unchanged.

mod copy;
mod tree;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use dalica_core::names::MAX_NAME_BYTES;

use crate::atomic::{Identity, is_temp_name};
use crate::create::plain_name;
use crate::dirfd::{Folder, Kind, NoReplace};
use crate::faults::{self, Fault, points};
use crate::trash::{self, Sent, Trashed};
use crate::{Error, Result, guard, io_err};

/// The deepest a folder tree copied across volumes may go.
pub const MAX_DEPTH: usize = 32;
/// The most entries (files and folders) a folder tree copied across volumes may hold.
pub const MAX_ENTRIES: u64 = 1_000_000;

/// What an operation that never replaces did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// It did it.
    Done(T),
    /// The target name is taken; nothing changed. The caller renames or skips.
    Exists,
}

/// How a file moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Moved {
    /// Renamed on one volume; the file's identity, unchanged.
    Renamed(Identity),
    /// Copied across volumes and verified (the copy's identity), then the source was handled.
    Copied { identity: Identity, source: Finished },
}

/// What happened to the source of a move that copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finished {
    /// The source is in the OS trash.
    Trashed(Trashed),
    /// The source's volume has no trash: the source is still in place, beside its verified copy.
    NoTrash,
}

/// How a folder moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderMoved {
    /// Renamed or moved on one volume.
    Renamed,
    /// Copied across volumes file by file (`files` files, each verified), then the source tree was
    /// handled.
    Copied { files: u64, source: Finished },
}

/// Moves the file `name` in `from`, which must still have the identity `expect`, to `new_name` in
/// `to` (the same folder for a rename), never replacing a file (see the module docs). Fault points
/// `faults::MOVE_POINTS`; across volumes `faults::COPY_POINTS` then `faults::FINISH_POINTS`.
pub fn move_file(from: &Path, name: &str, expect: Identity, to: &Path, new_name: &str) -> Result<Outcome<Moved>> {
    guard::check()?;
    let (old, new) = names(from, name, new_name)?;
    let (src, dest) = open_pair(from, to)?;
    let mut cross = forced_cross_volume()?;
    if !cross {
        faults::at(points::MOVE_CHECK, "stat", &src.join(old))?;
        check_file(&src, old, expect)?;
        faults::at(points::MOVE_RENAME, "rename", &dest.join(new))?;
        match rename_free(&src, old, &dest, new)? {
            Renamed::Done => {}
            Renamed::Taken => {
                if !(case_change(from, to, old, new) && same_object_at(&dest, new, &expect)? && rename_case(&src, old, new)?) {
                    return Ok(Outcome::Exists);
                }
            }
            Renamed::CrossVolume => cross = true,
        }
        if !cross {
            faults::at(points::MOVE_FSYNC_DIR, "fsync", dest.path())?;
            dest.sync()?;
            src.sync()?;
            faults::at(points::MOVE_VERIFY, "verify", &dest.join(new))?;
            return match dest.entry(new)? {
                Some(e) if e.kind == Kind::File && e.identity == expect => Ok(Outcome::Done(Moved::Renamed(e.identity))),
                _ => Err(Error::VerifyFailed(dest.join(new))),
            };
        }
    }
    let identity = match copy::place(&src, old, expect, &dest, new)? {
        Outcome::Done(identity) => identity,
        Outcome::Exists => return Ok(Outcome::Exists),
    };
    let source = finish(&src, old, expect, &dest, new, identity)?;
    Ok(Outcome::Done(Moved::Copied { identity, source }))
}

/// Copies the file `name` in `from`, which must still have the identity `expect`, to `new_name` in
/// `to`, never replacing a file: a temporary file, `fsync`, the source's modification time and
/// permissions, a byte-for-byte comparison with the source, then the name by a rename that never
/// replaces. Returns the copy's identity. The first phase of a cross-volume move. Fault points
/// `faults::COPY_POINTS` (and the [`points::COPY_FLIP`] switch).
pub fn copy_file(from: &Path, name: &str, expect: Identity, to: &Path, new_name: &str) -> Result<Outcome<Identity>> {
    guard::check()?;
    let (old, new) = names(from, name, new_name)?;
    let (src, dest) = open_pair(from, to)?;
    copy::place(&src, old, expect, &dest, new)
}

/// What an import shares with the thread that shows it: the bytes written so far and the request
/// to stop. One per import, shared by reference with the copying thread (it is `Sync`).
#[derive(Debug, Default)]
pub struct Progress {
    bytes: AtomicU64,
    stop: AtomicBool,
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    /// Bytes written into copies so far (those of a copy that then failed included: it is the work
    /// done, for a speed read-out, not what was imported).
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Asks the import to stop: the file being copied is finished, the next is not started.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Whether [`Progress::stop`] was called.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn add(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }
}

/// What [`import_copy`] did with one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Imported {
    /// Copied and verified; the copy's identity.
    Copied(Identity),
    /// The target name is taken; nothing was written. The caller renames (`-001`) and tries again.
    Exists,
    /// The import was asked to stop before this file; nothing was written.
    Stopped,
}

/// Imports the file `name` in `from`, which must still have the identity `expect` (recorded when
/// the source was listed), as `new_name` in the destination folder `to`: a temporary file written
/// while `progress` counts its bytes, `fsync`, the source's modification time (and permissions on
/// Unix), a byte-for-byte check against the source, then the name by a rename that never replaces
/// (see the module docs). A failed check removes the temporary copy and leaves the source as it
/// was. Fault points `faults::IMPORT_POINTS` (and the [`points::COPY_FLIP`] switch).
pub fn import_copy(from: &Path, name: &str, expect: Identity, to: &Path, new_name: &str, progress: &Progress) -> Result<Imported> {
    guard::check()?;
    let (old, new) = names(from, name, new_name)?;
    if progress.stopped() {
        return Ok(Imported::Stopped);
    }
    let (src, dest) = open_pair(from, to)?;
    faults::at(points::MOVE_CHECK, "stat", &src.join(old))?;
    check_file(&src, old, expect)?;
    let entry = src.entry(old)?.ok_or_else(|| Error::Changed(src.join(old)))?;
    let mut input = copy::open_source(&src, old, expect)?;
    let modified = input.metadata().and_then(|m| m.modified()).ok();

    // 1. A temporary file, written, given the source's time and permissions, made durable.
    faults::at(points::COPY_TEMP, "create", dest.path())?;
    let (tmp, mut out) = import_temp(&dest, new)?;
    let tmp_path = dest.join(&tmp);
    let cleanup = |e: Error| {
        if !matches!(e, Error::Crashed(_)) {
            let _ = dest.remove(&tmp);
        }
        e
    };
    let first = counted_stream(&mut input, &mut out, expect.size(), &src.join(old), &tmp_path, progress).map_err(cleanup)?;
    copy::keep_attributes(&out, &entry, modified);
    faults::at(points::COPY_FSYNC, "fsync", &tmp_path).map_err(cleanup)?;
    faults::fsync(&tmp_path, || out.sync_all()).map_err(|e| cleanup(io_err("fsync", &tmp_path)(e)))?;
    copy::flip_switch(&mut out, first, &tmp_path).map_err(cleanup)?;
    drop(out);
    let copied = match dest.entry(&tmp).map_err(cleanup)? {
        Some(e) if e.kind == Kind::File => e.identity,
        _ => return Err(cleanup(Error::Changed(tmp_path))),
    };

    // 2. Byte for byte against the source, before it gets a name.
    faults::at(points::COPY_VERIFY, "verify", &tmp_path).map_err(cleanup)?;
    if !copy::same_bytes(&src, old, expect, &dest, &tmp).map_err(cleanup)? {
        return Err(cleanup(Error::VerifyFailed(dest.join(new))));
    }

    // 3. The name, never replacing a file.
    faults::at(points::COPY_COMMIT, "rename", &dest.join(new)).map_err(cleanup)?;
    match rename_free(&dest, &tmp, &dest, new).map_err(cleanup)? {
        Renamed::Done => {}
        Renamed::Taken => {
            let _ = dest.remove(&tmp);
            return Ok(Imported::Exists);
        }
        Renamed::CrossVolume => return Err(cleanup(io_err("rename", &dest.join(new))(io::ErrorKind::CrossesDevices.into()))),
    }
    faults::at(points::COPY_FSYNC_DIR, "fsync", dest.path())?;
    dest.sync()?;
    faults::at(points::MOVE_VERIFY, "verify", &dest.join(new))?;
    match dest.entry(new)? {
        Some(e) if e.kind == Kind::File && e.identity == copied => Ok(Imported::Copied(copied)),
        _ => Err(Error::VerifyFailed(dest.join(new))),
    }
}

/// Bytes read at a time while importing.
const IMPORT_CHUNK: usize = 1 << 20;

/// A new temporary file for `name` in `dir` (the shape `stale` removes after a crash).
fn import_temp(dir: &Folder, name: &OsStr) -> Result<(OsString, File)> {
    for _ in 0..16 {
        let tmp = copy::temp_name(name);
        if let Some(f) = dir.create_new(&tmp, false)? {
            return Ok((tmp, f));
        }
    }
    Err(io_err("create", dir.path())(io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")))
}

/// Copies exactly `size` bytes from `input` to `out`, adding each chunk written to `progress`
/// (more or fewer bytes mean the source changed while it was read: [`Error::Changed`]), and
/// returns the first byte. Fault point [`points::COPY_WRITE`]: an error, or a crash after half the
/// first chunk was written.
fn counted_stream(input: &mut File, out: &mut File, size: u64, src_path: &Path, out_path: &Path, progress: &Progress) -> Result<Option<u8>> {
    let point = points::COPY_WRITE;
    let mut buf = vec![0u8; IMPORT_CHUNK];
    let (mut total, mut first) = (0u64, None);
    let mut fault = faults::hit(point);
    loop {
        let n = match input.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_err("read", src_path)(e)),
        };
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        if first.is_none() {
            first = chunk.first().copied();
        }
        if let Some(f) = fault.take() {
            let _ = out.write_all(chunk.get(..n / 2).unwrap_or_default());
            return Err(match f {
                Fault::Error => io_err("write", out_path)(faults::injected(point)),
                Fault::Crash => Error::Crashed(point),
            });
        }
        out.write_all(chunk).map_err(io_err("write", out_path))?;
        progress.add(n as u64);
        total = total.saturating_add(n as u64);
        if total > size {
            return Err(Error::Changed(src_path.to_path_buf()));
        }
    }
    // An empty source still reaches the point.
    match fault {
        Some(Fault::Error) => return Err(io_err("write", out_path)(faults::injected(point))),
        Some(Fault::Crash) => return Err(Error::Crashed(point)),
        None => {}
    }
    if total != size {
        return Err(Error::Changed(src_path.to_path_buf()));
    }
    Ok(first)
}

/// The second phase of a cross-volume move: when the source `name` in `from` still has the identity
/// `expect` and `new_name` in `to` is still the verified copy (`copy`, from [`copy_file`]), sends the
/// source to the OS trash. Anything else is refused with nothing changed. Fault points
/// `faults::FINISH_POINTS`.
pub fn finish_move(from: &Path, name: &str, expect: Identity, to: &Path, new_name: &str, copy: Identity) -> Result<Finished> {
    guard::check()?;
    let (old, new) = names(from, name, new_name)?;
    let (src, dest) = open_pair(from, to)?;
    finish(&src, old, expect, &dest, new, copy)
}

/// Makes the folder `name` in `parent`; [`Outcome::Exists`] when the name is taken (by anything).
/// Fault points `faults::FOLDER_NEW_POINTS`.
pub fn new_folder(parent: &Path, name: &str) -> Result<Outcome<()>> {
    guard::check()?;
    if !new_name_fits(name, None) {
        return Err(refuse_name(parent, name));
    }
    not_a_link(parent)?;
    let dir = Folder::open(parent)?;
    not_a_link(parent)?;
    let os = OsStr::new(name);
    faults::at(points::FOLDER_MKDIR, "create folder", &dir.join(os))?;
    if !dir.make_dir(os)? {
        return Ok(Outcome::Exists);
    }
    faults::at(points::FOLDER_MKDIR_FSYNC, "fsync", parent)?;
    dir.sync()?;
    Ok(Outcome::Done(()))
}

/// Renames or moves the folder `name` in `from` to `new_name` in `to`, with everything in it,
/// never replacing anything (see the module docs). Fault points `faults::FOLDER_MOVE_POINTS`;
/// across volumes `faults::TREE_POINTS`.
pub fn move_folder(from: &Path, name: &str, to: &Path, new_name: &str) -> Result<Outcome<FolderMoved>> {
    guard::check()?;
    let (old, new) = names(from, name, new_name)?;
    not_into_itself(&from.join(old), to)?;
    let (src, dest) = open_pair(from, to)?;
    let identity = match src.entry(old)? {
        Some(e) if e.kind == Kind::Dir => e.identity,
        Some(e) if e.kind == Kind::Link => return Err(Error::Link(src.join(old))),
        Some(_) => return Err(io_err("move", &src.join(old))(io::Error::other("not a folder"))),
        None => return Err(io_err("move", &src.join(old))(io::ErrorKind::NotFound.into())),
    };
    let mut cross = forced_cross_volume()?;
    if !cross {
        faults::at(points::FOLDER_CHECK, "stat", &src.join(old))?;
        check_folder(&src, old, &identity)?;
        faults::at(points::FOLDER_RENAME, "rename", &dest.join(new))?;
        match rename_free(&src, old, &dest, new)? {
            Renamed::Done => {}
            Renamed::Taken => {
                if !(case_change(from, to, old, new) && same_object_at(&dest, new, &identity)? && rename_case(&src, old, new)?) {
                    return Ok(Outcome::Exists);
                }
            }
            Renamed::CrossVolume => cross = true,
        }
        if !cross {
            faults::at(points::FOLDER_FSYNC_DIR, "fsync", dest.path())?;
            dest.sync()?;
            src.sync()?;
            return match dest.entry(new)? {
                Some(e) if e.kind == Kind::Dir && e.identity.same_object(&identity) => Ok(Outcome::Done(FolderMoved::Renamed)),
                _ => Err(Error::VerifyFailed(dest.join(new))),
            };
        }
    }
    faults::at(points::FOLDER_CHECK, "stat", &src.join(old))?;
    check_folder(&src, old, &identity)?;
    let files = match tree::copy_folder(&src, old, &dest, new)? {
        Outcome::Done(files) => files,
        Outcome::Exists => return Ok(Outcome::Exists),
    };
    faults::at(points::FINISH_CHECK, "stat", &src.join(old))?;
    check_folder(&src, old, &identity)?;
    if !dest.entry(new)?.is_some_and(|e| e.kind == Kind::Dir) {
        return Err(Error::Changed(dest.join(new)));
    }
    let source = to_trash(&src.join(old))?;
    Ok(Outcome::Done(FolderMoved::Copied { files, source }))
}

/// The second phase on held folders (see [`finish_move`]).
fn finish(src: &Folder, old: &OsStr, expect: Identity, dest: &Folder, new: &OsStr, copy: Identity) -> Result<Finished> {
    faults::at(points::FINISH_CHECK, "stat", &src.join(old))?;
    check_file(src, old, expect)?;
    match dest.entry(new)? {
        Some(e) if e.kind == Kind::File && e.identity == copy => {}
        _ => return Err(Error::Changed(dest.join(new))),
    }
    to_trash(&src.join(old))
}

fn to_trash(path: &Path) -> Result<Finished> {
    let path = std::path::absolute(path).map_err(io_err("trash", path))?;
    Ok(match trash::send(&path)? {
        Sent::Trashed(t) => Finished::Trashed(t),
        Sent::NoTrash => Finished::NoTrash,
    })
}

/// What [`rename_free`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Renamed {
    Done,
    Taken,
    /// The two folders are on different volumes; nothing changed.
    CrossVolume,
}

/// Renames `from` in `src` to `to` in `dest` only if `to` is free (see the module docs).
fn rename_free(src: &Folder, from: &OsStr, dest: &Folder, to: &OsStr) -> Result<Renamed> {
    match src.rename_noreplace(from, dest, to) {
        Ok(NoReplace::Done) => Ok(Renamed::Done),
        Ok(NoReplace::Taken) => Ok(Renamed::Taken),
        Ok(NoReplace::Unsupported) => {
            if dest.entry(to)?.is_some() {
                return Ok(Renamed::Taken);
            }
            match src.rename_to(from, dest, to) {
                Ok(()) => Ok(Renamed::Done),
                Err(e) if crosses_volumes(&e) => Ok(Renamed::CrossVolume),
                Err(e) => Err(e),
            }
        }
        Err(e) if crosses_volumes(&e) => Ok(Renamed::CrossVolume),
        Err(e) => Err(e),
    }
}

/// A rename that only changes the case of `old` in `dir` (whose volume ignores case, so `new`
/// names the same file): through a temporary name, put back if the second step fails. `false` when
/// the temporary step could not be taken.
fn rename_case(dir: &Folder, old: &OsStr, new: &OsStr) -> Result<bool> {
    let tmp = copy::temp_name(new);
    if rename_free(dir, old, dir, &tmp)? != Renamed::Done {
        return Ok(false);
    }
    match rename_free(dir, &tmp, dir, new) {
        Ok(Renamed::Done) => Ok(true),
        other => {
            let _ = rename_free(dir, &tmp, dir, old);
            other.map(|_| false)
        }
    }
}

/// `old` and `new` are two spellings of one name in one folder (only the case differs).
fn case_change(from: &Path, to: &Path, old: &OsStr, new: &OsStr) -> bool {
    from == to && old != new && old.to_string_lossy().to_lowercase() == new.to_string_lossy().to_lowercase()
}

fn same_object_at(dir: &Folder, name: &OsStr, identity: &Identity) -> Result<bool> {
    Ok(dir.entry(name)?.is_some_and(|e| e.identity.same_object(identity)))
}

fn crosses_volumes(e: &Error) -> bool {
    matches!(e, Error::Io { source, .. } if source.kind() == io::ErrorKind::CrossesDevices)
}

/// The [`points::MOVE_CROSS_VOLUME`] switch.
fn forced_cross_volume() -> Result<bool> {
    match faults::hit(points::MOVE_CROSS_VOLUME) {
        Some(Fault::Error) => Ok(true),
        Some(Fault::Crash) => Err(Error::Crashed(points::MOVE_CROSS_VOLUME)),
        None => Ok(false),
    }
}

/// Both folders, refused when either is a link, opened once each (fault point
/// [`points::MOVE_OPEN_DIR`] between the check and the opening).
fn open_pair(from: &Path, to: &Path) -> Result<(Folder, Folder)> {
    not_a_link(from)?;
    not_a_link(to)?;
    faults::at(points::MOVE_OPEN_DIR, "open", to)?;
    let pair = (Folder::open(from)?, Folder::open(to)?);
    // A folder swapped for a link before it was opened was followed by the opening: refused here.
    // One swapped after it was opened is not followed (the held folder is the real one).
    not_a_link(from)?;
    not_a_link(to)?;
    Ok(pair)
}

/// A link in place of a folder is refused (a missing folder fails when it is opened).
pub(crate) fn not_a_link(dir: &Path) -> Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() => Err(Error::Link(dir.to_path_buf())),
        _ => Ok(()),
    }
}

/// The regular file `name` in `dir` has the identity `expect`; a link in its place is
/// [`Error::Link`] (a move never follows it, and `atomic::identity` refuses to describe one).
fn check_file(dir: &Folder, name: &OsStr, expect: Identity) -> Result<()> {
    match dir.entry(name)? {
        Some(e) if e.kind == Kind::Link => Err(Error::Link(dir.join(name))),
        Some(e) if e.kind == Kind::File && e.identity == expect => Ok(()),
        Some(_) => Err(Error::Changed(dir.join(name))),
        None => Err(io_err("stat", &dir.join(name))(io::ErrorKind::NotFound.into())),
    }
}

/// The folder `name` in `dir` is still the folder `identity` names (its contents may change).
fn check_folder(dir: &Folder, name: &OsStr, identity: &Identity) -> Result<()> {
    match dir.entry(name)? {
        Some(e) if e.kind == Kind::Link => Err(Error::Link(dir.join(name))),
        Some(e) if e.kind == Kind::Dir && e.identity.same_object(identity) => Ok(()),
        _ => Err(Error::Changed(dir.join(name))),
    }
}

/// A folder moved into itself (or into a folder inside it) is refused.
fn not_into_itself(folder: &Path, to: &Path) -> Result<()> {
    if let (Ok(folder), Ok(to)) = (std::fs::canonicalize(folder), std::fs::canonicalize(to))
        && to.starts_with(&folder)
    {
        return Err(io_err("move", &folder)(io::Error::new(io::ErrorKind::InvalidInput, "a folder cannot move into itself")));
    }
    Ok(())
}

/// Dalica's own names, which only Dalica's own modules move: the sidecar, the originals backup,
/// temporary files.
fn own_name(name: &str) -> bool {
    name.starts_with(".dalica") || is_temp_name(OsStr::new(name))
}

/// A name that may already exist in a folder and be moved from it: one component (no separator,
/// no NUL, not `.` or `..`), at most 255 bytes, not one of Dalica's own.
pub(crate) fn existing_name(name: &str) -> bool {
    let separators: &[char] = if cfg!(windows) { &['/', '\\', ':', '\0'] } else { &['/', '\0'] };
    !name.is_empty() && name != "." && name != ".." && name.len() <= MAX_NAME_BYTES && !name.contains(separators) && !own_name(name)
}

/// A name a file or folder may be given: [`plain_name`], no control characters and no trailing dot
/// or space (Windows would drop them), or the source's own name `old` kept unchanged.
fn new_name_fits(new: &str, old: Option<&str>) -> bool {
    let fits = plain_name(new) && !new.chars().any(char::is_control) && !new.ends_with(['.', ' ']);
    fits || (old == Some(new) && existing_name(new))
}

fn refuse_name(folder: &Path, name: &str) -> Error {
    io_err("move", folder)(io::Error::new(io::ErrorKind::InvalidInput, format!("not a plain file name: {name:?}")))
}

/// The source and target names as components, or the refusal of a name that does not fit.
fn names<'a>(from: &Path, name: &'a str, new_name: &'a str) -> Result<(&'a OsStr, &'a OsStr)> {
    if !existing_name(name) {
        return Err(refuse_name(from, name));
    }
    if !new_name_fits(new_name, Some(name)) {
        return Err(refuse_name(from, new_name));
    }
    Ok((OsStr::new(name), OsStr::new(new_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_one_component_and_never_dalicas_own() {
        for ok in ["photo.jpg", "a b.png", "Lisboa à noite.jpg", ".hidden.jpg"] {
            assert!(names(Path::new("."), ok, ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", "a/b.jpg", "a\0b", ".dalica.json", ".dalica-originals", ".x.jpg.dalica-tmp-0123456789abcdef"] {
            assert!(names(Path::new("."), bad, "x.jpg").is_err(), "source {bad:?}");
            assert!(names(Path::new("."), "x.jpg", bad).is_err(), "target {bad:?}");
        }
        // A new name must fit a new file; the source's own name may be kept as it is.
        for bad in ["CON", "nul.txt", "x.jpg:stream", "trailing.", "trailing ", "a\nb.jpg", "a\\b.jpg"] {
            assert!(names(Path::new("."), "x.jpg", bad).is_err(), "target {bad:?}");
        }
        if cfg!(unix) {
            assert!(names(Path::new("."), "12:30.jpg", "12:30.jpg").is_ok());
            assert!(names(Path::new("."), "12:30.jpg", "13:30.jpg").is_err());
        }
    }

    #[test]
    fn a_case_change_is_one_name_in_one_folder() {
        let (a, b) = (Path::new("/p"), Path::new("/q"));
        assert!(case_change(a, a, OsStr::new("photo.jpg"), OsStr::new("Photo.JPG")));
        assert!(!case_change(a, b, OsStr::new("photo.jpg"), OsStr::new("Photo.JPG")));
        assert!(!case_change(a, a, OsStr::new("photo.jpg"), OsStr::new("photo.jpg")));
        assert!(!case_change(a, a, OsStr::new("photo.jpg"), OsStr::new("other.jpg")));
    }
}
