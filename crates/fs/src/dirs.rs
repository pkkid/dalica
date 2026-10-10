//! Creating folders: Dalica's own (logs, the library directory, caches) and folders the user asked
//! for. Folder creation is a disk write, so it lives here (layer rule 4).

use std::path::Path;

use crate::{Result, guard, io_err};

/// Creates `path` and any missing parents; succeeds if it already exists as a folder.
pub fn create_dir_all(path: &Path) -> Result<()> {
    guard::check()?;
    std::fs::create_dir_all(path).map_err(io_err("create folder", path))
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
}
