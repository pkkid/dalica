//! Path params (`control-protocol.md` §5.7): absolute and normalised (checked on the UI thread,
//! before anything happens), then resolved through symlinks and checked against the automation
//! roots (on a worker: it touches the file system).
//!
//! The rules are the engine's ([`dalica_engine::automation`], shared with the MCP server); this
//! module keeps the control channel's calls and turns a refusal into its [`Error`].

use std::path::{Path, PathBuf};

#[cfg(test)]
use dalica_engine::automation::ORIGINALS;
use dalica_engine::automation::{self, Policy, Refusal, Refused};
pub use dalica_engine::automation::{Output, default_write_roots};

use super::{Code, Error};

impl From<Refused> for Error {
    fn from(r: Refused) -> Error {
        let code = match r.kind {
            Refusal::BadParams => Code::BadParams,
            Refusal::PathDenied => Code::PathDenied,
            Refusal::NotFound => Code::NotFound,
            Refusal::ExternalDenied => Code::ExternalDenied,
        };
        Error::new(code, r.message)
    }
}

/// Absolute, normalised (no `.` or `..`), not too long: `bad_params` for a relative path,
/// `path_denied` for one that climbs with `..`. Touches nothing.
pub fn lexical(path: &Path, param: &str) -> Result<(), Error> {
    Ok(automation::lexical(path, param)?)
}

/// A folder or file automation reads (a folder to add): inside a read root or a watched folder;
/// with no read root, only in the demo. Returns the resolved path.
pub fn check_read(path: &Path, read_roots: &[PathBuf], watched: &[PathBuf], demo: bool) -> Result<PathBuf, Error> {
    let mut policy = Policy::with_roots(read_roots, &[]);
    policy.set_demo(demo);
    Ok(policy.check_read(path, "path", watched)?)
}

/// A new file automation writes (`ui.screenshot`): its folder exists; the file, resolved, is inside
/// a write root, outside every watched folder and `.dalica-originals`; an existing file there is
/// only replaced when it is a `.png` (checked by the caller's extension rule). Returns the path
/// to write.
pub fn check_write(path: &Path, write_roots: &[PathBuf], watched: &[PathBuf]) -> Result<PathBuf, Error> {
    Ok(Policy::with_roots(&[], write_roots).check_write(path, "path", watched)?)
}

/// Where automation makes new files that become library photos (`control-protocol.md` §5.7: an
/// export's `dir`, Save As's `path`): resolved, inside a write root **or** a watched folder,
/// never inside `.dalica-originals`; a file never over an existing one and only in a folder that
/// exists, a folder never where a file is. Returns the resolved path.
pub fn check_output(path: &Path, write_roots: &[PathBuf], watched: &[PathBuf], kind: Output) -> Result<PathBuf, Error> {
    Ok(Policy::with_roots(&[], write_roots).check_output(path, "path", watched, kind)?)
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
