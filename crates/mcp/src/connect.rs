//! Connect mode (`mcp.md` §1, §4.3): the MCP server, or `dalica-cli run --connect`, drives a
//! running desktop app through its loopback control channel (`control-protocol.md`), so the tools
//! act on what the user sees.
//!
//! - **[`Client`]**: one authenticated connection to `dalica --control PORT`. It is opened on the
//!   first call (the server may start before the app) and its first line is always `auth` with the
//!   token read from the token file (the app's default file for the port unless one is named; read
//!   again on every connect, since a restarted app has a new token). The token never appears in a
//!   log, a message or an error. A request that fails in transit (the connection drops, a reply
//!   times out, a reply cannot be read) drops the connection and answers `timeout`: the operation
//!   **may have completed**, and nothing is ever sent again by itself; the next call reconnects.
//!   A connection that cannot be made, or whose `auth` fails, ran nothing (`unreachable`, or the
//!   app's `unauthorized`). Reply lines whose `id` is not the request's are skipped.
//! - **[`Connect`]**: the [`Backend`] over a client. Methods go to the app as they are, except:
//!   `engine.execute` of a command that starts a job is sent with `wait: false`, then the job is
//!   followed with `jobs.wait` so its progress reaches the tool's [`Progress`], and a cancelled
//!   request becomes `jobs.cancel` of that job (`mcp.md` §6); a photo command without `ids` (an
//!   `engine.execute`, or a step of `engine.group`) gets the app's Photo Tray first, by the rule
//!   headless mode applies ([`crate::tray::fill_ids`]; the tray from `library.inspect`, M10.54);
//!   the headless backend's own methods are
//!   mapped onto the app: `tray.state` from `library.inspect` (`selection`, `tray`), `tray.select`
//!   as `select.set` (with the ids an `add` or `remove` leaves) then `tray.hold`, and `engine.group`
//!   as one `engine.execute` for one step, or `batch` (stopping at the first failure) for several:
//!   the app has no grouped execute, so several steps are several undo steps there.
//!   `render` reads the photo and its recipe from the app (`photo.inspect`) and renders the file
//!   through the one output pipeline, as the headless backend does.
//! - **[`Options`]**: `--connect [HOST:PORT]` (default [`crate::DEFAULT_ADDR`]; loopback addresses
//!   only) and `--control-token-file PATH`, shared by `dalica-cli run` and `dalica-cli mcp`.
//!
//! After LightCraft `crates/mcp/src/backend.rs` (`Remote`: `lazy`, `reconnect`, `roundtrip`, its
//! `Backend` impl) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`). Changed for Dalica: authentication
//! first, never a resend (LightCraft retries once), mismatched reply ids skipped, a reply deadline
//! per request, reply lines capped, the control protocol's error codes kept, and the job, tray and
//! group mappings above.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_create::output::{self, Choice, OutFormat, Purpose, Quality, Spec};
use dalica_edit::Recipe;
use serde_json::{Map, Value, json};

use crate::backend::{Backend, Progress, Rendered, ToolError};
use crate::tray;

/// Kept for `dalica-cli mcp` until its `--connect` option is wired to [`Options`] and [`Connect`]
/// (M10.17 built them; the CLI's `mcp` command file belongs to M10.16 while it runs).
pub const CONNECT_NOT_BUILT: &str = "`dalica-cli mcp --connect` is not wired yet (M10.17 built connect mode in dalica-mcp; the option follows once M10.16 lands). Meanwhile `dalica-cli run --connect HOST:PORT` drives the app; or run `dalica-cli mcp --demo` or `--library DIR` for a headless session";

/// How long a request may wait for its reply: the app's 60 s reply deadline plus headroom
/// (`control-protocol.md` §4).
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(70);
/// How long opening the connection may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest reply line read: the app's 8 MiB reply limit plus room for the envelope.
const MAX_REPLY: usize = (8 << 20) + (64 << 10);
/// The longest token file read; its first line is the token.
const MAX_TOKEN_FILE: u64 = 4096;
/// Hex characters in a token (`control-protocol.md` §2.1).
const TOKEN_HEX: usize = 64;
/// How long one `jobs.wait` of a followed job waits before progress is reported again.
const POLL_MS: u64 = 250;
/// The most photos one `tray.select` takes (as the headless backend).
const MAX_TRAY_IDS: usize = 100_000;

