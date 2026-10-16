//! Connect mode (`mcp.md` §1, §4.3) against an in-process fake control server: authentication
//! first, a request lost in transit answered "may have completed" and never resent, the next call
//! reconnecting, mismatched reply ids skipped, jobs followed and cancelled through `jobs.cancel`,
//! the headless helper methods mapped onto the app, a photo command without `ids` acting on the
//! app's Photo Tray as in headless mode, each UI tool mapped to its control method, and the UI
//! tools of a headless server saying how to connect.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dalica_mcp::{Backend, Client, Connect, Headless, NoProgress, Progress, Source, ToolError};
use serde_json::{Value, json};

use crate::common::{self, code, ok};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// What the fake app does with one request.
enum Act {
    /// Writes these lines (replies, or anything else).
    Lines(Vec<Value>),
    /// Closes the connection without a reply.
    Drop,
    /// Never answers.
    Hang,
}

type Handler = dyn Fn(usize, &Value) -> Act + Send + Sync;

/// A control server on an ephemeral loopback port that checks `auth` first and records every
/// line it reads with the number of its connection.
struct FakeApp {
    addr: SocketAddr,
    token_file: PathBuf,
    seen: Arc<Mutex<Vec<(usize, Value)>>>,
    _dir: tempfile::TempDir,
}

fn reply(request: &Value, result: Value) -> Value {
    json!({ "id": request["id"], "ok": true, "result": result })
}

fn failure(request: &Value, code: &str, error: &str) -> Value {
    json!({ "id": request["id"], "ok": false, "code": code, "error": error })
}

/// Answers every request with `{method, params}`.
fn echo(_conn: usize, r: &Value) -> Act {
    Act::Lines(vec![reply(r, json!({ "method": r["method"], "params": r["params"] }))])
}

impl FakeApp {
    fn start(handler: impl Fn(usize, &Value) -> Act + Send + Sync + 'static) -> FakeApp {
        let dir = tempfile::tempdir().unwrap();
        let token_file = dir.path().join("control.token");
        std::fs::write(&token_file, format!("{TOKEN}\n")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<(usize, Value)>>> = Arc::default();
        let handler: Arc<Handler> = Arc::new(handler);
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(stream) = stream else { return };
                let (log, handler) = (Arc::clone(&log), Arc::clone(&handler));
                std::thread::spawn(move || serve(n, stream, &log, handler.as_ref()));
            }
        });
        FakeApp { addr, token_file, seen, _dir: dir }
    }

    fn client(&self) -> Client {
        Client::new(self.addr, self.token_file.clone())
    }

    fn connect(&self) -> Connect {
        Connect::new(self.client())
    }

    /// Every line read so far.
    fn seen(&self) -> Vec<(usize, Value)> {
        self.seen.lock().unwrap().clone()
    }

    /// The requests read so far (not `auth`): (connection, method, params).
    fn requests(&self) -> Vec<(usize, String, Value)> {
        self.seen()
            .into_iter()
            .filter(|(_, r)| r["method"] != "auth")
            .map(|(n, r)| (n, r["method"].as_str().unwrap_or_default().to_string(), r["params"].clone()))
            .collect()
    }

    fn methods(&self) -> Vec<String> {
        self.requests().into_iter().map(|(_, m, _)| m).collect()
    }
}

fn serve(n: usize, stream: TcpStream, log: &Mutex<Vec<(usize, Value)>>, handler: &Handler) {
    let mut out = stream.try_clone().unwrap();
    let mut authed = false;
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let r: Value = serde_json::from_str(&line).unwrap();
        log.lock().unwrap().push((n, r.clone()));
        let lines = if !authed {
            if r["method"] != "auth" || r["params"]["token"] != TOKEN {
                let _ = writeln!(out, "{}", failure(&r, "unauthorized", "the first line must be a successful auth"));
                return;
            }
            authed = true;
            vec![reply(&r, json!({ "app": "dalica", "protocol": 1 }))]
        } else {
            match handler(n, &r) {
                Act::Lines(lines) => lines,
                Act::Drop => return,
                Act::Hang => continue,
            }
        };
        for l in lines {
            if writeln!(out, "{l}").is_err() {
                return;
            }
        }
    }
}

