//! Undo Save and Revert from a legacy original (M14.14, `data-model.md` §6 and §9): a photo the
//! previous organizer saved, whose original it kept in the folder's legacy originals folder
//! (`dalica_sidecar::legacy::originals_dir_name`), and that has no record of Dalica's own.
//!
//! The original is **copied**, never moved: nothing in the legacy folder is written, renamed or
//! removed (legacy files are read-only; the old program may still be run on the same folders). The
//! saved file goes to the OS trash rather than being written over, because Dalica cannot always
//! render it again (its edits may be `legacy:` entries it cannot draw). [`restore`] takes these
//! steps, every one through `dalica-fs`:
//!
//! 1. **Check** the name (one plain component) and read the kept original whole, never through a
//!    link: it must still be the original the scan found ([`Kept`]: its size and `md5:` digest;
//!    an empty or unknown digest confirms nothing and is refused).
//! 2. **Read** the saved file whole: a regular, writable file that is still the version the caller
//!    saw ([`Options::seen`]), as M8's Undo Save checks it. A saved file that is gone takes the
//!    original into its free name (only without a [`Seen`] check, as M8's).
//! 3. **Trash** the saved file. A volume without a trash refuses with nothing changed; an item
//!    that turns out not to be the file read in step 2 comes back from the trash and is refused.
//! 4. **Copy** the original's bytes to the photo's name through `fs`'s verified replace, with the
//!    original's modification time, where nothing is.
//!
//! A failure in step 4 before the copy is committed puts the saved file back: from the trash, or,
//! where the trash cannot be listed (macOS), by writing the bytes read in step 2 where nothing is
//! (the trashed one stays in the trash); the photo is then as it was. A failure after it (the copy
//! did not verify) leaves the copy at the name and the saved file in the trash, and the error says
//! so (an `fs` error of the step `restore`, naming the photo). A simulated crash (`fs`'s fault
//! points) stops at once. Nothing here panics. Call it on a worker, never on the UI thread.

use std::io;
use std::path::Path;

use dalica_core::Timestamp;
use dalica_fs::atomic::replace_keeping_times;
use dalica_fs::trash::{self, Restore, Sent, Trashed};
use dalica_fs::{Expect, Identity};
use dalica_sidecar::legacy::originals_dir_name;

use super::{Error, MAX_FILE, Options, Result, Seen, backup_name_ok, digest};

/// What the scan found of a legacy original (the library's `LegacyOriginal`): the file must
/// still be this one before it is copied back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// Its size in bytes.
    pub size: u64,
    /// Its digest, `md5:` and 32 lowercase hex digits.
    pub digest: String,
}

/// What [`restore`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restored {
    /// The restored file's identity (the copy of the original at the photo's name).
    pub identity: Identity,
    /// The saved file, in the OS trash; `None` when it was already gone.
    pub trashed: Option<Trashed>,
}

