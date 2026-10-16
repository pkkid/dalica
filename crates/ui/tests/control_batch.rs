//! The control method `batch` (`control-protocol.md` §6.3): command and method steps run in order
//! with the semantics of separate requests (one undo step per command), `stopOnError` stops at the
//! first failure or goes on, the steps that may not be steps refuse the whole batch before anything
//! runs, a batch over 256 steps is `limit`, and results that would pass the reply limit stop the
//! batch with a `limit` entry.

use std::sync::Arc;

use crate::common::new_driver;
use dalica_ui::control::{Config, Connection, Control, MAX_REPLY_LINE, Reply, Request, reply_line, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: dalica_fs::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
}

fn setup() -> T {
    let dir = dalica_engine::demo::fresh_dir().unwrap();
    let session = crate::common::demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
    let control = Control::new(Config { demo: true, ..Config::default() });
    let mut d = new_driver(App::with_picker(Some(session), control.picker()), 1400, 900, 1.0);
    d.settle(600).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn batch(t: &mut T, params: Value) -> Value {
    call(t, "batch", params.clone()).unwrap_or_else(|e| panic!("batch {params}: {e}"))
}

fn refused(t: &mut T, params: Value) -> &'static str {
    match call(t, "batch", params.clone()) {
        Ok(v) => panic!("batch {params} answered {v}"),
        Err(e) => e.code.as_str(),
    }
}

fn starred(t: &T, id: u64) -> bool {
    t.d.app().session().unwrap().library().photo(dalica_core::PhotoId::new(id)).is_some_and(|p| p.star)
}

fn undo_steps(t: &T) -> usize {
    t.d.app().session().unwrap().undo_stack().len()
}

fn star(id: u64) -> Value {
    json!({ "command": "photo.star.set", "params": { "ids": [id], "on": true } })
}

#[test]
fn a_mixed_batch_runs_in_order_with_one_undo_step_per_command() {
    let mut t = setup();
    let before = undo_steps(&t);
    let r = batch(
        &mut t,
        json!({ "steps": [
            { "command": "select.set", "params": { "ids": [2] } },
            { "method": "ui.key", "params": { "combo": "Ctrl+8" } },
            star(3),
            { "command": "photo.caption.set", "params": { "ids": [3], "caption": "Harbour at dusk" } },
            { "method": "engine.execute", "params": { "command": "photo.inspect", "params": { "ids": [2, 3] } } },
            { "method": "ui.state" },
        ] }),
    );
    assert_eq!((r["completed"].as_u64(), r["failed"].as_u64()), (Some(6), Some(0)), "{r}");
    let results = r["results"].as_array().unwrap();
    assert!(results.iter().all(|e| e["ok"] == true), "{r}");
    // Each step saw the ones before it: the key starred the selected photo, then the inspection
    // read both stars and the caption.
    let inspected = &results[4]["result"];
    assert_eq!((inspected[0]["star"].clone(), inspected[1]["star"].clone()), (json!(true), json!(true)), "{inspected}");
    assert_eq!(inspected[1]["caption"], "Harbour at dusk");
    assert_eq!(results[5]["result"]["view"], "library");
    // Three changes, three undo steps: undoing one takes back the caption only.
    assert_eq!(undo_steps(&t), before + 3);
    call(&mut t, "engine.execute", json!({ "command": "edit.undo" })).unwrap();
    let after = call(&mut t, "engine.execute", json!({ "command": "photo.inspect", "params": { "ids": [3] } })).unwrap();
    assert!(after[0]["caption"].as_str().is_none_or(str::is_empty), "{after}");
    assert!(starred(&t, 3) && starred(&t, 2));
}

