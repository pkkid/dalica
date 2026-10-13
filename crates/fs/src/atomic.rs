//! Atomic replace with a read-before-write identity check (`security.md` §5.2, §5.3, §5.8).
//!
//! [`read`] returns a file's bytes with its [`Identity`]; [`replace`] writes new bytes only if the
//! target still has the identity the caller read (or is still absent), through a temporary file in
//! the same folder:
//!
//! 0. validate the target's folder and open it once (`dirfd`, §5.6); every later step names files
//!    relative to that held folder, so a folder swapped for a link after validation cannot
//!    redirect the write (Unix: a directory handle; Windows: the folder's identity re-checked
//!    before each step);
//! 1. check the target against [`Expect`];
//! 2. create `.<name>.dalica-tmp-<16 hex>` next to it with exclusive creation;
//! 3. write, give it the times to keep (if asked: [`replace_keeping_times`]), `fsync`, copy the
//!    target's permissions;
//! 4. check the target again; if it changed, remove the temporary file and refuse;
//! 5. rename over the target, then `fsync` the folder (Unix);
//! 6. re-read the target (never through a link) and compare.
//!
//! A failure before step 5 leaves the target untouched; after it, the target holds the new bytes.
//! A crash leaves at most a stray temporary file ([`is_temp_name`]; `stale` removes it a day
//! later). Writes never go through a symbolic link (§5.6): a target that is a link is refused with
//! [`Error::Link`].
//!
//! [`replace_keeping_times`] sets the times on the temporary file before the rename, so the file
//! appears under the target's name already carrying them: a crash at any point leaves the old file
//! with its old time or the new file with the old time (never the new file with the time of the
//! write). A platform or file system that refuses the times on the temporary file is not an error
//! there; the caller compares the returned identity's time and puts it back after the rename.

use std::collections::hash_map::RandomState;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::dirfd::{Entry, Folder, Kind};
use crate::faults::{self, Fault, points};
use crate::times::Times;
use crate::{Error, Result, guard, io_err};

/// Fault point: giving the temporary file the times to keep ([`replace_keeping_times`]), between
/// writing it and its `fsync`. An error is taken as a platform that refuses: the write goes on
/// without the times (the caller puts them back after the rename).
pub const KEEP_TIMES: &str = "atomic.keep_times";

/// The points of [`replace_keeping_times`], in the order it reaches them.
pub const KEEPING_TIMES_POINTS: [&str; 8] =
    [points::OPEN_DIR, points::CREATE_TEMP, points::WRITE, KEEP_TIMES, points::FSYNC, points::RENAME, points::FSYNC_DIR, points::VERIFY];

/// What identifies one version of a file: device, inode (creation time on Windows, where the file
/// index is not available on stable Rust), size and modification time. A rewrite through a rename,
/// an append or an in-place edit changes it, unless it keeps both the size and the modification
/// time: an in-place edit that puts the time back keeps the inode anywhere, and on Windows NTFS
/// "tunnels" the old creation time onto a file renamed over the same name within about 15
/// seconds, so a same-size [`replace_keeping_times`] can leave the identity as it was there. It is
/// a read-before-write guard against other programs' ordinary rewrites, not a content hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime_ns: i128,
}

