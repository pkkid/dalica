//! `dalica-cli inspect`: a library as JSON — watched folders, folders with their photo and star
//! counts, starred photos, and what opening the library repaired. Reads only (it still takes the
//! library lock, like every session).

use std::process::ExitCode;

use serde_json::json;

pub fn main(args: &[String]) -> ExitCode {
    let Some(dir) = crate::library_arg(args) else {
        eprintln!("dalica-cli inspect: --library DIR is required");
        return ExitCode::from(2);
    };
    let mut s = match crate::open(&dir) {
        Ok(s) => s,
        Err(code) => return code,
    };
    s.wait_idle();
    let lib = s.library();
    let folders: Vec<_> = lib
        .folders_in_display_order()
        .iter()
        .map(|f| {
            let photos = lib.photos_in(f.id);
            json!({ "id": f.id, "path": f.path, "photos": photos.len(), "starred": photos.iter().filter(|p| p.star).count() })
        })
        .collect();
    let starred: Vec<_> = lib
        .starred()
        .iter()
        .filter_map(|p| Some(json!({ "id": p.id, "path": lib.folder(p.folder)?.path.join(&p.name), "width": p.width, "height": p.height })))
        .collect();
    let recovery: Vec<String> = s.recovery().iter().map(|r| format!("{r:?}")).collect();
    let summary = json!({
        "library": dir,
        "roots": s.roots(),
        "folders": folders,
        "photos": lib.photos().count(),
        "starred": starred,
        "recovery": recovery,
        "notices": s.notices(),
        "scanWarnings": s.scan_warnings(),
    });
    crate::print(&serde_json::to_string_pretty(&summary).unwrap_or_default())
}
