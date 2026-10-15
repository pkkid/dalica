//! The photo metadata query: `photo.metadata {id, all?}` (the Properties pane; `ui-layout.md` §7).
//!
//! The rows come in the pane's order: Path, File size, Dimensions, Date taken, Date digitized,
//! File modified, Camera, Lens, Exposure, Aperture, ISO, Focal length, Flash, Orientation, Color
//! space, GPS, Caption, Tags, then Rating when the file holds one (`xmp-interop.md` §2: other
//! ratings are shown, never turned into a star). Rows without a value are left out. What the
//! library knows (path, size, dimensions as displayed, the effective date taken, the file time, and
//! Dalica's caption and tags, which win over the file's for display, `xmp-interop.md` §5) is
//! answered at once; what only the file says comes from the session's
//! [`crate::metadata_cache::MetadataCache`], read on a worker. The first ask answers `pending: true` with the library's rows; ask again (the UI each
//! frame while the pane shows it) for the rest. The owner thread never reads the file.
//!
//! With `all`, the answer adds Show All Metadata: every field of the file as `{group, label,
//! value}` rows (`dalica_meta::listing`). `file` gives the owned fields as the file holds them, and
//! `changedElsewhere` the note of `xmp-interop.md` §5 rule 3 when the scan or the write-back reported one.

use dalica_core::{DateTime, PhotoId};
use dalica_library::Library;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::cmd::photo::displayed_size;
use crate::error::CmdError;
use crate::metadata_cache::{FileMetadata, Lookup};
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;

/// The note shown when another application changed a field Dalica kept its own value of.
pub const CHANGED_ELSEWHERE: &str = "Changed in another application; Dalica's value kept";
/// The note of a date taken that is the file's modification time (`xmp-interop.md` §2).
pub const DATE_FROM_FILE: &str = "date from file";

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "photo.metadata",
        label: "Photo Metadata",
        menu: "",
        keys: &[],
        params: r#"{"id":PhotoId, "all"?:bool}: the Properties rows [{key, label, value, note?}] of a photo; the file's fields are read on a worker, so the first ask answers pending: true with the library's rows only (ask again). With all: every metadata field as all.rows [{group, label, value}]. Also file (the owned fields the file holds), problems, error (the file could not be read) and changedElsewhere"#,
        effect: Effect::Query,
        history: History::None,
        enabled: crate::registry::always,
        run: metadata,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    id: PhotoId,
    #[serde(default)]
    all: bool,
}

/// The Properties rows, built in the pane's order.
#[derive(Default)]
struct Rows(Vec<Value>);

impl Rows {
    fn push(&mut self, key: &str, label: &str, value: Option<String>) {
        self.push_noted(key, label, value, None);
    }

    fn push_noted(&mut self, key: &str, label: &str, value: Option<String>, note: Option<&str>) {
        let Some(value) = value.filter(|v| !v.is_empty()) else { return };
        let mut row = Map::new();
        row.insert("key".into(), json!(key));
        row.insert("label".into(), json!(label));
        row.insert("value".into(), json!(value));
        if let Some(n) = note {
            row.insert("note".into(), json!(n));
        }
        self.0.push(Value::Object(row));
    }
}

