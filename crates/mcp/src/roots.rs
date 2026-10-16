//! The roots automation paths are checked against, and what the launch allowed (`mcp.md` §5,
//! `control-protocol.md` §5.6–§5.7): the engine's policy ([`dalica_engine::automation`], the one
//! the control channel uses too), with its refusals as tool errors, and the MCP client's roots.
//!
//! - **Read roots** (folders that may be added): `--automation-read-root`, the folders given on the
//!   command line, the demo's photo folder, and the client's `file://` roots ([`client_roots`],
//!   from `roots/list`; `--no-client-roots` never asks). A watched folder may always be read.
//! - **Write roots** (where new files are made): `--automation-write-root`, else the defaults, the
//!   OS temporary folder and `<cache>/Dalica/automation` ([`default_write_roots`]). Client roots are
//!   never write roots. An export's folder and a Save As file may also be made inside a watched
//!   folder (they join the library by design); nothing is ever made inside `.dalica-originals`,
//!   and a new file never replaces one.
//!
//! Every path is absolute and normalised (no `.` or `..`), then resolved through its links before
//! it is compared, so a link cannot lead out of a root.

use std::path::{Component, Path, PathBuf};

pub use dalica_engine::automation::{MAX_PATH, MAX_ROOTS, Output, default_write_roots};
use dalica_engine::automation::{Policy, Refused, plain};
use serde_json::Value;

use crate::backend::ToolError;

impl From<Refused> for ToolError {
    fn from(r: Refused) -> ToolError {
        ToolError::new(r.code(), r.message)
    }
}

/// The read and write roots and the permissions of one session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roots {
    policy: Policy,
}

impl Roots {
    /// Roots from the launch's policy (`--automation-*`).
    pub fn new(policy: Policy) -> Roots {
        Roots { policy }
    }

    /// Adds a read root (a folder given on the command line, the demo's photos).
    pub fn add_read(&mut self, dir: PathBuf) {
        self.policy.add_read(dir);
    }

    /// The read roots given at launch (the client's are [`Roots::client`]).
    pub fn read(&self) -> &[PathBuf] {
        self.policy.read_roots()
    }

    /// The client's roots, as last listed.
    pub fn client(&self) -> &[PathBuf] {
        self.policy.client_roots()
    }

    /// Replaces the client's roots (read roots only); returns how many are kept.
    pub fn set_client(&mut self, roots: Vec<PathBuf>) -> usize {
        self.policy.set_client_roots(roots)
    }

    /// The write roots in effect.
    pub fn write(&self) -> Vec<PathBuf> {
        self.policy.write_roots()
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// A folder automation reads (a folder to add, `param` naming it in messages): inside a read
    /// root, a client root or a watched folder. Returns the resolved path.
    pub fn check_read(&self, path: &Path, param: &str, watched: &[PathBuf]) -> Result<PathBuf, ToolError> {
        Ok(self.policy.check_read(path, param, watched)?)
    }

    /// A new file that must not join the library (a render): inside a write root, outside every
    /// watched folder and `.dalica-originals`. Returns the path to write.
    pub fn check_write(&self, path: &Path, param: &str, watched: &[PathBuf]) -> Result<PathBuf, ToolError> {
        Ok(self.policy.check_write(path, param, watched)?)
    }

    /// Where automation makes new files that join the library (an export's `dir`, Save As's
    /// `path`): inside a write root **or** a watched folder, never inside `.dalica-originals`; a
    /// file never over an existing one, a folder never where a file is. Returns the resolved path.
    pub fn check_output(&self, path: &Path, param: &str, watched: &[PathBuf], kind: Output) -> Result<PathBuf, ToolError> {
        Ok(self.policy.check_output(path, param, watched, kind)?)
    }
}

/// The folders of a `roots/list` result (`{roots: [{uri, name?}]}`): each `file://` URI on this
/// machine (no host, or `localhost`) as an absolute path, percent-decoded ([`file_uri_path`]);
/// anything else left out. At most [`MAX_ROOTS`].
pub fn client_roots(result: &Value) -> Vec<PathBuf> {
    let Some(list) = result.get("roots").and_then(Value::as_array) else { return Vec::new() };
    list.iter().filter_map(|r| r.get("uri").and_then(Value::as_str)).filter_map(file_uri_path).take(MAX_ROOTS).collect()
}

/// The local path a `file://` URI names (RFC 8089): `None` for another scheme, a host other than
/// `localhost`, a query or fragment, a bad escape or a NUL, a path that is not absolute or that
/// climbs with `..`. On Windows the path must start with a drive (`file:///C:/Photos`) and comes
/// back in its plain form (`C:\Photos`: backslashes, an upper-case drive letter), the form the
/// policy compares and answers in ([`dalica_engine::automation::plain`]).
pub fn file_uri_path(uri: &str) -> Option<PathBuf> {
    if uri.len() > MAX_PATH.saturating_mul(3) {
        return None;
    }
    if !uri.get(..7)?.eq_ignore_ascii_case("file://") {
        return None;
    }
    let rest = uri.get(7..)?;
    if rest.contains(['?', '#']) {
        return None;
    }
    let slash = rest.find('/')?;
    let host = rest.get(..slash)?;
    if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let text = percent_decode(rest.get(slash..)?)?;
    if text.contains('\0') {
        return None;
    }
    let path = if cfg!(windows) {
        // `/C:/Photos` names `C:\Photos`.
        let b = text.as_bytes();
        if !(b.get(1).is_some_and(u8::is_ascii_alphabetic) && b.get(2) == Some(&b':')) {
            return None;
        }
        plain(Path::new(&text.get(1..)?.replace('/', "\\")))
    } else {
        PathBuf::from(text)
    };
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(path)
}

/// `%XX` escapes decoded; `None` for a broken escape or bytes that are not UTF-8.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = s.get(i.checked_add(1)?..i.checked_add(3)?).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i = i.checked_add(3)?;
        } else {
            out.push(b);
            i = i.checked_add(1)?;
        }
    }
    String::from_utf8(out).ok()
}
