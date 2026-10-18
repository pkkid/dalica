//! The organizing helpers of `mcp.md` §4.2 over the real framing, on `scenes` folders: each tool's
//! wiring onto its engine commands, its arguments and its result. What the commands do is proved
//! by the engine's tests; here only what the tools add.

use std::collections::BTreeSet;

use dalica_engine::automation::Policy;
use dalica_mcp::{Headless, Source};
use serde_json::{Value, json};

use crate::common::{Client, code, initialize, ok};

/// A tools/call whose arguments fail the tool's schema: JSON-RPC -32602.
fn refused(c: &mut Client, id: u64, name: &str, args: Value) {
    let r = c.request(id, "tools/call", json!({ "name": name, "arguments": args }));
    assert_eq!(r["error"]["code"], -32602, "{name} {args}: {r}");
}

/// The ids of a `query_photos` or `library.query` answer.
fn ids(answer: &Value) -> Vec<u64> {
    answer["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect()
}

/// The organizing tools are listed, each with its schema and hints.
#[test]
fn the_organizing_tools_are_listed() {
    let mut c = Client::start(|| Box::new(Headless::open(Source::Empty, &[]).unwrap()));
    initialize(&mut c);
    let list = c.request(2, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let hint = |n: &str, h: &str| tools.iter().find(|t| t["name"] == n).unwrap_or_else(|| panic!("{n} listed"))["annotations"][h].clone();
    for (name, read_only) in [
        ("scan_folder", false),
        ("query_photos", true),
        ("photo_info", true),
        ("select_photos", false),
        ("set_metadata", false),
        ("create_album", false),
        ("jobs_list", true),
        ("jobs_cancel", false),
    ] {
        assert_eq!(hint(name, "readOnlyHint"), json!(read_only), "{name}");
        assert_eq!(hint(name, "openWorldHint"), json!(false), "{name}");
    }
    let jobs = c.call(3, "jobs_list", json!({}));
    assert!(ok(&jobs)["result"].is_array(), "{jobs}");
    assert_eq!(ok(&c.call(4, "jobs_cancel", json!({}))), &json!({ "cancelled": [] }));
    refused(&mut c, 5, "jobs_cancel", json!({ "job": "all" }));
    refused(&mut c, 6, "jobs_list", json!({ "kind": "scan" }));
    c.close();
}

/// `scan_folder` of 200 photos in a read root answers the counts after progress that rises to
/// the total; a path outside the roots, a relative one and bad arguments fail.
#[test]
fn scan_folder_answers_the_counts_with_increasing_progress() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Photos");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..200u64 {
        let scene = dalica_scenes::Scene::new(i, 120, 80);
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), dalica_scenes::jpeg(&scene, 80, None).unwrap()).unwrap();
    }
    let root = dir.path().to_path_buf();
    let mut c = Client::start(move || {
        let mut h = Headless::open(Source::Empty, &[]).unwrap();
        h.roots_mut().add_read(root);
        Box::new(h)
    });
    initialize(&mut c);
    c.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "scan_folder", "arguments": { "path": folder }, "_meta": { "progressToken": "scan" },
    } }));
    let (lines, reply) = c.until(&json!(2));
    let r = ok(&reply["result"]);
    assert_eq!(
        (&r["folder"], &r["section"], &r["photos"], &r["videos"], &r["skipped"], &r["problems"]),
        (&json!(1), &json!("folder:1"), &json!(200), &json!(0), &json!(0), &json!([])),
        "{r}"
    );
    let steps: Vec<(u64, u64)> = lines
        .iter()
        .filter(|l| l["method"] == "notifications/progress" && l["params"]["progressToken"] == "scan")
        .map(|l| (l["params"]["progress"].as_u64().unwrap(), l["params"]["total"].as_u64().unwrap()))
        .collect();
    assert!(steps.windows(2).all(|w| w[0].0 < w[1].0), "strictly increasing: {steps:?}");
    assert_eq!(steps.last(), Some(&(200, 200)), "{steps:?}");

    let all = ok(&c.call(3, "query_photos", json!({ "limit": 1 }))).clone();
    assert_eq!(all["total"], 200, "the scanned photos are in the library");
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(code(&c.call(4, "scan_folder", json!({ "path": outside.path() }))), "path_denied", "a folder outside the read roots");
    assert_eq!(code(&c.call(5, "scan_folder", json!({ "path": "Photos" }))), "bad_params");
    refused(&mut c, 6, "scan_folder", json!({}));
    refused(&mut c, 7, "scan_folder", json!({ "path": folder, "mode": "sometimes" }));
    refused(&mut c, 8, "scan_folder", json!({ "path": folder, "recursive": true }));
    c.close();
}

