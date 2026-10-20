//! The output helpers (`mcp.md` §4.2, §6, §10): `render_photo` renders through the one output
//! pipeline without changing the library, and writes only new files inside a write root;
//! `export` runs `export.run` with the dialog's options, reporting and stopping per photo. What
//! the pipeline and the export job do is proved in `dalica-create` and `dalica-engine`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::common::{base64_decode, code, ok};
use crate::edit_tools::{folder, invalid};

/// A decoded image: its size and its 8-bit RGBA pixels.
fn rgb(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let d = dalica_image::decode(bytes, &Default::default()).unwrap();
    (d.width, d.height, d.data)
}

fn image(result: &Value) -> Vec<u8> {
    let block = &result["content"][0];
    assert_eq!(block["type"], "image", "{result}");
    base64_decode(block["data"].as_str().unwrap())
}

#[test]
fn render_photo_renders_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = folder(dir.path(), 1, (640, 480), None, &[]);
    ok(&c.call(2, "apply_edit", json!({ "ids": [1], "op": "crop", "params": { "rect": [0, 0, 0.5, 1] } })));
    let inspect = json!({ "id": "photo.inspect", "params": { "ids": [1] } });
    let before_all = ok(&c.call(3, "command_run", inspect.clone())).clone();

    // With its edits; without them, the decoded original exactly; a recipe previewed, not applied.
    let edited = c.call(4, "render_photo", json!({ "id": 1 }));
    assert_eq!((ok(&edited)["width"].as_u64(), ok(&edited)["height"].as_u64()), (Some(320), Some(480)), "{edited}");
    assert_eq!(ok(&edited)["recipe"], json!(["crop(0,0,0.5,1)"]));
    let original = std::fs::read(dir.path().join("Photos").join("IMG_0001.jpg")).unwrap();
    let plain = c.call(5, "render_photo", json!({ "id": 1, "before": true, "size": 2048, "format": "png" }));
    assert_eq!((ok(&plain)["mimeType"].as_str(), ok(&plain)["recipe"].clone()), (Some("image/png"), json!([])));
    assert_eq!(rgb(&image(&plain)), rgb(&original), "before: the decoded original, pixel for pixel");
    let previewed = c.call(6, "render_photo", json!({ "id": 1, "recipe": "crop(0,0,0.5,0.5)", "size": 100, "format": "jpeg" }));
    assert_eq!((ok(&previewed)["width"].as_u64(), ok(&previewed)["height"].as_u64()), (Some(100), Some(75)), "never enlarged, fit to 100");
    assert_eq!(ok(&previewed)["mimeType"], "image/jpeg");
    let (w, h, _) = rgb(&image(&previewed));
    assert_eq!((w, h), (100, 75));
    assert_eq!(ok(&c.call(7, "command_run", inspect.clone())), &before_all, "renders change nothing in the library");

    // `path`: a new file inside a write root (the temporary folder), never over one, never in a
    // watched folder or outside the roots.
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("render.png");
    let written = c.call(8, "render_photo", json!({ "id": 1, "size": 64, "path": target }));
    // The answer names the file as it was asked for (never `/private/var/…` for `/var/…` on
    // macOS, never a verbatim `\\?\` path on Windows), and that name is the very file written.
    assert_eq!(ok(&written)["path"], json!(target));
    let answered = PathBuf::from(ok(&written)["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&answered).unwrap(), image(&written), "the file is the image returned");
    assert_eq!(code(&c.call(9, "render_photo", json!({ "id": 1, "path": target }))), "path_denied", "never over a file");
    let outside: PathBuf = std::env::temp_dir().parent().unwrap().join("dalica-mcp-render-denied.png");
    assert_eq!(code(&c.call(10, "render_photo", json!({ "id": 1, "path": outside }))), "path_denied");
    let watched = dir.path().join("Photos").join("render.png");
    assert_eq!(code(&c.call(11, "render_photo", json!({ "id": 1, "path": watched }))), "path_denied");
    assert!(!watched.exists() && !outside.exists());
    assert_eq!(code(&c.call(12, "render_photo", json!({ "id": 1, "path": "render.png" }))), "bad_params", "a relative path");

    // Limits, graceful failures, strict arguments.
    assert_eq!(code(&c.call(13, "render_photo", json!({ "id": 1, "size": 2049 }))), "limit");
    assert_eq!(code(&c.call(14, "render_photo", json!({ "id": 1, "size": 8 }))), "limit");
    assert_eq!(code(&c.call(15, "render_photo", json!({}))), "disabled", "nothing in the tray");
    assert_eq!(code(&c.call(16, "render_photo", json!({ "id": 99 }))), "not_found");
    assert_eq!(code(&c.call(17, "render_photo", json!({ "id": 1, "before": true, "recipe": "" }))), "bad_params");
    assert_eq!(code(&c.call(18, "render_photo", json!({ "id": 1, "recipe": [7] }))), "bad_params");
    invalid(&mut c, 19, "render_photo", json!({ "id": 1, "format": "gif" }));
    invalid(&mut c, 20, "render_photo", json!({ "id": 1, "max_side": 64 }));
    invalid(&mut c, 21, "render_photo", json!({ "id": "1" }));
    assert_eq!(ok(&c.call(22, "command_run", inspect)), &before_all);
    c.close();
}

/// The files of an export folder (its sidecar left out).
fn exported(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.file_name().unwrap().to_string_lossy() != ".dalica.json").collect();
    files.sort();
    files
}

