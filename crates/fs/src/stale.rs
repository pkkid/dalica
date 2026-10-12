//! Stale temporary files (`security.md` §5.8): what a crashed atomic replace leaves behind.
//!
//! [`remove_stale_temps`] removes, from one folder, only regular files whose name has exactly the
//! shape of [`crate::atomic::is_temp_name`] (`.<name>.dalica-tmp-<16 hex>`) and whose modification
//! time is more than [`STALE_AFTER`] before `now`. It works through the held folder of `dirfd`, so a
//! folder swapped for a link mid-way cannot point it elsewhere; it never follows a link: a link
//! with a temporary's name is left alone, and so are folders, other kinds of entry and every other
//! name. It reports what it removed and what it could not remove.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::atomic::is_temp_name;
use crate::dirfd::{Folder, Kind};
use crate::faults::{self, Fault, points};
use crate::{Error, Result, guard, io_err};

/// How old a temporary file must be before it is stale: no write takes a day, so a temporary this
/// old belongs to a process that crashed (`security.md` §5.8).
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// At most this many entries of one folder are looked at (the walk's entry bound,
/// `data-model.md` §7).
pub const LIST_LIMIT: usize = 1_000_000;

/// What [`remove_stale_temps`] did in one folder.
#[derive(Debug, Default)]
pub struct StaleTemps {
    /// The temporary files removed, in name order.
    pub removed: Vec<PathBuf>,
    /// Stale temporary files that could not be examined or removed; the rest went on.
    pub errors: Vec<Error>,
}

/// Removes the stale temporary files of `folder` (not its subfolders): regular files of exactly
/// the atomic-replace temporary shape last modified more than [`STALE_AFTER`] before `now`.
/// Fails only when the folder itself cannot be opened or listed.
pub fn remove_stale_temps(folder: &Path, now: SystemTime) -> Result<StaleTemps> {
    guard::check()?;
    let held = Folder::open(folder)?;
    let listed = held.names(is_temp_name, LIST_LIMIT)?;
    let mut names = listed.names;
    names.sort();
    let now_ns = nanos_since_epoch(now);
    let stale_ns = i128::try_from(STALE_AFTER.as_nanos()).unwrap_or(i128::MAX);
    let mut out = StaleTemps::default();
    if !listed.complete {
        let more = io::Error::other(format!("more than {LIST_LIMIT} entries; the rest were not looked at"));
        out.errors.push(io_err("list", folder)(more));
    }
    for name in names {
        let path = held.join(&name);
        let entry = match held.entry(&name) {
            Ok(Some(e)) => e,
            // Gone already (another process cleaned up, or its write finished).
            Ok(None) => continue,
            Err(e) => {
                out.errors.push(e);
                continue;
            }
        };
        // Never a link (removing it would be harmless, but it is not ours), a folder or a device.
        if entry.kind != Kind::File {
            continue;
        }
        // A time in the future (clock skew) is never stale.
        if now_ns.saturating_sub(entry.identity.mtime_ns()) <= stale_ns {
            continue;
        }
        match faults::hit(points::STALE_REMOVE) {
            Some(Fault::Error) => {
                out.errors.push(io_err("remove", &path)(faults::injected(points::STALE_REMOVE)));
                continue;
            }
            Some(Fault::Crash) => return Err(Error::Crashed(points::STALE_REMOVE)),
            None => {}
        }
        match held.remove(&name) {
            Ok(()) => out.removed.push(path),
            Err(e) => out.errors.push(e),
        }
    }
    Ok(out)
}

fn nanos_since_epoch(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => i128::try_from(e.duration().as_nanos()).map_or(i128::MIN, |n| -n),
    }
}
