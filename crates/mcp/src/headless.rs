//! The headless backend (`mcp.md` §1): an in-process engine [`Session`] with no window, answering
//! the control-protocol methods the tools use, so agents can organize, edit, render and export
//! without the app.
//!
//! - **Where the session comes from** ([`Source`]): `--demo`, the demo library written as real
//!   JPEG files with sidecars into a temporary folder removed on exit; `--library DIR`, a library
//!   directory (one process at a time); neither, an empty library in a temporary folder removed on
//!   exit (the engine has no in-memory session, so "in memory" is a library nobody keeps).
//!   Folders named on the command line are added as Scan Once and become read roots.
//! - **What the launch allowed** ([`Headless::open_with`], `mcp.md` §1 and §5): the
//!   `--automation-*` roots and permissions as a [`Policy`], the client's roots as they are listed
//!   ([`Backend::set_client_roots`]), and `--read-only`, an engine switch
//!   ([`Session::open_read_only`]): nothing is written, and file commands answer `read_only`.
//! - **`engine.execute {command, params?, wait?}`** runs the command through
//!   [`Session::execute`] after the checks the control channel makes (`control-protocol.md` §5):
//!   an effect outside Dalica is refused without `--automation-allow-external`, paths are checked
//!   against the [`Roots`], Rebuild needs
//!   `"confirm": true`, and a photo command without `ids` gets the Photo Tray's ([`Tray`]). A
//!   command that starts jobs waits for them (unless `wait: false`), reporting their photos done
//!   through the call's [`Progress`] and cancelling them, before their next photo, when it says
//!   so; the reply is then the job's final entry with its result.
//! - **People data** (`mcp.md` §5, `faces.md` §7): every command reply, of `engine.execute` and of
//!   each `engine.group` step, goes through [`Policy::people_reply`]: without
//!   `--automation-allow-people` the names and nicknames of `faces.list`, `faces.add` and
//!   `people.list` read `null` and face rectangles are left out (ids and counts kept), and face
//!   descriptors never leave, whatever was granted. The one other reply that would name a person,
//!   `export.defaults` of a person's section (`folderName`, the person's name), suggests the
//!   dialog's fallback name (`Export <date>`) instead, so the `export` tool's folder carries no name.
//!   Searches (`library.query`, `search.set`, so `query_photos`) run without the permission with
//!   people's names left out of the match ([`Policy::people_params`]): `person:` finds nothing and
//!   is `unavailable`, plain words skip names; the filter by person id still holds.
//! - **Face models**: the session looks for them in [`dalica_engine::faces_scan::default_models_dir`],
//!   as the app and the CLI do, so a model installed there scans faces here too.
//! - **`engine.commands`**, **`jobs.list`**, **`jobs.wait`**, **`jobs.cancel`** as in
//!   `control-protocol.md` §6.2 and §6.13; the `ui.*` methods need the app (connect mode).
//! - Three methods of MCP's own for the helpers of `mcp.md` §4.2: **`engine.group {steps,
//!   label?}`** (several library commands as one undo step, `set_metadata`), **`tray.select {ids,
//!   section?, mode?, hold?}`** and **`tray.state`** (the headless [`Tray`], `select_photos` and
//!   `photo_info`). Connect mode (M10.17) maps them onto the app: a grouped execute, `select.set`
//!   then `tray.hold`, and `library.inspect` → `selection` and `tray`.
//!
//! After LightCraft `crates/mcp/src/headless.rs` (`Headless`, its `Backend` impl and `Drop`) @
//! b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: Dalica's sources and engine,
//! the control channel's checks, waiting on any job instead of an export emulated here, and
//! rendering through the one output pipeline (`dalica_create::output::render`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_create::output::{self, Choice, OutFormat, Purpose, Quality, Spec};
use dalica_edit::Recipe;
use dalica_engine::automation::Policy;
use dalica_engine::{CmdError, CommandSpec, JobEntry, JobId, Session};
use serde_json::{Map, Value, json};

use crate::backend::{Backend, Progress, Rendered, ToolError};
use crate::roots::{Output, Roots};
use crate::tray::{Mode, Tray};

/// How long the wait for a job sleeps between two polls.
const WAIT_STEP: Duration = Duration::from_millis(5);
/// How long a cancelled job is given to finish the photos it is writing.
const CANCEL_WAIT: Duration = Duration::from_secs(60);
/// `jobs.wait`'s default and largest timeout (`control-protocol.md` §6.13).
const JOBS_WAIT_MS: u64 = 55_000;

/// Commands whose app asks first that the engine runs without asking, and the question
/// (`control-protocol.md` §5.5, as the control channel's `CONFIRM`).
const CONFIRM: &[(&str, &str)] = &[(
    "library.rebuild",
    "Rebuild the library? Dalica forgets its index and reads every watched folder again; stars and captions come back from the folders.",
)];

