//! The watched folders (`data-model.md` §4, §7; `organizing.md` §2): the Folder Manager's rules — a
//! mode per folder (Scan Always, Scan Once, Remove) and the "Find faces in this folder" flag, each
//! inherited by subfolders unless they set their own ([`dalica_scan::Rules`]) — kept in
//! `folders.json` in the library directory, read when the session opens and written through the
//! I/O worker.
//!
//! - **Changing rules** (`Session::change_rules`, run by `folders.set`, `folder.remove`,
//!   `folders.add` and by undo and redo): folders under a changed folder that are no longer
//!   indexed leave the library (nothing on disk changes; their sidecars stay as they are), and the
//!   regions that became indexed are scanned in the background ([`Rules::needs_scan`]). A Scan Once
//!   region already in the library (brought back by an undo) is not scanned again: it stays frozen.
//!   The rules a change replaced go on the command's undo step ([`Modes`]), so undo restores them
//!   together with the library folders it removed.
//! - **`folders.json` v2:** `{"version": 2, "folders": [{"path", "mode"?, "faces"?, "indexed"?}]}`.
//!   A v1 file (`{"version": 1, "folders": [{"path"}]}`, every folder watched) is read as Scan
//!   Always rules and written back as v2 at once, atomically. Keys this version does not know, at
//!   the top and in each entry, and entries it cannot read, are kept verbatim. A file that is not
//!   such an object is kept aside as `.corrupt-<time>`.
//! - **Browsing** (`folders.browse`): the standard places and volumes, and a folder's subfolders,
//!   are read on worker threads ([`dalica_scan::volumes`] reads the disk, and a sleeping network
//!   volume can stall it). The command answers from what the workers brought back, or says the
//!   answer is pending and asks a worker; [`Session::poll`] takes the answers in.
//! - **Moved and renamed folders** (M9): when Dalica moves or renames a folder, the entries at and
//!   under it follow it ([`Session::rebase_folders`]): each keeps its rule and its unknown keys
//!   under the new path, and `folders.json` is written again.
//! - **The drive question:** setting a volume root or the home folder to Scan Always needs
//!   `confirm`. Which folders those are is known without touching the disk: a path with no parent,
//!   the home folder (from the environment, or the places a worker read) and the volumes the last
//!   browse listed.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use dalica_core::{FolderId, Timestamp};
use dalica_library::Op;
use dalica_scan::{Children, Mode, Place, PlaceKind, Places, Rule, Rules, Volume, VolumeKind};
use dalica_store::Recovery;
use serde_json::{Map, Value, json};

use crate::error::CmdError;
use crate::session::{Msg, Session};

/// The watched-folders file in the library directory (`data-model.md` §4).
pub const FOLDERS_FILE: &str = "folders.json";
/// The version of `folders.json` this version writes.
pub const FOLDERS_VERSION: u64 = 2;
/// The question a volume root or the home folder set to Scan Always asks first
/// (`organizing.md` §2).
pub const DRIVE_QUESTION: &str = "Watching a whole drive can slow down your computer and fill the library with non-photo images. It's usually better to choose a few folders. Watch the whole drive anyway?";
/// The most `folders.browse` threads at once; more requests wait their turn.
const BROWSE_THREADS: usize = 4;
/// The most browse answers kept; the cache starts over when it is full.
const BROWSE_CACHE: usize = 512;

/// Rules a change replaced: each folder with the rule to put back (an empty rule removes it). An
/// undo step carries one (`undo.rs`), and putting it back gives the rules to redo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modes(pub(crate) Vec<(PathBuf, Rule)>);

