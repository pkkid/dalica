//! W16, Legacy library (`mcp.md` §9; `data-model.md` §9 and §10.11): `scan_folder` on a folder
//! holding only the previous organizer's sidecar (written here from the documented format, never
//! a real one) shows its data in `photo_info`: stars, captions, a recipe keeping an operation
//! Dalica has no equivalent for as a `legacy:` entry, an album and a person. The scan writes
//! nothing; `set_metadata` starring one photo is the folder's first write: `.dalica.json` then
//! holds every photo and the legacy file is byte-identical; `cmd_legacy_inspect` says `both`.

use std::path::Path;

use dalica_engine::automation::Policy;
use dalica_mcp::{Headless, Source};
use dalica_sidecar::legacy::sidecar_names;
use serde_json::{Value, json};

use crate::common::{Client, initialize};
use crate::harness::{Workspace, bytes, eventually, ids, photo_name, sidecar, sidecar_entry};

/// The album's token and the contact's id, as the legacy format writes them.
const ALBUM: &str = "5e1f0c2a9b3d4e6f708192a3b4c5d6e7";
const CONTACT: &str = "1a2b3c4d5e6f7081";
const PERSON: &str = "Marta Vieira";
const ALBUM_NAME: &str = "Summer House";

/// An agent on a headless server with `--automation-allow-people` (so `photo_info` names the
/// person), the workspace a read root.
struct Agent {
    client: Client,
    next: u64,
}

impl Agent {
    fn start(ws: &Workspace) -> Agent {
        let (library, root) = (ws.library(), ws.path().to_path_buf());
        let mut client = Client::start(move || {
            let mut policy = Policy::default();
            policy.set_allow_people(true);
            let mut h = Headless::open_with(Source::Library(library), &[], policy, false).unwrap();
            h.roots_mut().add_read(root);
            Box::new(h)
        });
        initialize(&mut client);
        Agent { client, next: 100 }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        self.next += 1;
        let r = self.client.call(self.next, name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }
}

/// The legacy sidecar of the folder (CRLF, as the previous organizer writes it): photo 1 starred,
/// captioned, in black and white with a red-eye fix (kept as `legacy:`), showing the person, in
/// the album; photo 2 captioned and in the album; photo 3 starred.
fn legacy_sidecar() -> String {
    let lines = [
        "[encoding]".to_string(),
        "utf8=1".into(),
        format!("[{}]", photo_name(1)),
        "star=yes".into(),
        "caption=Breakfast on the terrace".into(),
        "filters=bw=1;redeye=1;".into(),
        format!("faces=rect64(4000400080008000),{CONTACT};"),
        format!("albums={ALBUM}"),
        format!("[{}]", photo_name(2)),
        "caption=The garden gate".into(),
        format!("albums={ALBUM}"),
        format!("[{}]", photo_name(3)),
        "star=yes".into(),
        "[Contacts2]".into(),
        format!("{CONTACT}={PERSON};;"),
        format!("[.album:{ALBUM}]"),
        format!("name={ALBUM_NAME}"),
        format!("token={ALBUM}"),
    ];
    lines.join("\r\n") + "\r\n"
}

/// The source `cmd_legacy_inspect` gives the one folder with legacy data.
fn source(agent: &mut Agent) -> String {
    let inspected = agent.tool("cmd_legacy_inspect", json!({}));
    let folders = inspected["folders"].as_array().unwrap_or_else(|| panic!("{inspected}"));
    assert_eq!(folders.len(), 1, "{inspected}");
    folders[0]["source"].as_str().unwrap().to_string()
}

/// Every file of `dir` (not its subfolders) with its bytes, sorted by name.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), bytes(&e.path())))
        .collect();
    out.sort();
    out
}