/// `query_photos` with words, `tag:`, `-word`, `tags` and the engine's filters answers what
/// `library.query` answers; the filters and sorts done here narrow and order those matches; each
/// photo carries the fields of `mcp.md` §4.2.
#[test]
fn query_photos_equals_library_query() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 12, 120, 80);
    initialize(&mut c);
    for (n, (cmd, params)) in [
        ("photo.tags.add", json!({ "ids": [1, 2, 3], "tags": ["beach"] })),
        ("photo.tags.add", json!({ "ids": [3], "tags": ["night"] })),
        ("photo.caption.set", json!({ "ids": [2], "caption": "tram stop" })),
        ("photo.star.set", json!({ "ids": [2, 4, 5], "on": true })),
    ]
    .into_iter()
    .enumerate()
    {
        ok(&c.call(10 + n as u64, "command_run", json!({ "id": cmd, "params": params })));
    }
    let cases = [
        (json!({ "query": "tram" }), json!({ "text": "tram" })),
        (json!({ "query": "tag:beach" }), json!({ "text": "tag:beach" })),
        (json!({ "query": "img -tram" }), json!({ "text": "img -tram" })),
        (json!({ "tags": ["beach", "night"] }), json!({ "text": "tag:beach tag:night" })),
        (json!({ "query": "tag:beach", "starred": true }), json!({ "text": "tag:beach", "starred": true })),
        (json!({ "geotagged": true }), json!({ "filters": { "geotagged": true } })),
        (json!({ "section": "folder:1", "offset": 2, "limit": 5 }), json!({ "section": "folder:1", "offset": 2, "limit": 5 })),
    ];
    for (n, (ours, theirs)) in cases.into_iter().enumerate() {
        let id = 20 + 2 * n as u64;
        let mine = ok(&c.call(id, "query_photos", ours.clone())).clone();
        let engine = ok(&c.call(id + 1, "command_run", json!({ "id": "library.query", "params": theirs }))).clone();
        assert_eq!((ids(&mine), &mine["total"]), (ids(&engine), &engine["total"]), "{ours}");
    }
    let tram = ok(&c.call(40, "query_photos", json!({ "query": "tram" }))).clone();
    let photo = &tram["photos"][0];
    let keys: BTreeSet<&str> = photo.as_object().unwrap().keys().map(String::as_str).collect();
    for k in ["id", "path", "name", "section", "star", "caption", "tags", "date", "width", "height", "edited", "hidden", "video"] {
        assert!(keys.contains(k), "{k} in {photo}");
    }
    assert_eq!(
        (&photo["section"], &photo["caption"], &photo["tags"], &photo["video"]),
        (&json!("folder:1"), &json!("tram stop"), &json!(["beach"]), &json!(false))
    );

    assert_eq!(ids(ok(&c.call(41, "query_photos", json!({ "has_caption": true })))), [2]);
    assert_eq!(ids(ok(&c.call(42, "query_photos", json!({ "sort": "name", "reverse": true, "limit": 3 })))), [12, 11, 10]);
    let starred_uncaptioned = ok(&c.call(43, "query_photos", json!({ "starred": true, "has_caption": false }))).clone();
    assert_eq!((ids(&starred_uncaptioned), &starred_uncaptioned["total"]), (vec![4, 5], &json!(2)));
    assert_eq!(code(&c.call(44, "query_photos", json!({ "limit": 1001 }))), "limit");
    assert_eq!(code(&c.call(45, "query_photos", json!({ "taken_after": "June" }))), "bad_params");
    assert_eq!(code(&c.call(46, "query_photos", json!({ "section": "folder:99" }))), "not_found");
    refused(&mut c, 47, "query_photos", json!({ "text": "tram" }));
    refused(&mut c, 48, "query_photos", json!({ "sort": "colour" }));
    refused(&mut c, 49, "query_photos", json!({ "tags": "beach" }));
    c.close();
}

