//! The control-method handlers (`control-protocol.md` §3–§6): every method an agent, a test or a
//! script calls on the running UI, as pure functions from a request to an [`Outcome`] over a
//! [`Host`] (the app and its egui context). There is no socket here: the transport (`--control`,
//! `auth`, framing, connection limits) wires these handlers to TCP, and `--headless` runs them on the
//! headless [`Driver`](crate::Driver), which is a [`Player`].
//!
//! # How a request runs
//!
//! [`Connection::handle`] checks every parameter and limit first; a refused request changes
//! nothing and runs no frame (§4). It answers with an [`Outcome`]:
//!
//! - [`Outcome::Reply`]: the reply, ready to send.
//! - [`Outcome::Play`]: input (or frames, a settle, a capture) to play first; its continuation
//!   runs on the UI thread once the input has been consumed by a frame and one more frame has been
//!   laid out (§5.2), and gives the next outcome.
//! - [`Outcome::Work`]: file-system or encoding work (path checks, PNG encoding, writing a
//!   screenshot) that must not run on the UI thread (§5.1); it runs on a worker and gives a
//!   [`Next`]: the reply, or a step back on the UI thread.
//!
//! [`run`] plays an outcome to its reply on a [`Player`] (the headless driver), running work on a
//! scoped worker thread; [`serve`] is `handle` + `run`. The window plays the same outcomes over its
//! own frames. Panics anywhere in a handler or a continuation are caught and answered with
//! `internal` (§3).
//!
//! `auth` is the transport's: it answers the first line itself and never passes `auth` here.
//!
//! # What automation never does, whatever the route
//!
//! While a request runs, from [`Connection::handle`] until its reply (across the frames its input
//! plays, and again in each step back on the UI thread after work), the UI thread is marked as
//! driven by automation ([`automated`]). Anything the app runs in that time, a dialog button
//! pressed by `ui.dialog.confirm`, a click from `ui.clickWidget` or `ui.pointer`, a key, a UI
//! command run through `engine.execute` or `ui.menu.invoke`, asks [`refuse`] before it runs a
//! command whose danger depends on who asks: a command of `automation::NEVER_AUTOMATED` (removing
//! imported files from a card) is refused, and `import.run`'s destination must lie in a write root
//! or a watched folder, with its links resolved off the UI thread before the request's input plays
//! ([`vet`]; `security.md` §5.7). The person's own clicks, outside any request, are not marked.

mod batch;
mod dialog;
mod engine;
mod host;
mod input;
mod inspect;
mod jobs;
mod menu;
mod paths;
mod people;
mod picker;
mod screenshot;
mod vet;
mod view;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_engine::CmdError;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::headless::DriverError;
pub use host::{DragTo, Host, Input, Played, Player, Window, frames_for};
pub use paths::default_write_roots;
pub use picker::{PickerKind, PickerRequest, StubPicker};
pub use screenshot::CpuMirror;

/// The request line limit, newline included (§4).
pub const MAX_REQUEST_LINE: usize = 1 << 20;
/// How deep arrays and objects may nest in a request line, the request object itself included
/// (`security.md` §6.5).
pub const MAX_REQUEST_DEPTH: usize = 64;
/// The reply line limit, newline included (§4).
pub const MAX_REPLY_LINE: usize = 8 << 20;
/// `ui.clickWidget` `count` (§4).
pub const MAX_CLICK_COUNT: u64 = 8;
/// Drag `steps` (§4).
pub const MAX_DRAG_STEPS: u64 = 1_000;
/// `ui.pointer` events per request (§4).
pub const MAX_POINTER_EVENTS: usize = 10_000;
/// `ui.type` text, in bytes (§4).
pub const MAX_TEXT_BYTES: usize = 64 << 10;
/// `ui.wait` / `ui.settle` (and `ui.hoverWidget` rest) timeouts (§4).
pub const MAX_WAIT_MS: u64 = 60_000;
/// Screenshot edge and area (§4); the rasterizer may refuse less (`limit` as well).
pub const MAX_SHOT_SIDE: u64 = 16_384;
pub const MAX_SHOT_PIXELS: u64 = 64 << 20;
/// Screenshot scale: (0, 4] (§4).
pub const MAX_SCALE: f64 = 4.0;
/// How long a screenshot settles first (§6.11).
pub const SCREENSHOT_SETTLE_MS: u64 = 5_000;
/// `ui.settle` default timeout.
pub const DEFAULT_SETTLE_MS: u64 = 5_000;
/// The farthest a coordinate or a scroll may reach, in points (the driver's bound).
pub const MAX_COORD: f64 = 1.0e6;