impl Modes {
    /// The folders whose rules it puts back.
    pub fn folders(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(|(p, _)| p.as_path())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What a change of rules did.
#[derive(Debug, Default)]
pub(crate) struct Changed {
    /// The rules it replaced.
    pub(crate) before: Modes,
    pub(crate) removed_folders: usize,
    pub(crate) removed_photos: usize,
    /// The regions it started scans of.
    pub(crate) scans: Vec<PathBuf>,
}

/// One `folders.browse` answer: the places and volumes (for no path) or a folder's subfolders.
#[derive(Clone, Debug)]
pub(crate) enum Answer {
    Places { known: Places, existing: Vec<Place>, volumes: Vec<Volume> },
    Children(Children),
    Failed(String),
}

/// The browse workers and their answers.
pub(crate) struct Browse {
    answers: HashMap<Option<PathBuf>, Answer>,
    queued: VecDeque<Option<PathBuf>>,
    in_flight: HashSet<Option<PathBuf>>,
    tx: Sender<(Option<PathBuf>, Answer)>,
    rx: Receiver<(Option<PathBuf>, Answer)>,
    /// Places and volumes set by [`Session::set_places`], used instead of the system's.
    fixed: Option<(Places, Vec<Volume>)>,
}

impl Default for Browse {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Browse { answers: HashMap::new(), queued: VecDeque::new(), in_flight: HashSet::new(), tx, rx, fixed: None }
    }
}

/// The watched folders of a session.
#[derive(Default)]
pub(crate) struct Folders {
    pub(crate) rules: Rules,
    /// The units (folders set to Scan Always or Scan Once), by path.
    pub(crate) roots: Vec<PathBuf>,
    /// `folders.json` existed when the session opened, or has been written since.
    pub(crate) saved: bool,
    /// Top-level keys of `folders.json` this version does not know.
    extra: Map<String, Value>,
    /// Keys of each entry this version does not know.
    entry_extra: BTreeMap<PathBuf, Map<String, Value>>,
    /// Entries this version cannot read, written back as they were.
    unreadable: Vec<Value>,
    /// The rules the running command replaced, for its undo step (taken by `Session::execute`).
    pub(crate) change: Option<Modes>,
    pub(crate) browse: Browse,
}

impl Folders {
    /// The watched folders from `folders.json` (call it off the UI thread); a damaged file is kept
    /// aside and reported. A v1 file is written back as v2 here, atomically.
    pub(crate) fn read(path: &Path, now: Timestamp) -> (Folders, Option<Recovery>) {
        let read = match dalica_fs::read(path, 64 << 20) {
            Ok(Some(r)) => r,
            Ok(None) => return (Folders::default(), None),
            Err(e) => {
                let folders = Folders { saved: true, ..Folders::default() };
                return (folders, Some(Recovery::RebuildNeeded { reason: format!("folders.json: {e}") }));
            }
        };
        match Folders::parse(&read.bytes) {
            Some((mut folders, version)) => {
                folders.saved = true;
                if version < FOLDERS_VERSION {
                    // Written back as v2 now, so the rules never depend on reading v1 again; the
                    // write expects the file just read, so a concurrent change is never overwritten.
                    if let Err(e) = dalica_fs::replace(path, &folders.to_json(), dalica_fs::Expect::Unchanged(read.identity)) {
                        log::warn!("folders.json: not written back as v{FOLDERS_VERSION}: {e}");
                    }
                }
                (folders, None)
            }
            None => {
                let stamp = dalica_core::DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
                let kept = dalica_fs::set_aside(path, &format!("corrupt-{stamp}Z")).map(|p| p.display().to_string()).unwrap_or_default();
                let reason = format!("folders.json was damaged and kept as {kept}; add your folders again");
                (Folders { saved: true, ..Folders::default() }, Some(Recovery::RebuildNeeded { reason }))
            }
        }
    }

    /// Parses `folders.json` (any version) into rules, with its version; `None` when it is not an
    /// object with a `folders` array.
    pub(crate) fn parse(bytes: &[u8]) -> Option<(Folders, u64)> {
        let Value::Object(mut top) = serde_json::from_slice::<Value>(bytes).ok()? else { return None };
        let version = match top.remove("version") {
            None => 1,
            Some(v) => v.as_u64()?,
        };
        let Value::Array(entries) = top.remove("folders")? else { return None };
        let mut folders = Folders { extra: top, ..Folders::default() };
        for entry in entries {
            if folders.rules.len() >= dalica_scan::rules::MAX_RULES {
                folders.unreadable.push(entry);
                continue;
            }
            match read_entry(&entry, version) {
                Some((path, rule, extra)) if folders.rules.get(&path).is_none() && !rule.is_empty() => {
                    folders.rules.set(path.clone(), rule);
                    if !extra.is_empty() {
                        folders.entry_extra.insert(path, extra);
                    }
                }
                _ => folders.unreadable.push(entry),
            }
        }
        folders.refresh_roots();
        Some((folders, version))
    }

