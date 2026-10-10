//! Security tests of the control channel (`control-protocol.md` §1–§4, `security.md` §6.2–§6.5,
//! roadmap M2.5), against the real `dalica` binary started with `--headless --control <port>` on an
//! empty library in a temporary home: what a hostile or confused client sends is answered with one
//! error line and the connection is closed, nothing it sent runs, the connection limit holds, a token
//! file others may read is refused, and no flag, value or variable makes the app listen anywhere
//! but `127.0.0.1`.
//!
//! Every instance runs with its home, settings, cache, run and temp folders in its own temporary
//! directory, on a port the OS picked, so the tests run in parallel with each other and with the
//! rest of the suite.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The request line limit (`control-protocol.md` §4): 1 MiB, the newline included.
const MAX_LINE: usize = 1 << 20;
/// Open connections at once (§4).
const MAX_CONNECTIONS: usize = 16;
/// The first line must be a successful `auth` within this (`security.md` §6.5).
const AUTH_WITHIN: Duration = Duration::from_secs(10);
/// The longest a test waits for a reply, a close or the app.
const PATIENCE: Duration = Duration::from_secs(60);
/// The longest a test waits for the server to free the slots of connections the client closed. The
/// server frees a slot when that connection's thread reads the end of its stream, which can come a
/// little after the client's close returned (notably on Windows), so a connection made right after
/// may still be refused with `limit`.
const SLOT_FREED_WITHIN: Duration = Duration::from_secs(10);
/// How long a new connection waits for a first line before it counts as accepted: a refused one is
/// answered `limit` as soon as it is accepted; an accepted one says nothing until `auth`.
const REFUSAL_WITHIN: Duration = Duration::from_secs(1);

/// A port nobody listens on now (the OS picks it; it is free again once the probe closes).
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// `dalica ARGS` with every folder it could touch inside `home`, its standard error in
/// `home/stderr.log`.
fn dalica(home: &Path, args: &[String]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_dalica"));
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    // A file, not a pipe: a pipe nobody reads could fill and stall the app.
    let log = std::fs::OpenOptions::new().create(true).append(true).open(home.join("stderr.log")).unwrap();
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log));
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join("config")),
        ("XDG_CACHE_HOME", home.join("cache")),
        ("XDG_DATA_HOME", home.join("data")),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("APPDATA", home.join("appdata")),
        ("LOCALAPPDATA", home.join("localappdata")),
        ("TMPDIR", tmp.clone()),
        ("TMP", tmp.clone()),
        ("TEMP", tmp),
    ] {
        c.env(k, v);
    }
    for k in [
        "DALICA_HOME",
        "DALICA_CONTROL_PORT",
        "DALICA_CONTROL_TOKEN_FILE",
        "DALICA_AUTOMATION_READ_ROOT",
        "DALICA_AUTOMATION_WRITE_ROOT",
        "DALICA_PROFILE",
    ] {
        c.env_remove(k);
    }
    c
}

/// What the app in `home` wrote to standard error so far.
fn stderr_of(home: &Path) -> String {
    std::fs::read_to_string(home.join("stderr.log")).unwrap_or_default()
}

/// `--headless --control PORT --library <home>/library --control-token-file TOKEN_FILE`.
fn headless_args(home: &Path, port: u16, token_file: &Path) -> Vec<String> {
    let library = home.join("library");
    std::fs::create_dir_all(&library).unwrap();
    let mut args: Vec<String> = ["--headless", "--control", &port.to_string(), "--library"].map(String::from).to_vec();
    args.push(library.to_str().unwrap().to_string());
    args.push("--control-token-file".to_string());
    args.push(token_file.to_str().unwrap().to_string());
    args
}

