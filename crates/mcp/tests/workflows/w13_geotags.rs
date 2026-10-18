//! W13, Geotags (`mcp.md` §9; `organizing.md` §7, `xmp-interop.md` §2): `set_metadata {geo}` gives
//! three photos of five a location in one undo step; the GPS reaches each of their files (read
//! back by the independent reader) and no other file; `photo_info` and
//! `query_photos {geotagged: true}` report them; `set_metadata {geo: null}` clears it and the GPS
//! leaves the files; one `cmd_edit_undo` brings it back, in the library and in the files.

use std::path::{Path, PathBuf};

use dalica_meta::Limits;
use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, eventually, photo_name};

/// Lisbon, Praça do Comércio.
const LISBON: [f64; 2] = [38.707_5, -9.136_6];
/// A file's GPS rounds through degrees, minutes and seconds as rationals.
const CLOSE: f64 = 1e-5;

/// The file's GPS as (the place read, after precedence; EXIF's; XMP's), each `[lat, lon]` or
/// `None`.
type Gps = (Option<[f64; 2]>, Option<[f64; 2]>, Option<[f64; 2]>);

fn gps(path: &Path) -> Option<Gps> {
    let bytes = std::fs::read(path).ok()?;
    let read = dalica_meta::read(&bytes, &Limits::default()).ok()?;
    let point = |p: &Option<dalica_meta::Place>| p.as_ref().map(|p| [p.latitude(), p.longitude()]);
    Some((point(&read.owned.place), point(&read.sources.exif.place), point(&read.sources.xmp.place)))
}

fn at_lisbon(p: Option<[f64; 2]>) -> bool {
    p.is_some_and(|[lat, lon]| (lat - LISBON[0]).abs() < CLOSE && (lon - LISBON[1]).abs() < CLOSE)
}

/// Waits until each file of `located` holds Lisbon as its GPS, and each of `bare` holds no GPS at
/// all (neither in EXIF nor in XMP).
fn wait_files(located: &[PathBuf], bare: &[PathBuf], what: &str) {
    eventually(
        what,
        || {
            let here = located.iter().all(|p| gps(p).is_some_and(|(place, _, _)| at_lisbon(place)));
            let none = bare.iter().all(|p| gps(p) == Some((None, None, None)));
            (here && none).then_some(())
        },
        || format!("{:?}", located.iter().chain(bare).map(|p| gps(p)).collect::<Vec<_>>()),
    );
}

/// `photo_info`'s geotag of `id`.
fn geotag(agent: &mut Agent, id: u64) -> Value {
    let info = agent.photo(id);
    assert_eq!(info["geo"], json!(!info["geotag"].is_null()), "{info}");
    info["geotag"].clone()
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

#[test]
fn w13_geotags() {
    let ws = Workspace::new();
    let folder = ws.folder("Lisbon", 5, 300);
    let files: Vec<PathBuf> = (1..=5).map(|n| folder.join(photo_name(n))).collect();
    let (located, bare) = files.split_at(3);
    for p in &files {
        assert_eq!(gps(p), Some((None, None, None)), "{}: the procedural photos carry no GPS", p.display());
    }

    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let all = agent.section_ids(&section);
    assert_eq!(all.len(), 5, "{all:?}");
    let (three, two) = all.split_at(3);
    assert!(agent.query(json!({ "geotagged": true })).is_empty());

    // Three photos geotagged as one step.
    let depth = agent.inspect()["undo"]["depth"].as_u64().unwrap();
    let set = agent.tool("set_metadata", json!({ "ids": three, "geo": LISBON }));
    assert_eq!((&set["changed"], &set["results"][0]["changed"]), (&json!(["geo"]), &json!(3)), "{set}");
    assert_eq!(agent.inspect()["undo"]["depth"], json!(depth + 1), "one undo step");
    wait_files(located, bare, "the GPS of the three geotagged photos is in their files, and only theirs");
    for id in three {
        let g = geotag(&mut agent, *id);
        let point = [g[0].as_f64().unwrap(), g[1].as_f64().unwrap()];
        assert!(at_lisbon(Some(point)), "photo {id}: {g}");
    }
    for id in two {
        assert_eq!(geotag(&mut agent, *id), Value::Null, "photo {id}");
    }
    assert_eq!(sorted(agent.query(json!({ "geotagged": true }))), three);
    assert_eq!(sorted(agent.query(json!({ "geotagged": false }))), two);
    let found = agent.tool("query_photos", json!({ "geotagged": true, "limit": 1 }));
    assert_eq!((&found["photos"][0]["geo"], found["photos"][0]["geotag"].is_array()), (&json!(true), true), "{found}");

    // Cleared: the GPS leaves the files and the queries.
    let cleared = agent.tool("set_metadata", json!({ "ids": three, "geo": null }));
    assert_eq!(cleared["results"][0]["changed"], json!(3), "{cleared}");
    wait_files(&[], &files, "the GPS is gone from every file");
    assert!(agent.query(json!({ "geotagged": true })).is_empty());
    for id in three {
        assert_eq!(geotag(&mut agent, *id), Value::Null, "photo {id}");
    }

    // One undo restores the location of all three, in the library and in the files.
    let undone = agent.call("cmd_edit_undo", json!({}));
    assert_eq!(undone["isError"], json!(false), "{undone}");
    assert_eq!(sorted(agent.query(json!({ "geotagged": true }))), three);
    wait_files(located, bare, "one undo writes the GPS back into the three files");
    agent.quit();
}