    /// `folders.json` as written (v2).
    pub(crate) fn to_json(&self) -> Vec<u8> {
        let mut entries: Vec<Value> = self
            .rules
            .iter()
            .map(|(path, rule)| {
                let mut e = self.entry_extra.get(path).cloned().unwrap_or_default();
                e.insert("path".into(), json!(path));
                match rule.mode {
                    Some(m) => e.insert("mode".into(), json!(m.id())),
                    None => e.remove("mode"),
                };
                match rule.faces {
                    Some(f) => e.insert("faces".into(), json!(f)),
                    None => e.remove("faces"),
                };
                if rule.mode == Some(Mode::Once) && rule.indexed {
                    e.insert("indexed".into(), json!(true));
                } else {
                    e.remove("indexed");
                }
                Value::Object(e)
            })
            .collect();
        entries.extend(self.unreadable.iter().cloned());
        let mut top = self.extra.clone();
        top.insert("version".into(), json!(FOLDERS_VERSION));
        top.insert("folders".into(), Value::Array(entries));
        let mut bytes = serde_json::to_vec_pretty(&Value::Object(top)).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    /// Moves the entries at and under `from` to the same place under `to` (rules and unknown keys;
    /// an entry already at a new path is replaced, as the folder there is the moved one). Entries
    /// it cannot read are kept as they are. Returns how many entries moved.
    pub(crate) fn rebase(&mut self, from: &Path, to: &Path) -> usize {
        if from == to {
            return 0;
        }
        let moved: Vec<(PathBuf, PathBuf, Rule)> = self
            .rules
            .iter()
            .filter_map(|(p, rule)| {
                let rest = p.strip_prefix(from).ok()?;
                let new = if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) };
                Some((p.to_path_buf(), new, *rule))
            })
            .collect();
        let mut extras = Vec::new();
        for (old, _, _) in &moved {
            self.rules.set(old.clone(), Rule::default());
            extras.push(self.entry_extra.remove(old));
        }
        for ((_, new, rule), extra) in moved.iter().zip(extras) {
            self.rules.set(new.clone(), *rule);
            match extra {
                Some(e) => {
                    self.entry_extra.insert(new.clone(), e);
                }
                None => {
                    self.entry_extra.remove(new);
                }
            }
        }
        self.refresh_roots();
        moved.len()
    }

    /// Brings the list of units in line with the rules.
    pub(crate) fn refresh_roots(&mut self) {
        self.roots = self.rules.units();
    }
}

/// `folders.json` as [`Folders::parse`] reads it, laid out for the fuzz target `folders_json`
/// (`crates/engine/fuzz`, `security.md` §7.1): not an API.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedFolders {
    /// The file's version (1 when it names none).
    pub version: u64,
    /// Each folder read, once, with its rule, in path order.
    pub rules: Vec<(PathBuf, Rule)>,
    /// The units (folders set to Scan Always or Scan Once).
    pub roots: Vec<PathBuf>,
    /// Top-level keys this version does not know.
    pub extra: Map<String, Value>,
    /// Keys of each entry this version does not know.
    pub entry_extra: BTreeMap<PathBuf, Map<String, Value>>,
    /// Entries it could not read, as they were.
    pub unreadable: Vec<Value>,
    /// The file as this version writes it back (v2).
    pub written: Vec<u8>,
}

/// [`Folders::parse`] for the fuzz target: `None` when the file is refused (and a session keeps it
/// aside as damaged), else what it read and what it would write back.
#[doc(hidden)]
pub fn parse_for_fuzzing(bytes: &[u8]) -> Option<ParsedFolders> {
    let (f, version) = Folders::parse(bytes)?;
    Some(ParsedFolders {
        version,
        rules: f.rules.iter().map(|(p, r)| (p.to_path_buf(), *r)).collect(),
        roots: f.roots.clone(),
        written: f.to_json(),
        extra: f.extra,
        entry_extra: f.entry_extra,
        unreadable: f.unreadable,
    })
}

/// One entry of `folders.json`: its folder, rule and unknown keys. In a v1 file every folder is
/// watched; from v2 on, an entry without a mode only sets the faces flag.
fn read_entry(entry: &Value, version: u64) -> Option<(PathBuf, Rule, Map<String, Value>)> {
    let mut e = entry.as_object()?.clone();
    let path = dalica_scan::folder_path(Path::new(e.remove("path")?.as_str()?)).ok()?;
    let mode = match e.remove("mode") {
        None if version < 2 => Some(Mode::Always),
        None | Some(Value::Null) => None,
        Some(v) => Some(Mode::from_id(v.as_str()?)?),
    };
    let faces = match e.remove("faces") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool()?),
    };
    let indexed = match e.remove("indexed") {
        None => false,
        Some(v) => v.as_bool()? && mode == Some(Mode::Once),
    };
    Some((path, Rule { mode, faces, indexed }, e))
}