#[test]
fn w16_legacy() {
    let ws = Workspace::new();
    let folder = ws.folder("Summer", 3, 700);
    let legacy = folder.join(&sidecar_names()[0]);
    std::fs::write(&legacy, legacy_sidecar()).unwrap();
    let legacy_bytes = bytes(&legacy);
    let before = files(&folder);

    let mut agent = Agent::start(&ws);
    let section = agent.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
    let three = ids(&agent.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })));
    assert_eq!(three.len(), 3, "{three:?}");
    let by_name = |agent: &mut Agent, n: u64| -> (u64, Value) {
        for id in &three {
            let info = agent.tool("photo_info", json!({ "id": id }));
            if info["name"] == json!(photo_name(n)) {
                return (*id, info);
            }
        }
        panic!("no photo {}", photo_name(n));
    };

    // The legacy data, as the scan applied it: stars, captions, the recipe with its `legacy:`
    // entry, the album and the person.
    let (first, info) = by_name(&mut agent, 1);
    assert_eq!((&info["star"], &info["caption"]), (&json!(true), &json!("Breakfast on the terrace")), "{info}");
    assert_eq!(info["recipe"]["list"], json!(["bw()", "legacy:redeye(1)"]), "the red-eye fix has no equivalent: {info}");
    assert_eq!(info["albums"][0]["name"], json!(ALBUM_NAME), "{info}");
    let album_id = info["albums"][0]["id"].as_str().unwrap().to_string();
    let faces = info["faces"].as_array().unwrap();
    assert_eq!((faces.len(), &faces[0]["name"]), (1, &json!(PERSON)), "{info}");
    let (second, info) = by_name(&mut agent, 2);
    assert_eq!(
        (&info["star"], &info["caption"], &info["albums"][0]["name"]),
        (&json!(false), &json!("The garden gate"), &json!(ALBUM_NAME)),
        "{info}"
    );
    let (_, info) = by_name(&mut agent, 3);
    assert_eq!((&info["star"], &info["caption"], &info["albums"]), (&json!(true), &json!(""), &json!([])), "{info}");
    let people = agent.tool("cmd_people_list", json!({}));
    assert_eq!((&people["people"][0]["name"], people["people"].as_array().map(Vec::len)), (&json!(PERSON), Some(1)), "{people}");
    let album = format!("album:{album_id}");
    let mut members = ids(&agent.tool("doc_inspect", json!({ "photos": album, "limit": 10 })));
    members.sort_unstable();
    let mut want = vec![first, second];
    want.sort_unstable();
    assert_eq!(members, want, "the album holds the two photos");
    assert_eq!(source(&mut agent), "legacy");
    assert_eq!(files(&folder), before, "the scan writes nothing");

    // The first change: Dalica's own sidecar written with every photo, the legacy file untouched.
    let set = agent.tool("set_metadata", json!({ "ids": [second], "star": true }));
    assert_eq!(set["changed"], json!(["star"]), "{set}");
    let names: Vec<String> = (1..=3).map(photo_name).collect();
    eventually(
        "the first write gives the folder a .dalica.json holding every photo",
        || names.iter().all(|n| sidecar_entry(&folder, n)["star"] == json!(true)).then_some(()),
        || format!("{:#}", sidecar(&folder)),
    );
    let entry = sidecar_entry(&folder, &photo_name(1));
    assert_eq!((&entry["caption"], &entry["edits"]), (&json!("Breakfast on the terrace"), &json!(["bw()", "legacy:redeye(1)"])), "{entry}");
    assert_eq!(sidecar_entry(&folder, &photo_name(2))["caption"], json!("The garden gate"));
    assert_eq!(
        sidecar(&folder)["albums"].as_object().map(|a| a.values().map(|v| v["name"].clone()).collect::<Vec<_>>()),
        Some(vec![json!(ALBUM_NAME)])
    );
    assert_eq!(bytes(&legacy), legacy_bytes, "the legacy file is byte-identical");
    let others: Vec<(String, Vec<u8>)> = files(&folder).into_iter().filter(|(n, _)| n != dalica_sidecar::FILE_NAME).collect();
    assert_eq!(others, before, "nothing but Dalica's own sidecar was written");
    eventually("the folder's source turns both", || (source(&mut agent) == "both").then_some(()), String::new);
}