/// `photo_info` lists every field of `mcp.md` §4.2, `held` from the tray.
#[test]
fn photo_info_lists_every_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 2, 120, 80);
    initialize(&mut c);
    ok(&c.call(2, "set_metadata", json!({ "ids": [1], "star": true, "caption": "Harbour", "tags_add": ["sea"] })));
    ok(&c.call(3, "select_photos", json!({ "ids": [1], "section": "folder:1", "hold": true })));
    let info = ok(&c.call(4, "photo_info", json!({ "id": 1 }))).clone();
    let keys: BTreeSet<&str> = info.as_object().unwrap().keys().map(String::as_str).collect();
    for k in [
        "id",
        "path",
        "geo",
        "faces",
        "name",
        "section",
        "fileSize",
        "dimensions",
        "dates",
        "exif",
        "star",
        "caption",
        "tags",
        "rotation",
        "recipe",
        "albums",
        "geotag",
        "hidden",
        "held",
        "originals",
        "unsavedEdits",
        "video",
    ] {
        assert!(keys.contains(k), "{k} in {info}");
    }
    assert!(info["path"].as_str().unwrap().ends_with("IMG_0001.jpg"), "{info}");
    assert_eq!((&info["star"], &info["caption"], &info["tags"], &info["held"]), (&json!(true), &json!("Harbour"), &json!(["sea"]), &json!(true)));
    assert_eq!((&info["dimensions"]["width"], &info["dimensions"]["height"]), (&json!(120), &json!(80)));
    assert_eq!(info["recipe"], json!({ "string": "", "list": [], "redo": [] }));
    assert_eq!(
        (&info["rotation"], &info["unsavedEdits"], &info["originals"], &info["video"]),
        (&json!(0), &json!(false), &json!(false), &json!(false))
    );
    assert_eq!(info["metadataPending"], false, "the file's fields were read");
    assert!(info["exif"].as_object().is_some_and(|e| e.contains_key("camera")), "{info}");
    assert_eq!(ok(&c.call(5, "photo_info", json!({ "id": 2 })))["held"], false);
    assert_eq!(code(&c.call(6, "photo_info", json!({ "id": 99 }))), "not_found");
    refused(&mut c, 7, "photo_info", json!({ "id": "1" }));
    refused(&mut c, 8, "photo_info", json!({}));
    c.close();
}

/// `select_photos` sets, adds to and holds the selection; an unknown id is an error and the
/// selection is unchanged.
#[test]
fn select_photos_refuses_an_unknown_id_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 4, 120, 80);
    initialize(&mut c);
    let state = ok(&c.call(2, "select_photos", json!({ "ids": [1, 2], "section": "folder:1" }))).clone();
    assert_eq!((&state["section"], &state["selection"], &state["tray"]), (&json!("folder:1"), &json!([1, 2]), &json!([1, 2])));
    let added = ok(&c.call(3, "select_photos", json!({ "ids": [3], "mode": "add", "hold": true }))).clone();
    assert_eq!((&added["held"], &added["tray"]), (&json!([1, 2, 3]), &json!([1, 2, 3])));
    assert_eq!(code(&c.call(4, "select_photos", json!({ "ids": [4, 999] }))), "not_found");
    let after = ok(&c.call(5, "select_photos", json!({ "ids": [], "mode": "add" }))).clone();
    assert_eq!((&after["selection"], &after["tray"]), (&json!([1, 2, 3]), &json!([1, 2, 3])), "unchanged by the failure");
    let removed = ok(&c.call(6, "select_photos", json!({ "ids": [1], "mode": "remove" }))).clone();
    assert_eq!((&removed["selection"], &removed["tray"]), (&json!([2, 3]), &json!([1, 2, 3])), "held photos stay in the tray");
    refused(&mut c, 7, "select_photos", json!({ "ids": [1], "mode": "toggle" }));
    refused(&mut c, 8, "select_photos", json!({ "section": "folder:1" }));
    refused(&mut c, 9, "select_photos", json!({ "ids": ["1"] }));
    c.close();
}

