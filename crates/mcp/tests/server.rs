//! The MCP server through the real framing (`mcp.md` §3, §4.1, §6, §10): newline-delimited
//! JSON-RPC on in-process pipes, a headless demo session behind it.

use std::path::Path;
use std::time::{Duration, Instant};

use dalica_mcp::{Backend, Headless, MAX_LINE, Progress, Rendered, SUPPORTED_VERSIONS, Source, ToolError};
use serde_json::{Value, json};

use crate::common::{Client, base64_decode, code, initialize, ok};

/// `initialize` answers each revision it speaks with that revision, and any other with its own;
/// the server names itself, its capabilities and how to use it.
#[test]
fn initialize_negotiates_the_protocol_revision() {
    let mut c = Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    for (n, asked) in (1..).zip(SUPPORTED_VERSIONS.iter().copied().chain(["2099-01-01"])) {
        let r = c.request(n, "initialize", json!({ "protocolVersion": asked, "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }));
        let want = if asked == "2099-01-01" { dalica_mcp::PROTOCOL_VERSION } else { asked };
        assert_eq!(r["result"]["protocolVersion"], want, "{r}");
        assert_eq!(r["result"]["serverInfo"]["name"], "dalica");
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], false, "headless: the tools never change");
        assert!(r["result"]["capabilities"]["resources"].is_object());
        let text = r["result"]["instructions"].as_str().unwrap();
        assert!(["command_list", "Photo Tray", "\"confirm\": true", "empty library"].iter().all(|w| text.contains(w)), "{text}");
    }
    assert_eq!(c.request(9, "ping", json!({}))["result"], json!({}));
    c.close();
}

/// The four shared tools on a demo session, each with its title, schema and four hints.
#[test]
fn the_shared_tools_work_on_a_demo_session() {
    let mut c = Client::demo();
    initialize(&mut c);
    let list = c.request(2, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    for name in ["command_list", "command_run", "command_batch", "render_preview"] {
        let t = tools.iter().find(|t| t["name"] == name).unwrap_or_else(|| panic!("{name} in {list}"));
        assert!(t["title"].is_string() && t["inputSchema"]["type"] == "object", "{t}");
        let hints = &t["annotations"];
        assert!(["readOnlyHint", "destructiveHint", "idempotentHint"].iter().all(|h| hints[h].is_boolean()), "{t}");
        assert_eq!(hints["openWorldHint"], false);
    }
    let read_only = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap()["annotations"]["readOnlyHint"].clone();
    assert_eq!((read_only("command_list"), read_only("render_preview"), read_only("command_run")), (json!(true), json!(true), json!(false)));

    // command_list: the registry, filtered.
    let all = c.call(3, "command_list", json!({}));
    let star = ok(&all)["commands"].as_array().unwrap().iter().find(|x| x["id"] == "photo.star.toggle").cloned().unwrap();
    assert_eq!((star["label"].as_str(), star["effect"].as_str()), (Some("Star / Unstar"), Some("library")));
    let some = c.call(4, "command_list", json!({ "filter": "STAR", "enabled_only": true }));
    let ids: Vec<&str> = ok(&some)["commands"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"photo.star.set") && ids.iter().all(|i| i.contains("star") || i.contains("Star")), "{ids:?}");

    // command_run: one command, its result; an error is a result with the protocol's code.
    let starred = c.call(5, "command_run", json!({ "id": "photo.star.set", "params": { "ids": [1, 2], "on": true } }));
    assert_eq!(ok(&starred)["on"], true);
    assert_eq!(starred["content"][0]["type"], "text");
    let shown = c.call(6, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [1] } }));
    assert_eq!(ok(&shown)["result"][0]["star"], true, "an array result is wrapped as {{result}}");
    assert_eq!(code(&c.call(7, "command_run", json!({ "id": "photo.nope" }))), "unknown_command");
    assert_eq!(code(&c.call(8, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [9999] } }))), "not_found");
    assert_eq!(code(&c.call(9, "command_run", json!({ "id": "library.rebuild" }))), "needs_confirm");

    // command_batch: in order, stopping at the first failure; over 256 steps runs nothing.
    let batch = c.call(
        10,
        "command_batch",
        json!({ "steps": [
        { "id": "photo.star.set", "params": { "ids": [3], "on": true } },
        { "id": "photo.nope" },
        { "id": "photo.star.set", "params": { "ids": [4], "on": true } },
    ] }),
    );
    assert_eq!(batch["isError"], true);
    let b = &batch["structuredContent"];
    assert_eq!((b["completed"].as_u64(), b["failed"].as_u64(), b["results"].as_array().map(Vec::len)), (Some(1), Some(1), Some(2)), "{b}");
    assert_eq!(b["results"][1]["code"], "unknown_command");
    let on = c.call(
        11,
        "command_batch",
        json!({ "steps": [{ "id": "photo.nope" }, { "id": "photo.star.set", "params": { "ids": [4], "on": true } }], "stop_on_error": false }),
    );
    assert_eq!(on["structuredContent"]["completed"], 1, "{on}");
    let many: Vec<Value> = (0..257).map(|_| json!({ "id": "photo.star.set", "params": { "ids": [5], "on": true } })).collect();
    assert_eq!(code(&c.call(12, "command_batch", json!({ "steps": many }))), "limit");
    let five = c.call(13, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [3, 4, 5] } }));
    let stars: Vec<bool> = ok(&five)["result"].as_array().unwrap().iter().map(|p| p["star"].as_bool().unwrap()).collect();
    assert_eq!(stars, [true, true, false], "the over-long batch ran no step");

    // render_preview: the photo with its edits, never enlarged, changing nothing.
    let before = c.call(14, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [1] } }));
    let wide = c.call(15, "render_preview", json!({ "id": 1, "max_side": 64 }));
    let img = &wide["content"][0];
    assert_eq!((img["type"].as_str(), img["mimeType"].as_str()), (Some("image"), Some("image/png")), "{wide}");
    let png = dalica_image::decode(&base64_decode(img["data"].as_str().unwrap()), &Default::default()).unwrap();
    let (w, h) = (ok(&wide)["width"].as_u64().unwrap(), ok(&wide)["height"].as_u64().unwrap());
    assert_eq!((u64::from(png.width), u64::from(png.height)), (w, h));
    assert!(w == 64 && (42..=43).contains(&h), "{w}×{h}");
    let after = c.call(16, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [1] } }));
    assert_eq!(ok(&after), ok(&before), "a render changes nothing");
    c.call(17, "command_run", json!({ "id": "photo.rotate.cw", "params": { "ids": [1] } }));
    let turned = c.call(18, "render_preview", json!({ "id": 1, "max_side": 64 }));
    assert_eq!((ok(&turned)["width"].as_u64(), ok(&turned)["height"].as_u64()), (Some(h), Some(64)), "the edits are rendered");
    assert_eq!(ok(&turned)["recipe"].as_array().map(Vec::len), Some(1), "{turned}");
    let full = c.call(19, "render_preview", json!({ "id": 1, "max_side": 2048 }));
    assert_eq!((ok(&full)["width"].as_u64(), ok(&full)["height"].as_u64()), (Some(320), Some(480)), "never enlarged");
    assert_eq!(code(&c.call(20, "render_preview", json!({ "id": 1, "max_side": 2049 }))), "limit");
    assert_eq!(code(&c.call(21, "render_preview", json!({}))), "disabled", "nothing in the tray");
    assert_eq!(code(&c.call(22, "render_preview", json!({ "id": 9999 }))), "not_found");
    c.close();
}