/// The first request of every connection is `auth` with the file's token; a wrong token is the
/// app's `unauthorized` and never shows the token; no app, or no token file, ran nothing.
#[test]
fn auth_comes_first_and_the_token_stays_secret() {
    let app = FakeApp::start(echo);
    let mut c = app.client();
    assert!(!c.is_connected(), "nothing is opened before the first call");
    assert_eq!(c.call("ui.state", json!({})).unwrap()["method"], "ui.state");
    c.call("ui.inspect", json!({})).unwrap();
    let seen = app.seen();
    assert_eq!(seen.iter().map(|(n, r)| (*n, r["method"].as_str().unwrap())).collect::<Vec<_>>(), [(0, "auth"), (0, "ui.state"), (0, "ui.inspect")]);
    assert_eq!(seen[0].1["params"]["token"], TOKEN);

    let wrong = "f".repeat(64);
    let file = app.token_file.with_file_name("wrong.token");
    std::fs::write(&file, &wrong).unwrap();
    let e = Client::new(app.addr, file).call("ui.state", json!({})).unwrap_err();
    assert_eq!(e.code, "unauthorized");
    assert!(!e.message.contains(&wrong) && !e.to_json().to_string().contains(&wrong), "{e}");

    let e = Client::new(app.addr, app.token_file.with_file_name("missing.token")).call("ui.state", json!({})).unwrap_err();
    assert_eq!(e.code, "unreachable");
    assert!(e.message.contains("nothing ran"), "{e}");

    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let e = Client::new(closed, app.token_file.clone()).call("ui.state", json!({})).unwrap_err();
    assert_eq!(e.code, "unreachable");
    assert!(e.message.contains("dalica --control"), "{e}");
}

/// A connection dropped while a request was on its way answers that it may have completed; the
/// request is not resent, and the next call opens a new, authenticated connection.
#[test]
fn a_dropped_connection_may_have_completed_and_the_next_call_reconnects() {
    let app = FakeApp::start(|n, r| if n == 0 && r["method"] == "engine.execute" { Act::Drop } else { echo(n, r) });
    let mut c = app.client();
    let e = c.call("engine.execute", json!({ "command": "photo.star.toggle" })).unwrap_err();
    assert_eq!(e.code, "timeout");
    assert!(e.message.contains("may have completed") && e.message.contains("nothing was resent"), "{e}");
    assert!(!c.is_connected());
    assert_eq!(c.call("ui.state", json!({})).unwrap()["method"], "ui.state");
    let seen: Vec<(usize, String)> = app.seen().into_iter().map(|(n, r)| (n, r["method"].as_str().unwrap().to_string())).collect();
    let want: Vec<(usize, String)> =
        [(0, "auth"), (0, "engine.execute"), (1, "auth"), (1, "ui.state")].iter().map(|(n, m)| (*n, m.to_string())).collect();
    assert_eq!(seen, want, "sent once, then a new connection");
}

/// A reply line whose id is not the request's is skipped.
#[test]
fn a_mismatched_id_is_skipped() {
    let app = FakeApp::start(|_, r| {
        Act::Lines(vec![
            json!({ "id": 999, "ok": true, "result": "stale" }),
            json!({ "id": "auth", "ok": true, "result": "late" }),
            reply(r, json!("right")),
        ])
    });
    let mut c = app.client();
    assert_eq!(c.call("ui.state", json!({})).unwrap(), json!("right"));
    assert_eq!(c.call("ui.state", json!({})).unwrap(), json!("right"), "the connection is still in step");
}

