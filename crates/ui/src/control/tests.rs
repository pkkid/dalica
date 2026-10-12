//! The control handlers against the headless driver: every method, every §3 code it can return
//! and every §4 limit it checks (`control-protocol.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Effect, Session};
use dalica_ui_kit::registry;
use serde_json::{Value, json};

use super::*;
use crate::app::App;
use crate::headless::{Driver, KeyState};

/// A running app on a library of one folder of `n` photos (copies of one small JPEG), driven
/// headlessly, with a control channel.
struct T {
    dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The watched folder.
    folder: PathBuf,
    ids: Vec<u64>,
}

/// A new file, through the fs crate.
fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    dalica_fs::replace(path.as_ref(), bytes.as_ref(), dalica_fs::Expect::Absent).unwrap();
}

fn photos(folder: &Path, n: usize) {
    let jpeg = dalica_scenes::encode::jpeg(&dalica_scenes::Scene::new(7, 96, 64), 80, None).unwrap();
    dalica_fs::create_dir_all(folder).unwrap();
    for i in 0..n {
        write(folder.join(format!("IMG_{i:04}.jpg")), &jpeg);
    }
}

fn setup_with(n: usize, config: impl FnOnce(&Path) -> Config) -> T {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("2026-06-01 Trip");
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    if n > 0 {
        photos(&folder, n);
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
    }
    let found = s.execute("library.query", &json!({ "limit": 10000 })).unwrap();
    let ids = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    let control = Control::new(config(dir.path()));
    let mut d = Driver::new(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(3600).unwrap();
    T { dir, control, conn: Connection::new(), d, folder, ids }
}

fn setup(n: usize) -> T {
    setup_with(n, |_| Config::default())
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    match call(t, method, params.clone()) {
        Ok(v) => v,
        Err(e) => panic!("{method} {params}: {e}"),
    }
}

/// The error code of a failed call; panics on success.
fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

/// Fails with `want` and runs no frame.
fn refused(t: &mut T, method: &str, params: Value, want: &str) -> Error {
    let frames = t.d.frames();
    let e = call(t, method, params.clone()).unwrap_err();
    assert_eq!(e.code.as_str(), want, "{method} {params}: {e}");
    assert_eq!(t.d.frames(), frames, "{method} {params}: a refused request runs no frame");
    e
}

fn starred(t: &T, id: u64) -> bool {
    t.d.app().session().unwrap().library().photo(PhotoId::new(id)).unwrap().star
}

fn thumb(id: u64) -> String {
    format!("thumb:{id}")
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    (reader.info().width, reader.info().height)
}

fn unbase64(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {c}"),
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let n = chunk.iter().filter(|&&c| c != b'=').fold(0u32, |n, &c| (n << 6) | u32::from(value(c))) << (6 * pad);
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    out
}

#[test]
fn requests_parse_and_replies_frame_as_the_spec_says() {
    let r = Request::parse(br#"{"id": 7, "method": "ui.state", "params": null}"#).unwrap().unwrap();
    assert_eq!((r.id, r.method.as_str(), r.params), (json!(7), "ui.state", Value::Null));
    let r = Request::parse(b"{\"method\": \"ui.state\"}\r\n").unwrap().unwrap();
    assert_eq!((r.id, r.params), (Value::Null, Value::Null), "id and params may be left out");
    assert_eq!(Request::parse(b"  \n").unwrap(), None, "blank lines are skipped");
    let mut long = br#"{"method": "ui.type", "params": {"text": ""#.to_vec();
    long.resize(MAX_REQUEST_LINE + 1, b'a');
    for bad in [&b"nope"[..], b"[1]", b"{\"id\": 1}", b"{\"method\": 5}", b"\xff\xfe", b"GET / HTTP/1.1", &long] {
        let e = Request::parse(bad).unwrap_err();
        assert_eq!(e.code, Code::Parse, "{:?}", String::from_utf8_lossy(&bad[..bad.len().min(40)]));
        assert!(e.code.closes());
    }
    // Replies.
    assert_eq!(reply_json(&json!("a"), &Ok(json!({ "x": 1 }))), json!({ "id": "a", "ok": true, "result": { "x": 1 } }));
    let mut e = Error::new(Code::NeedsConfirm, "asks first");
    e.prompt = Some(json!({ "text": "Sure?" }));
    assert_eq!(
        reply_json(&json!(2), &Err(e)),
        json!({ "id": 2, "ok": false, "code": "needs_confirm", "error": "asks first", "prompt": { "text": "Sure?" } })
    );
    let line = reply_line(&json!(3), &Ok(json!("x")));
    assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
    let huge = reply_line(&json!(4), &Ok(Value::String("a".repeat(MAX_REPLY_LINE))));
    let v: Value = serde_json::from_str(&huge).unwrap();
    assert_eq!((v["id"].clone(), v["code"].as_str()), (json!(4), Some("limit")), "a reply over 8 MiB is a limit error");
    // Codes.
    let names: std::collections::HashSet<&str> = Code::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), 16);
    assert_eq!(Code::ALL.iter().filter(|c| c.closes()).count(), 2);
    for (e, want) in [
        (CmdError::UnknownCommand("x".into()), "unknown_command"),
        (CmdError::BadParams("x".into()), "bad_params"),
        (CmdError::Disabled("x".into()), "disabled"),
        (CmdError::NotFound("x".into()), "not_found"),
        (CmdError::Limit("x".into()), "limit"),
        (CmdError::Unsaved("disk full".into()), "unsaved"),
        (CmdError::Internal("x".into()), "internal"),
    ] {
        let e = Error::from(e);
        assert_eq!(e.code.as_str(), want);
        if want == "unsaved" {
            assert!(e.message.contains("not written to disk: disk full; Dalica will retry"), "{}", e.message);
        }
    }
    assert_eq!(Error::from(DriverError::NoWidget("button:x".into())).code, Code::NotFound);
    assert_eq!(Error::from(DriverError::NotSettled { frames: 1, busy: vec![] }).code, Code::Timeout);
}