/// A helper's arguments are checked before anything runs: an unknown key is `-32602` naming it and
/// the keys accepted, as are a missing or ill-typed one; an unknown tool is `-32602` too.
#[test]
fn helper_arguments_are_strict() {
    let mut c = Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    let r = c.request(2, "tools/call", json!({ "name": "command_run", "arguments": { "id": "photo.star.set", "colour": "red" } }));
    assert_eq!(r["error"]["code"], -32602, "{r}");
    let m = r["error"]["message"].as_str().unwrap();
    assert!(m.contains("\"colour\"") && m.contains("id, params, wait"), "{m}");
    let missing = c.request(3, "tools/call", json!({ "name": "command_run", "arguments": {} }));
    assert_eq!(missing["error"]["code"], -32602);
    let typed = c.request(4, "tools/call", json!({ "name": "render_preview", "arguments": { "max_side": "big" } }));
    assert_eq!(typed["error"]["code"], -32602);
    let step = c.request(5, "tools/call", json!({ "name": "command_batch", "arguments": { "steps": [{ "id": "x", "command": "y" }] } }));
    assert!(step["error"]["message"].as_str().unwrap().contains("\"command\""), "{step}");
    let unknown = c.request(6, "tools/call", json!({ "name": "no_such_tool", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], -32602);
    c.close();
}

/// Lines that are not requests: a parse error answers `id: null` and the next line is read; an
/// over-long line is `-32700` and skipped; not a request `-32600`; an unknown method `-32601`; an
/// unknown resource `-32002`; notifications get no answer.
#[test]
fn bad_lines_get_protocol_errors_and_the_session_goes_on() {
    let mut c = Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    c.send_raw(b"{\"jsonrpc\": \"2.0\", \"id\": 1, \"method\": ");
    let parse = c.recv();
    assert_eq!((parse["id"].clone(), parse["error"]["code"].clone()), (Value::Null, json!(-32700)), "{parse}");
    assert_eq!(c.request(2, "ping", json!({}))["result"], json!({}), "the next line is read");

    let mut long = br#"{"jsonrpc":"2.0","id":3,"method":"ping","params":{"pad":""#.to_vec();
    long.resize(MAX_LINE + 16, b'a');
    long.extend_from_slice(br#""}}"#);
    c.send_raw(&long);
    let over = c.recv();
    assert_eq!((over["id"].clone(), over["error"]["code"].clone()), (Value::Null, json!(-32700)), "{over}");
    assert_eq!(c.request(4, "ping", json!({}))["result"], json!({}));

    c.send(&json!({ "jsonrpc": "2.0", "id": 5 }));
    assert_eq!(c.recv()["error"]["code"], -32600);
    c.send(&json!({ "jsonrpc": "1.0", "id": 6, "method": "ping" }));
    assert_eq!(c.recv()["error"]["code"], -32600);
    assert_eq!(c.request(7, "tools/unknown", json!({}))["error"]["code"], -32601);
    assert_eq!(c.request(8, "resources/read", json!({ "uri": "dalica://nothing" }))["error"]["code"], -32002);
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 99 } }));
    assert_eq!(c.request(9, "ping", json!({})), json!({ "jsonrpc": "2.0", "id": 9, "result": {} }), "no answer to the notifications");
    c.close();
}

