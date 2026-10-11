//! What a packed cache needs from the disk (`architecture.md` §6.6): Dalica's own cache files, never
//! the user's.
//!
//! - [`PackFile`]: an append-only file read with positioned reads (no memory mapping). Appends are
//!   not synced one by one (a cache can lose its tail); [`PackFile::sync`] makes them durable and
//!   [`PackFile::truncate`] cuts a torn tail found when the file is opened again.
//! - [`TempFile`]: a new file streamed into `.<name>.dalica-tmp-<16 hex>` next to its target and
//!   committed by one rename (fsync, rename, folder fsync), for rewriting a pack whole. Dropped
//!   uncommitted, it removes itself.
//! - [`list_shaped`] and [`remove_shaped`]: list and remove only regular files whose name has the
//!   cache's own shape, so a cache folder shared with anything else never loses a foreign file.
//!
//! Every step that touches the disk has a fault point ([`points`], feature `faults`), and every
//! call is refused on the UI thread.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::faults::{self, Fault};
use crate::{Error, Result, guard, io_err};

/// The pack fault points.
pub mod points {
    /// A positioned read.
    pub const READ: &str = "pack.read";
    /// Appending bytes to a pack file (a fault writes half of them first).
    pub const APPEND: &str = "pack.append";
    /// `fsync` of a pack file.
    pub const SYNC: &str = "pack.sync";
    /// Cutting a pack file's torn tail.
    pub const TRUNCATE: &str = "pack.truncate";
    /// Creating a streamed temporary file.
    pub const TEMP_CREATE: &str = "pack.temp_create";
    /// Writing into a streamed temporary file (a fault writes half of the bytes first).
    pub const TEMP_WRITE: &str = "pack.temp_write";
    /// `fsync` of a streamed temporary file before its rename.
    pub const TEMP_SYNC: &str = "pack.temp_sync";
    /// Renaming a streamed temporary file over its target.
    pub const TEMP_RENAME: &str = "pack.temp_rename";
    /// Removing a cache file.
    pub const REMOVE: &str = "pack.remove";
}

/// Every pack fault point, reads first.
pub const PACK_POINTS: [&str; 9] = [
    points::READ,
    points::APPEND,
    points::SYNC,
    points::TRUNCATE,
    points::TEMP_CREATE,
    points::TEMP_WRITE,
    points::TEMP_SYNC,
    points::TEMP_RENAME,
    points::REMOVE,
];

/// The pack fault points that write.
pub const PACK_WRITE_POINTS: [&str; 8] =
    [points::APPEND, points::SYNC, points::TRUNCATE, points::TEMP_CREATE, points::TEMP_WRITE, points::TEMP_SYNC, points::TEMP_RENAME, points::REMOVE];

/// The most directory entries [`list_shaped`] looks at.
pub const LIST_LIMIT: usize = 100_000;

/// Returns the armed fault at `point` as the error it stands for, or `Ok`.
fn fault_at(point: &'static str, op: &'static str, path: &Path) -> Result<()> {
    match faults::hit(point) {
        Some(Fault::Error) => Err(io_err(op, path)(faults::injected(point))),
        Some(Fault::Crash) => Err(Error::Crashed(point)),
        None => Ok(()),
    }
}

/// Metadata of `path` without following a final link; `None` if it does not exist; a link is refused.
fn plain_meta(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(Error::Link(path.to_path_buf())),
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err("stat", path)(e)),
    }
}

/// An append-only cache file read with positioned reads. Reads take `&self`, so several threads
/// can read one pack at once (each read names its own offset).
#[derive(Debug)]
pub struct PackFile {
    file: File,
    path: PathBuf,
    len: u64,
}

impl PackFile {
    /// Opens `path` for reading and appending, creating it empty when missing. A final symbolic
    /// link or a folder is refused.
    pub fn open(path: &Path) -> Result<PackFile> {
        guard::check()?;
        if plain_meta(path)?.is_some_and(|m| m.is_dir()) {
            return Err(io_err("open", path)(io::Error::other("is a folder")));
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(io_err("open", path))?;
        let len = file.metadata().map_err(io_err("stat", path))?.len();
        Ok(PackFile { file, path: path.to_path_buf(), len })
    }

    /// The file's length as far as this handle knows (appends through it included).
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Fills `buf` from `offset`. A read past the end is an error (`UnexpectedEof`).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        guard::check()?;
        fault_at(points::READ, "read", &self.path)?;
        read_exact_at(&self.file, buf, offset).map_err(io_err("read", &self.path))
    }

