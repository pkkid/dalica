//! W15, Collage (`mcp.md` §9; `creating.md` §1 and §3): `cmd_create_collage` opens the Collage
//! room with eight photos; `cmd_collage_set {style: "grid"}` lays them out as a grid;
//! `cmd_collage_create` with a folder in the write root (`<Pictures>/Dalica/Collages`) writes a
//! JPEG of the page's ratio at 3000 px on its long side and its `.dalica-collage` recipe beside it;
//! the new photo joins the library in the Collages project (`photo_info`, `doc_inspect`); and
//! `cmd_collage_edit` reopens the room from the recipe with the eight clips.

use std::path::{Path, PathBuf};

use dalica_engine::automation::Policy;
use dalica_mcp::{Headless, Source};
use serde_json::{Value, json};

use crate::common::{Client, initialize};
use crate::harness::{Workspace, bytes, eventually, ids, photo_name, sidecar};

/// The long side Create Collage writes (`collage.create`).
const LONG_SIDE: u64 = 3000;

/// An agent on a headless server whose session knows a Pictures folder inside the workspace (the
/// harness's agents keep the system's), the workspace a read root, the write roots the defaults
/// (the system's temporary folder, which holds the workspace).
struct Collager {
    client: Client,
    next: u64,
}

impl Collager {
    fn start(ws: &Workspace, pictures: PathBuf) -> Collager {
        let (library, root) = (ws.library(), ws.path().to_path_buf());
        let mut client = Client::start(move || {
            let mut h = Headless::open_with(Source::Library(library), &[], Policy::default(), false).unwrap();
            h.roots_mut().add_read(root);
            h.session_mut().set_pictures_dir(Some(pictures));
            Box::new(h)
        });
        initialize(&mut client);
        Collager { client, next: 100 }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        self.next += 1;
        let r = self.client.call(self.next, name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }
}

/// The row of the projects group of `doc_inspect`'s tree for the folder at `path`.
fn project_row(inspected: &Value, path: &Path) -> Option<Value> {
    let groups = inspected["tree"]["groups"].as_array()?;
    let projects = groups.iter().find(|g| g["group"] == json!("projects"))?;
    projects["rows"].as_array()?.iter().find(|r| r["path"].as_str().map(Path::new) == Some(path)).cloned()
}

/// The clips of the open collage, by photo id, sorted.
fn clip_ids(inspected: &Value) -> Vec<u64> {
    let mut out: Vec<u64> =
        inspected["clips"].as_array().unwrap_or_else(|| panic!("no clips: {inspected}")).iter().map(|c| c["id"].as_u64().unwrap()).collect();
    out.sort_unstable();
    out
}

#[test]
fn w15_collage() {
    let ws = Workspace::new();
    let folder = ws.folder("Garden", 8, 500);
    let originals: Vec<Vec<u8>> = (1..=8).map(|n| bytes(&folder.join(photo_name(n)))).collect();
    let pictures = ws.path().join("Pictures");
    let collages = pictures.join("Dalica").join("Collages");
    let mut agent = Collager::start(&ws, pictures);
    let scanned = agent.tool("scan_folder", json!({ "path": folder }));
    let section = scanned["section"].as_str().unwrap().to_string();
    let mut eight = ids(&agent.tool("doc_inspect", json!({ "photos": section, "limit": 1000 })));
    eight.sort_unstable();
    assert_eq!(eight.len(), 8, "{eight:?}");

    // The room opens with the eight photos, laid out as a grid.
    let opened = agent.tool("cmd_create_collage", json!({ "ids": eight }));
    assert_eq!((&opened["added"], &opened["opened"]), (&json!(8), &json!(true)), "{opened}");
    let set = agent.tool("cmd_collage_set", json!({ "style": "grid" }));
    assert_eq!(set["changed"], json!(true), "{set}");
    let room = agent.tool("cmd_collage_inspect", json!({}));
    assert_eq!((&room["document"]["style"], clip_ids(&room)), (&json!("grid"), eight.clone()), "{room}");
    assert_eq!(room["placements"].as_array().map(Vec::len), Some(8), "every clip on the page: {room}");
    let (page_w, page_h) = (room["page"]["width"].as_f64().unwrap(), room["page"]["height"].as_f64().unwrap());

    // Create Collage into the write root: the JPEG at the page's ratio, its recipe beside it.
    let made = agent.tool("cmd_collage_create", json!({ "path": collages }));
    assert_eq!(made["state"], json!("done"), "{made}");
    let result = &made["result"];
    let jpeg = PathBuf::from(result["path"].as_str().unwrap_or_else(|| panic!("no path: {made}")));
    let recipe = PathBuf::from(result["recipe"].as_str().unwrap());
    assert_eq!((jpeg.parent(), recipe.parent()), (Some(collages.as_path()), Some(collages.as_path())), "{made}");
    assert_eq!(recipe.file_stem(), jpeg.file_stem(), "the recipe has the picture's name");
    let file = bytes(&jpeg);
    assert!(file.starts_with(&[0xFF, 0xD8, 0xFF]), "{}: a JPEG", jpeg.display());
    let header = dalica_image::read_header(&file, &dalica_image::Limits::default()).unwrap();
    let (w, h) = (u64::from(header.width), u64::from(header.height));
    assert_eq!(w.max(h), LONG_SIDE, "{w} × {h}");
    let ratio = page_w / page_h;
    // The short side rounds to a whole pixel.
    assert!((w as f64 / h as f64 - ratio).abs() <= ratio / w.min(h) as f64, "the page's ratio {ratio}: {w} × {h}");
    let saved: Value = serde_json::from_slice(&bytes(&recipe)).unwrap_or_else(|e| panic!("{}: {e}", recipe.display()));
    let recorded: Vec<u64> = saved["clips"].as_array().unwrap_or_else(|| panic!("{saved}")).iter().map(|c| c["id"].as_u64().unwrap()).collect();
    assert_eq!((&saved["style"], recorded, &saved["output"]), (&json!("grid"), eight.clone(), &json!({ "width": w, "height": h })), "{saved}");
    for (n, original) in originals.iter().enumerate() {
        assert_eq!(&bytes(&folder.join(photo_name(n as u64 + 1))), original, "the photos are untouched");
    }

    // The collage is a photo of the library, in the Collages project. Its folder, new to the library, is scanned when the job ends (as Scan Always) and is the
    // Collages project: an agent asks until the projects group lists it with the picture.
    let section = eventually(
        "the Collages folder joins the library's projects with the collage",
        || {
            let inspected = agent.tool("doc_inspect", json!({}));
            project_row(&inspected, &collages).filter(|r| r["count"] == json!(1)).map(|r| r["section"].as_str().unwrap().to_string())
        },
        String::new,
    );
    let listed = ids(&agent.tool("doc_inspect", json!({ "photos": section, "limit": 10 })));
    assert_eq!(listed.len(), 1, "{listed:?}");
    let photo = listed[0];
    assert_eq!(result["photo"].as_u64().unwrap_or(photo), photo, "{made}");
    let info = agent.tool("photo_info", json!({ "id": photo }));
    assert_eq!((Path::new(info["path"].as_str().unwrap()), &info["section"]), (jpeg.as_path(), &json!(section)), "{info}");
    assert_eq!((info["dimensions"]["width"].as_u64(), info["dimensions"]["height"].as_u64()), (Some(w), Some(h)), "{info}");
    assert_eq!(sidecar(&collages)["folder"]["project"], json!("collages"), "the folder's mark");

    // Edit Collage reopens the room from the recipe with the eight clips.
    let reopened = agent.tool("cmd_collage_edit", json!({ "id": photo }));
    assert_eq!(reopened["state"], json!("done"), "{reopened}");
    assert_eq!((&reopened["result"]["opened"], &reopened["result"]["missing"]), (&json!(true), &json!([])), "{reopened}");
    let room = agent.tool("cmd_collage_inspect", json!({}));
    assert_eq!((&room["document"]["style"], clip_ids(&room)), (&json!("grid"), eight), "{room}");
}