/// The headless backend with one command planted to panic.
struct Planted(Headless);

impl Backend for Planted {
    fn call(&mut self, method: &str, params: Value, progress: &mut dyn Progress) -> Result<Value, ToolError> {
        if params["command"] == "test.boom" {
            panic!("planted");
        }
        self.0.call(method, params, progress)
    }

    fn render(&mut self, id: Option<u64>, max_side: u32) -> Result<Rendered, ToolError> {
        self.0.render(id, max_side)
    }

    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        "planted".into()
    }
}

/// A panic in a tool is `isError` with `code: "internal"`, and the session keeps serving.
#[test]
fn a_panic_in_a_tool_is_an_internal_error() {
    let mut c = Client::start(|| Box::new(Planted(Headless::open(Source::Demo, &[]).unwrap())));
    let boom = c.call(2, "command_run", json!({ "id": "test.boom" }));
    assert_eq!(code(&boom), "internal");
    assert!(boom["structuredContent"]["error"].as_str().unwrap().contains("planted"), "{boom}");
    let after = c.call(3, "command_run", json!({ "id": "photo.star.set", "params": { "ids": [1], "on": true } }));
    assert_eq!(ok(&after)["on"], true);
    c.close();
}

/// The photos of the job tests: 50, large enough that exporting them takes a while.
const JOB_PHOTOS: u64 = 50;
const JOB_SIDE: (u32, u32) = (1200, 800);

