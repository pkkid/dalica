//! The TCP side of the control channel (`control-protocol.md` §2–§4, `security.md` §6.2–§6.5): a
//! listener on `127.0.0.1` only, one thread per connection, newline-framed JSON requests, `auth`
//! first (answered here, in constant time, never by the UI), and the limits. Authenticated requests
//! go to the UI thread through a channel; it answers them from its logic tick.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_ui::control::{Code, Error, MAX_REQUEST_LINE, Reply, Request, reply_line};
use serde_json::{Value, json};

use super::token::Token;

/// Open connections at once (§4); more get one `limit` line and are closed.
pub const MAX_CONNECTIONS: usize = 16;
/// Socket read and write timeout (§4): a connection idle this long is closed.
pub const IDLE: Duration = Duration::from_secs(30);
/// The first line must be a successful `auth` within this (`security.md` §6.5).
pub const AUTH_WITHIN: Duration = Duration::from_secs(10);
/// A request still queued this long after it arrived is refused before dispatch (§4).
pub const REPLY_DEADLINE: Duration = Duration::from_secs(60);
/// The longest a dispatched request may run before its connection answers `timeout`: `ui.wait`
/// and `ui.settle` take up to 60 s, and a screenshot settles 5 s, then captures.
pub const RUN_LIMIT: Duration = Duration::from_secs(70);
/// `protocol` in the `auth` reply (§2.2): bumped on incompatible changes.
pub const PROTOCOL: u64 = 1;
/// The method the app handles after flushing, then exits (§6).
pub const QUIT: &str = "app.quit";

/// Wakes the UI thread when a message arrives (the window: `request_repaint`).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the connection threads tell the UI thread.
pub enum Msg {
    /// An authenticated request, in arrival order per connection.
    Request(Pending),
    /// A connection closed: release the keys it held down (§6.9).
    Closed(u64),
}

/// What the UI thread tells a connection about its request.
#[derive(Debug)]
enum Answer {
    /// The request was taken off the queue and started.
    Dispatched,
    Done(Reply),
}

/// A request waiting for the UI thread.
pub struct Pending {
    pub conn: u64,
    pub req: Request,
    arrived: Instant,
    answer: SyncSender<Answer>,
}

impl Pending {
    /// Takes the request off the queue: `false` (and a `timeout` reply) when it waited past the
    /// deadline, so it must not run.
    pub fn dispatch(&self) -> bool {
        if self.arrived.elapsed() > REPLY_DEADLINE {
            let _ = self.answer.try_send(Answer::Done(Err(queued_too_long())));
            return false;
        }
        let _ = self.answer.try_send(Answer::Dispatched);
        true
    }

    /// Sends the reply (dropped if the connection has given up on it).
    pub fn reply(self, reply: Reply) {
        let _ = self.answer.try_send(Answer::Done(reply));
    }
}

fn queued_too_long() -> Error {
    Error::new(Code::Timeout, format!("the request was still queued after {} s and did not run; the app is busy", REPLY_DEADLINE.as_secs()))
}

/// The UI thread's end of the channel.
pub struct Hub {
    pub rx: Receiver<Msg>,
    quit_written: Receiver<()>,
}

impl Hub {
    /// Waits (at most `within`) until the reply to `app.quit` has been written to its client.
    pub fn wait_quit_written(&self, within: Duration) {
        let _ = self.quit_written.recv_timeout(within);
    }
}

/// What every connection thread shares.
struct Shared {
    token: Token,
    tx: Mutex<Sender<Msg>>,
    wake: Waker,
    open: AtomicUsize,
    quit_written: SyncSender<()>,
}

impl Shared {
    fn send(&self, msg: Msg) -> bool {
        let sent = self.tx.lock().unwrap_or_else(PoisonError::into_inner).send(msg).is_ok();
        (self.wake)();
        sent
    }
}

/// Binds `127.0.0.1:port`: always the loopback address, whatever the caller wants.
pub fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

/// Starts accepting on `listener` (already bound) on its own thread.
pub fn start(listener: TcpListener, token: Token, wake: Waker) -> std::io::Result<Hub> {
    let (tx, rx) = mpsc::channel();
    let (quit_tx, quit_rx) = mpsc::sync_channel(1);
    let shared = Arc::new(Shared { token, tx: Mutex::new(tx), wake, open: AtomicUsize::new(0), quit_written: quit_tx });
    std::thread::Builder::new().name("dalica-control".into()).spawn(move || accept(listener, shared))?;
    Ok(Hub { rx, quit_written: quit_rx })
}

