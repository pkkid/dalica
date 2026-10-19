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
//! - **One form on both sides**: the path and every root are resolved the same way and put in their
//!   plain form ([`plain`]) before they are compared. On Windows resolving gives the verbatim form
//!   (`\\?\C:\…`, `\\?\UNC\server\share\…`) and expands 8.3 short names (`RUNNER~1`), while
//!   roots, `file:///C:/…` URIs and an agent's own paths are plain (`C:\…`); the plain form drops the
//!   verbatim prefix and upper-cases the drive letter, so every spelling of one place compares
//!   equal. The paths the checks return are in that plain form too, the form an agent can give
//!   back ([`lexical`] refuses a verbatim one).
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
//!
//! - **People data** (`control-protocol.md` §6.4, `faces.md` §7): without
//!   `--automation-allow-people` the replies of the commands that carry it ([`PEOPLE_REPLIES`]:
//!   `faces.list`, `faces.add`, `people.list`) reach automation with names and nicknames `null`
//!   and face rectangles left out, ids and counts kept ([`Policy::people_reply`]); face
//!   descriptors are never returned, whatever was granted. The commands themselves run as asked,
//!   but for the searches ([`NAME_SEARCHES`]: `library.query`, `search.set`, `slideshow.list`'s
//!   `search`), which run with
//!   people's names left out of the match ([`Policy::people_params`]: `"withholdNames": true`), so
//!   no reply tells a guessed name from an unknown one; nothing is refused, and the filter by
//!   person id (`people`) still holds: ids are not names.
//! - **Face points** (`control-protocol.md` §6.4): without `--automation-allow-people` the replies
//!   that tell where a face is ([`FACE_POINTS`]: `slideshow.list`'s `focus`) give each photo's
//!   focus as the photo's centre `[0.5, 0.5]` and no face ([`Policy::people_reply`]); the window's
//!   own show still moves toward the face.
//! - **A person's export folder** (`control-protocol.md` §6.13): an export of a person's section
//!   from the window makes its folder under the person's name; without `--automation-allow-people`
//!   that job's entry reaches automation with its folder, name and written files `null`
//!   ([`withhold_export_folder`]), the surface knowing the job by its id.

use std::path::{Component, Path, PathBuf, Prefix};

use serde_json::{Map, Value};

use crate::registry::Effect;

/// The longest path accepted, in bytes.
pub const MAX_PATH: usize = 4096;

/// The folder that keeps originals; automation never writes into it.
pub const ORIGINALS: &str = ".dalica-originals";

/// The most roots of one kind kept (an MCP client's list beyond it is cut).
pub const MAX_ROOTS: usize = 256;

/// Commands no automation surface runs, whatever it was granted (`security.md` §5.7): removing
/// imported files from a card is the one permanent deletion in Dalica, and only the person at the
/// computer answers its question.
pub const NEVER_AUTOMATED: &[&str] = &["import.removeFromSource"];

/// The commands whose replies carry people data (`control-protocol.md` §6.4): names and nicknames
/// (`name`, `nickname`) and face rectangles (`rect`, `displayed`). Without
/// `--automation-allow-people` an automation surface passes their replies through
/// [`Policy::people_reply`]. A command that comes to answer with people data joins this list.
pub const PEOPLE_REPLIES: &[&str] = &["faces.list", "faces.add", "people.list"];

/// The commands whose replies tell where a face is (`control-protocol.md` §6.4): `slideshow.list`'s
/// `focus`, the face Pan and Zoom moves each photo toward (`{face, point}`). Without
/// `--automation-allow-people` an automation surface passes their replies through
/// [`Policy::people_reply`], which gives every photo's focus as its centre and no face. A command
/// that comes to answer with a face's place joins this list.
pub const FACE_POINTS: &[&str] = &["slideshow.list"];

/// The commands that search by text (`faces.md` §7, `control-protocol.md` §6.4): without
/// `--automation-allow-people` an automation surface runs them with people's names and nicknames
/// left out of the match ([`Policy::people_params`]): `person:` matches nothing and is listed
/// `unavailable`, plain words skip names, everything else matches as before. A command that comes
/// to search by text joins this list.
pub const NAME_SEARCHES: &[&str] = &["library.query", "search.set", "slideshow.list"];