/// `export.run` of photos 1–50 into `dir` as a tools/call with a progress token.
fn export_call(id: u64, dir: &Path, token: &str) -> Value {
    let ids: Vec<u64> = (1..=50).collect();
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
        "name": "command_run",
        "arguments": { "id": "export.run", "params": { "ids": ids, "dir": dir, "folderName": "Out", "size": "original" } },
        "_meta": { "progressToken": token },
    } })
}

fn progress_of(v: &Value, token: &str) -> Option<(u64, u64)> {
    (v["method"] == "notifications/progress" && v["params"]["progressToken"] == token)
        .then(|| (v["params"]["progress"].as_u64().unwrap(), v["params"]["total"].as_u64().unwrap()))
}

/// A long job reports strictly increasing progress, at most ten notifications a second plus the
/// final one, and `ping` is answered while it runs.
#[test]
fn a_job_reports_progress_and_answers_ping() {
    let (photos, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut c = Client::photos(photos.path(), JOB_PHOTOS, JOB_SIDE.0, JOB_SIDE.1);
    initialize(&mut c);
    let start = Instant::now();
    c.send(&export_call(2, out.path(), "p"));
    let first = c.recv();
    assert!(progress_of(&first, "p").is_some(), "{first}");
    c.send(&json!({ "jsonrpc": "2.0", "id": "ping-1", "method": "ping" }));
    let (lines, reply) = c.until(&json!(2));
    let elapsed = start.elapsed();
    assert!(lines.iter().any(|l| l["id"] == "ping-1" && l["result"] == json!({})), "ping answered before the job's reply: {lines:?}");
    let steps: Vec<(u64, u64)> = std::iter::once(&first).chain(&lines).filter_map(|l| progress_of(l, "p")).collect();
    assert!(steps.windows(2).all(|w| w[0].0 < w[1].0), "strictly increasing: {steps:?}");
    assert_eq!(steps.last(), Some(&(50, 50)), "{steps:?}");
    let allowed = elapsed.as_secs_f64() * 10.0 + 2.0;
    assert!((steps.len() as f64) <= allowed, "{} notifications in {elapsed:?}", steps.len());
    println!("{} notifications in {elapsed:?}: {steps:?}", steps.len());
    let entry = ok(&reply["result"]);
    assert_eq!((entry["state"].as_str(), entry["result"]["written"].as_array().map(Vec::len)), (Some("done"), Some(50)), "{entry}");
    // No token, no notifications.
    let quiet = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "command_run",
        "arguments": { "id": "export.run", "params": { "ids": [1, 2], "dir": out.path(), "folderName": "Quiet" } } } });
    c.send(&quiet);
    let (lines, _) = c.until(&json!(3));
    assert!(lines.is_empty(), "{lines:?}");
    c.close();
}

/// Cancelling a job stops it before its next photo: no response, the photos done stay (each a
/// complete file), and the session goes on.
#[test]
fn a_cancel_stops_a_job_before_its_next_photo() {
    let (photos, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut c = Client::photos(photos.path(), JOB_PHOTOS, JOB_SIDE.0, JOB_SIDE.1);
    initialize(&mut c);
    c.send(&export_call(2, out.path(), "c"));
    let first = c.recv();
    assert!(progress_of(&first, "c").is_some_and(|(done, _)| done >= 1), "{first}");
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 2, "reason": "test" } }));
    c.send(&json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }));
    let (lines, _) = c.until(&json!(3));
    assert!(lines.iter().all(|l| l.get("id") != Some(&json!(2))), "a cancelled request gets no response: {lines:?}");
    assert!(c.try_recv(Duration::from_millis(300)).is_none());
    let files: Vec<_> = std::fs::read_dir(out.path().join("Out"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy() != ".dalica.json")
        .collect();
    println!("{} of 50 photos written before the cancel", files.len());
    assert!(!files.is_empty() && files.len() < 50, "{} of 50 written: {files:?}", files.len());
    for f in &files {
        let img = dalica_image::decode(&std::fs::read(f).unwrap(), &Default::default()).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert_eq!((img.width, img.height), JOB_SIDE, "{}", f.display());
    }
    assert_eq!(ok(&c.call(4, "command_run", json!({ "id": "photo.star.set", "params": { "ids": [1], "on": true } })))["on"], true);
    c.close();
}