/// `set_metadata` with star, caption and tags is one undo step that one `cmd_edit_undo` restores; without
/// `ids` it acts on the tray; nothing to change and an empty tray fail.
#[test]
fn set_metadata_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 3, 120, 80);
    initialize(&mut c);
    assert_eq!(code(&c.call(2, "set_metadata", json!({ "star": true }))), "disabled", "an empty tray");
    let fields = |c: &mut Client, id: u64, photo: u64| {
        let i = ok(&c.call(id, "photo_info", json!({ "id": photo }))).clone();
        (i["star"].clone(), i["caption"].clone(), i["tags"].clone())
    };
    let set = ok(&c.call(3, "set_metadata", json!({ "ids": [1, 2], "star": true, "caption": "Tram 28", "tags_add": ["lisbon", "tram"] }))).clone();
    assert_eq!(set["changed"], json!(["star", "caption", "tags_add"]));
    assert_eq!(fields(&mut c, 4, 2), (json!(true), json!("Tram 28"), json!(["lisbon", "tram"])));
    let undone = c.call(5, "cmd_edit_undo", json!({}));
    ok(&undone);
    for (n, photo) in [1, 2].into_iter().enumerate() {
        assert_eq!(fields(&mut c, 6 + n as u64, photo), (json!(false), json!(""), json!([])), "one Undo restores photo {photo}");
    }

    ok(&c.call(10, "select_photos", json!({ "ids": [3], "section": "folder:1" })));
    let tray = ok(&c.call(11, "set_metadata", json!({ "hidden": true, "tags_remove": ["none"] }))).clone();
    assert_eq!(tray["ids"], json!([3]));
    assert_eq!(ok(&c.call(12, "photo_info", json!({ "id": 3 })))["hidden"], true);
    assert_eq!(code(&c.call(15, "set_metadata", json!({ "ids": [1] }))), "bad_params");
    assert_eq!(code(&c.call(16, "set_metadata", json!({ "ids": [1, 99], "star": true }))), "not_found");
    assert_eq!(fields(&mut c, 17, 1).0, json!(false), "a failed group changes nothing");
    refused(&mut c, 18, "set_metadata", json!({ "ids": [1], "rating": 5 }));
    refused(&mut c, 19, "set_metadata", json!({ "ids": [1], "star": "yes" }));
    c.close();
}

/// `create_album` makes an album of the tray's photos by default, or of `ids`.
#[test]
fn create_album_from_the_tray() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 4, 120, 80);
    initialize(&mut c);
    ok(&c.call(2, "select_photos", json!({ "ids": [2, 3], "section": "folder:1" })));
    let album = ok(&c.call(3, "create_album", json!({ "name": "Best", "description": "the good ones" }))).clone();
    assert_eq!((&album["name"], &album["count"]), (&json!("Best"), &json!(2)), "{album}");
    let section = album["section"].as_str().unwrap().to_string();
    assert_eq!(section, format!("album:{}", album["album"].as_str().unwrap()));
    assert_eq!(ids(ok(&c.call(4, "query_photos", json!({ "section": section })))), [2, 3]);
    let named = ok(&c.call(5, "create_album", json!({ "name": "One", "ids": [4] }))).clone();
    assert_eq!(named["count"], 1);
    assert_eq!(code(&c.call(6, "create_album", json!({ "name": "Bad", "ids": [99] }))), "not_found");
    refused(&mut c, 7, "create_album", json!({ "ids": [1] }));
    refused(&mut c, 8, "create_album", json!({ "name": "X", "date": "2026-01-01" }));
    c.close();
}

/// `set_metadata {geo}` sets and (with `null`) removes the location in the same undo step as the
/// other fields; `photo_info` and `query_photos` carry the geotag; removing it from more than five
/// photos is `needs_confirm`, changing nothing, until `confirm: true`; a point off the globe fails.
#[test]
fn set_metadata_sets_and_clears_the_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Client::photos(dir.path(), 7, 120, 80);
    initialize(&mut c);
    let geotag = |c: &mut Client, id: u64, photo: u64| ok(&c.call(id, "photo_info", json!({ "id": photo })))["geotag"].clone();
    let set = ok(&c.call(2, "set_metadata", json!({ "ids": [1, 2], "geo": [38.7, -9.1], "star": true }))).clone();
    assert_eq!(set["changed"], json!(["star", "geo"]), "{set}");
    assert_eq!(geotag(&mut c, 3, 1), json!([38.7, -9.1]));
    assert_eq!(ok(&c.call(4, "photo_info", json!({ "id": 1 })))["geo"], true);
    assert_eq!(geotag(&mut c, 5, 3), Value::Null);
    let found = ok(&c.call(6, "query_photos", json!({ "geotagged": true }))).clone();
    assert_eq!((ids(&found), &found["photos"][0]["geotag"]), (vec![1, 2], &json!([38.7, -9.1])), "{found}");
    ok(&c.call(7, "cmd_edit_undo", json!({})));
    let info = ok(&c.call(8, "photo_info", json!({ "id": 2 }))).clone();
    assert_eq!((&info["geotag"], &info["star"]), (&Value::Null, &json!(false)), "one undo takes back the star and the place");

    let all = json!([1, 2, 3, 4, 5, 6, 7]);
    ok(&c.call(9, "set_metadata", json!({ "ids": all, "geo": [41.15, -8.61] })));
    let asked = c.call(10, "set_metadata", json!({ "ids": all, "geo": null }));
    assert_eq!(code(&asked), "needs_confirm", "{asked}");
    assert!(asked["structuredContent"]["prompt"]["text"].is_string(), "the question: {asked}");
    assert_eq!(geotag(&mut c, 11, 7), json!([41.15, -8.61]), "nothing changed");
    ok(&c.call(12, "set_metadata", json!({ "ids": all, "geo": null, "confirm": true })));
    assert_eq!(geotag(&mut c, 13, 7), Value::Null);
    assert!(ids(ok(&c.call(14, "query_photos", json!({ "geotagged": true })))).is_empty());
    assert_eq!(code(&c.call(15, "set_metadata", json!({ "ids": [1], "geo": [91.0, 0.0] }))), "bad_params");
    assert_eq!(code(&c.call(16, "set_metadata", json!({ "ids": [1], "geo": "Lisbon" }))), "bad_params");
    c.close();
}

