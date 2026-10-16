//! `dalica-cli run`: commands on a library from a JSON-lines script (`architecture.md` §3). Each
//! line is `{"command": id, "params": {...}}`; blank lines and lines starting with `#` are skipped.
//! Each command's result is printed as one JSON line: `{"ok": true, "result": …}` or
//! `{"ok": false, "code": …, "error": …}`. Background work (scans) finishes before the next line,
//! and every write is flushed before exiting.
//!
//! **Connect mode** (`mcp.md` §8): `run --connect [HOST:PORT] [--control-token-file PATH]
//! [--script FILE | SCRIPT | -] [--keep-going]` (or `--cmd ID [--params JSON]`) runs the steps in a
//! running `dalica --control` app through the MCP server's own client ([`dalica_mcp::Client`],
//! loopback only, authenticated with the app's token file). A line is `{"command": id, "params"?,
//! "wait"?}` (`engine.execute`, a job it starts waited for) or `{"method": name, "params"?}` (any
//! control method: `ui.*`, `library.inspect`, …). Each step prints `{"ok", "command" | "method",
//! "result" | "code" + "error" (+ "prompt"), "ms"}`. The first failure stops the run with exit
//! status 1; with `--keep-going` every step runs and the status is 1 when any failed.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use dalica_engine::Session;
use dalica_mcp::connect::{Client, Connect, Options};
use dalica_mcp::{Backend, NoProgress};
use serde_json::{Map, Value, json};

/// The longest script line accepted.
const MAX_LINE: usize = 16 << 20;

pub fn main(args: &[String]) -> ExitCode {
    let mut rest = args.to_vec();
    match Options::take(&mut rest) {
        Ok(Some(options)) => return connect(&options, &rest),
        Ok(None) => {}
        Err(e) => {
            eprintln!("dalica-cli run: {e}");
            return ExitCode::from(2);
        }
    }
    let Some(dir) = crate::library_arg(args) else {
        eprintln!("dalica-cli run: --library DIR is required");
        return ExitCode::from(2);
    };
    let single = args.iter().position(|a| a == "--cmd").and_then(|i| args.get(i + 1)).cloned();
    let params = match args.iter().position(|a| a == "--params").and_then(|i| args.get(i + 1)) {
        Some(p) => match serde_json::from_str::<Value>(p) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("dalica-cli run: --params is not JSON: {e}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let script = script_arg(args);
    let mut session = match crate::open(&dir) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let mut out = std::io::stdout().lock();
    let mut failed = false;
    let code = if let Some(id) = single {
        failed |= !step(&mut session, &id, &params.unwrap_or(Value::Null), &mut out);
        None
    } else {
        run_script(&mut session, script, &mut out, &mut failed)
    };
    if let Err(e) = session.flush() {
        let _ = writeln!(out, "{}", json!({ "ok": false, "code": e.code(), "error": e.to_string() }));
        failed = true;
    }
    match code {
        Some(c) => c,
        None if failed => ExitCode::from(1),
        None => ExitCode::SUCCESS,
    }
}

/// The script path: the first argument that is not an option or an option's value (`-` = stdin).
fn script_arg(args: &[String]) -> Option<PathBuf> {
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(a.as_str(), "--library" | "--cmd" | "--params") {
            skip = true;
            continue;
        }
        if a != "-" {
            return Some(PathBuf::from(a));
        }
    }
    None
}

/// Runs every line; returns `Some(2)` at a bad line (after printing why).
fn run_script(s: &mut Session, script: Option<PathBuf>, out: &mut impl Write, failed: &mut bool) -> Option<ExitCode> {
    let reader: Box<dyn BufRead> = match &script {
        Some(path) => match std::fs::File::open(path) {
            Ok(f) => Box::new(std::io::BufReader::new(f)),
            Err(e) => {
                eprintln!("dalica-cli run: cannot read {}: {e}", path.display());
                return Some(ExitCode::from(2));
            }
        },
        None => Box::new(std::io::stdin().lock()),
    };
    for (n, line) in reader.split(b'\n').enumerate() {
        let bad = |why: String| {
            eprintln!("dalica-cli run: line {}: {why}", n + 1);
            Some(ExitCode::from(2))
        };
        let line = match line {
            Ok(l) if l.len() > MAX_LINE => return bad(format!("longer than {MAX_LINE} bytes")),
            Ok(l) => l,
            Err(e) => return bad(e.to_string()),
        };
        let Ok(text) = std::str::from_utf8(&line) else { return bad("not UTF-8".into()) };
        let text = text.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let request: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return bad(format!("not JSON: {e}")),
        };
        let Some(id) = request.get("command").and_then(Value::as_str) else {
            return bad("a line needs {\"command\": id, \"params\": {...}}".into());
        };
        if request.as_object().is_some_and(|o| o.keys().any(|k| k != "command" && k != "params")) {
            return bad("only \"command\" and \"params\" are allowed".into());
        }
        *failed |= !step(s, id, request.get("params").unwrap_or(&Value::Null), out);
    }
    None
}

