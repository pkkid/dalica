//! Name suggestions (`roadmap.md` M13.26; `faces.md` §5, `organizing.md` §6.3): a face near Ana's
//! is suggested as Ana and confirming it makes it a reference; Confirm All is one undo step; a
//! rejected pair never comes back, also after its photo is renamed and after a restart; nothing is suggested with Suggest names off,
//! below the threshold or without a lead over the runner-up; Find More lists the near misses most
//! similar first; `faces.evaluate` reports per confidence step; each command's contract.
//!
//! The embeddings are synthetic, planted in the recogniser's cache before the session loads it, so
//! no photo is ever read: a four-number recogniser (whose own output is never used) is installed
//! with `matchCosine` 0.5 and no table, so the Suggestion threshold is `0.5 + (confidence − 85) ×
//! 0.01` and Find More's bar 0.4 at the default confidence.

use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::{PhotoId, Rect01};
use dalica_engine::Session;
use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::known;
use dalica_faces::manifest::{InputSpec, OutputSpec};
use dalica_faces::synthetic::encode;
use dalica_fs::Expect;
use dalica_library::FacePerson;
use dalica_store::faces_cache::{CachedFace, FacesCache, Model, PhotoStamp, Record};
use serde_json::{Value, json};

use crate::common::{jpeg, lisbon, off_owner, open, read_json, wait_files};

const WAIT: Duration = Duration::from_secs(120);
const RECOGNISER: &str = "test-recogniser";

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

/// The `n`th face rectangle of a photo (stored frame).
fn rect(n: usize) -> [f64; 4] {
    let x = 0.05 + 0.3 * n as f64;
    [x, 0.2, x + 0.2, 0.5]
}

/// A library of `photos` photos with Ana and Bo; the session, the photos and the two people.
struct Setup {
    lib: tempfile::TempDir,
    disk: tempfile::TempDir,
    folder: PathBuf,
    s: Session,
    ids: Vec<PhotoId>,
    ana: String,
    bo: String,
}

fn setup(photos: u64) -> Setup {
    let all: Vec<Vec<u8>> = (1..=photos).map(jpeg).collect();
    let (lib, disk, folder, mut s, ids) = lisbon(&all, None);
    let ana = s.execute("people.create", &json!({ "name": "Ana" })).unwrap()["person"].as_str().unwrap().to_string();
    let bo = s.execute("people.create", &json!({ "name": "Bo" })).unwrap()["person"].as_str().unwrap().to_string();
    Setup { lib, disk, folder, s, ids, ana, bo }
}

impl Setup {
    /// Adds a face to photo `i` (its `n`th), of `person` or unnamed; its id.
    fn face(&mut self, i: usize, n: usize, person: Option<&str>) -> String {
        let mut p = json!({ "id": self.ids[i], "rect": rect(n) });
        if let Some(person) = person {
            p["person"] = json!(person);
        }
        self.s.execute("faces.add", &p).unwrap()["face"].as_str().unwrap().to_string()
    }

    /// Plants `embeddings` (photo, face number, vector) in the recogniser's cache, then gives the
    /// session its models and waits for the face scan to load them.
    fn plant(&mut self, embeddings: &[(usize, usize, [f32; 4])]) {
        let (models, sha) = models(self.disk.path());
        let records: Vec<Record> = embeddings
            .iter()
            .map(|(i, n, v)| {
                let [x0, y0, x1, y1] = rect(*n);
                let photo = PhotoStamp::of(self.s.library().photo(self.ids[*i]).unwrap());
                Record::Face { photo, face: CachedFace { rect: Rect01::new(x0, y0, x1, y1).unwrap(), landmarks: None, embedding: unit(*v) } }
            })
            .collect();
        let dir = self.lib.path().to_path_buf();
        off_owner(move || {
            let (mut cache, _) = FacesCache::open(&dir, &Model { id: RECOGNISER, sha256: &sha }, 4).unwrap();
            cache.append(&records).unwrap();
        });
        self.load(&models);
        assert_eq!(self.s.faces_index().faces(), embeddings.len(), "the planted embeddings are in the index");
    }

