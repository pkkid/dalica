//! One tool per command (`mcp.md` §4.4): generated from the registry, its hints derived from the
//! command's effect and confirmation, run through `engine.execute`; `--compact` leaves them out.
//! What each command does is the engine's tests'; here only the generation and the wiring, once
//! over the whole registry.

use std::collections::{BTreeMap, BTreeSet};

use dalica_mcp::{Backend, Headless, Progress, Rendered, Server, Source, ToolError};
use serde_json::{Map, Value, json};

use crate::common::{Client, code, initialize, ok};

/// The tool name of command `id`.
fn name(id: &str) -> String {
    format!("cmd_{}", id.replace('.', "_"))
}

/// `tools/list`, by name.
fn tools(c: &mut Client, id: u64) -> BTreeMap<String, Value> {
    let r = c.request(id, "tools/list", json!({}));
    r["result"]["tools"].as_array().unwrap().iter().map(|t| (t["name"].as_str().unwrap().to_string(), t.clone())).collect()
}

/// The registry, as `command_list` gives it.
fn commands(c: &mut Client, id: u64) -> Vec<Value> {
    ok(&c.call(id, "command_list", json!({})))["commands"].as_array().unwrap().clone()
}

/// Photo `id` as `photo.inspect` gives it.
fn photo(c: &mut Client, call: u64, id: u64) -> Value {
    ok(&c.call(call, "command_run", json!({ "id": "photo.inspect", "params": { "ids": [id] } })))["result"][0].clone()
}

/// One `cmd_` tool per registry entry, named from its id, titled by its label, described by its
/// id, menu path and params doc, taking a loose object; every `Query` is `readOnlyHint` and
/// `idempotentHint`, nothing else is read-only, an `External` command is destructive, and no tool
/// reaches the open world.
#[test]
fn one_tool_per_registry_entry() {
    let mut c = Client::demo();
    initialize(&mut c);
    let listed = tools(&mut c, 2);
    let registry = commands(&mut c, 3);
    let generated: Vec<&String> = listed.keys().filter(|n| n.starts_with("cmd_")).collect();
    assert_eq!(generated.len(), registry.len(), "one tool per command");
    for cmd in &registry {
        let id = cmd["id"].as_str().unwrap();
        let tool = listed.get(&name(id)).unwrap_or_else(|| panic!("no tool for {id}"));
        let description = tool["description"].as_str().unwrap();
        assert_eq!(tool["title"], cmd["label"], "{id}");
        assert!(description.contains(&format!("`{id}`")) && description.contains(cmd["params"].as_str().unwrap()), "{id}: {description}");
        if let Some(menu) = cmd["menu"].as_str().filter(|m| !m.is_empty()) {
            let last = menu.rsplit('/').next().unwrap();
            assert!(description.contains("Menu: ") && description.contains(last), "{id}: {description}");
        }
        assert_eq!(tool["inputSchema"]["type"], "object", "{id}");
        assert_eq!(tool["inputSchema"]["additionalProperties"], true, "{id}");
        let hints = &tool["annotations"];
        let query = cmd["effect"] == "query";
        assert_eq!(hints["readOnlyHint"], query, "{id}");
        assert_eq!(hints["openWorldHint"], false, "{id}");
        if query {
            assert_eq!((&hints["idempotentHint"], &hints["destructiveHint"]), (&json!(true), &json!(false)), "{id}");
        }
        if cmd["effect"] == "external" {
            assert_eq!(hints["destructiveHint"], true, "{id}");
        }
    }
    c.close();
}

/// A generated tool runs its command: `cmd_photo_star_toggle` stars a photo and `cmd_edit_undo`
/// takes it back.
#[test]
fn cmd_photo_star_toggle_stars_and_undoes() {
    let mut c = Client::demo();
    initialize(&mut c);
    let before = photo(&mut c, 2, 3)["star"].as_bool().unwrap();
    assert_eq!(ok(&c.call(3, "cmd_photo_star_toggle", json!({ "ids": [3] })))["ids"], json!([3]));
    assert_eq!(photo(&mut c, 4, 3)["star"], !before);
    ok(&c.call(5, "cmd_edit_undo", json!({})));
    assert_eq!(photo(&mut c, 6, 3)["star"], before);
    assert_eq!(code(&c.call(7, "cmd_photo_star_toggle", json!({ "ids": [999_999] }))), "not_found", "the command checks its params");
    let r = c.request(8, "tools/call", json!({ "name": "cmd_photo_star_toggle", "arguments": [3] }));
    assert_eq!(r["error"]["code"], -32602, "{r}");
    let r = c.request(9, "tools/call", json!({ "name": "cmd_photo_no_such_thing", "arguments": {} }));
    assert_eq!(r["error"]["code"], -32602, "{r}");
    c.close();
}