/// A request whose `x` (beside `method`) nests arrays so the line is `depth` deep in all, the
/// request object included.
fn nested(method: &str, depth: usize) -> String {
    let n = depth - 1;
    format!(r#"{{"id": 1, "method": "{method}", "x": {}{}}}"#, "[".repeat(n), "]".repeat(n))
}

/// The `parse` refusal for a line nested too deep; any other outcome fails.
fn too_deep(line: &[u8]) -> Error {
    let e = Request::parse(line).unwrap_err();
    assert_eq!(e.code, Code::Parse, "{e}");
    assert!(e.code.closes());
    assert!(e.message.contains("deeper than 64 levels"), "{e}");
    e
}

#[test]
fn requests_nested_deeper_than_64_levels_are_refused_before_json_parsing() {
    assert_eq!(MAX_REQUEST_DEPTH, 64);
    // Exactly 64 levels is a request; 65 is refused, whatever the parser itself would take (128).
    let r = Request::parse(nested("ui.state", 64).as_bytes()).unwrap().unwrap();
    assert_eq!(r.method, "ui.state");
    for depth in [65, 100, 128, 129, 10_000] {
        too_deep(nested("ui.state", depth).as_bytes());
    }
    // In `params`, with objects, or mixed: the same count.
    let params = |open: &str, close: &str, n: usize| format!(r#"{{"method": "ui.set", "params": {{"a": {}1{}}}}}"#, open.repeat(n), close.repeat(n));
    assert!(Request::parse(params("[", "]", 62).as_bytes()).unwrap().is_some());
    too_deep(params("[", "]", 63).as_bytes());
    assert!(Request::parse(params(r#"{"k": "#, "}", 62).as_bytes()).unwrap().is_some());
    too_deep(params(r#"{"k": "#, "}", 63).as_bytes());
    too_deep(params(r#"[{"k": "#, "}]", 32).as_bytes());
    // Brackets inside strings are text, escaped quotes and backslashes included.
    let text = format!(r#"{}\"{}\\{}\"\\\"{}"#, "[".repeat(1000), "{".repeat(1000), "[{".repeat(500), "]}".repeat(10));
    let r = Request::parse(format!(r#"{{"method": "ui.type", "params": {{"text": "{text}"}}}}"#).as_bytes()).unwrap().unwrap();
    assert_eq!(r.params["text"].as_str().map(|s| s.matches('[').count()), Some(1500));
    assert_eq!(r.params["text"].as_str().map(|s| s.matches('"').count()), Some(3));
    // An escaped backslash ends with the string's real closing quote: what follows is counted.
    let after_backslash = |n: usize| format!(r#"{{"method": "m", "t": "a\\", "x": {}{}}}"#, "[".repeat(n), "]".repeat(n));
    assert!(Request::parse(after_backslash(63).as_bytes()).unwrap().is_some());
    too_deep(after_backslash(64).as_bytes());
    // A string with an escaped quote does not end there.
    let line = format!(r#"{{"method": "m", "t": "\" {} "}}"#, "[".repeat(100));
    assert!(Request::parse(line.as_bytes()).unwrap().is_some());
    // Closing more than was opened never wraps the count; the parser refuses the junk.
    assert_eq!(
        Request::parse(format!("]]]]{}", nested("m", 2)).as_bytes()).unwrap_err().message,
        "not a request: the line is not JSON; send one JSON object per line"
    );
    assert!(!nests_deeper_than(format!("{}{}", "]".repeat(100), "[".repeat(64)).as_bytes(), 64));
    assert!(nests_deeper_than(format!("{}{}", "]".repeat(100), "[".repeat(65)).as_bytes(), 64));
}

#[test]
fn a_one_mib_line_of_open_brackets_is_refused_fast_without_recursion() {
    // At the size limit (one byte more is the line-size refusal), all `[`; then 64 levels of
    // padding with the 65th level at the very end, the scan's worst case.
    let all_open = vec![b'['; MAX_REQUEST_LINE];
    let mut late = b"{\"method\": \"m\", \"x\": ".to_vec();
    late.extend(std::iter::repeat_n(b'[', 63));
    let tail = b"[]";
    while late.len() + 2 + tail.len() <= MAX_REQUEST_LINE {
        late.extend_from_slice(b"0,");
    }
    late.extend_from_slice(tail);
    assert!(late.len() <= MAX_REQUEST_LINE);
    for line in [&all_open, &late] {
        let start = std::time::Instant::now();
        too_deep(line);
        let took = start.elapsed();
        eprintln!("refused a {}-byte line nested too deep in {took:?}", line.len());
        assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    }
}

/// The transport's path for one line: parse, and only a request reaches a handler.
fn line(t: &mut T, line: &str) -> Reply {
    match Request::parse(line.as_bytes())? {
        Some(req) => serve(&t.control, &mut t.conn, &mut t.d, &req),
        None => panic!("a blank line"),
    }
}

#[test]
fn a_line_nested_too_deep_reaches_no_handler_and_runs_no_frame() {
    let mut t = setup(1);
    let a = t.ids[0];
    let star = |depth: usize| {
        let n = depth - 1;
        format!(
            r#"{{"id": 1, "method": "engine.execute", "params": {{"command": "photo.star.toggle", "params": {{"ids": [{a}]}}}}, "x": {}{}}}"#,
            "[".repeat(n),
            "]".repeat(n)
        )
    };
    let frames = t.d.frames();
    let e = line(&mut t, &star(65)).unwrap_err();
    assert_eq!(e.code, Code::Parse, "{e}");
    assert_eq!(t.d.frames(), frames, "a refused line runs no frame");
    assert!(!starred(&t, a), "a refused line runs no command");
    assert!(t.d.app().session().unwrap().undo_stack().is_empty());
    // The same line 64 deep is an ordinary request.
    line(&mut t, &star(64)).unwrap();
    assert!(starred(&t, a));
}

#[test]
fn methods_are_listed_unknown_ones_refused_unbuilt_ones_unsupported_and_panics_caught() {
    let mut t = setup(0);
    let list = ok(&mut t, "methods", json!({}));
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|m| m["method"].as_str().unwrap()).collect();
    for m in [
        "engine.execute",
        "engine.commands",
        "ui.state",
        "ui.set",
        "ui.widgets",
        "ui.clickWidget",
        "ui.pointer",
        "ui.key",
        "ui.type",
        "ui.screenshot",
        "ui.inspect",
        "ui.settle",
        "ui.wait",
        "ui.filePicker",
        "ui.menu.tree",
        "batch",
        "jobs.list",
    ] {
        assert!(names.contains(&m), "{m}");
    }
    assert!(!names.contains(&"auth"), "auth is the transport's");
    for m in ["ui.dialog.confirm", "ui.dialog.cancel", "jobs.list", "jobs.wait", "jobs.cancel"] {
        let entry = list.as_array().unwrap().iter().find(|e| e["method"] == m).unwrap();
        assert_eq!((entry["built"].as_bool(), entry.get("planned")), (Some(true), None), "{m}");
    }
    for m in ["batch", "library.inspect", "app.quit"] {
        refused(&mut t, m, json!({}), "unsupported");
    }
    let e = refused(&mut t, "ui.clickwidget", json!({}), "unknown_method");
    assert!(e.message.contains("methods"), "{}", e.message);
    refused(&mut t, "auth", json!({ "token": "x" }), "unknown_method");
    // The dispatch guard: a panic in a handler, a continuation or a worker is `internal`.
    let e = run(&mut t.d, guard(|| panic!("boom"))).unwrap_err();
    assert!(e.code == Code::Internal && e.message.contains("boom"), "{e}");
    let after_play = Outcome::Play(Input::Frames(1), Box::new(|_, _| panic!("late")));
    assert_eq!(run(&mut t.d, after_play).unwrap_err().code, Code::Internal);
    let in_work = Outcome::Work(Box::new(|| panic!("worker")));
    assert_eq!(run(&mut t.d, in_work).unwrap_err().code, Code::Internal);
    let resumed = Outcome::Work(Box::new(|| Next::Resume(Box::new(|_| panic!("resumed")))));
    assert_eq!(run(&mut t.d, resumed).unwrap_err().code, Code::Internal);
    // The app keeps running.
    ok(&mut t, "ui.state", json!({}));
}

/// Every built method refuses params of the wrong shape (and unknown keys) with `bad_params`,
/// changes nothing and runs no frame.
#[test]
fn every_method_refuses_hostile_params_and_changes_nothing() {
    let mut t = setup(3);
    let before = t.d.app().state().clone();
    let built: Vec<&str> = METHODS.iter().filter(|m| m.run.is_some()).map(|m| m.name).collect();
    assert_eq!(built.len(), 26, "{built:?}");
    for m in &built {
        for params in [json!([1]), json!("x"), json!(7), json!(true), json!({ "zzz": 1 })] {
            let e = refused(&mut t, m, params.clone(), "bad_params");
            if params.is_object() {
                assert!(e.message.contains("zzz") && e.message.contains(m), "{m}: {}", e.message);
            }
        }
    }
    assert_eq!(t.d.app().state(), &before);
    assert!(t.ids.iter().all(|&id| !starred(&t, id)));
}

#[test]
fn engine_execute_runs_commands_and_returns_every_code_it_can() {
    let mut t = setup(3);
    let (a, b) = (t.ids[0], t.ids[1]);
    let r = ok(&mut t, "engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": [a] } }));
    assert!(starred(&t, a), "{r}");
    assert_eq!(t.d.app().session().unwrap().undo_stack().len(), 1, "one call, one undo step");
    // UI_COMMANDS run here too; `wait` is accepted.
    let r = ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [b] }, "wait": true }));
    assert_eq!(r, json!({ "selection": [b] }));
    let r = ok(&mut t, "engine.execute", json!({ "command": "select.none" }));
    assert_eq!(r, json!({ "selection": [] }));
    // The registered commands' own refusals.
    refused(&mut t, "engine.execute", json!({ "command": "no.such" }), "unknown_command");
    for bad in [
        json!({}),
        json!({ "command": 5 }),
        json!({ "command": "photo.star.toggle", "params": "x" }),
        json!({ "command": "photo.star.toggle", "params": [1] }),
        json!({ "command": "photo.star.toggle", "wait": "yes" }),
        json!({ "command": "photo.star.toggle", "extra": 1 }),
    ] {
        refused(&mut t, "engine.execute", bad, "bad_params");
    }
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": "1" } })), "bad_params");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": [999_999] } })), "not_found");
    let e = refused(&mut t, "engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": [] } }), "disabled");
    assert!(!e.message.is_empty());
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "library.query", "params": { "limit": 20_000 } })), "limit");
    assert_eq!(t.d.app().session().unwrap().undo_stack().len(), 1, "no refusal added an undo step");
    // A destructive command asks first (§5.5).
    let e = refused(&mut t, "engine.execute", json!({ "command": "library.rebuild" }), "needs_confirm");
    let prompt = e.prompt.unwrap();
    assert!(prompt["text"].as_str().unwrap().contains("Rebuild") && prompt["buttons"].as_array().unwrap().len() == 2, "{prompt}");
    refused(&mut t, "engine.execute", json!({ "command": "library.rebuild", "params": { "confirm": false } }), "needs_confirm");
    refused(&mut t, "engine.execute", json!({ "command": "library.rebuild", "params": { "confirm": "yes" } }), "bad_params");
    ok(&mut t, "engine.execute", json!({ "command": "library.rebuild", "params": { "confirm": true } }));
    t.d.app_mut().session_mut().unwrap().wait_idle();
    // Paths read by commands follow §5.7: no read root, no demo → refused; `..` refused; a
    // relative path is the command's own bad_params; a missing folder is not_found.
    let other = t.dir.path().join("other");
    photos(&other, 1);
    let e = refused(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": other } }), "path_denied");
    assert!(e.message.contains("--automation-read-root"), "{}", e.message);
    let climbing = t.folder.join("..").join("other");
    refused(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": climbing } }), "path_denied");
    refused(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": "relative/dir" } }), "bad_params");
    let missing = t.dir.path().join("missing");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": missing } })), "not_found");
    // The watched folder itself may be added again (a rescan).
    let watched = t.folder.clone();
    let again = ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": watched } }));
    assert_eq!(again["new"], false);
    assert_eq!(t.d.app().session().unwrap().roots().len(), 1, "nothing outside the roots was added");
    // Effects outside Dalica (no such command is registered yet; the rule is checked directly).
    assert_eq!(engine::external_allowed(Some(Effect::External), false, "x").unwrap_err().code, Code::ExternalDenied);
    assert!(engine::external_allowed(Some(Effect::External), true, "x").is_ok());
    assert!(engine::external_allowed(Some(Effect::Library), false, "x").is_ok());
    assert!(engine::external_allowed(None, false, "x").is_ok());
}

#[test]
fn engine_execute_with_a_read_root_or_the_demo_adds_folders_and_waits_for_a_library() {
    let mut t = setup_with(1, |dir| Config { read_roots: vec![dir.join("extra")], ..Config::default() });
    let inside = t.dir.path().join("extra").join("2026-07 More");
    photos(&inside, 2);
    let r = ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": inside } }));
    assert_eq!(r["new"], true);
    let outside = t.dir.path().join("elsewhere");
    photos(&outside, 1);
    refused(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": outside } }), "path_denied");
    let mut demo = setup_with(0, |_| Config { demo: true, ..Config::default() });
    let folder = demo.dir.path().join("anywhere");
    photos(&folder, 1);
    assert_eq!(ok(&mut demo, "engine.execute", json!({ "command": "folders.add", "params": { "path": folder } }))["new"], true);
    // An empty library: rebuilding is disabled (checked before it asks to confirm).
    let mut empty = setup(0);
    let e = refused(&mut empty, "engine.execute", json!({ "command": "library.rebuild" }), "disabled");
    assert_eq!(e.message, "Add a folder first.");
    // No library open yet: engine commands are disabled, unknown ones unknown.
    let control = Control::new(Config::default());
    let mut d = Driver::new(App::new(None), 320, 200, 1.0);
    let mut conn = Connection::new();
    let mut c = |m: &str, p: Value| serve(&control, &mut conn, &mut d, &Request::new(m, p));
    assert_eq!(c("engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": [1] } })).unwrap_err().code, Code::Disabled);
    assert_eq!(c("engine.execute", json!({ "command": "nope" })).unwrap_err().code, Code::UnknownCommand);
    let all = c("engine.commands", json!({})).unwrap();
    assert!(all.as_array().unwrap().iter().any(|c| c["id"] == "photo.star.toggle" && c["enabled"] == false));
}

