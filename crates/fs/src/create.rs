//! New files and folders that never replace one (`security.md` §5.4, `data-model.md` §6): Save a
//! Copy, Save As, a JPEG written beside a photo Dalica cannot write in place, every exported file,
//! and the export folder itself.
//!
//! [`new_file`] writes the bytes under the first free name of `core::names::candidates`
//! (`taken.jpg`, `taken-001.jpg`, `taken-002.jpg`, …), all relative to the folder held once
//! (`dirfd`, `security.md` §5.6):
//!
//! 1. create `.<name>.dalica-tmp-<16 hex>` with exclusive creation, write, `fsync`;
//! 2. give it the first free name in one step that never replaces a file: `renameat2` with
//!    `RENAME_NOREPLACE` on Linux and Android, `renameatx_np` with `RENAME_EXCL` on Apple systems,
//!    a hard link then an unlink of the temporary name on other systems (Windows included) and
//!    where the volume refuses the flag; a taken name moves on to the next candidate, so two
//!    writers racing for one name get two names (never "check, then create");
//! 3. `fsync` the folder, then read the file back and compare.
//!
//! A volume with neither (FAT, exFAT, some shares) gets the bytes written straight into the first
//! name exclusive creation (`O_EXCL`) gives; there a crash can leave a torn file under the new name
//! (never under an existing one), the only case without the temporary file.
//!
//! [`exact_file`] writes a file under one exact name or not at all (the first `.dalica.json` of a
//! folder the previous organizer managed, `data-model.md` §9): the same temporary file, written,
//! `fsync`ed and read back **before** the commit, then the same one step that never replaces, for
//! that one name only. A file of the name that appears at any moment before the commit is kept
//! byte for byte and the answer is [`Error::Exists`] (or [`Error::Link`] when it is a link); there
//! is no "check, then create" window. A crash leaves either no file under the name or the whole
//! verified bytes, except on a volume without links (the exclusive creation above).
//!
//! [`new_folder`] makes a folder under the first free name (`Export`, `Export-001`, …).
//!
//! Names are one plain path component ([`plain_name`]) that is fit for a new file: the caller
//! sanitizes generated names; this module refuses what would escape the folder, name a device or
//! collide with Dalica's own files, with nothing written.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use dalica_core::names::{MAX_NAME_BYTES, MAX_NUMBER, candidates};

use crate::atomic::{Identity, is_temp_name};
use crate::dirfd::{Folder, Kind, NoReplace};
use crate::faults::{self, Fault, points};
use crate::{Error, Result, guard, io_err};

/// A file [`new_file`] wrote: the name it got and its identity (for a later `Expect::Unchanged`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub name: String,
    pub identity: Identity,
}

/// Whether `name` may be used to reach a file in a folder (`security.md` §5.4): one plain path
/// component, not empty, not `.` or `..`, no `/`, `\` or NUL, no drive or stream prefix (`C:`,
/// `name:stream`), not a Windows reserved device name (`CON`, `nul.txt`, `COM1`, …), at most 255
/// bytes, and not one of Dalica's own names (`.dalica.json`, `.dalica-originals`, a temporary).
pub fn plain_name(name: &str) -> bool {
    one_component(name) && !name.starts_with(".dalica")
}

/// One path component that names no device and no temporary file: [`plain_name`] without the
/// check for Dalica's own names (an [`exact_file`] may be one of them).
fn one_component(name: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
        "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if name.is_empty() || name == "." || name == ".." || name.len() > MAX_NAME_BYTES || name.contains(['/', '\\', '\0', ':']) {
        return false;
    }
    if is_temp_name(OsStr::new(name)) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// [`plain_name`], and fit for a name Dalica gives a new file: no control characters and no
/// trailing dot or space (Windows drops those, so the file would get another name than asked).
fn new_name(name: &str) -> bool {
    plain_name(name) && fit_for_windows(name)
}

/// No control characters and no trailing dot or space.
fn fit_for_windows(name: &str) -> bool {
    !name.chars().any(char::is_control) && !name.ends_with(['.', ' '])
}

