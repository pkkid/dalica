//! The MCP client's roots and the launch's permissions (`roadmap.md` M10.16; `mcp.md` §5): the
//! server asks `roots/list` after `initialized` and on `notifications/roots/list_changed`; a client
//! root becomes a read root and never a write root; a refresh drops a removed root; an answer to an
//! older request is ignored; `--no-client-roots` and a client without the capability are never
//! asked; `--read-only` reaches the engine. Driven through [`Server::handle_line`].

use std::path::{Path, PathBuf};

use dalica_engine::automation::{Policy, plain};
use dalica_mcp::roots::{client_roots, file_uri_path};
use dalica_mcp::{Headless, Server, Source};
use serde_json::{Value, json};

/// The `file://` URI of `path`, spaces and percent signs escaped (`file:///C:/x` on Windows).
fn uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/").replace('%', "%25").replace(' ', "%20");
    if text.starts_with('/') { format!("file://{text}") } else { format!("file:///{text}") }
}

/// A scratch tree: two folders a client may list (`one`, `two`), and `out`, the only write root.
struct Tree {
    _dir: tempfile::TempDir,
    one: PathBuf,
    two: PathBuf,
    out: PathBuf,
}

fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    // Resolved and plain (not `\\?\C:\…`): a path an agent may give, and one a `file://` URI names.
    let root = plain(&std::fs::canonicalize(dir.path()).unwrap());
    let t = Tree { one: root.join("one folder"), two: root.join("two"), out: root.join("out"), _dir: dir };
    for d in [&t.one, &t.two, &t.out] {
        std::fs::create_dir_all(d).unwrap();
    }
    t
}

fn server(t: &Tree, ask: bool) -> Server {
    let mut policy = Policy::default();
    policy.add_write(t.out.clone());
    Server::new(Box::new(Headless::open_with(Source::Empty, &[], policy, false).unwrap())).with_client_roots(ask)
}

fn line(s: &mut Server, msg: Value) -> Option<Value> {
    s.handle_line(&msg.to_string()).map(|l| serde_json::from_str(&l).unwrap())
}

