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

/// What [`check_output`] checks: a folder new files are made in (an export's location, which may
/// not exist yet), or a new file (Save As).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Folder,
    File,
}

/// `path` resolved through the symlinks of the part of it that exists: the nearest existing
/// ancestor resolved, the rest appended (it has no `..`, so nothing in it can climb).
fn resolve_existing(path: &Path) -> Result<PathBuf, Error> {
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
            _ => return Err(Error::new(Code::NotFound, format!("no part of {} exists", path.display()))),
        }
    }
}

/// Where automation makes new files that become library photos (`control-protocol.md` §5.7: an
/// export's `dir`, Save As's `path`): resolved, inside a write root **or** a watched folder (an
/// export or a saved copy there is meant to join the library), never inside `.dalica-originals`;
/// a file never over an existing one and only in a folder that exists, a folder never where a
/// file is. Returns the resolved path.
pub fn check_output(path: &Path, write_roots: &[PathBuf], watched: &[PathBuf], kind: Output) -> Result<PathBuf, Error> {
    let target = match kind {
        Output::Folder => {
            let target = resolve_existing(path)?;
            if target.exists() && !target.is_dir() {
                return Err(Error::new(Code::BadParams, format!("{} is a file, not a folder", path.display())));
            }
            target
        }
        Output::File => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(Error::new(Code::BadParams, format!("{} names no file", path.display())));
            };
            let target = resolve(parent)?.join(name);
            if std::fs::symlink_metadata(&target).is_ok() {
                return Err(Error::new(Code::PathDenied, format!("{} already exists; a new file never replaces one", path.display())));
            }
            target
        }
    };
    if target.components().any(|c| c.as_os_str() == ORIGINALS) {
        return Err(Error::new(Code::PathDenied, format!("{} is inside a {ORIGINALS} folder, where automation never writes", path.display())));
    }
    if !inside(&target, write_roots) && !inside(&target, watched) {
        return Err(Error::new(
            Code::PathDenied,
            format!(
                "{} is outside every automation write root (--automation-write-root; default: the temporary folder) and every watched folder",
                path.display()
            ),
        ));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(r: Result<PathBuf, Error>) -> &'static str {
        match r {
            Ok(p) => panic!("allowed: {}", p.display()),
            Err(e) => e.code.as_str(),
        }
    }

    #[test]
    fn new_files_go_into_a_write_root_or_a_watched_folder_and_never_replace_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (out, watched, elsewhere) = (root.join("out"), root.join("photos"), root.join("elsewhere"));
        for d in [&out, &watched, &elsewhere, &watched.join(ORIGINALS)] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(watched.join("taken.jpg"), b"x").unwrap();
        let (roots, w) = (vec![out.clone()], vec![watched.clone()]);
        // A folder that does not exist yet, under a write root or a watched folder.
        assert_eq!(check_output(&out.join("Exports").join("deep"), &roots, &w, Output::Folder).unwrap(), out.join("Exports").join("deep"));
        assert_eq!(check_output(&watched, &roots, &w, Output::Folder).unwrap(), watched);
        assert_eq!(code(check_output(&elsewhere.join("Exports"), &roots, &w, Output::Folder)), "path_denied");
        assert_eq!(code(check_output(&watched.join("taken.jpg"), &roots, &w, Output::Folder)), "bad_params");
        assert_eq!(code(check_output(&watched.join(ORIGINALS).join("x"), &roots, &w, Output::Folder)), "path_denied");
        // A file: its folder exists, it does not.
        assert_eq!(check_output(&watched.join("new.png"), &roots, &w, Output::File).unwrap(), watched.join("new.png"));
        assert_eq!(code(check_output(&watched.join("taken.jpg"), &roots, &w, Output::File)), "path_denied");
        assert_eq!(code(check_output(&watched.join("missing").join("a.jpg"), &roots, &w, Output::File)), "not_found");
        assert_eq!(code(check_output(&elsewhere.join("a.jpg"), &roots, &w, Output::File)), "path_denied");
        assert_eq!(code(check_output(&watched.join(ORIGINALS).join("a.jpg"), &roots, &w, Output::File)), "path_denied");
        #[cfg(unix)]
        {
            // A link inside a root that points outside it is followed before the check.
            std::os::unix::fs::symlink(&elsewhere, out.join("link")).unwrap();
            assert_eq!(code(check_output(&out.join("link").join("Exports"), &roots, &w, Output::Folder)), "path_denied");
            assert_eq!(code(check_output(&out.join("link").join("a.jpg"), &roots, &w, Output::File)), "path_denied");
        }
    }
}