fn refuse_name(folder: &Path, name: &str) -> Error {
    io_err("create", folder)(io::Error::new(io::ErrorKind::InvalidInput, format!("not a plain file name: {name:?}")))
}

/// Writes `bytes` to a new file in `folder` named `name`, or the first free `name-001`, `-002`, …
/// (see the module docs), and returns the name it got. Never replaces a file; refuses a name that
/// is not [`plain_name`]. Fault points: `faults::NEW_FILE_POINTS` (and
/// `faults::NEW_FILE_DIRECT_POINTS` after `create.no_links`).
pub fn new_file(folder: &Path, name: &str, bytes: &[u8]) -> Result<Created> {
    guard::check()?;
    if !new_name(name) {
        return Err(refuse_name(folder, name));
    }
    let dir = Folder::open(folder)?;

    // 1. The temporary file, written and made durable.
    let tmp = write_temp(&dir, name, bytes)?;
    let tmp_path = dir.join(&tmp);
    let cleanup = |e: Error| {
        let _ = dir.remove(&tmp);
        e
    };

    // 2. The first free name, never replacing a file.
    match faults::hit(points::NEW_COMMIT) {
        Some(Fault::Error) => return Err(cleanup(io_err("rename", &tmp_path)(faults::injected(points::NEW_COMMIT)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_COMMIT)),
        None => {}
    }
    let links = match faults::hit(points::NEW_NO_LINKS) {
        Some(Fault::Error) => false,
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_NO_LINKS)),
        None => true,
    };
    let (mut got, mut direct) = (None, !links);
    if links {
        for candidate in candidates(name).filter(|c| new_name(c)) {
            match dir.rename_noreplace(&tmp, &dir, OsStr::new(&candidate)).map_err(cleanup)? {
                NoReplace::Done => {
                    got = Some(candidate);
                    break;
                }
                NoReplace::Taken => {}
                NoReplace::Unsupported => {
                    direct = true;
                    break;
                }
            }
        }
    }
    let name = match got {
        Some(n) => n,
        None if direct => {
            let written = write_direct(&dir, name, bytes);
            // The temporary file is not needed either way, unless a simulated crash stopped here.
            if !matches!(written, Err(Error::Crashed(_))) {
                let _ = dir.remove(&tmp);
            }
            written?
        }
        None => return Err(cleanup(no_free_name(folder, name))),
    };

    // 3. Durable, then verified.
    let identity = durable_and_verified(&dir, &name, bytes)?;
    Ok(Created { name, identity })
}

/// The points of [`exact_file`], in order, on a volume with links: those of
/// [`faults::NEW_FILE_POINTS`] with [`VERIFY_TEMP`] before the commit.
pub const EXACT_FILE_POINTS: [&str; 7] =
    [points::NEW_TEMP, points::NEW_WRITE, points::NEW_FSYNC, VERIFY_TEMP, points::NEW_COMMIT, points::NEW_FSYNC_DIR, points::NEW_VERIFY];

/// Fault point of [`exact_file`]: re-reading the temporary file before the commit.
pub const VERIFY_TEMP: &str = "create.verify_temp";