/// `initialize` (with the `roots` capability when `roots`), then `initialized`: what the server
/// sent in answer to the notification.
fn start(s: &mut Server, roots: bool) -> Option<Value> {
    let caps = if roots { json!({ "roots": { "listChanged": true } }) } else { json!({}) };
    let init =
        line(s, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": caps } }));
    assert!(init.unwrap()["result"]["serverInfo"].is_object());
    line(s, json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
}

/// The id of a `roots/list` request the server sent.
fn roots_request(sent: Option<Value>) -> Value {
    let r = sent.expect("the server asks for the roots");
    assert_eq!((r["jsonrpc"].as_str(), r["method"].as_str()), (Some("2.0"), Some("roots/list")), "{r}");
    r["id"].clone()
}

fn answer(s: &mut Server, id: &Value, folders: &[&Path]) {
    let roots: Vec<Value> = folders.iter().map(|f| json!({ "uri": uri(f), "name": "a folder" })).collect();
    assert_eq!(line(s, json!({ "jsonrpc": "2.0", "id": id, "result": { "roots": roots } })), None, "nothing answers a response");
}

/// The code a tool call failed with, `"ok"` when it did not.
fn tool(s: &mut Server, name: &str, args: Value) -> String {
    let r = line(s, json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": name, "arguments": args } })).unwrap();
    let result = &r["result"];
    if result["isError"] == json!(true) { result["structuredContent"]["code"].as_str().unwrap().to_string() } else { "ok".into() }
}

fn scan(s: &mut Server, folder: &Path) -> String {
    tool(s, "scan_folder", json!({ "path": folder }))
}

/// An export into `folder`: refused for its path before anything runs, or refused for having no
/// photo once the path passed.
fn export_into(s: &mut Server, folder: &Path) -> String {
    tool(s, "command_run", json!({ "id": "export.run", "params": { "ids": [1], "dir": folder, "folderName": "x" } }))
}

#[test]
fn a_client_root_is_a_read_root_never_a_write_root_and_a_refresh_drops_a_removed_one() {
    let t = tree();
    let mut s = server(&t, true);
    assert_eq!(scan(&mut s, &t.one), "path_denied", "no root before the client lists any");
    let first = roots_request(start(&mut s, true));
    answer(&mut s, &first, &[&t.one, &t.two]);
    assert_eq!(scan(&mut s, &t.one), "ok", "a client root is a read root");
    // (`two` is listed but not watched: an export may go into a watched folder.)
    assert_eq!(export_into(&mut s, &t.two), "path_denied", "a client root is never a write root");
    assert_ne!(export_into(&mut s, &t.out), "path_denied", "the write root given");

    let second = roots_request(line(&mut s, json!({ "jsonrpc": "2.0", "method": "notifications/roots/list_changed" })));
    assert_ne!(first, second);
    answer(&mut s, &second, &[&t.one]);
    assert_eq!(scan(&mut s, &t.two), "path_denied", "the refresh dropped the removed root");
    // A late answer to the first request changes nothing.
    answer(&mut s, &first, &[&t.one, &t.two]);
    assert_eq!(scan(&mut s, &t.two), "path_denied");
    // An error answer to the latest request drops every client root (fail closed).
    let third = roots_request(line(&mut s, json!({ "jsonrpc": "2.0", "method": "notifications/roots/list_changed" })));
    line(&mut s, json!({ "jsonrpc": "2.0", "id": third, "error": { "code": -32601, "message": "no roots" } }));
    assert_eq!(scan(&mut s, &t.two), "path_denied");
    let inside = t.one.join("2026-07");
    std::fs::create_dir_all(&inside).unwrap();
    assert_eq!(scan(&mut s, &inside), "ok", "a folder already watched stays readable");
}

#[test]
fn a_client_is_asked_only_when_it_has_roots_and_no_client_roots_was_not_given() {
    let t = tree();
    assert_eq!(start(&mut server(&t, true), false), None, "no roots capability");
    let mut s = server(&t, false);
    assert_eq!(start(&mut s, true), None, "--no-client-roots");
    assert_eq!(line(&mut s, json!({ "jsonrpc": "2.0", "method": "notifications/roots/list_changed" })), None);
}

#[test]
fn only_local_file_uris_become_roots() {
    let t = tree();
    let listed = json!({ "roots": [
        { "uri": uri(&t.one) },
        { "uri": "https://example.com/photos" },
        { "uri": "file://nas/share/photos" },
        { "uri": "file:///photos/../etc" },
        { "uri": "file:///bad%zzescape" },
        { "uri": "file:///photos?x=1" },
        { "name": "no uri" },
    ] });
    assert_eq!(client_roots(&listed), vec![t.one.clone()]);
    assert_eq!(client_roots(&json!({ "roots": 3 })), Vec::<PathBuf>::new());
    #[cfg(unix)]
    {
        assert_eq!(file_uri_path("file://localhost/srv/My%20Photos"), Some(PathBuf::from("/srv/My Photos")));
        assert_eq!(file_uri_path("FILE:///srv/photos"), Some(PathBuf::from("/srv/photos")));
        assert_eq!(file_uri_path("file:///srv/a%00b"), None, "a NUL");
    }
    #[cfg(windows)]
    {
        // A plain local path, separators and all, whatever the drive letter's case.
        assert_eq!(file_uri_path("file:///C:/My%20Photos").unwrap().as_os_str(), std::ffi::OsStr::new(r"C:\My Photos"));
        assert_eq!(file_uri_path("file:///c:/Photos/2026").unwrap().as_os_str(), std::ffi::OsStr::new(r"C:\Photos\2026"));
        assert_eq!(file_uri_path("file:///srv/photos"), None, "no drive");
    }
}

#[test]
fn a_read_only_launch_refuses_an_export_with_read_only() {
    let t = tree();
    let mut policy = Policy::default();
    policy.add_write(t.out.clone());
    let h = Headless::open_with(Source::Empty, &[], policy, true).unwrap();
    assert!(h.session().is_read_only());
    let mut s = Server::new(Box::new(h));
    start(&mut s, false);
    assert_eq!(export_into(&mut s, &t.out), "read_only");
}
