//! Append-only, size-capped files with rotation, for Dalica's own logs (`development.md` §10), so
//! the app's logger writes through `fs` like everything else (layer rule 4).
//!
//! An append that would take the file past its cap writes nothing and returns
//! [`Error::CapExceeded`], so a file never grows past its cap. Each append is followed by an
//! `fsync`, so the log of a crashed run survives. [`rotate`] shifts `name.log` → `name.1.log` →
//! `name.2.log` (the oldest is dropped) before a new run starts writing.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::faults::{self, Fault, points};
use crate::{Error, Result, guard, io_err};

/// An open append-only file with a size cap.
#[derive(Debug)]
pub struct Appender {
    file: File,
    path: PathBuf,
    size: u64,
    cap: u64,
}

impl Appender {
    /// Opens (or creates) `path` for appending. A final symbolic link is refused.
    pub fn open(path: &Path, cap: u64) -> Result<Appender> {
        guard::check()?;
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() => return Err(Error::Link(path.to_path_buf())),
            _ => {}
        }
        let file = OpenOptions::new().append(true).create(true).open(path).map_err(io_err("open", path))?;
        let size = file.metadata().map_err(io_err("stat", path))?.len();
        Ok(Appender { file, path: path.to_path_buf(), size, cap })
    }

    /// Appends `bytes` and syncs them to disk; refuses (writing nothing) if the file would pass its cap.
    pub fn append(&mut self, bytes: &[u8]) -> Result<()> {
        guard::check()?;
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if self.size.checked_add(len).is_none_or(|end| end > self.cap) {
            return Err(Error::CapExceeded { path: self.path.clone(), cap: self.cap });
        }
        if let Some(fault) = faults::hit(points::APPEND_WRITE) {
            let half = bytes.get(..bytes.len() / 2).unwrap_or_default();
            let _ = self.file.write_all(half);
            self.resync_size();
            return Err(match fault {
                Fault::Error => io_err("append", &self.path)(faults::injected(points::APPEND_WRITE)),
                Fault::Crash => Error::Crashed(points::APPEND_WRITE),
            });
        }
        if let Err(e) = self.file.write_all(bytes) {
            self.resync_size();
            return Err(io_err("append", &self.path)(e));
        }
        self.size += len;
        match faults::hit(points::APPEND_FSYNC) {
            Some(Fault::Error) => return Err(io_err("fsync", &self.path)(faults::injected(points::APPEND_FSYNC))),
            Some(Fault::Crash) => return Err(Error::Crashed(points::APPEND_FSYNC)),
            None => {}
        }
        self.file.sync_data().map_err(io_err("fsync", &self.path))
    }

    /// The file's size in bytes as far as this appender knows.
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn cap(&self) -> u64 {
        self.cap
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// After a partial write the size is unknown: ask the file.
    fn resync_size(&mut self) {
        if let Ok(m) = self.file.metadata() {
            self.size = m.len();
        }
    }
}

/// Appends `bytes` to `path` (created if missing) under `cap`; returns the new size.
pub fn append(path: &Path, bytes: &[u8], cap: u64) -> Result<u64> {
    let mut a = Appender::open(path, cap)?;
    a.append(bytes)?;
    Ok(a.size())
}

/// Shifts `name.ext` → `name.1.ext` → … → `name.<keep>.ext`, dropping the oldest; missing files are
/// skipped. With `keep == 0` the file is just removed. Afterwards `path` does not exist.
pub fn rotate(path: &Path, keep: usize) -> Result<()> {
    guard::check()?;
    if keep == 0 {
        return remove_if_present(path);
    }
    for i in (1..keep).rev() {
        rename_if_present(&numbered(path, i)?, &numbered(path, i + 1)?)?;
    }
    rename_if_present(path, &numbered(path, 1)?)
}

/// `dalica.log` with `n` → `dalica.<n>.log`; `notes` → `notes.<n>`.
fn numbered(path: &Path, n: usize) -> Result<PathBuf> {
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| Error::Io {
        op: "rotate",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "not a UTF-8 file name"),
    })?;
    let name = match dalica_core::names::split_ext(name) {
        (stem, Some(ext)) => format!("{stem}.{n}.{ext}"),
        (stem, None) => format!("{stem}.{n}"),
    };
    Ok(path.with_file_name(name))
}

