//! A folder tree copied across volumes: into a temporary folder beside the target, every file
//! written, `fsync`ed and compared byte for byte with its source, then the copy given its name by a
//! rename that never replaces a folder. Bounded by [`MAX_DEPTH`] and [`MAX_ENTRIES`]; a link or a
//! special file inside refuses the copy. A failed copy removes only what it made (unless a
//! simulated crash stopped it, like a killed process).

use std::ffi::OsStr;
use std::io;

use super::copy::{flip_switch, keep_attributes, open_source, same_bytes, stream, temp_name};
use super::{MAX_DEPTH, MAX_ENTRIES, Outcome, Renamed, rename_free};
use crate::atomic::is_temp_name;
use crate::dirfd::{Entry, Folder, Kind};
use crate::faults::{self, points};
use crate::{Error, Result, io_err};

/// Copies the folder `name` in `src_parent` to `to` in `dest_parent`; returns how many files it
/// copied.
pub(super) fn copy_folder(src_parent: &Folder, name: &OsStr, dest_parent: &Folder, to: &OsStr) -> Result<Outcome<u64>> {
    let src = src_parent.open_dir(name)?.ok_or_else(|| Error::Changed(src_parent.join(name)))?;
    faults::at(points::TREE_MKDIR, "create folder", dest_parent.path())?;
    let mut tmp = None;
    for _ in 0..16 {
        let candidate = temp_name(to);
        if dest_parent.make_dir(&candidate)? {
            tmp = Some(candidate);
            break;
        }
    }
    let Some(tmp) = tmp else {
        return Err(io_err("create folder", dest_parent.path())(io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")));
    };
    let cleanup = |e: Error| {
        if !matches!(e, Error::Crashed(_)) {
            remove_own_tree(dest_parent, &tmp, 0);
        }
        e
    };
    let mut entries = 0u64;
    let files = {
        let copy = dest_parent.open_dir(&tmp).map_err(cleanup)?.ok_or_else(|| cleanup(Error::Changed(dest_parent.join(&tmp))))?;
        copy_tree(&src, &copy, 0, &mut entries).map_err(cleanup)?
    };
    faults::at(points::TREE_COMMIT, "rename", &dest_parent.join(to)).map_err(cleanup)?;
    match rename_free(dest_parent, &tmp, dest_parent, to).map_err(cleanup)? {
        Renamed::Done => {}
        Renamed::Taken => {
            remove_own_tree(dest_parent, &tmp, 0);
            return Ok(Outcome::Exists);
        }
        Renamed::CrossVolume => return Err(cleanup(io_err("rename", &dest_parent.join(to))(io::ErrorKind::CrossesDevices.into()))),
    }
    dest_parent.sync()?;
    Ok(Outcome::Done(files))
}

/// Copies everything in `src` into the empty folder `dest`, depth first, in name order; returns how
/// many files it copied. `entries` counts every entry seen so far against [`MAX_ENTRIES`].
fn copy_tree(src: &Folder, dest: &Folder, depth: usize, entries: &mut u64) -> Result<u64> {
    if depth > MAX_DEPTH {
        return Err(too_large(src, "is nested too deep to copy"));
    }
    let limit = usize::try_from(MAX_ENTRIES.saturating_add(2)).unwrap_or(usize::MAX);
    let listed = src.names(|n| !is_temp_name(n), limit)?;
    if !listed.complete {
        return Err(too_large(src, "holds too many entries to copy"));
    }
    let mut names = listed.names;
    names.sort();
    let mut files = 0u64;
    for name in &names {
        *entries = entries.saturating_add(1);
        if *entries > MAX_ENTRIES {
            return Err(too_large(src, "holds too many entries to copy"));
        }
        let path = src.join(name);
        match src.entry(name)? {
            None => return Err(Error::Changed(path)),
            Some(e) if e.kind == Kind::File => {
                copy_one(src, name, &e, dest)?;
                files = files.saturating_add(1);
            }
            Some(e) if e.kind == Kind::Dir => {
                faults::at(points::TREE_MKDIR, "create folder", &dest.join(name))?;
                if !dest.make_dir(name)? {
                    return Err(Error::Changed(dest.join(name)));
                }
                let sub_src = src.open_dir(name)?.ok_or_else(|| Error::Changed(path.clone()))?;
                let sub_dest = dest.open_dir(name)?.ok_or_else(|| Error::Changed(dest.join(name)))?;
                files = files.saturating_add(copy_tree(&sub_src, &sub_dest, depth.saturating_add(1), entries)?);
            }
            Some(e) if e.kind == Kind::Link => return Err(Error::Link(path)),
            Some(_) => return Err(io_err("copy", &path)(io::Error::other("neither a file nor a folder"))),
        }
    }
    dest.sync()?;
    Ok(files)
}

/// Copies the file `name` (described by `entry`) from `src` into a new file of the same name in
/// `dest`, then compares the two byte for byte.
fn copy_one(src: &Folder, name: &OsStr, entry: &Entry, dest: &Folder) -> Result<()> {
    let mut input = open_source(src, name, entry.identity)?;
    let modified = input.metadata().and_then(|m| m.modified()).ok();
    let path = dest.join(name);
    let Some(mut out) = dest.create_new(name, false)? else { return Err(Error::Changed(path)) };
    let first = stream(&mut input, &mut out, entry.identity.size(), points::TREE_WRITE, &src.join(name), &path)?;
    keep_attributes(&out, entry, modified);
    faults::at(points::TREE_FSYNC, "fsync", &path)?;
    faults::fsync(&path, || out.sync_all()).map_err(io_err("fsync", &path))?;
    flip_switch(&mut out, first, &path)?;
    drop(out);
    faults::at(points::TREE_VERIFY, "verify", &path)?;
    if !same_bytes(src, name, entry.identity, dest, name)? {
        return Err(Error::VerifyFailed(path));
    }
    Ok(())
}

fn too_large(dir: &Folder, why: &str) -> Error {
    io_err("copy", dir.path())(io::Error::other(why.to_string()))
}

/// Removes the tree Dalica made at `name` in `parent` (a failed copy's temporary folder): its
/// files, then its folders, best effort, never following a link.
fn remove_own_tree(parent: &Folder, name: &OsStr, depth: usize) {
    if depth <= MAX_DEPTH
        && let Ok(Some(dir)) = parent.open_dir(name)
        && let Ok(listed) = dir.names(|_| true, usize::try_from(MAX_ENTRIES).unwrap_or(usize::MAX))
    {
        for child in &listed.names {
            match dir.entry(child) {
                Ok(Some(e)) if e.kind == Kind::Dir => remove_own_tree(&dir, child, depth.saturating_add(1)),
                Ok(Some(_)) => {
                    let _ = dir.remove(child);
                }
                _ => {}
            }
        }
    }
    let _ = std::fs::remove_dir(parent.join(name));
}