/// What the library holds for plausible params.
struct Known {
    photo: u64,
    folder: u64,
    album: Value,
}

/// A command's required params from its params doc, with plausible values: the library's own photo,
/// folder and album for ids, a word for text, the first choice of a list of literals. Never
/// `confirm` (nor `dontAsk`), so a command that asks first answers `needs_confirm`.
fn plausible(doc: &str, known: &Known) -> Value {
    let signature = doc.find("}:").and_then(|end| doc.get(..=end)).unwrap_or(doc);
    let mut params = Map::new();
    let mut rest = signature;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let key = &after[..close];
        rest = &after[close + 1..];
        let Some(ty) = rest.strip_prefix(':') else { continue };
        if ["confirm", "dontAsk"].contains(&key) || params.contains_key(key) {
            continue;
        }
        let ty = ty.trim_start();
        let value = if ty.starts_with("[PhotoId]") {
            json!([known.photo])
        } else if ty.starts_with("PhotoId") {
            json!(known.photo)
        } else if ty.starts_with("FolderId") {
            json!(known.folder)
        } else if ty.starts_with("AlbumId") {
            known.album.clone()
        } else if ty.starts_with("string") || ty.starts_with("id") {
            json!("x")
        } else if ty.starts_with("bool") {
            json!(false)
        } else if ty.starts_with("int") || ty.starts_with("number") || ty.starts_with(|c: char| c.is_ascii_digit()) {
            json!(1)
        } else if let Some(literal) = ty.strip_prefix('"').and_then(|t| t.split('"').next()) {
            json!(literal)
        } else {
            continue;
        };
        params.insert(key.to_string(), value);
    }
    Value::Object(params)
}

/// Every command, run through its tool with plausible params and without `confirm`: each one
/// answering `needs_confirm` is `destructiveHint` (and changes nothing, which the next commands'
/// runs rely on). Delete, Delete Folder and Rebuild Library are among them, so the check is not
/// empty.
#[test]
fn every_command_asking_first_is_destructive() {
    let mut c = Client::demo();
    initialize(&mut c);
    let listed = tools(&mut c, 2);
    let folder = photo(&mut c, 3, 2)["folder"].as_u64().unwrap();
    let created = ok(&c.call(4, "cmd_album_create", json!({ "name": "Asked", "ids": [2] }))).clone();
    let album = created.get("album").or(created.get("id")).cloned().unwrap_or_else(|| panic!("album.create answered {created}"));
    let known = Known { photo: 2, folder, album };
    let mut asked = BTreeSet::new();
    for (n, cmd) in (10..).zip(commands(&mut c, 5)) {
        let id = cmd["id"].as_str().unwrap();
        let params = plausible(cmd["params"].as_str().unwrap(), &known);
        let r = c.call(n, &name(id), params.clone());
        if r["structuredContent"]["code"] == "needs_confirm" {
            assert!(r["structuredContent"]["prompt"].is_object(), "{id}: {r}");
            assert_eq!(listed[&name(id)]["annotations"]["destructiveHint"], true, "{id} asks first ({params}) but is not destructive");
            asked.insert(id.to_string());
        }
    }
    eprintln!("{} commands asked first: {asked:?}", asked.len());
    for id in ["photo.delete", "folder.delete", "library.rebuild"] {
        assert!(asked.contains(id), "{id} did not ask first: asked {asked:?}");
    }
    c.close();
}