/// A request whose reply does not come in time answers that it may have completed, is never
/// sent again, and the next call reconnects.
#[test]
fn a_reply_timeout_never_resends() {
    let app = FakeApp::start(|n, r| if r["method"] == "jobs.list" && n == 0 { Act::Hang } else { echo(n, r) });
    let mut c = app.client().with_timeout(Duration::from_millis(300));
    let e = c.call("jobs.list", json!({})).unwrap_err();
    assert_eq!(e.code, "timeout");
    assert!(e.message.contains("may have completed") && e.message.contains("300 ms"), "{e}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(app.methods(), ["jobs.list"], "sent once, never again");
    assert_eq!(c.call("jobs.list", json!({})).unwrap()["method"], "jobs.list");
    assert_eq!(app.requests().last().map(|(n, _, _)| *n), Some(1), "on a new connection");
}

/// Reports progress and cancels after `stop_after` reports (never with `None`).
struct Counting {
    reports: Vec<(u64, u64)>,
    stop_after: Option<usize>,
}

impl Progress for Counting {
    fn report(&mut self, done: u64, total: u64, _message: &str) -> bool {
        self.reports.push((done, total));
        self.stop_after.is_none_or(|n| self.reports.len() < n)
    }
}

/// A command that starts a job is sent with `wait: false`, its job followed with `jobs.wait`
/// (progress reported) and answered merged; a cancelled request cancels the job with
/// `jobs.cancel`.
#[test]
fn jobs_are_followed_and_cancelled_through_jobs_cancel() {
    let polls = Arc::new(Mutex::new(0u64));
    let p = Arc::clone(&polls);
    let app = FakeApp::start(move |_, r| {
        let result = match r["method"].as_str().unwrap() {
            "engine.execute" => json!({ "folder": "/out/Trip", "job": 7, "pending": true }),
            "jobs.wait" => {
                let mut n = p.lock().unwrap();
                *n += 1;
                if *n < 3 {
                    json!({ "job": 7, "label": "Exporting", "state": "running", "progress": { "done": *n, "total": 3 }, "pending": true })
                } else {
                    json!({ "job": 7, "state": "done", "progress": { "done": 3, "total": 3 }, "result": { "files": 3 } })
                }
            }
            "jobs.cancel" => json!({ "cancelled": [7] }),
            _ => json!({}),
        };
        Act::Lines(vec![reply(r, result)])
    });
    let mut b = app.connect();
    let mut progress = Counting { reports: Vec::new(), stop_after: None };
    let r = b.call("engine.execute", json!({ "command": "export.run", "params": { "dir": "/out" } }), &mut progress).unwrap();
    assert_eq!(r, json!({ "folder": "/out/Trip", "job": 7, "state": "done", "progress": { "done": 3, "total": 3 }, "result": { "files": 3 } }));
    assert_eq!(progress.reports, [(1, 3), (2, 3), (3, 3)]);
    let reqs = app.requests();
    assert_eq!(reqs[0].2, json!({ "command": "export.run", "params": { "dir": "/out" }, "wait": false }));
    assert_eq!(reqs[1].2, json!({ "job": 7, "timeoutMs": 250 }));

    *polls.lock().unwrap() = 0;
    let mut cancelling = Counting { reports: Vec::new(), stop_after: Some(1) };
    let e = b.call("engine.execute", json!({ "command": "export.run", "params": { "dir": "/out" } }), &mut cancelling).unwrap_err();
    assert_eq!(e.code, "cancelled");
    let last = app.requests().pop().unwrap();
    assert_eq!((last.1.as_str(), last.2), ("jobs.cancel", json!({ "job": 7 })));

    let before = app.requests().len();
    b.call("engine.execute", json!({ "command": "export.run", "wait": false }), &mut NoProgress).unwrap();
    assert_eq!(app.requests().len(), before + 1, "wait: false is not followed");
}

/// The fake app's library state for the helper mapping.
fn library_app() -> FakeApp {
    FakeApp::start(|_, r| {
        let m = r["method"].as_str().unwrap();
        let cmd = r["params"]["command"].as_str().unwrap_or_default();
        Act::Lines(vec![match (m, cmd) {
            ("library.inspect", _) => reply(
                r,
                json!({ "selection": { "section": "folder:1", "ids": [2, 3] }, "tray": { "items": [9, 2, 3], "held": [9], "traySelected": [] } }),
            ),
            ("batch", _) => {
                let steps = r["params"]["steps"].as_array().unwrap();
                if steps.iter().any(|s| s["command"] == "photo.caption.set") {
                    reply(
                        r,
                        json!({ "completed": 1, "failed": 1, "results": [{ "ok": true, "result": {} }, { "ok": false, "code": "not_found", "error": "no photo 99" }] }),
                    )
                } else {
                    reply(
                        r,
                        json!({ "completed": 2, "failed": 0, "results": [{ "ok": true, "result": { "a": 1 } }, { "ok": true, "result": { "b": 2 } }] }),
                    )
                }
            }
            ("engine.execute", "photo.star.set") => reply(r, json!({ "starred": [2] })),
            ("engine.execute", _) => reply(r, json!({})),
            _ => failure(r, "unknown_method", "no such method"),
        }])
    })
}

/// `tray.state` from `library.inspect`; `tray.select` as `select.set` (an `add` keeping the
/// selection) then `tray.hold`; `engine.group` as one `engine.execute`, or a `batch` whose first
/// failure is the answer with its step named.
#[test]
fn the_headless_helper_methods_map_onto_the_app() {
    let app = library_app();
    let mut b = app.connect();
    let state = json!({ "section": "folder:1", "selection": [2, 3], "held": [9], "tray": [9, 2, 3] });
    assert_eq!(b.call("tray.state", json!({}), &mut NoProgress).unwrap(), state);
    assert_eq!(b.call("tray.select", json!({ "ids": [4, 2], "mode": "add", "hold": true }), &mut NoProgress).unwrap(), state);
    let sent: Vec<(String, Value)> = app.requests().into_iter().skip(1).map(|(_, m, p)| (m, p)).collect();
    assert_eq!(
        sent,
        [
            ("library.inspect".to_string(), json!({})),
            ("engine.execute".into(), json!({ "command": "select.set", "params": { "ids": [2, 3, 4], "section": "folder:1" } })),
            ("engine.execute".into(), json!({ "command": "tray.hold", "params": {} })),
            ("library.inspect".into(), json!({})),
        ]
    );
    assert_eq!(b.call("tray.select", json!({ "ids": [1], "mode": "toggle" }), &mut NoProgress).unwrap_err().code, "bad_params");

    let one = json!({ "steps": [{ "command": "photo.star.set", "params": { "ids": [2], "on": true } }], "label": "Set Metadata" });
    assert_eq!(b.call("engine.group", one, &mut NoProgress).unwrap(), json!({ "results": [{ "starred": [2] }] }));
    // Steps without `ids` get the app's tray, read once for the group.
    let before = app.requests().len();
    let two = json!({ "steps": [{ "command": "photo.star.set", "params": { "on": true } }, { "command": "photo.hide" }] });
    assert_eq!(b.call("engine.group", two, &mut NoProgress).unwrap(), json!({ "results": [{ "a": 1 }, { "b": 2 }] }));
    let sent: Vec<(String, Value)> = app.requests().into_iter().skip(before).map(|(_, m, p)| (m, p)).collect();
    let steps = json!([{ "command": "photo.star.set", "params": { "on": true, "ids": [9, 2, 3] } }, { "command": "photo.hide", "params": { "ids": [9, 2, 3] } }]);
    assert_eq!(sent, [("library.inspect".to_string(), json!({})), ("batch".into(), json!({ "steps": steps, "stopOnError": true }))]);
    let failing = json!({ "steps": [{ "command": "photo.star.set" }, { "command": "photo.caption.set" }] });
    let e = b.call("engine.group", failing, &mut NoProgress).unwrap_err();
    assert_eq!(e.code, "not_found");
    assert!(e.message.starts_with("step 2: no photo 99"), "{e}");
    assert_eq!(b.call("engine.group", json!({ "steps": [] }), &mut NoProgress).unwrap_err().code, "limit");
}

/// A fake app whose Photo Tray holds `tray` (`library.inspect`) and whose `engine.execute` runs in
/// a real headless session over the demo library, on a thread of its own, so its answers are the
/// engine's.
fn engine_app(tray: Vec<u64>) -> FakeApp {
    type Job = (Value, std::sync::mpsc::Sender<Result<Value, ToolError>>);
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    std::thread::spawn(move || {
        let mut h = Headless::open(Source::Demo, &[]).unwrap();
        for (params, answer) in rx {
            let _ = answer.send(h.call("engine.execute", params, &mut NoProgress));
        }
    });
    let tx = Mutex::new(tx);
    FakeApp::start(move |_, r| {
        Act::Lines(vec![match r["method"].as_str().unwrap() {
            "library.inspect" => reply(r, json!({ "selection": { "section": "folder:1", "ids": tray }, "tray": { "items": tray, "held": [] } })),
            "engine.execute" => {
                let (answer, result) = std::sync::mpsc::channel();
                tx.lock().unwrap().send((r["params"].clone(), answer)).unwrap();
                match result.recv().unwrap() {
                    Ok(v) => reply(r, v),
                    Err(e) => failure(r, &e.code, &e.message),
                }
            }
            _ => failure(r, "unknown_method", "no such method"),
        }])
    })
}

/// A photo command without `ids` acts on the app's Photo Tray (M10.54; `mcp.md` §4.1): with two
/// photos in the tray it changes both; with an empty tray it answers what headless mode answers.
#[test]
fn a_photo_command_without_ids_acts_on_the_apps_tray() {
    let app = engine_app(vec![2, 3]);
    let mut b = app.connect();
    let r = b.call("engine.execute", json!({ "command": "photo.star.toggle" }), &mut NoProgress).unwrap();
    assert_eq!(r["ids"], json!([2, 3]), "both photos of the tray: {r}");
    let sent = app.requests().into_iter().find(|(_, m, _)| m == "engine.execute").map(|(_, _, p)| p);
    assert_eq!(sent, Some(json!({ "command": "photo.star.toggle", "params": { "ids": [2, 3] }, "wait": false })));
    b.call("engine.execute", json!({ "command": "photo.rotate.cw", "params": {} }), &mut NoProgress).unwrap();
    assert_eq!(app.requests().last().map(|(_, _, p)| p["params"].clone()), Some(json!({ "ids": [2, 3] })));

    let empty = engine_app(Vec::new());
    let e = empty.connect().call("engine.execute", json!({ "command": "photo.rotate.cw" }), &mut NoProgress).unwrap_err();
    let mut h = Headless::open(Source::Demo, &[]).unwrap();
    let want = h.call("engine.execute", json!({ "command": "photo.rotate.cw" }), &mut NoProgress).unwrap_err();
    assert_eq!((e.code.as_str(), e.message.as_str()), (want.code.as_str(), want.message.as_str()));
    assert_eq!(e.code, "disabled", "{e}");
}

/// `render` reads the photo and its recipe from the app and renders the file here, through the
/// output pipeline, never enlarged.
#[test]
fn render_goes_through_the_apps_photo() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("IMG_0001.jpg");
    std::fs::write(&photo, dalica_scenes::jpeg(&dalica_scenes::Scene::new(1, 400, 300), 90, None).unwrap()).unwrap();
    let path = photo.to_string_lossy().into_owned();
    let app = FakeApp::start(move |_, r| {
        Act::Lines(vec![match r["method"].as_str().unwrap() {
            "engine.execute" => reply(r, json!([{ "id": 5, "path": path, "recipe": { "edits": ["rotate(90)"], "redo": [] } }])),
            _ => reply(r, json!({ "selection": {}, "tray": { "items": [5], "held": [] } })),
        }])
    });
    let mut b = app.connect();
    let r = b.render(None, 200).unwrap();
    assert_eq!((r.id, r.mime, r.width, r.height, r.recipe.clone()), (5, "image/png", 150, 200, vec!["rotate(90)".to_string()]));
    assert_eq!(png_size(&r.bytes), (150, 200), "the recipe from the app applied");
    let last = app.requests().pop().unwrap();
    assert_eq!(last.2, json!({ "command": "photo.inspect", "params": { "ids": [5] } }));
}