#[test]
fn stop_on_error_stops_at_the_first_failure_and_false_goes_on() {
    let mut t = setup();
    let steps = json!([star(4), { "command": "photo.star.set", "params": { "ids": [999999], "on": true } }, { "method": "ui.nope" }, star(5)]);
    let r = batch(&mut t, json!({ "steps": steps }));
    assert_eq!((r["completed"].as_u64(), r["failed"].as_u64()), (Some(1), Some(1)), "{r}");
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{r}");
    assert_eq!((results[1]["ok"].clone(), results[1]["code"].clone()), (json!(false), json!("not_found")), "{r}");
    assert!(results[1]["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(starred(&t, 4) && !starred(&t, 5), "stopped before the last step");

    let r = batch(&mut t, json!({ "steps": steps, "stopOnError": false }));
    assert_eq!((r["completed"].as_u64(), r["failed"].as_u64()), (Some(2), Some(2)), "{r}");
    let codes: Vec<Value> = r["results"].as_array().unwrap().iter().map(|e| e["code"].clone()).collect();
    assert_eq!(codes, [Value::Null, json!("not_found"), json!("unknown_method"), Value::Null]);
    assert!(starred(&t, 5), "went on past the failures");
}

/// `batch`, `auth` and `app.quit` are never steps, and a step of the wrong shape refuses the
/// batch: nothing runs, not even the steps before it.
#[test]
fn forbidden_and_malformed_steps_refuse_the_whole_batch() {
    let mut t = setup();
    for bad in [
        json!({ "method": "batch", "params": { "steps": [] } }),
        json!({ "method": "auth", "params": { "token": "x" } }),
        json!({ "method": "app.quit" }),
        json!({ "command": "photo.star.set", "method": "ui.state" }),
        json!({}),
        json!(7),
        json!({ "method": 3 }),
        json!({ "command": "photo.star.set", "zzz": 1 }),
        json!({ "method": "ui.state", "wait": true }),
    ] {
        assert_eq!(refused(&mut t, json!({ "steps": [star(6), bad] })), "bad_params", "{bad}");
    }
    for bad in [json!({}), json!({ "steps": 3 }), json!({ "steps": [], "stopOnError": "yes" }), json!({ "steps": [], "zzz": 1 })] {
        assert_eq!(refused(&mut t, bad.clone()), "bad_params", "{bad}");
    }
    assert!(!starred(&t, 6), "nothing ran");
    assert_eq!(batch(&mut t, json!({ "steps": [] })), json!({ "completed": 0, "failed": 0, "results": [] }));
}

#[test]
fn a_batch_of_257_steps_is_limit_and_256_run() {
    let mut t = setup();
    let mut steps = vec![star(7)];
    steps.extend(std::iter::repeat_n(json!({ "method": "methods" }), 256));
    assert_eq!(refused(&mut t, json!({ "steps": steps })), "limit");
    assert!(!starred(&t, 7), "nothing ran");
    steps.pop();
    assert_eq!(batch(&mut t, json!({ "steps": steps }))["completed"], 256);
    assert!(starred(&t, 7));
}

/// Results that would pass the 8 MiB reply stop the batch: the step that hit the budget ran, its
/// result is replaced by a `limit` entry, and the reply still fits in a line.
#[test]
fn an_oversized_aggregate_stops_with_limit() {
    let mut t = setup();
    let one = serde_json::to_string(&call(&mut t, "engine.commands", json!({})).unwrap()).unwrap().len();
    let n = MAX_REPLY_LINE / one + 2;
    assert!(n <= 256, "engine.commands is {one} bytes: {n} steps would be needed");
    let steps = vec![json!({ "method": "engine.commands" }); n];
    let r = batch(&mut t, json!({ "steps": steps, "stopOnError": false }));
    let results = r["results"].as_array().unwrap();
    assert!(results.len() < n, "stopped early: {} of {n}", results.len());
    let last = results.last().unwrap();
    assert_eq!((last["ok"].clone(), last["code"].clone()), (json!(false), json!("limit")), "{last}");
    assert_eq!(r["failed"], 1);
    assert_eq!(r["completed"].as_u64(), Some(results.len() as u64 - 1));
    let line = reply_line(&json!(1), &Ok(r.clone()));
    assert!(line.len() < MAX_REPLY_LINE && line.contains("\"ok\":true"), "the reply fits: {} bytes", line.len());
}

/// A key a step holds down is let go when the batch ends: the connection never knew of it.
#[test]
fn keys_held_by_a_step_are_let_go_when_the_batch_ends() {
    let mut t = setup();
    let r = batch(&mut t, json!({ "steps": [{ "method": "ui.key", "params": { "key": "A", "state": "down" } }] }));
    assert_eq!(r["completed"], 1, "{r}");
    assert!(t.conn.held().is_empty());
    assert!(!t.d.ctx().input(|i| i.key_down(egui::Key::A)), "the key is up again");
}