/// The places the environment names, without reading the disk: under `$DALICA_PLACES_HOME`, else
/// the home (`HOME`, `USERPROFILE` on Windows) and its standard folders. A worker's answer replaces
/// it with the system's own (the XDG user-dirs file may move them).
fn env_places() -> Places {
    let abs = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    match abs(dalica_scan::volumes::PLACES_HOME_ENV).or_else(|| abs(if cfg!(windows) { "USERPROFILE" } else { "HOME" })) {
        Some(home) => Places::under(&home),
        None => Places::default(),
    }
}

/// A place's English name, as the Folder Manager shows it.
pub fn place_name(kind: PlaceKind) -> &'static str {
    match kind {
        PlaceKind::Pictures => "Pictures",
        PlaceKind::Documents => "Documents",
        PlaceKind::Desktop => "Desktop",
        PlaceKind::Home => "Home",
    }
}

fn place_id(kind: PlaceKind) -> &'static str {
    match kind {
        PlaceKind::Pictures => "pictures",
        PlaceKind::Documents => "documents",
        PlaceKind::Desktop => "desktop",
        PlaceKind::Home => "home",
    }
}

fn volume_id(kind: VolumeKind) -> &'static str {
    match kind {
        VolumeKind::System => "system",
        VolumeKind::Mounted => "mounted",
        VolumeKind::Network => "network",
    }
}

/// Reads one browse answer (on a worker thread).
fn browse_answer(path: Option<&Path>, fixed: Option<(Places, Vec<Volume>)>) -> Answer {
    match path {
        None => {
            let (known, volumes) = fixed.unwrap_or_else(|| (Places::system(), dalica_scan::volumes()));
            Answer::Places { existing: known.existing(), known, volumes }
        }
        Some(p) => match dalica_scan::children(p) {
            Ok(c) => Answer::Children(c),
            Err(e) => Answer::Failed(format!("{}: {e}", p.display())),
        },
    }
}

impl Session {
    /// The units: the folders set to Scan Always or Scan Once, by path.
    pub fn roots(&self) -> &[PathBuf] {
        &self.folders.roots
    }

    /// The Folder Manager's rules.
    pub fn folder_rules(&self) -> &Rules {
        &self.folders.rules
    }

    /// A library that has never had a folder: no `folders.json` when it opened and none added
    /// since. The UI shows the first-run panel for it (`organizing.md` §1).
    pub fn is_new_library(&self) -> bool {
        !self.folders.saved && self.folders.roots.is_empty() && self.library.folders().next().is_none()
    }

    /// Watches `path` (Scan Always) and scans it in the background; a folder already set to Scan
    /// Always is scanned again. Returns whether it was new. No undo step (`folders.add`).
    pub(crate) fn add_root(&mut self, path: PathBuf) -> bool {
        let old = self.folders.rules.get(&path).copied().unwrap_or_default();
        if old.mode == Some(Mode::Always) {
            self.start_region_scan(&path);
            return false;
        }
        let changed = self.change_rules(vec![(path.clone(), Rule { mode: Some(Mode::Always), faces: old.faces, indexed: false })]);
        if !changed.scans.iter().any(|s| path.starts_with(s)) {
            self.start_region_scan(&path);
        }
        true
    }

