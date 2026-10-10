//! `dalica-cli run`: commands on a library from a JSON-lines script (`architecture.md` §3). Each
//! line is `{"command": id, "params": {...}}`; blank lines and lines starting with `#` are skipped.
//! Each command's result is printed as one JSON line: `{"ok": true, "result": …}` or
//! `{"ok": false, "code": …, "error": …}`. Background work (scans) finishes before the next line,
//! and every write is flushed before exiting.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use dalica_engine::Session;
use serde_json::{Value, json};

/// The longest script line accepted.
const MAX_LINE: usize = 16 << 20;

pub fn main(args: &[String]) -> ExitCode {
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