#[test]
fn engine_commands_lists_engine_and_ui_commands_with_effective_shortcuts() {
    let mut t = setup(1);
    let all = ok(&mut t, "engine.commands", json!({}));
    let all = all.as_array().unwrap();
    let star = all.iter().find(|c| c["id"] == "photo.star.toggle").unwrap();
    let want = if cfg!(target_os = "macos") { "Cmd+8" } else { "Ctrl+8" };
    assert_eq!((star["shortcut"].as_str(), star["ui"].as_bool(), star["live"].as_bool()), (Some(want), Some(false), Some(true)));
    assert_eq!(star["menu"], "Picture/Star / Unstar");
    assert!(all.iter().any(|c| c["id"] == "select.set" && c["ui"] == true));
    let found = ok(&mut t, "engine.commands", json!({ "filter": "STAR" }));
    assert!(found.as_array().unwrap().iter().all(|c| c.to_string().to_lowercase().contains("star")));
    assert!(found.as_array().unwrap().len() >= 2);
    let enabled = ok(&mut t, "engine.commands", json!({ "enabledOnly": true }));
    assert!(enabled.as_array().unwrap().iter().all(|c| c["enabled"] == true));
    assert!(enabled.as_array().unwrap().len() < all.len(), "photo.star.toggle needs ids, so it is disabled for {{}}");
    // A rebinding shows up.
    let rebind = json!({ "command": "app.setShortcut", "params": { "command": "photo.star.toggle", "context": "library", "key": "Ctrl+9" } });
    ok(&mut t, "engine.execute", rebind);
    let star = ok(&mut t, "engine.commands", json!({ "filter": "photo.star.toggle" }));
    assert_eq!(star[0]["shortcut"].as_str().map(|s| s.ends_with("+9")), Some(true), "{star}");
    for bad in [json!({ "filter": 5 }), json!({ "enabledOnly": "yes" })] {
        refused(&mut t, "engine.commands", bad, "bad_params");
    }
}

