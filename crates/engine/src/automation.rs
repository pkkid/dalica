//! What automation may touch (`control-protocol.md` §5.6–§5.7, `mcp.md` §5): the read and write
//! roots that path params are checked against, and the permissions the launch gave. One policy for
//! every surface: the control channel (`ui`'s `control/paths.rs`) and the MCP server (`mcp`'s
//! `roots.rs`) are thin adapters over this module.
//!
//! - **Roots are trusted launch configuration** (`--automation-read-root`, `--automation-write-root`,
//!   the folders on the command line, the demo's folder); **path params are hostile**. Whatever was
//!   not granted is refused: no read root means no folder outside the watched ones may be added
//!   (except in the demo); no write root given means the defaults ([`default_write_roots`]).
//! - **Read roots**: folders that may be added or read. An MCP client's `file://` roots are read
//!   roots only, and are replaced as a whole when the client's list changes
//!   ([`Policy::set_client_roots`]); they are never write roots.
//! - **Write roots**: where automation makes new files (screenshots, renders: [`Policy::check_write`];
//!   exports and Save As: [`Policy::check_output`]). Never inside `.dalica-originals/`; a
//!   screenshot or render never inside a watched folder; an export or a Save As may be (they join
//!   the library by design, §5.7); a new file never over an existing one (but a screenshot may
//!   replace a `.png`).
//! - Every path is checked lexically first ([`lexical`]: absolute, no `.` or `..`, not too long;
//!   on Windows no verbatim or device prefix, no stream, no reserved device name, no component
//!   ending in a dot or a space), then **resolved through its links** before it is compared, so a
//!   link cannot lead out of a root. A root that does not exist holds nothing.
//! - Comparisons that **allow** are exact (component by component, after resolving); comparisons
//!   that **refuse** (a watched folder, `.dalica-originals`) also ignore case on the systems whose
//!   volumes usually do (Windows, macOS), so a differently-cased spelling cannot slip past them.
//!
//! The checks read the file system (they resolve links): call them off the UI thread, except
//! [`lexical`], which touches nothing.
//!
//! After PhotoCraft `crates/automation/src/workspace.rs` (`AuthorizedWorkspace`, `request_path`,
//! `relative_path`, `is_windows_device_name`) @ 583425b4, MIT OR Apache-2.0 (`NOTICE`): roots as
//! trusted configuration and request paths as untrusted input, omitted authority failing closed,
//! Windows prefixes, streams and device names refused before any I/O. Changed for Dalica: no
//! directory capabilities (`cap-std`): absolute paths are resolved through their links and
//! compared, as the control channel did; several roots of each kind; the watched folders and
//! `.dalica-originals`; the MCP client's roots; the external and people permissions.

use std::path::{Component, Path, PathBuf, Prefix};

use crate::registry::Effect;

/// The longest path accepted, in bytes.
pub const MAX_PATH: usize = 4096;

/// The folder that keeps originals; automation never writes into it.
pub const ORIGINALS: &str = ".dalica-originals";

/// The most roots of one kind kept (an MCP client's list beyond it is cut).
pub const MAX_ROOTS: usize = 256;

/// Why a path or an effect was refused: the control protocol's error codes (`control-protocol.md`
/// §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A relative path, one too long, one with a `.` component, a folder where a file is meant.
    BadParams,
    /// Outside every root, or a forbidden target.
    PathDenied,
    /// The path (or its folder) does not exist.
    NotFound,
    /// An effect outside Dalica without `--automation-allow-external`.
    ExternalDenied,
}

impl Refusal {
    /// The control protocol's error code.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::BadParams => "bad_params",
            Refusal::PathDenied => "path_denied",
            Refusal::NotFound => "not_found",
            Refusal::ExternalDenied => "external_denied",
        }
    }
}

/// A refused path or effect, with the message the caller shows.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}: {message}", kind.code())]
pub struct Refused {
    pub kind: Refusal,
    pub message: String,
}

impl Refused {
    fn new(kind: Refusal, message: impl Into<String>) -> Refused {
        Refused { kind, message: message.into() }
    }

    /// The control protocol's error code.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

/// What [`Policy::check_output`] checks: a folder new files are made in (an export's, which need
/// not exist yet), or a new file (Save As).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Folder,
    File,
}