    /// `len` bytes from `offset`, refusing more than `limit` (checked before allocating).
    pub fn read_vec(&self, offset: u64, len: u64, limit: u64) -> Result<Vec<u8>> {
        if len > limit {
            return Err(Error::TooLarge { path: self.path.clone(), limit });
        }
        let n = usize::try_from(len).map_err(|_| Error::TooLarge { path: self.path.clone(), limit })?;
        let mut buf = vec![0; n];
        self.read_at(offset, &mut buf)?;
        Ok(buf)
    }

    /// Appends `bytes` at the end and returns the offset they start at. Not synced: see
    /// [`PackFile::sync`]. After a failure the length is re-read from the file, so a torn append
    /// shows as a tail the caller can cut.
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64> {
        guard::check()?;
        let at = self.len;
        let n = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let end = at.checked_add(n).ok_or_else(|| io_err("append", &self.path)(io::Error::other("file too long")))?;
        if let Some(fault) = faults::hit(points::APPEND) {
            let half = bytes.get(..bytes.len() / 2).unwrap_or_default();
            let _ = write_all_at(&self.file, half, at);
            self.resync_len();
            return Err(match fault {
                Fault::Error => io_err("append", &self.path)(faults::injected(points::APPEND)),
                Fault::Crash => Error::Crashed(points::APPEND),
            });
        }
        if let Err(e) = write_all_at(&self.file, bytes, at) {
            self.resync_len();
            return Err(io_err("append", &self.path)(e));
        }
        self.len = end;
        Ok(at)
    }

    /// Makes every append so far durable.
    pub fn sync(&self) -> Result<()> {
        guard::check()?;
        fault_at(points::SYNC, "fsync", &self.path)?;
        self.file.sync_data().map_err(io_err("fsync", &self.path))
    }

    /// Cuts the file to `len` bytes (a torn tail); never grows it.
    pub fn truncate(&mut self, len: u64) -> Result<()> {
        guard::check()?;
        if len >= self.len {
            return Ok(());
        }
        fault_at(points::TRUNCATE, "truncate", &self.path)?;
        if let Err(e) = self.file.set_len(len) {
            self.resync_len();
            return Err(io_err("truncate", &self.path)(e));
        }
        self.len = len;
        Ok(())
    }

