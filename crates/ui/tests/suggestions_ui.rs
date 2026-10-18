//! Suggestions in the window (M13.30, `organizing.md` §6.2–§6.4, `faces.md` §5), driven headlessly
//! by widget id on nine photos with synthetic embeddings: Ana's face on photos 1 and 2, Bo's on 3,
//! unnamed faces near Ana's on 4 to 6, near Bo's on 7, far from both on 8, and a near miss of
//! Ana's (below the Suggestion threshold, above Find More's bar) on 9.
//!
//! - the tree's "?" badge is lit from the pending count, and goes when none are left;
//! - Ana's section shows her suggestions first, each with "Ana?", ✓ and ×, under the strip
//!   "Suggestions:"; ✓ moves one into her photos as one undo step; × rejects one for good (no undo
//!   step); Show only suggestions hides her confirmed photos; Confirm All is one step;
//! - Find More adds the near miss to her section, and its ✓ names it hers;
//! - "Name?" on the Unnamed grid and in the People pane confirms;
//! - at 1400 × 900 the strip (with the header) and a suggested face with its marks match
//!   `tests/fingerprints/suggestions-strip-1400x900.json` and `suggestions-face-1400x900.json`
//!   (`DALICA_BLESS=1` stores new ones; renders and the review's screenshots go to
//!   `target/test-output/ui/`).
//!
//! The embeddings are planted in the recogniser's cache, as the engine's own suggestion tests do
//! (`crates/engine/tests/suggestions.rs`): a four-number recogniser with `matchCosine` 0.5 and no
//! table, so the Suggestion threshold is 0.5 and Find More's bar 0.4; no photo is ever read for a
//! face.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, SMALL, check_rect, new_driver, shot, try_widget, widget};
use dalica_core::{PersonId, PhotoId, Rect01};
use dalica_engine::Session;
use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::known;
use dalica_faces::manifest::{InputSpec, OutputSpec};
use dalica_faces::synthetic::encode;
use dalica_fs::Expect;
use dalica_store::faces_cache::{CachedFace, FacesCache, Model, PhotoStamp, Record};
use dalica_ui::state::{AlbumSection, FaceRef, Pane, PersonKey, Section};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(120);
const RECOGNISER: &str = "test-recogniser";

const ANA: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const BO: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
/// Near Ana's and far enough from [`NEAR_MISS`] that, confirmed, they never make it a suggestion.
const NEAR_ANA: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.3], [1.0, 0.1, 0.0, 0.2], [0.9, 0.0, 0.0, 0.3]];
const NEAR_BO: [f32; 4] = [0.1, 1.0, 0.2, 0.0];
const FAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// 0.45 from Ana: below the Suggestion threshold, above Find More's bar.
const NEAR_MISS: [f32; 4] = [0.45, 0.0, 0.893, 0.0];
const RECT: [f64; 4] = [0.3, 0.15, 0.65, 0.75];

/// A recogniser giving four numbers per face (a 1 × 1 convolution, average pooling, flattening).
fn recogniser_bytes() -> Vec<u8> {
    let nodes = vec![
        Node::new("Conv", &["data", "w", "b"], "c", &[("kernel_shape", Attr::Ints(vec![1, 1]))]),
        Node::new("GlobalAveragePool", &["c"], "p", &[]),
        Node::new("Flatten", &["p"], "emb", &[("axis", Attr::Int(1))]),
    ];
    let mix = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let weights = [("w".to_string(), Weight::f32(&[4, 3, 1, 1], mix)), ("b".to_string(), Weight::f32(&[4], vec![0.0; 4]))];
    encode(&Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("data".into(), vec![1, 3, 112, 112])], outputs: vec!["emb".into()] })
}