/// The write roots used when none is given: the OS temporary folder and `automation` in Dalica's
/// cache folder (the folder whose `thumbs` the thumbnail cache is: `<cache>/Dalica/automation`).
pub fn default_write_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    let cache = crate::thumbs::default_cache_dir(|k| std::env::var_os(k)).and_then(|t| t.parent().map(Path::to_path_buf));
    roots.extend(cache.map(|c| c.join("automation")));
    roots
}

/// The read and write roots and the permissions of one automation surface (an app started with
/// `--control`, one MCP server). Built from trusted launch configuration; checks hostile paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    read: Vec<PathBuf>,
    /// The write roots given; empty means [`default_write_roots`].
    write: Vec<PathBuf>,
    /// The MCP client's roots (read roots only).
    client: Vec<PathBuf>,
    demo: bool,
    allow_external: bool,
    allow_people: bool,
}

impl Policy {
    /// A policy with these read roots and write roots (empty: the defaults), nothing else granted.
    pub fn with_roots(read: &[PathBuf], write: &[PathBuf]) -> Policy {
        let mut p = Policy::default();
        for r in read {
            p.add_read(r.clone());
        }
        for w in write {
            p.add_write(w.clone());
        }
        p
    }

    /// Adds a read root (`--automation-read-root`, a folder given on the command line, the demo's
    /// photos). Only an absolute path is taken; `false` when it was not.
    pub fn add_read(&mut self, dir: PathBuf) -> bool {
        add_root(&mut self.read, dir)
    }

    /// Adds a write root (`--automation-write-root`). Only an absolute path is taken; `false` when
    /// it was not.
    pub fn add_write(&mut self, dir: PathBuf) -> bool {
        add_root(&mut self.write, dir)
    }

    /// Replaces the MCP client's roots (each a read root, never a write root). Relative paths are
    /// dropped, duplicates kept once, at most [`MAX_ROOTS`]. Returns how many are kept.
    pub fn set_client_roots(&mut self, roots: Vec<PathBuf>) -> usize {
        self.client.clear();
        for r in roots {
            add_root(&mut self.client, r);
        }
        self.client.len()
    }

    /// `--demo`: with no read root, any folder may be added (the demo is a scratch library).
    pub fn set_demo(&mut self, demo: bool) {
        self.demo = demo;
    }

    /// `--automation-allow-external` (§5.6).
    pub fn set_allow_external(&mut self, allow: bool) {
        self.allow_external = allow;
    }

    /// `--automation-allow-people`: people names and face regions may be returned.
    pub fn set_allow_people(&mut self, allow: bool) {
        self.allow_people = allow;
    }

    /// The read roots given (the client's are [`Policy::client_roots`]).
    pub fn read_roots(&self) -> &[PathBuf] {
        &self.read
    }

    /// The MCP client's roots, as last listed.
    pub fn client_roots(&self) -> &[PathBuf] {
        &self.client
    }

    /// The write roots in effect: those given, else [`default_write_roots`].
    pub fn write_roots(&self) -> Vec<PathBuf> {
        if self.write.is_empty() { default_write_roots() } else { self.write.clone() }
    }

    pub fn demo(&self) -> bool {
        self.demo
    }

    pub fn allow_external(&self) -> bool {
        self.allow_external
    }

    pub fn allow_people(&self) -> bool {
        self.allow_people
    }

    /// A command with `effect` (`id` naming it): an effect outside Dalica needs
    /// `--automation-allow-external` (§5.6).
    pub fn check_effect(&self, effect: Effect, id: &str) -> Result<(), Refused> {
        if effect == Effect::External && !self.allow_external {
            return Err(Refused::new(
                Refusal::ExternalDenied,
                format!("`{id}` reaches outside Dalica; automation runs it only when started with --automation-allow-external"),
            ));
        }
        Ok(())
    }