#[test]
fn ui_state_and_ui_set_check_every_field_before_changing_any() {
    let mut t = setup(2);
    let state = ok(&mut t, "ui.state", json!(null));
    assert_eq!(state, serde_json::to_value(t.d.app().state()).unwrap());
    assert_eq!(state["thumbSize"], 144.0);
    let r = ok(&mut t, "ui.set", json!({ "thumbSize": 200 }));
    assert_eq!((r["thumbSize"].as_f64(), t.d.app().state().thumb_size), (Some(200.0), 200.0));
    let folder = t.d.app().session().unwrap().library().folders().next().unwrap().id;
    assert_eq!(ok(&mut t, "ui.set", json!({ "folder": folder.get() }))["folder"], json!(folder.get()));
    let before = t.d.app().state().clone();
    for bad in [
        json!({ "thumbSize": 10 }),
        json!({ "thumbSize": 1000, "view": "library" }),
        json!({ "thumbSize": "big" }),
        json!({ "view": "edit" }),
        json!({ "selection": { "ids": [], "anchor": null } }),
        json!({ "status": "hi" }),
        json!({ "thumbSize": 160, "zoom": 2 }),
        json!({ "folder": "one" }),
    ] {
        refused(&mut t, "ui.set", bad, "bad_params");
    }
    refused(&mut t, "ui.set", json!({ "thumbSize": 160, "folder": 999_999 }), "not_found");
    assert_eq!(t.d.app().state(), &before, "nothing changed");
}

#[test]
fn ui_inspect_reports_the_window_hover_the_picker_and_perf() {
    let mut t = setup(3);
    let v = ok(&mut t, "ui.inspect", json!({}));
    let window = json!({ "width": 1400.0, "height": 900.0, "pixelsPerPoint": 1.0, "focused": true, "drawing": true, "headless": true });
    assert_eq!(v["window"], window);
    assert_eq!(v["thumbSize"], 144.0, "the view state is in it");
    assert!(v["perf"]["frames"].as_u64().unwrap() > 0 && v["perf"]["cellsVisited"].as_u64().unwrap() == 3, "{}", v["perf"]);
    assert_eq!((v["filePicker"].clone(), v["hovered"].clone(), v["libraryOpen"].clone()), (Value::Null, Value::Null, json!(true)));
    let cell = thumb(t.ids[1]);
    ok(&mut t, "ui.hoverWidget", json!({ "id": cell }));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["hovered"], json!(cell));
    let max = ok(&mut t, "ui.inspect", json!({ "resetPerf": true }))["perf"]["maxFrameMs"].as_f64().unwrap();
    let after = ok(&mut t, "ui.inspect", json!({}))["perf"]["maxFrameMs"].as_f64().unwrap();
    assert!(after <= max, "{after} > {max}");
    refused(&mut t, "ui.inspect", json!({ "resetPerf": 1 }), "bad_params");
}