// ---------------------------------------------------------------------------------------------
// Options

/// `--connect [HOST:PORT]` and `--control-token-file PATH`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub addr: SocketAddr,
    /// `None`: the app's default token file for the port.
    pub token_file: Option<PathBuf>,
}

impl Options {
    /// Takes `--connect [HOST:PORT]` and `--control-token-file PATH` out of `args`. `Ok(None)` when
    /// `--connect` is not given (a token file without it is an error). The value of `--connect` is
    /// the next argument when it does not start with `-` and holds a `:`.
    pub fn take(args: &mut Vec<String>) -> Result<Option<Options>, String> {
        let mut addr = None;
        let mut token_file = None;
        let mut rest = Vec::with_capacity(args.len());
        let mut it = std::mem::take(args).into_iter().peekable();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--connect" => {
                    let value = it.next_if(|v| !v.starts_with('-') && v.contains(':'));
                    addr = Some(parse_addr(value.as_deref().unwrap_or(crate::DEFAULT_ADDR))?);
                }
                "--control-token-file" => match it.next() {
                    Some(p) => token_file = Some(PathBuf::from(p)),
                    None => return Err("--control-token-file needs a file".into()),
                },
                _ => rest.push(a),
            }
        }
        *args = rest;
        match (addr, token_file) {
            (Some(addr), token_file) => Ok(Some(Options { addr, token_file })),
            (None, Some(_)) => Err("--control-token-file is for connect mode: give --connect too".into()),
            (None, None) => Ok(None),
        }
    }

    /// The token file to read: the one named, else the app's default for the port.
    pub fn token_path(&self) -> Result<PathBuf, String> {
        match &self.token_file {
            Some(p) => Ok(p.clone()),
            None => default_token_file(self.addr.port(), |k| std::env::var_os(k))
                .ok_or_else(|| "no home folder to find the control token file in; give --control-token-file".to_string()),
        }
    }
}

/// `HOST:PORT` with a loopback host (`127.0.0.1`, `[::1]`, `localhost`); anything else is refused,
/// as the app listens on loopback only (`control-protocol.md` §1).
pub fn parse_addr(text: &str) -> Result<SocketAddr, String> {
    let Some((host, port)) = text.rsplit_once(':') else { return Err(format!("--connect takes HOST:PORT, not {text:?}")) };
    let port: u16 = port.parse().ok().filter(|p| *p != 0).ok_or_else(|| format!("--connect: {port:?} is not a port"))?;
    let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let ip = match host.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) if host.eq_ignore_ascii_case("localhost") => IpAddr::from([127, 0, 0, 1]),
        Err(_) => return Err(format!("--connect: {host:?} is not a loopback address (127.0.0.1, [::1] or localhost)")),
    };
    if !ip.is_loopback() {
        return Err(format!("--connect: {ip} is not a loopback address; the app's control channel listens on loopback only"));
    }
    Ok(SocketAddr::new(ip, port))
}

