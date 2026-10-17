//! The photo viewer's session (`viewing.md` §3, `roadmap.md` M11): the file `dalica --view` opened,
//! its folder's photos in name order, and whether the viewer is read-only because another Dalica
//! holds the library. A stub until M11.12 fills it.

use std::path::{Path, PathBuf};

/// The viewer's state; nothing open unless the app was started with `--view`.
#[derive(Debug, Default)]
pub struct Viewer {
    file: Option<PathBuf>,
    read_only: bool,
}

impl Viewer {
    /// The file the viewer shows.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// Changes are refused ("Dalica is already open").
    pub fn read_only(&self) -> bool {
        self.read_only
    }
}