/// The undo labels `engine.group` takes (the engine's undo step holds a fixed text); the first is
/// the default.
const GROUP_LABELS: &[&str] = &["Set Metadata", "Change Photos"];

/// The most photos one `tray.select` takes.
const MAX_TRAY_IDS: usize = 100_000;

/// Where a headless session comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An empty library in a temporary folder, removed on exit.
    Empty,
    /// The demo library (`dalica_engine::demo`) in a temporary folder, removed on exit.
    Demo,
    /// A library directory, opened (or made) where it is.
    Library(PathBuf),
}

/// Why a headless session could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenFailure {
    /// Another Dalica process has the library open.
    Locked(String),
    Failed(String),
}

impl std::fmt::Display for OpenFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenFailure::Locked(m) | OpenFailure::Failed(m) => f.write_str(m),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> OpenFailure {
    OpenFailure::Failed(e.to_string())
}

fn open_failure(e: dalica_engine::OpenError) -> OpenFailure {
    match e {
        dalica_engine::OpenError::Locked(_) => OpenFailure::Locked(format!("{e}; use connect mode (--connect) to drive the app that has it")),
        other => failed(other),
    }
}

/// The headless backend. Dropping it closes the session, which writes what is pending (it does not
/// wait for a scan still running: that one goes on next time, as when the app quits).
pub struct Headless {
    session: Session,
    tray: Tray,
    roots: Roots,
    describe: String,
    /// The temporary folder of a demo or an empty library: declared after `session`, so it is
    /// removed after the session has closed.
    _temp: Option<dalica_fs::TempDir>,
}

impl Headless {
    /// Opens a session from `source` and adds `folders` to it as Scan Once (they become read
    /// roots), with nothing else granted: the default write roots, no effect outside Dalica, no
    /// people data. Off any UI thread: it reads and writes the disk.
    pub fn open(source: Source, folders: &[PathBuf]) -> Result<Headless, OpenFailure> {
        Headless::open_with(source, folders, Policy::default(), false)
    }

    /// [`Headless::open`] with what the command line allowed: `policy` (`--automation-read-root`,
    /// `--automation-write-root`, `--automation-allow-external`, `--automation-allow-people`) and
    /// `read_only` (`--read-only`: the session writes nothing; the demo's photos are written
    /// before it opens, into its temporary folder).
    pub fn open_with(source: Source, folders: &[PathBuf], policy: Policy, read_only: bool) -> Result<Headless, OpenFailure> {
        let clock = Arc::new(SystemClock);
        let mut roots = Roots::new(policy);
        let open = |dir: &Path, clock: Arc<SystemClock>| {
            if read_only { Session::open_read_only(dir, clock, &dalica_engine::session::Options::default()) } else { Session::open(dir, clock) }
        };
        let (mut session, temp, mut describe) = match source {
            Source::Demo => {
                let dir = dalica_engine::demo::fresh_dir().map_err(failed)?;
                let session = dalica_engine::demo::open(dir.path(), clock).map_err(failed)?;
                roots.add_read(dalica_engine::demo::photos_dir(dir.path()));
                (session, Some(dir), format!("headless, the demo library ({} photos, deleted on exit)", dalica_engine::demo::PHOTOS))
            }
            Source::Library(dir) => {
                let describe = format!("headless, the library {}", dir.display());
                (open(&dir, clock).map_err(open_failure)?, None, describe)
            }
            Source::Empty => {
                let dir = dalica_fs::TempDir::new("dalica-mcp").map_err(failed)?;
                let session = open(&dir.path().join("library"), clock).map_err(open_failure)?;
                (session, Some(dir), "headless, an empty library (deleted on exit)".to_string())
            }
        };
        if read_only {
            session.make_read_only();
            describe.push_str(", read-only (nothing is written to disk)");
        }
        session.set_face_models_dir(dalica_engine::faces_scan::default_models_dir());
        let mut h = Headless { session, tray: Tray::default(), roots, describe, _temp: temp };
        h.add_folders(folders)?;
        Ok(h)
    }

    /// A backend over a session opened elsewhere (tests), with the default roots.
    pub fn new(session: Session) -> Headless {
        Headless { session, tray: Tray::default(), roots: Roots::default(), describe: "headless".into(), _temp: None }
    }