/// The app's default token file for `port`: `<run>/control-<port>.token`, `<run>` being
/// `$XDG_RUNTIME_DIR/dalica` on Linux when set, else the cache folder's `run` (Linux
/// `$XDG_CACHE_HOME/dalica/run`, macOS `~/Library/Caches/Dalica/run`, Windows
/// `%LOCALAPPDATA%\Dalica\cache\run`), as the app makes it (`control-protocol.md` §2.1). `None`
/// without a home.
pub fn default_token_file(port: u16, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    let run = if cfg!(windows) {
        abs("LOCALAPPDATA").map(|p| p.join("Dalica").join("cache").join("run"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Caches").join("Dalica").join("run"))
    } else if let Some(run) = abs("XDG_RUNTIME_DIR") {
        Some(run.join("dalica"))
    } else {
        abs("XDG_CACHE_HOME").or_else(|| abs("HOME").map(|h| h.join(".cache"))).map(|c| c.join("dalica").join("run"))
    };
    run.map(|d| d.join(format!("control-{port}.token")))
}

/// The token on the first line of the file at `path`. Errors name the file, never its content.
fn read_token(path: &Path) -> Result<String, ToolError> {
    let shown = path.display();
    let refuse = |why: String| ToolError::new("unreachable", why);
    let bytes = match dalica_fs::read(path, MAX_TOKEN_FILE) {
        Ok(Some(read)) => read.bytes,
        Ok(None) => {
            return Err(refuse(format!(
                "no control token file at {shown}: is Dalica running with --control on this port? (or give --control-token-file); nothing ran"
            )));
        }
        Err(e) => return Err(refuse(format!("cannot read the control token file {shown}: {e}; nothing ran"))),
    };
    let line = bytes.split(|&b| b == b'\n').next().unwrap_or_default();
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.len() == TOKEN_HEX && line.iter().all(u8::is_ascii_hexdigit) {
        Ok(String::from_utf8_lossy(line).into_owned())
    } else {
        Err(refuse(format!("the control token file {shown} does not start with a {TOKEN_HEX}-character hex token; nothing ran")))
    }
}

// ---------------------------------------------------------------------------------------------
// Client

/// An open, authenticated connection.
struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// One connection to a running app's control channel, opened and authenticated on first use.
pub struct Client {
    addr: SocketAddr,
    token_file: PathBuf,
    conn: Option<Conn>,
    next_id: u64,
    timeout: Duration,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("addr", &self.addr).field("connected", &self.conn.is_some()).finish_non_exhaustive()
    }
}

/// How a request failed on its way.
enum Transit {
    /// Nothing reached the app (no connection, or `auth` failed): safe to call again.
    NotSent(ToolError),
    /// The request may have reached the app.
    Lost(String),
}

impl Client {
    /// A client for the app at `addr`, reading the token from `token_file` when it connects.
    /// Nothing is opened until the first call.
    pub fn new(addr: SocketAddr, token_file: PathBuf) -> Client {
        Client { addr, token_file, conn: None, next_id: 1, timeout: REPLY_TIMEOUT }
    }

    /// From the command line's [`Options`].
    pub fn from_options(o: &Options) -> Result<Client, String> {
        Ok(Client::new(o.addr, o.token_path()?))
    }

    /// How long a request waits for its reply ([`REPLY_TIMEOUT`] by default).
    pub fn with_timeout(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Whether a connection is open now.
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    /// Calls `method` with `params` and returns its `result`, or the app's error (`code`,
    /// `error`, `prompt`). A failure in transit drops the connection and answers `timeout`: the
    /// operation may have completed; it is never resent.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let line = json!({ "id": id, "method": method, "params": params }).to_string();
        match self.roundtrip(&line, &json!(id)) {
            Ok(reply) => reply_result(&reply),
            Err(Transit::NotSent(e)) => Err(e),
            Err(Transit::Lost(why)) => {
                self.conn = None;
                log::warn!("the request {method} to the app at {} failed in transit: {why}", self.addr);
                Err(ToolError::new(
                    "timeout",
                    format!(
                        "the connection to Dalica at {} failed while `{method}` was on its way ({why}): the operation may have completed. \
                         Inspect before retrying; nothing was resent, and the next call reconnects",
                        self.addr
                    ),
                ))
            }
        }
    }

    /// Opens and authenticates the connection.
    fn connect(&mut self) -> Result<(), ToolError> {
        self.conn = None;
        let (addr, port) = (self.addr, self.addr.port());
        let unreachable = |e: std::io::Error| {
            ToolError::new(
                "unreachable",
                format!("Dalica is not reachable at {addr} ({e}); nothing ran. Start it with `dalica --control {port}`, then call again"),
            )
        };
        let token = read_token(&self.token_file)?;
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
        let _ = stream.set_nodelay(true);
        stream.set_write_timeout(Some(self.timeout)).map_err(unreachable)?;
        let reader = BufReader::new(stream.try_clone().map_err(unreachable)?);
        self.conn = Some(Conn { reader, writer: stream });
        let auth = json!({ "id": "auth", "method": "auth", "params": { "token": token } }).to_string();
        let reply = match self.exchange(&auth, &json!("auth")) {
            Ok(r) => r,
            Err(why) => {
                self.conn = None;
                return Err(ToolError::new("unreachable", format!("Dalica at {addr} did not answer the authentication ({why}); nothing ran")));
            }
        };
        if let Err(e) = reply_result(&reply) {
            self.conn = None;
            return Err(e);
        }
        log::info!("connected to Dalica at {addr} (token from {})", self.token_file.display());
        Ok(())
    }

    fn roundtrip(&mut self, line: &str, id: &Value) -> Result<Value, Transit> {
        if self.conn.is_none() {
            self.connect().map_err(Transit::NotSent)?;
        }
        self.exchange(line, id).map_err(Transit::Lost)
    }

    /// Writes `line` and reads lines until the reply whose `id` is `id`, within the timeout.
    fn exchange(&mut self, line: &str, id: &Value) -> Result<Value, String> {
        let timeout = self.timeout;
        let deadline = Instant::now().checked_add(timeout);
        let Some(conn) = self.conn.as_mut() else { return Err("not connected".into()) };
        let mut framed = String::with_capacity(line.len().saturating_add(1));
        framed.push_str(line);
        framed.push('\n');
        conn.writer.write_all(framed.as_bytes()).and_then(|()| conn.writer.flush()).map_err(|e| format!("the request was not written: {e}"))?;
        let no_reply = || format!("no reply within {} ms", timeout.as_millis());
        loop {
            let left = deadline.map_or(timeout, |d| d.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return Err(no_reply());
            }
            conn.reader.get_ref().set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
            let bytes = match read_line(&mut conn.reader, MAX_REPLY) {
                Ok(Some(b)) => b,
                Ok(None) => return Err("the app closed the connection".into()),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Err(no_reply()),
                Err(e) => return Err(e.to_string()),
            };
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let reply: Value = serde_json::from_str(text).map_err(|e| format!("a reply that is not JSON: {e}"))?;
            if reply.get("id") == Some(id) {
                return Ok(reply);
            }
            log::debug!("skipping a reply line for id {} while waiting for {id}", reply.get("id").unwrap_or(&Value::Null));
        }
    }
}

/// One line without its newline, at most `limit` bytes (longer is an error); `None` at the end.
fn read_line(r: &mut impl BufRead, limit: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok((!out.is_empty()).then_some(out));
        }
        let (take, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (buf.len(), false),
        };
        if out.len().saturating_add(take) > limit {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("a reply line longer than {limit} bytes")));
        }
        out.extend_from_slice(buf.get(..take).unwrap_or_default());
        r.consume(if done { take.saturating_add(1) } else { take });
        if done {
            return Ok(Some(out));
        }
    }
}