/// The stable error codes of §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    Parse,
    Unauthorized,
    UnknownMethod,
    UnknownCommand,
    BadParams,
    Disabled,
    NeedsConfirm,
    NotFound,
    Busy,
    PathDenied,
    ExternalDenied,
    Unsupported,
    Limit,
    Timeout,
    Unsaved,
    ReadOnly,
    Internal,
}

impl Code {
    pub const ALL: [Code; 17] = [
        Code::Parse,
        Code::Unauthorized,
        Code::UnknownMethod,
        Code::UnknownCommand,
        Code::BadParams,
        Code::Disabled,
        Code::NeedsConfirm,
        Code::NotFound,
        Code::Busy,
        Code::PathDenied,
        Code::ExternalDenied,
        Code::Unsupported,
        Code::Limit,
        Code::Timeout,
        Code::Unsaved,
        Code::ReadOnly,
        Code::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Code::Parse => "parse",
            Code::Unauthorized => "unauthorized",
            Code::UnknownMethod => "unknown_method",
            Code::UnknownCommand => "unknown_command",
            Code::BadParams => "bad_params",
            Code::Disabled => "disabled",
            Code::NeedsConfirm => "needs_confirm",
            Code::NotFound => "not_found",
            Code::Busy => "busy",
            Code::PathDenied => "path_denied",
            Code::ExternalDenied => "external_denied",
            Code::Unsupported => "unsupported",
            Code::Limit => "limit",
            Code::Timeout => "timeout",
            Code::Unsaved => "unsaved",
            Code::ReadOnly => "read_only",
            Code::Internal => "internal",
        }
    }

    /// Whether the transport closes the connection after this error (§3).
    pub fn closes(self) -> bool {
        matches!(self, Code::Parse | Code::Unauthorized)
    }
}

/// A failed request: a stable code and an English sentence an agent can act on.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub code: Code,
    pub message: String,
    /// `needs_confirm`: the dialog's text and buttons (§5.5).
    pub prompt: Option<Value>,
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Error {
        Error { code, message: message.into(), prompt: None }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl From<CmdError> for Error {
    fn from(e: CmdError) -> Error {
        let code = match &e {
            CmdError::UnknownCommand(_) => Code::UnknownCommand,
            CmdError::BadParams(_) => Code::BadParams,
            CmdError::Disabled(_) => Code::Disabled,
            CmdError::NotFound(_) => Code::NotFound,
            CmdError::Limit(_) => Code::Limit,
            CmdError::Unsaved(_) => Code::Unsaved,
            CmdError::ReadOnly(_) => Code::ReadOnly,
            CmdError::Internal(_) => Code::Internal,
        };
        let message = match e {
            CmdError::UnknownCommand(id) => format!("no command `{id}` (engine.commands lists them)"),
            CmdError::Unsaved(why) => format!("saved in memory but not written to disk: {why}; Dalica will retry"),
            other => other.to_string(),
        };
        Error::new(code, message)
    }
}

impl From<DriverError> for Error {
    fn from(e: DriverError) -> Error {
        match e {
            DriverError::NoWidget(id) => Error::new(Code::NotFound, format!("no widget `{id}` on screen (see ui.widgets)")),
            DriverError::Disabled(id) => Error::new(Code::Disabled, format!("widget `{id}` is disabled")),
            DriverError::Timeout(_) | DriverError::NotSettled { .. } => Error::new(Code::Timeout, e.to_string()),
            DriverError::BadKey(..) | DriverError::BadInput(_) => Error::new(Code::BadParams, e.to_string()),
            DriverError::Command(c) => c.into(),
            DriverError::Raster(r) => Error::new(Code::Limit, r.to_string()),
        }
    }
}

/// What a request answers: its result, or why it failed.
pub type Reply = Result<Value, Error>;

/// The reply line object (§2.3): `{id, ok, result}` or `{id, ok, code, error, prompt?}`.
pub fn reply_json(id: &Value, reply: &Reply) -> Value {
    match reply {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(e) => {
            let mut v = json!({ "id": id, "ok": false, "code": e.code.as_str(), "error": e.message });
            if let (Some(prompt), Some(obj)) = (&e.prompt, v.as_object_mut()) {
                obj.insert("prompt".into(), prompt.clone());
            }
            v
        }
    }
}

/// The reply line, newline included. A reply over [`MAX_REPLY_LINE`] is replaced by a `limit` error
/// (§4).
pub fn reply_line(id: &Value, reply: &Reply) -> String {
    let mut line = reply_json(id, reply).to_string();
    if line.len() >= MAX_REPLY_LINE {
        let e = Error::new(
            Code::Limit,
            format!("the reply is {} bytes, over the {MAX_REPLY_LINE}-byte limit (screenshots: write to a `path` instead)", line.len()),
        );
        line = reply_json(id, &Err(e)).to_string();
    }
    line.push('\n');
    line
}

/// One request (§2.3).
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Echoed back; `null` when the request had none.
    pub id: Value,
    pub method: String,
    /// As sent; `null` when omitted. Handlers treat `null` as `{}` and refuse other non-objects.
    pub params: Value,
}

