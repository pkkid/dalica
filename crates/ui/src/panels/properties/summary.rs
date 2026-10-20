//! The Properties pane for several photos or a folder selected as a unit: what the library knows of
//! them, with no file read. Several photos: Pictures, Dates (the first and last day taken, the
//! user's dates first), Size on disk and Tags (most used first, "lisbon (40), tram (6)"); a folder
//! adds its Path, Place and Description at the top. Written as the status line writes them
//! ([`crate::status_text`]). Kept per pane and made again only when the library or the selection
//! changed.

use std::sync::Arc;

use dalica_core::{FolderId, PhotoId};
use dalica_library::Library;
use egui::Id;

use super::answer::{Row, one_line};
use crate::app::App;
use crate::status_text as status;
use crate::tl;

fn row(key: &str, label: String, value: String) -> Row {
    Row { key: key.to_owned(), label, value: one_line(&value), note: None }
}

/// The rows for the photos `ids` (unknown ones left out) and, for a folder selected as a unit,
/// `folder`.
pub fn rows(library: &Library, ids: &[PhotoId], folder: Option<FolderId>) -> Vec<Row> {
    let mut out = Vec::new();
    if let Some(f) = folder.and_then(|f| library.folder(f).map(|folder| (f, folder))) {
        out.push(row("path", tl!(ctx = "property", "Path"), f.1.path.display().to_string()));
        let info = library.folder_info(f.0);
        if !info.place.is_empty() {
            out.push(row("place", tl!(ctx = "property", "Place"), info.place.clone()));
        }
        if !info.description.is_empty() {
            out.push(row("description", tl!(ctx = "property", "Description"), info.description.clone()));
        }
    }
    let photos: Vec<_> = ids.iter().filter_map(|id| library.photo(*id)).collect();
    out.push(row("pictures", tl!(ctx = "property", "Pictures"), status::grouped(photos.len())));
    if let Some(dates) = status::date_range(photos.iter().filter_map(|p| library.sort_date(p).map(|d| d.date()))) {
        out.push(row("dates", tl!(ctx = "property", "Dates"), dates));
    }
    let bytes = photos.iter().fold(0u64, |sum, p| sum.saturating_add(p.size));
    out.push(row("size", tl!(ctx = "property", "Size on disk"), status::size(bytes)));
    let tags = status::selection_tags(library, ids);
    if !tags.is_empty() {
        let list: Vec<String> = tags.iter().map(|(tag, n)| format!("{tag} ({n})")).collect();
        out.push(row("tags", tl!(ctx = "property", "Tags"), list.join(&tl!(ctx = "list", ", "))));
    }
    out
}

/// The rows last made and what for.
#[derive(Clone, Default)]
struct Kept {
    revision: Option<u64>,
    ids: Vec<PhotoId>,
    folder: Option<FolderId>,
    /// The language the rows were written in.
    language: &'static str,
    rows: Arc<Vec<Row>>,
}

/// [`rows`] for the selection, made again only when the library or the selection changed
/// (compared in place, so an unchanged frame copies nothing).
pub fn kept(app: &App, ctx: &egui::Context, folder: Option<FolderId>) -> Arc<Vec<Row>> {
    let key = Id::new("properties.summary");
    let Some(library) = app.session().map(dalica_engine::Session::library) else { return Arc::default() };
    let revision = library.revision();
    let ids = &app.state().selection.ids;
    let language = crate::i18n::current().code;
    let hit = ctx.data_mut(|d| {
        let k = d.get_temp_mut_or_default::<Kept>(key);
        (k.revision == Some(revision) && k.folder == folder && k.ids == *ids && k.language == language).then(|| Arc::clone(&k.rows))
    });
    if let Some(rows) = hit {
        return rows;
    }
    let made = Arc::new(rows(library, ids, folder));
    ctx.data_mut(|d| d.insert_temp(key, Kept { revision: Some(revision), ids: ids.clone(), folder, language, rows: Arc::clone(&made) }));
    made
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn several_photos_and_a_folder() {
        let (_dir, mut s, ids) = session_with_photos(3);
        s.execute("photo.tags.add", &json!({ "ids": [ids[0], ids[1]], "tags": "tram" })).unwrap();
        s.execute("photo.tags.add", &json!({ "ids": [ids[1]], "tags": "beach" })).unwrap();
        let folder = s.library().photo(ids[0]).unwrap().folder;
        s.execute("folder.info.set", &json!({ "id": folder, "place": "Lisbon", "description": "A weekend\naway" })).unwrap();
        let lib = s.library();
        let keys = |rows: &[Row]| rows.iter().map(|r| r.key.clone()).collect::<Vec<_>>();
        let two = rows(lib, &[ids[0], ids[1], PhotoId::new(999_999)], None);
        assert_eq!(keys(&two), ["pictures", "dates", "size", "tags"]);
        assert_eq!(two[0].value, "2", "unknown photos are left out");
        assert_eq!(two[3].value, "tram (2), beach (1)");
        let all = rows(lib, &ids, Some(folder));
        assert_eq!(keys(&all), ["path", "place", "description", "pictures", "dates", "size", "tags"]);
        assert_eq!(all[2].value, "A weekend away", "one line");
        assert_eq!(all[0].value, lib.folder(folder).unwrap().path.display().to_string());
        let none = rows(lib, &[], Some(FolderId::new(999_999)));
        assert_eq!(keys(&none), ["pictures", "size"]);
        assert_eq!((none[0].value.as_str(), none[1].value.as_str()), ("0", "0 bytes"));
    }
}