    /// A folder or file automation reads (a folder to add; `param` names it in messages): inside a
    /// read root, a client root or a watched folder; with no read root at all, anywhere in the
    /// demo. Returns the resolved path.
    pub fn check_read(&self, path: &Path, param: &str, watched: &[PathBuf]) -> Result<PathBuf, Refused> {
        lexical(path, param)?;
        let real = resolve(path)?;
        let no_roots = self.read.is_empty() && self.client.is_empty();
        if inside(&real, watched) || inside(&real, &self.read) || inside(&real, &self.client) || (self.demo && no_roots) {
            return Ok(real);
        }
        let why = if no_roots {
            "no read root was given (--automation-read-root, a folder on the command line, or an MCP client root)"
        } else {
            "it is outside every read root (--automation-read-root, the folders on the command line, the MCP client's roots)"
        };
        Err(Refused::new(Refusal::PathDenied, format!("{} cannot be read by automation: {why}", path.display())))
    }

    /// A new file automation writes that must not join the library (a screenshot, a render): its
    /// folder exists; the file, resolved, is inside a write root, outside every watched folder and
    /// `.dalica-originals`. An existing file there may be replaced (the caller allows only its own
    /// kind, a `.png`); a link is followed and must end at a `.png` file. Returns the path to
    /// write.
    pub fn check_write(&self, path: &Path, param: &str, watched: &[PathBuf]) -> Result<PathBuf, Refused> {
        lexical(path, param)?;
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(Refused::new(Refusal::BadParams, format!("{} names no file", path.display())));
        };
        let mut target = resolve(parent)?.join(name);
        match std::fs::symlink_metadata(&target) {
            Ok(m) if m.is_dir() => return Err(Refused::new(Refusal::BadParams, format!("{} is a folder", path.display()))),
            Ok(m) if m.file_type().is_symlink() => {
                target = resolve(&target)?;
                if !target.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) || target.is_dir() {
                    return Err(Refused::new(Refusal::PathDenied, format!("{} links to something that is not a .png file", path.display())));
                }
            }
            _ => {}
        }
        refuse_originals(&target, path)?;
        if inside_loosely(&target, watched) {
            return Err(Refused::new(Refusal::PathDenied, format!("{} is inside a watched folder: it would become a library photo", path.display())));
        }
        if !inside(&target, &self.write_roots()) {
            return Err(Refused::new(
                Refusal::PathDenied,
                format!("{} is outside every automation write root (--automation-write-root; default: the temporary folder)", path.display()),
            ));
        }
        Ok(target)
    }

    /// Where automation makes new files that join the library (an export's `dir`, Save As's
    /// `path`): inside a write root **or** a watched folder, never inside `.dalica-originals`; a
    /// file never over an existing one and only in a folder that exists, a folder never where a
    /// file is. Returns the resolved path.
    pub fn check_output(&self, path: &Path, param: &str, watched: &[PathBuf], kind: Output) -> Result<PathBuf, Refused> {
        lexical(path, param)?;
        let target = match kind {
            Output::Folder => {
                let target = resolve_existing(path)?;
                if target.exists() && !target.is_dir() {
                    return Err(Refused::new(Refusal::BadParams, format!("{} is a file, not a folder", path.display())));
                }
                target
            }
            Output::File => {
                let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err(Refused::new(Refusal::BadParams, format!("{} names no file", path.display())));
                };
                let target = resolve(parent)?.join(name);
                if std::fs::symlink_metadata(&target).is_ok() {
                    return Err(Refused::new(Refusal::PathDenied, format!("{} already exists; a new file never replaces one", path.display())));
                }
                target
            }
        };
        refuse_originals(&target, path)?;
        if !inside(&target, &self.write_roots()) && !inside(&target, watched) {
            return Err(Refused::new(
                Refusal::PathDenied,
                format!(
                    "{} is outside every automation write root (--automation-write-root; default: the temporary folder) and every watched folder",
                    path.display()
                ),
            ));
        }
        Ok(target)
    }
}

/// Adds `dir` to `roots` when it is absolute and not there yet, up to [`MAX_ROOTS`].
fn add_root(roots: &mut Vec<PathBuf>, dir: PathBuf) -> bool {
    if !dir.is_absolute() || dir.as_os_str().len() > MAX_PATH {
        return false;
    }
    if !roots.contains(&dir) && roots.len() < MAX_ROOTS {
        roots.push(dir);
    }
    true
}