    fn load(&mut self, models: &Path) {
        self.s.set_face_models_dir(Some(models.to_path_buf()));
        assert!(self.s.wait_faces(WAIT), "the face scan loaded the cache");
        assert_eq!(self.s.face_scan().stats().read, 0, "no photo was read: every face came from the cache");
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn err(&mut self, id: &str, p: Value) -> &'static str {
        self.s.execute(id, &p).unwrap_err().code()
    }

    /// The suggestions as (face, person) pairs, in Unnamed's order.
    fn suggested(&mut self) -> Vec<(String, String)> {
        let v = self.run("faces.suggest", json!({}));
        v["suggestions"].as_array().unwrap().iter().map(|m| (m["face"].as_str().unwrap().into(), m["person"].as_str().unwrap().into())).collect()
    }

    fn person_of(&self, face: &str) -> FacePerson {
        let (photo, index) = face.split_once('.').unwrap();
        let r = dalica_library::FaceRef { photo: photo.parse().unwrap(), index: index.parse().unwrap() };
        self.s.library().face(r).unwrap().person.clone()
    }

    fn named(&self, face: &str, person: &str) -> bool {
        matches!(self.person_of(face), FacePerson::Person(p) if p.as_str() == person)
    }
}

const ANA: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const BO: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
/// 0.894 from Ana, 0 from Bo.
const NEAR_ANA: [f32; 4] = [1.0, 0.0, 0.5, 0.0];
/// 0.287 from Ana (below 0.5) but 0.685 from [`NEAR_ANA`].
const NEAR_NEAR_ANA: [f32; 4] = [0.3, 0.0, 1.0, 0.0];

#[test]
fn a_face_near_anas_is_suggested_as_ana_and_confirming_it_makes_it_a_reference() {
    let mut t = setup(4);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    t.face(0, 0, Some(&ana));
    t.face(1, 0, Some(&bo));
    let near = t.face(2, 0, None);
    let next = t.face(3, 0, None);
    t.plant(&[(0, 0, ANA), (1, 0, BO), (2, 0, NEAR_ANA), (3, 0, NEAR_NEAR_ANA)]);

    let v = t.run("faces.suggest", json!({}));
    assert_eq!((v["on"].clone(), v["model"].clone(), v["threshold"].clone()), (json!(true), json!(RECOGNISER), json!(0.5)));
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 1, "{v}");
    let m = &v["suggestions"][0];
    assert_eq!((m["face"].as_str(), m["person"].as_str()), (Some(near.as_str()), Some(ana.as_str())));
    assert!((m["score"].as_f64().unwrap() - 0.894).abs() < 0.01, "{m}");
    assert_eq!(m["runnerUp"]["person"], json!(bo));
    assert_eq!(v["pending"], json!({ ana.clone(): 1 }), "the pending count per person");
    assert_eq!(t.s.pending_suggestions().get(&dalica_core::PersonId::parse(&ana).unwrap()), Some(&1));
    assert!(!t.s.execute("faces.suggest", &json!({ "person": bo })).unwrap()["suggestions"].as_array().unwrap().iter().any(|_| true));
    let by_photo = t.run("faces.suggest", json!({ "ids": [t.ids[2]] }));
    assert_eq!(by_photo["total"], json!(1));
    // No name or rectangle in the reply: it needs no people permission.
    let text = v.to_string();
    assert!(!text.contains("Ana") && !text.contains("rect"), "{text}");

    // Confirming names the face; it is a reference from then on: the face near it is suggested.
    assert_eq!(t.run("faces.confirm", json!({ "faces": [near] })), json!({ "changed": 1 }));
    assert!(t.named(&near, &ana));
    assert_eq!(t.suggested(), [(next.clone(), ana.clone())], "a face like the confirmed one is suggested as Ana now");
    let score = t.run("faces.suggest", json!({}))["suggestions"][0]["score"].as_f64().unwrap();
    assert!((score - 0.685).abs() < 0.01, "matched against the confirmed face: {score}");

    // Undo restores the face and the suggestions exactly; redo again.
    t.run("edit.undo", json!({}));
    assert_eq!(t.person_of(&near), FacePerson::Unnamed);
    assert_eq!(t.suggested(), [(near.clone(), ana.clone())]);
    t.run("edit.redo", json!({}));
    assert!(t.named(&near, &ana));

    // A face without a suggestion cannot be confirmed.
    assert_eq!(t.err("faces.confirm", json!({ "faces": [near] })), "bad_params", "a named face has no suggestion");
    assert_eq!(t.err("faces.confirm", json!({ "faces": [] })), "disabled");
    assert_eq!(t.err("faces.confirm", json!({ "faces": ["99.0"] })), "not_found");
    assert_eq!(t.err("faces.confirm", json!({ "faces": ["x"] })), "bad_params");
    assert_eq!(t.err("faces.suggest", json!({ "ids": [t.ids[0]], "person": ana })), "bad_params");
    assert_eq!(t.err("faces.suggest", json!({ "person": "p0000ffff" })), "not_found");
    assert_eq!(t.err("faces.suggest", json!({ "limit": 10_001 })), "bad_params");
}

