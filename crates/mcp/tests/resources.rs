//! The resources (`mcp.md` §7): listed, each read as live JSON equal to its tool's result, a change
//! shown by the next read, an unknown URI `-32002`, the selection and tray in both modes.

use dalica_core::PhotoId;
use dalica_mcp::tray::Mode;
use dalica_mcp::{Backend, Headless, Progress, Rendered, Server, Source, ToolError};
use serde_json::{Value, json};

use crate::common::{Client, initialize, ok};

/// Reads `uri`: its JSON, after checking the contents block.
fn read(c: &mut Client, id: u64, uri: &str) -> Value {
    let r = c.request(id, "resources/read", json!({ "uri": uri }));
    contents(&r, uri)
}

fn contents(r: &Value, uri: &str) -> Value {
    let block = &r["result"]["contents"][0];
    assert_eq!((&block["uri"], &block["mimeType"]), (&json!(uri), &json!("application/json")), "{r}");
    serde_json::from_str(block["text"].as_str().unwrap()).unwrap()
}

/// The four resources and the photo template are listed, and each reads: the library as
/// `doc_inspect`, the commands as `command_list`, a photo with its id and no people data, the
/// edit operations, an empty tray.
#[test]
fn each_resource_reads() {
    let mut c = Client::demo();
    initialize(&mut c);
    let listed = c.request(2, "resources/list", json!({}));
    let uris: Vec<&str> = listed["result"]["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    assert_eq!(uris, ["dalica://library", "dalica://commands", "dalica://selection", "dalica://edit-ops"]);
    let templates = c.request(3, "resources/templates/list", json!({}));
    assert_eq!(templates["result"]["resourceTemplates"][0]["uriTemplate"], "dalica://photo/{id}");

    assert_eq!(read(&mut c, 4, "dalica://library"), *ok(&c.call(5, "doc_inspect", json!({}))));
    assert_eq!(read(&mut c, 6, "dalica://commands"), *ok(&c.call(7, "command_list", json!({}))));
    let photo = read(&mut c, 8, "dalica://photo/3");
    assert_eq!(photo["id"], 3, "{photo}");
    assert!(photo.get("faces").is_none() && photo.get("people").is_none(), "{photo}");
    let ops = read(&mut c, 9, "dalica://edit-ops");
    assert!(ops["ops"].as_array().is_some_and(|o| !o.is_empty()), "{ops}");
    assert_eq!(read(&mut c, 10, "dalica://selection"), json!({ "tray": { "items": [] }, "total": 0, "photos": [] }));
    c.close();
}

/// A photo URI whose id is not a photo's, a malformed one and an unknown URI are `-32002`; a read
/// without `uri` is `-32602`.
#[test]
fn an_unknown_resource_is_not_found() {
    let mut c = Client::demo();
    initialize(&mut c);
    for (id, uri) in
        (2..).zip(["dalica://photo/999999", "dalica://photo/abc", "dalica://photo/", "dalica://photo/-3", "dalica://people", "file:///etc"])
    {
        let r = c.request(id, "resources/read", json!({ "uri": uri }));
        assert_eq!(r["error"]["code"], -32002, "{uri}: {r}");
    }
    assert_eq!(c.request(20, "resources/read", json!({}))["error"]["code"], -32602);
    c.close();
}

/// Nothing is cached: a star given between two reads shows in the photo and the library.
#[test]
fn a_change_between_two_reads_shows() {
    let mut c = Client::demo();
    initialize(&mut c);
    let (photo, library) = (read(&mut c, 2, "dalica://photo/3"), read(&mut c, 3, "dalica://library"));
    ok(&c.call(4, "cmd_photo_star_toggle", json!({ "ids": [3] })));
    let (after, library_after) = (read(&mut c, 5, "dalica://photo/3"), read(&mut c, 6, "dalica://library"));
    assert_eq!(after["star"], !photo["star"].as_bool().unwrap());
    assert!(library_after["revision"].as_u64() > library["revision"].as_u64(), "{library_after}");
    c.close();
}

/// Headless, the selection resource is the Photo Tray the photo tools act on, with a summary of
/// each photo.
#[test]
fn the_selection_is_the_headless_tray() {
    let mut h = Headless::open(Source::Demo, &[]).unwrap();
    h.tray_mut().select(&[PhotoId::new(3), PhotoId::new(2)], Some("folder:1"), Mode::Replace);
    let mut server = Server::new(Box::new(h));
    let line = json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/read", "params": { "uri": "dalica://selection" } }).to_string();
    let r: Value = serde_json::from_str(&server.handle_line(&line).unwrap()).unwrap();
    let selection = contents(&r, "dalica://selection");
    assert_eq!((&selection["tray"]["items"], &selection["total"]), (&json!([3, 2]), &json!(2)), "{selection}");
    let summaries = selection["photos"].as_array().unwrap();
    assert_eq!(summaries.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(), [json!(3), json!(2)]);
    for key in ["name", "path", "star", "caption", "tags", "hidden", "edited"] {
        assert!(summaries[0].get(key).is_some(), "{key}: {selection}");
    }
}

/// An app whose tray holds photo 5 (held) and selection photo 5; `photo.inspect` answers it.
struct App;

impl Backend for App {
    fn call(&mut self, method: &str, params: Value, _progress: &mut dyn Progress) -> Result<Value, ToolError> {
        match (method, params["command"].as_str()) {
            ("library.inspect", _) => {
                Ok(json!({ "revision": 1, "selection": { "section": "folder:1", "ids": [5] }, "tray": { "items": [5], "held": [5] } }))
            }
            ("engine.execute", Some("photo.inspect")) => {
                assert_eq!(params["params"], json!({ "ids": [5] }));
                Ok(json!([{ "id": 5, "name": "a.jpg", "star": true, "recipe": { "edits": ["autofix()"], "redo": [] } }]))
            }
            _ => Err(ToolError::new("unknown_method", method)),
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

/// With the app attached, the selection resource is its selection and tray, with the summaries.
#[test]
fn the_selection_is_the_apps_in_connect_mode() {
    let mut c = Client::start(|| Box::new(App));
    initialize(&mut c);
    let selection = read(&mut c, 2, "dalica://selection");
    assert_eq!(
        selection,
        json!({
            "selection": { "section": "folder:1", "ids": [5] },
            "tray": { "items": [5], "held": [5] },
            "total": 1,
            "photos": [{ "id": 5, "name": "a.jpg", "star": true, "edited": true }],
        })
    );
    c.close();
}
