//! The hand-off's wire format and its info file (adapted from PhotoCraft
//! `apps/photocraft/src/single_instance.rs` @ d95fdf31, MIT OR Apache-2.0).
//!
//! - **The info file** `instance.json` in the settings folder: `{"port", "token", "pid"}`, written
//!   through `fs` atomically and private to the user (`0600` on Unix), read with a size cap.
//! - **The request**, one line: `{"token": "<64 hex>", "open": ["<absolute path>", …],
//!   "wait_ms": N}`, at most [`MAX_REQUEST`] bytes, [`MAX_PATHS`] paths of at most
//!   [`MAX_PATH_BYTES`] bytes each, every one absolute; anything else is refused whole.
//! - **The reply**, one line: `{"ok": true}` once the UI took the paths, else
//!   `{"ok": false, "error": "not taken"}`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

/// The info file in the settings folder.
pub const INFO_FILE: &str = "instance.json";
/// The most paths one launch hands over.
pub const MAX_PATHS: usize = 64;
/// The longest path handed over (bytes of UTF-8).
pub const MAX_PATH_BYTES: usize = 4096;
/// The longest request line (newline included): every path at its longest, each byte escaped.
pub const MAX_REQUEST: usize = MAX_PATHS * MAX_PATH_BYTES * 2 + 1024;
/// The longest reply line.
pub const MAX_REPLY: usize = 256;
/// The longest info file read.
const MAX_INFO: u64 = 4096;
/// Hex characters in a token (256 bits).
const TOKEN_HEX: usize = 64;