/// A reply's `result`, or its error as a [`ToolError`].
fn reply_result(reply: &Value) -> Result<Value, ToolError> {
    if reply.get("ok") == Some(&Value::Bool(true)) {
        return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
    }
    let code = reply.get("code").and_then(Value::as_str).unwrap_or("internal");
    let message = reply.get("error").and_then(Value::as_str).unwrap_or("the app answered an error without a message");
    Err(ToolError { prompt: reply.get("prompt").cloned(), ..ToolError::new(code, message) })
}

// ---------------------------------------------------------------------------------------------
// Connect: the backend

/// The [`Backend`] of connect mode: a running app through a [`Client`].
#[derive(Debug)]
pub struct Connect {
    client: Client,
}

impl Connect {
    pub fn new(client: Client) -> Connect {
        Connect { client }
    }

    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    /// `engine.execute {command, params?, wait?}`: a job the command starts is followed with
    /// `jobs.wait`, reporting its progress, and cancelled with `jobs.cancel` when the request is.
    fn execute(&mut self, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        let wait = params.get("wait").cloned().unwrap_or(Value::Null);
        let Value::Object(mut p) = params else { return self.client.call("engine.execute", params) };
        let mut photos = None;
        self.fill_from_tray(&mut p, &mut photos)?;
        if !(wait.is_null() || wait == Value::Bool(true)) {
            // `wait: false` answers at once; a bad `wait` is the app's to refuse.
            return self.client.call("engine.execute", Value::Object(p));
        }
        p.insert("wait".into(), Value::Bool(false));
        let first = self.client.call("engine.execute", Value::Object(p))?;
        let Some(job) = pending_job(&first) else { return Ok(first) };
        let entry = self.follow(job, progress)?;
        Ok(merge_job(first, entry))
    }

