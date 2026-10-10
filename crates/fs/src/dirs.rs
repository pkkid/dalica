//! Creating folders: Dalica's own (logs, the library directory, caches, throwaway folders) and
//! folders the user asked for. Folder creation is a disk write, so it lives here (layer rule 4).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Result, guard, io_err};

/// Creates `path` and any missing parents; succeeds if it already exists as a folder.
pub fn create_dir_all(path: &Path) -> Result<()> {
    guard::check()?;
    std::fs::create_dir_all(path).map_err(io_err("create folder", path))
}

/// Creates `path` and any missing parents as private folders: only their owner may enter the ones
/// it creates (mode 0700 on Unix); folders that already exist keep their permissions. Succeeds if
/// `path` already exists as a folder.
pub fn create_private_dir_all(path: &Path) -> Result<()> {
    guard::check()?;
    let mut builder = private_dir_builder();
    builder.recursive(true);
    builder.create(path).map_err(io_err("create folder", path))
}

/// Removes a file Dalica made for itself and nobody else uses (the control token file at exit). A
/// missing file is fine; a folder is refused. A symbolic link is removed, never what it points to.
pub fn remove_own_file(path: &Path) -> Result<()> {
    guard::check()?;
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err("stat", path)(e)),
        Ok(m) if m.is_dir() => return Err(io_err("remove", path)(std::io::Error::other("is a folder"))),
        Ok(_) => {}
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_err("remove", path)(e)),
        _ => Ok(()),
    }
}

/// Names tried before [`TempDir::new`] gives up.
const TEMP_TRIES: u64 = 1000;
static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// A new, empty folder in the OS temporary folder that Dalica made for itself (`--demo-fresh`),
/// removed with everything in it by [`TempDir::close`] or on drop. It is always a folder this
/// process created (an existing name is never reused), and on Unix only its owner may enter it.
#[derive(Debug)]
#[must_use = "the folder is removed when this is dropped"]
pub struct TempDir {
    path: PathBuf,
    removed: bool,
}

impl TempDir {
    /// Makes `<temp>/<prefix>-<pid>-<n>` with the first `n` whose name is free.
    pub fn new(prefix: &str) -> Result<TempDir> {
        TempDir::new_in(&std::env::temp_dir(), prefix)
    }