/// Writes `bytes` to a new file named exactly `name` in `folder` and returns its identity, or
/// writes nothing: never replaces a file, a folder or a link of the name, whenever it appeared
/// (see the module docs). A taken name is [`Error::Exists`], a link under it [`Error::Link`]; a
/// name that is not one plain component (`..`, a separator, a device, a temporary name) and a
/// folder path with `.` or `..` in it are refused with nothing written. Fault points
/// [`EXACT_FILE_POINTS`] (and `faults::NEW_FILE_DIRECT_POINTS` after `create.no_links`).
pub fn exact_file(folder: &Path, name: &str, bytes: &[u8]) -> Result<Identity> {
    guard::check()?;
    if !one_component(name) || !fit_for_windows(name) {
        return Err(refuse_name(folder, name));
    }
    if folder.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(io_err("create", folder)(io::Error::new(io::ErrorKind::InvalidInput, "the folder path has a `.` or `..` in it")));
    }
    let dir = Folder::open(folder)?;
    let target = OsStr::new(name);

    // 1. The temporary file, written, made durable and read back.
    let tmp = write_temp(&dir, name, bytes)?;
    let tmp_path = dir.join(&tmp);
    let cleanup = |e: Error| {
        let _ = dir.remove(&tmp);
        e
    };
    match faults::hit(VERIFY_TEMP) {
        Some(Fault::Error) => return Err(cleanup(io_err("verify", &tmp_path)(faults::injected(VERIFY_TEMP)))),
        Some(Fault::Crash) => return Err(Error::Crashed(VERIFY_TEMP)),
        None => {}
    }
    verify(&dir, &tmp, bytes).map_err(cleanup)?;

    // 2. The one name, never replacing what is there.
    match faults::hit(points::NEW_COMMIT) {
        Some(Fault::Error) => return Err(cleanup(io_err("rename", &tmp_path)(faults::injected(points::NEW_COMMIT)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_COMMIT)),
        None => {}
    }
    let links = match faults::hit(points::NEW_NO_LINKS) {
        Some(Fault::Error) => false,
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_NO_LINKS)),
        None => true,
    };
    let done = if links { dir.rename_noreplace(&tmp, &dir, target).map_err(cleanup)? } else { NoReplace::Unsupported };
    match done {
        NoReplace::Done => {}
        NoReplace::Taken => return Err(cleanup(taken(&dir, target))),
        NoReplace::Unsupported => {
            let written = write_direct_once(&dir, target, bytes);
            if !matches!(written, Err(Error::Crashed(_))) {
                let _ = dir.remove(&tmp);
            }
            written?;
        }
    }

    // 3. Durable, then verified again under its name.
    durable_and_verified(&dir, name, bytes)
}

/// The error for a name [`exact_file`] found taken: [`Error::Link`] when it is a link.
fn taken(dir: &Folder, name: &OsStr) -> Error {
    match dir.entry(name) {
        Ok(Some(e)) if e.kind == Kind::Link => Error::Link(dir.join(name)),
        _ => Error::Exists(dir.join(name)),
    }
}