    /// Waits for `job`, reporting its progress; cancels it when `progress` says so.
    fn follow(&mut self, job: u64, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        loop {
            let entry = self.client.call("jobs.wait", json!({ "job": job, "timeoutMs": POLL_MS }))?;
            let done = entry.pointer("/progress/done").and_then(Value::as_u64).unwrap_or(0);
            let total = entry.pointer("/progress/total").and_then(Value::as_u64).unwrap_or(0);
            let label = entry.get("label").and_then(Value::as_str).unwrap_or("");
            let wanted = progress.report(done, total, label);
            if entry.get("pending") != Some(&Value::Bool(true)) {
                return Ok(entry);
            }
            if !wanted {
                if let Err(e) = self.client.call("jobs.cancel", json!({ "job": job })) {
                    log::warn!("cancelling job {job} in the app failed: {e}");
                }
                return Err(ToolError::new("cancelled", "the request was cancelled"));
            }
        }
    }

    /// A step `{command, params?}` whose command takes `ids` and was given none gets the app's
    /// Photo Tray ([`tray::fill_ids`]); `photos` keeps the tray once read, so the steps of one
    /// group read it once. A command the engine does not list, or `params` that are not an
    /// object, go as they are: the app answers them.
    fn fill_from_tray(&mut self, step: &mut Map<String, Value>, photos: &mut Option<Vec<PhotoId>>) -> Result<(), ToolError> {
        let Some(doc) = step.get("command").and_then(Value::as_str).and_then(params_doc) else { return Ok(()) };
        let mut params = match step.get("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(o)) => o.clone(),
            Some(_) => return Ok(()),
        };
        let filled = tray::fill_ids(&mut params, doc, || match photos {
            Some(ids) => Ok::<_, ToolError>(ids.clone()),
            None => {
                let ids = self.tray_photos()?;
                *photos = Some(ids.clone());
                Ok(ids)
            }
        })?;
        if filled {
            step.insert("params".into(), Value::Object(params));
        }
        Ok(())
    }