    /// Sets the rules of these folders (an empty rule removes one): folders under them that are no
    /// longer indexed leave the library, and regions that became indexed are scanned. Returns what
    /// it did, with the rules it replaced. The library changes go through [`Session::apply`], so a
    /// command collects them for its undo step. Never touches the disk.
    pub(crate) fn change_rules(&mut self, changes: Vec<(PathBuf, Rule)>) -> Changed {
        let before = self.folders.rules.clone();
        let mut replaced = Vec::new();
        for (path, rule) in changes {
            let old = self.folders.rules.get(&path).copied().unwrap_or_default();
            if old == rule {
                continue;
            }
            self.folders.rules.set(path.clone(), rule);
            if rule.is_empty() {
                self.folders.entry_extra.remove(&path);
            }
            replaced.push((path, old));
        }
        if replaced.is_empty() {
            return Changed::default();
        }
        self.folders.refresh_roots();
        let changed: Vec<PathBuf> = replaced.iter().map(|(p, _)| p.clone()).collect();
        let rules = &self.folders.rules;
        let gone: Vec<(FolderId, usize)> = self
            .library
            .folders()
            .filter(|f| changed.iter().any(|p| f.path.starts_with(p)) && !rules.indexes(&f.path))
            .map(|f| (f.id, self.library.photos_in(f.id).len()))
            .collect();
        let mut out = Changed::default();
        for (id, photos) in gone {
            match self.apply(Op::RemoveFolder { id }) {
                Ok(_) => {
                    out.removed_folders += 1;
                    out.removed_photos += photos;
                }
                Err(e) => self.scan_warnings.push(format!("folder {id} not removed from the library: {e}")),
            }
        }
        let rules = &self.folders.rules;
        let in_library = |region: &Path| {
            let excluded = rules.inside(region);
            self.library.folders().any(|f| f.path.starts_with(region) && !excluded.iter().any(|x| f.path.starts_with(x)))
        };
        let scans: Vec<PathBuf> = changed
            .iter()
            .filter(|p| {
                let now = rules.effective(p).mode;
                Rules::needs_scan(before.effective(p).mode, now) && !(now == Mode::Once && in_library(p))
            })
            .cloned()
            .collect();
        // A region inside another one scanned here is walked by that scan.
        let scans: Vec<PathBuf> = scans
            .iter()
            .filter(|p| !scans.iter().any(|q| q != *p && p.starts_with(q) && !rules.inside(q).iter().any(|x| p.starts_with(x))))
            .cloned()
            .collect();
        for region in &scans {
            self.start_region_scan(region);
        }
        self.save_folders();
        out.before = Modes(replaced);
        out.scans = scans;
        out
    }

    /// Puts back the rules an undo or redo step carries and returns the ones they replaced (for
    /// the opposite step).
    pub(crate) fn swap_modes(&mut self, modes: Modes) -> Modes {
        self.change_rules(modes.0).before
    }

    /// A folder Dalica moved or renamed from `from` to `to`: the watched-folder entries at and under
    /// it follow it, keeping their rules and unknown keys, and `folders.json` is written through the
    /// I/O worker when any moved; the watcher follows at the next poll. Nothing else changes: the
    /// library's folders follow through the operation's own ops. Returns how many entries moved.
    /// Never touches the disk.
    pub fn rebase_folders(&mut self, from: &Path, to: &Path) -> usize {
        let n = self.folders.rebase(from, to);
        if n > 0 {
            self.save_folders();
        }
        n
    }

    /// Writes `folders.json` through the I/O worker.
    pub(crate) fn save_folders(&mut self) {
        self.folders.saved = true;
        let bytes = self.folders.to_json();
        self.send(Msg::Write(self.dir.join(FOLDERS_FILE), bytes));
    }

    /// Uses these places and volumes instead of the system's (tests, demos): for browsing, friendly
    /// names and the drive question.
    pub fn set_places(&mut self, places: Places, volumes: Vec<Volume>) {
        self.folders.browse.fixed = Some((places, volumes));
        self.folders.browse.answers.remove(&None);
    }

    /// The places known now, without reading the disk.
    fn known_places(&self) -> Places {
        if let Some((places, _)) = &self.folders.browse.fixed {
            return places.clone();
        }
        match self.folders.browse.answers.get(&None) {
            Some(Answer::Places { known, .. }) => known.clone(),
            _ => env_places(),
        }
    }

