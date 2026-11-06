//! `dalica-cli` against the running app for M16's acceptance (`m16.rs`): the port the live app
//! (`m11_live.rs`) listens on, read from its log; `dalica-cli run --connect` over the control
//! channel (a JSON-lines script, one result per line); and `dalica-cli mcp --connect` over real
//! standard input and output, as an MCP client drives it (`mcp.md` §8). Every folder the CLI could
//! touch is inside the test's home, as for the app.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// `dalica-cli`, built for this test run.
pub fn cli_exe() -> PathBuf {
    let status = dalica_acceptance::cargo().args(["build", "--quiet", "-p", "dalica-cli"]).status().unwrap();
    assert!(status.success(), "building dalica-cli failed");
    let exe = std::env::current_exe().unwrap();
    exe.parent().and_then(Path::parent).unwrap().join(format!("dalica-cli{}", std::env::consts::EXE_SUFFIX))
}

/// The address the headless app started in `home` listens on, from the line it writes to its log
/// (`dalica: headless, control on 127.0.0.1:PORT, token file …`; the last start's).
pub fn address(home: &Path) -> String {
    let log = std::fs::read_to_string(home.join("stderr.log")).unwrap();
    log.lines()
        .rev()
        .find_map(|l| l.strip_prefix("dalica: headless, control on ").and_then(|r| r.split(',').next()))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("the app's log names no control address:\n{log}"))
}

/// The token file the live app writes (`m11_live.rs`).
pub fn token_file(home: &Path) -> PathBuf {
    home.join("agent.token")
}

/// `cmd` with every folder it could touch inside `home`.
fn contained(cmd: &mut Command, home: &Path) {
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("USERPROFILE", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join("config")),
        ("XDG_CACHE_HOME", home.join("cache")),
        ("DALICA_CACHE_DIR", home.join("dalica-cache")),
        ("XDG_DATA_HOME", home.join("data")),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("APPDATA", home.join("appdata")),
        ("LOCALAPPDATA", home.join("localappdata")),
        ("TMPDIR", tmp.clone()),
        ("TMP", tmp.clone()),
        ("TEMP", tmp),
    ] {
        cmd.env(k, v);
    }
    for k in [
        "DALICA_HOME",
        "DALICA_CONTROL_PORT",
        "DALICA_CONTROL_TOKEN_FILE",
        "DALICA_AUTOMATION_READ_ROOT",
        "DALICA_AUTOMATION_WRITE_ROOT",
        "DALICA_PROFILE",
        "DALICA_DEVICE_ROOTS",
    ] {
        cmd.env_remove(k);
    }
}

/// `dalica-cli run --connect ADDR --control-token-file TOKEN SCRIPT` over the app started in
/// `home`: the steps' result lines, each checked `ok`.
pub fn run(cli: &Path, home: &Path, steps: &[Value]) -> Vec<Value> {
    let script = home.join("tmp").join("steps.jsonl");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, steps.iter().map(|s| format!("{s}\n")).collect::<String>()).unwrap();
    let mut cmd = Command::new(cli);
    cmd.args(["run", "--connect", &address(home), "--control-token-file"]).arg(token_file(home)).arg("--script").arg(&script);
    contained(&mut cmd, home);
    let out = cmd.stdin(Stdio::null()).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "dalica-cli run --connect: {}\n{stdout}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    let lines: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))).collect();
    assert_eq!(lines.len(), steps.len(), "a result per step:\n{stdout}");
    for l in &lines {
        assert_eq!(l["ok"], true, "{l}");
    }
    lines
}

/// `dalica-cli mcp --connect` over the app started in `home`, driven over its standard input and
/// output; killed if the test ends first.
pub struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    next: u64,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Mcp {
    pub fn connect(cli: &Path, home: &Path) -> Mcp {
        let mut cmd = Command::new(cli);
        cmd.args(["mcp", "--connect", &address(home), "--control-token-file"]).arg(token_file(home));
        contained(&mut cmd, home);
        let log = std::fs::File::create(home.join("mcp-stderr.log")).unwrap();
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::from(log)).spawn().unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut m = Mcp { child, input, lines, next: 0 };
        let init = m.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "accept-m16", "version": "0" } }),
        );
        assert_eq!(init["serverInfo"]["name"], "dalica", "{init}");
        m.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        m
    }

    fn send(&mut self, msg: &Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let start = Instant::now();
        loop {
            let left = PATIENCE.checked_sub(start.elapsed()).unwrap_or_default();
            let line = self.lines.recv_timeout(left).unwrap_or_else(|e| panic!("{method} {params}: no reply ({e})"));
            let msg: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC on standard output ({e}): {line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{line}");
            if msg["id"] == json!(id) {
                assert!(msg.get("error").is_none(), "{method} {params}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    /// A tool call that must succeed; its structured answer.
    pub fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    /// The names of the tools the server lists.
    pub fn tools(&mut self) -> Vec<String> {
        let tools = self.request("tools/list", json!({}));
        tools["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str().map(str::to_string)).collect()
    }

    /// Closes its input and waits for it to exit 0.
    pub fn finish(mut self) {
        drop(self.input.take());
        let end = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "dalica-cli mcp exited with {status}");
                return;
            }
            assert!(Instant::now() < end, "dalica-cli mcp did not exit after its input closed");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