/// A PNG's width and height from its header.
fn png_size(png: &[u8]) -> (u32, u32) {
    let be = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    (be(16), be(20))
}

/// Writes a small window capture for the fake `ui.screenshot`.
fn capture(dir: &Path) -> (Vec<u8>, PathBuf) {
    let png = dalica_scenes::png(&dalica_scenes::Scene::new(2, 320, 200)).unwrap();
    let path = dir.join("shot.png");
    std::fs::write(&path, &png).unwrap();
    (png, path)
}

/// Each UI tool sends its control method with its arguments renamed; `screenshot` comes back as
/// image content scaled to `max_size`; `tools/list` holds them and `listChanged` is on.
#[test]
fn each_ui_tool_maps_to_its_method() {
    let dir = tempfile::tempdir().unwrap();
    let (png, shot) = capture(dir.path());
    let inline = dalica_mcp_base64(&png);
    let app = FakeApp::start(move |n, r| {
        if r["method"] == "ui.screenshot" {
            let mut result = json!({ "width": 320, "height": 200, "headless": true });
            if r["params"].get("path").is_none() {
                result["png"] = json!(inline);
            } else {
                result["path"] = r["params"]["path"].clone();
            }
            return Act::Lines(vec![reply(r, result)]);
        }
        echo(n, r)
    });
    let addr = app.addr;
    let token = app.token_file.clone();
    let mut c = common::Client::start(move || Box::new(Connect::new(Client::new(addr, token))));
    let init = common::initialize(&mut c);
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], true);
    let list = c.request(2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for t in
        ["screenshot", "inspect_ui", "list_widgets", "click", "drag", "press_key", "type_text", "scroll", "menu_invoke", "settle", "control_call"]
    {
        assert!(names.contains(&t), "{t} listed in connect mode");
    }

    let cases = [
        ("inspect_ui", json!({ "reset_perf": true }), "ui.inspect", json!({ "resetPerf": true })),
        ("list_widgets", json!({ "filter": "thumb:", "region": "lightTable" }), "ui.widgets", json!({ "filter": "thumb:", "region": "lightTable" })),
        ("click", json!({ "widget": "thumb:3", "count": 2, "ctrl": true }), "ui.clickWidget", json!({ "id": "thumb:3", "count": 2, "ctrl": true })),
        (
            "click",
            json!({ "x": 10.0, "y": 20.0, "button": "right" }),
            "ui.pointer",
            json!({ "button": "right", "events": [{ "kind": "move", "x": 10.0, "y": 20.0 }, { "kind": "down", "x": 10.0, "y": 20.0 }, { "kind": "up", "x": 10.0, "y": 20.0 }] }),
        ),
        (
            "drag",
            json!({ "widget": "thumb:5", "to_widget": "tree:folder:2", "ctrl": true }),
            "ui.dragWidget",
            json!({ "id": "thumb:5", "toWidget": "tree:folder:2", "ctrl": true }),
        ),
        ("drag", json!({ "widget": "thumb:5", "to_x": 1.0, "to_y": 2.0 }), "ui.dragWidget", json!({ "id": "thumb:5", "toX": 1.0, "toY": 2.0 })),
        ("press_key", json!({ "combo": "Ctrl+H", "state": "down" }), "ui.key", json!({ "combo": "Ctrl+H", "state": "down" })),
        ("press_key", json!({ "key": "F2", "shift": true }), "ui.key", json!({ "key": "F2", "shift": true })),
        ("type_text", json!({ "text": "Tram 28" }), "ui.type", json!({ "text": "Tram 28" })),
        ("scroll", json!({ "dy": 120.0, "widget": "region:lightTable" }), "ui.scroll", json!({ "dy": 120.0, "widget": "region:lightTable" })),
        (
            "menu_invoke",
            json!({ "path": "Picture/Batch Edit/Rotate Clockwise" }),
            "ui.menu.invoke",
            json!({ "path": "Picture/Batch Edit/Rotate Clockwise" }),
        ),
        (
            "menu_invoke",
            json!({ "id": "photo.rotate.cw", "params": { "x": 1 } }),
            "ui.menu.invoke",
            json!({ "id": "photo.rotate.cw", "params": { "x": 1 } }),
        ),
        ("settle", json!({ "timeout_ms": 2000, "jobs": true }), "ui.settle", json!({ "timeoutMs": 2000, "jobs": true })),
        ("control_call", json!({ "method": "ui.menu.tree", "params": { "context": "photo" } }), "ui.menu.tree", json!({ "context": "photo" })),
    ];
    for (i, (tool, args, method, params)) in cases.iter().enumerate() {
        let r = c.call(10 + i as u64, tool, args.clone());
        assert_eq!(ok(&r), &json!({ "method": method, "params": params }), "{tool} {args}");
    }
    for (tool, args) in [
        ("click", json!({})),
        ("click", json!({ "widget": "a", "x": 1.0, "y": 2.0 })),
        ("drag", json!({ "widget": "a" })),
        ("press_key", json!({})),
        ("menu_invoke", json!({})),
    ] {
        assert_eq!(code(&c.call(50, tool, args.clone())), "bad_params", "{tool} {args}");
    }

    let r = c.call(60, "screenshot", json!({}));
    assert_eq!(r["content"][0]["type"], "image");
    assert_eq!(common::base64_decode(r["content"][0]["data"].as_str().unwrap()), png, "fits: as the app sent it");
    assert_eq!(ok(&r)["width"], 320);
    let r = c.call(61, "screenshot", json!({ "max_size": 160, "headless": true, "path": shot.to_string_lossy() }));
    assert_eq!(png_size(&common::base64_decode(r["content"][0]["data"].as_str().unwrap())), (160, 100), "scaled to max_size");
    assert_eq!((ok(&r)["width"].as_u64(), ok(&r)["height"].as_u64()), (Some(160), Some(100)));
    let shots: Vec<Value> = app.requests().into_iter().filter(|(_, m, _)| m == "ui.screenshot").map(|(_, _, p)| p).collect();
    assert_eq!(shots, [json!({}), json!({ "headless": true, "path": shot.to_string_lossy() })]);
}