    /// The volumes known now (the last browse's), without reading the disk.
    fn known_volumes(&self) -> Vec<Volume> {
        if let Some((_, volumes)) = &self.folders.browse.fixed {
            return volumes.clone();
        }
        match self.folders.browse.answers.get(&None) {
            Some(Answer::Places { volumes, .. }) => volumes.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether `path` is a volume root or the home folder: watching it asks first.
    pub(crate) fn is_drive_or_home(&self, path: &Path) -> bool {
        path.parent().is_none() || self.known_places().home.as_deref() == Some(path) || self.known_volumes().iter().any(|v| v.path == path)
    }

    /// The friendly name of `path` ("Pictures", "Home") if it is a standard place.
    pub(crate) fn friendly_name(&self, path: &Path) -> Option<&'static str> {
        self.known_places().all().into_iter().find(|p| p.path == path).map(|p| place_name(p.kind))
    }

    /// How the Folder Manager shows `path`: its effective mode, whether it sets it, faces.
    pub(crate) fn folder_state(&self, path: &Path) -> Map<String, Value> {
        let rules = &self.folders.rules;
        let e = rules.effective(path);
        let mut row = Map::new();
        row.insert("path".into(), json!(path));
        row.insert("mode".into(), json!(e.mode.id()));
        row.insert("explicit".into(), json!(e.explicit(path)));
        row.insert("faces".into(), json!(rules.faces(path)));
        row
    }

    /// The `folders.browse` answer for `path` (`None`: the places and volumes) if a worker has
    /// brought it; otherwise asks one (unless one is on it) and gives `None`. `refresh` drops the
    /// cached answer first. A failure is given once, then forgotten.
    pub(crate) fn browse(&mut self, path: Option<PathBuf>, refresh: bool) -> Option<Answer> {
        let b = &mut self.folders.browse;
        if refresh && !b.in_flight.contains(&path) {
            b.answers.remove(&path);
        }
        if let Some(answer) = b.answers.get(&path) {
            let answer = answer.clone();
            if matches!(answer, Answer::Failed(_)) {
                b.answers.remove(&path);
            }
            return Some(answer);
        }
        if !b.in_flight.contains(&path) && !b.queued.contains(&path) {
            b.queued.push_back(path);
        }
        self.start_browse_workers();
        None
    }

    /// Starts queued browse requests while fewer than [`BROWSE_THREADS`] run.
    fn start_browse_workers(&mut self) {
        let b = &mut self.folders.browse;
        while b.in_flight.len() < BROWSE_THREADS {
            let Some(path) = b.queued.pop_front() else { break };
            let (tx, fixed, key) = (b.tx.clone(), b.fixed.clone(), path.clone());
            let spawned = std::thread::Builder::new().name("dalica-browse".into()).spawn(move || {
                let answer = browse_answer(key.as_deref(), fixed);
                let _ = tx.send((key, answer));
            });
            match spawned {
                Ok(_) => {
                    b.in_flight.insert(path);
                }
                Err(e) => {
                    b.answers.insert(path, Answer::Failed(format!("cannot start a browse: {e}")));
                }
            }
        }
    }

    /// Takes in the browse answers that arrived (with `block`, waits until none is outstanding).
    /// Returns how many it took.
    pub(crate) fn poll_browse(&mut self, block: bool) -> usize {
        let mut n = 0;
        loop {
            let b = &mut self.folders.browse;
            let message = if block && !b.in_flight.is_empty() { b.rx.recv().map_err(|_| TryRecvError::Disconnected) } else { b.rx.try_recv() };
            match message {
                Ok((path, answer)) => {
                    b.in_flight.remove(&path);
                    if b.answers.len() >= BROWSE_CACHE {
                        b.answers.clear();
                    }
                    b.answers.insert(path, answer);
                    n += 1;
                    self.start_browse_workers();
                }
                Err(_) => return n,
            }
        }
    }

    /// Browse requests asked and not answered yet.
    pub fn browse_pending(&self) -> usize {
        self.folders.browse.in_flight.len() + self.folders.browse.queued.len()
    }

    /// An answer as `folders.browse` replies it, with each row's state.
    pub(crate) fn browse_reply(&self, path: Option<&Path>, answer: Answer) -> Result<Value, CmdError> {
        let row = |p: &Path, fields: &[(&str, Value)]| {
            let mut row = self.folder_state(p);
            for (k, v) in fields {
                row.insert((*k).to_string(), v.clone());
            }
            Value::Object(row)
        };
        match answer {
            Answer::Failed(e) => Err(CmdError::NotFound(e)),
            Answer::Places { existing, volumes, .. } => {
                let places: Vec<Value> =
                    existing.iter().map(|p| row(&p.path, &[("kind", json!(place_id(p.kind))), ("name", json!(place_name(p.kind)))])).collect();
                let volumes: Vec<Value> =
                    volumes.iter().map(|v| row(&v.path, &[("kind", json!(volume_id(v.kind))), ("name", json!(v.name))])).collect();
                Ok(json!({ "pending": false, "path": null, "places": places, "volumes": volumes }))
            }
            Answer::Children(c) => {
                let folders: Vec<Value> = c.folders.iter().map(|f| row(&f.path, &[("name", json!(f.name)), ("link", json!(f.is_link))])).collect();
                Ok(json!({ "pending": false, "path": path, "folders": folders, "truncated": c.truncated }))
            }
        }
    }
}

// Its paths are Unix paths; the same parsing runs on every system in `tests/folders.rs`.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn v1_reads_as_scan_always_and_v2_keeps_what_it_does_not_know() {
        let (f, v) = Folders::parse(br#"{"version":1,"folders":[{"path":"/p/a"},{"path":"/p/b","note":"x"}]}"#).unwrap();
        assert_eq!(v, 1);
        assert_eq!(f.roots, [PathBuf::from("/p/a"), PathBuf::from("/p/b")]);
        let written: Value = serde_json::from_slice(&f.to_json()).unwrap();
        assert_eq!(written["version"], 2);
        assert_eq!(written["folders"][1], json!({ "path": "/p/b", "mode": "always", "note": "x" }));
        let v2 = br#"{"version":2,"future":[1],"folders":[{"path":"/p","mode":"once","indexed":true,"faces":false},{"path":"/p/x","mode":"sometimes"},{"path":"rel"},{"path":"/p/y","faces":true}]}"#;
        let (f, _) = Folders::parse(v2).unwrap();
        assert_eq!(f.rules.get(Path::new("/p")), Some(&Rule { mode: Some(Mode::Once), faces: Some(false), indexed: true }));
        assert_eq!(f.rules.get(Path::new("/p/y")), Some(&Rule { mode: None, faces: Some(true), indexed: false }));
        assert_eq!(f.roots, [PathBuf::from("/p")]);
        let again: Value = serde_json::from_slice(&f.to_json()).unwrap();
        assert_eq!(again["future"], json!([1]));
        let entries = again["folders"].as_array().unwrap();
        assert!(entries.contains(&json!({ "path": "/p/x", "mode": "sometimes" })), "an entry it cannot read is kept");
        assert!(entries.contains(&json!({ "path": "rel" })));
        assert!(Folders::parse(b"[]").is_none());
        assert!(Folders::parse(br#"{"folders":3}"#).is_none());
    }

    #[test]
    fn rebase_moves_the_entries_at_and_under_a_folder_and_keeps_the_others() {
        let file = br#"{"version":2,"folders":[
            {"path":"/p/a","mode":"always","note":"kept"},
            {"path":"/p/a/b","mode":"remove"},
            {"path":"/p/a/b/c","faces":true},
            {"path":"/p/ab","mode":"once"},
            {"path":"/q","mode":"always"},
            {"path":"/q/z/a","mode":"once","stale":1},
            {"path":"/p/a/x","mode":"sometimes"}]}"#;
        let (mut f, _) = Folders::parse(file).unwrap();
        assert_eq!(f.rebase(Path::new("/p/a"), Path::new("/q/z/a")), 3);
        let rule = |p: &str| f.rules.get(Path::new(p)).copied();
        assert_eq!(rule("/q/z/a"), Some(Rule { mode: Some(Mode::Always), faces: None, indexed: false }), "the moved entry replaces a stale one");
        assert_eq!(rule("/q/z/a/b").and_then(|r| r.mode), Some(Mode::Remove));
        assert_eq!(rule("/q/z/a/b/c").and_then(|r| r.faces), Some(true));
        assert_eq!(rule("/p/a"), None);
        assert_eq!(rule("/p/a/b"), None);
        assert_eq!(rule("/p/ab").and_then(|r| r.mode), Some(Mode::Once), "a sibling whose name starts the same stays");
        assert_eq!(rule("/q").and_then(|r| r.mode), Some(Mode::Always));
        assert_eq!(f.roots, [PathBuf::from("/p/ab"), PathBuf::from("/q"), PathBuf::from("/q/z/a")], "the units follow");
        let written: Value = serde_json::from_slice(&f.to_json()).unwrap();
        let entries = written["folders"].as_array().unwrap();
        assert!(entries.contains(&json!({ "path": "/q/z/a", "mode": "always", "note": "kept" })), "unknown keys travel");
        assert!(entries.contains(&json!({ "path": "/p/a/x", "mode": "sometimes" })), "an entry it cannot read is kept as it is");
        assert!(!entries.iter().any(|e| e.get("stale").is_some()));
        assert_eq!(f.rebase(Path::new("/nowhere"), Path::new("/q/n")), 0);
        assert_eq!(f.rebase(Path::new("/q"), Path::new("/q")), 0);
    }
}