impl Request {
    pub fn new(method: impl Into<String>, params: Value) -> Request {
        Request { id: Value::Null, method: method.into(), params }
    }

    /// Reads one line (with or without its newline). `Ok(None)` for a blank line (skipped);
    /// `Err` is a `parse` error after which the transport closes the connection (§2.3).
    pub fn parse(line: &[u8]) -> Result<Option<Request>, Error> {
        let parse = |why: &str| Error::new(Code::Parse, format!("not a request: {why}; send one JSON object per line"));
        if line.len() > MAX_REQUEST_LINE {
            return Err(parse(&format!("the line is over {MAX_REQUEST_LINE} bytes")));
        }
        let text = std::str::from_utf8(line).map_err(|_| parse("the line is not UTF-8"))?;
        let text = text.trim_end_matches(['\n', '\r']);
        if text.trim().is_empty() {
            return Ok(None);
        }
        if nests_deeper_than(text.as_bytes(), MAX_REQUEST_DEPTH) {
            return Err(parse(&format!("arrays and objects nest deeper than {MAX_REQUEST_DEPTH} levels")));
        }
        let value: Value = serde_json::from_str(text).map_err(|_| parse("the line is not JSON"))?;
        Request::from_value(value).map(Some)
    }

    /// A request from its JSON object; `parse` if it is not one.
    pub fn from_value(value: Value) -> Result<Request, Error> {
        let Value::Object(mut obj) = value else {
            return Err(Error::new(Code::Parse, "not a request: a request is a JSON object"));
        };
        let Some(Value::String(method)) = obj.remove("method") else {
            return Err(Error::new(Code::Parse, "not a request: `method` must be a string"));
        };
        Ok(Request { id: obj.remove("id").unwrap_or(Value::Null), method, params: obj.remove("params").unwrap_or(Value::Null) })
    }
}