fn rename_if_present(from: &Path, to: &Path) -> Result<()> {
    match fs::symlink_metadata(from) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err("stat", from)(e)),
        Ok(_) => {}
    }
    match faults::hit(points::ROTATE) {
        Some(Fault::Error) => return Err(io_err("rename", from)(faults::injected(points::ROTATE))),
        Some(Fault::Crash) => return Err(Error::Crashed(points::ROTATE)),
        None => {}
    }
    fs::rename(from, to).map_err(io_err("rename", from))
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err("remove", path)(e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_until_the_cap_and_never_past_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dalica.log");
        let mut a = Appender::open(&p, 10).unwrap();
        a.append(b"12345").unwrap();
        a.append(b"6789").unwrap();
        assert!(matches!(a.append(b"ab"), Err(Error::CapExceeded { cap: 10, .. })));
        a.append(b"0").unwrap();
        assert!(matches!(a.append(b"x"), Err(Error::CapExceeded { .. })));
        assert_eq!(fs::read(&p).unwrap(), b"1234567890");
        // Reopening sees the existing size.
        assert!(matches!(append(&p, b"y", 10), Err(Error::CapExceeded { .. })));
        assert_eq!(append(&p, b"y", 11).unwrap(), 11);
        assert!(fs::metadata(&p).unwrap().len() <= 11);
    }

    #[test]
    fn a_huge_record_is_refused_without_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.log");
        let mut a = Appender::open(&p, u64::MAX).unwrap();
        a.size = u64::MAX - 1;
        assert!(matches!(a.append(b"abc"), Err(Error::CapExceeded { .. })));
    }

    #[test]
    fn rotation_keeps_the_last_runs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dalica.log");
        for run in ["one", "two", "three", "four"] {
            rotate(&p, 2).unwrap();
            assert!(!p.exists());
            append(&p, run.as_bytes(), 1 << 20).unwrap();
        }
        let read = |n: &str| fs::read_to_string(dir.path().join(n)).unwrap();
        assert_eq!(read("dalica.log"), "four");
        assert_eq!(read("dalica.1.log"), "three");
        assert_eq!(read("dalica.2.log"), "two");
        assert!(!dir.path().join("dalica.3.log").exists());
        rotate(&p, 0).unwrap();
        assert!(!p.exists());
        assert_eq!(numbered(Path::new("notes"), 1).unwrap(), PathBuf::from("notes.1"));
    }

    #[cfg(unix)]
    #[test]
    fn appending_through_a_link_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.log");
        fs::write(&real, b"").unwrap();
        let link = dir.path().join("dalica.log");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(append(&link, b"x", 100), Err(Error::Link(_))));
    }

    #[test]
    fn appending_on_the_ui_thread_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.log");
        let mut a = Appender::open(&p, 100).unwrap();
        let _ui = guard::mark_ui_thread();
        assert!(matches!(a.append(b"x"), Err(Error::OnUiThread)));
        assert!(matches!(rotate(&p, 2), Err(Error::OnUiThread)));
        assert!(matches!(Appender::open(&p, 100), Err(Error::OnUiThread)));
    }

    #[cfg(feature = "faults")]
    #[test]
    fn faults_never_take_a_log_past_its_cap() {
        use crate::faults::{APPEND_POINTS, arm, disarm_all};
        for point in APPEND_POINTS {
            for fault in [Fault::Error, Fault::Crash] {
                let dir = tempfile::tempdir().unwrap();
                let p = dir.path().join("dalica.log");
                append(&p, b"first\n", 16).unwrap();
                rotate(&p, 2).unwrap();
                append(&p, b"second\n", 16).unwrap();
                arm(point, fault);
                let result = if point == points::ROTATE { rotate(&p, 2) } else { append(&p, b"third\n", 16).map(|_| ()) };
                disarm_all();
                assert!(result.is_err(), "{point} {fault:?}");
                for name in ["dalica.log", "dalica.1.log"] {
                    let len = fs::metadata(dir.path().join(name)).map_or(0, |m| m.len());
                    assert!(len <= 16, "{point} {fault:?}: {name} is {len} bytes");
                }
                // The file is still usable afterwards, and still capped.
                let mut a = Appender::open(&p, 16).unwrap();
                while a.append(b"z").is_ok() {}
                assert_eq!(fs::metadata(&p).unwrap().len(), 16, "{point} {fault:?}");
            }
        }
    }
}
