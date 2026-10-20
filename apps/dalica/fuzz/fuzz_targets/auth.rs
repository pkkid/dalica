//! Fuzz target `auth`: any bytes as what a client sends first on a control connection, against a
//! fixed token (`control-protocol.md` §2–§4, `security.md` §6.3, §6.5, §7.1), through the app's
//! own transport: its line framing ([`frame`], each read handing out as many bytes as the input's
//! second byte says) and the step each line takes ([`step`]). The first byte picks the line limit:
//! the transport's, or a small one so that lines reach it (the framing does not depend on its
//! size). Whatever the input:
//!
//! - lines are cut at each newline, whatever the reads hand out; a line over the limit (newline
//!   included), or an unterminated one that reaches it, is refused with `parse` and nothing after
//!   it is read;
//! - a blank line is skipped; a line that is not a request is answered `parse`, and closes;
//! - until a successful `auth` nothing is passed on: `auth` succeeds exactly when its `token` is the
//!   token, anything else is answered `unauthorized` and closes; after it, `auth` is still
//!   answered here (a wrong token closes) and every other request is passed on;
//! - every answer is one JSON line echoing the request's id, and never contains the token.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

// The app's own modules, compiled in from `src/control` (the transport names `super::token`).
#[allow(dead_code)]
#[path = "../../src/control/token.rs"]
mod token;
#[allow(dead_code)]
#[path = "../../src/control/transport.rs"]
mod transport;

use dalica_ui::control::{Code, MAX_REQUEST_LINE, Request, reply_line};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use token::Token;
use transport::{LineError, Step, frame, step, too_long};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The answer as the transport writes it: one JSON line echoing `id`, without the token.
fn answered(id: &Value, reply: &dalica_ui::control::Reply) {
    let line = reply_line(id, reply);
    let (body, end) = line.split_at(line.len() - 1);
    assert_eq!(end, "\n");
    assert!(!body.contains('\n'), "an answer spans lines");
    let v: Value = serde_json::from_str(body).expect("an answer is JSON");
    assert_eq!(v.get("id"), Some(id));
    assert_eq!(v.get("ok"), Some(&Value::Bool(reply.is_ok())));
    if let Err(e) = reply {
        assert!(!e.message.contains(TOKEN), "an answer shows the token");
    }
}

/// One line through the step; whether the connection stays open.
fn line(token: &Token, authed: &mut bool, line: &[u8]) -> bool {
    let before = *authed;
    let parsed = Request::parse(line);
    match step(token, authed, line) {
        Step::Skip => {
            assert!(matches!(parsed, Ok(None)), "a request was skipped");
            assert_eq!(*authed, before);
            true
        }
        Step::Forward(req) => {
            assert!(before, "a request was passed on before auth");
            assert!(*authed);
            assert_ne!(req.method, "auth", "auth was passed on");
            true
        }
        Step::Answer { id, reply, close } => {
            answered(&id, &reply);
            match parsed {
                Err(_) => {
                    assert_eq!((id, close, *authed), (Value::Null, true, before));
                    assert_eq!(reply.map_err(|e| e.code).unwrap_err(), Code::Parse);
                }
                Ok(None) => panic!("a blank line was answered"),
                Ok(Some(req)) => {
                    assert!(req.method == "auth" || !before, "an authenticated request was not passed on");
                    let right = req.method == "auth" && req.params.get("token").and_then(Value::as_str) == Some(TOKEN);
                    assert_eq!(reply.is_ok(), right, "auth's answer does not follow the token");
                    assert_eq!(close, !right);
                    if right {
                        assert!(*authed);
                    } else {
                        assert_eq!(*authed, before);
                        assert_eq!(reply.map_err(|e| e.code).unwrap_err(), Code::Unauthorized);
                    }
                }
            }
            !close
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let [limit, chunk, input @ ..] = data else { return };
    let max = if *limit == 0 { MAX_REQUEST_LINE } else { usize::from(*limit) };
    let chunk = usize::from(*chunk) + 1;
    let token = Token::from_file_text(TOKEN.as_bytes()).expect("a token");
    // The lines the input holds, for the framing's oracle.
    let mut expected = input.split_inclusive(|&b| b == b'\n');
    let mut authed = false;
    let mut current = Vec::new();
    let mut rest = input;
    loop {
        if rest.is_empty() {
            // The end of the stream: a last line without a newline is taken as it is.
            if !current.is_empty() {
                assert_eq!(expected.next(), Some(current.as_slice()));
                line(&token, &mut authed, &current);
            }
            assert_eq!(expected.next(), None);
            return;
        }
        let buf = rest.get(..chunk.min(rest.len())).unwrap_or(rest);
        match frame(current.len(), buf, max) {
            Err(e) => {
                assert_eq!(e, LineError::TooLong);
                let whole = expected.next().expect("a line is over the limit");
                let terminated = whole.ends_with(b"\n");
                assert!(whole.len() > max || (!terminated && whole.len() >= max), "a line within the limit was refused");
                answered(&Value::Null, &Err(too_long()));
                return;
            }
            Ok((take, done)) => {
                assert!(take > 0 && take <= buf.len());
                current.extend_from_slice(&buf[..take]);
                rest = &rest[take..];
                assert!(current.len() <= max);
                assert_eq!(done, current.ends_with(b"\n"));
                if done {
                    let whole = std::mem::take(&mut current);
                    assert_eq!(expected.next(), Some(whole.as_slice()));
                    if !line(&token, &mut authed, &whole) {
                        return;
                    }
                }
            }
        }
    }
});
