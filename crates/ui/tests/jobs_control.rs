//! `jobs.list`, `jobs.wait` and `jobs.cancel` (M5.14, `control-protocol.md` §6.13) over the
//! session's job registry, driven headlessly: a scan added over automation is listed with its
//! progress (and in `ui.inspect` → `jobs`); `jobs.wait` answers `pending` while it runs and its final
//! entry once it has finished, without blocking a frame; `jobs.cancel` stops a scan; bad parameters
//! are `bad_params`, unknown jobs `not_found`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dalica_engine::Session;
use dalica_scan::HeaderReader;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A folder `name` of `n` photos under `root`.
fn folder(root: &Path, name: &str, n: usize) -> PathBuf {
    let dir = root.join("photos").join(name);
    dalica_fs::create_dir_all(&dir).unwrap();
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 96, 64), 80, None).unwrap();
    for i in 0..n {
        dalica_fs::replace(&dir.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg, dalica_fs::Expect::Absent).unwrap();
    }
    dir
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

/// A header reader that holds every file until the gate opens (at most 20 s per file).
fn gated() -> (HeaderReader, Gate) {
    let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (g, inner) = (Arc::clone(&gate), dalica_scan::default_reader());
    let reader: HeaderReader = Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
        let (lock, cv) = &*g;
        let _ = cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(20), |open| !*open).unwrap();
        inner(path, limits)
    });
    (reader, gate)
}

fn set(gate: &Gate, open: bool) {
    *gate.0.lock().unwrap() = open;
    gate.1.notify_all();
}

struct T {
    _dir: tempfile::TempDir,
    root: PathBuf,
    control: Control,
    conn: Connection,
    d: Driver,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let session = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let control = Control::new(Config { read_roots: vec![root.join("photos")], ..Config::default() });
    let mut d = Driver::new(App::with_picker(Some(session), control.picker()), 1400, 900, 1.0);
    d.settle(600).unwrap();
    T { _dir: dir, root, control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

/// A scan added over automation, held by the gate: listed with its progress, `pending` while it
/// runs, its final entry once the gate opens.
#[test]
fn a_scan_is_listed_with_progress_and_waited_for() {
    let mut t = setup();
    assert_eq!(ok(&mut t, "jobs.list", json!({})), json!([]));
    let (reader, gate) = gated();
    t.d.app_mut().session_mut().unwrap().set_scan_reader(Some(reader));
    let photos = folder(&t.root, "2026-06-01 Trip", 6);
    let added = ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": photos } }));
    assert_eq!(added["scanning"], true);

    let list = ok(&mut t, "jobs.list", Value::Null);
    let jobs = list.as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{list}");
    let job = jobs[0]["job"].as_u64().unwrap();
    assert_eq!((jobs[0]["kind"].as_str(), jobs[0]["state"].as_str(), jobs[0]["cancellable"].as_bool()), (Some("scan"), Some("running"), Some(true)));
    assert!(jobs[0]["progress"]["done"].is_u64() && jobs[0]["progress"]["total"].is_u64(), "{list}");
    assert!(jobs[0]["label"].as_str().is_some_and(|l| !l.is_empty()));
    // `ui.inspect` lists the same jobs.
    let inspected = ok(&mut t, "ui.inspect", json!({}))["jobs"].clone();
    assert_eq!(inspected[0]["job"], job);

    // Still held: a short wait answers `pending`, with frames running meanwhile (no frame blocked).
    let frames = t.d.frames();
    let waited = ok(&mut t, "jobs.wait", json!({ "job": job, "timeoutMs": 300 }));
    assert_eq!((waited["job"].as_u64(), waited["pending"].as_bool(), waited["state"].as_str()), (Some(job), Some(true), Some("running")));
    assert!(t.d.frames() > frames + 1, "jobs.wait lets frames run");

    // Released: the final entry, every photo indexed.
    set(&gate, true);
    let done = ok(&mut t, "jobs.wait", json!({ "job": job }));
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["progress"], json!({ "done": 6, "total": 6 }));
    assert!(done.get("pending").is_none() && done["result"].is_object(), "{done}");
    assert_eq!(t.d.app().session().unwrap().library().photos().count(), 6);
    // A finished job answers at once, and stays listed.
    assert_eq!(ok(&mut t, "jobs.wait", json!({ "job": job }))["state"], "done");
    assert_eq!(ok(&mut t, "jobs.list", json!({}))[0]["state"], "done");
    // Cancelling everything now cancels nothing.
    assert_eq!(ok(&mut t, "jobs.cancel", json!({})), json!({ "cancelled": [] }));
}

/// `jobs.cancel` stops a scan at its next file: it ends `cancelled`, and what it read stays.
#[test]
fn jobs_cancel_stops_a_scan() {
    let mut t = setup();
    let (reader, gate) = gated();
    t.d.app_mut().session_mut().unwrap().set_scan_reader(Some(reader));
    let photos = folder(&t.root, "2026-06-02 Hike", 40);
    ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": photos } }));
    let job = ok(&mut t, "jobs.list", json!({}))[0]["job"].as_u64().unwrap();
    assert_eq!(ok(&mut t, "jobs.cancel", json!({ "job": job })), json!({ "cancelled": [job] }));
    // The held file is read, then the walk stops.
    set(&gate, true);
    let end = ok(&mut t, "jobs.wait", json!({ "job": job }));
    assert_eq!(end["state"], "cancelled", "{end}");
    assert_eq!(end["result"]["cancelled"], true, "{end}");
    let indexed = t.d.app().session().unwrap().library().photos().count();
    assert!(indexed < 40, "{indexed} of 40 indexed after the cancel");
    // A finished job cannot be cancelled again.
    assert_eq!(ok(&mut t, "jobs.cancel", json!({ "job": job })), json!({ "cancelled": [] }));
}

/// Bad parameters are `bad_params`, unknown jobs `not_found`, waits past the limit `limit`; none
/// runs a frame.
#[test]
fn bad_params_and_unknown_jobs_are_refused() {
    let mut t = setup();
    let frames = t.d.frames();
    for (method, params) in [
        ("jobs.list", json!({ "job": 1 })),
        ("jobs.list", json!([1])),
        ("jobs.wait", json!({})),
        ("jobs.wait", json!({ "job": "1" })),
        ("jobs.wait", json!({ "job": -1 })),
        ("jobs.wait", json!({ "job": 1.5 })),
        ("jobs.wait", json!({ "job": 1, "timeoutMs": -5 })),
        ("jobs.wait", json!({ "job": 1, "zzz": 1 })),
        ("jobs.wait", json!("x")),
        ("jobs.cancel", json!({ "job": "all" })),
        ("jobs.cancel", json!({ "jobs": [1] })),
        ("jobs.cancel", json!(7)),
    ] {
        assert_eq!(code(&mut t, method, params.clone()), "bad_params", "{method} {params}");
    }
    assert_eq!(code(&mut t, "jobs.wait", json!({ "job": 99 })), "not_found");
    assert_eq!(code(&mut t, "jobs.wait", json!({ "job": u64::MAX })), "not_found");
    assert_eq!(code(&mut t, "jobs.cancel", json!({ "job": 99 })), "not_found");
    assert_eq!(code(&mut t, "jobs.wait", json!({ "job": 1, "timeoutMs": 60_001 })), "limit");
    assert_eq!(t.d.frames(), frames, "refused requests run no frame");
}