/// A models folder holding the recogniser (no detector: nothing new is found); its SHA-256.
fn models(root: &Path) -> (PathBuf, String) {
    let bytes = recogniser_bytes();
    let sha = dalica_faces::hash::sha256_hex(&bytes);
    let mut m = known::sface();
    m.id = RECOGNISER.into();
    m.input = InputSpec::default();
    m.output = OutputSpec::Embedding { dim: 4 };
    m.thresholds.match_cosine = Some(0.5);
    m.calibration = None;
    m.sha256 = Some(sha.clone());
    m.size_bytes = Some(bytes.len() as u64);
    let models = root.join("models");
    let dir = models.join(RECOGNISER);
    dalica_fs::create_dir_all(&dir).unwrap();
    dalica_fs::replace(&dir.join("model.onnx"), &bytes, Expect::Absent).unwrap();
    dalica_fs::replace(&dir.join("face-model.json"), &serde_json::to_vec_pretty(&m).unwrap(), Expect::Absent).unwrap();
    (models, sha)
}

fn unit(v: [f32; 4]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    photos: Vec<PhotoId>,
    ana: PersonId,
    bo: PersonId,
}

/// The nine photos and their faces (see the module's doc), the embeddings loaded, at `w × h`.
fn setup(w: u32, h: u32) -> T {
    dalica_ui::commands::suggestions::reset();
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 9, 47).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let (models, sha) = models(dir.path());
    let library = dir.path().join("library");
    let mut s = Session::open(&library, Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 9);
    let person = |s: &mut Session, name: &str| -> PersonId {
        let r = s.execute("people.create", &json!({ "name": name })).unwrap();
        PersonId::parse(r["person"].as_str().unwrap()).unwrap()
    };
    let (ana, bo) = (person(&mut s, "Ana"), person(&mut s, "Bo"));
    let who: [(Option<&PersonId>, [f32; 4]); 9] = [
        (Some(&ana), ANA),
        (Some(&ana), ANA),
        (Some(&bo), BO),
        (None, NEAR_ANA[0]),
        (None, NEAR_ANA[1]),
        (None, NEAR_ANA[2]),
        (None, NEAR_BO),
        (None, FAR),
        (None, NEAR_MISS),
    ];
    let mut records = Vec::new();
    for (id, (person, v)) in ids.iter().zip(who) {
        let mut p = json!({ "id": id, "rect": RECT });
        if let Some(person) = person {
            p["person"] = json!(person);
        }
        s.execute("faces.add", &p).unwrap();
        let photo = PhotoStamp::of(s.library().photo(*id).unwrap());
        let [x0, y0, x1, y1] = RECT;
        records.push(Record::Face { photo, face: CachedFace { rect: Rect01::new(x0, y0, x1, y1).unwrap(), landmarks: None, embedding: unit(v) } });
    }
    // The cache is written off the session's thread, as any other writer's would be.
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let (mut cache, _) = FacesCache::open(&library, &Model { id: RECOGNISER, sha256: &sha }, 4).unwrap();
            cache.append(&records).unwrap();
        })
        .join()
        .unwrap();
    });
    s.set_face_models_dir(Some(models));
    assert!(s.wait_faces(WAIT), "the face scan loaded the cache");
    assert_eq!(s.faces_index().faces(), 9, "the planted embeddings are in the index");
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, photos: ids, ana, bo }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    t.d.settle(FRAMES).unwrap();
}

fn has(t: &T, id: &str) -> bool {
    try_widget(&t.d, id).is_some()
}

fn section_of(p: &PersonId) -> Section {
    Section::Album(AlbumSection::Person(PersonKey::of(p)))
}

/// The face on photo `i` (each photo holds one).
fn face(t: &T, i: usize) -> FaceRef {
    FaceRef { photo: t.photos[i], index: 0 }
}

/// Its tile in `p`'s section.
fn tile(t: &T, i: usize, p: &PersonId) -> String {
    format!("face:{}/in/{p}", face(t, i))
}

fn person_of(t: &T, i: usize) -> Option<PersonId> {
    let f = face(t, i);
    let lib = t.d.app().session().unwrap().library();
    lib.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) }).unwrap().person.person().cloned()
}

fn section_photos(t: &T, p: &PersonId) -> Vec<PhotoId> {
    t.d.app().panels().unwrap().sections.get(section_of(p)).map(|s| s.photos.clone()).unwrap_or_default()
}

