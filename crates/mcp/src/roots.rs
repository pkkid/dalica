//! The roots automation paths are checked against (`mcp.md` §5, `control-protocol.md` §5.7), as
//! the control channel checks them (`ui`'s `control/paths.rs`, whose rules these are; M10.16 moves
//! both into one engine module and adds the `--automation-*` flags and the client's roots).
//!
//! - **Read roots** (folders that may be added): the folders given on the command line and the
//!   demo's photo folder. A watched folder may always be read.
//! - **Write roots** (where new files are made): today the defaults, the OS temporary folder and
//!   `<cache>/automation` ([`default_write_roots`]). An export's folder and a Save As file may
//!   also be made inside a watched folder (they join the library by design); nothing is ever made
//!   inside `.dalica-originals`, and a new file never replaces one.
//!
//! Every path is absolute and normalised (no `.` or `..`), then resolved through its links before
//! it is compared, so a link cannot lead out of a root.

use std::path::{Component, Path, PathBuf};

use crate::backend::ToolError;

/// The longest path accepted, in bytes.
pub const MAX_PATH: usize = 4096;

/// The folder that keeps originals; automation never writes into it.
const ORIGINALS: &str = ".dalica-originals";

/// The write roots used when none is given: the OS temporary folder and `automation` in Dalica's
/// cache folder (the folder whose `thumbs` the thumbnail cache is; `control-protocol.md` §2.1).
pub fn default_write_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    let cache = dalica_engine::thumbs::default_cache_dir(|k| std::env::var_os(k)).and_then(|t| t.parent().map(Path::to_path_buf));
    roots.extend(cache.map(|c| c.join("automation")));
    roots
}

/// What [`Roots::check_output`] checks: a folder new files are made in (an export's, which need
/// not exist yet), or a new file (Save As).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Folder,
    File,
}

/// The read and write roots of one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots {
    read: Vec<PathBuf>,
    write: Vec<PathBuf>,
}

impl Default for Roots {
    fn default() -> Roots {
        Roots { read: Vec::new(), write: default_write_roots() }
    }
}

impl Roots {
    /// Adds a read root (a folder given on the command line, the demo's photos).
    pub fn add_read(&mut self, dir: PathBuf) {
        if !self.read.contains(&dir) {
            self.read.push(dir);
        }
    }

    pub fn read(&self) -> &[PathBuf] {
        &self.read
    }

    pub fn write(&self) -> &[PathBuf] {
        &self.write
    }

    /// A folder automation reads (a folder to add, `param` naming it in messages): inside a read
    /// root or a watched folder. Returns the resolved path.
    pub fn check_read(&self, path: &Path, param: &str, watched: &[PathBuf]) -> Result<PathBuf, ToolError> {
        lexical(path, param)?;
        let real = resolve(path)?;
        if inside(&real, watched) || inside(&real, &self.read) {
            return Ok(real);
        }
        let why =
            if self.read.is_empty() { "no read root was given (start with the folder on the command line)" } else { "it is outside every read root" };
        Err(ToolError::new("path_denied", format!("{} cannot be read by automation: {why}", path.display())))
    }

    /// Where automation makes new files that join the library (an export's `dir`, Save As's
    /// `path`): inside a write root **or** a watched folder, never inside `.dalica-originals`; a
    /// file never over an existing one, a folder never where a file is. Returns the resolved path.
    pub fn check_output(&self, path: &Path, param: &str, watched: &[PathBuf], kind: Output) -> Result<PathBuf, ToolError> {
        lexical(path, param)?;
        let target = match kind {
            Output::Folder => {
                let target = resolve_existing(path)?;
                if target.exists() && !target.is_dir() {
                    return Err(ToolError::new("bad_params", format!("{} is a file, not a folder", path.display())));
                }
                target
            }
            Output::File => {
                let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err(ToolError::new("bad_params", format!("{} names no file", path.display())));
                };
                let target = resolve(parent)?.join(name);
                if std::fs::symlink_metadata(&target).is_ok() {
                    return Err(ToolError::new("path_denied", format!("{} already exists; a new file never replaces one", path.display())));
                }
                target
            }
        };
        if target.components().any(|c| c.as_os_str() == ORIGINALS) {
            return Err(ToolError::new("path_denied", format!("{} is inside a {ORIGINALS} folder, where automation never writes", path.display())));
        }
        if !inside(&target, &self.write) && !inside(&target, watched) {
            return Err(ToolError::new(
                "path_denied",
                format!("{} is outside every automation write root (default: the temporary folder) and every watched folder", path.display()),
            ));
        }
        Ok(target)
    }
}