/// Absolute, normalised (no `.` or `..`), not too long, and on Windows an ordinary path
/// ([`windows_problem`]); touches nothing. `bad_params` for a relative or malformed path,
/// `path_denied` for one that climbs with `..` or uses a prefix that skips Windows' checks.
pub fn lexical(path: &Path, param: &str) -> Result<(), Refused> {
    if path.as_os_str().len() > MAX_PATH {
        return Err(Refused::new(Refusal::BadParams, format!("`{param}` is longer than {MAX_PATH} bytes")));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Refused::new(Refusal::PathDenied, format!("`{param}` {} has a `..` component; give a normalised absolute path", path.display())));
    }
    for c in path.components() {
        if let Component::Prefix(p) = c
            && matches!(p.kind(), Prefix::Verbatim(_) | Prefix::VerbatimUNC(..) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_))
        {
            return Err(Refused::new(
                Refusal::PathDenied,
                format!("`{param}` {} uses a verbatim or device prefix; give an ordinary absolute path", path.display()),
            ));
        }
    }
    if !path.is_absolute() {
        return Err(Refused::new(Refusal::BadParams, format!("`{param}` {} is not an absolute path", path.display())));
    }
    // `components()` drops a `.` inside a path, so look at the text.
    if path.as_os_str().to_string_lossy().split(['/', '\\']).any(|part| part == ".") {
        return Err(Refused::new(Refusal::BadParams, format!("`{param}` {} has a `.` component; give a normalised absolute path", path.display())));
    }
    if cfg!(windows) {
        for c in path.components() {
            if let Component::Normal(name) = c
                && let Some(why) = windows_problem(&name.to_string_lossy())
            {
                return Err(Refused::new(Refusal::BadParams, format!("`{param}` {}: {why}", path.display())));
            }
        }
    }
    Ok(())
}

/// What is wrong with one path component on Windows, where it would not name what it says: a
/// stream (`a.png:x`), a name ending in a dot or a space (Windows drops them, so it aliases
/// another name), a reserved device name (`NUL`, `COM1.txt`).
pub fn windows_problem(component: &str) -> Option<&'static str> {
    if component.contains(':') {
        return Some("a `:` (a drive, device or stream) inside a path is not allowed");
    }
    if component.ends_with(['.', ' ']) {
        return Some("a name ending in a dot or a space is not allowed");
    }
    let stem = component.split('.').next().unwrap_or(component).trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.bytes().all(|b| (b'1'..=b'9').contains(&b)));
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT") {
        return Some("a reserved device name is not allowed");
    }
    None
}

fn resolve(path: &Path) -> Result<PathBuf, Refused> {
    std::fs::canonicalize(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Refused::new(Refusal::NotFound, format!("{} does not exist", path.display())),
        _ => Refused::new(Refusal::PathDenied, format!("{} cannot be resolved: {e}", path.display())),
    })
}

/// Whether the resolved `path` is inside one of `roots` (each resolved; roots that do not exist
/// hold nothing), comparing components exactly: the comparison that allows.
fn inside(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| path.starts_with(&r))
}

/// [`inside`], also ignoring case where volumes usually do (Windows, macOS): the comparison that
/// refuses.
fn inside_loosely(path: &Path, roots: &[PathBuf]) -> bool {
    if inside(path, roots) {
        return true;
    }
    if !cfg!(any(windows, target_os = "macos")) {
        return false;
    }
    let folded = |p: &Path| -> Vec<String> { p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect() };
    let at = folded(path);
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| at.starts_with(&folded(&r)))
}

/// `path` (the caller's spelling, for the message) refused when `target` lies in a
/// `.dalica-originals` folder, in any case.
fn refuse_originals(target: &Path, path: &Path) -> Result<(), Refused> {
    if target.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(ORIGINALS)) {
        return Err(Refused::new(Refusal::PathDenied, format!("{} is inside a {ORIGINALS} folder, where automation never writes", path.display())));
    }
    Ok(())
}

/// `path` resolved through the links of the part of it that exists: the nearest existing ancestor
/// resolved, the rest appended (it has no `..`, so nothing in it can climb).
fn resolve_existing(path: &Path) -> Result<PathBuf, Refused> {
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
            _ => return Err(Refused::new(Refusal::NotFound, format!("no part of {} exists", path.display()))),
        }
    }
}