fn accept(listener: TcpListener, shared: Arc<Shared>) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log::warn!("control: accept failed: {e}");
                // Out of file descriptors and the like: do not spin.
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        if shared.open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            refuse(stream);
            continue;
        }
        shared.open.fetch_add(1, Ordering::SeqCst);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let s = shared.clone();
        let spawned = std::thread::Builder::new().name(format!("dalica-control-{id}")).spawn(move || {
            let _slot = Slot { shared: s.clone(), id };
            connection(stream, id, &s);
        });
        if let Err(e) = spawned {
            log::warn!("control: cannot start a connection thread: {e}");
            shared.open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// One error line to a connection over the limit, then close it. Bounded, so a client that does
/// not read cannot stall the listener.
fn refuse(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let e = Error::new(Code::Limit, format!("{MAX_CONNECTIONS} connections are open, the most allowed; close one and connect again"));
    let _ = stream.write_all(reply_line(&Value::Null, &Err(e)).as_bytes());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// Frees a connection's slot and tells the UI it closed, however the thread ends.
struct Slot {
    shared: Arc<Shared>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.shared.open.fetch_sub(1, Ordering::SeqCst);
        self.shared.send(Msg::Closed(self.id));
    }
}

/// Why a line could not be read.
#[derive(Debug, PartialEq)]
pub enum LineError {
    /// Longer than the limit (newline included).
    TooLong,
    /// Nothing arrived within the timeout (or the deadline passed).
    Timeout,
    Io(ErrorKind),
}

/// Reads one `\n`-terminated line of at most `max` bytes (newline included). `Ok(None)` at the end
/// of the stream; a last line without a newline is returned as it is. Each read waits at most
/// [`IDLE`], and never past `deadline`.
pub fn read_line(r: &mut BufReader<TcpStream>, max: usize, deadline: Option<Instant>) -> Result<Option<Vec<u8>>, LineError> {
    let mut line = Vec::new();
    loop {
        let wait = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(LineError::Timeout);
                }
                left.min(IDLE)
            }
            None => IDLE,
        };
        r.get_ref().set_read_timeout(Some(wait)).map_err(|e| LineError::Io(e.kind()))?;
        let buf = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Err(LineError::Timeout),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(LineError::Io(e.kind())),
        };
        if buf.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let (take, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        // Without its newline yet, a line already at the limit can only grow past it.
        if line.len().saturating_add(take) > max || (!done && line.len().saturating_add(take) >= max) {
            return Err(LineError::TooLong);
        }
        line.extend_from_slice(buf.get(..take).unwrap_or_default());
        r.consume(take);
        if done {
            return Ok(Some(line));
        }
    }
}

fn app_info() -> Value {
    let build = if cfg!(debug_assertions) { "dev build" } else { "release" };
    json!({ "app": "dalica", "version": format!("{} ({build})", env!("CARGO_PKG_VERSION")), "protocol": PROTOCOL, "pid": std::process::id() })
}

/// Answers an `auth` request: the app's identity, or `unauthorized`.
fn check_auth(token: &Token, req: &Request) -> Reply {
    let given = req.params.get("token").and_then(Value::as_str);
    match given {
        Some(t) if token.matches(t) => Ok(app_info()),
        Some(_) => Err(Error::new(Code::Unauthorized, "wrong token; read it from the token file (control-protocol.md §2.1); closing the connection")),
        None => Err(Error::new(Code::Unauthorized, "auth needs {\"token\": \"<64 hex characters>\"}; closing the connection")),
    }
}

/// Writes one reply line; false when the client is gone.
fn write(stream: &mut TcpStream, id: &Value, reply: &Reply) -> bool {
    stream.write_all(reply_line(id, reply).as_bytes()).and_then(|()| stream.flush()).is_ok()
}