/// A temporary file for `name` in `dir` holding `bytes`, made durable (fault points
/// `create.temp`, `create.write`, `create.fsync`); an error removes it, a crash leaves it.
fn write_temp(dir: &Folder, name: &str, bytes: &[u8]) -> Result<OsString> {
    faults::at(points::NEW_TEMP, "create", dir.path())?;
    let (tmp, mut file) = create_temp(dir, name)?;
    let tmp_path = dir.join(&tmp);
    let cleanup = |e: Error| {
        let _ = dir.remove(&tmp);
        e
    };
    if let Some(fault) = faults::hit(points::NEW_WRITE) {
        let _ = file.write_all(bytes.get(..bytes.len() / 2).unwrap_or_default());
        return Err(match fault {
            Fault::Error => cleanup(io_err("write", &tmp_path)(faults::injected(points::NEW_WRITE))),
            Fault::Crash => Error::Crashed(points::NEW_WRITE),
        });
    }
    file.write_all(bytes).map_err(|e| cleanup(io_err("write", &tmp_path)(e)))?;
    match faults::hit(points::NEW_FSYNC) {
        Some(Fault::Error) => return Err(cleanup(io_err("fsync", &tmp_path)(faults::injected(points::NEW_FSYNC)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_FSYNC)),
        None => {}
    }
    faults::fsync(&tmp_path, || file.sync_all()).map_err(|e| cleanup(io_err("fsync", &tmp_path)(e)))?;
    Ok(tmp)
}

/// The folder made durable, then `name` read back and compared with `bytes` (fault points
/// `create.fsync_dir`, `create.verify`); returns its identity.
fn durable_and_verified(dir: &Folder, name: &str, bytes: &[u8]) -> Result<Identity> {
    let path = dir.join(OsStr::new(name));
    faults::at(points::NEW_FSYNC_DIR, "fsync", dir.path())?;
    dir.sync()?;
    faults::at(points::NEW_VERIFY, "verify", &path)?;
    verify(dir, OsStr::new(name), bytes)
}

/// Reads `name` back (never through a link) and compares it with `bytes`; returns its identity.
fn verify(dir: &Folder, name: &OsStr, bytes: &[u8]) -> Result<Identity> {
    let path = dir.join(name);
    let back = dir.open_read(name)?;
    let identity = Identity::of_file(&back).map_err(io_err("verify", &path))?;
    let mut read_back = Vec::with_capacity(bytes.len());
    back.take(u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(1)).read_to_end(&mut read_back).map_err(io_err("verify", &path))?;
    if read_back != bytes {
        return Err(Error::VerifyFailed(path));
    }
    Ok(identity)
}

/// A volume without hard links or a non-replacing rename: the bytes go straight into the first
/// name exclusive creation gives. A failure removes that file (it is ours, made a moment ago).
/// The names are exhausted when every candidate is taken.
fn write_direct(dir: &Folder, name: &str, bytes: &[u8]) -> Result<String> {
    for candidate in candidates(name).filter(|c| new_name(c)) {
        match write_direct_one(dir, OsStr::new(&candidate), bytes)? {
            Some(()) => return Ok(candidate),
            None => continue,
        }
    }
    Err(no_free_name(dir.path(), name))
}

/// [`write_direct`] for one exact name: [`Error::Exists`] (or [`Error::Link`]) when it is taken.
fn write_direct_once(dir: &Folder, name: &OsStr, bytes: &[u8]) -> Result<()> {
    write_direct_one(dir, name, bytes)?.ok_or_else(|| taken(dir, name))
}

/// The bytes written into `name` by exclusive creation and made durable; `None` when the name is
/// taken (nothing written). A failure removes the file (it is ours, made a moment ago).
fn write_direct_one(dir: &Folder, name: &OsStr, bytes: &[u8]) -> Result<Option<()>> {
    let Some(mut file) = dir.create_new(name, false)? else { return Ok(None) };
    let path = dir.join(name);
    let cleanup = |e: Error| {
        let _ = dir.remove(name);
        e
    };
    if let Some(fault) = faults::hit(points::NEW_DIRECT_WRITE) {
        let _ = file.write_all(bytes.get(..bytes.len() / 2).unwrap_or_default());
        return Err(match fault {
            Fault::Error => cleanup(io_err("write", &path)(faults::injected(points::NEW_DIRECT_WRITE))),
            Fault::Crash => Error::Crashed(points::NEW_DIRECT_WRITE),
        });
    }
    file.write_all(bytes).map_err(|e| cleanup(io_err("write", &path)(e)))?;
    match faults::hit(points::NEW_DIRECT_FSYNC) {
        Some(Fault::Error) => return Err(cleanup(io_err("fsync", &path)(faults::injected(points::NEW_DIRECT_FSYNC)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::NEW_DIRECT_FSYNC)),
        None => {}
    }
    faults::fsync(&path, || file.sync_all()).map_err(|e| cleanup(io_err("fsync", &path)(e)))?;
    Ok(Some(()))
}

fn no_free_name(folder: &Path, name: &str) -> Error {
    io_err("create", folder)(io::Error::new(io::ErrorKind::AlreadyExists, format!("no free name for {name:?}")))
}

/// Makes a new folder in `parent` named `name`, or the first free `name-001`, `name-002`, … (the
/// number goes last: a folder has no extension), and returns the name it got. Never reuses an
/// existing folder; refuses a name that is not [`plain_name`]. Fault points
/// `faults::NEW_FOLDER_POINTS`.
pub fn new_folder(parent: &Path, name: &str) -> Result<String> {
    guard::check()?;
    if !new_name(name) {
        return Err(refuse_name(parent, name));
    }
    let dir = Folder::open(parent)?;
    faults::at(points::NEW_MKDIR, "create folder", parent)?;
    let mut got = None;
    for candidate in folder_candidates(name).filter(|c| new_name(c)) {
        if dir.make_dir(OsStr::new(&candidate))? {
            got = Some(candidate);
            break;
        }
    }
    let Some(got) = got else { return Err(no_free_name(parent, name)) };
    faults::at(points::NEW_MKDIR_FSYNC, "fsync", parent)?;
    dir.sync()?;
    Ok(got)
}

/// `name`, then `name-001`, `name-002`, … up to `MAX_NUMBER`, each shortened on a character
/// boundary to fit 255 bytes.
fn folder_candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(name.to_string()).chain((1..=MAX_NUMBER).map(move |n| {
        let number = format!("-{n:03}");
        let mut keep = name.len().min(MAX_NAME_BYTES.saturating_sub(number.len()));
        while !name.is_char_boundary(keep) {
            keep -= 1;
        }
        format!("{}{number}", name.get(..keep).unwrap_or_default())
    }))
}

const TEMP_MARK: &str = ".dalica-tmp-";
/// Longest part of the target's name kept in a temporary name, so the whole stays within 255 bytes.
const TEMP_NAME_KEEP: usize = 255 - 1 - TEMP_MARK.len() - 16;

/// Creates a new temporary file for `name` in `dir` (the shape `atomic::is_temp_name` knows, so
/// `stale` removes one a crash left behind); returns its name and the open file.
fn create_temp(dir: &Folder, name: &str) -> Result<(OsString, File)> {
    let mut keep = name.len().min(TEMP_NAME_KEEP);
    while !name.is_char_boundary(keep) {
        keep -= 1;
    }
    let stem = name.get(..keep).unwrap_or_default();
    for _ in 0..16 {
        let tmp = OsString::from(format!(".{stem}{TEMP_MARK}{}", random_hex()));
        if let Some(f) = dir.create_new(&tmp, false)? {
            return Ok((tmp, f));
        }
    }
    Err(io_err("create", dir.path())(io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")))
}

/// 16 hex digits from the standard library's per-process random hash keys and a counter.
fn random_hex() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    if let Ok(d) = UNIX_EPOCH.elapsed() {
        h.write_u128(d.as_nanos());
    }
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_are_one_harmless_component() {
        for ok in ["photo.jpg", "Lisboa à noite.jpg", ".hidden.jpg", "a b.png", "console.jpg", "icon.jpg", "COM10.jpg", &"é".repeat(127)] {
            assert!(plain_name(ok), "{ok}");
        }
        let long = "x".repeat(256);
        for bad in [
            "",
            ".",
            "..",
            "../x.jpg",
            "a/b.jpg",
            "a\\b.jpg",
            "C:x.jpg",
            "x.jpg:stream",
            "a\0b",
            "CON",
            "nul.txt",
            "com1.jpg",
            "LPT9 .jpg",
            ".dalica.json",
            ".dalica-originals",
            ".x.jpg.dalica-tmp-0123456789abcdef",
            &long,
        ] {
            assert!(!plain_name(bad), "{bad:?}");
        }
        for bad in ["photo.", "photo ", "a\nb.jpg", "a\u{7f}.jpg"] {
            assert!(!new_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn folder_candidates_number_last_and_fit() {
        let c: Vec<String> = folder_candidates("Export 2026.06").take(3).collect();
        assert_eq!(c, ["Export 2026.06", "Export 2026.06-001", "Export 2026.06-002"]);
        let long = "é".repeat(200);
        let n = folder_candidates(&long).nth(1).unwrap();
        assert!(n.len() <= MAX_NAME_BYTES && n.ends_with("-001"));
        assert_eq!(folder_candidates("x").count(), 1 + MAX_NUMBER as usize);
    }

    #[test]
    fn temporary_names_have_the_shape_stale_removes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::open(dir.path()).unwrap();
        for name in ["photo.jpg", &"x".repeat(255), &"é".repeat(127)] {
            let (n, _f) = create_temp(&folder, name).unwrap();
            assert!(n.len() <= 255 && is_temp_name(&n), "{n:?}");
        }
    }
}