/// Runs one command, waits for its background work and prints its result line.
fn step(s: &mut Session, id: &str, params: &Value, out: &mut impl Write) -> bool {
    let result = s.execute(id, params);
    s.wait_idle();
    let line = match &result {
        Ok(v) => json!({ "ok": true, "command": id, "result": v }),
        Err(e) => json!({ "ok": false, "command": id, "code": e.code(), "error": e.to_string() }),
    };
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
    result.is_ok()
}

// ---------------------------------------------------------------------------------------------
// Connect mode

/// What `run --connect` was asked to run, besides the connection.
#[derive(Debug, Default, PartialEq)]
struct ConnectArgs {
    script: Option<PathBuf>,
    single: Option<(String, Value)>,
    keep_going: bool,
}

fn connect_args(args: &[String]) -> Result<ConnectArgs, String> {
    let mut out = ConnectArgs::default();
    let (mut cmd, mut params) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--keep-going" => out.keep_going = true,
            "--script" => out.script = Some(PathBuf::from(it.next().ok_or("--script needs a file")?)),
            "--cmd" => cmd = Some(it.next().ok_or("--cmd needs a command id")?.clone()),
            "--params" => {
                let p = it.next().ok_or("--params needs JSON")?;
                params = Some(serde_json::from_str::<Value>(p).map_err(|e| format!("--params is not JSON: {e}"))?);
            }
            "--library" | "--demo" | "--demo-fresh" => return Err(format!("{a} cannot be combined with --connect: the app has its library open")),
            "-" => out.script = None,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => out.script = Some(PathBuf::from(other)),
        }
    }
    match (cmd, params) {
        (Some(id), p) => out.single = Some((id, p.unwrap_or(Value::Null))),
        (None, Some(_)) => return Err("--params goes with --cmd".into()),
        (None, None) => {}
    }
    Ok(out)
}

/// A script line of connect mode: `{"command", "params"?, "wait"?}` or `{"method", "params"?}`.
/// Returns the label key (`command` or `method`), its name, the control method and its params.
fn connect_step(request: &Value) -> Result<(&'static str, String, String, Value), String> {
    let Some(o) = request.as_object() else { return Err("a line must be a JSON object".into()) };
    let params = o.get("params").cloned().unwrap_or(Value::Null);
    if !(params.is_object() || params.is_null()) {
        return Err("\"params\" must be an object".into());
    }
    match (o.get("command"), o.get("method")) {
        (Some(Value::String(id)), None) => {
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "command" | "params" | "wait")) {
                return Err(format!("a command line takes \"command\", \"params\" and \"wait\", not {k:?}"));
            }
            let mut p = Map::new();
            p.insert("command".into(), json!(id));
            if !params.is_null() {
                p.insert("params".into(), params);
            }
            if let Some(w) = o.get("wait") {
                p.insert("wait".into(), w.clone());
            }
            Ok(("command", id.clone(), "engine.execute".into(), Value::Object(p)))
        }
        (None, Some(Value::String(m))) => {
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "method" | "params")) {
                return Err(format!("a method line takes \"method\" and \"params\", not {k:?}"));
            }
            Ok(("method", m.clone(), m.clone(), if params.is_null() { json!({}) } else { params }))
        }
        _ => Err("a line needs a \"command\" id or a \"method\" name (one of them)".into()),
    }
}