/// The "?" count of `p`'s row in the tree.
fn badge(t: &T, p: &PersonId) -> usize {
    let m = dalica_ui::panels::folders::current_model(t.d.ctx());
    m.row_of_section(section_of(p)).and_then(|i| m.row(i)).map(|r| r.pending).unwrap()
}

fn ids(t: &T, of: &[usize]) -> Vec<PhotoId> {
    of.iter().map(|i| t.photos[*i]).collect()
}

#[test]
fn anas_section_confirms_rejects_and_confirms_all_her_suggestions() {
    let mut t = setup(LARGE.0, LARGE.1);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    assert_eq!((badge(&t, &ana), badge(&t, &bo)), (3, 1), "the badges count what waits");
    click(&mut t, &format!("tree:person:{ana}"));
    let header = format!("section:person:{ana}");
    // Her suggestions first, then her own photos.
    let photos = section_photos(&t, &ana);
    assert_eq!(photos[..3], ids(&t, &[3, 4, 5]), "{photos:?}");
    assert_eq!(photos.len(), 5);
    for part in ["confirmAll", "confirm", "reject", "findMore", "onlySuggestions"] {
        assert!(has(&t, &format!("{header}/{part}")), "{part}");
    }
    assert_eq!(widget(&t.d, &tile(&t, 3, &ana)).label, "Ana?");
    assert!(!has(&t, &tile(&t, 3, &bo)) && !has(&t, &tile(&t, 6, &ana)), "Bo's suggestion is not hers");

    // ✓ moves one into her photos, as one undo step.
    let id = format!("{}/confirm", tile(&t, 3, &ana));
    click(&mut t, &id);
    assert_eq!(person_of(&t, 3), Some(ana.clone()));
    assert_eq!(badge(&t, &ana), 2);
    assert!(!has(&t, &tile(&t, 3, &ana)) && section_photos(&t, &ana).contains(&t.photos[3]), "now one of her photos");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!((person_of(&t, 3), badge(&t, &ana)), (None, 3));

    // The strip's Confirm acts on the suggestion selected in her section.
    let id = tile(&t, 5, &ana);
    click(&mut t, &id);
    assert!(widget(&t.d, &format!("{header}/confirm")).enabled);
    click(&mut t, &format!("{header}/confirm"));
    assert_eq!(person_of(&t, 5), Some(ana.clone()));
    run(&mut t, "edit.undo", json!({}));

    // × rejects one for good: no undo step brings it back.
    let id = format!("{}/reject", tile(&t, 4, &ana));
    click(&mut t, &id);
    assert_eq!((person_of(&t, 4), badge(&t, &ana)), (None, 2));
    assert!(!section_photos(&t, &ana).contains(&t.photos[4]));
    let suggested = t.d.app_mut().session_mut().unwrap().execute("faces.suggest", &json!({ "person": ana })).unwrap();
    assert_eq!(suggested["total"], json!(2));

    // Show only suggestions hides her confirmed photos.
    click(&mut t, &format!("{header}/onlySuggestions"));
    assert_eq!(t.d.state().people.only_suggestions, std::slice::from_ref(&ana));
    assert_eq!(section_photos(&t, &ana), ids(&t, &[3, 5]));
    click(&mut t, &format!("{header}/onlySuggestions"));
    assert_eq!(section_photos(&t, &ana).len(), 4);

    // Confirm All: both, as one step; the badge and the strip go when none are left.
    click(&mut t, &format!("{header}/confirmAll"));
    assert_eq!((person_of(&t, 3), person_of(&t, 5)), (Some(ana.clone()), Some(ana.clone())));
    assert_eq!(badge(&t, &ana), 0);
    assert!(!has(&t, &format!("{header}/confirmAll")), "the strip goes with them");
    assert_eq!(badge(&t, &bo), 1, "Bo's still waits");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!((person_of(&t, 3), person_of(&t, 5), badge(&t, &ana)), (None, None, 2), "one step");
}

