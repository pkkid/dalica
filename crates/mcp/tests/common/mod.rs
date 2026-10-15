//! A client that drives a [`Server`] through the real framing: newline-delimited JSON-RPC over two
//! in-process pipes, the server serving on its own thread as it does on stdio. After LightCraft
//! `crates/mcp/tests/e2e.rs` (`session`, `call`) @ b3d392b4, MIT OR Apache-2.0 (`NOTICE`).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dalica_mcp::{Backend, Headless, Server, Source};
use serde_json::{Value, json};

/// How long a test waits for one line from the server before it fails.
const PATIENCE: Duration = Duration::from_secs(120);

pub struct Client {
    to_server: Option<std::io::PipeWriter>,
    lines: Receiver<Value>,
    server: Option<JoinHandle<()>>,
}

impl Client {
    /// Starts a server over the backend `make` builds (on the server's thread).
    pub fn start(make: impl FnOnce() -> Box<dyn Backend> + Send + 'static) -> Client {
        let (server_in, to_server) = std::io::pipe().unwrap();
        let (from_server, server_out) = std::io::pipe().unwrap();
        let server = std::thread::spawn(move || {
            let mut server = Server::new(make());
            server.serve(server_in, server_out).unwrap();
        });
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(from_server).lines() {
                let Ok(line) = line else { return };
                let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("the server wrote a line that is not JSON ({e}): {line}"));
                if tx.send(v).is_err() {
                    return;
                }
            }
        });
        Client { to_server: Some(to_server), lines, server: Some(server) }
    }

    /// A server over a fresh demo library.
    pub fn demo() -> Client {
        Client::start(|| Box::new(Headless::open(Source::Demo, &[]).unwrap()))
    }

    /// A server over an empty library with one folder of `n` procedural photos of `width` ×
    /// `height` added on the command line, scanned before the first request; their ids are 1…n.
    pub fn photos(dir: &Path, n: u64, width: u32, height: u32) -> Client {
        let folder = dir.join("Photos");
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..n {
            let scene = dalica_scenes::Scene::new(i, width, height);
            std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
        }
        Client::start(move || {
            let mut h = Headless::open(Source::Empty, &[folder]).unwrap();
            h.session_mut().wait_idle();
            assert_eq!(h.session().library().photos().count() as u64, n);
            Box::new(h)
        })
    }

    /// Writes `line` and its newline as they are.
    pub fn send_raw(&mut self, line: &[u8]) {
        let w = self.to_server.as_mut().unwrap();
        w.write_all(line).unwrap();
        w.write_all(b"\n").unwrap();
        w.flush().unwrap();
    }

    pub fn send(&mut self, msg: &Value) {
        self.send_raw(msg.to_string().as_bytes());
    }

    /// The next line the server writes.
    pub fn recv(&self) -> Value {
        match self.lines.recv_timeout(PATIENCE) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => panic!("no line from the server in {PATIENCE:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("the server's output closed"),
        }
    }

    /// The next line, if one comes within `wait`.
    pub fn try_recv(&self, wait: Duration) -> Option<Value> {
        self.lines.recv_timeout(wait).ok()
    }

    /// Lines until the response with `id`: (the lines before it, the response).
    pub fn until(&self, id: &Value) -> (Vec<Value>, Value) {
        let mut before = Vec::new();
        let start = Instant::now();
        loop {
            let v = self.recv();
            if v.get("id") == Some(id) && v.get("method").is_none() {
                return (before, v);
            }
            before.push(v);
            assert!(start.elapsed() < PATIENCE, "no response {id} in {PATIENCE:?}");
        }
    }

    /// Sends request `method` and returns its response.
    pub fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.until(&json!(id)).1
    }

    /// Calls tool `name` and returns its result (failing on a protocol error).
    pub fn call(&mut self, id: u64, name: &str, args: Value) -> Value {
        let r = self.request(id, "tools/call", json!({ "name": name, "arguments": args }));
        assert!(r.get("error").is_none(), "{name}: {r}");
        r["result"].clone()
    }

    /// Closes the server's input and waits for it to end.
    pub fn close(mut self) {
        drop(self.to_server.take());
        self.server.take().unwrap().join().unwrap();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        drop(self.to_server.take());
        if let Some(s) = self.server.take() {
            let _ = s.join();
        }
    }
}

/// `initialize` then `notifications/initialized`.
pub fn initialize(c: &mut Client) -> Value {
    let r =
        c.request(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    r
}

/// A tool result's `structuredContent`, asserting it is not an error.
pub fn ok(result: &Value) -> &Value {
    assert_eq!(result["isError"], json!(false), "{result}");
    &result["structuredContent"]
}

/// A tool error's `code`, asserting it is one.
pub fn code(result: &Value) -> &str {
    assert_eq!(result["isError"], json!(true), "{result}");
    result["structuredContent"]["code"].as_str().unwrap()
}

/// Decodes standard base64.
pub fn base64_decode(text: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {c}"),
    };
    let bytes: Vec<u8> = text.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, c)| n | (u32::from(val(*c)) << (18 - 6 * i)));
        out.extend(n.to_be_bytes()[1..chunk.len()].iter());
    }
    out
}