/// The hand-off's token. `Debug` never shows it, and it is never logged.
pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl Secret {
    /// 256 fresh bits from the OS random source.
    pub fn generate() -> Result<Secret, String> {
        let mut bytes = [0u8; TOKEN_HEX / 2];
        getrandom::fill(&mut bytes).map_err(|e| format!("the OS random source failed: {e}"))?;
        Ok(Secret(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Whether `given` is this token, in a time that depends only on the token's length.
    pub fn matches(&self, given: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), given.as_bytes());
        let mut diff = u8::from(a.len() != b.len());
        for (i, x) in a.iter().enumerate() {
            diff |= x ^ b.get(i).copied().unwrap_or(0);
        }
        std::hint::black_box(diff) == 0
    }

    #[cfg(test)]
    pub fn reveal_for_test(&self) -> &str {
        &self.0
    }
}

/// Writes the port and the token to the info file in `dir`, replacing the last one (the lock is
/// held, so no other Dalica writes it).
pub fn write_info(dir: &Path, port: u16, token: &Secret) -> Result<(), String> {
    let path = dir.join(INFO_FILE);
    let body = json!({ "port": port, "token": token.0, "pid": std::process::id() }).to_string();
    let expect = match dalica_fs::identity(&path) {
        Ok(Some(id)) => dalica_fs::Expect::Unchanged(id),
        Ok(None) => dalica_fs::Expect::Absent,
        Err(e) => return Err(e.to_string()),
    };
    dalica_fs::replace_private(&path, body.as_bytes(), expect).map(|_| ()).map_err(|e| e.to_string())
}

/// The port and token of the running Dalica, when the info file in `dir` holds them.
pub fn read_info(dir: &Path) -> Option<(u16, String)> {
    let read = dalica_fs::read(&dir.join(INFO_FILE), MAX_INFO).ok()??;
    let v: Value = serde_json::from_slice(&read.bytes).ok()?;
    let port = u16::try_from(v.get("port")?.as_u64()?).ok().filter(|p| *p != 0)?;
    let token = v.get("token")?.as_str()?;
    (token.len() == TOKEN_HEX && token.bytes().all(|b| b.is_ascii_hexdigit())).then(|| (port, token.to_string()))
}

/// A later launch's paths, checked to fit the request.
#[derive(Debug)]
pub struct Request {
    open: Vec<String>,
}

impl Request {
    /// `paths` as the request carries them; `Err` when they would be refused (see the module docs).
    pub fn new(paths: &[PathBuf]) -> Result<Request, String> {
        if paths.len() > MAX_PATHS {
            return Err(format!("more than {MAX_PATHS} files"));
        }
        let mut open = Vec::with_capacity(paths.len());
        for p in paths {
            let s = p.to_str().ok_or("a file name is not valid Unicode")?;
            if s.len() > MAX_PATH_BYTES || !p.is_absolute() {
                return Err(format!("{} cannot be handed over", p.display()));
            }
            open.push(s.to_string());
        }
        let r = Request { open };
        let longest = r.line(&"0".repeat(TOKEN_HEX), Duration::MAX).len().saturating_add(1);
        if longest > MAX_REQUEST {
            return Err(format!("the file names are longer than {MAX_REQUEST} bytes together"));
        }
        Ok(r)
    }

    /// The request line (without its newline), saying the sender waits `wait` for the UI.
    pub fn line(&self, token: &str, wait: Duration) -> String {
        let wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        json!({ "token": token, "open": self.open, "wait_ms": wait_ms }).to_string()
    }
}

/// The paths of a request line and how long its sender waits; `Err` says why it is refused.
pub fn parse_request(line: &[u8], token: &Secret) -> Result<(Vec<PathBuf>, Duration), String> {
    if line.len() > MAX_REQUEST {
        return Err("too long".into());
    }
    let v: Value = serde_json::from_slice(line).map_err(|_| "not JSON".to_string())?;
    let given = v.get("token").and_then(Value::as_str).ok_or("no token")?;
    if !token.matches(given) {
        return Err("a wrong token".into());
    }
    let open = v.get("open").and_then(Value::as_array).ok_or("no `open` list")?;
    if open.len() > MAX_PATHS {
        return Err(format!("{} paths, more than {MAX_PATHS}", open.len()));
    }
    let mut paths = Vec::with_capacity(open.len());
    for p in open {
        let s = p.as_str().ok_or("a path that is not a string")?;
        if s.len() > MAX_PATH_BYTES {
            return Err(format!("a path longer than {MAX_PATH_BYTES} bytes"));
        }
        let path = PathBuf::from(s);
        if !path.is_absolute() {
            return Err("a path that is not absolute".into());
        }
        paths.push(path);
    }
    let wait = Duration::from_millis(v.get("wait_ms").and_then(Value::as_u64).unwrap_or(0));
    Ok((paths, wait))
}

/// The reply line (without its newline).
pub fn reply(taken: bool) -> String {
    if taken { json!({ "ok": true }) } else { json!({ "ok": false, "error": "not taken" }) }.to_string()
}

/// Whether a reply line says the paths were taken.
pub fn is_taken(line: &[u8]) -> bool {
    serde_json::from_slice::<Value>(line).ok().and_then(|v| v.get("ok")?.as_bool()) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn a_request_carries_absolute_paths_and_the_token() {
        let token = Secret::generate().unwrap();
        let paths = vec![abs("one.jpg"), abs("två é 写真.png")];
        let line = Request::new(&paths).unwrap().line(token.reveal_for_test(), Duration::from_millis(250));
        assert_eq!(parse_request(line.as_bytes(), &token), Ok((paths, Duration::from_millis(250))));
        assert_eq!(
            parse_request(Request::new(&[]).unwrap().line(token.reveal_for_test(), Duration::ZERO).as_bytes(), &token),
            Ok((vec![], Duration::ZERO))
        );
        assert!(!format!("{token:?}").contains(token.reveal_for_test()), "Debug hides the token");
    }

    #[test]
    fn a_request_without_the_token_malformed_or_too_large_is_refused() {
        let token = Secret::generate().unwrap();
        let t = token.reveal_for_test();
        let wrong = Secret::generate().unwrap();
        let ok = |open: Vec<String>| json!({ "token": t, "open": open }).to_string();
        assert!(parse_request(json!({ "token": wrong.reveal_for_test(), "open": [] }).to_string().as_bytes(), &token).is_err());
        let a = abs("a.jpg").to_string_lossy().into_owned();
        for bad in [
            String::new(),
            "not json".into(),
            "{}".into(),
            r#"{"token":5}"#.into(),
            json!({ "token": t }).to_string(),
            json!({ "token": t, "open": [1] }).to_string(),
            json!({ "token": &t[1..], "open": [] }).to_string(),
            json!({ "token": format!("{t}0"), "open": [] }).to_string(),
            ok(vec!["relative.jpg".into()]),
            ok(vec![format!("{a}{}", "x".repeat(MAX_PATH_BYTES))]),
            ok(vec![a.clone(); MAX_PATHS + 1]),
        ] {
            assert!(parse_request(bad.as_bytes(), &token).is_err(), "{bad:.80}");
        }
        assert!(parse_request(ok(vec![a.clone(); MAX_PATHS]).as_bytes(), &token).is_ok());
        assert!(parse_request(&vec![b' '; MAX_REQUEST + 1], &token).is_err());
        // The sender refuses what the receiver would.
        assert!(Request::new(&vec![abs("a.jpg"); MAX_PATHS + 1]).is_err());
        assert!(Request::new(&[PathBuf::from("relative.jpg")]).is_err());
        assert!(Request::new(&[abs(&"x".repeat(MAX_PATH_BYTES))]).is_err());
        let longest: Vec<PathBuf> = (0..MAX_PATHS).map(|i| abs(&format!("{i:02}{}", "\"".repeat(MAX_PATH_BYTES - 200)))).collect();
        assert!(Request::new(&longest).is_ok(), "every path at its longest, escaped, still fits");
    }

    #[test]
    fn the_reply_says_whether_the_paths_were_taken() {
        assert!(is_taken(reply(true).as_bytes()));
        assert!(!is_taken(reply(false).as_bytes()));
        assert!(!is_taken(b"garbage"));
    }

    #[test]
    fn the_info_file_is_written_through_fs_private_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_info(dir.path()), None);
        let token = Secret::generate().unwrap();
        write_info(dir.path(), 4242, &token).unwrap();
        assert_eq!(read_info(dir.path()), Some((4242, token.reveal_for_test().to_string())));
        // Replaced by the next first Dalica.
        let next = Secret::generate().unwrap();
        write_info(dir.path(), 4243, &next).unwrap();
        assert_eq!(read_info(dir.path()), Some((4243, next.reveal_for_test().to_string())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(INFO_FILE)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // Damaged or hostile contents are not a running Dalica.
        for bad in [&b"{}"[..], b"not json", br#"{"port":0,"token":"00"}"#, br#"{"port":70000,"token":"00"}"#, br#"{"port":1,"token":"zz"}"#] {
            std::fs::write(dir.path().join(INFO_FILE), bad).unwrap();
            assert_eq!(read_info(dir.path()), None, "{}", String::from_utf8_lossy(bad));
        }
    }
}
