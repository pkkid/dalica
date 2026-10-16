//! `batch {steps, stopOnError?}` (`control-protocol.md` §6.3): several command and method steps in
//! one request, in order, with the semantics of separate requests. Its entry of
//! [`METHODS`](super::METHODS) is [`METHOD`], so building it changes only this module.
//!
//! Each step is handled as its own request would be, and its outcome is chained to the next step:
//! a step's input has played (and its work has run off the UI thread) before the next one starts.
//! Every command step is its own undo step (a batch is not a transaction). Everything about the
//! batch itself is checked before any step runs: the step count, each step's shape, and the
//! methods that may not be steps (`batch`, `auth`, `app.quit`).
//!
//! The steps run in continuations, which hold no borrow of the connection: they run on a copy of
//! it, and a key a step holds down (`ui.key {state: down}`) is let go when the batch ends, so a
//! connection never leaves a key down that it does not know of.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::engine::{self, Wait};
use super::host::{Host, Input};
use super::{Call, Code, Connection, Control, Error, MAX_REPLY_LINE, METHODS, Method, Next, Outcome, Reply, guard, internal, method};
use crate::headless::KeyState;

/// The method's entry in [`METHODS`](super::METHODS).
pub(super) const METHOD: Method = method("batch", "{steps, stopOnError?}", "{completed, failed, results} (§6.3)", run);

/// The most steps one batch may have (§4).
pub const MAX_STEPS: usize = 256;

/// The bytes the step results may take: the reply limit, less room for the reply's own fields.
const BUDGET: usize = MAX_REPLY_LINE - 4096;

/// Methods that are never steps: a batch in a batch, the transport's `auth`, and quitting.
const NOT_STEPS: &[&str] = &["batch", "auth", "app.quit"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Params {
    steps: Vec<Value>,
    stop_on_error: Option<bool>,
}

/// One step, checked.
enum Step {
    /// `{command, params?, wait?}`: `engine.execute` with these params.
    Command(Value),
    /// `{method, params?}`.
    Method(String, Value),
}

/// A batch on its way: the steps still to run and what the finished ones replied.
struct Batch {
    steps: VecDeque<Step>,
    stop_on_error: bool,
    results: Vec<Value>,
    completed: u64,
    failed: u64,
    /// The bytes of `results`, serialized.
    size: usize,
    control: Control,
    conn: Connection,
    /// The keys the connection held when the batch started.
    held_before: Vec<egui::Key>,
    /// When every command step's wait for its job ends (one deadline for the batch, §5.3).
    deadline: Instant,
}

/// `batch {steps, stopOnError?}`.
fn run(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Params = call.params(params)?;
    if p.steps.len() > MAX_STEPS {
        return Err(Error::new(Code::Limit, format!("at most {MAX_STEPS} steps in one batch, not {}", p.steps.len())));
    }
    let steps = p.steps.iter().enumerate().map(|(i, s)| step(call, i, s)).collect::<Result<VecDeque<_>, _>>()?;
    let batch = Batch {
        steps,
        stop_on_error: p.stop_on_error.unwrap_or(true),
        results: Vec::new(),
        completed: 0,
        failed: 0,
        size: 0,
        control: Control { config: call.control.config.clone(), picker: call.control.picker.clone() },
        conn: Connection { held: call.conn.held.clone() },
        held_before: call.conn.held(),
        deadline: Wait::deadline_from_now(),
    };
    Ok(next(batch, call.host))
}

/// Step `i` of the batch, checked: an object with `command` or `method`, and only its keys.
fn step(call: &Call<'_>, i: usize, value: &Value) -> Result<Step, Error> {
    let Value::Object(o) = value else {
        return Err(call.bad(format!("step {i} must be an object ({{command, params?, wait?}} or {{method, params?}}), not {value}")));
    };
    let keys = |allowed: &[&str]| match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(call.bad(format!("step {i} has an unknown key `{k}` (it takes {})", allowed.join(", ")))),
        None => Ok(()),
    };
    match (o.get("command"), o.get("method")) {
        (Some(_), Some(_)) => Err(call.bad(format!("step {i} gives both `command` and `method`; give one"))),
        (Some(_), None) => {
            keys(&["command", "params", "wait"])?;
            Ok(Step::Command(value.clone()))
        }
        (None, Some(Value::String(name))) => {
            keys(&["method", "params"])?;
            if NOT_STEPS.contains(&name.as_str()) {
                return Err(call.bad(format!("step {i}: `{name}` cannot be a step of a batch")));
            }
            Ok(Step::Method(name.clone(), o.get("params").cloned().unwrap_or(Value::Null)))
        }
        (None, Some(other)) => Err(call.bad(format!("step {i}: `method` must be a method name, not {other}"))),
        (None, None) => Err(call.bad(format!("step {i} needs `command` or `method`"))),
    }
}