#[test]
fn ui_widgets_lists_the_last_frame_filtered() {
    let mut t = setup(3);
    let all = ok(&mut t, "ui.widgets", json!({}));
    let cell = all.as_array().unwrap().iter().find(|w| w["id"] == thumb(t.ids[0])).unwrap().clone();
    assert_eq!((cell["role"].as_str(), cell["checked"].as_bool(), cell["enabled"].as_bool()), (Some("thumbnail"), Some(false), Some(true)));
    assert_eq!(cell["rect"].as_array().unwrap().len(), 4);
    assert!(all.as_array().unwrap().iter().any(|w| w["id"] == "menu:File" && w.get("checked").is_none() && w["label"] == "File"));
    let first = thumb(t.ids[0]);
    ok(&mut t, "ui.clickWidget", json!({ "id": first }));
    let thumbs = ok(&mut t, "ui.widgets", json!({ "filter": "thumb:" }));
    assert!(thumbs.as_array().unwrap().iter().all(|w| w["id"].as_str().unwrap().contains("thumb:")));
    assert!(thumbs.as_array().unwrap().iter().any(|w| w["id"] == thumb(t.ids[0]) && w["checked"] == true), "the click shows at once (§5.2)");
    assert_eq!(ok(&mut t, "ui.widgets", json!({ "filter": "nothing-has-this" })), json!([]));
    assert!(ok(&mut t, "ui.widgets", json!({ "region": "lightTable" })).as_array().unwrap().iter().any(|w| w["id"] == thumb(t.ids[0])));
    refused(&mut t, "ui.widgets", json!({ "region": "attic" }), "bad_params");
    refused(&mut t, "ui.widgets", json!({ "region": 5 }), "bad_params");
}

#[test]
fn ui_click_widget_clicks_with_counts_buttons_points_and_modifiers() {
    let mut t = setup(4);
    let ids = t.ids.clone();
    let r = ok(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[0]) }));
    assert_eq!(r["rect"].as_array().unwrap().len(), 4);
    assert_eq!(t.d.app().state().selection.ids, [PhotoId::new(ids[0])], "the reply comes after the click played out");
    ok(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[2]), "ctrl": true, "fx": 0.1, "fy": 0.9 }));
    assert_eq!(t.d.app().state().selection.ids.len(), 2, "Ctrl toggles");
    ok(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[3]), "count": 2 }));
    assert_eq!(t.d.app().state().selection.ids, [PhotoId::new(ids[3])]);
    ok(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[1]), "count": 8 }));
    assert_eq!(t.d.app().state().selection.ids, [PhotoId::new(ids[1])], "eight presses, raw");
    ok(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[2]), "button": "right" }));
    assert_eq!(t.d.app().state().selection.ids, [PhotoId::new(ids[2])], "a right click on an unselected photo selects it first");
    // Limits, then params, then the widget.
    for count in [0, 9, 1_000_000] {
        refused(&mut t, "ui.clickWidget", json!({ "id": thumb(ids[0]), "count": count }), "limit");
    }
    for bad in [
        json!({ "id": thumb(ids[0]), "fx": 1.5 }),
        json!({ "id": thumb(ids[0]), "fy": -0.1 }),
        json!({ "id": thumb(ids[0]), "button": "thumb" }),
        json!({ "id": thumb(ids[0]), "count": -1 }),
        json!({ "id": thumb(ids[0]), "count": 1.5 }),
        json!({ "id": 5 }),
        json!({}),
    ] {
        refused(&mut t, "ui.clickWidget", bad, "bad_params");
    }
    refused(&mut t, "ui.clickWidget", json!({ "id": "button:nope" }), "not_found");
    ok(&mut t, "engine.execute", json!({ "command": "select.none" }));
    let e = refused(&mut t, "ui.clickWidget", json!({ "id": "button:star" }), "disabled");
    assert!(e.message.contains("button:star"), "{}", e.message);
    assert_eq!(t.d.app().state().selection.ids.len(), 0);
}

#[test]
fn ui_key_presses_combos_and_named_keys_and_holds_them() {
    let mut t = setup(2);
    let a = t.ids[0];
    ok(&mut t, "ui.clickWidget", json!({ "id": thumb(a) }));
    assert_eq!(ok(&mut t, "ui.key", json!({ "combo": "Ctrl+8" })), json!({}));
    assert!(starred(&t, a), "the reply comes after the key played out");
    ok(&mut t, "ui.key", json!({ "key": "F4" }));
    assert!(!starred(&t, a), "F4 toggles back");
    ok(&mut t, "ui.key", json!({ "key": "8", "ctrl": true }));
    assert!(starred(&t, a), "the ctrl flag is the command modifier, as in combos");
    for alias in ["esc", "return", "left", "space", "Escape", "ArrowLeft", "Backslash"] {
        ok(&mut t, "ui.key", json!({ "key": alias }));
    }
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+Alt+Space", "state": "down" }));
    assert_eq!(t.conn.held(), [egui::Key::Space]);
    assert!(t.d.ctx().input(|i| i.key_down(egui::Key::Space) && i.modifiers.alt));
    ok(&mut t, "ui.key", json!({ "combo": "Space", "state": "up" }));
    assert!(t.conn.held().is_empty() && !t.d.ctx().input(|i| i.key_down(egui::Key::Space)));
    ok(&mut t, "ui.key", json!({ "key": "Z", "state": "down", "shift": true }));
    let released = t.conn.release();
    assert!(released.iter().any(|i| matches!(i, Input::Key { key: egui::Key::Z, state: KeyState::Up, .. })), "{released:?}");
    for input in &released {
        t.d.play(input).unwrap();
    }
    assert!(!t.d.ctx().input(|i| i.key_down(egui::Key::Z)), "closing the connection lets go of held keys");
    for bad in [
        json!({ "key": "Hyper" }),
        json!({ "combo": "Hyper+X" }),
        json!({ "combo": "Ctrl+" }),
        json!({ "key": "A", "combo": "A" }),
        json!({}),
        json!({ "combo": "Ctrl+8", "shift": true }),
        json!({ "key": "A", "state": "sideways" }),
        json!({ "key": 8 }),
    ] {
        refused(&mut t, "ui.key", bad, "bad_params");
    }
}