#[test]
fn confirm_all_is_one_undo_step() {
    let mut t = setup(6);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    t.face(0, 0, Some(&ana));
    t.face(0, 1, Some(&bo));
    let near: Vec<String> = (1..4).map(|i| t.face(i, 0, None)).collect();
    let bos = t.face(4, 0, None);
    t.plant(&[(0, 0, ANA), (0, 1, BO), (1, 0, NEAR_ANA), (2, 0, [1.0, 0.1, 0.0, 0.3]), (3, 0, [1.0, 0.0, 0.2, 0.2]), (4, 0, [0.1, 1.0, 0.0, 0.2])]);
    assert_eq!(t.run("faces.suggest", json!({}))["pending"], json!({ ana.clone(): 3, bo.clone(): 1 }));

    let steps = t.s.undo_stack().len();
    assert_eq!(t.run("faces.confirmAll", json!({ "person": ana })), json!({ "changed": 3 }));
    assert_eq!(t.s.undo_stack().len(), steps + 1, "one undo step");
    assert!(near.iter().all(|f| t.named(f, &ana)));
    assert_eq!(t.suggested(), [(bos.clone(), bo.clone())], "Bo's suggestion stays");
    t.run("edit.undo", json!({}));
    assert!(near.iter().all(|f| t.person_of(f) == FacePerson::Unnamed), "one undo gives every face back");
    assert_eq!(t.suggested().len(), 4);
    t.run("edit.redo", json!({}));
    assert!(near.iter().all(|f| t.named(f, &ana)));
    assert_eq!(t.run("faces.confirmAll", json!({ "person": ana })), json!({ "changed": 0 }), "nothing left to confirm");
    assert_eq!(t.s.undo_stack().len(), steps + 1, "no step for nothing");
    assert_eq!(t.err("faces.confirmAll", json!({ "person": "p0000ffff" })), "not_found");
    assert_eq!(t.err("faces.confirmAll", json!({ "person": "ana" })), "bad_params");
}