/// Whether `[` and `{` nest deeper than `limit` outside strings (escapes honoured), in one linear
/// pass with no recursion, stopping at the first level over. It runs before the parser, whose own
/// limit is 128 and fixed; malformed JSON is left to the parser to refuse.
fn nests_deeper_than(json: &[u8], limit: usize) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &b in json {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth = depth.saturating_add(1);
                if depth > limit {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// What the app was started with that decides what automation may do (§1, §5.6, §5.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// `--automation-read-root`: folders automation may add or read from.
    pub read_roots: Vec<PathBuf>,
    /// `--automation-write-root`: folders automation may write new files into; empty means the
    /// defaults ([`default_write_roots`]).
    pub write_roots: Vec<PathBuf>,
    /// `--automation-allow-external`.
    pub allow_external: bool,
    /// `--automation-allow-people`: people's names and face rectangles reach control clients (in
    /// the commands' replies and in the window's: `ui.inspect`, `ui.state`, `ui.widgets`,
    /// `ui.menu.tree`: `control/people.rs`), and their searches match names (§6.4).
    pub allow_people: bool,
    /// `--demo` / `--demo-fresh`: adding folders needs no read root.
    pub demo: bool,
}

impl Config {
    /// The write roots in effect.
    pub fn effective_write_roots(&self) -> Vec<PathBuf> {
        if self.write_roots.is_empty() { default_write_roots() } else { self.write_roots.clone() }
    }
}

/// What all connections share: the configuration and the file-picker stub (§5.6). Build the app
/// with [`Control::picker`] so pickers opened by synthetic input wait for `ui.filePicker`.
pub struct Control {
    config: Arc<Config>,
    picker: Arc<StubPicker>,
}

impl Control {
    pub fn new(config: Config) -> Control {
        Control { config: Arc::new(config), picker: Arc::new(StubPicker::new()) }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The picker to give [`App::with_picker`](crate::App::with_picker).
    pub fn picker(&self) -> Arc<dyn crate::Picker> {
        self.picker.clone()
    }

    /// The stub itself (what is waiting, answers).
    pub fn stub(&self) -> &StubPicker {
        &self.picker
    }
}

/// Continues on the UI thread after input has played.
pub type Then = Box<dyn FnOnce(&mut dyn Host, Result<Played, DriverError>) -> Outcome>;
/// Work for a worker thread.
pub type Work = Box<dyn FnOnce() -> Next + Send>;
/// Continues on the UI thread after work.
pub type Resume = Box<dyn FnOnce(&mut dyn Host) -> Outcome + Send>;

/// What handling a request (or one of its steps) gives.
pub enum Outcome {
    Reply(Reply),
    /// Play the input, then continue (on the UI thread).
    Play(Input, Then),
    /// Run off the UI thread.
    Work(Work),
}

/// What work gives.
pub enum Next {
    Reply(Reply),
    /// Continue on the UI thread.
    Resume(Resume),
}

impl Outcome {
    /// Plays `input`, then replies `result` (input methods whose reply is known up front).
    pub fn play_then_reply(input: Input, result: Value) -> Outcome {
        Outcome::Play(input, Box::new(move |_, played| Outcome::Reply(played.map(|_| result).map_err(Error::from))))
    }
}

impl From<Reply> for Outcome {
    fn from(r: Reply) -> Outcome {
        Outcome::Reply(r)
    }
}

impl From<Error> for Outcome {
    fn from(e: Error) -> Outcome {
        Outcome::Reply(Err(e))
    }
}

/// Runs `f`, turning a panic into an `internal` reply (the dispatch guard, §3).
pub fn guard(f: impl FnOnce() -> Outcome) -> Outcome {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(o) => o,
        Err(panic) => Outcome::Reply(Err(internal(&*panic))),
    }
}

fn internal(panic: &(dyn std::any::Any + Send)) -> Error {
    let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
    log::error!("a control handler panicked: {message}");
    Error::new(Code::Internal, format!("internal error (logged; the app keeps running): {message}"))
}

/// Plays `outcome` to its reply on `player`. Work runs on a scoped worker thread, so a thread marked
/// as the UI thread never touches the disk.
pub fn run(player: &mut dyn Player, mut outcome: Outcome) -> Reply {
    loop {
        outcome = match outcome {
            Outcome::Reply(r) => return r,
            Outcome::Play(input, then) => {
                let played = player.play(&input);
                let host: &mut dyn Host = player.as_host();
                guard(move || then(host, played))
            }
            Outcome::Work(work) => {
                let next = std::thread::scope(|s| match std::thread::Builder::new().name("dalica-control".into()).spawn_scoped(s, work) {
                    Ok(handle) => handle.join().unwrap_or_else(|panic| Next::Reply(Err(internal(&*panic)))),
                    Err(e) => Next::Reply(Err(Error::new(Code::Internal, format!("cannot start a worker: {e}")))),
                });
                match next {
                    Next::Reply(r) => return r,
                    Next::Resume(k) => {
                        let host: &mut dyn Host = player.as_host();
                        guard(move || k(host))
                    }
                }
            }
        }
    }
}

/// [`Connection::handle`] and [`run`]: one request to its reply, on the headless driver.
pub fn serve(control: &Control, conn: &mut Connection, player: &mut dyn Player, req: &Request) -> Reply {
    let outcome = conn.handle(control, player.as_host(), req);
    run(player, outcome)
}

/// One connection's state: the keys it holds down (released when it closes, §6.9).
#[derive(Debug, Default)]
pub struct Connection {
    held: Vec<(egui::Key, egui::Modifiers)>,
}

impl Connection {
    pub fn new() -> Connection {
        Connection::default()
    }

    /// Handles one request (never `auth`, which the transport answers). The UI thread is marked
    /// as driven by automation until the request's reply ([`automated`]); as it and each of its
    /// steps begin, the export jobs started from a person's section are marked ([`jobs::observe`]).
    pub fn handle(&mut self, control: &Control, host: &mut dyn Host, req: &Request) -> Outcome {
        let running = Running::new(&control.config);
        vet::forget();
        jobs::observe(host);
        let outcome = self.dispatch(control, host, req);
        let outcome = vet::before_input(host, &control.config, outcome);
        hold(outcome, &control.config, running)
    }

    fn dispatch(&mut self, control: &Control, host: &mut dyn Host, req: &Request) -> Outcome {
        guard(|| match METHODS.iter().find(|m| m.name == req.method) {
            None => Error::new(Code::UnknownMethod, format!("no method `{}` (`methods` lists them)", req.method)).into(),
            Some(m) => match m.run {
                None => Error::new(Code::Unsupported, format!("`{}` is not built yet ({})", m.name, m.planned)).into(),
                Some(run) => {
                    let mut call = Call { host, control, conn: self, method: m };
                    run(&mut call, &req.params).unwrap_or_else(Outcome::from)
                }
            },
        })
    }

    /// The key releases to play when the connection closes: every key it left down.
    pub fn release(&mut self) -> Vec<Input> {
        std::mem::take(&mut self.held)
            .into_iter()
            .map(|(key, _)| Input::Key { key, modifiers: egui::Modifiers::NONE, state: crate::headless::KeyState::Up })
            .collect()
    }

    pub(crate) fn hold(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        self.held.retain(|(k, _)| *k != key);
        self.held.push((key, modifiers));
    }

    pub(crate) fn let_go(&mut self, key: egui::Key) {
        self.held.retain(|(k, _)| *k != key);
    }

    /// The keys held down now.
    pub fn held(&self) -> Vec<egui::Key> {
        self.held.iter().map(|(k, _)| *k).collect()
    }
}

thread_local! {
    /// The configurations of the control requests running on this thread (the UI thread), newest
    /// last.
    static RUNNING: RefCell<Vec<Arc<Config>>> = const { RefCell::new(Vec::new()) };
}

/// Marks a control request as running on this thread while it lives: until its reply, or until
/// the outcome holding it is dropped. Never sent to another thread (it is not `Send`), so it ends
/// on the thread it started on.
struct Running(PhantomData<*const ()>);

impl Running {
    fn new(config: &Arc<Config>) -> Running {
        let _ = RUNNING.try_with(|r| r.try_borrow_mut().map(|mut r| r.push(Arc::clone(config))));
        Running(PhantomData)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = RUNNING.try_with(|r| r.try_borrow_mut().map(|mut r| r.pop()));
    }
}

/// `outcome` with the request still marked as running until its reply: through each
/// continuation after input, and again (marked anew on the UI thread) in each step back after work.
fn hold(outcome: Outcome, config: &Arc<Config>, running: Running) -> Outcome {
    match outcome {
        Outcome::Reply(r) => Outcome::Reply(r),
        Outcome::Play(input, then) => {
            let config = Arc::clone(config);
            Outcome::Play(
                input,
                Box::new(move |host, played| {
                    jobs::observe(host);
                    let next = then(host, played);
                    hold(next, &config, running)
                }),
            )
        }
        Outcome::Work(work) => {
            drop(running);
            let config = Arc::clone(config);
            Outcome::Work(Box::new(move || match work() {
                Next::Resume(k) => Next::Resume(Box::new(move |host| {
                    let running = Running::new(&config);
                    jobs::observe(host);
                    let next = k(host);
                    hold(next, &config, running)
                })),
                reply => reply,
            }))
        }
    }
}

/// The configuration of the control request driving this thread now, if one is.
pub fn automated() -> Option<Arc<Config>> {
    RUNNING.try_with(|r| r.try_borrow().ok().and_then(|r| r.last().cloned())).ok().flatten()
}

/// What the app asks before it runs command `id` with `params` on behalf of whoever is driving it:
/// nothing while no control request runs on this thread (the person's own clicks); otherwise a
/// command automation never runs (`automation::never_automated`) is refused, and `import.run`'s
/// `destination` must be a folder automation may write into ([`refuse_folder`]).
pub fn refuse(id: &str, params: &Value, watched: &[PathBuf]) -> Result<(), CmdError> {
    if automated().is_none() {
        return Ok(());
    }
    dalica_engine::automation::never_automated(id).map_err(|r| CmdError::Disabled(r.message))?;
    if id == "import.run" {
        let Some(dest) = params.get("destination").and_then(Value::as_str).map(PathBuf::from) else {
            return Err(CmdError::BadParams("`destination` must be a path".into()));
        };
        refuse_folder(&dest, "destination", watched)?;
    }
    Ok(())
}

/// What the app asks before a command of its own writes new files into folder `dest` (the
/// parameter `what`; the Import room's destination, the Collage room's folder) on behalf of whoever
/// is driving it: nothing while no control request runs on this thread; otherwise `dest` must lie
/// inside a write root or a watched folder (`watched`): compared as written first, then with its
/// links resolved, from the check made off the UI thread before the request's input played or its
/// command ran ([`vet`]; no disk access here, this runs on the UI thread). A folder that check did
/// not see is refused.
pub fn refuse_folder(dest: &Path, what: &str, watched: &[PathBuf]) -> Result<(), CmdError> {
    let Some(config) = automated() else { return Ok(()) };
    dalica_engine::automation::lexical(dest, what).map_err(|r| CmdError::BadParams(r.message))?;
    if !inside_any(dest, &config.effective_write_roots()) && !inside_any(dest, watched) {
        return Err(CmdError::Disabled(format!(
            "{} is outside every automation write root (--automation-write-root; default: the temporary folder) and every watched folder",
            dest.display()
        )));
    }
    match vet::verdict(dest, watched) {
        Some(Ok(())) => Ok(()),
        Some(Err(why)) => Err(CmdError::Disabled(why)),
        None => Err(CmdError::Disabled(format!(
            "{} was not checked with its links resolved before this request ran (the folder changed while it played); send it again",
            dest.display()
        ))),
    }
}

/// Whether `path` is one of `roots` or inside one, as written.
fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// One handler call.
pub(crate) struct Call<'a> {
    pub host: &'a mut dyn Host,
    pub control: &'a Control,
    pub conn: &'a mut Connection,
    pub method: &'static Method,
}