    /// The photos in the app's Photo Tray (`library.inspect` → `tray.items`), in tray order.
    fn tray_photos(&mut self) -> Result<Vec<PhotoId>, ToolError> {
        let state = self.tray_state()?;
        Ok(state["tray"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(PhotoId::new).collect())
    }

    /// `{section, selection, held, tray}` from the app's `library.inspect`.
    fn tray_state(&mut self) -> Result<Value, ToolError> {
        let lib = self.client.call("library.inspect", json!({}))?;
        let list = |p: &str| lib.pointer(p).filter(|v| v.is_array()).cloned().unwrap_or_else(|| json!([]));
        Ok(json!({
            "section": lib.pointer("/selection/section").cloned().unwrap_or(Value::Null),
            "selection": list("/selection/ids"),
            "held": list("/tray/held"),
            "tray": list("/tray/items"),
        }))
    }

    /// `tray.select {ids, section?, mode?, hold?}` as the app's `select.set` then `tray.hold`.
    fn tray_select(&mut self, params: &Value) -> Result<Value, ToolError> {
        let p = fields(params, &["ids", "section", "mode", "hold"], "tray.select")?;
        let Some(list) = p.get("ids").and_then(Value::as_array) else { return Err(bad("`ids` must be a list of photo ids")) };
        if list.len() > MAX_TRAY_IDS {
            return Err(ToolError::new("limit", format!("at most {MAX_TRAY_IDS} photos at once")));
        }
        let mut ids = Vec::with_capacity(list.len());
        for v in list {
            ids.push(v.as_u64().ok_or_else(|| bad(format!("`ids` must hold photo ids, not {v}")))?);
        }
        let mut section = match p.get("section") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(other) => return Err(bad(format!("`section` must be a section id (folder:<id>, album:<id>), not {other}"))),
        };
        let mode = match p.get("mode") {
            None | Some(Value::Null) => "replace",
            Some(Value::String(s)) if matches!(s.as_str(), "replace" | "add" | "remove") => s.as_str(),
            Some(other) => return Err(bad(format!("`mode` must be replace, add or remove, not {other}"))),
        };
        let hold = match p.get("hold") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(other) => return Err(bad(format!("`hold` must be true or false, not {other}"))),
        };
        let chosen: Vec<u64> = if mode == "replace" {
            ids
        } else {
            let state = self.tray_state()?;
            let current_section = state.get("section").and_then(Value::as_str).map(str::to_string);
            let same = section.is_none() || section == current_section;
            let current: Vec<u64> =
                if same { state["selection"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect() } else { Vec::new() };
            if section.is_none() {
                section = current_section;
            }
            if mode == "add" {
                let mut out = current;
                for id in ids {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
                out
            } else {
                current.into_iter().filter(|id| !ids.contains(id)).collect()
            }
        };
        let mut set = Map::new();
        set.insert("ids".into(), json!(chosen));
        if let Some(s) = section {
            set.insert("section".into(), json!(s));
        }
        self.client.call("engine.execute", json!({ "command": "select.set", "params": set }))?;
        if hold {
            self.client.call("engine.execute", json!({ "command": "tray.hold", "params": {} }))?;
        }
        self.tray_state()
    }

    /// `engine.group {steps, label?}`: one step is one `engine.execute`; several are one `batch`
    /// that stops at the first failure, which is answered with the step named.
    fn group(&mut self, params: &Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        let p = fields(params, &["steps", "label"], "engine.group")?;
        let Some(steps) = p.get("steps").and_then(Value::as_array) else { return Err(bad("`steps` must be a list of {command, params?}")) };
        let max = dalica_engine::session::MAX_GROUP_STEPS;
        if steps.is_empty() || steps.len() > max {
            return Err(ToolError::new("limit", format!("`steps` holds 1 to {max} commands, not {}", steps.len())));
        }
        let mut prepared = Vec::with_capacity(steps.len());
        let mut photos = None;
        for (n, step) in steps.iter().enumerate() {
            let at = |e: ToolError| ToolError { message: format!("step {}: {}", n.saturating_add(1), e.message), ..e };
            let s = fields(step, &["command", "params"], "a step of engine.group").map_err(at)?;
            let Some(command) = s.get("command").and_then(Value::as_str) else { return Err(at(bad("`command` must name the command"))) };
            let mut o = Map::new();
            o.insert("command".into(), json!(command));
            if let Some(params) = s.get("params").filter(|v| !v.is_null()) {
                o.insert("params".into(), params.clone());
            }
            self.fill_from_tray(&mut o, &mut photos).map_err(at)?;
            prepared.push(Value::Object(o));
        }
        if let [one] = prepared.as_slice() {
            let r = self.execute(one.clone(), progress)?;
            return Ok(json!({ "results": [r] }));
        }
        let reply = self.client.call("batch", json!({ "steps": prepared, "stopOnError": true }))?;
        let results = reply.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut out = Vec::with_capacity(results.len());
        for (n, r) in results.iter().enumerate() {
            if r.get("ok") == Some(&Value::Bool(true)) {
                out.push(r.get("result").cloned().unwrap_or(Value::Null));
                continue;
            }
            let e = reply_result(r).err().unwrap_or_else(|| ToolError::new("internal", "a failed step without an error"));
            return Err(ToolError {
                message: format!(
                    "step {}: {} (the {n} step(s) before it stay applied: the app runs them as separate undo steps)",
                    n.saturating_add(1),
                    e.message
                ),
                ..e
            });
        }
        Ok(json!({ "results": out }))
    }
}

impl Backend for Connect {
    fn call(&mut self, method: &str, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        let params = if params.is_null() { json!({}) } else { params };
        match method {
            "engine.execute" => self.execute(params, progress),
            "engine.group" => self.group(&params, progress),
            "tray.select" => self.tray_select(&params),
            "tray.state" => {
                fields(&params, &[], method)?;
                self.tray_state()
            }
            other => self.client.call(other, params),
        }
    }