/// `run --connect`: the steps sent to the app, one result line each.
fn connect(options: &Options, args: &[String]) -> ExitCode {
    let parsed = match connect_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("dalica-cli run: {e}");
            return ExitCode::from(2);
        }
    };
    let client = match Client::from_options(options) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dalica-cli run: {e}");
            return ExitCode::from(2);
        }
    };
    let mut app = Connect::new(client);
    let mut out = std::io::stdout().lock();
    let mut failed = false;
    if let Some((id, params)) = parsed.single {
        let request = json!({ "command": id, "params": params });
        return match connect_step(&request) {
            Ok(step) => {
                if send(&mut app, step, &mut out) {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => {
                eprintln!("dalica-cli run: {e}");
                ExitCode::from(2)
            }
        };
    }
    let reader: Box<dyn BufRead> = match &parsed.script {
        Some(path) => match std::fs::File::open(path) {
            Ok(f) => Box::new(std::io::BufReader::new(f)),
            Err(e) => {
                eprintln!("dalica-cli run: cannot read {}: {e}", path.display());
                return ExitCode::from(2);
            }
        },
        None => Box::new(std::io::stdin().lock()),
    };
    for (n, line) in reader.split(b'\n').enumerate() {
        let bad = |why: String| {
            eprintln!("dalica-cli run: line {}: {why}", n.saturating_add(1));
            ExitCode::from(2)
        };
        let line = match line {
            Ok(l) if l.len() > MAX_LINE => return bad(format!("longer than {MAX_LINE} bytes")),
            Ok(l) => l,
            Err(e) => return bad(e.to_string()),
        };
        let Ok(text) = std::str::from_utf8(&line) else { return bad("not UTF-8".into()) };
        let text = text.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let step = match serde_json::from_str::<Value>(text).map_err(|e| format!("not JSON: {e}")).and_then(|v| connect_step(&v)) {
            Ok(s) => s,
            Err(e) => return bad(e),
        };
        if !send(&mut app, step, &mut out) {
            failed = true;
            if !parsed.keep_going {
                break;
            }
        }
    }
    if failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

/// Sends one step and prints its result line; whether it succeeded.
fn send(app: &mut Connect, (key, name, method, params): (&'static str, String, String, Value), out: &mut impl Write) -> bool {
    let start = Instant::now();
    let result = app.call(&method, params, &mut NoProgress);
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let line = match &result {
        Ok(v) => json!({ "ok": true, key: name, "result": v, "ms": ms }),
        Err(e) => {
            let mut o = json!({ "ok": false, key: name, "ms": ms });
            if let (Some(o), Value::Object(err)) = (o.as_object_mut(), e.to_json()) {
                o.extend(err);
            }
            o
        }
    };
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
    result.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    /// The script, `--cmd` with `--params`, `--keep-going`; a library or an unknown option is refused.
    #[test]
    fn connect_options_parse() {
        let a = connect_args(&strings(&["--script", "s.jsonl", "--keep-going"])).unwrap();
        assert_eq!(a, ConnectArgs { script: Some("s.jsonl".into()), single: None, keep_going: true });
        let a = connect_args(&strings(&["--cmd", "select.all", "--params", "{\"x\":1}"])).unwrap();
        assert_eq!(a.single, Some(("select.all".to_string(), json!({ "x": 1 }))));
        assert_eq!(connect_args(&strings(&["-"])).unwrap().script, None);
        for bad in [&["--library", "x"][..], &["--demo"], &["--frobnicate"], &["--params", "{}"], &["--cmd"], &["--params", "{"]] {
            assert!(connect_args(&strings(bad)).is_err(), "{bad:?}");
        }
    }

    /// Command lines become `engine.execute`; method lines go as they are; anything else is refused.
    #[test]
    fn connect_script_lines_parse() {
        let (key, name, method, params) = connect_step(&json!({ "command": "photo.star.set", "params": { "on": true }, "wait": false })).unwrap();
        assert_eq!(
            (key, name.as_str(), method.as_str(), params),
            ("command", "photo.star.set", "engine.execute", json!({ "command": "photo.star.set", "params": { "on": true }, "wait": false }))
        );
        let (key, _, method, params) = connect_step(&json!({ "method": "ui.key" })).unwrap();
        assert_eq!((key, method.as_str(), params), ("method", "ui.key", json!({})));
        for bad in [
            json!({ "command": "a", "method": "b" }),
            json!({}),
            json!({ "method": "ui.key", "wait": true }),
            json!({ "command": "a", "x": 1 }),
            json!({ "command": "a", "params": [1] }),
            json!([1]),
        ] {
            assert!(connect_step(&bad).is_err(), "{bad}");
        }
    }
}