    fn resync_len(&mut self) {
        if let Ok(m) = self.file.metadata() {
            self.len = m.len();
        }
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(n) => {
                let rest = std::mem::take(&mut buf);
                buf = rest.get_mut(n..).unwrap_or_default();
                offset = offset.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(n) => {
                buf = buf.get(n..).unwrap_or_default();
                offset = offset.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn read_exact_at(mut file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)
}

#[cfg(not(any(unix, windows)))]
fn write_all_at(mut file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    use std::io::{Seek as _, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(buf)
}

/// The marker inside temporary names, the same shape as `atomic`'s (`is_temp_name`).
const TEMP_MARK: &str = ".dalica-tmp-";
/// Longest part of the target's name kept in a temporary name, so the whole stays within 255 bytes.
const TEMP_NAME_KEEP: usize = 255 - 1 - TEMP_MARK.len() - 16;

/// A new file streamed into a temporary file next to its target and committed by one rename.
/// Dropped before [`TempFile::commit`], it removes the temporary file (unless a simulated crash
/// stopped it, which leaves the file as a killed process would).
#[derive(Debug)]
#[must_use = "an uncommitted temporary file is removed when dropped"]
pub struct TempFile {
    file: Option<File>,
    tmp: PathBuf,
    target: PathBuf,
    len: u64,
    /// Leave the temporary file behind on drop (a simulated crash).
    keep: bool,
}

impl TempFile {
    /// Creates `.<name>.dalica-tmp-<16 hex>` in `target`'s folder (exclusive creation).
    pub fn create(target: &Path) -> Result<TempFile> {
        guard::check()?;
        let name = target.file_name().ok_or_else(|| io_err("create", target)(io::Error::new(io::ErrorKind::InvalidInput, "no file name")))?;
        let dir = parent_of(target);
        fault_at(points::TEMP_CREATE, "create", dir)?;
        let lossy = name.to_string_lossy();
        let mut keep = lossy.len().min(TEMP_NAME_KEEP);
        while !lossy.is_char_boundary(keep) {
            keep -= 1;
        }
        let stem = lossy.get(..keep).unwrap_or_default();
        let mut last = None;
        for _ in 0..16 {
            let tmp = dir.join(format!(".{stem}{TEMP_MARK}{}", random_hex()));
            match OpenOptions::new().write(true).create_new(true).open(&tmp) {
                Ok(file) => return Ok(TempFile { file: Some(file), tmp, target: target.to_path_buf(), len: 0, keep: false }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(io_err("create", &tmp)(e)),
            }
        }
        Err(io_err("create", dir)(last.unwrap_or_else(|| io::Error::other("no free temporary name"))))
    }

    /// The bytes written so far.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The temporary file's path.
    pub fn temp_path(&self) -> &Path {
        &self.tmp
    }

    /// Appends `bytes` and returns the offset they start at.
    pub fn write(&mut self, bytes: &[u8]) -> Result<u64> {
        guard::check()?;
        let at = self.len;
        let end = at
            .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| io_err("write", &self.tmp)(io::Error::other("file too long")))?;
        let Some(file) = self.file.as_mut() else { return Err(io_err("write", &self.tmp)(io::Error::other("already closed"))) };
        if let Some(fault) = faults::hit(points::TEMP_WRITE) {
            let half = bytes.get(..bytes.len() / 2).unwrap_or_default();
            let _ = file.write_all(half);
            return Err(match fault {
                Fault::Error => io_err("write", &self.tmp)(faults::injected(points::TEMP_WRITE)),
                Fault::Crash => self.crash(points::TEMP_WRITE),
            });
        }
        file.write_all(bytes).map_err(io_err("write", &self.tmp))?;
        self.len = end;
        Ok(at)
    }

    /// fsync, rename over the target (replacing it), fsync the folder. On failure before the
    /// rename the temporary file is removed and the target untouched.
    pub fn commit(mut self) -> Result<()> {
        guard::check()?;
        let Some(file) = self.file.take() else { return Err(io_err("commit", &self.tmp)(io::Error::other("already closed"))) };
        match faults::hit(points::TEMP_SYNC) {
            Some(Fault::Error) => return Err(io_err("fsync", &self.tmp)(faults::injected(points::TEMP_SYNC))),
            Some(Fault::Crash) => return Err(self.crash(points::TEMP_SYNC)),
            None => {}
        }
        file.sync_all().map_err(io_err("fsync", &self.tmp))?;
        drop(file);
        match faults::hit(points::TEMP_RENAME) {
            Some(Fault::Error) => return Err(io_err("rename", &self.target)(faults::injected(points::TEMP_RENAME))),
            Some(Fault::Crash) => return Err(self.crash(points::TEMP_RENAME)),
            None => {}
        }
        if plain_meta(&self.target)?.is_some_and(|m| m.is_dir()) {
            return Err(io_err("rename", &self.target)(io::Error::other("is a folder")));
        }
        fs::rename(&self.tmp, &self.target).map_err(io_err("rename", &self.target))?;
        // Renamed: nothing left to clean up.
        self.keep = true;
        sync_dir(parent_of(&self.target))
    }

    fn crash(&mut self, point: &'static str) -> Error {
        self.keep = true;
        Error::Crashed(point)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            drop(self.file.take());
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

fn parent_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// 16 hex digits from the standard library's per-process random hash keys and a counter.
fn random_hex() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    if let Ok(d) = std::time::UNIX_EPOCH.elapsed() {
        h.write_u128(d.as_nanos());
    }
    format!("{:016x}", h.finish())
}

/// Makes a rename durable: `fsync` the folder on Unix (Windows journals renames itself).
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir).and_then(|d| d.sync_all()).map_err(io_err("fsync", dir))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

/// One cache file found by [`list_shaped`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheFile {
    pub name: String,
    pub len: u64,
}

/// The regular files directly in `dir` whose (UTF-8) name passes `shape`, sorted by name. Links,
/// folders and every other name are left out; at most [`LIST_LIMIT`] entries are looked at. A
/// missing folder lists nothing.
pub fn list_shaped(dir: &Path, shape: impl Fn(&str) -> bool) -> Result<Vec<CacheFile>> {
    guard::check()?;
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_err("list", dir)(e)),
    };
    let mut out = Vec::new();
    for entry in entries.take(LIST_LIMIT) {
        let entry = entry.map_err(io_err("list", dir))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if !shape(&name) {
            continue;
        }
        // `DirEntry::metadata` does not follow links.
        match entry.metadata() {
            Ok(m) if m.file_type().is_file() => out.push(CacheFile { name, len: m.len() }),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("stat", &entry.path())(e)),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Removes `path` if it is a regular file whose name passes `shape`; `Ok(false)` when it does not
/// exist. Any other name, a folder or a link is refused and left alone.
pub fn remove_shaped(path: &Path, shape: impl Fn(&str) -> bool) -> Result<bool> {
    guard::check()?;
    if !path.file_name().and_then(OsStr::to_str).is_some_and(shape) {
        return Err(io_err("remove", path)(io::Error::new(io::ErrorKind::InvalidInput, "not a cache file name")));
    }
    match plain_meta(path)? {
        None => return Ok(false),
        Some(m) if !m.is_file() => return Err(io_err("remove", path)(io::Error::other("not a regular file"))),
        Some(_) => {}
    }
    fault_at(points::REMOVE, "remove", path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_err("remove", path)(e)),
    }
}