impl Identity {
    pub fn of(meta: &Metadata) -> Identity {
        let mtime_ns = meta.modified().ok().map_or(0, nanos_since_epoch);
        let (dev, ino) = dev_ino(meta);
        Identity { dev, ino, size: meta.len(), mtime_ns }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    #[cfg(unix)]
    pub(crate) fn from_parts(dev: u64, ino: u64, size: u64, mtime_ns: i128) -> Identity {
        Identity { dev, ino, size, mtime_ns }
    }

    /// The same file or folder (device and inode), whatever its size and time now.
    pub(crate) fn same_object(&self, other: &Identity) -> bool {
        (self.dev, self.ino) == (other.dev, other.ino)
    }

    /// Modification time in nanoseconds since the Unix epoch (negative before it).
    pub(crate) fn mtime_ns(&self) -> i128 {
        self.mtime_ns
    }

    /// Whether this version's modification time is `time` (to the nanosecond the platform keeps).
    pub fn modified_at(&self, time: SystemTime) -> bool {
        self.mtime_ns == nanos_since_epoch(time)
    }
}

fn nanos_since_epoch(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

#[cfg(unix)]
fn dev_ino(meta: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

#[cfg(windows)]
fn dev_ino(meta: &Metadata) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (0, meta.creation_time())
}

#[cfg(not(any(unix, windows)))]
fn dev_ino(_meta: &Metadata) -> (u64, u64) {
    (0, 0)
}

/// A file's content together with the identity it had while it was read.
#[derive(Clone, Debug)]
pub struct Read {
    pub bytes: Vec<u8>,
    pub identity: Identity,
}

/// What the target of [`replace`] must still be when it is replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// It must not exist (a new file).
    Absent,
    /// It must still be the version the caller read.
    Unchanged(Identity),
}

/// Reads `path` whole, refusing files larger than `limit` bytes. `Ok(None)` when it does not exist.
/// A final symbolic link is refused, and so is a file that changed while it was being read.
pub fn read(path: &Path, limit: u64) -> Result<Option<Read>> {
    guard::check()?;
    let Some(before) = target_meta(path)? else { return Ok(None) };
    if before.is_dir() {
        return Err(Error::Io { op: "read", path: path.to_path_buf(), source: io::Error::other("is a folder") });
    }
    let file = File::open(path).map_err(io_err("open", path))?;
    let opened = file.metadata().map_err(io_err("stat", path))?;
    // The path was swapped (for a link or another file) between the check and the open.
    if Identity::of(&opened) != Identity::of(&before) {
        return Err(Error::Changed(path.to_path_buf()));
    }
    if opened.len() > limit {
        return Err(Error::TooLarge { path: path.to_path_buf(), limit });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(io_err("read", path))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Error::TooLarge { path: path.to_path_buf(), limit });
    }
    let after = fs::symlink_metadata(path).map_err(io_err("stat", path))?;
    if Identity::of(&after) != Identity::of(&opened) {
        return Err(Error::Changed(path.to_path_buf()));
    }
    Ok(Some(Read { bytes, identity: Identity::of(&opened) }))
}

/// The identity of `path` without following a final link, or `None` when it does not exist.
pub fn identity(path: &Path) -> Result<Option<Identity>> {
    guard::check()?;
    Ok(target_meta(path)?.as_ref().map(Identity::of))
}

/// Atomically replaces (or creates) `path` with `bytes`, if it still matches `expect`. Returns the
/// new file's identity, to pass as the `expect` of the next write.
pub fn replace(path: &Path, bytes: &[u8], expect: Expect) -> Result<Identity> {
    replace_with(path, bytes, expect, false, None)
}

/// [`replace`] that keeps `times` (a photo's modification time across a metadata write,
/// `data-model.md` §2): the temporary file gets them before it is renamed over the target, so no
/// crash can leave the new bytes with the time of the write. Returns the new file's identity;
/// where the platform did not keep the time ([`Identity::modified_at`] says no), the caller puts
/// it back with [`set_times`](crate::set_times). Fault point [`KEEP_TIMES`].
pub fn replace_keeping_times(path: &Path, bytes: &[u8], expect: Expect, times: &Times) -> Result<Identity> {
    replace_with(path, bytes, expect, false, Some(times))
}

/// [`replace`] for a file only its owner may read (a credential such as the control token,
/// `security.md` §6.3): the temporary file is created with mode `0600` on Unix, so the bytes are
/// never readable by anyone else, not even for a moment. Elsewhere the file gets its folder's
/// permissions.
pub fn replace_private(path: &Path, bytes: &[u8], expect: Expect) -> Result<Identity> {
    replace_with(path, bytes, expect, true, None)
}

