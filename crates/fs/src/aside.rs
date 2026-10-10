//! Setting damaged files aside (`data-model.md` §3, `security.md` §4.2): a sidecar, settings file
//! or index that cannot be read is renamed to `<name>.<tag>` (e.g. `.dalica.json.corrupt-20261010T051500Z`)
//! and kept, never deleted. If that name is taken, `-001`, `-002`, … are tried.

use std::path::{Path, PathBuf};

use crate::{Error, Result, guard, io_err};

/// Renames `path` to the first free `<name>.<tag>[-NNN]` in the same folder and returns the new
/// path. A final symbolic link is refused. The free-name check and the rename are two steps; the
/// library lock keeps other Dalica processes out of that gap.
pub fn set_aside(path: &Path, tag: &str) -> Result<PathBuf> {
    guard::check()?;
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => return Err(Error::Link(path.to_path_buf())),
        Ok(_) => {}
        Err(e) => return Err(io_err("set aside", path)(e)),
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = format!("{name}.{tag}");
    // The number goes last, so every set-aside copy starts with `<name>.<tag>`.
    let candidates = std::iter::once(base.clone()).chain((1..=9999).map(|n| format!("{base}-{n:03}")));
    for candidate in candidates {
        let target = path.with_file_name(&candidate);
        match std::fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::rename(path, &target).map_err(io_err("set aside", path))?;
                return Ok(target);
            }
            _ => continue,
        }
    }
    Err(io_err("set aside", path)(std::io::Error::other("no free name")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_file_under_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".dalica.json");
        std::fs::write(&p, b"{broken").unwrap();
        let a = set_aside(&p, "corrupt-20261010T051500Z").unwrap();
        assert_eq!(a.file_name().unwrap(), ".dalica.json.corrupt-20261010T051500Z");
        assert!(!p.exists());
        std::fs::write(&p, b"{broken again").unwrap();
        let b = set_aside(&p, "corrupt-20261010T051500Z").unwrap();
        assert_eq!(b.file_name().unwrap(), ".dalica.json.corrupt-20261010T051500Z-001");
        assert_eq!(std::fs::read(&a).unwrap(), b"{broken");
        assert_eq!(std::fs::read(&b).unwrap(), b"{broken again");
        assert!(matches!(set_aside(&p, "x"), Err(Error::Io { .. })));
        let _ui = guard::mark_ui_thread();
        assert!(matches!(set_aside(&a, "x"), Err(Error::OnUiThread)));
    }
}