#[test]
fn export_writes_jpegs_at_the_long_edge() {
    let (dir, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut c = folder(dir.path(), 5, (2400, 1600), None, &[]);
    let r = ok(&c.call(2, "export", json!({ "ids": [1, 2, 3, 4, 5], "dir": out.path(), "folder_name": "Out" }))).clone();
    assert_eq!(
        (&r["total"], &r["done"], &r["cancelled"], &r["skipped"], &r["failed"]),
        (&json!(5), &json!(5), &json!(false), &json!([]), &json!([])),
        "{r}"
    );
    assert_eq!(r["folder"], json!(out.path().join("Out")));
    let files = exported(&out.path().join("Out"));
    assert_eq!(files.len(), 5, "{files:?}");
    let listed = r["files"].as_array().unwrap();
    let mut ids: Vec<u64> = listed.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2, 3, 4, 5], "each file names its photo");
    for f in listed {
        let path = PathBuf::from(f["path"].as_str().unwrap());
        assert!(files.contains(&path) && path.extension().unwrap() == "jpg", "{f}");
        let bytes = std::fs::read(&path).unwrap();
        let (w, h, _) = rgb(&bytes);
        assert!(w.max(h) <= 1600, "{w}×{h}");
        assert_eq!(
            (f["width"].as_u64(), f["height"].as_u64(), f["bytes"].as_u64()),
            (Some(u64::from(w)), Some(u64::from(h)), Some(bytes.len() as u64))
        );
    }

    // The options mapped: size 0 is the original size, numbered names; the tray without ids.
    let full =
        ok(&c.call(3, "export", json!({ "ids": [2], "dir": out.path(), "folder_name": "Full", "size": 0, "numbered": true, "quality": "auto" })))
            .clone();
    assert_eq!((&full["files"][0]["id"], &full["files"][0]["width"]), (&json!(2), &json!(2400)), "{full}");
    assert!(full["files"][0]["path"].as_str().unwrap().ends_with("001-IMG_0002.jpg"), "{full}");
    assert_eq!(code(&c.call(4, "export", json!({ "dir": out.path() }))), "disabled", "an empty tray");

    // Graceful failures and strict arguments.
    assert_eq!(code(&c.call(5, "export", json!({ "ids": [1], "dir": out.path(), "videos": "frame" }))), "unsupported");
    assert_eq!(code(&c.call(6, "export", json!({ "ids": [1], "dir": out.path(), "size": 50 }))), "bad_params");
    assert_eq!(code(&c.call(7, "export", json!({ "ids": [99], "dir": out.path(), "folder_name": "X" }))), "not_found");
    let outside = std::env::temp_dir().parent().unwrap().join("dalica-mcp-export-denied");
    assert_eq!(code(&c.call(8, "export", json!({ "ids": [1], "dir": outside, "folder_name": "X" }))), "path_denied");
    invalid(&mut c, 9, "export", json!({ "ids": [1] }));
    invalid(&mut c, 10, "export", json!({ "ids": [1], "dir": out.path(), "folderName": "X" }));
    invalid(&mut c, 11, "export", json!({ "ids": [1], "dir": out.path(), "size": "big" }));
    assert_eq!(exported(out.path()).len(), 2, "only Out and Full were made: {:?}", exported(out.path()));
    c.close();
}

/// A cancelled export stops before its next photo: no response, and the files done are complete.
///
/// The folder is read only once the export has ended. A `ping` is no such point: the server
/// answers it while the tool is still running (`mcp.md` §6), as the photos in flight when the
/// cancel arrives are still being written to their temporary names and renamed into place, so a
/// listing taken then could name a temporary file gone by the time it is read. Any other request
/// waits for the tool to return, and the tool returns only once the cancelled job has ended.
#[test]
fn a_cancelled_export_leaves_only_complete_files() {
    let (dir, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut c = folder(dir.path(), 40, (1200, 800), None, &[]);
    let ids: Vec<u64> = (1..=40).collect();
    c.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "export", "arguments": { "ids": ids, "dir": out.path(), "folder_name": "Out", "size": 0 },
        "_meta": { "progressToken": "x" },
    } }));
    loop {
        let v = c.recv();
        assert_ne!(v.get("id"), Some(&json!(2)), "the export ended before it could be cancelled: {v}");
        if v["method"] == "notifications/progress" && v["params"]["progress"].as_u64().unwrap_or(0) >= 1 {
            break;
        }
    }
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 2 } }));
    c.send(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }));
    let (lines, _) = c.until(&json!(3));
    assert!(lines.iter().all(|l| l.get("id") != Some(&json!(2))), "a cancelled request gets no response: {lines:?}");
    let files = exported(&out.path().join("Out"));
    assert!(!files.is_empty() && files.len() < 40, "{} of 40 written", files.len());
    let temporary = files.iter().filter(|f| f.file_name().unwrap().to_string_lossy().starts_with('.')).collect::<Vec<_>>();
    assert!(temporary.is_empty(), "no temporary file is left: {temporary:?}");
    for f in &files {
        let (w, h, _) = rgb(&std::fs::read(f).unwrap());
        assert_eq!((w, h), (1200, 800), "{}", f.display());
    }
    c.close();
}