/// A server over 3 procedural photos (ids 1–3) with or without `--automation-allow-people`.
fn people_client(dir: &std::path::Path, allow: bool) -> Client {
    let folder = dir.join("Photos");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let scene = dalica_scenes::Scene::new(i, 120, 80);
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), dalica_scenes::jpeg(&scene, 90, None).unwrap()).unwrap();
    }
    let mut c = Client::start(move || {
        let mut policy = Policy::default();
        policy.set_allow_people(allow);
        let mut h = Headless::open_with(Source::Empty, &[folder], policy, false).unwrap();
        h.session_mut().wait_idle();
        Box::new(h)
    });
    initialize(&mut c);
    c
}

/// People data over MCP (`mcp.md` §5): without `--automation-allow-people` every reply that
/// carries it (`faces.add`, `faces.list`, `people.list`, through `command_run`, a `cmd_*` tool or a
/// `command_batch` step, and `photo_info`'s faces) has names `null` and no face rectangles, ids
/// and counts kept, and a person's section suggests no export folder named after them; with it
/// they pass. A search by a name (`query_photos {query: "person:Ana"}`, or the plain word) finds
/// her photo only with it.
#[test]
fn people_data_is_withheld_without_the_permission() {
    for allow in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut c = people_client(dir.path(), allow);
        let added = ok(&c.call(2, "cmd_faces_add", json!({ "id": 1, "rect": [0.2, 0.2, 0.5, 0.6] }))).clone();
        assert_eq!((added["face"].as_str(), added.get("rect").is_some()), (Some("1.0"), allow), "{added}");
        let named =
            ok(&c.call(3, "command_run", json!({ "id": "faces.name", "params": { "faces": ["1.0"], "name": "Ana", "create": true } }))).clone();
        let person = named["person"].as_str().unwrap().to_string();
        let name = if allow { json!("Ana") } else { Value::Null };

        let people = ok(&c.call(4, "cmd_people_list", json!({}))).clone();
        assert_eq!((&people["people"][0]["name"], &people["people"][0]["count"]), (&name, &json!(1)), "{people}");
        let faces = ok(&c.call(5, "command_run", json!({ "id": "faces.list", "params": { "ids": [1] } }))).clone();
        let face = &faces["faces"][0];
        assert_eq!(
            (&face["name"], &face["person"], face.get("rect").is_some(), face.get("displayed").is_some()),
            (&name, &json!(person), allow, allow)
        );
        let batch = ok(&c.call(6, "command_batch", json!({ "steps": [{ "id": "people.list" }] }))).clone();
        assert_eq!(batch["results"][0]["result"]["people"][0]["name"], name, "{batch}");
        let info = ok(&c.call(7, "photo_info", json!({ "id": 1 }))).clone();
        assert_eq!((&info["faces"][0]["name"], info["faces"][0].get("rect").is_some()), (&name, allow), "{info}");
        assert_eq!(ok(&c.call(8, "photo_info", json!({ "id": 2 })))["faces"], json!([]));

        let defaults = ok(&c.call(9, "cmd_export_defaults", json!({ "section": format!("person:{person}") }))).clone();
        let folder = if allow { json!("Ana") } else { defaults["fallbackName"].clone() };
        assert_eq!(defaults["folderName"], folder, "{defaults}");

        // A search by her name finds her photo only with the permission (no guess-oracle).
        let hers: Vec<u64> = if allow { vec![1] } else { vec![] };
        assert_eq!(ids(ok(&c.call(10, "query_photos", json!({ "query": "person:Ana" })))), hers, "person:");
        assert_eq!(ids(ok(&c.call(11, "query_photos", json!({ "query": "Ana" })))), hers, "a plain word");
        c.close();
    }
}