/// Absolute, normalised (no `.` or `..`), not too long: `bad_params` for a relative path,
/// `path_denied` for one that climbs with `..`.
fn lexical(path: &Path, param: &str) -> Result<(), ToolError> {
    if path.as_os_str().len() > MAX_PATH {
        return Err(ToolError::new("bad_params", format!("`{param}` is longer than {MAX_PATH} bytes")));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ToolError::new("path_denied", format!("`{param}` {} has a `..` component; give a normalised absolute path", path.display())));
    }
    if !path.is_absolute() {
        return Err(ToolError::new("bad_params", format!("`{param}` {} is not an absolute path", path.display())));
    }
    // `components()` drops a `.` inside a path, so look at the text.
    if path.as_os_str().to_string_lossy().split(['/', '\\']).any(|part| part == ".") {
        return Err(ToolError::new("bad_params", format!("`{param}` {} has a `.` component; give a normalised absolute path", path.display())));
    }
    Ok(())
}

fn resolve(path: &Path) -> Result<PathBuf, ToolError> {
    std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ToolError::new("not_found", format!("{} does not exist", path.display())),
        _ => ToolError::new("path_denied", format!("{} cannot be resolved: {e}", path.display())),
    })
}

/// Whether the resolved `path` is inside one of `roots` (each resolved; roots that do not exist
/// hold nothing).
fn inside(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| path.starts_with(&r))
}

/// `path` resolved through the links of the part of it that exists: the nearest existing ancestor
/// resolved, the rest appended (it has no `..`, so nothing in it can climb).
fn resolve_existing(path: &Path) -> Result<PathBuf, ToolError> {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if std::fs::symlink_metadata(at).is_ok() {
            let mut real = resolve(at)?;
            real.extend(rest.iter().rev());
            return Ok(real);
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                at = parent;
            }
            _ => return Err(ToolError::new("not_found", format!("no part of {} exists", path.display()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code<T>(r: Result<T, ToolError>) -> String {
        match r {
            Ok(_) => "ok".into(),
            Err(e) => e.code,
        }
    }

    /// Reads only inside a read root or a watched folder; new files inside a write root or a
    /// watched folder, never in `.dalica-originals` or over a file; paths absolute and normalised.
    #[test]
    fn paths_stay_inside_their_roots() {
        let dir = tempfile::tempdir().unwrap();
        let (photos, out, other) = (dir.path().join("photos"), dir.path().join("out"), dir.path().join("other"));
        for d in [&photos, &out, &other, &photos.join(ORIGINALS)] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(out.join("taken.jpg"), b"x").unwrap();
        let mut roots = Roots { read: Vec::new(), write: vec![out.clone()] };
        assert_eq!(code(roots.check_read(&photos, "path", &[])), "path_denied");
        roots.add_read(photos.clone());
        assert_eq!(code(roots.check_read(&photos.join(ORIGINALS), "path", &[])), "ok");
        assert_eq!(code(roots.check_read(&other, "path", std::slice::from_ref(&other))), "ok", "a watched folder is readable");
        assert_eq!(code(roots.check_read(&other.join("missing"), "path", std::slice::from_ref(&other))), "not_found");
        assert_eq!(code(roots.check_read(&photos.join("..").join("other"), "path", &[])), "path_denied");
        assert_eq!(code(roots.check_read(Path::new("photos"), "path", &[])), "bad_params");

        assert_eq!(code(roots.check_output(&out.join("new").join("deeper"), "dir", &[], Output::Folder)), "ok", "made when missing");
        assert_eq!(code(roots.check_output(&out.join("taken.jpg"), "dir", &[], Output::Folder)), "bad_params");
        assert_eq!(code(roots.check_output(&out.join("taken.jpg"), "path", &[], Output::File)), "path_denied");
        assert_eq!(code(roots.check_output(&out.join("new.jpg"), "path", &[], Output::File)), "ok");
        assert_eq!(code(roots.check_output(&other.join("x"), "dir", &[], Output::Folder)), "path_denied");
        assert_eq!(code(roots.check_output(&photos.join("x"), "dir", std::slice::from_ref(&photos), Output::Folder)), "ok", "into a watched folder");
        assert_eq!(code(roots.check_output(&photos.join(ORIGINALS).join("x"), "dir", std::slice::from_ref(&photos), Output::Folder)), "path_denied");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other, out.join("link")).unwrap();
            assert_eq!(code(roots.check_output(&out.join("link").join("x"), "dir", &[], Output::Folder)), "path_denied", "a link out of a root");
        }
        assert!(Roots::default().write().contains(&std::env::temp_dir()));
    }
}