/// Serves one connection until it closes, breaks a rule or idles out.
fn connection(stream: TcpStream, id: u64, shared: &Shared) {
    let opened = Instant::now();
    let _ = stream.set_nodelay(true);
    if stream.set_write_timeout(Some(IDLE)).is_err() {
        return;
    }
    let Ok(mut out) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut authed = false;
    loop {
        let deadline = (!authed).then_some(opened + AUTH_WITHIN);
        let line = match read_line(&mut reader, MAX_REQUEST_LINE, deadline) {
            Ok(Some(line)) => line,
            Ok(None) | Err(LineError::Io(_)) => return,
            Err(LineError::Timeout) if !authed => {
                let e = Error::new(Code::Unauthorized, format!("no auth within {} s; closing the connection", AUTH_WITHIN.as_secs()));
                write(&mut out, &Value::Null, &Err(e));
                return;
            }
            // Idle: the client reconnects (§4).
            Err(LineError::Timeout) => return,
            Err(LineError::TooLong) => {
                let e = Error::new(Code::Parse, format!("not a request: the line is over {MAX_REQUEST_LINE} bytes; closing the connection"));
                write(&mut out, &Value::Null, &Err(e));
                return;
            }
        };
        let req = match Request::parse(&line) {
            Ok(None) => continue,
            Ok(Some(req)) => req,
            Err(mut e) => {
                e.message.push_str("; closing the connection");
                write(&mut out, &Value::Null, &Err(e));
                return;
            }
        };
        if !authed || req.method == "auth" {
            let reply = if req.method == "auth" {
                check_auth(&shared.token, &req)
            } else {
                Err(Error::new(Code::Unauthorized, "the first request must be auth (control-protocol.md §2.2); closing the connection"))
            };
            let ok = reply.is_ok();
            if !write(&mut out, &req.id, &reply) || !ok {
                return;
            }
            authed = true;
            continue;
        }
        let quit = req.method == QUIT;
        let rid = req.id.clone();
        let reply = ask_ui(shared, id, req);
        let written = write(&mut out, &rid, &reply);
        if quit {
            let _ = shared.quit_written.try_send(());
            return;
        }
        if !written || reply.as_ref().is_err_and(|e| e.code.closes()) {
            return;
        }
    }
}