impl Call<'_> {
    /// `params` as `T`: `null` is `{}`, anything but an object is `bad_params`, and serde's message
    /// (the field) is followed by the method's accepted keys.
    pub fn params<T: DeserializeOwned>(&self, params: &Value) -> Result<T, Error> {
        let obj = object(self.method, params)?;
        serde_json::from_value(Value::Object(obj)).map_err(|e| self.bad(e))
    }

    pub fn bad(&self, why: impl std::fmt::Display) -> Error {
        bad_params(self.method, why)
    }
}

fn bad_params(m: &Method, why: impl std::fmt::Display) -> Error {
    let takes = if m.params == "—" { "no parameters".to_string() } else { m.params.to_string() };
    Error::new(Code::BadParams, format!("{}: {why}; it takes {takes}", m.name))
}

fn object(m: &Method, params: &Value) -> Result<Map<String, Value>, Error> {
    match params {
        Value::Null => Ok(Map::new()),
        Value::Object(o) => Ok(o.clone()),
        other => Err(bad_params(m, format!("params must be an object, not {other}"))),
    }
}

type Handler = fn(&mut Call<'_>, &Value) -> Result<Outcome, Error>;

/// One method of §6.
pub struct Method {
    pub name: &'static str,
    /// The accepted parameters, as `methods` lists them.
    pub params: &'static str,
    pub result: &'static str,
    run: Option<Handler>,
    /// For a method not built yet: where it is planned.
    planned: &'static str,
}

const fn method(name: &'static str, params: &'static str, result: &'static str, run: Handler) -> Method {
    Method { name, params, result, run: Some(run), planned: "" }
}

const fn later(name: &'static str, params: &'static str, result: &'static str, planned: &'static str) -> Method {
    Method { name, params, result, run: None, planned }
}

/// Every method of §6 but `auth` (the transport's), in the spec's order.
pub static METHODS: &[Method] = &[
    method("methods", "—", "every method with its params doc", engine::methods),
    method("engine.execute", "{command, params?, wait?}", "the command's result", engine::execute),
    method("engine.commands", "{filter?, enabledOnly?}", "the command registry (§6.2)", engine::commands),
    batch::METHOD,
    inspect::METHOD,
    method("ui.state", "—", "the view state (§6.5)", view::state),
    method("ui.set", "partial view state", "the new view state", view::set),
    method("ui.inspect", "{resetPerf?}", "the view state plus window, perf and more (§6.6)", view::inspect),
    method("ui.resize", "{width, height}", "{width, height} in points", input::resize),
    method("ui.widgets", "{filter?, region?}", "[{id, rect, enabled, checked?, label?, role}] (§6.7)", view::widgets),
    method("ui.clickWidget", "{id, count?, button?, fx?, fy?, ctrl?, shift?, alt?, cmd?}", "{rect}", view::click_widget),
    method(
        "ui.dragWidget",
        "{id, toWidget? | toX, toY | dx, dy, steps?, fx?, fy?, button?, ctrl?, shift?, alt?, cmd?}",
        "{from, to}",
        input::drag_widget,
    ),
    method("ui.hoverWidget", "{id, fx?, fy?, ms?}", "{rect}", input::hover_widget),
    method("ui.pointer", "{events: [{kind: move|down|drag|up, x, y}], space?, button?, ctrl?, shift?, alt?, cmd?}", "{events}", input::pointer),
    method("ui.key", "{key | combo, state?: press|down|up, ctrl?, shift?, alt?, cmd?, macCtrl?}", "{}", input::key),
    method("ui.type", "{text}", "{}", input::type_text),
    method("ui.scroll", "{dx?, dy?, widget?, ctrl?, shift?, alt?}", "{}", input::scroll),
    method("ui.wait", "{ms}", "{frames}", input::wait),
    method("ui.menu.tree", "{context?}", "the menu bar (or a context menu) as shown (§6.10)", menu::tree),
    method("ui.menu.invoke", "{path | id, params?, wait?}", "the command's result, or {dialog}", menu::invoke),
    method("ui.dialog.confirm", "{button?}", "what the dialog's button ran", dialog::confirm),
    method("ui.dialog.cancel", "—", "{}", dialog::cancel),
    method("ui.filePicker", "{path} | {cancel: true}", "the command's result (§5.6)", picker::answer),
    method(
        "ui.screenshot",
        "{path?, headless?, widget?, region?, scale?, settle?}",
        "{path, width, height, headless} (§6.11)",
        screenshot::screenshot,
    ),
    method("ui.settle", "{timeoutMs?, jobs?}", "{settled, frames, ms, busy} (§6.12)", input::settle),
    method("jobs.list", "—", "[{job, kind, label, state, progress, cancellable, startedMs, result?, error?}] (§6.13)", jobs::jobs_list),
    method("jobs.wait", "{job, timeoutMs?}", "the job's final entry, or its entry with pending: true", jobs::jobs_wait),
    method("jobs.cancel", "{job?}", "{cancelled: [job…]}", jobs::jobs_cancel),
    later("app.quit", "—", "{}", "no milestone plans it yet"),
];
