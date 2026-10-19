//! Fuzz target `request_line`: any bytes as one line a client sends on the control channel
//! (`control-protocol.md` §2–§4, `security.md` §6.5, §7.1), read as the transport reads every line
//! after `auth`. Whatever the input:
//!
//! - the line is refused with `parse` (over the line limit, not UTF-8, nested past the depth limit,
//!   not JSON, not an object, no string `method`), skipped when blank, or read as a request whose
//!   parameters nest no deeper than the limit;
//! - a request is handled by the running UI (the headless driver of an app with no library open, a
//!   stub file picker, the default automation settings): its parameters and limits are checked
//!   first, and an answer given up front is never `internal` (a handler that panicked); input it
//!   would play and work it would run off the UI thread are dropped unplayed, so nothing reaches
//!   the disk;
//! - every answer, refusal or reply, is one JSON line under the reply limit that echoes the id.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use std::cell::RefCell;

use dalica_ui::control::{
    Code, Config, Connection, Control, MAX_REPLY_LINE, MAX_REQUEST_DEPTH, MAX_REQUEST_LINE, Outcome, Player, Reply, Request, reply_line,
};
use dalica_ui::{App, Driver};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

struct World {
    control: Control,
    driver: Driver,
}

thread_local! {
    static WORLD: RefCell<Option<World>> = const { RefCell::new(None) };
}

fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(m) => 1 + m.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// The answer as the transport sends it: one JSON line under the limit, echoing `id`.
fn answer(id: &Value, reply: &Reply) {
    let line = reply_line(id, reply);
    assert!(line.len() <= MAX_REPLY_LINE);
    let (body, end) = line.split_at(line.len() - 1);
    assert_eq!(end, "\n");
    assert!(!body.contains('\n'), "a reply spans lines");
    let v: Value = serde_json::from_str(body).expect("a reply line is JSON");
    assert_eq!(v.get("id"), Some(id));
    assert_eq!(v.get("ok"), Some(&Value::Bool(reply.is_ok())));
}

fuzz_target!(|data: &[u8]| {
    let req = match Request::parse(data) {
        Err(e) => {
            assert_eq!(e.code, Code::Parse, "{e}");
            answer(&Value::Null, &Err(e));
            return;
        }
        Ok(None) => {
            let text = std::str::from_utf8(data).expect("a blank line is UTF-8");
            assert!(text.trim().is_empty());
            return;
        }
        Ok(Some(req)) => req,
    };
    assert!(data.len() <= MAX_REQUEST_LINE);
    assert!(depth(&req.params) < MAX_REQUEST_DEPTH, "the parameters nest past the limit");
    WORLD.with_borrow_mut(|world| {
        let w = world.get_or_insert_with(|| {
            let control = Control::new(Config::default());
            let driver = Driver::new(App::with_picker(None, control.picker()), 640, 480, 1.0);
            World { control, driver }
        });
        let mut conn = Connection::new();
        let outcome = conn.handle(&w.control, w.driver.as_host(), &req);
        if let Outcome::Reply(reply) = &outcome {
            if let Err(e) = reply {
                assert_ne!(e.code, Code::Internal, "{}: {e}", req.method);
            }
            answer(&req.id, reply);
        }
        drop(outcome);
        drop(conn.release());
    });
});
