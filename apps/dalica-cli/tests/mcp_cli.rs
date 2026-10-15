//! `dalica-cli mcp` on real standard input and output (`mcp.md` §1, §3): it answers `initialize`,
//! writes nothing but protocol messages to standard output, logs to standard error, and deletes
//! its demo on exit.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_demo_answers_initialize_on_stdio_and_logs_to_stderr() {
    let (cache, temp) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_dalica-cli"))
        .args(["mcp", "--demo"])
        .env("DALICA_CACHE_DIR", cache.path())
        .envs(["TMPDIR", "TMP", "TEMP"].map(|k| (k, temp.path())))
        .env("DALICA_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for msg in [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "command_run", "arguments": { "id": "photo.star.set", "params": { "ids": [1], "on": true } } } }),
    ] {
        writeln!(input, "{msg}").unwrap();
    }
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let replies: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON-RPC ({e}): {l}"))).collect();
    assert!(replies.iter().all(|r| r["jsonrpc"] == "2.0"), "{stdout}");
    assert_eq!(replies.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(), [json!(1), json!(2)], "{stdout}");
    assert_eq!(
        (replies[0]["result"]["protocolVersion"].as_str(), replies[0]["result"]["serverInfo"]["name"].as_str()),
        (Some("2025-06-18"), Some("dalica"))
    );
    assert_eq!(replies[1]["result"]["isError"], false, "{stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("dalica-cli mcp: serving headless, the demo library"), "{stderr}");
    assert!(!stdout.contains("dalica-cli mcp:"), "no log line on standard output");
    let left: Vec<_> =
        std::fs::read_dir(temp.path()).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n.to_string_lossy().starts_with("dalica-demo")).collect();
    assert!(left.is_empty(), "the demo is deleted on exit: {left:?}");
}

#[test]
fn mcp_refuses_bad_options() {
    for (args, want) in [
        (&["mcp", "--connect"][..], "M10.17"),
        (&["mcp", "--demo", "--library", "x"], "cannot be combined"),
        (&["mcp", "--frobnicate"], "unknown option"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_dalica-cli")).args(args).stdin(Stdio::null()).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains(want), "{args:?}");
    }
}