/// Undo Save or Revert of photo `name` in `folder` from its legacy original `kept`: the original
/// copied back over it, the saved file in the trash. Which one it is changes nothing on disk (the
/// caller puts the recipe back as unsaved edits for Undo Save, and drops the edits for Revert).
pub fn restore(folder: &Path, name: &str, kept: &Kept, options: &Options) -> Result<Restored> {
    if !backup_name_ok(name) || name == originals_dir_name() {
        return Err(Error::Name(name.to_string()));
    }
    // 1. The kept original, still the one the scan found.
    let dir = folder.join(originals_dir_name());
    match dalica_fs::stat(&dir)? {
        Some(s) if !s.is_file => {}
        Some(_) => return Err(Error::NotAFile(dir)),
        None => return Err(Error::NotSaved(name.to_string())),
    }
    let source = dir.join(name);
    let times = match dalica_fs::stat(&source)? {
        Some(s) if s.is_file => s.times,
        Some(_) => return Err(Error::NotAFile(source)),
        None => return Err(Error::NotSaved(name.to_string())),
    };
    let Some(original) = dalica_fs::read(&source, MAX_FILE)? else { return Err(Error::NotSaved(name.to_string())) };
    if kept.digest.is_empty() || original.bytes.len() as u64 != kept.size || digest(&original.bytes) != kept.digest {
        return Err(Error::Record(format!("{} is not the original the library recorded", source.display())));
    }

    // 2. The saved file, still the version the caller saw.
    let path = folder.join(name);
    let saved = match dalica_fs::stat(&path)? {
        None if options.seen.is_some() => return Err(Error::Changed(path)),
        None => None,
        Some(s) if !s.is_file => return Err(Error::NotAFile(path)),
        Some(s) if !s.writable => return Err(Error::ReadOnly(path)),
        Some(s) => {
            let now = Seen { size: s.identity.size(), mtime: Timestamp::from_system_time(s.times.modified) };
            if options.seen.is_some_and(|seen| seen != now) {
                return Err(Error::Changed(path));
            }
            match dalica_fs::read(&path, MAX_FILE)? {
                Some(read) if read.identity == s.identity => Some((read, s.times)),
                _ => return Err(Error::Changed(path)),
            }
        }
    };

    // 3. The saved file to the trash.
    let trashed = match &saved {
        None => None,
        Some((read, _)) => match trash::send(&path)? {
            Sent::NoTrash => {
                let why = io::Error::other("its volume has no trash, so the saved file would be lost; nothing was changed");
                return Err(Error::Fs(dalica_fs::Error::Io { op: "trash", path, source: why }));
            }
            Sent::Trashed(t) if t.identity == read.identity => Some(t),
            Sent::Trashed(t) => {
                // Changed between the read and the send: put it back and refuse.
                return Err(match trash::restore(std::slice::from_ref(&t)) {
                    Ok(back) if back == [Restore::Restored] => Error::Changed(path),
                    _ => incomplete(&path, "the file changed while it was being restored and is now in the trash"),
                });
            }
        },
    };

    // 4. The original's bytes at the photo's name.
    match replace_keeping_times(&path, &original.bytes, Expect::Absent, &times) {
        Ok(identity) => {
            if !identity.modified_at(times.modified) {
                // A platform that refused the time on the temporary file: put it back now. A
                // failure leaves the right bytes with today's time, which is no loss.
                let _ = dalica_fs::set_times(&path, &times, identity);
            }
            let identity = dalica_fs::identity(&path)?.unwrap_or(identity);
            Ok(Restored { identity, trashed })
        }
        Err(dalica_fs::Error::Crashed(point)) => Err(Error::Fs(dalica_fs::Error::Crashed(point))),
        Err(e) => match (&trashed, &saved) {
            (Some(t), Some((read, saved_times))) => Err(take_back(&path, t, &read.bytes, saved_times, e)),
            _ => Err(Error::Fs(e)),
        },
    }
}

/// The copy failed with `e`: the saved file comes back where nothing took its name (from the
/// trash, else its bytes written again), and the photo is as it was; else the error says where
/// things are.
fn take_back(path: &Path, trashed: &Trashed, bytes: &[u8], times: &dalica_fs::Times, e: dalica_fs::Error) -> Error {
    let unverified = || incomplete(path, &format!("the copy of the original did not verify ({e}); the saved file is in the trash"));
    match trash::restore(std::slice::from_ref(trashed)) {
        Ok(back) if back == [Restore::Restored] => return Error::Fs(e),
        // The copy took the name before it failed to verify.
        Ok(back) if back == [Restore::Exists] => return unverified(),
        _ => {}
    }
    match replace_keeping_times(path, bytes, Expect::Absent, times) {
        Ok(_) => Error::Fs(e),
        Err(dalica_fs::Error::Changed(_)) => unverified(),
        Err(again) => {
            incomplete(path, &format!("{e}, then {again}; the saved file is in the trash and the original is still in the previous program's folder"))
        }
    }
}

/// A restore that stopped part-way, saying where the files are.
fn incomplete(path: &Path, reason: &str) -> Error {
    Error::Fs(dalica_fs::Error::Io { op: "restore", path: path.to_path_buf(), source: io::Error::other(reason.to_string()) })
}