    fn render(&mut self, id: Option<u64>, max_side: u32) -> Result<Rendered, ToolError> {
        let id = match id {
            Some(id) => id,
            None => match self.tray_state()?["tray"].as_array().and_then(|t| t.first()).and_then(Value::as_u64) {
                Some(id) => id,
                None => return Err(ToolError::new("disabled", "Select a photo first, or pass `id`.")),
            },
        };
        let photos = self.client.call("engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [id] } }))?;
        let Some(photo) = photos.as_array().and_then(|p| p.first()) else { return Err(ToolError::new("not_found", format!("no photo {id}"))) };
        let path = PathBuf::from(photo.get("path").and_then(Value::as_str).unwrap_or_default());
        let entries: Vec<&str> = photo.pointer("/recipe/edits").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        render_file(id, &path, &entries, max_side)
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("connected to the Dalica app at {} (its control channel)", self.client.addr)
    }
}

/// The params doc of engine command `command` (the app's registry is the engine's), `None` for a
/// command the engine does not list (the app's own view commands take no photo ids).
fn params_doc(command: &str) -> Option<&'static str> {
    dalica_engine::cmd::specs().into_iter().find(|s| s.id == command).map(|s| s.params)
}

/// The job a reply names while it is still running (`{job, pending: true}`).
fn pending_job(reply: &Value) -> Option<u64> {
    if reply.get("pending") == Some(&Value::Bool(true)) { reply.get("job").and_then(Value::as_u64) } else { None }
}

/// The command's own fields, then the job's final entry (`pending` dropped, `scanning: true`
/// made false), as the app's own wait answers (`control-protocol.md` §5.3).
fn merge_job(first: Value, entry: Value) -> Value {
    let Value::Object(mut out) = first else { return entry };
    if let Value::Object(e) = entry {
        out.extend(e);
    }
    out.remove("pending");
    if out.get("scanning") == Some(&Value::Bool(true)) {
        out.insert("scanning".into(), Value::Bool(false));
    }
    Value::Object(out)
}

/// Renders the photo file at `path` with the recipe `entries`, its long side at most `max_side`,
/// through the one output pipeline (PNG, or JPEG when the PNG would pass the cap).
fn render_file(id: u64, path: &Path, entries: &[&str], max_side: u32) -> Result<Rendered, ToolError> {
    let recipe = Recipe::parse(entries).map_err(|e| ToolError::new("unsupported", format!("photo {id}'s edits cannot be read: {e}")))?;
    let bytes = match dalica_fs::read(path, dalica_create::export::MAX_FILE) {
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
    Ok(Rendered { id, mime, width: out.report.width, height: out.report.height, recipe: recipe.print(), bytes: out.bytes })
}

/// Downscales a PNG so its long side is at most `max_side` (a screenshot), through the output
/// pipeline; the bytes as they are when they already fit.
pub(crate) fn fit_png(png: Vec<u8>, width: u64, height: u64, max_side: u32) -> Result<(Vec<u8>, u64, u64), ToolError> {
    if width.max(height) <= u64::from(max_side) {
        return Ok((png, width, height));
    }
    let mut spec = Spec::new(Purpose::Export);
    spec.format = Choice::Exactly(OutFormat::Png);
    spec.long_side = Some(max_side);
    let out = output::render(&png, &Recipe::default(), &spec)
        .map_err(|e| ToolError::new("internal", format!("the screenshot cannot be scaled: {}", e.reason())))?;
    Ok((out.bytes, u64::from(out.report.width), u64::from(out.report.height)))
}

/// `params` as an object of `allowed` keys (`null` is `{}`); anything else is `bad_params`.
fn fields(params: &Value, allowed: &[&str], method: &str) -> Result<Map<String, Value>, ToolError> {
    let map = match params {
        Value::Null => return Ok(Map::new()),
        Value::Object(m) => m,
        other => return Err(bad(format!("{method} takes an object, not {other}"))),
    };
    if let Some(k) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(bad(format!("{method} takes no \"{k}\"; it takes: {}", allowed.join(", "))));
    }
    Ok(map.clone())
}

fn bad(message: impl Into<String>) -> ToolError {
    ToolError::new("bad_params", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    /// Loopback only; the port is required and not 0.
    #[test]
    fn connect_addresses_are_loopback_only() {
        assert_eq!(parse_addr("127.0.0.1:7990").unwrap(), "127.0.0.1:7990".parse().unwrap());
        assert_eq!(parse_addr("[::1]:18020").unwrap(), "[::1]:18020".parse().unwrap());
        assert_eq!(parse_addr("localhost:18020").unwrap(), "127.0.0.1:18020".parse().unwrap());
        assert_eq!(parse_addr("127.8.9.10:1").unwrap().port(), 1);
        for bad in ["192.168.1.2:7990", "0.0.0.0:7990", "example.com:7990", "127.0.0.1", "127.0.0.1:0", "127.0.0.1:x", "[::2]:7990"] {
            assert!(parse_addr(bad).is_err(), "{bad}");
        }
    }

    /// `--connect` takes an optional address, `--control-token-file` a path; the rest is left.
    #[test]
    fn options_are_taken_out_of_the_arguments() {
        let mut a = strings(&["--connect", "--script", "s.jsonl"]);
        let o = Options::take(&mut a).unwrap().unwrap();
        assert_eq!((o.addr, o.token_file, a), ("127.0.0.1:7990".parse().unwrap(), None, strings(&["--script", "s.jsonl"])));
        let mut a = strings(&["x", "--control-token-file", "t", "--connect", "localhost:18020"]);
        let o = Options::take(&mut a).unwrap().unwrap();
        assert_eq!((o.addr.port(), o.token_file, a), (18020, Some(PathBuf::from("t")), strings(&["x"])));
        assert_eq!(Options::take(&mut strings(&["a"])).unwrap(), None);
        assert!(Options::take(&mut strings(&["--control-token-file", "t"])).is_err());
        assert!(Options::take(&mut strings(&["--connect", "10.0.0.1:80"])).is_err());
        assert!(Options::take(&mut strings(&["--connect", "--control-token-file"])).is_err());
    }

    /// The app's default token file: the runtime folder on Linux, the cache's `run` otherwise.
    #[test]
    fn the_default_token_file_is_the_apps() {
        fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
        }
        let f = default_token_file(18020, env(&[("HOME", "/home/a"), ("XDG_RUNTIME_DIR", "/run/user/1000"), ("LOCALAPPDATA", "C:\\L")]));
        let name = f.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str());
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert_eq!(name, Some("control-18020.token"));
            assert_eq!(f, Some(PathBuf::from("/run/user/1000/dalica/control-18020.token")));
            assert_eq!(default_token_file(1, env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.cache/dalica/run/control-1.token")));
            assert_eq!(default_token_file(1, env(&[("HOME", "rel")])), None);
        }
    }

    /// Lines up to the limit, the last one without its newline, and an over-long one refused.
    #[test]
    fn reply_lines_are_capped() {
        let mut r = std::io::Cursor::new(b"ab\n\ncd".to_vec());
        assert_eq!(read_line(&mut r, 8).unwrap(), Some(b"ab".to_vec()));
        assert_eq!(read_line(&mut r, 8).unwrap(), Some(Vec::new()));
        assert_eq!(read_line(&mut r, 8).unwrap(), Some(b"cd".to_vec()));
        assert_eq!(read_line(&mut r, 8).unwrap(), None);
        assert!(read_line(&mut std::io::Cursor::new(b"123456789\n".to_vec()), 8).is_err());
    }

    /// The command's own fields, then the job's; `pending` dropped, `scanning` false.
    #[test]
    fn a_followed_job_merges_into_the_reply() {
        let first = json!({ "folder": 3, "job": 7, "pending": true, "scanning": true });
        let entry = json!({ "job": 7, "state": "done", "result": { "photos": 2 } });
        assert_eq!(pending_job(&first), Some(7));
        assert_eq!(pending_job(&entry), None);
        assert_eq!(merge_job(first, entry), json!({ "folder": 3, "job": 7, "scanning": false, "state": "done", "result": { "photos": 2 } }));
    }
}