    /// Adds `folders` as Scan Once, each a read root.
    fn add_folders(&mut self, folders: &[PathBuf]) -> Result<(), OpenFailure> {
        if folders.is_empty() {
            return Ok(());
        }
        let mut changes = Vec::new();
        for f in folders {
            let abs = std::path::absolute(f).map_err(|e| failed(format!("{}: {e}", f.display())))?;
            self.roots.add_read(abs.clone());
            let real = self.roots.check_read(&abs, "folder", &[]).map_err(|e| failed(e.message))?;
            changes.push(json!({ "path": real, "mode": "once" }));
        }
        self.session.execute("folders.set", &json!({ "changes": changes })).map_err(failed)?;
        self.describe.push_str(&format!(", with {} folder(s) scanned once", folders.len()));
        Ok(())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn tray(&self) -> &Tray {
        &self.tray
    }

    pub fn tray_mut(&mut self) -> &mut Tray {
        &mut self.tray
    }

    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    pub fn roots_mut(&mut self) -> &mut Roots {
        &mut self.roots
    }

    /// The tray's photos that still exist.
    fn tray_photos(&mut self) -> Vec<PhotoId> {
        let lib = self.session.library();
        let exists = |id: PhotoId| lib.photo(id).is_some();
        self.tray.retain(exists);
        self.tray.photos()
    }

    /// `engine.execute {command, params?, wait?}`.
    fn execute(&mut self, params: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        let p = fields(params, &["command", "params", "wait"], "engine.execute")?;
        let Some(id) = p.get("command").and_then(Value::as_str) else { return Err(bad("`command` must name the command")) };
        let wait = match p.get("wait") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(w)) => *w,
            Some(other) => return Err(bad(format!("`wait` must be true or false, not {other}"))),
        };
        let (spec, params) = self.prepare(id, p.get("params"))?;
        let before: HashSet<JobId> = self.session.jobs().iter().map(|e| e.job).collect();
        let result = self.session.execute(id, &params);
        self.session.poll();
        let value = self.people_reply(id, &params, result.map_err(|e| command_error(e, &spec, &params))?);
        if !wait {
            return Ok(value);
        }
        let named = value.get("job").and_then(Value::as_u64).filter(|_| value.get("pending") == Some(&Value::Bool(true)));
        let mut jobs: Vec<JobId> = self.session.jobs().iter().filter(|e| !before.contains(&e.job) && !e.state.is_finished()).map(|e| e.job).collect();
        if let Some(j) = named
            && !jobs.contains(&j)
        {
            jobs.push(j);
        }
        if jobs.is_empty() {
            return Ok(value);
        }
        self.wait_jobs(&jobs, progress)?;
        match named.and_then(|j| self.session.job(j)) {
            Some(entry) => Ok(serde_json::to_value(entry).unwrap_or(Value::Null)),
            None => Ok(value),
        }
    }

    /// The reply of command `id` (run with `params`) as automation may see it: through
    /// [`Policy::people_reply`], and without the people permission `export.defaults` of a person's
    /// section suggests the fallback folder name rather than the person's name.
    fn people_reply(&self, id: &str, params: &Value, reply: Value) -> Value {
        let policy = self.roots.policy();
        let mut reply = policy.people_reply(id, reply);
        let person = params.get("section").and_then(Value::as_str).is_some_and(|s| s.starts_with("person:"));
        if id == "export.defaults"
            && person
            && !policy.allow_people()
            && let Some(o) = reply.as_object_mut()
        {
            let fallback = o.get("fallbackName").cloned().unwrap_or(Value::Null);
            o.insert("folderName".into(), fallback);
        }
        reply
    }

    /// The control channel's checks before command `id` runs with `params` (`control-protocol.md`
    /// §5): the command exists, reaches nothing outside Dalica, keeps to the path roots, has
    /// `"confirm": true` where the app asks first, and without `ids` gets the Photo Tray's. Returns
    /// its spec and the params to run it with.
    fn prepare(&mut self, id: &str, params: Option<&Value>) -> Result<(CommandSpec, Value), ToolError> {
        let mut cmd = match params {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(o)) => o.clone(),
            Some(other) => return Err(bad(format!("`params` must be an object, not {other}"))),
        };
        let Some(spec) = self.session.spec(id).copied() else {
            return Err(ToolError::new("unknown_command", format!("no command `{id}` (command_list lists them)")));
        };
        self.roots.policy().check_command(spec.effect, id, &cmd)?;
        self.roots.policy().people_params(id, &mut cmd);
        self.check_paths(id, &cmd)?;
        if let Some((_, question)) = CONFIRM.iter().find(|(c, _)| *c == id) {
            match cmd.remove("confirm") {
                Some(Value::Bool(true)) => {}
                None | Some(Value::Bool(false)) => return Err(needs_confirm(&spec, question)),
                Some(other) => return Err(bad(format!("`confirm` must be true or false, not {other}"))),
            }
        }
        // A photo command without `ids` acts on the Photo Tray, as the bottom-bar buttons do.
        crate::tray::fill_ids(&mut cmd, spec.params, || Ok::<_, ToolError>(self.tray_photos()))?;
        Ok((spec, Value::Object(cmd)))
    }

    /// `engine.group {steps: [{command, params?}], label?}`: the steps as **one** library undo step
    /// ([`Session::execute_group`]), each step checked first as `engine.execute` checks its command
    /// (a step without `ids` gets the Photo Tray's). A failed step undoes the steps before it and
    /// answers its error, the step named. Replies `{results}`, one per step. In connect mode
    /// (M10.17) this maps onto the app's grouped execute of the same steps.
    fn group(&mut self, params: &Value) -> Result<Value, ToolError> {
        let p = fields(params, &["steps", "label"], "engine.group")?;
        let label = match p.get("label") {
            None | Some(Value::Null) => GROUP_LABELS[0],
            Some(Value::String(s)) => match GROUP_LABELS.iter().find(|l| **l == s.as_str()) {
                Some(l) => l,
                None => return Err(bad(format!("`label` must be one of {}, not {s:?}", GROUP_LABELS.join(", ")))),
            },
            Some(other) => return Err(bad(format!("`label` must be text, not {other}"))),
        };
        let Some(steps) = p.get("steps").and_then(Value::as_array) else { return Err(bad("`steps` must be a list of {command, params?}")) };
        if steps.is_empty() || steps.len() > dalica_engine::session::MAX_GROUP_STEPS {
            return Err(ToolError::new(
                "limit",
                format!("`steps` holds 1 to {} commands, not {}", dalica_engine::session::MAX_GROUP_STEPS, steps.len()),
            ));
        }
        let mut prepared: Vec<(&'static str, Value)> = Vec::with_capacity(steps.len());
        let mut specs = Vec::with_capacity(steps.len());
        for (n, step) in steps.iter().enumerate() {
            let at = |e: ToolError| ToolError { message: format!("step {}: {}", n.saturating_add(1), e.message), ..e };
            let s = fields(step, &["command", "params"], "a step of engine.group").map_err(at)?;
            let Some(id) = s.get("command").and_then(Value::as_str) else { return Err(at(bad("`command` must name the command"))) };
            let (spec, params) = self.prepare(id, s.get("params")).map_err(at)?;
            prepared.push((spec.id, params));
            specs.push(spec);
        }
        let result = self.session.execute_group(&prepared, label);
        self.session.poll();
        let results = result.map_err(|e| group_error(e, &specs, &prepared))?;
        let results: Vec<Value> = prepared.iter().zip(results).map(|((id, params), r)| self.people_reply(id, params, r)).collect();
        Ok(json!({ "results": results }))
    }

    /// `tray.select {ids, section?, mode?, hold?}`: the selection set to, added to or taken from
    /// `ids` in `section` (`mode` `replace`, the default, `add` or `remove`; [`Tray::select`]), then
    /// held when `hold` is true. Every id must be a photo of the library: an unknown one is
    /// `not_found` and leaves the tray as it was. Replies `{section, selection, held, tray}` (as
    /// `tray.state`). In connect mode (M10.17) this maps onto the app's `select.set {ids, section}`
    /// (with the ids an `add` or `remove` leaves, worked out from `library.inspect` → `selection`)
    /// and then `tray.hold`.
    fn tray_select(&mut self, params: &Value) -> Result<Value, ToolError> {
        let p = fields(params, &["ids", "section", "mode", "hold"], "tray.select")?;
        let Some(list) = p.get("ids").and_then(Value::as_array) else { return Err(bad("`ids` must be a list of photo ids")) };
        if list.len() > MAX_TRAY_IDS {
            return Err(ToolError::new("limit", format!("at most {MAX_TRAY_IDS} photos at once")));
        }
        let mut ids = Vec::with_capacity(list.len());
        for v in list {
            let Some(n) = v.as_u64() else { return Err(bad(format!("`ids` must hold photo ids, not {v}"))) };
            let id = PhotoId::new(n);
            if self.session.library().photo(id).is_none() {
                return Err(ToolError::new("not_found", format!("no photo {id}; the selection is unchanged")));
            }
            ids.push(id);
        }
        let section = match p.get("section") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.split_once(':').is_some_and(|(k, v)| matches!(k, "folder" | "album") && !v.is_empty()) => Some(s.as_str()),
            Some(other) => return Err(bad(format!("`section` must be a section id (folder:<id>, album:<id>), not {other}"))),
        };
        let mode = match p.get("mode") {
            None | Some(Value::Null) => Mode::Replace,
            Some(Value::String(s)) => Mode::parse(s).ok_or_else(|| bad(format!("`mode` must be replace, add or remove, not {s:?}")))?,
            Some(other) => return Err(bad(format!("`mode` must be replace, add or remove, not {other}"))),
        };
        let hold = match p.get("hold") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(other) => return Err(bad(format!("`hold` must be true or false, not {other}"))),
        };
        self.tray.select(&ids, section, mode);
        if hold {
            self.tray.hold();
        }
        Ok(self.tray.to_json())
    }

    /// The control channel's path rules (`control-protocol.md` §5.7) for the commands that read a
    /// folder or make new files.
    fn check_paths(&self, id: &str, cmd: &Map<String, Value>) -> Result<(), ToolError> {
        let watched = self.session.roots().to_vec();
        let path = |param: &str| -> Result<Option<PathBuf>, ToolError> {
            match cmd.get(param) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(PathBuf::from(s))),
                Some(other) => Err(bad(format!("`{param}` must be a path, not {other}"))),
            }
        };
        match id {
            "export.run" => path("dir")?.map_or(Ok(()), |p| self.roots.check_output(&p, "dir", &watched, Output::Folder).map(|_| ())),
            "photo.saveTo" => path("path")?.map_or(Ok(()), |p| self.roots.check_output(&p, "path", &watched, Output::File).map(|_| ())),
            "import.run" => path("destination")?.map_or(Ok(()), |p| self.roots.check_output(&p, "destination", &watched, Output::Folder).map(|_| ())),
            "folders.add" => path("path")?.map_or(Ok(()), |p| self.roots.check_read(&p, "path", &watched).map(|_| ())),
            "folders.set" => {
                let changes = cmd.get("changes").and_then(Value::as_array).cloned().unwrap_or_default();
                for c in changes.iter().filter(|c| c.get("mode").and_then(Value::as_str) != Some("remove")) {
                    if let Some(p) = c.get("path").and_then(Value::as_str) {
                        self.roots.check_read(Path::new(p), "changes[].path", &watched)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Waits until every job of `jobs` has ended, reporting their photos done. When `progress`
    /// says the request was cancelled, cancels them (each ends before its next photo; the photos
    /// done stay done) and answers `cancelled`.
    fn wait_jobs(&mut self, jobs: &[JobId], progress: &mut dyn Progress) -> Result<(), ToolError> {
        loop {
            self.session.poll();
            let entries: Vec<JobEntry> = jobs.iter().filter_map(|j| self.session.job(*j)).collect();
            let done = entries.iter().fold(0u64, |n, e| n.saturating_add(e.progress.done));
            let total = entries.iter().fold(0u64, |n, e| n.saturating_add(e.progress.total));
            let running = entries.iter().find(|e| !e.state.is_finished());
            let label = running.or(entries.last()).map_or("", |e| e.label.as_str()).to_string();
            if !progress.report(done, total, &label) {
                self.cancel(jobs);
                return Err(ToolError::new("cancelled", "the request was cancelled"));
            }
            if running.is_none() {
                return Ok(());
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// Cancels `jobs` and waits (at most [`CANCEL_WAIT`]) for the photos they are writing.
    fn cancel(&mut self, jobs: &[JobId]) {
        for j in jobs {
            let _ = self.session.cancel_jobs(Some(*j));
        }
        let start = Instant::now();
        while start.elapsed() < CANCEL_WAIT {
            self.session.poll();
            if jobs.iter().filter_map(|j| self.session.job(*j)).all(|e| e.state.is_finished()) {
                return;
            }
            std::thread::sleep(WAIT_STEP);
        }
        log::warn!("cancelled jobs {jobs:?} were still running after {} s", CANCEL_WAIT.as_secs());
    }

    /// `engine.commands {filter?, enabledOnly?}` (`control-protocol.md` §6.2).
    fn commands(&self, params: &Value) -> Result<Value, ToolError> {
        let p = fields(params, &["filter", "enabledOnly"], "engine.commands")?;
        let filter = match p.get("filter") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.to_lowercase(),
            Some(other) => return Err(bad(format!("`filter` must be text, not {other}"))),
        };
        let enabled_only = match p.get("enabledOnly") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(other) => return Err(bad(format!("`enabledOnly` must be true or false, not {other}"))),
        };
        let list: Vec<Value> = self
            .session
            .commands()
            .into_iter()
            .filter(|c| filter.is_empty() || [c.id, c.label, c.menu].iter().any(|t| t.to_lowercase().contains(&filter)))
            .filter(|c| !enabled_only || c.enabled)
            .filter_map(|c| serde_json::to_value(c).ok())
            .collect();
        Ok(Value::Array(list))
    }

    /// `jobs.wait {job, timeoutMs?}`: the job's final entry, or its running one with
    /// `pending: true` after the timeout.
    fn jobs_wait(&mut self, params: &Value) -> Result<Value, ToolError> {
        let p = fields(params, &["job", "timeoutMs"], "jobs.wait")?;
        let Some(job) = p.get("job").and_then(Value::as_u64) else { return Err(bad("`job` must be a job id")) };
        let ms = match p.get("timeoutMs") {
            None | Some(Value::Null) => JOBS_WAIT_MS,
            Some(v) => v.as_u64().ok_or_else(|| bad(format!("`timeoutMs` must be a whole number, not {v}")))?,
        };
        if ms > JOBS_WAIT_MS {
            return Err(ToolError::new("limit", format!("`timeoutMs` is at most {JOBS_WAIT_MS}, not {ms}")));
        }
        let entry = self.session.wait_job(job, Duration::from_millis(ms))?;
        let mut v = serde_json::to_value(&entry).unwrap_or(Value::Null);
        if !entry.state.is_finished()
            && let Some(o) = v.as_object_mut()
        {
            o.insert("pending".into(), Value::Bool(true));
        }
        Ok(v)
    }
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        self.session.poll();
        match method {
            "engine.execute" => self.execute(&params, progress),
            "engine.commands" => self.commands(&params),
            "jobs.list" => {
                fields(&params, &[], method)?;
                Ok(serde_json::to_value(self.session.jobs()).unwrap_or(Value::Null))
            }
            "jobs.wait" => self.jobs_wait(&params),
            "engine.group" => self.group(&params),
            "tray.select" => self.tray_select(&params),
            "tray.state" => {
                fields(&params, &[], method)?;
                self.tray_photos();
                Ok(self.tray.to_json())
            }
            "jobs.cancel" => {
                let p = fields(&params, &["job"], method)?;
                let job = match p.get("job") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_u64().ok_or_else(|| bad(format!("`job` must be a job id, not {v}")))?),
                };
                Ok(json!({ "cancelled": self.session.cancel_jobs(job)? }))
            }
            m if m.starts_with("ui.") => Err(ToolError::new(
                "unsupported",
                format!("`{m}` needs the desktop app: start it with `dalica --control PORT` and run the MCP server with `--connect`"),
            )),
            other => Err(ToolError::new("unknown_method", format!("no method `{other}` in a headless session"))),
        }
    }

    fn render(&mut self, id: Option<u64>, max_side: u32) -> Result<Rendered, ToolError> {
        self.session.poll();
        let id = match id {
            Some(id) => PhotoId::new(id),
            None => match self.tray_photos().first() {
                Some(id) => *id,
                None => return Err(ToolError::new("disabled", "Select a photo first, or pass `id`.")),
            },
        };
        let lib = self.session.library();
        let Some((photo, folder)) = lib.photo(id).and_then(|p| Some((p, lib.folder(p.folder)?))) else {
            return Err(ToolError::new("not_found", format!("no photo {id}")));
        };
        let path = folder.path.join(&photo.name);
        let recipe =
            Recipe::parse(&lib.info(id).edits).map_err(|e| ToolError::new("unsupported", format!("photo {id}'s edits cannot be read: {e}")))?;
        let bytes = match dalica_fs::read(&path, dalica_create::export::MAX_FILE) {
            Ok(Some(read)) => read.bytes,
            Ok(None) => return Err(ToolError::new("not_found", format!("photo {id}'s file {} is missing", path.display()))),
            Err(e) => return Err(ToolError::new("not_found", format!("photo {id}'s file cannot be read: {e}"))),
        };
        let mut spec = Spec::new(Purpose::Export);
        spec.format = Choice::Exactly(OutFormat::Png);
        spec.long_side = Some(max_side);
        let fail = |e: output::Error| ToolError::new("unsupported", format!("photo {id} cannot be rendered: {}", e.reason()));
        let mut out = output::render(&bytes, &recipe, &spec).map_err(fail)?;
        let mut mime = "image/png";
        if out.bytes.len() > crate::MAX_PNG_BYTES {
            spec.format = Choice::Exactly(OutFormat::Jpeg);
            spec.quality = Quality::Fixed(90);
            out = output::render(&bytes, &recipe, &spec).map_err(fail)?;
            mime = "image/jpeg";
        }
        Ok(Rendered { id: id.get(), mime, width: out.report.width, height: out.report.height, recipe: recipe.print(), bytes: out.bytes })
    }

    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        self.describe.clone()
    }

    fn tick(&mut self) {
        self.session.poll();
    }

    fn set_client_roots(&mut self, roots: Vec<PathBuf>) {
        let kept = self.roots.set_client(roots);
        log::info!("the client's roots: {kept} folder(s), read only");
    }
}