#[test]
fn find_more_the_unnamed_grid_and_the_people_pane_confirm_too() {
    let mut t = setup(LARGE.0, LARGE.1);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    // Find More: the near miss joins her section, and its ✓ names it hers.
    click(&mut t, &format!("tree:person:{ana}"));
    assert!(!section_photos(&t, &ana).contains(&t.photos[8]));
    click(&mut t, &format!("section:person:{ana}/findMore"));
    assert!(section_photos(&t, &ana).contains(&t.photos[8]), "{:?}", section_photos(&t, &ana));
    let id = format!("{}/confirm", tile(&t, 8, &ana));
    click(&mut t, &id);
    assert_eq!(person_of(&t, 8), Some(ana.clone()));
    assert_eq!(badge(&t, &ana), 3, "Find More's faces are no suggestions");

    // The Unnamed grid: "Bo?" with ✓ / ×; ✓ names the face Bo's.
    click(&mut t, "tree:person:unnamed");
    assert_eq!(widget(&t.d, &face(&t, 6).widget()).label, "Bo?");
    assert!(!has(&t, &format!("{}/confirm", face(&t, 7).widget())), "no suggestion, no marks");
    let id = format!("{}/confirm", face(&t, 6).widget());
    click(&mut t, &id);
    assert_eq!((person_of(&t, 6), badge(&t, &bo)), (Some(bo.clone()), 0));

    // The People pane: "Ana?" on the photo selected, and its ✓.
    let folder = t.d.widgets().into_iter().find(|w| w.id.starts_with("tree:folder:")).unwrap().id;
    click(&mut t, &folder);
    run(&mut t, "pane.people", json!({}));
    let sel = json!({ "ids": [t.photos[4]], "mode": "replace" });
    run(&mut t, "select.set", sel);
    let id = format!("{}/confirm", face(&t, 4).widget());
    click(&mut t, &id);
    assert_eq!(person_of(&t, 4), Some(ana.clone()));
}

/// Waits until every face drawn has its picture.
fn wait_for_faces(t: &mut T) {
    for _ in 0..200 {
        t.d.settle(FRAMES).unwrap();
        if t.d.app().session().unwrap().face_tiles().pending() == 0 {
            t.d.settle(FRAMES).unwrap();
            if t.d.app().session().unwrap().face_tiles().pending() == 0 {
                return;
            }
        }
        t.d.app_mut().session_mut().unwrap().wait_face_tiles(Duration::from_secs(5));
    }
    panic!("the face pictures never arrived");
}

#[test]
fn the_strip_and_a_suggested_face_match_their_fingerprints() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ana = t.ana.clone();
    click(&mut t, &format!("tree:person:{ana}"));
    wait_for_faces(&mut t);
    check_rect(&t.d, "suggestions-strip-1400x900", widget(&t.d, &format!("section:person:{ana}")).rect);
    check_rect(&t.d, "suggestions-face-1400x900", widget(&t.d, &format!("thumb:{}/in/{ana}", t.photos[3].get())).rect);
    // Screenshots for the review: the section, the Unnamed grid and the People pane, both sizes.
    for (w, h) in [LARGE, SMALL] {
        t.d.resize(w, h);
        click(&mut t, &format!("tree:person:{ana}"));
        wait_for_faces(&mut t);
        shot(&t.d, &format!("suggestions-section-{w}x{h}"));
        click(&mut t, "tree:person:unnamed");
        wait_for_faces(&mut t);
        shot(&t.d, &format!("suggestions-unnamed-{w}x{h}"));
        let folder = t.d.widgets().into_iter().find(|wi| wi.id.starts_with("tree:folder:")).unwrap().id;
        click(&mut t, &folder);
        if t.d.state().pane != Some(Pane::People) {
            run(&mut t, "pane.people", json!({}));
        }
        let sel = json!({ "ids": [t.photos[3], t.photos[6]], "mode": "replace" });
        run(&mut t, "select.set", sel);
        wait_for_faces(&mut t);
        shot(&t.d, &format!("suggestions-pane-{w}x{h}"));
    }
}