#[test]
fn a_rejected_pair_never_comes_back_also_after_a_restart() {
    let mut t = setup(4);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    t.face(0, 0, Some(&ana));
    t.face(1, 0, Some(&bo));
    let near = t.face(2, 0, None);
    let other = t.face(3, 0, None);
    t.plant(&[(0, 0, ANA), (1, 0, BO), (2, 0, NEAR_ANA), (3, 0, [1.0, 0.0, 0.0, 0.4])]);
    assert_eq!(t.suggested(), [(near.clone(), ana.clone()), (other.clone(), ana.clone())]);

    let steps = t.s.undo_stack().len();
    assert_eq!(t.run("faces.reject", json!({ "faces": [near] })), json!({ "rejected": 1 }));
    assert_eq!(t.s.undo_stack().len(), steps, "rejecting is no undo step");
    assert_eq!(t.suggested(), [(other.clone(), ana.clone())]);
    assert_eq!(t.err("faces.confirm", json!({ "faces": [near] })), "bad_params", "nothing left to confirm");
    assert_eq!(t.err("faces.reject", json!({ "faces": [near] })), "bad_params", "no suggestion left to reject");
    assert_eq!(t.run("faces.reject", json!({ "faces": [near], "person": ana })), json!({ "rejected": 0 }), "already rejected");
    let more = t.run("faces.findMore", json!({ "person": ana }));
    assert!(!more["faces"].as_array().unwrap().iter().any(|f| f["face"] == json!(near)), "Find More leaves the pair out: {more}");
    // Another face named Ana, or a confirmed one, does not bring it back.
    t.run("faces.confirm", json!({ "faces": [other] }));
    assert!(t.suggested().is_empty());
    assert_eq!(t.err("faces.reject", json!({ "faces": [other], "person": ana })), "bad_params", "a named face is not rejected");

    // The pair follows its photo renamed in the session; people.json keeps it (the photo by path)
    // beside the people.
    t.run("photo.rename", json!({ "ids": [t.ids[2]], "name": "Renamed" }));
    wait_files(&mut t.s);
    assert!(t.suggested().is_empty());
    t.s.flush().unwrap();
    let file = read_json(&t.lib.path().join("people.json")).unwrap();
    let photo = t.folder.join("Renamed.jpg");
    assert_eq!(file["rejected"].as_array().map(Vec::len), Some(1), "{file}");
    assert_eq!((file["rejected"][0]["photo"].as_str(), file["rejected"][0]["person"].as_str()), (photo.to_str(), Some(ana.as_str())));
    assert!(file["people"][&ana].is_object() && file["people"][&bo].is_object(), "the people stay: {file}");

    // After a restart the pair is still rejected.
    let Setup { lib, disk, folder, s, ids, ana, .. } = t;
    drop(s);
    let s = open(lib.path());
    let mut t = Setup { lib, disk, folder, s, ids, ana: ana.clone(), bo };
    let models = t.disk.path().join("models");
    t.load(&models);
    assert!(t.suggested().is_empty(), "the rejected pair does not come back after a restart");
    assert_eq!(t.s.library().photo(t.ids[2]).map(|p| p.name.as_str()), Some("Renamed.jpg"));
    assert!(t.s.suggestions().ready());
    assert_eq!(t.s.suggestions().rejected(), 1);
    // Removing Ana's confirmed face leaves the rejection in place too.
    t.run("faces.remove", json!({ "faces": [other] }));
    assert_eq!(t.suggested(), [(other.clone(), ana.clone())], "the face sent back to Unnamed is suggested again");
}

#[test]
fn nothing_is_suggested_with_suggest_names_off_below_the_threshold_or_without_a_lead() {
    let mut t = setup(5);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    t.face(0, 0, Some(&ana));
    t.face(1, 0, Some(&bo));
    let near = t.face(2, 0, None);
    let weak = t.face(3, 0, None);
    let between = t.face(4, 0, None);
    // 0.45 from Ana: below 0.5; Ana and Bo alike: no lead over the runner-up.
    t.plant(&[(0, 0, ANA), (1, 0, BO), (2, 0, NEAR_ANA), (3, 0, [0.45, 0.0, 0.893, 0.0]), (4, 0, [1.0, 1.0, 0.0, 0.0])]);
    assert_eq!(t.suggested(), [(near.clone(), ana.clone())]);

    t.run("options.set", json!({ "values": { "people.suggestNames": false } }));
    let v = t.run("faces.suggest", json!({}));
    assert_eq!((v["on"].clone(), v["total"].clone(), v["pending"].clone()), (json!(false), json!(0), json!({})));
    assert_eq!(t.err("faces.confirm", json!({ "faces": [near] })), "disabled");
    assert_eq!(t.err("faces.confirmAll", json!({ "person": ana })), "disabled");
    assert!(t.s.pending_suggestions().is_empty());

    // On again, at confidence 50 (0.15) the weak match is suggested too; the tie never is.
    t.run("options.set", json!({ "values": { "people.suggestNames": true, "people.suggestionConfidence": 50 } }));
    let v = t.run("faces.suggest", json!({}));
    assert_eq!(v["threshold"], json!(0.15));
    assert_eq!(t.suggested(), [(near.clone(), ana.clone()), (weak.clone(), ana.clone())]);
    assert!(!t.suggested().iter().any(|(f, _)| *f == between));
    // At 95 (0.6) only the strong one.
    t.run("options.set", json!({ "values": { "people.suggestionConfidence": 95 } }));
    assert_eq!(t.suggested(), [(near.clone(), ana.clone())]);
}

