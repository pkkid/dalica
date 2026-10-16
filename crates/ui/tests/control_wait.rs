//! Replies that wait for their job (`control-protocol.md` §5.3): `engine.execute` of a command
//! that starts a job lets frames run until the job ends and replies with its final entry; with
//! `wait: false` it names the job at once (`{job, pending: true}`), and `jobs.wait` then gives the
//! result. The scans (`folders.add`, `library.rebuild`) name their scan job.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::common::new_driver;
use dalica_engine::Session;
use dalica_engine::watching::WatchMode;
use dalica_scan::HeaderReader;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A folder `name` of `n` photos under `root`.
fn folder(root: &Path, name: &str, n: usize) -> PathBuf {
    let dir = root.join("photos").join(name);
    dalica_fs::create_dir_all(&dir).unwrap();
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(5, 96, 64), 80, None).unwrap();
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

fn open(gate: &Gate) {
    *gate.0.lock().unwrap() = true;
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
    let mut session = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    // No watcher: a late change event would start a rescan job of its own.
    session.set_watching(WatchMode::Off);
    let control = Control::new(Config { read_roots: vec![root.join("photos")], write_roots: vec![root.join("out")], ..Config::default() });
    let mut d = new_driver(App::with_picker(Some(session), control.picker()), 1400, 900, 1.0);
    d.settle(600).unwrap();
    T { _dir: dir, root, control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn photos(t: &T) -> usize {
    t.d.app().session().unwrap().library().photos().count()
}

#[test]
fn folders_add_replies_after_its_scan_with_the_jobs_result() {
    let mut t = setup();
    let dir = folder(&t.root, "2026-06-01 Trip", 5);
    let r = ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": dir } }));
    // The command's own fields, then the scan job's final entry.
    assert_eq!((r["new"].clone(), r["scanning"].clone()), (json!(true), json!(false)), "{r}");
    assert_eq!((r["kind"].as_str(), r["state"].as_str()), (Some("scan"), Some("done")), "{r}");
    assert!(r["job"].is_u64() && r["result"].is_object() && r.get("pending").is_none(), "{r}");
    assert_eq!(r["progress"], json!({ "done": 5, "total": 5 }), "{r}");
    assert_eq!(photos(&t), 5, "every photo is in the library when the reply comes");
    // The same entry as `jobs.wait` gives.
    let entry = ok(&mut t, "jobs.wait", json!({ "job": r["job"] }));
    assert_eq!((entry["state"].clone(), entry["result"].clone()), (r["state"].clone(), r["result"].clone()));
}

#[test]
fn wait_false_names_the_job_and_jobs_wait_returns_its_result() {
    let mut t = setup();
    let (reader, gate) = gated();
    t.d.app_mut().session_mut().unwrap().set_scan_reader(Some(reader));
    let dir = folder(&t.root, "2026-06-02 Hike", 4);
    let r = ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": dir }, "wait": false }));
    assert_eq!((r["pending"].clone(), r["new"].clone(), r["scanning"].clone()), (json!(true), json!(true), json!(true)), "{r}");
    let job = r["job"].as_u64().expect("the scan names its job");
    let listed = ok(&mut t, "jobs.list", json!({}));
    assert_eq!((listed[0]["job"].as_u64(), listed[0]["state"].as_str()), (Some(job), Some("running")), "{listed}");
    open(&gate);
    let end = ok(&mut t, "jobs.wait", json!({ "job": job }));
    assert_eq!((end["state"].as_str(), end["kind"].as_str()), (Some("done"), Some("scan")), "{end}");
    assert!(end["result"].is_object(), "{end}");
    assert_eq!(photos(&t), 4);
}

/// A command that names its job itself (`export.run`) waits for it too, and replies the export's
/// final entry; `wait: false` keeps its own `{job, pending: true, …}`.
#[test]
fn a_command_that_names_its_job_waits_for_it() {
    let mut t = setup();
    let dir = folder(&t.root, "2026-06-03 Coast", 2);
    ok(&mut t, "engine.execute", json!({ "command": "folders.add", "params": { "path": dir } }));
    let ids: Vec<u64> = t.d.app().session().unwrap().library().photos().map(|p| p.id.get()).collect();
    let out = t.root.join("out");
    dalica_fs::create_dir_all(&out).unwrap();
    let export = |name: &str| json!({ "ids": ids, "dir": out, "folderName": name, "size": 100 });
    let r = ok(&mut t, "engine.execute", json!({ "command": "export.run", "params": export("Waited") }));
    assert_eq!((r["kind"].as_str(), r["state"].as_str(), r["total"].as_u64()), (Some("export"), Some("done"), Some(2)), "{r}");
    assert!(r.get("pending").is_none(), "{r}");
    let written = std::fs::read_dir(out.join("Waited")).unwrap().filter_map(Result::ok);
    let jpegs = written.filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg"))).count();
    assert_eq!(jpegs, 2, "the export has written its pictures");
    let r = ok(&mut t, "engine.execute", json!({ "command": "export.run", "params": export("Later"), "wait": false }));
    assert_eq!(r["pending"], true, "{r}");
    assert_eq!(ok(&mut t, "jobs.wait", json!({ "job": r["job"] }))["state"], "done");
}

#[test]
fn wait_takes_a_boolean() {
    let mut t = setup();
    for wait in [json!("yes"), json!(1), json!([])] {
        let e = call(&mut t, "engine.execute", json!({ "command": "select.none", "wait": wait })).unwrap_err();
        assert_eq!(e.code.as_str(), "bad_params", "{wait}");
    }
}
