//! Atomic replace with a read-before-write identity check (`security.md` §5.2, §5.3, §5.8).
//!
//! [`read`] returns a file's bytes with its [`Identity`]; [`replace`] writes new bytes only if the
//! target still has the identity the caller read (or is still absent), through a temporary file in
//! the same folder:
//!
//! 1. check the target against [`Expect`];
//! 2. create `.<name>.dalica-tmp-<16 hex>` next to it with exclusive creation;
//! 3. write, `fsync`, copy the target's permissions;
//! 4. check the target again; if it changed, remove the temporary file and refuse;
//! 5. rename over the target, then `fsync` the folder (Unix);
//! 6. re-read the target and compare.
//!
//! A failure before step 5 leaves the target untouched; after it, the target holds the new bytes.
//! A crash leaves at most a stray temporary file ([`is_temp_name`]). Writes never go through a
//! symbolic link (§5.6): a target that is a link is refused with [`Error::Link`].

use std::collections::hash_map::RandomState;
use std::ffi::OsStr;
use std::fs::{self, File, Metadata, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use crate::faults::{self, Fault, points};
use crate::{Error, Result, guard, io_err};

/// What identifies one version of a file: device, inode (creation time on Windows, where the file
/// index is not available on stable Rust), size and modification time. A rewrite through a rename,
/// an append or an in-place edit changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime_ns: i128,
}

impl Identity {
    pub fn of(meta: &Metadata) -> Identity {
        let mtime_ns = meta.modified().ok().map_or(0, |t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
            Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
        });
        let (dev, ino) = dev_ino(meta);
        Identity { dev, ino, size: meta.len(), mtime_ns }
    }

    pub fn size(&self) -> u64 {
        self.size
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
    let old = check_target(path, expect)?;

    // 2. The temporary file.
    match faults::hit(points::CREATE_TEMP) {
        Some(Fault::Error) => return Err(io_err("create", dir)(faults::injected(points::CREATE_TEMP))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::CREATE_TEMP)),
        None => {}
    }
    let (tmp, mut file) = create_temp(dir, name)?;
    let cleanup = |e: Error| {
        let _ = fs::remove_file(&tmp);
        e
    };

    // 3. Write, fsync, permissions.
    if let Some(fault) = faults::hit(points::WRITE) {
        let half = bytes.get(..bytes.len() / 2).unwrap_or_default();
        let _ = file.write_all(half);
        return Err(match fault {
            Fault::Error => cleanup(io_err("write", &tmp)(faults::injected(points::WRITE))),
            Fault::Crash => Error::Crashed(points::WRITE),
        });
    }
    file.write_all(bytes).map_err(|e| cleanup(io_err("write", &tmp)(e)))?;
    match faults::hit(points::FSYNC) {
        Some(Fault::Error) => return Err(cleanup(io_err("fsync", &tmp)(faults::injected(points::FSYNC)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::FSYNC)),
        None => {}
    }
    file.sync_all().map_err(|e| cleanup(io_err("fsync", &tmp)(e)))?;
    if let Some(old) = &old {
        // Best effort: some file systems (FAT, some shares) have no permission bits to copy.
        let _ = file.set_permissions(old.permissions());
    }
    drop(file);

    // 4. The target must still be what the caller read.
    check_target(path, expect).map_err(cleanup)?;

    // 5. Rename over the target, then make the rename durable.
    match faults::hit(points::RENAME) {
        Some(Fault::Error) => return Err(cleanup(io_err("rename", path)(faults::injected(points::RENAME)))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::RENAME)),
        None => {}
    }
    fs::rename(&tmp, path).map_err(|e| cleanup(io_err("rename", path)(e)))?;
    match faults::hit(points::FSYNC_DIR) {
        Some(Fault::Error) => return Err(io_err("fsync", dir)(faults::injected(points::FSYNC_DIR))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::FSYNC_DIR)),
        None => {}
    }
    sync_dir(dir)?;

    // 6. Verify.
    match faults::hit(points::VERIFY) {
        Some(Fault::Error) => return Err(io_err("verify", path)(faults::injected(points::VERIFY))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::VERIFY)),
        None => {}
    }
    let mut back = File::open(path).map_err(io_err("verify", path))?;
    let meta = back.metadata().map_err(io_err("verify", path))?;
    let mut read_back = Vec::with_capacity(bytes.len());
    back.read_to_end(&mut read_back).map_err(io_err("verify", path))?;
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

/// Checks the target against `expect`; returns its metadata when it exists.
fn check_target(path: &Path, expect: Expect) -> Result<Option<Metadata>> {
    let meta = target_meta(path)?;
    let ok = match (&meta, expect) {
        (None, Expect::Absent) => true,
        (Some(m), Expect::Unchanged(id)) => Identity::of(m) == id,
        _ => false,
    };
    if ok { Ok(meta) } else { Err(Error::Changed(path.to_path_buf())) }
}

fn create_temp(dir: &Path, name: &OsStr) -> Result<(PathBuf, File)> {
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
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(io_err("create", &tmp)(e)),
        }
    }
    Err(io_err("create", dir)(last.unwrap_or_else(|| io::Error::other("no free temporary name"))))
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

/// Makes a rename durable: `fsync` the folder on Unix. Windows journals renames itself and cannot
/// open a folder as a file.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir).and_then(|d| d.sync_all()).map_err(io_err("fsync", dir))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<()> {
    Ok(())
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
        let (tmp, _f) = create_temp(dir.path(), OsStr::new(&"x".repeat(255))).unwrap();
        let n = tmp.file_name().unwrap();
        assert!(n.len() <= 255 && is_temp_name(n));
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