fn replace_with(path: &Path, bytes: &[u8], expect: Expect, private: bool, keep: Option<&Times>) -> Result<Identity> {
    guard::check()?;
    let name = path.file_name().ok_or_else(|| Error::Io {
        op: "replace",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "no file name"),
    })?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // 0. The folder, validated and held; everything below is relative to it.
    let folder = Folder::open(dir)?;
    // 1. The target must be what the caller read.
    let old = check_target(&folder, name, path, expect)?;

    // 2. The temporary file.
    match faults::hit(points::CREATE_TEMP) {
        Some(Fault::Error) => return Err(io_err("create", dir)(faults::injected(points::CREATE_TEMP))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::CREATE_TEMP)),
        None => {}
    }
    let (tmp, mut file) = create_temp(&folder, name, private)?;
    let tmp_path = folder.join(&tmp);
    let cleanup = |e: Error| {
        let _ = folder.remove(&tmp);
        e
    };

    // 3. Write, fsync, permissions.
    if let Some(fault) = faults::hit(points::WRITE) {
        let half = bytes.get(..bytes.len() / 2).unwrap_or_default();
        let _ = file.write_all(half);
        return Err(match fault {
            Fault::Error => cleanup(io_err("write", &tmp_path)(faults::injected(points::WRITE))),
            Fault::Crash => Error::Crashed(points::WRITE),
        });
    }
    file.write_all(bytes).map_err(|e| cleanup(io_err("write", &tmp_path)(e)))?;
    if let Some(times) = keep {
        // Before the `fsync`, so the times are as durable as the bytes. Best effort, like the
        // permissions: a file system that refuses them leaves the caller's fallback to put the time
        // back after the rename (an injected error stands for that).
        match faults::hit(KEEP_TIMES) {
            Some(Fault::Crash) => return Err(Error::Crashed(KEEP_TIMES)),
            Some(Fault::Error) => {}
            None => {
                let _ = file.set_times(times.file_times());
            }
        }
    }
    match faults::hit(points::FSYNC) {
        Some(Fault::Error) => return Err(cleanup(io_err("fsync", &tmp_path)(faults::injected(points::FSYNC)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::FSYNC)),
        None => {}
    }
    file.sync_all().map_err(|e| cleanup(io_err("fsync", &tmp_path)(e)))?;
    if let Some(old) = old.as_ref().filter(|_| !private) {
        // Best effort: some file systems (FAT, some shares) have no permission bits to copy.
        let _ = Folder::copy_permissions(&file, old);
    }
    drop(file);

    // 4. The target must still be what the caller read.
    check_target(&folder, name, path, expect).map_err(cleanup)?;

    // 5. Rename over the target, then make the rename durable.
    match faults::hit(points::RENAME) {
        Some(Fault::Error) => return Err(cleanup(io_err("rename", path)(faults::injected(points::RENAME)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::RENAME)),
        None => {}
    }
    folder.rename(&tmp, name).map_err(cleanup)?;
    match faults::hit(points::FSYNC_DIR) {
        Some(Fault::Error) => return Err(io_err("fsync", dir)(faults::injected(points::FSYNC_DIR))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::FSYNC_DIR)),
        None => {}
    }
    folder.sync()?;

    // 6. Verify, reading the target back through the held folder without following a link.
    match faults::hit(points::VERIFY) {
        Some(Fault::Error) => return Err(io_err("verify", path)(faults::injected(points::VERIFY))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::VERIFY)),
        None => {}
    }
    let back = folder.open_read(name)?;
    let meta = back.metadata().map_err(io_err("verify", path))?;
    let mut read_back = Vec::with_capacity(bytes.len());
    // One byte past the expected length is enough to tell a longer file apart.
    back.take(u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(1)).read_to_end(&mut read_back).map_err(io_err("verify", path))?;
    if read_back != bytes {
        return Err(Error::VerifyFailed(path.to_path_buf()));
    }
    Ok(Identity::of(&meta))
}

/// Whether `name` has the shape of this module's temporary files: `.<name>.dalica-tmp-<16 hex>`.
/// Cleanup of stale temporaries removes only files of exactly this shape (`security.md` §5.8).
pub fn is_temp_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else { return false };
    let Some(rest) = name.strip_prefix('.') else { return false };
    match rest.rsplit_once(TEMP_MARK) {
        Some((target, hex)) => !target.is_empty() && hex.len() == 16 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        None => false,
    }
}

const TEMP_MARK: &str = ".dalica-tmp-";
/// Longest part of the target's name kept in a temporary name, so the whole stays within 255 bytes.
const TEMP_NAME_KEEP: usize = 255 - 1 - TEMP_MARK.len() - 16;

/// Metadata of `path` without following a final link; `None` if it does not exist.
fn target_meta(path: &Path) -> Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(Error::Link(path.to_path_buf())),
        Ok(m) => Ok(Some(m)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err("stat", path)(e)),
    }
}

/// Checks the target `name` in `folder` (at `path`) against `expect`, without following a link;
/// returns what it is when it exists.
fn check_target(folder: &Folder, name: &OsStr, path: &Path, expect: Expect) -> Result<Option<Entry>> {
    let entry = folder.entry(name)?;
    if entry.as_ref().is_some_and(|e| e.kind == Kind::Link) {
        return Err(Error::Link(path.to_path_buf()));
    }
    let ok = match (&entry, expect) {
        (None, Expect::Absent) => true,
        (Some(e), Expect::Unchanged(id)) => e.identity == id,
        _ => false,
    };
    if ok { Ok(entry) } else { Err(Error::Changed(path.to_path_buf())) }
}

