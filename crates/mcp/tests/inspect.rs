//! `doc_inspect` (`mcp.md` §4.1): headless it is the engine query `library.inspect`, unchanged; with
//! a window attached it asks the control method of that name; bad arguments fail gracefully. What
//! the inspection holds is `crates/engine/tests/library_inspect.rs`'s.

use dalica_mcp::{Backend, Progress, Rendered, ToolError};
use serde_json::{Value, json};

use crate::common::{Client, code, initialize, ok};

#[test]
fn doc_inspect_is_the_engine_query_headless() {
    let mut c = Client::demo();
    initialize(&mut c);
    let listed = c.request(2, "tools/list", json!({}));
    let tool = listed["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "doc_inspect").cloned().expect("doc_inspect is listed");
    assert_eq!(tool["annotations"]["readOnlyHint"], true);

    for (n, args) in [json!({}), json!({ "photos": true, "offset": 3, "limit": 4 }), json!({ "photos": "folder:2" })].into_iter().enumerate() {
        let id = 10 + 2 * n as u64;
        let inspected = ok(&c.call(id, "doc_inspect", args.clone())).clone();
        let query = ok(&c.call(id + 1, "command_run", json!({ "id": "library.inspect", "params": args }))).clone();
        assert_eq!(inspected, query, "{args}");
    }
    let page = ok(&c.call(20, "doc_inspect", json!({ "photos": true, "limit": 4 }))).clone();
    assert_eq!(page["photos"].as_array().map(Vec::len), Some(4));
    assert_eq!(page["demo"], true);

    // Bad arguments: a value the engine refuses is `isError`, an unknown or ill-typed one -32602.
    assert_eq!(code(&c.call(21, "doc_inspect", json!({ "photos": true, "limit": 5000 }))), "bad_params");
    assert_eq!(code(&c.call(22, "doc_inspect", json!({ "section": "folder:99" }))), "not_found");
    assert_eq!(code(&c.call(23, "doc_inspect", json!({ "photos": 1 }))), "bad_params");
    for (id, args) in [(24, json!({ "people": true })), (25, json!({ "offset": "3" }))] {
        let r = c.request(id, "tools/call", json!({ "name": "doc_inspect", "arguments": args }));
        assert_eq!(r["error"]["code"], -32602, "{args}: {r}");
    }
    c.close();
}

/// A backend with a window attached that answers every call with what it was asked.
struct Window;

impl Backend for Window {
    fn call(&mut self, method: &str, params: Value, _progress: &mut dyn Progress) -> Result<Value, ToolError> {
        Ok(json!({ "method": method, "params": params, "revision": 1, "counts": {}, "tree": {}, "sections": [] }))
    }

    fn render(&mut self, _id: Option<u64>, _max_side: u32) -> Result<Rendered, ToolError> {
        Err(ToolError::new("unsupported", "no rendering here"))
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        "a window".into()
    }
}

#[test]
fn doc_inspect_asks_the_window_in_connect_mode() {
    let mut c = Client::start(|| Box::new(Window));
    initialize(&mut c);
    let r = ok(&c.call(2, "doc_inspect", json!({ "photos": true, "section": "folder:1", "limit": null }))).clone();
    assert_eq!((&r["method"], &r["params"]), (&json!("library.inspect"), &json!({ "photos": true, "section": "folder:1" })));
    c.close();
}
