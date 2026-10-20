//! Fuzz target `handoff`: any bytes as the line a later launch sends the running Dalica
//! (`viewing.md` §3.4, `security.md` §2.3), through the app's own parser (`instance/wire.rs`'s
//! [`parse_request`]) against a fixed token, and as the reply line the later launch reads
//! ([`is_taken`]). Whatever the input:
//!
//! - the line is refused with a reason, or it carried the token, at most [`MAX_PATHS`] paths, each
//!   absolute and at most [`MAX_PATH_BYTES`] bytes, and a line over [`MAX_REQUEST`] is never read;
//! - a request the sender can make of those paths ([`Request::new`]) reads back as the same paths
//!   and wait; with another token, it is refused;
//! - a reply is taken exactly when it is a JSON object whose `ok` is `true`.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

// The app's own module, compiled in from `src/instance`.
#[allow(dead_code)]
#[path = "../../src/instance/wire.rs"]
mod wire;

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use wire::{MAX_PATH_BYTES, MAX_PATHS, MAX_REQUEST, Request, Secret, is_taken, parse_request};

/// Two tokens and the first one's text, read back as a later launch reads it: through the info
/// file (written once, in a folder of the temporary directory).
fn tokens() -> &'static (Secret, String, Secret) {
    static TOKENS: OnceLock<(Secret, String, Secret)> = OnceLock::new();
    TOKENS.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dalica-handoff-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder for the info file");
        let token = Secret::generate().expect("a token");
        wire::write_info(&dir, 1, &token).expect("the info file");
        let (_, text) = wire::read_info(&dir).expect("the info file reads back");
        let _ = std::fs::remove_dir_all(&dir);
        (token, text, Secret::generate().expect("a second token"))
    })
}

fuzz_target!(|data: &[u8]| {
    let (token, text, other) = tokens();
    // The first byte, when it is `!`, puts the token in: `{"token": "<the token>", ` then the rest,
    // so lines reach past the token check.
    let line: Vec<u8> = match data.split_first() {
        Some((b'!', rest)) => [format!("{{\"token\":\"{text}\",").as_bytes(), rest].concat(),
        _ => data.to_vec(),
    };
    let parsed = parse_request(&line, token);
    if line.len() > MAX_REQUEST {
        assert!(parsed.is_err(), "a line over the limit was read");
    }
    match parsed {
        Err(why) => assert!(!why.is_empty(), "refused without a reason"),
        Ok((paths, wait)) => {
            let v: Value = serde_json::from_slice(&line).expect("an accepted line is JSON");
            assert_eq!(v.get("token").and_then(Value::as_str), Some(text.as_str()), "accepted without the token");
            assert!(paths.len() <= MAX_PATHS);
            for p in &paths {
                assert!(p.is_absolute(), "a relative path was accepted");
                assert!(p.to_str().is_some_and(|s| s.len() <= MAX_PATH_BYTES));
            }
            if let Ok(request) = Request::new(&paths) {
                let again = request.line(text, wait);
                assert!(again.len() < MAX_REQUEST);
                assert_eq!(parse_request(again.as_bytes(), token), Ok((paths.clone(), wait)));
                assert!(parse_request(again.as_bytes(), other).is_err(), "another token was accepted");
            }
        }
    }
    // The same bytes as the reply a later launch reads.
    let taken = serde_json::from_slice::<Value>(data).ok().and_then(|v| v.get("ok").and_then(Value::as_bool)) == Some(true);
    assert_eq!(is_taken(data), taken);
});
