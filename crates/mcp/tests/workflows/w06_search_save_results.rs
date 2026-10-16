//! W06, Search and save results (`mcp.md` §9; `organizing.md` §8): ten photos tagged and two
//! captioned with "tram" are found by `query_photos` (the words of a caption, `tag:` and the tags
//! filter), and the search, saved as an album (Save Search Results), holds exactly its matches;
//! the album's memberships reach the sidecar and `albums.json`, and one Undo takes them away.

use std::path::Path;

use serde_json::{Value, json};

use crate::harness::{Agent, Workspace, eventually, sidecar, sidecar_entry};

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

/// The library's `albums.json`, `Null` while there is none.
fn albums_file(library: &Path) -> Value {
    std::fs::read(library.join("albums.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

/// Waits until the sidecar entries of `names` are members of `album` (or not).
fn wait_member(folder: &Path, names: &[String], album: &str, member: bool) {
    eventually(
        &format!("the sidecar records {names:?} in album {album}: {member}"),
        || names.iter().all(|n| sidecar_entry(folder, n)["albums"].get(album).is_some() == member).then_some(()),
        || sidecar(folder).to_string(),
    );
}

#[test]
fn w06_search_save_results() {
    let ws = Workspace::new();
    let folder = ws.folder("Lisbon", 14, 80);
    let mut agent = ws.agent();
    let section = agent.scan(&folder);
    let ids = agent.section_ids(&section);
    assert_eq!(ids.len(), 14, "{ids:?}");
    let name = |agent: &mut Agent, id: u64| agent.photo(id)["name"].as_str().unwrap().to_string();

    // Ten tagged; two captioned with "tram" (one of them tagged), one with a word that only
    // starts like it.
    let tagged = &ids[..10];
    let trams = [ids[3], ids[11]];
    agent.tool("set_metadata", json!({ "ids": tagged, "tags_add": ["transit"] }));
    agent.tool("set_metadata", json!({ "ids": [trams[0]], "caption": "The 28 tram up the hill" }));
    agent.tool("set_metadata", json!({ "ids": [trams[1]], "caption": "Tram stop at night" }));
    agent.tool("set_metadata", json!({ "ids": [ids[5]], "caption": "Trains at dawn" }));
    let tagged_names: Vec<String> = tagged.iter().map(|id| name(&mut agent, *id)).collect();
    let tram_names: Vec<String> = trams.iter().map(|id| name(&mut agent, *id)).collect();
    eventually(
        "the sidecar records the tags and the captions",
        || {
            let tags = tagged_names.iter().all(|n| sidecar_entry(&folder, n)["tags"] == json!(["transit"]));
            let captions =
                tram_names.iter().all(|n| sidecar_entry(&folder, n)["caption"].as_str().is_some_and(|c| c.to_lowercase().contains("tram")));
            (tags && captions).then_some(())
        },
        || sidecar(&folder).to_string(),
    );

    // The queries: the caption's word, `tag:`, the tags filter, and both together.
    assert_eq!(sorted(agent.query(json!({ "query": "tram" }))), sorted(trams.to_vec()), "a caption's word");
    assert_eq!(sorted(agent.query(json!({ "query": "tag:transit" }))), sorted(tagged.to_vec()), "tag:");
    assert_eq!(sorted(agent.query(json!({ "tags": ["transit"] }))), sorted(tagged.to_vec()), "the tags filter");
    assert_eq!(agent.query(json!({ "query": "tram tag:transit" })), vec![trams[0]], "every term applies");
    let found = agent.tool("query_photos", json!({ "query": "tram" }));
    for p in found["photos"].as_array().unwrap() {
        assert!(p["caption"].as_str().unwrap().to_lowercase().contains("tram"), "{p}");
    }

    // The search, saved as an album: exactly its matches, in light-table order.
    let s = agent.tool("cmd_search_set", json!({ "text": "tram" }));
    assert_eq!(s["text"], json!("tram"), "{s}");
    let saved = agent.tool("cmd_search_saveAsAlbum", json!({ "name": "Trams" }));
    assert_eq!((&saved["name"], &saved["count"]), (&json!("Trams"), &json!(2)), "{saved}");
    let album = saved["album"].as_str().unwrap().to_string();
    let album_section = format!("album:{album}");
    let in_order: Vec<u64> = ids.iter().copied().filter(|id| trams.contains(id)).collect();
    assert_eq!(agent.section_ids(&album_section), in_order, "the album holds the matches in light-table order");
    let sections = agent.inspect()["sections"].clone();
    assert!(sections.as_array().unwrap().iter().any(|s| s["section"] == json!(album_section) && s["name"] == json!("Trams")), "{sections}");
    wait_member(&folder, &tram_names, &album, true);
    let others: Vec<String> = ids.iter().filter(|id| !trams.contains(id)).map(|id| name(&mut agent, *id)).collect();
    wait_member(&folder, &others, &album, false);
    assert_eq!(sidecar(&folder)["albums"][&album]["name"], json!("Trams"), "the folder's sidecar names the album");
    eventually(
        "albums.json defines the album",
        || albums_file(&ws.library()).to_string().contains(&album).then_some(()),
        || albums_file(&ws.library()).to_string(),
    );

    // One Undo: the album is gone, from the library, the folder's album names and albums.json.
    // The photos' entries keep the membership, unused (`data-model.md` §3: a membership of an
    // album the library does not have is kept as written, and named nowhere it is never used).
    let undo = agent.inspect()["undo"]["undo"].clone();
    assert!(undo.as_str().is_some_and(|u| u.starts_with("Save Search Results")), "the next undo is the album: {undo}");
    agent.tool("cmd_edit_undo", json!({}));
    let sections = agent.inspect()["sections"].clone();
    assert!(!sections.to_string().contains(&album), "{sections}");
    for id in trams {
        let info = agent.photo(id);
        assert_eq!(info["albums"], json!([]), "{info}");
    }
    eventually(
        "the folder's sidecar no longer names the album",
        || sidecar(&folder)["albums"].get(&album).is_none().then_some(()),
        || sidecar(&folder).to_string(),
    );
    eventually(
        "albums.json no longer defines the album",
        || (!albums_file(&ws.library()).to_string().contains(&album)).then_some(()),
        || albums_file(&ws.library()).to_string(),
    );
    agent.quit();
}
