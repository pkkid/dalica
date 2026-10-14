//! Undo Save and Revert (`data-model.md` §6): the kept original moved back over the saved file,
//! then the record's entry dropped. The saved file is replaced, not sent to the trash: it can
//! always be rendered again from the original and its recipe.

use std::io;
use std::path::Path;

use dalica_core::Timestamp;
use dalica_fs::{Expect, Identity, originals as backup};
use dalica_sidecar::originals::{Entry, State};

use super::record::{Record, digest_matches};
use super::{Error, Options, Result, Seen, backup_name_ok};

/// What Undo Save or Revert did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restored {
    /// Undo Save: the recipe the saved file held, to put back as unsaved edits (in front of any
    /// edits made since). Revert: `None`, the edits are discarded.
    pub recipe: Option<Vec<String>>,
    /// The restored original's identity.
    pub identity: Identity,
    /// The record's entry that was dropped.
    pub entry: Entry,
}

/// Undo Save: the original of `name` in `folder` back at its name; answers the recorded recipe.
pub fn undo(folder: &Path, name: &str, options: &Options) -> Result<Restored> {
    put_back(folder, name, options, true)
}

/// Revert: the original of `name` in `folder` back at its name; the edits are discarded.
pub fn revert(folder: &Path, name: &str, options: &Options) -> Result<Restored> {
    put_back(folder, name, options, false)
}

/// Refused with nothing moved: a name that is not plain; no `done` entry ([`Error::NotSaved`],
/// [`Error::Pending`]); a read-only saved file, or one that is not the version the caller saw
/// ([`Options::seen`]); a backup that is missing, a link or not the recorded original.
fn put_back(folder: &Path, name: &str, options: &Options, keep_recipe: bool) -> Result<Restored> {
    if !backup_name_ok(name) {
        return Err(Error::Name(name.to_string()));
    }
    let path = folder.join(name);
    let mut record = Record::read(folder, options.now, false)?;
    let entry = match record.entry(name)? {
        None => return Err(Error::NotSaved(name.to_string())),
        Some(e) if e.state == State::Pending => return Err(Error::Pending(name.to_string())),
        Some(e) => e,
    };

    // The saved file must be the version the caller saw. One that is gone takes the original
    // back into its free name.
    let saved = match dalica_fs::stat(&path)? {
        None if options.seen.is_some() => return Err(Error::Changed(path)),
        None => Expect::Absent,
        Some(s) if !s.is_file => return Err(Error::NotAFile(path)),
        Some(s) if !s.writable => return Err(Error::ReadOnly(path)),
        Some(s) => {
            let now = Seen { size: s.identity.size(), mtime: Timestamp::from_system_time(s.times.modified) };
            if options.seen.is_some_and(|seen| seen != now) {
                return Err(Error::Changed(path));
            }
            Expect::Unchanged(s.identity)
        }
    };

    let check = |r: &mut dyn io::Read| digest_matches(&entry.digest, r);
    let kept = backup::Recorded { size: entry.size, digest_matches: &check };
    let identity = backup::restore(folder, name, saved, &kept)?;

    let dropped = record.put_back(name, None).and_then(|()| record.store());
    match dropped {
        Ok(()) => Ok(Restored { recipe: keep_recipe.then(|| entry.recipe.clone()), identity, entry }),
        Err(e) if e.crashed() => Err(e),
        // The original is back but the record still names it: the repair drops the entry.
        Err(e) => Err(Error::Incomplete { name: name.to_string(), reason: e.reason() }),
    }
}