/// The base64 the server writes, for the fake's inline screenshot.
fn dalica_mcp_base64(bytes: &[u8]) -> String {
    let r = dalica_mcp::ToolResult::image(bytes, "image/png", json!({}));
    r.content[0]["data"].as_str().unwrap().to_string()
}

/// In headless mode the UI tools are not listed, and a call answers how to connect.
#[test]
fn headless_ui_tools_answer_how_to_connect() {
    let mut c = common::Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    let init = common::initialize(&mut c);
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], false);
    let list = c.request(2, "tools/list", json!({}));
    assert!(!list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "click"));
    for (i, tool) in ["screenshot", "click", "control_call"].iter().enumerate() {
        let r = c.call(3 + i as u64, tool, json!({}));
        assert_eq!(code(&r), "unsupported");
        let msg = r["structuredContent"]["error"].as_str().unwrap();
        assert!(msg.contains("dalica --control") && msg.contains("--connect"), "{msg}");
    }
}

/// A failure the app answers keeps its code and its prompt.
#[test]
fn the_apps_errors_keep_their_code_and_prompt() {
    let app = FakeApp::start(|_, r| {
        Act::Lines(vec![json!({ "id": r["id"], "ok": false, "code": "needs_confirm", "error": "Delete 2 photos?", "prompt": { "title": "Delete" } })])
    });
    let e: ToolError = app.client().call("engine.execute", json!({ "command": "photo.delete" })).unwrap_err();
    assert_eq!((e.code.as_str(), e.message.as_str(), e.prompt), ("needs_confirm", "Delete 2 photos?", Some(json!({ "title": "Delete" }))));
}