/// A child process, killed if a test ends (or fails) without it having exited.
struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl Proc {
    /// Waits for the exit; kills the process and fails the test if it is still running after
    /// [`PATIENCE`].
    fn exit_code(&mut self, home: &Path) -> Option<i32> {
        let end = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status.code();
            }
            assert!(Instant::now() < end, "dalica is still running after {PATIENCE:?}: {}", stderr_of(home));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A running headless `dalica`. Its home is removed after the process is gone (fields drop in
/// order).
struct App {
    proc: Proc,
    port: u16,
    token: String,
    home: tempfile::TempDir,
}

impl App {
    /// A headless instance on a free port in a fresh home, once it accepts connections; `env` is
    /// added to its environment.
    fn start(env: &[(&str, &str)]) -> App {
        let home = tempfile::tempdir().unwrap();
        let token_file = home.path().join("agent.token");
        App::launch(home, &token_file, env)
    }

    /// A headless instance in `home` with `token_file`, once it accepts connections. Another
    /// process may take the probed port before the app binds it; then it tries another.
    fn launch(home: tempfile::TempDir, token_file: &Path, env: &[(&str, &str)]) -> App {
        for _ in 0..5 {
            let port = free_port();
            let mut cmd = dalica(home.path(), &headless_args(home.path(), port, token_file));
            for (k, v) in env {
                cmd.env(k, v);
            }
            let mut proc = Proc(cmd.spawn().unwrap());
            let ready = format!("dalica: headless, control on 127.0.0.1:{port}");
            let end = Instant::now() + PATIENCE;
            loop {
                let log = stderr_of(home.path());
                if log.contains(&ready) {
                    let token = std::fs::read_to_string(token_file).unwrap().trim().to_string();
                    assert_eq!(token.len(), 64, "a 64-character token in {}", token_file.display());
                    return App { proc, port, token, home };
                }
                if let Some(status) = proc.0.try_wait().unwrap() {
                    assert!(status.code() == Some(2) && log.contains("in use"), "dalica exited early with {status}: {log}");
                    break;
                }
                assert!(Instant::now() < end, "dalica did not start within {PATIENCE:?}: {log}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        panic!("five probed ports were taken before dalica could bind them");
    }

    /// Asserts the app is still running and serving: a new connection authenticates and reads the
    /// UI state.
    fn assert_serving(&mut self, after: &str) {
        assert!(self.proc.0.try_wait().unwrap().is_none(), "dalica exited after {after}: {}", stderr_of(self.home.path()));
        let mut c = Conn::open(self.port);
        c.auth(&self.token);
        let state = c.request("ui.state", json!({}));
        assert_eq!(state["ok"], true, "ui.state after {after}: {state}");
    }

    /// Quits the app over the channel; it must exit cleanly.
    fn quit(self) {
        let mut c = Conn::open(self.port);
        c.auth(&self.token);
        self.quit_over(c);
    }

    /// Quits the app over `c` (authenticated); it must exit cleanly.
    fn quit_over(mut self, mut c: Conn) {
        assert_eq!(c.request("app.quit", json!({}))["ok"], true);
        assert_eq!(self.proc.exit_code(self.home.path()), Some(0), "{}", stderr_of(self.home.path()));
    }
}

/// What reading a line from the server gave.
#[derive(Debug)]
enum Got {
    Line(String),
    /// The server closed the connection (end of stream).
    Closed,
    /// The OS reset or aborted the connection.
    Reset(ErrorKind),
}

/// A control connection.
struct Conn {
    out: TcpStream,
    replies: BufReader<TcpStream>,
    next: u64,
}

impl Conn {
    fn open(port: u16) -> Conn {
        let out = TcpStream::connect(("127.0.0.1", port)).unwrap();
        out.set_read_timeout(Some(PATIENCE)).unwrap();
        out.set_write_timeout(Some(PATIENCE)).unwrap();
        out.set_nodelay(true).unwrap();
        let replies = BufReader::new(out.try_clone().unwrap());
        Conn { out, replies, next: 0 }
    }

    /// Sends `bytes` in one write, as they are.
    fn send(&mut self, bytes: &[u8]) {
        self.out.write_all(bytes).unwrap();
    }

    fn read(&mut self) -> Got {
        let mut line = Vec::new();
        match self.replies.read_until(b'\n', &mut line) {
            Ok(0) => Got::Closed,
            Ok(_) => Got::Line(String::from_utf8(line).expect("replies are UTF-8")),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                panic!("no reply and no close within {PATIENCE:?}")
            }
            Err(e) => Got::Reset(e.kind()),
        }
    }

    /// The next reply line, which must be a JSON object, never anything HTTP-like.
    fn reply(&mut self) -> Value {
        match self.read() {
            Got::Line(line) => {
                assert!(!line.starts_with("HTTP/"), "the server never answers like HTTP: {line:?}");
                assert!(line.ends_with('\n'), "a reply is one whole line: {line:?}");
                let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"));
                assert!(v.is_object(), "{line}");
                v
            }
            other => panic!("expected a reply line, got {other:?}"),
        }
    }

    /// Asserts the server closed the connection without another reply.
    fn assert_closed(&mut self, what: &str) {
        match self.read() {
            Got::Closed | Got::Reset(_) => {}
            Got::Line(line) => panic!("{what}: the connection is still open and answered {line:?}"),
        }
    }

    /// Sends one request line with the next id; its reply, which must echo the id.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = format!("{}\n", json!({ "id": self.next, "method": method, "params": params }));
        self.send(line.as_bytes());
        let v = self.reply();
        assert_eq!(v["id"], self.next, "{v}");
        v
    }

    fn auth(&mut self, token: &str) {
        let v = self.request("auth", json!({ "token": token }));
        assert_eq!((v["ok"].clone(), v["result"]["app"].clone()), (json!(true), json!("dalica")), "{v}");
    }

    /// What the server sends first, within `wait`; `None` when it sends nothing in that time.
    fn first_within(&mut self, wait: Duration) -> Option<Got> {
        self.replies.get_ref().set_read_timeout(Some(wait)).unwrap();
        let mut line = Vec::new();
        let got = match self.replies.read_until(b'\n', &mut line) {
            Ok(0) => Some(Got::Closed),
            Ok(_) => Some(Got::Line(String::from_utf8(line).expect("replies are UTF-8"))),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                assert!(line.is_empty(), "half a line, then nothing for {wait:?}: {:?}", String::from_utf8_lossy(&line));
                None
            }
            Err(e) => Some(Got::Reset(e.kind())),
        };
        self.replies.get_ref().set_read_timeout(Some(PATIENCE)).unwrap();
        got
    }

    /// Asserts this new connection, to which nothing was sent, is refused: one `limit` line with
    /// no id that says how many connections are open, then the close.
    fn assert_limit(&mut self, what: &str) {
        let v = self.reply();
        assert_eq!((v["ok"].clone(), v["code"].clone(), v["id"].clone()), (json!(false), json!("limit"), Value::Null), "{what}: {v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("16 connections"), "{what}: {v}");
        self.assert_closed(what);
    }

    /// An authenticated connection, made while the server may still be freeing the slots of
    /// connections the client just closed: until one is free it refuses with `limit`, so this
    /// tries again with a short backoff for up to [`SLOT_FREED_WITHIN`]. Nothing is sent before
    /// the server's first line, so every refused try must be a clean `limit` line and the close
    /// (sending first could make the server close with unread data, a reset that may drop the
    /// line). Returns the connection and the tries it took.
    fn open_once_a_slot_is_free(port: u16, token: &str) -> (Conn, u32) {
        let end = Instant::now() + SLOT_FREED_WITHIN;
        let mut backoff = Duration::from_millis(10);
        let mut tries = 0;
        loop {
            tries += 1;
            let mut c = Conn::open(port);
            match c.first_within(REFUSAL_WITHIN) {
                // Silent: accepted, it waits for `auth`.
                None => {
                    c.auth(token);
                    return (c, tries);
                }
                Some(Got::Line(line)) => {
                    let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("try {tries}: not JSON ({e}): {line:?}"));
                    assert_eq!(
                        (v["ok"].clone(), v["code"].clone(), v["id"].clone()),
                        (json!(false), json!("limit"), Value::Null),
                        "try {tries}: {v}"
                    );
                    c.assert_closed(&format!("refused try {tries}"));
                }
                Some(other) => panic!("try {tries}: a new connection must be accepted or refused with a `limit` line, got {other:?}"),
            }
            assert!(Instant::now() < end, "no slot was free {SLOT_FREED_WITHIN:?} after connections closed: {tries} tries were refused with `limit`");
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(Duration::from_millis(200));
        }
    }

    /// Sends `bytes`; asserts one error reply with `code` that says the connection closes and does
    /// not echo the token, then the close. Returns the reply.
    fn refused(&mut self, bytes: &[u8], code: &str, token: &str) -> Value {
        let what = String::from_utf8_lossy(bytes.get(..bytes.len().min(80)).unwrap_or_default()).into_owned();
        self.send(bytes);
        let v = self.reply();
        assert_eq!((v["ok"].clone(), v["code"].clone()), (json!(false), json!(code)), "{what:?}: {v}");
        let message = v["error"].as_str().unwrap_or_default();
        assert!(message.contains("closing the connection"), "{what:?}: the message says the connection closes: {v}");
        assert!(!message.contains(token), "{what:?}: the token is never in a reply");
        self.assert_closed(&what);
        v
    }
}

fn auth_line(id: Value, token: &str) -> String {
    format!("{}\n", json!({ "id": id, "method": "auth", "params": { "token": token } }))
}

/// Scope: a wrong token closes the connection (`control-protocol.md` §2.2).
#[test]
fn a_wrong_token_gets_unauthorized_and_the_connection_closes() {
    let mut app = App::start(&[]);
    let token = app.token.clone();
    let flipped = {
        let (head, last) = token.split_at(63);
        format!("{head}{}", if last == "0" { "1" } else { "0" })
    };
    let wrong: Vec<(&str, Value)> = vec![
        ("zeros", json!({ "token": "0".repeat(64) })),
        ("last character changed", json!({ "token": flipped })),
        ("upper case", json!({ "token": token.to_uppercase() })),
        ("one character more", json!({ "token": format!("{token}0") })),
        ("one character less", json!({ "token": &token[..63] })),
        ("with a newline escape", json!({ "token": format!("{token}\n") })),
        ("empty", json!({ "token": "" })),
        ("a number", json!({ "token": 5 })),
        ("null", json!({ "token": null })),
        ("an array holding it", json!({ "token": [token] })),
        ("no token key", json!({ "key": token })),
        ("params a string", json!(token)),
        ("no params", Value::Null),
    ];
    for (i, (what, params)) in wrong.into_iter().enumerate() {
        if params == json!({ "token": token }) {
            continue; // an all-digit token has no upper case: nothing to test
        }
        let mut c = Conn::open(app.port);
        let line = if params.is_null() {
            format!("{}\n", json!({ "id": i, "method": "auth" }))
        } else {
            format!("{}\n", json!({ "id": i, "method": "auth", "params": params }))
        };
        let v = c.refused(line.as_bytes(), "unauthorized", &token);
        assert_eq!(v["id"], i, "{what}: the id is echoed: {v}");
    }
    // A right `auth` and a quit sent right behind a wrong one never run: one reply, then the close.
    let mut c = Conn::open(app.port);
    let burst = format!("{}{}{}\n", auth_line(json!(1), &"f".repeat(64)), auth_line(json!(2), &token), json!({ "id": 3, "method": "app.quit" }));
    c.refused(burst.as_bytes(), "unauthorized", &token);
    app.assert_serving("wrong tokens");
    app.quit();
}

/// Scope: no `auth` closes the connection: a first request other than a successful `auth` is
/// refused and does not run (`control-protocol.md` §2.2).
#[test]
fn a_first_request_other_than_auth_is_refused_and_does_not_run() {
    let mut app = App::start(&[]);
    let token = app.token.clone();
    let firsts = [
        json!({ "id": 1, "method": "ui.state" }),
        json!({ "id": 2, "method": "app.quit" }),
        json!({ "id": 3, "method": "methods" }),
        json!({ "id": 4, "method": "engine.execute", "params": { "command": "library.refresh", "params": {} } }),
        json!({ "id": 5, "method": "ui.key", "params": { "combo": "Ctrl+Q" } }),
        // The token in another method's params authenticates nothing.
        json!({ "id": 6, "method": "ui.state", "params": { "token": token } }),
        // The method name is exact.
        json!({ "id": 7, "method": "AUTH", "params": { "token": token } }),
        json!({ "id": 8, "method": " auth", "params": { "token": token } }),
        json!({ "id": "x", "method": "auth.", "params": { "token": token } }),
    ];
    for first in firsts {
        let mut c = Conn::open(app.port);
        let v = c.refused(format!("{first}\n").as_bytes(), "unauthorized", &token);
        assert_eq!(v["id"], first["id"], "{v}");
    }
    // A quit then the right `auth` in one write: the quit is refused, the `auth` after it never read.
    let mut c = Conn::open(app.port);
    let burst = format!("{}\n{}", json!({ "id": 1, "method": "app.quit" }), auth_line(json!(2), &token));
    c.refused(burst.as_bytes(), "unauthorized", &token);
    app.assert_serving("requests before auth (two of them app.quit)");
    app.quit();
}

/// Scope: no `auth` closes the connection: a client that sends nothing, an unterminated line, or
/// only blank lines is answered `unauthorized` and closed 10 s after it connected
/// (`security.md` §6.3); blank lines do not extend that.
#[test]
fn a_connection_that_does_not_authenticate_within_ten_seconds_is_closed() {
    let mut app = App::start(&[]);
    let (port, token) = (app.port, app.token.clone());
    let unterminated = auth_line(json!(1), &token).trim_end().to_string();
    let clients: Vec<(&str, std::thread::JoinHandle<(Value, Duration)>)> = [
        ("silent", Vec::new()),
        // A whole, right `auth`, but never framed by a newline.
        ("unterminated", vec![unterminated.into_bytes()]),
        // A blank line every 0.5 s for 8 s (blank lines are skipped, and must not reset the clock).
        ("blank lines", vec![b"\n".to_vec(); 16]),
    ]
    .into_iter()
    .map(|(what, writes)| {
        let token = token.clone();
        let handle = std::thread::spawn(move || {
            let mut c = Conn::open(port);
            let opened = Instant::now();
            for (i, w) in writes.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
                c.send(w);
            }
            let v = c.reply();
            let took = opened.elapsed();
            let message = v["error"].as_str().unwrap_or_default();
            assert!(!message.contains(&token));
            c.assert_closed(what);
            (v, took)
        });
        (what, handle)
    })
    .collect();
    for (what, handle) in clients {
        let (v, took) = handle.join().unwrap();
        assert_eq!((v["ok"].clone(), v["code"].clone(), v["id"].clone()), (json!(false), json!("unauthorized"), Value::Null), "{what}: {v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("no auth within 10 s"), "{what}: {v}");
        // The clock starts at accept, which is after the client's connect returned (minus slack
        // for timer granularity); it must not restart on a blank line (that would close at ~18 s).
        assert!(took >= AUTH_WITHIN - Duration::from_millis(500), "{what}: closed after {took:?}, before the 10 s");
        assert!(took < AUTH_WITHIN + Duration::from_secs(5), "{what}: closed only after {took:?}");
        eprintln!("numbers: {what} closed after {:.2} s", took.as_secs_f64());
    }
    app.assert_serving("clients that never authenticated");
    app.quit();
}

/// Scope: an oversized line closes the connection (`control-protocol.md` §2.3, §4): 1 MiB with
/// its newline is a line; a line that reaches 1 MiB without its newline is refused with `parse`,
/// before and after `auth`, even when it would be a valid request; a flood is cut off.
#[test]
fn an_oversized_line_gets_a_parse_error_and_the_connection_closes() {
    let mut app = App::start(&[]);
    let token = app.token.clone();
    // A valid `auth` padded with spaces to exactly the limit, its newline included: accepted.
    let mut exact = auth_line(json!("big"), &token).trim_end().as_bytes().to_vec();
    exact.resize(MAX_LINE - 1, b' ');
    exact.push(b'\n');
    let mut c = Conn::open(app.port);
    c.send(&exact);
    let v = c.reply();
    assert_eq!((v["ok"].clone(), v["id"].clone()), (json!(true), json!("big")), "a line of exactly 1 MiB is read: {v}");
    assert_eq!(c.request("ui.state", json!({}))["ok"], true);

    // The same `auth` padded to the limit without its newline can only grow past it: refused.
    // (Exactly the limit: the server has read everything sent, so it closes cleanly and the reply
    // is never lost to a reset.)
    let mut over = exact.get(..MAX_LINE - 1).unwrap().to_vec();
    over.push(b' ');
    let mut c = Conn::open(app.port);
    let t = Instant::now();
    c.refused(&over, "parse", &token);
    let refused_ms = t.elapsed().as_secs_f64() * 1000.0;
    // After `auth` too.
    let mut c = Conn::open(app.port);
    c.auth(&token);
    c.refused(&vec![b'x'; MAX_LINE], "parse", &token);

    // A flood of 64 MiB with no newline: the server stops reading at the limit and closes. The
    // flood is still arriving then, so the OS may reset the connection and drop the error line
    // (Windows does); what must hold is that it ends and nothing ran.
    let mut c = Conn::open(app.port);
    let mut writer = c.out.try_clone().unwrap();
    let flood = std::thread::spawn(move || {
        let chunk = vec![b'{'; 1 << 20];
        let mut sent = 0usize;
        for _ in 0..64 {
            match writer.write_all(&chunk) {
                Ok(()) => sent += chunk.len(),
                Err(_) => break,
            }
        }
        sent
    });
    let ending = match c.read() {
        Got::Line(line) => {
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["code"], "parse", "{v}");
            c.assert_closed("a flood");
            "a parse error, then the close".to_string()
        }
        Got::Closed => "the close, without the error line".to_string(),
        Got::Reset(kind) => format!("a reset ({kind:?})"),
    };
    let sent = flood.join().unwrap();
    assert!(sent < 64 << 20, "the server stopped reading the flood (the client sent {sent} bytes)");
    eprintln!("numbers: a 1 MiB line refused in {refused_ms:.1} ms; the flood ended with {ending} after {} KiB sent", sent >> 10);
    app.assert_serving("oversized lines");
    app.quit();
}

/// Scope: non-JSON and HTTP request lines close the connection (`control-protocol.md` §2.3,
/// `security.md` §6.4), before and after `auth`; nothing sent after them runs, and the answer is
/// never HTTP-like.
#[test]
fn non_json_and_http_request_lines_get_a_parse_error_and_the_connection_closes() {
    let mut app = App::start(&[]);
    let token = app.token.clone();
    let port = app.port;
    // What a web page's cross-origin fetch or form post would send, with a request in its body
    // (even one that knows the token).
    let body = format!("{}{}\n", auth_line(json!(1), &token), json!({ "id": 2, "method": "app.quit" }));
    let post = format!(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://example.com\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let get = format!("GET /?method=app.quit HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\r\n");
    let junk: Vec<Vec<u8>> = vec![
        post.into_bytes(),
        get.into_bytes(),
        b"OPTIONS * HTTP/1.1\r\n\r\n".to_vec(),
        b"hello\n".to_vec(),
        b"[1]\n".to_vec(),
        b"42\n".to_vec(),
        b"\"auth\"\n".to_vec(),
        b"null\n".to_vec(),
        b"{\"id\":1}\n".to_vec(),
        b"{\"id\":1,\"method\":5}\n".to_vec(),
        b"{\"id\":1,\"method\":\"auth\"\n".to_vec(),
        b"\xff\xfe{\"method\":\"auth\"}\n".to_vec(),
        b"{\"method\":\"auth\"}\0\n".to_vec(),
    ];
    for line in &junk {
        let mut c = Conn::open(port);
        c.refused(line, "parse", &token);
        // After `auth` too.
        let mut c = Conn::open(port);
        c.auth(&token);
        c.refused(line, "parse", &token);
    }
    // Junk and a quit in one write after `auth`: the quit never runs.
    let mut c = Conn::open(port);
    c.auth(&token);
    c.refused(format!("not json\n{}\n", json!({ "id": 9, "method": "app.quit" })).as_bytes(), "parse", &token);
    app.assert_serving("non-JSON and HTTP lines (two smuggling app.quit)");
    app.quit();
}

/// Scope: the 17th connection is refused (`control-protocol.md` §4): with 16 open, another gets
/// one `limit` line and is closed, the 16 keep working, and closing one frees its slot (within
/// [`SLOT_FREED_WITHIN`]: the server frees it when it reads the close, a little after the client
/// made it), after which the count is exact again.
#[test]
fn the_seventeenth_connection_gets_a_limit_error_and_is_closed() {
    let app = App::start(&[]);
    let mut open: Vec<Conn> = (0..MAX_CONNECTIONS)
        .map(|_| {
            let mut c = Conn::open(app.port);
            // Authenticated, so none is closed by the 10 s auth deadline while the test runs.
            c.auth(&app.token);
            c
        })
        .collect();
    for n in [MAX_CONNECTIONS + 1, MAX_CONNECTIONS + 2] {
        Conn::open(app.port).assert_limit(&format!("connection {n}"));
    }
    for c in &mut open {
        assert_eq!(c.request("ui.state", json!({}))["ok"], true, "the 16 open connections keep working");
    }
    drop(open.pop());
    let t = Instant::now();
    let (mut again, tries) = Conn::open_once_a_slot_is_free(app.port, &app.token);
    let freed_ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(again.request("ui.state", json!({}))["ok"], true, "the connection in the freed slot works");
    open.push(again);
    // 16 open again: the next one is refused, whatever the server has noticed yet (an unnoticed
    // close only makes it count more).
    Conn::open(app.port).assert_limit("connection 17 after a slot was freed and taken");
    eprintln!(
        "numbers: the freed slot was taken on try {tries}, {freed_ms:.0} ms after the close (with the {REFUSAL_WITHIN:?} of silence that tells an accepted connection)"
    );
    // Closing all 16 at once: the quit waits for the server to free a slot the same way.
    drop(open);
    let (c, _) = Conn::open_once_a_slot_is_free(app.port, &app.token);
    app.quit_over(c);
}

/// Writes a token file holding `token` (mode 0600 on Unix, from the umask elsewhere).
fn write_token_file(path: &Path, token: &str) {
    std::fs::write(path, format!("{token}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

/// Launches `dalica` with `args(port)` on a free port and the `env`; it must exit with status 2
/// (without starting). Returns its standard error. Retries when the probed port was taken.
fn refused_launch(home: &Path, args: impl Fn(u16) -> Vec<String>, env: &[(&str, &str)]) -> String {
    for _ in 0..5 {
        let _ = std::fs::remove_file(home.join("stderr.log"));
        let port = free_port();
        let mut cmd = dalica(home, &args(port));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut proc = Proc(cmd.spawn().unwrap());
        let code = proc.exit_code(home);
        let why = stderr_of(home);
        if why.contains("the port is in use") {
            continue;
        }
        assert_eq!(code, Some(2), "{:?}: {why}", args(port));
        return why;
    }
    panic!("five probed ports were taken before dalica could bind them");
}

/// Scope: a group-readable token file is refused (`control-protocol.md` §2.1, `security.md`
/// §6.3): so is one others may read or write, and a link; the file is left as it was, the token
/// never printed; once private, the same file works.
#[cfg(unix)]
#[test]
fn a_token_file_others_may_read_is_refused_at_startup() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("agent.token");
    let token = "0123456789abcdef".repeat(4);
    write_token_file(&file, &token);
    for mode in [0o640, 0o660, 0o620, 0o604, 0o644, 0o606, 0o777] {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        let why = refused_launch(home.path(), |port| headless_args(home.path(), port, &file), &[]);
        assert!(why.contains("chmod 600") && why.contains(&format!("{mode:03o}")), "mode {mode:03o}: {why}");
        assert!(why.contains(file.to_str().unwrap()), "the message names the file: {why}");
        assert!(!why.contains(&token), "mode {mode:03o}: the token is never printed");
        let meta = std::fs::metadata(&file).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, mode, "the file's mode is left alone");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{token}\n"), "the file is left alone");
    }
    // The same file through the variable instead of the flag.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let why = refused_launch(
        home.path(),
        |port| headless_args(home.path(), port, &file).into_iter().take(5).collect(),
        &[("DALICA_CONTROL_TOKEN_FILE", file.to_str().unwrap())],
    );
    assert!(why.contains("chmod 600"), "DALICA_CONTROL_TOKEN_FILE: {why}");
    // A link to a private token file.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = home.path().join("link.token");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let why = refused_launch(home.path(), |port| headless_args(home.path(), port, &link), &[]);
    assert!(why.contains("symbolic link") && !why.contains(&token), "{why}");
    // Private again: the app starts with the file's token.
    let mut app = App::launch(home, &file, &[]);
    assert_eq!(app.token, token);
    app.assert_serving("a private token file");
    app.quit();
}

/// Scope: a token file that is not a private file holding a token is refused, on every OS
/// (`control-protocol.md` §2.1): a folder, or a file without a token on its first line.
#[test]
fn a_token_file_without_a_token_or_that_is_a_folder_is_refused_at_startup() {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().join("folder.token");
    std::fs::create_dir_all(&folder).unwrap();
    let why = refused_launch(home.path(), |port| headless_args(home.path(), port, &folder), &[]);
    assert!(why.contains("not a regular file"), "{why}");
    for (name, text) in
        [("empty", String::new()), ("short", "abc".to_string()), ("spaced", format!(" {}", "a".repeat(64))), ("not hex", "g".repeat(64))]
    {
        let file = home.path().join(format!("{name}.token"));
        write_token_file(&file, &text);
        let why = refused_launch(home.path(), |port| headless_args(home.path(), port, &file), &[]);
        assert!(why.contains("does not start with"), "{name}: {why}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{text}\n"), "{name}: the file is left alone");
    }
}

/// This machine's own non-loopback addresses that a connection could reach it on: the source
/// address of the default IPv4 and IPv6 routes (a UDP `connect` sends nothing).
fn own_non_loopback_addresses() -> Vec<IpAddr> {
    let mut found = Vec::new();
    for (bind, probe) in [("0.0.0.0:0", "192.0.2.1:9"), ("[::]:0", "[2001:db8::1]:9")] {
        let ip = UdpSocket::bind(bind).and_then(|s| s.connect(probe).and_then(|()| s.local_addr())).map(|a| a.ip());
        if let Ok(ip) = ip
            && !ip.is_loopback()
            && !ip.is_unspecified()
        {
            found.push(ip);
        }
    }
    found
}

/// Scope: no flag binds a non-loopback address (`control-protocol.md` §1, `security.md` §6.2):
/// an address in `--control` or `DALICA_CONTROL_PORT`, or an address flag, exits 2 without
/// starting; a running instance answers on `127.0.0.1` and not on the machine's own addresses,
/// whatever address variables are set.
#[test]
fn no_flag_binds_a_non_loopback_address() {
    let home = tempfile::tempdir().unwrap();
    let library = home.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let lib = library.to_str().unwrap().to_string();
    let with = |extra: &[&str]| {
        let mut args: Vec<String> = vec!["--headless".into(), "--library".into(), lib.clone()];
        args.extend(extra.iter().map(|s| s.to_string()));
        args
    };
    let addresses: Vec<Box<dyn Fn(u16) -> String>> = vec![
        Box::new(|p| format!("0.0.0.0:{p}")),
        Box::new(|_| "0.0.0.0".to_string()),
        Box::new(|p| format!("[::]:{p}")),
        Box::new(|_| "::".to_string()),
        Box::new(|p| format!("192.168.1.10:{p}")),
        Box::new(|p| format!("10.0.0.1:{p}")),
        Box::new(|p| format!("localhost:{p}")),
        Box::new(|p| format!("127.0.0.1:{p}")),
        Box::new(|p| format!("*:{p}")),
        Box::new(|p| format!("{p}@0.0.0.0")),
        Box::new(|p| format!("{p} ")),
    ];
    for address in &addresses {
        let why = refused_launch(home.path(), |p| with(&["--control", &address(p)]), &[]);
        assert!(why.contains("is not a port"), "--control {}: {why}", address(1));
        let why = refused_launch(home.path(), |_| with(&["--control"]), &[("DALICA_CONTROL_PORT", &address(18049))]);
        assert!(why.contains("DALICA_CONTROL_PORT") && why.contains("is not a port"), "DALICA_CONTROL_PORT={}: {why}", address(18049));
    }
    let flags: Vec<Box<dyn Fn(u16) -> Vec<String>>> = vec![
        Box::new(|p| with(&["--control", &p.to_string(), "--bind", "0.0.0.0"])),
        Box::new(|p| with(&["--control", &p.to_string(), "--host", "0.0.0.0"])),
        Box::new(|p| with(&["--control", &p.to_string(), "--listen", &format!("0.0.0.0:{p}")])),
        Box::new(|p| with(&["--control", &p.to_string(), "--address", "::"])),
        Box::new(|p| with(&["--control", &p.to_string(), "--control-address", "0.0.0.0"])),
        Box::new(|p| with(&["--control", &p.to_string(), "--control-host", "0.0.0.0"])),
        Box::new(|p| with(&[&format!("--control=0.0.0.0:{p}")])),
        Box::new(|p| with(&[&format!("--control={p}")])),
    ];
    for flag in &flags {
        let why = refused_launch(home.path(), flag, &[]);
        assert!(why.contains("usage: dalica"), "{:?}: {why}", flag(1));
    }

    // Address-like variables are not read: the instance is on loopback only.
    let env = [("DALICA_CONTROL_HOST", "0.0.0.0"), ("DALICA_CONTROL_ADDRESS", "0.0.0.0"), ("DALICA_CONTROL_BIND", "::"), ("HOST", "0.0.0.0")];
    let mut app = App::start(&env);
    app.assert_serving("start");
    let own = own_non_loopback_addresses();
    for ip in &own {
        let to = SocketAddr::new(*ip, app.port);
        let reached = TcpStream::connect_timeout(&to, Duration::from_secs(5));
        assert!(reached.is_err(), "dalica answers on {to}, a non-loopback address");
    }
    eprintln!("numbers: refused on {} own non-loopback address(es): {own:?}", own.len());
    app.quit();
}