#[test]
fn ui_type_types_text_within_its_limit() {
    let mut t = setup(1);
    assert_eq!(ok(&mut t, "ui.type", json!({ "text": "Lisbon 2026 ✓\n" })), json!({}));
    assert!(t.d.app().state().status.is_empty(), "with nothing focused the text goes nowhere");
    refused(&mut t, "ui.type", json!({ "text": "a".repeat(MAX_TEXT_BYTES + 1) }), "limit");
    ok(&mut t, "ui.type", json!({ "text": "a".repeat(MAX_TEXT_BYTES) }));
    for bad in [json!({ "text": "bell\u{7}" }), json!({ "text": 5 }), json!({})] {
        refused(&mut t, "ui.type", bad, "bad_params");
    }
}

#[test]
fn ui_pointer_plays_raw_gestures() {
    let mut t = setup(2);
    let cell = registry::find(t.d.ctx(), &thumb(t.ids[1])).unwrap().center();
    let (x, y) = (cell.x, cell.y);
    let gesture = json!({ "events": [{ "kind": "move", "x": x, "y": y }, { "kind": "down", "x": x, "y": y }, { "kind": "up", "x": x, "y": y }] });
    assert_eq!(ok(&mut t, "ui.pointer", gesture), json!({ "events": 3 }));
    assert_eq!(t.d.app().state().selection.ids, [PhotoId::new(t.ids[1])], "a press and release is a click");
    let many: Vec<Value> = (0..=MAX_POINTER_EVENTS).map(|_| json!({ "kind": "move", "x": 1, "y": 1 })).collect();
    refused(&mut t, "ui.pointer", json!({ "events": many }), "limit");
    refused(&mut t, "ui.pointer", json!({ "events": [], "space": "image" }), "unsupported");
    for bad in [
        json!({ "events": [{ "kind": "jump", "x": 1, "y": 1 }] }),
        json!({ "events": [{ "kind": "move", "x": 2.0e6, "y": 1 }] }),
        json!({ "events": [{ "kind": "move", "x": 1 }] }),
        json!({ "events": [], "space": "world" }),
        json!({ "events": [], "button": "fourth" }),
        json!({ "events": "all" }),
        json!({}),
    ] {
        refused(&mut t, "ui.pointer", bad, "bad_params");
    }
}

#[test]
fn drag_hover_scroll_resize_wait_and_settle() {
    let mut t = setup(120);
    let low = registry::widgets(t.d.ctx()).into_iter().find(|w| w.id.starts_with("thumb:") && w.rect[1] > 500.0).unwrap();
    // Scroll by the wheel over a widget.
    assert_eq!(ok(&mut t, "ui.scroll", json!({ "dy": -300, "widget": low.id })), json!({}));
    let moved = registry::find(t.d.ctx(), &low.id).unwrap();
    assert_eq!(low.rect[1] - moved.rect[1], 300.0);
    ok(&mut t, "ui.scroll", json!({ "dy": 300 }));
    refused(&mut t, "ui.scroll", json!({ "dy": 1, "widget": "thumb:0" }), "not_found");
    refused(&mut t, "ui.scroll", json!({ "dy": 2.0e6 }), "bad_params");
    refused(&mut t, "ui.scroll", json!({ "dy": 1, "ctrl": true }), "unsupported");
    // Drag the speed thumb by an offset (the content scrolls down while it is held, and it springs
    // back to the centre), then back onto the track's top, from a point inside it.
    let handle = registry::find(t.d.ctx(), "scroll:thumb").unwrap();
    let r = ok(&mut t, "ui.dragWidget", json!({ "id": "scroll:thumb", "dx": 0, "dy": 40 }));
    assert_eq!(r["from"], json!([handle.center().x, handle.center().y]));
    let scrolled = registry::find(t.d.ctx(), &low.id).unwrap();
    assert!(scrolled.rect[1] < low.rect[1], "the content moved up: {:?} → {:?}", low.rect, scrolled.rect);
    ok(&mut t, "ui.wait", json!({ "ms": 150 }));
    assert_eq!(registry::find(t.d.ctx(), "scroll:thumb").unwrap().rect, handle.rect, "the thumb is back at the centre");
    let track = registry::find(t.d.ctx(), "scroll:track").unwrap();
    let back = json!({ "id": "scroll:thumb", "fy": 0.2, "toX": track.rect[0] + 2.0, "toY": track.rect[1] - 100.0, "steps": 1000 });
    ok(&mut t, "ui.dragWidget", back);
    assert_eq!(registry::find(t.d.ctx(), &low.id).unwrap().rect, low.rect, "back at the top");
    ok(&mut t, "ui.dragWidget", json!({ "id": low.id, "toWidget": "scroll:track", "steps": 1 }));
    for steps in [0, 1001] {
        refused(&mut t, "ui.dragWidget", json!({ "id": "scroll:thumb", "dx": 0, "dy": 1, "steps": steps }), "limit");
    }
    for bad in [
        json!({ "id": "scroll:thumb" }),
        json!({ "id": "scroll:thumb", "dx": 1 }),
        json!({ "id": "scroll:thumb", "dx": 1, "dy": 1, "toWidget": "scroll:track" }),
        json!({ "id": "scroll:thumb", "toX": 1.0e7, "toY": 1 }),
        json!({ "id": "scroll:thumb", "dx": 1, "dy": 1, "fx": 2 }),
    ] {
        refused(&mut t, "ui.dragWidget", bad, "bad_params");
    }
    refused(&mut t, "ui.dragWidget", json!({ "id": "nope:x", "dx": 1, "dy": 1 }), "not_found");
    refused(&mut t, "ui.dragWidget", json!({ "id": "scroll:thumb", "toWidget": "nope:x" }), "not_found");
    // Hover, at a point and for a while.
    let frames = t.d.frames();
    ok(&mut t, "ui.hoverWidget", json!({ "id": low.id, "ms": 650 }));
    assert_eq!(t.d.frames() - frames, 40, "650 ms is 39 frames after the move");
    ok(&mut t, "ui.hoverWidget", json!({ "id": low.id, "fx": 0.1, "fy": 0.1, "ms": 0 }));
    refused(&mut t, "ui.hoverWidget", json!({ "id": low.id, "ms": MAX_WAIT_MS + 1 }), "limit");
    refused(&mut t, "ui.hoverWidget", json!({ "id": "nope:x" }), "not_found");
    refused(&mut t, "ui.hoverWidget", json!({ "id": low.id, "fx": 1.1 }), "bad_params");
    // Resize, in points.
    assert_eq!(ok(&mut t, "ui.resize", json!({ "width": 900, "height": 600 })), json!({ "width": 900.0, "height": 600.0 }));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["window"]["width"], 900.0);
    for bad in [json!({ "width": 0, "height": 600 }), json!({ "width": 9000, "height": 600 }), json!({ "width": 8000, "height": 8000 })] {
        refused(&mut t, "ui.resize", bad, "limit");
    }
    refused(&mut t, "ui.resize", json!({ "width": "wide", "height": 600 }), "bad_params");
    refused(&mut t, "ui.resize", json!({ "width": 900 }), "bad_params");
    // Wait and settle.
    assert_eq!(ok(&mut t, "ui.wait", json!({ "ms": 100 })), json!({ "frames": 6 }));
    refused(&mut t, "ui.wait", json!({ "ms": MAX_WAIT_MS + 1 }), "limit");
    for bad in [json!({}), json!({ "ms": -1 }), json!({ "ms": "soon" })] {
        refused(&mut t, "ui.wait", bad, "bad_params");
    }
    let s = ok(&mut t, "ui.settle", json!({}));
    assert_eq!((s["settled"].as_bool(), s["busy"].clone()), (Some(true), json!([])), "{s}");
    assert_eq!(s["frames"].as_u64(), Some(crate::headless::QUIET_FRAMES as u64));
    let s = ok(&mut t, "ui.settle", json!({ "timeoutMs": 0, "jobs": true }));
    assert_eq!((s["settled"].as_bool(), s["frames"].as_u64()), (Some(false), Some(0)));
    refused(&mut t, "ui.settle", json!({ "timeoutMs": MAX_WAIT_MS + 1 }), "limit");
    refused(&mut t, "ui.settle", json!({ "jobs": "yes" }), "bad_params");
}