/// The options whose change lets the app reach outside Dalica or decides where it connects
/// (`menus-and-shortcuts.md` §5 Network, M13.28): turning Allow map tiles on, and the tile server,
/// the geocoding service and the proxy. Without `--automation-allow-external`, `options.set`
/// changing one of them is refused like `map.allow` ([`external_option`]); turning Allow map tiles
/// off, the attribution and the log level are not.
pub const EXTERNAL_OPTIONS: &[&str] =
    &["network.allowMapTiles", "network.tileServer", "network.geocoder", "network.proxy", "network.proxyHost", "network.proxyPort"];

/// The param of [`NAME_SEARCHES`] that leaves names out of the match.
pub const WITHHOLD_NAMES: &str = "withholdNames";

/// Keys of a people reply set to `null` without the people permission.
const NAME_KEYS: &[&str] = &["name", "nickname"];
/// Keys of a people reply left out without the people permission.
const RECT_KEYS: &[&str] = &["rect", "displayed"];
/// Keys left out of every reply, whatever was granted: face descriptors never leave the engine.
const NEVER_KEYS: &[&str] = &["embedding", "embeddings", "landmarks"];
/// The deepest a reply is walked.
const MAX_REPLY_DEPTH: usize = 64;

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

    /// The reply of command `id` as this surface may give it (`control-protocol.md` §6.4): face
    /// descriptors (`embedding`, `landmarks`) are always left out; without
    /// `--automation-allow-people` the replies of [`PEOPLE_REPLIES`] have their names and
    /// nicknames `null` and their face rectangles left out, ids and counts kept, and the replies of
    /// [`FACE_POINTS`] give each photo's focus as its centre `[0.5, 0.5]` with no face. Other
    /// replies are returned as they are.
    pub fn people_reply(&self, id: &str, reply: Value) -> Value {
        if FACE_POINTS.contains(&id) && !self.allow_people {
            return centred(reply);
        }
        if !PEOPLE_REPLIES.contains(&id) {
            return reply;
        }
        withhold_people(reply, !self.allow_people)
    }

    /// The params of command `id` as this surface runs it (`faces.md` §7): without
    /// `--automation-allow-people` a search of [`NAME_SEARCHES`] gets `"withholdNames": true`,
    /// whatever the caller sent, so people's names take no part in its match. Other commands, and
    /// every command with the permission, run with the params as given.
    pub fn people_params(&self, id: &str, params: &mut Map<String, Value>) {
        if !self.allow_people && NAME_SEARCHES.contains(&id) {
            params.insert(WITHHOLD_NAMES.into(), Value::Bool(true));
        }
    }

    /// A command with `effect` (`id` naming it): an effect outside Dalica needs
    /// `--automation-allow-external` (§5.6), and a command of [`NEVER_AUTOMATED`] is refused
    /// whatever was granted ([`never_automated`]).
    pub fn check_effect(&self, effect: Effect, id: &str) -> Result<(), Refused> {
        never_automated(id)?;
        if effect == Effect::External && !self.allow_external {
            return Err(Refused::new(
                Refusal::ExternalDenied,
                format!("`{id}` reaches outside Dalica; automation runs it only when started with --automation-allow-external"),
            ));
        }
        Ok(())
    }

    /// Command `id` with `params`: [`Policy::check_effect`], then, without
    /// `--automation-allow-external`, an `options.set` changing an option of [`EXTERNAL_OPTIONS`]
    /// is refused ([`external_option`]).
    pub fn check_command(&self, effect: Effect, id: &str, params: &Map<String, Value>) -> Result<(), Refused> {
        self.check_effect(effect, id)?;
        if !self.allow_external
            && let Some(key) = external_option(id, params)
        {
            return Err(external_option_refused(key));
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

/// Refuses a command of [`NEVER_AUTOMATED`] (`external_denied`, as no grant allows it): every
/// automation surface asks this before it runs a command.
pub fn never_automated(id: &str) -> Result<(), Refused> {
    if NEVER_AUTOMATED.contains(&id) {
        return Err(Refused::new(
            Refusal::ExternalDenied,
            format!("`{id}` never runs over automation: it deletes files from a card, and only the person at the computer may answer its question"),
        ));
    }
    Ok(())
}

/// The option of [`EXTERNAL_OPTIONS`] that command `id` with `params` would change, if it is
/// `options.set` and changes one (`network.allowMapTiles` only when turned on): an effect outside
/// Dalica, which every automation surface refuses without `--automation-allow-external`.
pub fn external_option(id: &str, params: &Map<String, Value>) -> Option<&'static str> {
    if id != "options.set" {
        return None;
    }
    let values = params.get("values")?.as_object()?;
    EXTERNAL_OPTIONS.iter().copied().find(|key| match values.get(*key) {
        None => false,
        Some(v) if *key == "network.allowMapTiles" => *v != Value::Bool(false),
        Some(_) => true,
    })
}

/// The refusal of a change to option `key` of [`EXTERNAL_OPTIONS`].
pub fn external_option_refused(key: &str) -> Refused {
    Refused::new(
        Refusal::ExternalDenied,
        format!("changing `{key}` lets Dalica reach outside; automation changes it only when started with --automation-allow-external"),
    )
}

/// `reply` without face descriptors and, when `names` is set, with every name and nickname `null`
/// and every face rectangle left out, at any depth (a reply deeper than [`MAX_REPLY_DEPTH`] is
/// cut there: its deeper values are left out).
pub fn withhold_people(mut reply: Value, names: bool) -> Value {
    let mut stack: Vec<(&mut Value, usize)> = vec![(&mut reply, 0)];
    while let Some((value, depth)) = stack.pop() {
        match value {
            Value::Object(map) => {
                map.retain(|k, _| !NEVER_KEYS.contains(&k.as_str()) && !(names && RECT_KEYS.contains(&k.as_str())));
                for (k, v) in map.iter_mut() {
                    if (names && NAME_KEYS.contains(&k.as_str())) || depth >= MAX_REPLY_DEPTH {
                        *v = Value::Null;
                    } else {
                        stack.push((v, depth + 1));
                    }
                }
            }
            Value::Array(list) => {
                for v in list.iter_mut() {
                    if depth >= MAX_REPLY_DEPTH {
                        *v = Value::Null;
                    } else {
                        stack.push((v, depth + 1));
                    }
                }
            }
            _ => {}
        }
    }
    reply
}

/// `reply` with every photo's `focus` (`{"<PhotoId>": {face, point}}`) the photo's centre and no
/// face: `{"<PhotoId>": {point: [0.5, 0.5]}}`.
fn centred(mut reply: Value) -> Value {
    if let Some(Value::Object(focus)) = reply.get_mut("focus") {
        for v in focus.values_mut() {
            *v = serde_json::json!({ "point": [0.5, 0.5] });
        }
    }
    reply
}

/// The keys of an export job's `result` (`export.run`'s job: `{total, done, written, skipped,
/// failed, cancelled, folder, name}`) that hold the export folder's name: the folder, its name
/// and the files written into it.
pub const EXPORT_FOLDER_KEYS: &[&str] = &["folder", "name", "written"];

/// A job's entry (as `jobs.list` gives it) with [`EXPORT_FOLDER_KEYS`] of its `result` `null`, for
/// an export whose folder was named from a person's name (a person's section exported from the
/// window, its folder name suggested from theirs) and a client without the people permission
/// (`control-protocol.md` §6.4, §6.13). Which jobs those are is the caller's to know, by the job;
/// never by matching the folder's name against people's names (a guess-oracle). The counts stay.
pub fn withhold_export_folder(mut entry: Value) -> Value {
    if let Some(Value::Object(result)) = entry.get_mut("result") {
        for key in EXPORT_FOLDER_KEYS {
            if let Some(v) = result.get_mut(*key) {
                *v = Value::Null;
            }
        }
    }
    entry
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

/// `path` in its plain form, the one the checks compare and return: on Windows a drive or verbatim
/// path (`\\?\C:\a`, `c:\a`) becomes `C:\a` and a verbatim UNC path (`\\?\UNC\server\share\a`)
/// becomes `\\server\share\a`, but only when every name after the prefix is an ordinary one (no
/// `.`, `..`, `/`, stream, reserved device name or trailing dot or space: without the prefix those
/// would name something else, so such a path keeps its verbatim form, which [`lexical`] refuses
/// when it comes back). Anything else, and every path elsewhere, is returned unchanged. Touches
/// nothing.
pub fn plain(path: &Path) -> PathBuf {
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else { return path.to_path_buf() };
    let head = match prefix.kind() {
        Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", char::from(d.to_ascii_uppercase())),
        Prefix::VerbatimUNC(server, share) => match (server.to_str(), share.to_str()) {
            (Some(server), Some(share)) if !server.is_empty() && !share.is_empty() => format!(r"\\{server}\{share}"),
            _ => return path.to_path_buf(),
        },
        _ => return path.to_path_buf(),
    };
    // The names are read with the prefix's own rules (a verbatim path does not split at `/`).
    let rest = parts.as_path();
    if parts.next() != Some(Component::RootDir) {
        return path.to_path_buf();
    }
    let ordinary = |c: Component<'_>| match c {
        Component::Normal(name) => name.to_str().is_some_and(|n| !n.contains(['/', '\\']) && windows_problem(n).is_none()),
        _ => false,
    };
    if !parts.all(ordinary) {
        return path.to_path_buf();
    }
    let mut out = std::ffi::OsString::from(head);
    out.push(rest.as_os_str());
    PathBuf::from(out)
}

/// `path` resolved through its links, in its [`plain`] form.
fn resolve(path: &Path) -> Result<PathBuf, Refused> {
    std::fs::canonicalize(path).map(|p| plain(&p)).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Refused::new(Refusal::NotFound, format!("{} does not exist", path.display())),
        _ => Refused::new(Refusal::PathDenied, format!("{} cannot be resolved: {e}", path.display())),
    })
}