    /// Like [`TempDir::new`], in `parent` (which must exist).
    pub fn new_in(parent: &Path, prefix: &str) -> Result<TempDir> {
        guard::check()?;
        let pid = std::process::id();
        let mut last = parent.join(prefix);
        for _ in 0..TEMP_TRIES {
            let n = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!("{prefix}-{pid}-{n}"));
            match private_dir_builder().create(&path) {
                Ok(()) => return Ok(TempDir { path, removed: false }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = path,
                Err(e) => return Err(io_err("create folder", &path)(e)),
            }
        }
        Err(io_err("create folder", &last)(std::io::Error::other("no free name")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Removes the folder and everything in it now, and says if that failed (a drop cannot).
    pub fn close(mut self) -> Result<()> {
        self.remove()
    }

    fn remove(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        guard::check()?;
        // `remove_dir_all` removes symbolic links inside the folder, never what they point to.
        match std::fs::remove_dir_all(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("remove folder", &self.path)(e)),
        }
        self.removed = true;
        Ok(())
    }
}

/// A folder builder whose folders only their owner may enter (mode 0700 on Unix).
#[cfg(unix)]
fn private_dir_builder() -> std::fs::DirBuilder {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder
}

/// A folder builder; elsewhere the folder gets the platform's default permissions.
#[cfg(not(unix))]
fn private_dir_builder() -> std::fs::DirBuilder {
    std::fs::DirBuilder::new()
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn creates_nested_folders_and_accepts_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/c");
        create_dir_all(&p).unwrap();
        create_dir_all(&p).unwrap();
        assert!(p.is_dir());
        std::fs::write(dir.path().join("file"), b"").unwrap();
        assert!(matches!(create_dir_all(&dir.path().join("file/sub")), Err(Error::Io { .. })));
        let _ui = guard::mark_ui_thread();
        assert!(matches!(create_dir_all(&p), Err(Error::OnUiThread)));
    }

    #[test]
    fn private_folders_are_created_with_their_parents() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a").join("b");
        create_private_dir_all(&p).unwrap();
        create_private_dir_all(&p).unwrap();
        assert!(p.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for d in [dir.path().join("a"), p.clone()] {
                assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700, "{}", d.display());
            }
        }
        std::fs::write(dir.path().join("file"), b"").unwrap();
        assert!(matches!(create_private_dir_all(&dir.path().join("file")), Err(Error::Io { .. })));
        let _ui = guard::mark_ui_thread();
        assert!(matches!(create_private_dir_all(&p), Err(Error::OnUiThread)));
    }

    #[test]
    fn an_own_file_is_removed_a_missing_one_is_fine_a_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("control-1.token");
        std::fs::write(&f, b"x").unwrap();
        remove_own_file(&f).unwrap();
        assert!(!f.exists());
        remove_own_file(&f).unwrap();
        assert!(matches!(remove_own_file(dir.path()), Err(Error::Io { .. })));
        assert!(dir.path().is_dir());
        std::fs::write(&f, b"x").unwrap();
        let ui = guard::mark_ui_thread();
        assert!(matches!(remove_own_file(&f), Err(Error::OnUiThread)));
        drop(ui);
        assert!(f.exists());
    }

    #[test]
    fn a_temp_dir_is_new_and_removed_with_its_contents() {
        let parent = tempfile::tempdir().unwrap();
        let a = TempDir::new_in(parent.path(), "demo").unwrap();
        let b = TempDir::new_in(parent.path(), "demo").unwrap();
        assert_ne!(a.path(), b.path());
        assert!(a.path().is_dir() && a.path().starts_with(parent.path()));
        assert!(std::fs::read_dir(a.path()).unwrap().next().is_none(), "starts empty");
        std::fs::create_dir_all(a.path().join("x").join("y")).unwrap();
        std::fs::write(a.path().join("x").join("y").join("f"), b"1").unwrap();
        let (pa, pb) = (a.path().to_path_buf(), b.path().to_path_buf());
        a.close().unwrap();
        assert!(!pa.exists());
        drop(b);
        assert!(!pb.exists());
        // Already gone (someone else removed it): still fine.
        let c = TempDir::new_in(parent.path(), "demo").unwrap();
        std::fs::remove_dir(c.path()).unwrap();
        c.close().unwrap();
    }

    #[test]
    fn a_temp_dir_never_reuses_an_existing_folder() {
        let parent = tempfile::tempdir().unwrap();
        // The next names are taken, as a crashed run with the same process id would leave them.
        let (n, pid) = (TEMP_SERIAL.load(Ordering::Relaxed), std::process::id());
        for k in n..n + 5 {
            let old = parent.path().join(format!("demo-{pid}-{k}"));
            std::fs::create_dir(&old).unwrap();
            std::fs::write(old.join("old"), b"").unwrap();
        }
        let d = TempDir::new_in(parent.path(), "demo").unwrap();
        assert!(std::fs::read_dir(d.path()).unwrap().next().is_none());
        // A missing parent is an error.
        assert!(matches!(TempDir::new_in(&parent.path().join("missing"), "demo"), Err(Error::Io { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn a_temp_dir_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let d = TempDir::new_in(parent.path(), "demo").unwrap();
        assert_eq!(std::fs::metadata(d.path()).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn a_temp_dir_is_refused_on_the_ui_thread() {
        let parent = tempfile::tempdir().unwrap();
        let d = TempDir::new_in(parent.path(), "demo").unwrap();
        let path = d.path().to_path_buf();
        let ui = guard::mark_ui_thread();
        assert!(matches!(TempDir::new_in(parent.path(), "demo"), Err(Error::OnUiThread)));
        assert!(matches!(d.close(), Err(Error::OnUiThread)));
        drop(ui);
        // The refused close (and its drop) left the folder.
        assert!(path.is_dir());
    }
}