#[test]
fn find_more_lists_the_near_misses_most_similar_first() {
    let mut t = setup(6);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    t.face(0, 0, Some(&ana));
    t.face(0, 1, Some(&bo));
    let near = t.face(1, 0, None);
    let a = t.face(2, 0, None);
    let b = t.face(3, 0, None);
    t.face(4, 0, None);
    t.face(5, 0, None);
    // 0.894, 0.45, 0.42 from Ana: listed (above 0.4); 0.3 and Bo's face are not.
    t.plant(&[
        (0, 0, ANA),
        (0, 1, BO),
        (1, 0, NEAR_ANA),
        (3, 0, [0.42, 0.0, 0.907, 0.0]),
        (2, 0, [0.45, 0.0, 0.893, 0.0]),
        (4, 0, [0.3, 0.0, 0.954, 0.0]),
        (5, 0, [0.1, 1.0, 0.0, 0.0]),
    ]);
    let v = t.run("faces.findMore", json!({ "person": ana }));
    let faces: Vec<&str> = v["faces"].as_array().unwrap().iter().map(|f| f["face"].as_str().unwrap()).collect();
    assert_eq!(faces, [near.as_str(), a.as_str(), b.as_str()], "{v}");
    let scores: Vec<f64> = v["faces"].as_array().unwrap().iter().map(|f| f["score"].as_f64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]) && (scores[1] - 0.45).abs() < 0.01, "{scores:?}");
    assert_eq!((v["threshold"].clone(), v["total"].clone()), (json!(0.5), json!(3)));
    let page = t.run("faces.findMore", json!({ "person": ana, "offset": 1, "limit": 1 }));
    assert_eq!(page["faces"][0]["face"], json!(a));

    // Rejecting a near miss from Find More (the person given) takes it off the list.
    assert_eq!(t.run("faces.reject", json!({ "faces": [a], "person": ana })), json!({ "rejected": 1 }));
    let v = t.run("faces.findMore", json!({ "person": ana }));
    assert_eq!(v["total"], json!(2));
    // Find More works with Suggest names off: the user asked.
    t.run("options.set", json!({ "values": { "people.suggestNames": false } }));
    assert_eq!(t.run("faces.findMore", json!({ "person": ana }))["total"], json!(2));
    assert_eq!(t.err("faces.findMore", json!({ "person": "p0000ffff" })), "not_found");
    assert_eq!(t.err("faces.findMore", json!({})), "bad_params");
}

#[test]
fn evaluate_reports_per_confidence_step() {
    let mut t = setup(6);
    let (ana, bo) = (t.ana.clone(), t.bo.clone());
    // Before a model: nothing to evaluate.
    let v = t.run("faces.evaluate", json!({}));
    assert_eq!((v["model"].clone(), v["faces"].clone(), v["steps"].clone()), (Value::Null, json!(0), json!([])));
    for (i, who) in [&ana, &ana, &ana, &bo, &bo, &bo].into_iter().enumerate() {
        t.face(i, 0, Some(who));
    }
    // Ana: two alike and one weaker (0.57 from the first); Bo: two alike and one that looks like
    // Ana (named wrongly).
    t.plant(&[
        (0, 0, ANA),
        (1, 0, [1.0, 0.0, 0.2, 0.0]),
        (2, 0, [0.57, 0.0, 0.0, 0.8216]),
        (3, 0, BO),
        (4, 0, [0.0, 1.0, 0.0, 0.2]),
        (5, 0, [1.0, 0.0, 1.0, 0.0]),
    ]);
    let v = t.run("faces.evaluate", json!({}));
    assert_eq!((v["model"].clone(), v["faces"].clone(), v["confidence"].clone()), (json!(RECOGNISER), json!(6), json!(85)));
    let steps = v["steps"].as_array().unwrap();
    let got: Vec<(u64, u64, u64)> =
        steps.iter().map(|s| (s["confidence"].as_u64().unwrap(), s["suggested"].as_u64().unwrap(), s["right"].as_u64().unwrap())).collect();
    let want: Vec<(u64, u64, u64)> = (0..10).map(|k| (50 + 5 * k, if k < 9 { 6 } else { 5 }, if k < 9 { 5 } else { 4 })).collect();
    assert_eq!(got, want, "the weak face is suggested up to 90; the wrongly named one always, and wrongly");
    let thresholds: Vec<f64> = steps.iter().map(|s| s["threshold"].as_f64().unwrap()).collect();
    assert!(thresholds.windows(2).all(|w| w[0] < w[1]) && (thresholds[7] - 0.5).abs() < 1e-6, "{thresholds:?}");
    assert_eq!(t.err("faces.evaluate", json!({ "x": 1 })), "bad_params");
}