/// Creates a new temporary file for `name` in `folder`; returns its name and the open file.
fn create_temp(folder: &Folder, name: &OsStr, private: bool) -> Result<(OsString, File)> {
    let lossy = name.to_string_lossy();
    let mut keep = lossy.len().min(TEMP_NAME_KEEP);
    while !lossy.is_char_boundary(keep) {
        keep -= 1;
    }
    let stem = lossy.get(..keep).unwrap_or_default();
    for _ in 0..16 {
        let tmp = OsString::from(format!(".{stem}{TEMP_MARK}{}", random_hex()));
        if let Some(f) = folder.create_new(&tmp, private)? {
            return Ok((tmp, f));
        }
    }
    Err(io_err("create", folder.path())(io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")))
}

/// 16 hex digits from the standard library's per-process random hash keys and a counter.
fn random_hex() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
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

    const LIMIT: u64 = 1 << 20;

    fn temps(dir: &Path) -> Vec<String> {
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name()).filter(|n| is_temp_name(n)).map(|n| n.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn create_then_replace_then_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".dalica.json");
        assert!(read(&p, LIMIT).unwrap().is_none());
        let id1 = replace(&p, b"one", Expect::Absent).unwrap();
        let r = read(&p, LIMIT).unwrap().unwrap();
        assert_eq!((r.bytes.as_slice(), r.identity), (&b"one"[..], id1));
        let id2 = replace(&p, b"two two", Expect::Unchanged(id1)).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(fs::read(&p).unwrap(), b"two two");
        assert_eq!(identity(&p).unwrap(), Some(id2));
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn a_file_changed_between_read_and_write_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("photo.json");
        let id = replace(&p, b"mine", Expect::Absent).unwrap();
        // Another program rewrites it (new inode, size and time).
        fs::write(dir.path().join("other"), b"theirs!").unwrap();
        fs::rename(dir.path().join("other"), &p).unwrap();
        assert!(matches!(replace(&p, b"mine 2", Expect::Unchanged(id)), Err(Error::Changed(_))));
        assert_eq!(fs::read(&p).unwrap(), b"theirs!");
        // In-place edit of the same size.
        let id = read(&p, LIMIT).unwrap().unwrap().identity;
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::OpenOptions::new().write(true).open(&p).unwrap().write_all(b"THEIRS!").unwrap();
        assert!(matches!(replace(&p, b"x", Expect::Unchanged(id)), Err(Error::Changed(_))));
        // Deleted, or created behind our back.
        let id = read(&p, LIMIT).unwrap().unwrap().identity;
        fs::remove_file(&p).unwrap();
        assert!(matches!(replace(&p, b"x", Expect::Unchanged(id)), Err(Error::Changed(_))));
        fs::write(&p, b"new").unwrap();
        assert!(matches!(replace(&p, b"x", Expect::Absent), Err(Error::Changed(_))));
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn read_refuses_files_over_the_limit_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        replace(&p, &[7u8; 100], Expect::Absent).unwrap();
        assert!(matches!(read(&p, 99), Err(Error::TooLarge { limit: 99, .. })));
        assert_eq!(read(&p, 100).unwrap().unwrap().bytes.len(), 100);
        assert!(matches!(read(dir.path(), LIMIT), Err(Error::Io { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn writes_never_go_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.json");
        fs::write(&real, b"real").unwrap();
        let link = dir.path().join(".dalica.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(read(&link, LIMIT), Err(Error::Link(_))));
        assert!(matches!(identity(&link), Err(Error::Link(_))));
        let id = Identity::of(&fs::metadata(&real).unwrap());
        assert!(matches!(replace(&link, b"x", Expect::Unchanged(id)), Err(Error::Link(_))));
        assert!(matches!(replace(&link, b"x", Expect::Absent), Err(Error::Link(_))));
        assert_eq!(fs::read(&real).unwrap(), b"real");
    }

    #[cfg(unix)]
    #[test]
    fn replace_keeps_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        let id = replace(&p, b"a", Expect::Absent).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        let id = identity(&p).unwrap().unwrap_or(id);
        replace(&p, b"b", Expect::Unchanged(id)).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_replace_is_owner_only_from_the_start_and_stays_so() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret");
        let id = replace_private(&p, b"a", Expect::Absent).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        // An existing file's looser bits are not copied onto a private replacement.
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        let id = identity(&p).unwrap().unwrap_or(id);
        replace_private(&p, b"b", Expect::Unchanged(id)).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&p).unwrap(), b"b");
    }

    #[test]
    fn a_private_replace_checks_the_target_like_replace() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("secret");
        replace_private(&p, b"a", Expect::Absent).unwrap();
        assert!(matches!(replace_private(&p, b"b", Expect::Absent), Err(Error::Changed(_))));
        assert_eq!(fs::read(&p).unwrap(), b"a");
    }

    #[test]
    fn names_with_awkward_characters_and_great_length_work() {
        let dir = tempfile::tempdir().unwrap();
        let long = "é".repeat(127);
        // Windows forbids `"` in file names.
        let names = ["a]b.json", "say \"hi\".json", "a;b=c.json", "Lisboa à noite 東京 🌊.json", &long];
        for name in names.into_iter().filter(|n| cfg!(unix) || !n.contains('"')) {
            let p = dir.path().join(name);
            replace(&p, name.as_bytes(), Expect::Absent).unwrap();
            assert_eq!(read(&p, LIMIT).unwrap().unwrap().bytes, name.as_bytes());
        }
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn temp_names_have_one_recognisable_shape() {
        assert!(is_temp_name(OsStr::new(".photo.jpg.dalica-tmp-0123456789abcdef")));
        for other in [
            "photo.jpg",
            ".photo.jpg.dalica-tmp-0123",
            "photo.jpg.dalica-tmp-0123456789abcdef",
            "..dalica-tmp-0123456789abcdef",
            ".x.dalica-tmp-0123456789ABCDEF",
        ] {
            assert!(!is_temp_name(OsStr::new(other)), "{other}");
        }
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::open(dir.path()).unwrap();
        let (n, _f) = create_temp(&folder, OsStr::new(&"x".repeat(255)), false).unwrap();
        assert!(n.len() <= 255 && is_temp_name(&n));
    }

    #[test]
    fn every_call_on_the_ui_thread_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        let id = replace(&p, b"a", Expect::Absent).unwrap();
        let _ui = guard::mark_ui_thread();
        assert!(matches!(read(&p, LIMIT), Err(Error::OnUiThread)));
        assert!(matches!(identity(&p), Err(Error::OnUiThread)));
        assert!(matches!(replace(&p, b"b", Expect::Unchanged(id)), Err(Error::OnUiThread)));
        drop(_ui);
        assert_eq!(fs::read(&p).unwrap(), b"a");
    }

    /// Every fault point, as an error and as a crash, for a new file and a replaced one: the
    /// target holds exactly the old or the new bytes (`data-model.md` §10.5).
    #[cfg(feature = "faults")]
    #[test]
    fn each_fault_point_leaves_the_old_or_the_new_file() {
        use crate::faults::{ATOMIC_POINTS, arm, disarm_all};
        let old: &[u8] = b"{\n  \"photos\": { \"a.jpg\": { \"star\": true } }\n}\n";
        let new: &[u8] = b"{\n  \"photos\": { \"a.jpg\": { \"star\": true }, \"b.jpg\": { \"star\": true } }\n}\n";
        for (i, point) in ATOMIC_POINTS.into_iter().enumerate() {
            let after_rename = i > ATOMIC_POINTS.iter().position(|p| *p == points::RENAME).unwrap();
            for fault in [Fault::Error, Fault::Crash] {
                for existed in [true, false] {
                    let dir = tempfile::tempdir().unwrap();
                    let p = dir.path().join(".dalica.json");
                    let expect = if existed { Expect::Unchanged(replace(&p, old, Expect::Absent).unwrap()) } else { Expect::Absent };
                    arm(point, fault);
                    let err = replace(&p, new, expect).unwrap_err();
                    disarm_all();
                    let case = format!("{point} {fault:?} existed={existed}: {err}");
                    match fault {
                        Fault::Crash => assert!(matches!(err, Error::Crashed(p) if p == point), "{case}"),
                        Fault::Error => assert!(matches!(err, Error::Io { .. }), "{case}"),
                    }
                    let now = fs::read(&p).ok();
                    let want = if after_rename {
                        Some(new)
                    } else if existed {
                        Some(old)
                    } else {
                        None
                    };
                    assert_eq!(now.as_deref(), want, "{case}");
                    // An error cleans up; a crash leaves at most one temporary file.
                    let left = temps(dir.path()).len();
                    match fault {
                        Fault::Error => assert_eq!(left, 0, "{case}"),
                        Fault::Crash => assert!(left <= 1, "{case}"),
                    }
                    // The next write, after re-reading, succeeds and is clean.
                    let expect = match identity(&p).unwrap() {
                        Some(id) => Expect::Unchanged(id),
                        None => Expect::Absent,
                    };
                    replace(&p, b"after", expect).unwrap();
                    assert_eq!(fs::read(&p).unwrap(), b"after", "{case}");
                }
            }
        }
    }
}