#[test]
fn ui_settle_says_what_is_busy() {
    let control = Control::new(Config::default());
    let mut d = Driver::new(App::new(None), 320, 200, 1.0);
    let s = serve(&control, &mut Connection::new(), &mut d, &Request::new("ui.settle", json!({ "timeoutMs": 100 }))).unwrap();
    assert_eq!((s["settled"].as_bool(), s["frames"].as_u64(), s["busy"].clone()), (Some(false), Some(6), json!(["library: opening"])));
    assert!((s["ms"].as_f64().unwrap() - 100.0).abs() < 1e-6, "{s}");
}

#[test]
fn screenshots_settle_crop_scale_and_go_inline_or_to_a_checked_path() {
    let mut t = setup(3);
    // Inline, base64.
    let r = ok(&mut t, "ui.screenshot", json!({}));
    let got = (r["width"].as_u64(), r["height"].as_u64(), r["headless"].as_bool(), r["settled"].as_bool());
    assert_eq!(got, (Some(1400), Some(900), Some(true), Some(true)));
    assert_eq!(png_size(&unbase64(r["png"].as_str().unwrap())), (1400, 900));
    // To a file in a write root (the temporary folder by default), replacing an existing PNG.
    let out = t.dir.path().join("shots");
    dalica_fs::create_dir_all(&out).unwrap();
    let path = out.join("all.png");
    let r = ok(&mut t, "ui.screenshot", json!({ "path": path, "settle": false }));
    assert_eq!((r["path"].as_str(), r["settled"].clone()), (path.to_str(), Value::Null));
    assert_eq!(png_size(&std::fs::read(&path).unwrap()), (1400, 900));
    let cell = registry::find(t.d.ctx(), &thumb(t.ids[0])).unwrap();
    ok(&mut t, "ui.screenshot", json!({ "path": path, "widget": cell.id }));
    assert_eq!(png_size(&std::fs::read(&path).unwrap()), (cell.rect[2].round() as u32, cell.rect[3].round() as u32));
    // A region, at twice the scale; the window keeps its own scale.
    let r = ok(&mut t, "ui.screenshot", json!({ "region": [10, 20, 100, 50], "scale": 2 }));
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(200), Some(100)));
    let r = ok(&mut t, "ui.screenshot", json!({ "scale": 1.5, "settle": false }));
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(2100), Some(1350)));
    assert_eq!(t.d.size(), (1400, 900, 1.0));
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["window"]["pixelsPerPoint"], 1.0);
    // Limits (§4).
    for bad in [
        json!({ "scale": 0 }),
        json!({ "scale": 4.5 }),
        json!({ "scale": -1 }),
        json!({ "region": [0, 0, 0, 10] }),
        json!({ "region": [0, 0, 16_385, 10], "scale": 1 }),
        json!({ "region": [0, 0, 9000, 9000], "scale": 1 }),
        json!({ "region": [0, 0, 5000, 5000], "scale": 4 }),
    ] {
        refused(&mut t, "ui.screenshot", bad, "limit");
    }
    // Over what the rasterizer paints, or outside the window: limit after capturing, no harm done.
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "scale": 4, "settle": false })), "limit");
    assert_eq!(t.d.size(), (1400, 900, 1.0));
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "region": [5000, 5000, 10, 10], "settle": false })), "limit");
    // Params.
    for bad in [
        json!({ "path": "relative.png" }),
        json!({ "path": out.join("shot.jpg") }),
        json!({ "path": out.join(".").join("x.png") }),
        json!({ "widget": cell.id, "region": [0, 0, 1, 1] }),
        json!({ "region": "attic" }),
        json!({ "region": [1, 2, 3] }),
        json!({ "settle": "yes" }),
        json!({ "path": 5 }),
    ] {
        refused(&mut t, "ui.screenshot", bad, "bad_params");
    }
    // A region by name: the rect `ui.inspect` → `regions` reports; one not shown is not found.
    let tray = ok(&mut t, "ui.inspect", json!({}))["regions"]["tray"].clone();
    let r = ok(&mut t, "ui.screenshot", json!({ "region": "tray", "settle": false }));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (tray[2].as_f64().map(f64::round), tray[3].as_f64().map(f64::round)));
    refused(&mut t, "ui.screenshot", json!({ "region": "editPanel" }), "not_found");
    refused(&mut t, "ui.screenshot", json!({ "headless": false }), "unsupported");
    refused(&mut t, "ui.screenshot", json!({ "widget": "thumb:0" }), "not_found");
    refused(&mut t, "ui.screenshot", json!({ "path": out.join("..").join("x.png") }), "path_denied");
    // Targets (§5.7), checked off the UI thread after the capture.
    let in_library = t.folder.join("shot.png");
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": in_library, "settle": false })), "path_denied");
    let originals = out.join(".dalica-originals");
    dalica_fs::create_dir_all(&originals).unwrap();
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": originals.join("shot.png"), "settle": false })), "path_denied");
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": out.join("missing").join("shot.png"), "settle": false })), "not_found");
    dalica_fs::create_dir_all(&out.join("dir.png")).unwrap();
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": out.join("dir.png"), "settle": false })), "bad_params");
    assert!(!t.folder.join("shot.png").exists() && !originals.join("shot.png").exists());
}