/// `roots` resolved and in their [`plain`] form; roots that do not exist hold nothing.
fn resolved_roots(roots: &[PathBuf]) -> impl Iterator<Item = PathBuf> + '_ {
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).map(|r| plain(&r))
}

/// Whether the resolved `path` is inside one of `roots` (each resolved; roots that do not exist
/// hold nothing), both in their [`plain`] form, comparing components exactly: the comparison that
/// allows.
fn inside(path: &Path, roots: &[PathBuf]) -> bool {
    let path = plain(path);
    resolved_roots(roots).any(|r| path.starts_with(&r))
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
    let at = folded(&plain(path));
    resolved_roots(roots).any(|r| at.starts_with(&folded(&r)))
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

#[cfg(test)]
#[cfg(windows)]
mod windows_tests {
    use super::*;

    fn code<T>(r: Result<T, Refused>) -> &'static str {
        match r {
            Ok(_) => "ok",
            Err(e) => e.code(),
        }
    }

    /// `p` (plain, `C:\…`) as `\\?\C:\…`.
    fn verbatim(p: &Path) -> PathBuf {
        PathBuf::from(format!(r"\\?\{}", p.display()))
    }

    /// `p` (plain, `C:\…`) with a lower-case drive letter.
    fn lower_drive(p: &Path) -> PathBuf {
        let s = p.to_string_lossy();
        let (drive, rest) = s.split_at(1);
        PathBuf::from(format!("{}{rest}", drive.to_ascii_lowercase()))
    }

    #[test]
    fn the_plain_form_drops_a_verbatim_prefix_only_where_it_names_the_same_place() {
        for (raw, want) in [
            (r"\\?\C:\Photos\a.jpg", r"C:\Photos\a.jpg"),
            (r"\\?\c:\Photos", r"C:\Photos"),
            (r"c:\Photos", r"C:\Photos"),
            (r"C:\Photos", r"C:\Photos"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\share\Photos", r"\\server\share\Photos"),
            (r"\\server\share\Photos", r"\\server\share\Photos"),
        ] {
            assert_eq!(plain(Path::new(raw)).as_os_str(), std::ffi::OsStr::new(want), "{raw}");
        }
        // Without the prefix these would name something else: kept as they are.
        for raw in [r"\\?\C:\Photos\NUL", r"\\?\C:\Photos\a.", r"\\?\C:\Photos\a:s", r"\\?\C:\a\..\b", r"\\?\C:\a/b", r"\\.\C:\x", r"\\?\Volume{0}\x"]
        {
            assert_eq!(plain(Path::new(raw)).as_os_str(), std::ffi::OsStr::new(raw), "{raw}");
        }
    }

    #[test]
    fn roots_and_paths_in_verbatim_plain_short_and_lower_case_drive_forms_compare_equal_and_refusals_hold_in_each() {
        let dir = tempfile::tempdir().unwrap();
        // The temporary folder as given (on the runner an 8.3 short name, `RUNNER~1`) and resolved.
        let given = dir.path().to_path_buf();
        let root = plain(&std::fs::canonicalize(&given).unwrap());
        assert!(!root.to_string_lossy().starts_with(r"\\?\"), "{}", root.display());
        let (photos, extra, out, other) = (root.join("photos"), root.join("extra"), root.join("out"), root.join("other"));
        for d in [&photos, &extra.join("2026-07"), &out, &other, &photos.join(ORIGINALS), &out.join(ORIGINALS)] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(photos.join("taken.jpg"), b"x").unwrap();
        let as_given = |p: &Path| given.join(p.strip_prefix(&root).unwrap());
        let root_forms: [&dyn Fn(&Path) -> PathBuf; 4] = [&|p| p.to_path_buf(), &verbatim, &lower_drive, &as_given];
        let path_forms: [&dyn Fn(&Path) -> PathBuf; 3] = [&|p| p.to_path_buf(), &lower_drive, &as_given];
        for (i, root_form) in root_forms.iter().enumerate() {
            let p = Policy::with_roots(&[root_form(&extra)], &[root_form(&out)]);
            let watched = vec![root_form(&photos)];
            for (j, f) in path_forms.iter().enumerate() {
                let at = format!("root form {i}, path form {j}");
                // Allowed, answered in the plain form.
                assert_eq!(p.check_read(&f(&extra.join("2026-07")), "path", &watched), Ok(extra.join("2026-07")), "{at}");
                assert_eq!(p.check_read(&f(&photos), "path", &watched), Ok(photos.clone()), "{at}");
                assert_eq!(p.check_write(&f(&out.join("shot.png")), "path", &watched), Ok(out.join("shot.png")), "{at}");
                assert_eq!(p.check_output(&f(&out.join("Exports")), "dir", &watched, Output::Folder), Ok(out.join("Exports")), "{at}");
                assert_eq!(p.check_output(&f(&photos.join("new.jpg")), "path", &watched, Output::File), Ok(photos.join("new.jpg")), "{at}");
                // Refused.
                assert_eq!(code(p.check_read(&f(&extra.join("..").join("other")), "path", &watched)), "path_denied", "{at}: `..`");
                assert_eq!(code(p.check_read(&f(&other), "path", &watched)), "path_denied", "{at}: outside");
                assert_eq!(code(p.check_write(&f(&photos.join("shot.png")), "path", &watched)), "path_denied", "{at}: watched");
                assert_eq!(code(p.check_write(&f(&out.join(ORIGINALS).join("a.png")), "path", &watched)), "path_denied", "{at}: originals");
                let originals = photos.join(ORIGINALS).join("a.jpg");
                assert_eq!(code(p.check_output(&f(&originals), "path", &watched, Output::File)), "path_denied", "{at}: originals");
                assert_eq!(code(p.check_output(&f(&photos.join("taken.jpg")), "path", &watched, Output::File)), "path_denied", "{at}: existing");
                let upper = PathBuf::from(f(&photos).to_string_lossy().to_uppercase()).join("shot.png");
                assert_eq!(code(p.check_write(&upper, "path", &watched)), "path_denied", "{at}: watched, another case");
            }
            // A verbatim path param skips Windows' own checks: refused, allowed place or not.
            for path in [extra.join("2026-07"), out.join("shot.png"), photos.join("new.jpg"), other.clone(), photos.join(ORIGINALS).join("a.jpg")] {
                assert_eq!(code(p.check_read(&verbatim(&path), "path", &watched)), "path_denied", "root form {i}: {}", path.display());
                assert_eq!(code(p.check_write(&verbatim(&path), "path", &watched)), "path_denied", "root form {i}: {}", path.display());
                assert_eq!(
                    code(p.check_output(&verbatim(&path), "path", &watched, Output::File)),
                    "path_denied",
                    "root form {i}: {}",
                    path.display()
                );
            }
        }
    }
}