/// `cmd_photo_delete` without `confirm` answers `needs_confirm` with the question, and the photo
/// stays: in the library and on disk.
#[test]
fn cmd_photo_delete_without_confirm_asks_and_changes_nothing() {
    let mut c = Client::demo();
    initialize(&mut c);
    let before = photo(&mut c, 2, 4);
    let photos = ok(&c.call(3, "doc_inspect", json!({})))["counts"]["photos"].clone();
    let r = c.call(4, "cmd_photo_delete", json!({ "ids": [4] }));
    assert_eq!(code(&r), "needs_confirm");
    assert!(r["structuredContent"]["prompt"]["buttons"].is_array(), "{r}");
    assert_eq!(photo(&mut c, 5, 4), before);
    assert!(std::path::Path::new(before["path"].as_str().unwrap()).is_file());
    assert_eq!(ok(&c.call(6, "doc_inspect", json!({})))["counts"]["photos"], photos);
    c.close();
}

/// One message to `server`, its reply.
fn send(server: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
    serde_json::from_str(&server.handle_line(&line).unwrap()).unwrap()
}

/// `--compact` lists the helpers only (the same as the full list without its `cmd_` tools), while
/// `command_run` still reaches every command.
#[test]
fn compact_lists_only_the_helpers() {
    let names = |server: &mut Server| -> BTreeSet<String> {
        let r = send(server, 1, "tools/list", json!({}));
        r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    };
    let mut full = Server::new(Box::new(Headless::open(Source::Demo, &[]).unwrap()));
    let mut compact = Server::new(Box::new(Headless::open(Source::Demo, &[]).unwrap())).with_compact(true);
    let helpers: BTreeSet<String> = names(&mut full).into_iter().filter(|n| !n.starts_with("cmd_")).collect();
    assert!(helpers.contains("command_run"));
    assert_eq!(names(&mut compact), helpers);
    let r =
        send(&mut compact, 2, "tools/call", json!({ "name": "command_run", "arguments": { "id": "photo.star.toggle", "params": { "ids": [3] } } }));
    assert_eq!(r["result"]["isError"], false, "{r}");
}

/// The app's registry in connect mode: an engine command, a UI command (`effect: view`) and a menu
/// item whose command is not registered (`live: false`); every other call is echoed.
struct App;

impl Backend for App {
    fn call(&mut self, method: &str, params: Value, _progress: &mut dyn Progress) -> Result<Value, ToolError> {
        match method {
            "engine.commands" => Ok(json!([
                {"id": "photo.star.toggle", "label": "Star / Unstar", "menu": "Picture/Star / Unstar", "shortcut": "Ctrl+8", "params": "{\"ids\":[PhotoId]}", "effect": "library", "live": true, "ui": false},
                {"id": "pane.tags", "label": "Tags", "menu": "View/Tags", "shortcut": null, "params": "{}", "effect": "view", "live": true, "ui": true},
                {"id": "photo.locate", "label": "Locate on Disk", "menu": "Picture/Locate on Disk", "params": "", "live": false, "ui": false},
            ])),
            _ => Ok(json!({ "method": method, "params": params })),
        }
    }

    fn render(&mut self, _id: Option<u64>, _max_side: u32) -> Result<Rendered, ToolError> {
        Err(ToolError::new("unsupported", "no rendering here"))
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        "an app".into()
    }
}

/// In connect mode the app's `UI_COMMANDS` get tools too (a view change is neither read-only nor
/// destructive), a menu item without a registered command gets none, and a call is the app's
/// `engine.execute`.
#[test]
fn connect_mode_adds_the_ui_commands() {
    let mut c = Client::start(|| Box::new(App));
    initialize(&mut c);
    let listed = tools(&mut c, 2);
    let generated: Vec<&str> = listed.keys().map(String::as_str).filter(|n| n.starts_with("cmd_")).collect();
    assert_eq!(generated, ["cmd_pane_tags", "cmd_photo_star_toggle"]);
    let tags = &listed["cmd_pane_tags"];
    assert_eq!((&tags["annotations"]["readOnlyHint"], &tags["annotations"]["destructiveHint"]), (&json!(false), &json!(false)));
    assert!(listed["cmd_photo_star_toggle"]["description"].as_str().unwrap().contains("Menu: Picture ▸ Star / Unstar. Shortcut: Ctrl+8."));
    let r = ok(&c.call(3, "cmd_pane_tags", json!({ "on": true }))).clone();
    assert_eq!(r, json!({ "method": "engine.execute", "params": { "command": "pane.tags", "params": { "on": true } } }));
    c.close();
}