fn metadata(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Query { id, all } = params(p)?;
    let key = s.file_key(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let answer = s.metadata_cache.lookup(&key);
    let file = match &answer {
        Lookup::Ready(m) => Some(m.as_ref()),
        Lookup::Pending | Lookup::Failed(_) => None,
    };
    let lib = s.library();
    let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("name".into(), json!(photo.name));
    out.insert("pending".into(), json!(answer == Lookup::Pending));
    out.insert("rows".into(), Value::Array(rows(lib, id, &key.path.display().to_string(), file).0));
    let changed = s.metadata_cache.note(&key);
    if !changed.is_empty() {
        out.insert("changedElsewhere".into(), json!({ "fields": changed.iter().collect::<Vec<_>>(), "note": CHANGED_ELSEWHERE }));
    }
    match &answer {
        Lookup::Ready(m) => {
            out.insert("file".into(), owned(m));
            out.insert("problems".into(), json!(m.problems));
            if all {
                let rows: Vec<Value> = m.listing.rows.iter().map(|r| json!({ "group": r.group, "label": r.label, "value": r.value })).collect();
                out.insert("all".into(), json!({ "rows": rows, "truncated": m.listing.truncated }));
            }
        }
        Lookup::Failed(e) => {
            out.insert("error".into(), json!(e));
        }
        Lookup::Pending => {}
    }
    Ok(Value::Object(out))
}

/// The Properties rows of photo `id` at `path`: the library's, and the file's when it was read.
fn rows(lib: &Library, id: PhotoId, path: &str, file: Option<&FileMetadata>) -> Rows {
    let mut r = Rows::default();
    let Some(photo) = lib.photo(id) else { return r };
    let info = lib.info(id);
    let summary = file.map(|f| &f.summary);
    r.push("path", "Path", Some(path.to_string()));
    r.push("fileSize", "File size", Some(size_text(photo.size)));
    let (w, h) = displayed_size(&info.edits, photo.width, photo.height);
    r.push("dimensions", "Dimensions", (w > 0 && h > 0).then(|| format!("{w} × {h}")));
    let modified = DateTime::from_unix_secs(photo.mtime.as_secs());
    match lib.effective_date(id) {
        Some(d) => r.push("dateTaken", "Date taken", Some(d.to_string())),
        None => r.push_noted("dateTaken", "Date taken", modified.map(|d| d.to_string()), Some(DATE_FROM_FILE)),
    }
    r.push("dateDigitized", "Date digitized", summary.and_then(|s| s.date_digitized.as_ref()).map(ToString::to_string));
    r.push("fileModified", "File modified", modified.map(|d| d.to_string()));
    r.push("camera", "Camera", summary.and_then(dalica_meta::exif::Summary::camera));
    r.push("lens", "Lens", summary.and_then(|s| s.lens.clone()));
    r.push("exposure", "Exposure", summary.and_then(dalica_meta::exif::Summary::exposure));
    r.push("aperture", "Aperture", summary.and_then(dalica_meta::exif::Summary::aperture));
    r.push("iso", "ISO", summary.and_then(|s| s.iso).map(|i| i.to_string()));
    r.push("focalLength", "Focal length", summary.and_then(dalica_meta::exif::Summary::focal));
    r.push("flash", "Flash", summary.and_then(|s| s.flash_text()).map(str::to_owned));
    r.push("orientation", "Orientation", summary.and_then(|s| s.orientation_text()).map(str::to_owned));
    r.push("colorSpace", "Color space", summary.and_then(|s| s.color_space_text()).map(str::to_owned));
    r.push("gps", "GPS", file.and_then(|f| f.owned.place.as_ref()).map(ToString::to_string));
    r.push("caption", "Caption", Some(info.caption.clone()));
    r.push("tags", "Tags", Some(info.tags.join(", ")));
    r.push("rating", "Rating", file.and_then(|f| f.owned.rating).map(|n| n.to_string()));
    r
}

/// The owned fields as the file holds them (`xmp-interop.md` §2).
fn owned(m: &FileMetadata) -> Value {
    let o = &m.owned;
    let source = o.date_source.map(|s| match s {
        dalica_meta::DateSource::Exif => "exif",
        dalica_meta::DateSource::XmpDateTimeOriginal => "xmpDateTimeOriginal",
        dalica_meta::DateSource::XmpDateCreated => "xmpDateCreated",
        dalica_meta::DateSource::XmpCreateDate => "xmpCreateDate",
        dalica_meta::DateSource::Iptc => "iptc",
    });
    json!({
        "caption": o.caption,
        "tags": o.tags,
        "rating": o.rating,
        "place": o.place.map(|p| json!({ "latitude": p.latitude(), "longitude": p.longitude() })),
        "dateTaken": o.date_taken.as_ref().map(ToString::to_string),
        "dateSource": source,
    })
}

/// `3.1 MB`: bytes in decimal units with one decimal (below 1 KB, whole bytes), as the status
/// line writes them.
fn size_text(bytes: u64) -> String {
    const UNITS: [(f64, &str); 4] = [(1e12, "TB"), (1e9, "GB"), (1e6, "MB"), (1e3, "KB")];
    let b = bytes as f64;
    match UNITS.iter().find(|(scale, _)| b >= *scale) {
        Some((scale, unit)) => format!("{:.1} {unit}", b / scale),
        None if bytes == 1 => "1 byte".to_string(),
        None => format!("{bytes} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::cmd::testing::library;

    #[test]
    fn sizes_read_like_the_status_line() {
        assert_eq!(size_text(0), "0 bytes");
        assert_eq!(size_text(1), "1 byte");
        assert_eq!(size_text(999), "999 bytes");
        assert_eq!(size_text(1_500), "1.5 KB");
        assert_eq!(size_text(3_100_000), "3.1 MB");
        assert_eq!(size_text(u64::MAX), "18446744.1 TB");
    }

    #[test]
    fn bad_params_and_unknown_photos() {
        let (_dirs, mut s, ids) = library(1);
        assert!(matches!(s.execute("photo.metadata", &json!({ "id": 999 })), Err(CmdError::NotFound(_))));
        for bad in [
            json!({}),
            json!({ "id": "1" }),
            json!({ "id": -1 }),
            json!({ "ids": [ids[0]] }),
            json!({ "id": ids[0], "all": "yes" }),
            json!({ "id": ids[0], "extra": 1 }),
        ] {
            assert!(matches!(s.execute("photo.metadata", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }
}
