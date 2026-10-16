//! `dalica-cli run --connect` (`mcp.md` §8) against a fake control server: a script prints one
//! JSON line per step, stops with a non-zero status at the first failure, and runs every step with
//! `--keep-going` (still non-zero when one failed).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

const TOKEN: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

/// A control server on an ephemeral loopback port: `auth` first, then `engine.execute` answers
/// `{starred}`, `ui.clickWidget` fails with `not_found`, anything else echoes its method. Returns
/// the address and the methods it was sent.
fn fake_app() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let log = Arc::clone(&log);
            std::thread::spawn(move || {
                let mut out = stream.try_clone().unwrap();
                for (n, line) in BufReader::new(stream).lines().enumerate() {
                    let Ok(line) = line else { return };
                    let r: Value = serde_json::from_str(&line).unwrap();
                    let method = r["method"].as_str().unwrap_or_default().to_string();
                    let reply = if n == 0 {
                        if method != "auth" || r["params"]["token"] != TOKEN {
                            let _ = writeln!(out, "{}", json!({ "id": r["id"], "ok": false, "code": "unauthorized", "error": "auth first" }));
                            return;
                        }
                        json!({ "id": r["id"], "ok": true, "result": { "app": "dalica" } })
                    } else {
                        log.lock().unwrap().push(method.clone());
                        match method.as_str() {
                            "engine.execute" => json!({ "id": r["id"], "ok": true, "result": { "starred": r["params"]["params"]["ids"] } }),
                            "ui.clickWidget" => {
                                json!({ "id": r["id"], "ok": false, "code": "not_found", "error": "no widget `button:nope` on screen" })
                            }
                            _ => json!({ "id": r["id"], "ok": true, "result": { "method": method } }),
                        }
                    };
                    if writeln!(out, "{reply}").is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, seen)
}

/// Runs `dalica-cli run --connect` over `script`; (exit code, the JSON lines printed).
fn run(dir: &std::path::Path, addr: &str, script: &[Value], extra: &[&str]) -> (Option<i32>, Vec<Value>) {
    let token = dir.join("control.token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    let path = dir.join("steps.jsonl");
    let text: String = script.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(&path, format!("# a comment\n\n{text}")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli"))
        .args(["run", "--connect", addr, "--control-token-file"])
        .arg(&token)
        .arg("--script")
        .arg(&path)
        .args(extra)
        .env("DALICA_CACHE_DIR", dir.join("cache"))
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines = stdout.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))).collect();
    (out.status.code(), lines)
}

#[test]
fn a_connect_script_prints_a_line_per_step_and_stops_at_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (addr, seen) = fake_app();
    let star = json!({ "command": "photo.star.set", "params": { "ids": [1], "on": true } });
    let click = json!({ "method": "ui.clickWidget", "params": { "id": "button:nope" } });
    let state = json!({ "method": "ui.state" });

    let (code, lines) = run(dir.path(), &addr, &[star.clone(), state.clone()], &[]);
    assert_eq!(code, Some(0), "{lines:?}");
    assert_eq!(lines.len(), 2);
    assert_eq!((&lines[0]["ok"], &lines[0]["command"], &lines[0]["result"]), (&json!(true), &json!("photo.star.set"), &json!({ "starred": [1] })));
    assert_eq!((&lines[1]["method"], &lines[1]["result"]["method"]), (&json!("ui.state"), &json!("ui.state")));
    assert!(lines.iter().all(|l| l["ms"].is_u64()), "{lines:?}");

    seen.lock().unwrap().clear();
    let (code, lines) = run(dir.path(), &addr, &[star.clone(), click.clone(), state.clone()], &[]);
    assert_eq!(code, Some(1), "{lines:?}");
    assert_eq!(lines.len(), 2, "stopped at the failure: {lines:?}");
    assert_eq!((&lines[1]["ok"], &lines[1]["method"], &lines[1]["code"]), (&json!(false), &json!("ui.clickWidget"), &json!("not_found")));
    assert!(lines[1]["error"].as_str().unwrap().contains("button:nope"));
    assert_eq!(*seen.lock().unwrap(), ["engine.execute", "ui.clickWidget"], "nothing after the failure was sent");

    let (code, lines) = run(dir.path(), &addr, &[star, click, state], &["--keep-going"]);
    assert_eq!(code, Some(1), "a failure still fails the run: {lines:?}");
    assert_eq!(lines.iter().map(|l| l["ok"].clone()).collect::<Vec<_>>(), [json!(true), json!(false), json!(true)]);
}