fn bad(message: impl Into<String>) -> ToolError {
    ToolError::new("bad_params", message)
}

/// `params` as an object holding only `allowed` keys (`null` counts as `{}`).
fn fields<'a>(params: &'a Value, allowed: &[&str], method: &str) -> Result<std::borrow::Cow<'a, Map<String, Value>>, ToolError> {
    let map = match params {
        Value::Null => return Ok(std::borrow::Cow::Owned(Map::new())),
        Value::Object(o) => o,
        other => return Err(bad(format!("the params of {method} must be an object, not {other}"))),
    };
    if let Some(k) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(bad(format!("unknown param `{k}` for {method}; expected: {}", allowed.join(", "))));
    }
    Ok(std::borrow::Cow::Borrowed(map))
}

/// The `needs_confirm` error of a command whose app asks `question` first.
fn needs_confirm(spec: &CommandSpec, question: &str) -> ToolError {
    let title = spec.label.trim_end_matches('…');
    ToolError {
        prompt: Some(json!({ "title": title, "text": question, "buttons": [title, "Cancel"] })),
        ..ToolError::new(
            "needs_confirm",
            format!("`{}` asks first in the app; send \"confirm\": true in its params to go ahead (prompt holds the question)", spec.id),
        )
    }
}

/// A group's error as [`command_error`] reports a command's: a step that asks first and was not
/// given `"confirm": true` (`photo.geo.set` moving or removing many locations) is `needs_confirm`
/// with its question.
fn group_error(e: CmdError, specs: &[CommandSpec], steps: &[(&'static str, Value)]) -> ToolError {
    let asking = specs.iter().zip(steps).find(|(spec, (id, params))| {
        matches!(&e, CmdError::Disabled(m) if m.contains(&format!("({id})")))
            && spec.params.contains("\"confirm\"")
            && params.get("confirm") != Some(&Value::Bool(true))
    });
    match asking {
        Some((spec, (_, params))) => command_error(e, spec, params),
        None => e.into(),
    }
}

/// A command's error as the control channel reports it: an engine refusal that asks for
/// `"confirm": true` (Delete, Remove from Album, Move…) is `needs_confirm` with its question.
fn command_error(e: CmdError, spec: &CommandSpec, params: &Value) -> ToolError {
    match &e {
        CmdError::Disabled(m)
            if m.contains("\"confirm\": true") && spec.params.contains("\"confirm\"") && params.get("confirm") != Some(&Value::Bool(true)) =>
        {
            needs_confirm(spec, m)
        }
        _ => e.into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::NoProgress;

    use super::*;

    fn run(h: &mut Headless, command: &str, params: Value) -> Result<Value, ToolError> {
        h.call("engine.execute", json!({ "command": command, "params": params }), &mut NoProgress)
    }

    fn code<T: std::fmt::Debug>(r: Result<T, ToolError>) -> String {
        r.map(|v| format!("ok: {v:?}")).unwrap_or_else(|e| e.code)
    }

    /// The control channel's rules in a headless session: a photo command without `ids` acts on
    /// the tray, Rebuild and Delete ask first (`needs_confirm` with the question), paths stay
    /// inside their roots, `wait: false` answers the job at once and `jobs.wait` its end, and the
    /// window's methods name connect mode.
    #[test]
    fn engine_execute_follows_the_control_channel() {
        let mut h = Headless::open(Source::Demo, &[]).unwrap();
        assert_eq!(code(run(&mut h, "photo.star.toggle", json!({}))), "disabled", "an empty tray: Select some photos first");
        h.tray_mut().select(&[PhotoId::new(2), PhotoId::new(3)], Some("folder:1"), Mode::Replace);
        assert_eq!(run(&mut h, "photo.star.toggle", json!({})).unwrap()["ids"], json!([2, 3]));
        assert!(h.session().library().photo(PhotoId::new(3)).unwrap().star);

        let rebuild = run(&mut h, "library.rebuild", json!({})).unwrap_err();
        assert_eq!(
            (rebuild.code.as_str(), rebuild.prompt.as_ref().map(|p| p["buttons"].clone())),
            ("needs_confirm", Some(json!(["Rebuild Library", "Cancel"])))
        );
        let delete = run(&mut h, "photo.delete", json!({ "ids": [4] })).unwrap_err();
        assert!(delete.code == "needs_confirm" && delete.prompt.is_some(), "{delete:?}");
        assert!(h.session().library().photo(PhotoId::new(4)).is_some());

        let outside = std::env::temp_dir().parent().unwrap().join("dalica-mcp-denied");
        let dir = tempfile::tempdir().unwrap();
        let export = |dir: &Path| json!({ "ids": [1, 2], "dir": dir, "folderName": "Out", "size": 100 });
        assert_eq!(code(run(&mut h, "export.run", export(&outside))), "path_denied");
        // An import's destination keeps to the write roots like an export's folder, and removing
        // imported files from a card is never run, whatever was confirmed.
        assert_eq!(code(run(&mut h, "import.run", json!({ "destination": outside, "folderName": "Out" }))), "path_denied");
        assert_eq!(code(run(&mut h, "import.removeFromSource", json!({ "confirm": true }))), "external_denied");
        assert_eq!(code(run(&mut h, "folders.add", json!({ "path": dir.path() }))), "path_denied", "not a read root");
        let pending =
            h.call("engine.execute", json!({ "command": "export.run", "params": export(dir.path()), "wait": false }), &mut NoProgress).unwrap();
        assert_eq!(pending["pending"], true, "{pending}");
        let end = h.call("jobs.wait", json!({ "job": pending["job"] }), &mut NoProgress).unwrap();
        assert_eq!((end["state"].as_str(), end["result"]["done"].as_u64()), (Some("done"), Some(2)), "{end}");
        assert_eq!(code(h.call("jobs.wait", json!({ "job": 1, "timeoutMs": 60_000 }), &mut NoProgress)), "limit");
        assert_eq!(code(h.call("ui.key", json!({ "combo": "Ctrl+8" }), &mut NoProgress)), "unsupported");
        assert_eq!(code(h.call("engine.execute", json!({ "command": "photo.inspect", "colour": 1 }), &mut NoProgress)), "bad_params");
    }

    /// `tray.select` changes the tray only when every id exists; `tray.state` reads it back;
    /// `engine.group` is one undo step, takes the tray's ids, applies `engine.execute`'s checks to
    /// each step and leaves nothing behind when a step fails.
    #[test]
    fn tray_and_group_methods() {
        let mut h = Headless::open(Source::Demo, &[]).unwrap();
        let call = |h: &mut Headless, m: &str, p: Value| h.call(m, p, &mut NoProgress);
        let t = call(&mut h, "tray.select", json!({ "ids": [2, 3], "section": "folder:1", "hold": true })).unwrap();
        assert_eq!((&t["held"], &t["tray"]), (&json!([2, 3]), &json!([2, 3])));
        call(&mut h, "tray.select", json!({ "ids": [5], "section": "folder:1", "mode": "add" })).unwrap();
        assert_eq!(code(call(&mut h, "tray.select", json!({ "ids": [6, 999_999] }))), "not_found");
        assert_eq!(code(call(&mut h, "tray.select", json!({ "ids": [6], "mode": "toggle" }))), "bad_params");
        assert_eq!(code(call(&mut h, "tray.select", json!({ "ids": [6], "section": "elsewhere" }))), "bad_params");
        assert_eq!(call(&mut h, "tray.state", json!({})).unwrap()["tray"], json!([2, 3, 5]), "unchanged by the failures");

        let depth = h.session().undo_stack().len();
        let steps = json!([
            { "command": "photo.star.set", "params": { "on": true } },
            { "command": "photo.caption.set", "params": { "ids": [2], "caption": "Tram 28" } },
        ]);
        let r = call(&mut h, "engine.group", json!({ "steps": steps })).unwrap();
        assert_eq!(r["results"][0]["ids"], json!([2, 3, 5]), "a step without ids takes the tray's");
        assert_eq!(h.session().undo_stack().len(), depth + 1);
        assert!(h.session().library().photo(PhotoId::new(5)).unwrap().star);

        let failing = json!([
            { "command": "photo.caption.set", "params": { "ids": [4], "caption": "x" } },
            { "command": "photo.star.set", "params": { "ids": [999_999], "on": true } },
        ]);
        assert_eq!(code(call(&mut h, "engine.group", json!({ "steps": failing }))), "not_found");
        assert_eq!(h.session().library().info(PhotoId::new(4)).caption, "", "the first step was undone");
        assert_eq!(h.session().undo_stack().len(), depth + 1);
        let rebuild = json!([{ "command": "library.rebuild" }]);
        assert_eq!(code(call(&mut h, "engine.group", json!({ "steps": rebuild }))), "needs_confirm");
        assert_eq!(code(call(&mut h, "engine.group", json!({ "steps": [] }))), "limit");
        assert_eq!(code(call(&mut h, "engine.group", json!({ "steps": steps, "label": "Anything" }))), "bad_params");
        assert_eq!(code(call(&mut h, "tray.state", json!({ "x": 1 }))), "bad_params");
    }

    /// A headless session looks for face models where the app and the CLI do.
    #[test]
    fn the_session_has_the_default_models_folder() {
        let h = Headless::open(Source::Empty, &[]).unwrap();
        assert_eq!(h.session().face_scan().models_dir(), dalica_engine::faces_scan::default_models_dir().as_deref());
    }

    /// A face drawn as an `engine.group` step answers without its rectangle when the people
    /// permission was not given, and with it when it was; a group step that asks first (removing
    /// the location of more than five photos) is `needs_confirm` with its question.
    #[test]
    fn group_steps_withhold_people_data_and_ask_first() {
        for allow in [false, true] {
            let mut policy = Policy::default();
            policy.set_allow_people(allow);
            let mut h = Headless::open_with(Source::Demo, &[], policy, false).unwrap();
            let steps = json!([{ "command": "faces.add", "params": { "id": 1, "rect": [0.1, 0.1, 0.3, 0.4] } }]);
            let r = h.call("engine.group", json!({ "steps": steps }), &mut NoProgress).unwrap();
            assert_eq!((r["results"][0]["face"].as_str(), r["results"][0].get("rect").is_some()), (Some("1.0"), allow), "{r}");
        }
        let mut h = Headless::open(Source::Demo, &[]).unwrap();
        let ids = json!([1, 2, 3, 4, 5, 6]);
        h.call("engine.group", json!({ "steps": [{ "command": "photo.geo.set", "params": { "ids": ids, "geo": [38.7, -9.1] } }] }), &mut NoProgress)
            .unwrap();
        let clear = json!({ "steps": [{ "command": "photo.geo.set", "params": { "ids": ids, "geo": null } }] });
        let asked = h.call("engine.group", clear, &mut NoProgress).unwrap_err();
        assert!(asked.code == "needs_confirm" && asked.prompt.is_some(), "{asked:?}");
        assert!(h.session().library().is_geotagged(PhotoId::new(6)), "nothing changed");
    }
}