/// The next step's outcome, chained to the one after it; the batch's reply when none is left.
fn next(mut b: Batch, host: &mut dyn Host) -> Outcome {
    let Some(step) = b.steps.pop_front() else { return finish(b) };
    let outcome = guard(|| dispatch(&mut b, host, step));
    chain(host, outcome, Box::new(move |host, reply| record(b, host, reply)))
}

/// Handles one step as the request it stands for.
fn dispatch(b: &mut Batch, host: &mut dyn Host, step: Step) -> Outcome {
    let (name, params) = match step {
        Step::Command(params) => ("engine.execute".to_string(), params),
        Step::Method(name, params) => (name, params),
    };
    let Some(m) = METHODS.iter().find(|m| m.name == name) else {
        return Error::new(Code::UnknownMethod, format!("no method `{name}` (`methods` lists them)")).into();
    };
    let mut call = Call { host, control: &b.control, conn: &mut b.conn, method: m };
    let outcome = if m.name == "engine.execute" {
        engine::execute_until(&mut call, &params, b.deadline)
    } else {
        match m.run {
            None => Err(Error::new(Code::Unsupported, format!("`{}` is not built yet ({})", m.name, m.planned))),
            Some(run) => run(&mut call, &params),
        }
    };
    outcome.unwrap_or_else(Outcome::from)
}

/// What a step does once it has its reply.
type Done = Box<dyn FnOnce(&mut dyn Host, Reply) -> Outcome + Send>;

/// `outcome`, played as it would be, with `done` given its reply instead of the client.
fn chain(host: &mut dyn Host, outcome: Outcome, done: Done) -> Outcome {
    match outcome {
        Outcome::Reply(r) => done(host, r),
        Outcome::Play(input, then) => Outcome::Play(
            input,
            Box::new(move |host, played| {
                let o = guard(|| then(host, played));
                chain(host, o, done)
            }),
        ),
        Outcome::Work(work) => Outcome::Work(Box::new(move || {
            let next = catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|panic| Next::Reply(Err(internal(&*panic))));
            match next {
                Next::Reply(r) => Next::Resume(Box::new(move |host| done(host, r))),
                Next::Resume(k) => Next::Resume(Box::new(move |host| {
                    let o = guard(|| k(host));
                    chain(host, o, done)
                })),
            }
        })),
    }
}

/// A step's entry in `results`: `{ok, result}` or `{ok, code, error, prompt?}`.
fn entry(reply: Reply) -> Value {
    match reply {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(e) => {
            let mut v = Map::new();
            v.insert("ok".into(), Value::Bool(false));
            v.insert("code".into(), Value::String(e.code.as_str().into()));
            v.insert("error".into(), Value::String(e.message));
            if let Some(prompt) = e.prompt {
                v.insert("prompt".into(), prompt);
            }
            Value::Object(v)
        }
    }
}

/// Records a step's reply, then goes on (or stops: at a failure with `stopOnError`, or when the
/// results would not fit in a reply).
fn record(mut b: Batch, host: &mut dyn Host, reply: Reply) -> Outcome {
    let ok = reply.is_ok();
    let entry = entry(reply);
    // The entry and the comma after it.
    let len = serde_json::to_string(&entry).map_or(usize::MAX, |s| s.len()).saturating_add(1);
    if b.size.saturating_add(len) > BUDGET {
        let step = b.results.len();
        b.failed = b.failed.saturating_add(1);
        b.results.push(json!({
            "ok": false,
            "code": Code::Limit.as_str(),
            "error": format!("the batch's reply would pass {MAX_REPLY_LINE} bytes: stopped at step {step}, which ran (its reply is left out)"),
        }));
        return finish(b);
    }
    b.size = b.size.saturating_add(len);
    b.results.push(entry);
    if ok {
        b.completed = b.completed.saturating_add(1);
    } else {
        b.failed = b.failed.saturating_add(1);
        if b.stop_on_error {
            return finish(b);
        }
    }
    next(b, host)
}

/// Lets go of the keys the batch's steps left down (one at a time, as `ui.key {state: up}` would),
/// then replies `{completed, failed, results}`.
fn finish(mut b: Batch) -> Outcome {
    if let Some(pos) = b.conn.held.iter().position(|(k, _)| !b.held_before.contains(k)) {
        let (key, _) = b.conn.held.remove(pos);
        let up = Input::Key { key, modifiers: egui::Modifiers::NONE, state: KeyState::Up };
        return Outcome::Play(up, Box::new(move |_, _| finish(b)));
    }
    Outcome::Reply(Ok(json!({ "completed": b.completed, "failed": b.failed, "results": b.results })))
}
