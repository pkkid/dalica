//! What the People pane lists (`organizing.md` §6.4), read from the library for the photos it
//! follows ([`crate::commands::people_pane::subject`]: the photo in the edit view, else the
//! selection) and kept until the library or those photos change:
//!
//! - **People in these photos**: each person named on at least one of them, by name (ignoring
//!   case), with how many of the photos hold them and the first of their faces there (its tile,
//!   and the box it highlights);
//! - **Unnamed faces**: their faces nobody is named on yet (ignored faces left out), in photo
//!   order then their order on the photo, at most [`MAX_UNNAMED`] (the rest counted in `more`).
//!
//! The pane reads at most [`MAX_PHOTOS`] photos of a larger selection.

use std::collections::HashMap;
use std::sync::Arc;

use dalica_core::{PersonId, PhotoId, Rect01};
use dalica_library::{FacePerson, Library};
use egui::Id;

use crate::app::App;
use crate::state::FaceRef;

/// The most photos of the selection the pane reads.
pub const MAX_PHOTOS: usize = 10_000;
/// The most unnamed faces the pane lists.
pub const MAX_UNNAMED: usize = 200;

/// One person in the photos.
#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
    /// How many of the photos hold them.
    pub photos: usize,
    /// Their first face in the photos, and its rectangle (stored frame).
    pub face: FaceRef,
    pub rect: Rect01,
}

/// One unnamed face.
#[derive(Clone, Debug, PartialEq)]
pub struct Unnamed {
    pub face: FaceRef,
    pub rect: Rect01,
}

/// The pane's lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub people: Vec<Person>,
    pub unnamed: Vec<Unnamed>,
    /// Unnamed faces past [`MAX_UNNAMED`].
    pub more: usize,
}

/// The lists for `photos` (photos the library lacks are passed over).
pub fn build(lib: &Library, photos: &[PhotoId]) -> Model {
    let mut people: HashMap<PersonId, Person> = HashMap::new();
    let mut unnamed = Vec::new();
    let mut more = 0usize;
    for &photo in photos.iter().take(MAX_PHOTOS) {
        let mut seen: Vec<&PersonId> = Vec::new();
        for (i, f) in lib.faces(photo).iter().enumerate() {
            let Ok(index) = u16::try_from(i) else { break };
            let face = FaceRef { photo, index };
            match &f.person {
                FacePerson::Person(id) => {
                    let Some(p) = lib.person(id) else { continue };
                    let first = !seen.contains(&id);
                    seen.push(id);
                    let row =
                        people.entry(id.clone()).or_insert_with(|| Person { id: id.clone(), name: p.name.clone(), photos: 0, face, rect: f.rect });
                    if first {
                        row.photos += 1;
                    }
                }
                FacePerson::Unnamed if unnamed.len() < MAX_UNNAMED => unnamed.push(Unnamed { face, rect: f.rect }),
                FacePerson::Unnamed => more += 1,
                FacePerson::Ignored => {}
            }
        }
    }
    let mut people: Vec<Person> = people.into_values().collect();
    people.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    Model { people, unnamed, more }
}

/// The lists and what they were read for.
#[derive(Clone, Default)]
struct Cached {
    revision: Option<u64>,
    photos: Vec<PhotoId>,
    model: Arc<Model>,
}

/// The lists for the photos the pane follows, read again only when the library or those photos
/// changed. Empty before the library is open.
pub fn model(app: &App, ctx: &egui::Context) -> Arc<Model> {
    let Some(session) = app.session() else { return Arc::default() };
    let revision = session.library().revision();
    let photos = crate::commands::people_pane::subject(app);
    let key = Id::new("peoplePane.model");
    let hit = ctx.data(|d| d.get_temp::<Cached>(key)).filter(|c| c.revision == Some(revision) && c.photos == photos).map(|c| c.model);
    if let Some(model) = hit {
        return model;
    }
    let model = Arc::new(build(session.library(), &photos));
    ctx.data_mut(|d| d.insert_temp(key, Cached { revision: Some(revision), photos, model: Arc::clone(&model) }));
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn people_are_counted_once_per_photo_and_unnamed_faces_listed_in_order() {
        let (_dir, mut s, ids) = crate::app::tests::session_with_photos(3);
        let ana = s.execute("people.create", &json!({ "name": "ana" })).unwrap()["person"].clone();
        let rui = s.execute("people.create", &json!({ "name": "Rui" })).unwrap()["person"].clone();
        let add = |s: &mut dalica_engine::Session, id: PhotoId, x: f64, person: &serde_json::Value| {
            let mut p = json!({ "id": id, "rect": [x, 0.1, x + 0.1, 0.3] });
            if !person.is_null() {
                p["person"] = person.clone();
            }
            s.execute("faces.add", &p).unwrap();
        };
        add(&mut s, ids[0], 0.1, &ana);
        add(&mut s, ids[0], 0.3, &ana);
        add(&mut s, ids[0], 0.5, &serde_json::Value::Null);
        add(&mut s, ids[1], 0.1, &rui);
        add(&mut s, ids[1], 0.3, &ana);
        add(&mut s, ids[1], 0.5, &serde_json::Value::Null);
        add(&mut s, ids[2], 0.7, &serde_json::Value::Null);
        s.execute("faces.ignore", &json!({ "faces": [format!("{}.0", ids[2])] })).unwrap();
        let m = build(s.library(), &ids[..2]);
        let rows: Vec<(&str, usize, String)> = m.people.iter().map(|p| (p.name.as_str(), p.photos, p.face.to_string())).collect();
        assert_eq!(rows, [("ana", 2, format!("{}.0", ids[0])), ("Rui", 1, format!("{}.0", ids[1]))], "by name ignoring case");
        let unnamed: Vec<String> = m.unnamed.iter().map(|u| u.face.to_string()).collect();
        assert_eq!(unnamed, [format!("{}.2", ids[0]), format!("{}.2", ids[1])]);
        assert_eq!(build(s.library(), &ids[2..]), Model::default(), "an ignored face is not listed");
        assert_eq!(build(s.library(), &[PhotoId::new(999_999)]), Model::default());
    }
}
