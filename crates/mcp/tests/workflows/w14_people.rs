//! W14, People (`mcp.md` §5 and §9; `organizing.md` §6, `faces.md` §7, `xmp-interop.md` §6): with
//! `--automation-allow-people` and Store people names in photos on, the faces planted in four photos
//! (by the folder's sidecar, as a scan finds them) are named Ana by one `cmd_faces_name`;
//! `query_photos {query: "person:Ana"}` returns those four; the names reach the files as MWG face
//! regions (read back by the independent reader), never a fifth photo's unnamed face. In a session
//! without the permission the same library answers with names `null` and no face rectangles
//! (`cmd_people_list`, `cmd_faces_list`, `photo_info`), counts kept, and the export folder a
//! person's section suggests is not their name.

use std::path::{Path, PathBuf};

use dalica_engine::automation::Policy;
use dalica_mcp::{Headless, Source};
use dalica_meta::Limits;
use serde_json::{Value, json};

use crate::common::{Client, initialize};
use crate::harness::{Workspace, eventually, ids, photo_name};

/// The face planted in each of the first five photos (the fifth stays unnamed).
const FACE: [f64; 4] = [0.3, 0.2, 0.6, 0.7];
/// A file's region rounds through its centre and size.
const CLOSE: f64 = 1e-5;

/// An agent on a headless server started with `--automation-allow-people` (the harness's agents
/// have no people permission).
struct Allowed {
    client: Client,
    next: u64,
}

impl Allowed {
    fn start(ws: &Workspace) -> Allowed {
        let (library, root) = (ws.library(), ws.path().to_path_buf());
        let mut client = Client::start(move || {
            let mut policy = Policy::default();
            policy.set_allow_people(true);
            let mut h = Headless::open_with(Source::Library(library), &[], policy, false).unwrap();
            h.roots_mut().add_read(root);
            Box::new(h)
        });
        initialize(&mut client);
        Allowed { client, next: 100 }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        self.next += 1;
        let r = self.client.call(self.next, name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }
}

/// The folder sidecar planting one unnamed face in each of `names`.
fn plant_faces(folder: &Path, names: &[String]) {
    let photos: serde_json::Map<String, Value> = names.iter().map(|n| (n.clone(), json!({ "faces": [{ "rect": FACE }] }))).collect();
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": photos });
    std::fs::write(folder.join(dalica_sidecar::FILE_NAME), serde_json::to_vec_pretty(&sidecar).unwrap()).unwrap();
}

/// The names of the face regions in the file at `path`, each with whether its box is [`FACE`].
fn regions(path: &Path) -> Option<Vec<(Option<String>, bool)>> {
    let read = dalica_meta::read(&std::fs::read(path).ok()?, &Limits::default()).ok()?;
    let close = |r: &dalica_core::Rect01| [r.x0() - FACE[0], r.y0() - FACE[1], r.x1() - FACE[2], r.y1() - FACE[3]].iter().all(|d| d.abs() < CLOSE);
    Some(read.owned.faces.iter().map(|f| (f.name.clone(), close(&f.rect))).collect())
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

/// Whether a face of a reply carries a rectangle.
fn has_rect(face: &Value) -> bool {
    face.get("rect").is_some() || face.get("displayed").is_some()
}

#[test]
fn w14_people() {
    let ws = Workspace::new();
    let folder = ws.folder("Party", 6, 400);
    let names: Vec<String> = (1..=6).map(photo_name).collect();
    plant_faces(&folder, &names[..5]);
    let files: Vec<PathBuf> = names.iter().map(|n| folder.join(n)).collect();

    // With the people permission and Store people names in photos on.
    let mut agent = Allowed::start(&ws);
    agent.tool("cmd_options_set", json!({ "values": { "people.storeNamesInPhotos": true } }));
    let section = agent.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
    let all = ids(&agent.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })));
    assert_eq!(all.len(), 6, "{all:?}");
    let unnamed = agent.tool("cmd_faces_list", json!({ "unnamed": true }));
    let faces = unnamed["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 5, "the five planted faces: {unnamed}");
    assert!(faces.iter().all(|f| has_rect(f) && f["name"].is_null()), "{unnamed}");
    let four: Vec<Value> = faces.iter().filter(|f| all[..4].contains(&f["photo"].as_u64().unwrap())).map(|f| f["id"].clone()).collect();
    assert_eq!(four.len(), 4, "{unnamed}");

    let named = agent.tool("cmd_faces_name", json!({ "faces": four, "name": "Ana", "create": true }));
    assert_eq!((&named["created"], &named["changed"]), (&json!(true), &json!(4)), "{named}");
    let person = named["person"].as_str().unwrap().to_string();
    let found = agent.tool("query_photos", json!({ "query": "person:Ana", "limit": 100 }));
    assert_eq!(sorted(ids(&found)), all[..4], "{found}");
    let people = agent.tool("cmd_people_list", json!({}));
    assert_eq!((&people["people"][0]["name"], &people["people"][0]["count"]), (&json!("Ana"), &json!(4)), "{people}");
    let info = agent.tool("photo_info", json!({ "id": all[0] }));
    assert!(info["faces"][0]["name"] == json!("Ana") && has_rect(&info["faces"][0]), "{info}");

    // The names are in the four files, the fifth photo's unnamed face and the sixth photo in none.
    eventually(
        "Ana's face region is in the four files and in no other",
        || {
            let named = files[..4].iter().all(|p| regions(p) == Some(vec![(Some("Ana".to_string()), true)]));
            let others = files[4..].iter().all(|p| regions(p) == Some(Vec::new()));
            (named && others).then_some(())
        },
        || format!("{:?}", files.iter().map(|p| regions(p)).collect::<Vec<_>>()),
    );
    agent.client.close();

    // Without the permission: the same library, names null and no rectangles, counts kept.
    let mut agent = ws.agent();
    let people = agent.tool("cmd_people_list", json!({}));
    let ana = &people["people"][0];
    assert_eq!((&ana["id"], &ana["name"], &ana["count"]), (&json!(person), &Value::Null, &json!(4)), "{people}");
    let theirs = agent.tool("cmd_faces_list", json!({ "person": person }));
    let faces = theirs["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 4, "{theirs}");
    assert!(faces.iter().all(|f| f["name"].is_null() && f["person"] == json!(person) && !has_rect(f)), "{theirs}");
    for id in &all[..5] {
        let info = agent.photo(*id);
        let face = &info["faces"][0];
        assert!(face["name"].is_null() && face.get("id").is_some() && !has_rect(face), "photo {id}: {info}");
    }
    let defaults = agent.tool("cmd_export_defaults", json!({ "section": format!("person:{person}") }));
    assert_eq!(defaults["folderName"], defaults["fallbackName"], "the person's export folder is not named after them: {defaults}");
    assert_ne!(defaults["folderName"], json!("Ana"), "{defaults}");
    agent.quit();
}