#[test]
fn screenshots_outside_the_write_roots_are_refused_and_writes_never_run_on_the_ui_thread() {
    let mut t = setup_with(1, |dir| Config { write_roots: vec![dir.join("shots")], ..Config::default() });
    let shots = t.dir.path().join("shots");
    dalica_fs::create_dir_all(&shots).unwrap();
    let elsewhere = t.dir.path().join("elsewhere");
    dalica_fs::create_dir_all(&elsewhere).unwrap();
    assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": elsewhere.join("x.png"), "settle": false })), "path_denied");
    #[cfg(unix)]
    {
        // A link inside the root that points outside it is followed before the check.
        std::os::unix::fs::symlink(&elsewhere, shots.join("link")).unwrap();
        assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": shots.join("link").join("x.png"), "settle": false })), "path_denied");
        write(elsewhere.join("target.png"), b"x");
        std::os::unix::fs::symlink(elsewhere.join("target.png"), shots.join("file.png")).unwrap();
        assert_eq!(code(&mut t, "ui.screenshot", json!({ "path": shots.join("file.png"), "settle": false })), "path_denied");
        assert_eq!(std::fs::read(elsewhere.join("target.png")).unwrap(), b"x");
    }
    assert!(!elsewhere.join("x.png").exists());
    // With this thread marked as the UI thread, the PNG is still written: by a worker.
    let ui_thread = dalica_fs::mark_ui_thread();
    let before = dalica_fs::guard::refused();
    ok(&mut t, "ui.screenshot", json!({ "path": shots.join("ok.png") }));
    assert_eq!(dalica_fs::guard::refused(), before, "no file access from the UI thread");
    drop(ui_thread);
    assert_eq!(png_size(&std::fs::read(shots.join("ok.png")).unwrap()), (1400, 900));
}

/// Opens File ▸ Add Folder… by clicks and waits for the stubbed picker.
fn open_picker(t: &mut T) {
    ok(t, "ui.clickWidget", json!({ "id": "menu:File" }));
    ok(t, "ui.clickWidget", json!({ "id": "menuitem:folders.add" }));
    for _ in 0..1000 {
        if t.control.stub().waiting().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the picker never opened");
}

#[test]
fn a_picker_opened_by_synthetic_input_is_a_stub_answered_by_ui_file_picker() {
    let mut t = setup_with(2, |dir| Config { read_roots: vec![dir.join("extra")], ..Config::default() });
    refused(&mut t, "ui.filePicker", json!({ "cancel": true }), "not_found");
    open_picker(&mut t);
    let v = ok(&mut t, "ui.inspect", json!({}));
    assert_eq!(v["filePicker"], json!({ "command": "folders.add", "kind": "folder", "title": "Add Folder" }));
    // Checked first: the picker stays open.
    for bad in [json!({}), json!({ "path": "/x", "cancel": true }), json!({ "cancel": false }), json!({ "path": "relative" })] {
        refused(&mut t, "ui.filePicker", bad, "bad_params");
    }
    let outside = t.dir.path().join("elsewhere");
    photos(&outside, 1);
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": outside })), "path_denied");
    let missing = t.dir.path().join("extra").join("missing");
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": missing })), "not_found");
    assert!(t.control.stub().waiting().is_some(), "a refused answer leaves the picker open");
    // A good answer runs the command; the reply is its result, after the dialog closed.
    let inside = t.dir.path().join("extra").join("2026-07 More");
    photos(&inside, 2);
    let r = ok(&mut t, "ui.filePicker", json!({ "path": inside }));
    assert_eq!((r["new"].clone(), r["path"].as_str()), (json!(true), inside.to_str()));
    assert!(t.control.stub().waiting().is_none() && t.d.app().panels().unwrap().dialog.is_none());
    assert_eq!(t.d.app().session().unwrap().roots().len(), 2);
    // Cancelling adds nothing.
    open_picker(&mut t);
    assert_eq!(ok(&mut t, "ui.filePicker", json!({ "cancel": true })), json!({}));
    assert!(t.control.stub().waiting().is_none() && t.d.app().panels().unwrap().dialog.is_none());
    assert_eq!(t.d.app().session().unwrap().roots().len(), 2);
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["filePicker"], Value::Null);
}

#[test]
fn base64_crop_and_frame_helpers() {
    assert_eq!(screenshot::base64(b""), "");
    assert_eq!(screenshot::base64(b"f"), "Zg==");
    assert_eq!(screenshot::base64(b"fo"), "Zm8=");
    assert_eq!(screenshot::base64(b"foobar"), "Zm9vYmFy");
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(unbase64(&screenshot::base64(&bytes)), bytes);
    let image = dalica_ui_kit::Image { width: 4, height: 2, rgba: (0..32).collect() };
    let c = screenshot::crop_image(&image, [1.0, 1.0, 2.0, 5.0], 1.0).unwrap();
    assert_eq!((c.width, c.height, c.rgba.clone()), (2, 1, (20..28).collect::<Vec<u8>>()));
    assert!(screenshot::crop_image(&image, [10.0, 0.0, 1.0, 1.0], 1.0).is_none());
    assert!(screenshot::crop_image(&image, [-5.0, -5.0, 2.0, 2.0], 1.0).is_none());
    assert_eq!((frames_for(0), frames_for(16), frames_for(17)), (0, 1, 2));
    assert!(frames_for(u64::MAX) > 0, "no overflow");
}