/// Hands `req` to the UI thread and waits for its reply: until the deadline while it is queued,
/// then [`RUN_LIMIT`] once it runs.
fn ask_ui(shared: &Shared, conn: u64, req: Request) -> Reply {
    let (tx, rx) = mpsc::sync_channel(2);
    let arrived = Instant::now();
    if !shared.send(Msg::Request(Pending { conn, req, arrived, answer: tx })) {
        return Err(Error::new(Code::Internal, "the app is shutting down"));
    }
    let mut limit = arrived + REPLY_DEADLINE;
    loop {
        match rx.recv_timeout(limit.saturating_duration_since(Instant::now())) {
            Ok(Answer::Dispatched) => limit = Instant::now() + RUN_LIMIT,
            Ok(Answer::Done(reply)) => return reply,
            Err(RecvTimeoutError::Timeout) if limit == arrived + REPLY_DEADLINE => return Err(queued_too_long()),
            Err(RecvTimeoutError::Timeout) => {
                return Err(Error::new(
                    Code::Timeout,
                    format!("the UI did not answer within {} s; the request may still run or have run: inspect before retrying", RUN_LIMIT.as_secs()),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::new(Code::Internal, "the app dropped the request")),
        }
    }
}

/// For tests of the UI side: a pending request whose answers land in the returned receiver.
#[cfg(test)]
pub fn pending_for_test(conn: u64, req: Request, arrived: Instant) -> (Pending, impl Fn() -> Option<Reply>) {
    let (tx, rx) = mpsc::sync_channel(2);
    let take = move || loop {
        match rx.try_recv() {
            Ok(Answer::Done(r)) => return Some(r),
            Ok(Answer::Dispatched) => continue,
            Err(_) => return None,
        }
    };
    (Pending { conn, req, arrived, answer: tx }, take)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    /// A connected pair: the server's reader and the client's stream.
    fn pair() -> (BufReader<TcpStream>, TcpStream) {
        let l = bind(0).unwrap();
        let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (server, _) = l.accept().unwrap();
        (BufReader::new(server), client)
    }

    #[test]
    fn the_listener_is_on_loopback_only() {
        let l = bind(0).unwrap();
        let addr = l.local_addr().unwrap();
        assert_eq!(addr.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        // The port is taken now: a second bind fails (the app exits 2 on that).
        assert!(bind(addr.port()).is_err());
    }

    #[test]
    fn lines_are_framed_by_newlines_and_limited() {
        let (mut r, mut c) = pair();
        c.write_all(b"{\"a\":1}\n\n{\"b\"").unwrap();
        c.write_all(b":2}\r\n").unwrap();
        assert_eq!(read_line(&mut r, 100, None).unwrap().unwrap(), b"{\"a\":1}\n");
        assert_eq!(read_line(&mut r, 100, None).unwrap().unwrap(), b"\n");
        assert_eq!(read_line(&mut r, 100, None).unwrap().unwrap(), b"{\"b\":2}\r\n");
        // Exactly the limit with its newline is fine; one byte more is not, newline or not.
        c.write_all(&[b'x'; 9]).unwrap();
        c.write_all(b"\n").unwrap();
        assert_eq!(read_line(&mut r, 10, None).unwrap().unwrap().len(), 10);
        c.write_all(&[b'x'; 10]).unwrap();
        c.write_all(b"\n").unwrap();
        assert_eq!(read_line(&mut r, 10, None), Err(LineError::TooLong));
        let (mut r, mut c) = pair();
        c.write_all(&[b'x'; 10]).unwrap();
        assert_eq!(read_line(&mut r, 10, None), Err(LineError::TooLong));
        // A last line without a newline, then the end.
        let (mut r, mut c) = pair();
        c.write_all(b"tail").unwrap();
        drop(c);
        assert_eq!(read_line(&mut r, 10, None).unwrap().unwrap(), b"tail");
        assert_eq!(read_line(&mut r, 10, None), Ok(None));
    }

    #[test]
    fn a_huge_line_is_refused_without_reading_it_all() {
        let (mut r, mut c) = pair();
        let writer = std::thread::spawn(move || {
            let chunk = vec![b'x'; 64 << 10];
            // The server stops reading at the limit; the writes after that may fail.
            for _ in 0..64 {
                if c.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
        assert_eq!(read_line(&mut r, MAX_REQUEST_LINE, None), Err(LineError::TooLong));
        drop(r);
        writer.join().unwrap();
    }

    #[test]
    fn a_deadline_ends_a_slow_line() {
        let (mut r, mut c) = pair();
        c.write_all(b"{\"method\"").unwrap();
        let started = Instant::now();
        assert_eq!(read_line(&mut r, 100, Some(Instant::now() + Duration::from_millis(200))), Err(LineError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(read_line(&mut r, 100, Some(Instant::now() - Duration::from_millis(1))), Err(LineError::Timeout));
    }

    #[test]
    fn auth_answers_the_app_or_unauthorized() {
        let token = Token::generate().unwrap();
        let hex = format!("{token:?}");
        assert_eq!(hex, "Token(…)");
        let file = format!("{}\n", "ab".repeat(32));
        let t = Token::from_file_text(file.as_bytes()).unwrap();
        let ok = check_auth(&t, &Request::new("auth", json!({ "token": "ab".repeat(32) }))).unwrap();
        assert_eq!(ok["app"], "dalica");
        assert_eq!(ok["protocol"], PROTOCOL);
        assert_eq!(ok["pid"], std::process::id());
        for params in [json!({ "token": "ac".repeat(32) }), json!({}), json!({ "token": 5 }), json!(null), json!("ab")] {
            let e = check_auth(&t, &Request::new("auth", params.clone())).unwrap_err();
            assert_eq!(e.code, Code::Unauthorized, "{params}");
            assert!(!e.message.contains(&"ab".repeat(32)));
        }
    }

    /// A server on a free port, with its token; requests are answered by `ui` on another thread.
    fn server(ui: impl FnMut(Msg) + Send + 'static) -> (u16, Token) {
        let l = bind(0).unwrap();
        let port = l.local_addr().unwrap().port();
        let token = Token::generate().unwrap();
        let hub = start(l, token.clone(), Arc::new(|| {})).unwrap();
        let mut ui = ui;
        std::thread::spawn(move || {
            while let Ok(m) = hub.rx.recv() {
                ui(m);
            }
        });
        (port, token)
    }

    fn token_hex(t: &Token) -> String {
        t.reveal_for_test().to_string()
    }

    fn client(port: u16) -> (TcpStream, BufReader<TcpStream>) {
        let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let r = BufReader::new(s.try_clone().unwrap());
        (s, r)
    }

    fn send(s: &mut TcpStream, r: &mut BufReader<TcpStream>, line: &str) -> Option<Value> {
        s.write_all(line.as_bytes()).unwrap();
        s.write_all(b"\n").unwrap();
        let mut reply = String::new();
        match r.read_line(&mut reply) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&reply).unwrap()),
        }
    }

    fn closed(r: &mut BufReader<TcpStream>) -> bool {
        let mut rest = Vec::new();
        matches!(r.read_to_end(&mut rest), Ok(0))
    }

    #[test]
    fn a_session_authenticates_then_reaches_the_ui_in_order() {
        let (port, token) = server(|m| {
            if let Msg::Request(p) = m {
                assert!(p.dispatch());
                let echo = json!({ "method": p.req.method, "conn": p.conn });
                p.reply(Ok(echo));
            }
        });
        let (mut s, mut r) = client(port);
        let auth = json!({ "id": "auth", "method": "auth", "params": { "token": token_hex(&token) } }).to_string();
        let a = send(&mut s, &mut r, &auth).unwrap();
        assert_eq!((a["id"].clone(), a["ok"].clone(), a["result"]["app"].clone()), (json!("auth"), json!(true), json!("dalica")));
        for i in 0..3 {
            let v = send(&mut s, &mut r, &json!({ "id": i, "method": format!("m{i}") }).to_string()).unwrap();
            assert_eq!((v["id"].clone(), v["result"]["method"].clone()), (json!(i), json!(format!("m{i}"))));
        }
        // Blank lines are skipped; a second auth with the right token is fine.
        s.write_all(b"\n\r\n").unwrap();
        assert_eq!(send(&mut s, &mut r, &auth).unwrap()["ok"], true);
    }

    #[test]
    fn a_wrong_or_missing_auth_or_junk_closes_the_connection() {
        let (port, token) = server(|m| {
            if let Msg::Request(p) = m {
                p.reply(Ok(json!({})));
            }
        });
        for (line, code) in [
            (json!({ "id": 1, "method": "auth", "params": { "token": "00".repeat(32) } }).to_string(), "unauthorized"),
            (json!({ "id": 1, "method": "ui.state" }).to_string(), "unauthorized"),
            ("GET / HTTP/1.1".to_string(), "parse"),
            ("[1]".to_string(), "parse"),
        ] {
            let (mut s, mut r) = client(port);
            let v = send(&mut s, &mut r, &line).unwrap();
            assert_eq!((v["ok"].clone(), v["code"].clone()), (json!(false), json!(code)), "{line}");
            assert!(closed(&mut r), "{line}");
        }
        // Junk after auth closes too.
        let (mut s, mut r) = client(port);
        send(&mut s, &mut r, &json!({ "method": "auth", "params": { "token": token_hex(&token) } }).to_string()).unwrap();
        let v = send(&mut s, &mut r, "POST / HTTP/1.1").unwrap();
        assert_eq!(v["code"], "parse");
        assert!(closed(&mut r));
    }

    #[test]
    fn the_seventeenth_connection_is_refused_and_a_slot_frees_on_close() {
        let (port, _token) = server(|_| {});
        let mut open: Vec<(TcpStream, BufReader<TcpStream>)> = (0..MAX_CONNECTIONS).map(|_| client(port)).collect();
        // The accept thread has counted them all once a 17th is refused.
        let deadline = Instant::now() + Duration::from_secs(10);
        let refused = loop {
            let (_s, mut r) = client(port);
            let mut line = String::new();
            r.get_ref().set_read_timeout(Some(Duration::from_millis(500))).unwrap();
            if r.read_line(&mut line).is_ok_and(|n| n > 0) {
                break serde_json::from_str::<Value>(&line).unwrap();
            }
            assert!(Instant::now() < deadline, "no limit reply");
        };
        assert_eq!(refused["code"], "limit");
        drop(open.pop());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (mut s, mut r) = client(port);
            r.get_ref().set_read_timeout(Some(Duration::from_millis(300))).unwrap();
            s.write_all(b"\n").unwrap();
            let mut line = String::new();
            match r.read_line(&mut line) {
                // No reply: the connection was accepted and waits for auth.
                Err(_) => break,
                Ok(_) => assert!(Instant::now() < deadline, "the slot was not freed: {line}"),
            }
        }
    }

    #[test]
    fn a_request_queued_past_the_deadline_does_not_run() {
        let (p, take) = pending_for_test(1, Request::new("ui.state", json!({})), Instant::now() - REPLY_DEADLINE - Duration::from_secs(1));
        assert!(!p.dispatch());
        assert_eq!(take().unwrap().unwrap_err().code, Code::Timeout);
        let (p, take) = pending_for_test(1, Request::new("ui.state", json!({})), Instant::now());
        assert!(p.dispatch());
        p.reply(Ok(json!(1)));
        assert_eq!(take().unwrap().unwrap(), json!(1));
    }
}
