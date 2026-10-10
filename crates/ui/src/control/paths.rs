//! Path params (`control-protocol.md` §5.7): absolute and normalised (checked on the UI thread,
//! before anything happens), then resolved through symlinks and checked against the automation
//! roots (on a worker: it touches the file system).

use std::path::{Component, Path, PathBuf};

use super::{Code, Error};

/// The longest path accepted, in bytes.
pub const MAX_PATH: usize = 4096;

/// The folder that keeps originals; never written into by automation.
const ORIGINALS: &str = ".dalica-originals";

/// The write roots used when none is given: the OS temporary folder and Dalica's own automation
/// folder in the cache (`<cache>/automation`, the cache folder of §2.1).
pub fn default_write_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let cache = if cfg!(target_os = "windows") {
        env("LOCALAPPDATA").map(|d| d.join("Dalica").join("cache"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Caches").join("Dalica"))
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache"))).map(|c| c.join("dalica"))
    };
    roots.extend(cache.map(|c| c.join("automation")));
    roots
}

/// Absolute, normalised (no `.` or `..`), not too long: `bad_params` for a relative path,
/// `path_denied` for one that climbs with `..`.
pub fn lexical(path: &Path, param: &str) -> Result<(), Error> {
    if path.as_os_str().len() > MAX_PATH {
        return Err(Error::new(Code::BadParams, format!("`{param}` is longer than {MAX_PATH} bytes")));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::new(Code::PathDenied, format!("`{param}` {} has a `..` component; give a normalised absolute path", path.display())));
    }
    if !path.is_absolute() {
        return Err(Error::new(Code::BadParams, format!("`{param}` {} is not an absolute path", path.display())));
    }
    // `components()` drops a `.` inside a path, so look at the text.
    if path.as_os_str().to_string_lossy().split(['/', '\\']).any(|part| part == ".") {
        return Err(Error::new(Code::BadParams, format!("`{param}` {} has a `.` component; give a normalised absolute path", path.display())));
    }
    Ok(())
}

fn resolve(path: &Path) -> Result<PathBuf, Error> {
    std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::new(Code::NotFound, format!("{} does not exist", path.display())),
        _ => Error::new(Code::PathDenied, format!("{} cannot be resolved: {e}", path.display())),
    })
}

/// Whether the resolved `path` is inside one of `roots` (each resolved; roots that do not exist
/// hold nothing).
fn inside(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| path.starts_with(&r))
}

/// A folder or file automation reads (a folder to add): inside a read root or a watched folder;
/// with no read root, only in the demo. Returns the resolved path.
pub fn check_read(path: &Path, read_roots: &[PathBuf], watched: &[PathBuf], demo: bool) -> Result<PathBuf, Error> {
    let real = resolve(path)?;
    if inside(&real, watched) || inside(&real, read_roots) || (demo && read_roots.is_empty()) {
        return Ok(real);
    }
    let why = if read_roots.is_empty() { "no --automation-read-root was given" } else { "it is outside every --automation-read-root" };
    Err(Error::new(Code::PathDenied, format!("{} cannot be read by automation: {why}", path.display())))
}

/// A new file automation writes (`ui.screenshot`): its folder exists; the file, resolved, is inside
/// a write root, outside every watched folder and `.dalica-originals`; an existing file there is
/// only replaced when it is a `.png` (checked by the caller's extension rule). Returns the path
/// to write.
pub fn check_write(path: &Path, write_roots: &[PathBuf], watched: &[PathBuf]) -> Result<PathBuf, Error> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(Error::new(Code::BadParams, format!("{} names no file", path.display())));
    };
    let mut target = resolve(parent)?.join(name);
    match std::fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() => return Err(Error::new(Code::BadParams, format!("{} is a folder", path.display()))),
        Ok(m) if m.file_type().is_symlink() => {
            target = resolve(&target)?;
            if !target.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) || target.is_dir() {
                return Err(Error::new(Code::PathDenied, format!("{} links to something that is not a .png file", path.display())));
            }
        }
        _ => {}
    }
    if target.components().any(|c| c.as_os_str() == ORIGINALS) {
        return Err(Error::new(Code::PathDenied, format!("{} is inside a {ORIGINALS} folder, where automation never writes", path.display())));
    }
    if inside(&target, watched) {
        return Err(Error::new(Code::PathDenied, format!("{} is inside a watched folder: it would become a library photo", path.display())));
    }
    if !inside(&target, write_roots) {
        return Err(Error::new(
            Code::PathDenied,
            format!("{} is outside every automation write root (--automation-write-root; default: the temporary folder)", path.display()),
        ));
    }
    Ok(target)
}
